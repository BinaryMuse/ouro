use std::path::Path;
use std::process::Stdio;

use tokio::io::AsyncReadExt;
use tokio::process::Command;

/// Result of a shell command execution.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
}

/// Execute a shell command asynchronously with timeout and process-group management.
///
/// Spawns `sh -c <command>` in its own process group so that the entire group
/// can be killed on timeout (not just the parent shell). Stdout and stderr are
/// read concurrently in separate tasks so that partial output is captured even
/// if the process is killed mid-execution.
///
/// # Timeout behavior
///
/// When the timeout expires the entire process group is sent SIGKILL via
/// [`nix::sys::signal::killpg`], the child is reaped to prevent zombies, and
/// whatever output was buffered before the kill is returned.
///
/// # Important
///
/// - Does **not** use `kill_on_drop` (causes zombie processes).
/// - Uses `nix` crate for `killpg` (safe wrapper, no `unsafe` blocks).
pub async fn execute_shell(
    command: &str,
    working_dir: &Path,
    timeout_secs: u64,
) -> anyhow::Result<ExecResult> {
    let mut child = {
        // process_group(0) requires the CommandExt trait in scope.
        #[allow(unused_imports)]
        use std::os::unix::process::CommandExt;

        Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(working_dir)
            .process_group(0) // new process group for clean kill
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn shell process: {}", e))?
    };

    let child_pid = child
        .id()
        .ok_or_else(|| anyhow::anyhow!("Child process has no PID"))?;

    // Take ownership of stdout/stderr handles so we can read them in parallel tasks.
    let stdout_handle = child.stdout.take().expect("stdout piped");
    let stderr_handle = child.stderr.take().expect("stderr piped");

    // Spawn concurrent readers that buffer output as it arrives.
    let stdout_task = tokio::spawn(async move {
        let mut buf = String::new();
        let mut handle = stdout_handle;
        let _ = handle.read_to_string(&mut buf).await;
        buf
    });

    let stderr_task = tokio::spawn(async move {
        let mut buf = String::new();
        let mut handle = stderr_handle;
        let _ = handle.read_to_string(&mut buf).await;
        buf
    });

    let timeout_duration = std::time::Duration::from_secs(timeout_secs);

    // Wait for the child process only (not the reader tasks) under the timeout.
    // The reader tasks run independently and will complete once the pipes close
    // (either normally when the process exits, or when we kill it).
    match tokio::time::timeout(timeout_duration, child.wait()).await {
        // Process completed within timeout.
        Ok(Ok(status)) => {
            let stdout = stdout_task.await.unwrap_or_default();
            let stderr = stderr_task.await.unwrap_or_default();
            Ok(ExecResult {
                stdout,
                stderr,
                exit_code: status.code(),
                timed_out: false,
            })
        }
        // Process wait errored (not a timeout).
        Ok(Err(e)) => Err(anyhow::anyhow!("Process wait failed: {}", e)),
        // Timeout expired -- kill process group.
        Err(_elapsed) => {
            // Kill the entire process group via SIGKILL.
            let pgid = nix::unistd::Pid::from_raw(child_pid as i32);
            let _ = nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGKILL);

            // Reap the child to prevent zombie processes.
            let _ = child.wait().await;

            // Collect whatever partial output was buffered before the kill.
            // After killing the process group the pipes close, so the reader
            // tasks should complete shortly. Abort them after a brief grace
            // period to avoid hanging indefinitely.
            let partial_stdout = match tokio::time::timeout(
                std::time::Duration::from_millis(500),
                stdout_task,
            )
            .await
            {
                Ok(Ok(s)) => s,
                _ => String::new(),
            };

            let partial_stderr = match tokio::time::timeout(
                std::time::Duration::from_millis(500),
                stderr_task,
            )
            .await
            {
                Ok(Ok(s)) => s,
                _ => String::new(),
            };

            Ok(ExecResult {
                stdout: partial_stdout,
                stderr: partial_stderr,
                exit_code: None,
                timed_out: true,
            })
        }
    }
}
