//! Background process spawner.
//!
//! Spawns a long-lived shell process as a tokio child with piped stdin, stdout,
//! and stderr. Output is captured into a shared ring buffer (bounded
//! `VecDeque<String>`) that the parent agent can read via the manager's
//! `read_output` method.
//!
//! The spawned process:
//! - Runs in its own process group (`process_group(0)`) for clean shutdown
//! - Has `kill_on_drop(true)` as a safety net
//! - Respects a [`CancellationToken`] for graceful cancellation
//! - Reports exit status back through the [`SubAgentManager`]

use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use uuid::Uuid;

use super::manager::SubAgentManager;
use super::types::{SubAgentId, SubAgentKind, SubAgentResult, SubAgentStatus};
use crate::config::AppConfig;

/// Maximum number of lines retained in the output ring buffer.
const OUTPUT_BUFFER_CAPACITY: usize = 1000;

/// Spawn a background shell process with piped I/O and output capture.
///
/// The process runs `sh -c <command>` in the workspace directory with its own
/// process group. Stdin is retained in the manager for later writes; stdout and
/// stderr are captured into a shared ring buffer.
///
/// # Arguments
///
/// * `manager` - Registry to track the spawned process
/// * `command` - Shell command to execute (passed to `sh -c`)
/// * `parent_id` - Optional parent sub-agent ID (for nesting hierarchy)
/// * `config` - Application configuration (workspace path for cwd)
///
/// # Returns
///
/// The new process's ID on success, or an error string if limits are exceeded
/// or the process fails to spawn.
pub async fn spawn_background_process(
    manager: &SubAgentManager,
    command: String,
    parent_id: Option<SubAgentId>,
    config: &AppConfig,
) -> Result<SubAgentId, String> {
    // 1. Generate a unique ID for this process.
    let id: SubAgentId = Uuid::new_v4().to_string();

    // 2. Create a cancellation token as a child of the parent's (or root).
    let cancel_token = manager.create_child_token(parent_id.as_ref());

    // 3. Register with the manager (validates depth + count limits).
    manager.register(
        id.clone(),
        SubAgentKind::BackgroundProcess {
            command: command.clone(),
        },
        parent_id,
        cancel_token.clone(),
    )?;

    // 4. Spawn the process with piped stdin/stdout/stderr.
    //
    // process_group(0) requires the CommandExt trait on Unix.
    #[allow(unused_imports)]
    use std::os::unix::process::CommandExt;

    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&command)
        .current_dir(&config.workspace)
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("Failed to spawn background process: {e}"))?;

    // 5. Take the stdin handle and store it in the manager.
    if let Some(stdin) = child.stdin.take() {
        manager.set_stdin(&id, stdin);
    }

    // 6. Take stdout/stderr for reader tasks.
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to capture stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Failed to capture stderr".to_string())?;

    // 7. Create the shared output ring buffer.
    let output_buffer: Arc<Mutex<VecDeque<String>>> =
        Arc::new(Mutex::new(VecDeque::with_capacity(OUTPUT_BUFFER_CAPACITY)));
    manager.set_output_buffer(&id, output_buffer.clone());

    // 8. Spawn stdout reader task.
    let stdout_buf = output_buffer.clone();
    tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let mut buf = stdout_buf.lock().unwrap();
            if buf.len() >= OUTPUT_BUFFER_CAPACITY {
                buf.pop_front();
            }
            buf.push_back(line);
        }
    });

    // 9. Spawn stderr reader task (prefixes lines with "[stderr] ").
    let stderr_buf = output_buffer.clone();
    tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let mut buf = stderr_buf.lock().unwrap();
            if buf.len() >= OUTPUT_BUFFER_CAPACITY {
                buf.pop_front();
            }
            buf.push_back(format!("[stderr] {line}"));
        }
    });

    // 10. Spawn the monitor task (main JoinHandle).
    let task_id = id.clone();
    let task_manager = manager.clone();
    let task_command = command.clone();

    let handle = tokio::spawn(async move {
        let start = Instant::now();

        let outcome = tokio::select! {
            wait_result = child.wait() => {
                match wait_result {
                    Ok(status) => {
                        let code = status.code();
                        if code == Some(0) {
                            Ok(SubAgentStatus::Completed)
                        } else {
                            Ok(SubAgentStatus::Failed(format!(
                                "process exited with code {}",
                                code.map_or("unknown".to_string(), |c| c.to_string())
                            )))
                        }
                    }
                    Err(e) => Err(format!("process wait failed: {e}")),
                }
            }
            _ = cancel_token.cancelled() => {
                // Kill the entire process group via SIGKILL.
                if let Some(pid) = child.id() {
                    let pgid = nix::unistd::Pid::from_raw(pid as i32);
                    let _ = nix::sys::signal::killpg(
                        pgid,
                        nix::sys::signal::Signal::SIGKILL,
                    );
                }
                // Reap the child to prevent zombies.
                let _ = child.wait().await;
                Ok(SubAgentStatus::Killed)
            }
        };

        let elapsed = start.elapsed().as_secs_f64();

        // Read last lines of output for the result summary.
        let last_output = {
            let buf = output_buffer.lock().unwrap();
            let n = buf.len().min(20);
            let start = buf.len().saturating_sub(n);
            buf.iter().skip(start).cloned().collect::<Vec<_>>().join("\n")
        };

        match outcome {
            Ok(status) => {
                let status_str = match &status {
                    SubAgentStatus::Completed => "completed",
                    SubAgentStatus::Killed => "killed",
                    SubAgentStatus::Failed(_) => "failed",
                    SubAgentStatus::Running => "running",
                };

                let result = SubAgentResult {
                    agent_id: task_id.clone(),
                    status: status_str.to_string(),
                    summary: format!("Background process `{}` {status_str}", truncate(&task_command, 50)),
                    output: last_output,
                    files_modified: vec![],
                    elapsed_secs: elapsed,
                };
                task_manager.set_result(&task_id, result);
                task_manager.update_status(&task_id, status);
            }
            Err(e) => {
                let result = SubAgentResult {
                    agent_id: task_id.clone(),
                    status: "failed".to_string(),
                    summary: format!("Background process error: {e}"),
                    output: last_output,
                    files_modified: vec![],
                    elapsed_secs: elapsed,
                };
                task_manager.set_result(&task_id, result);
                task_manager.update_status(&task_id, SubAgentStatus::Failed(e));
            }
        }
    });

    // 11. Store the JoinHandle.
    manager.set_join_handle(&id, handle);

    Ok(id)
}

/// Truncate a string to `max_len` characters, appending "..." if truncated.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}...", &s[..max_len])
    } else {
        s.to_string()
    }
}
