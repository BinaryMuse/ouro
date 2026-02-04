use ouro::exec::{execute_shell, ExecResult};
use std::time::Instant;
use tempfile::TempDir;

fn setup_workspace() -> TempDir {
    tempfile::tempdir().expect("failed to create temp dir")
}

// ============================================================
// Normal execution
// ============================================================

#[tokio::test]
async fn test_normal_execution_stdout() {
    let ws = setup_workspace();
    let result = execute_shell("echo hello", ws.path(), 5).await.unwrap();
    assert_eq!(result.stdout, "hello\n");
    assert_eq!(result.exit_code, Some(0));
    assert!(!result.timed_out);
}

#[tokio::test]
async fn test_stderr_capture() {
    let ws = setup_workspace();
    let result = execute_shell("echo err >&2", ws.path(), 5).await.unwrap();
    assert_eq!(result.stderr, "err\n");
    assert_eq!(result.stdout, "");
    assert_eq!(result.exit_code, Some(0));
    assert!(!result.timed_out);
}

#[tokio::test]
async fn test_exit_code() {
    let ws = setup_workspace();
    let result = execute_shell("exit 42", ws.path(), 5).await.unwrap();
    assert_eq!(result.exit_code, Some(42));
    assert!(!result.timed_out);
}

#[tokio::test]
async fn test_working_directory() {
    let ws = setup_workspace();
    let canonical = std::fs::canonicalize(ws.path()).unwrap();
    let result = execute_shell("pwd", ws.path(), 5).await.unwrap();
    assert_eq!(result.stdout.trim(), canonical.to_str().unwrap());
    assert_eq!(result.exit_code, Some(0));
}

// ============================================================
// Timeout behavior
// ============================================================

#[tokio::test]
async fn test_timeout_kills_process() {
    let ws = setup_workspace();
    let start = Instant::now();
    let result = execute_shell("sleep 60", ws.path(), 1).await.unwrap();
    let elapsed = start.elapsed();

    assert!(result.timed_out, "should report timed_out");
    assert_eq!(result.exit_code, None, "timed-out process should have no exit code");
    assert!(
        elapsed.as_secs() < 5,
        "timeout should fire within ~2 seconds, took {:?}",
        elapsed
    );
}

#[tokio::test]
async fn test_timeout_no_zombies() {
    let ws = setup_workspace();
    // Run a process that spawns children, then timeout.
    let _result = execute_shell("sleep 60 & sleep 60 & wait", ws.path(), 1)
        .await
        .unwrap();

    // Give the OS a moment to clean up.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Check for zombie processes from our test. This is a best-effort check:
    // we look for defunct processes owned by our PID.
    let check = execute_shell(
        &format!("ps -o pid,stat,comm -p $(pgrep -P {} 2>/dev/null || echo 0) 2>/dev/null | grep -c Z || true", std::process::id()),
        ws.path(),
        5,
    )
    .await
    .unwrap();

    let zombie_count: i32 = check.stdout.trim().parse().unwrap_or(0);
    assert_eq!(zombie_count, 0, "no zombie processes should remain");
}

// ============================================================
// Mixed stdout and stderr
// ============================================================

#[tokio::test]
async fn test_mixed_stdout_stderr() {
    let ws = setup_workspace();
    let result = execute_shell("echo out && echo err >&2", ws.path(), 5)
        .await
        .unwrap();
    assert_eq!(result.stdout, "out\n");
    assert_eq!(result.stderr, "err\n");
    assert_eq!(result.exit_code, Some(0));
}

// ============================================================
// Serialization
// ============================================================

#[tokio::test]
async fn test_exec_result_serializes() {
    let result = ExecResult {
        stdout: "output".into(),
        stderr: "".into(),
        exit_code: Some(0),
        timed_out: false,
    };
    let json = serde_json::to_string(&result).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["stdout"], "output");
    assert_eq!(parsed["exit_code"], 0);
    assert_eq!(parsed["timed_out"], false);
}
