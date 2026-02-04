use ouro::config::AppConfig;
use ouro::safety::SafetyLayer;
use std::path::PathBuf;
use tempfile::TempDir;

// ─── Helper ───────────────────────────────────────────────────────────

fn setup_workspace() -> TempDir {
    tempfile::tempdir().expect("failed to create temp dir")
}

fn test_config(workspace: &std::path::Path, security_log: PathBuf, timeout: u64) -> AppConfig {
    AppConfig {
        model: "test-model".to_string(),
        workspace: workspace.to_path_buf(),
        shell_timeout_secs: timeout,
        context_limit: 8000,
        blocked_patterns: ouro::safety::defaults::default_blocklist(),
        security_log_path: security_log,
    }
}

// ============================================================
// SafetyLayer blocks dangerous commands
// ============================================================

#[tokio::test]
async fn test_safety_layer_blocks_sudo() {
    let ws = setup_workspace();
    let security_log = ws.path().join("security.log");
    let config = test_config(ws.path(), security_log, 5);
    let layer = SafetyLayer::new(&config).unwrap();

    let result = layer.execute("sudo ls").await.unwrap();

    // Should be blocked, not executed.
    assert_eq!(result.exit_code, Some(126));
    assert!(result.stdout.is_empty());
    assert!(!result.stderr.is_empty());

    // Stderr should contain structured blocked JSON.
    let parsed: serde_json::Value = serde_json::from_str(&result.stderr)
        .expect("stderr should be valid JSON for blocked commands");
    assert_eq!(parsed["blocked"], true);
    assert!(parsed["reason"].as_str().unwrap().contains("sudo") || parsed["reason"].as_str().unwrap().contains("Privilege"));
    assert_eq!(parsed["command"], "sudo ls");
}

#[tokio::test]
async fn test_safety_layer_blocks_rm_rf_root() {
    let ws = setup_workspace();
    let security_log = ws.path().join("security.log");
    let config = test_config(ws.path(), security_log, 5);
    let layer = SafetyLayer::new(&config).unwrap();

    let result = layer.execute("rm -rf /").await.unwrap();

    assert_eq!(result.exit_code, Some(126));
    let parsed: serde_json::Value = serde_json::from_str(&result.stderr).unwrap();
    assert_eq!(parsed["blocked"], true);
}

// ============================================================
// SafetyLayer executes allowed commands
// ============================================================

#[tokio::test]
async fn test_safety_layer_executes_echo() {
    let ws = setup_workspace();
    let security_log = ws.path().join("security.log");
    let config = test_config(ws.path(), security_log, 5);
    let layer = SafetyLayer::new(&config).unwrap();

    let result = layer.execute("echo hello").await.unwrap();

    assert_eq!(result.stdout, "hello\n");
    assert_eq!(result.exit_code, Some(0));
    assert!(!result.timed_out);
}

#[tokio::test]
async fn test_safety_layer_executes_in_workspace_dir() {
    let ws = setup_workspace();
    let canonical = std::fs::canonicalize(ws.path()).unwrap();
    let security_log = ws.path().join("security.log");
    let config = test_config(ws.path(), security_log, 5);
    let layer = SafetyLayer::new(&config).unwrap();

    let result = layer.execute("pwd").await.unwrap();
    assert_eq!(result.stdout.trim(), canonical.to_str().unwrap());
}

// ============================================================
// SafetyLayer handles timeout
// ============================================================

#[tokio::test]
async fn test_safety_layer_timeout() {
    let ws = setup_workspace();
    let security_log = ws.path().join("security.log");
    let config = test_config(ws.path(), security_log, 1); // 1 second timeout
    let layer = SafetyLayer::new(&config).unwrap();

    let start = std::time::Instant::now();
    let result = layer.execute("sleep 60").await.unwrap();
    let elapsed = start.elapsed();

    assert!(result.timed_out, "should report timed_out");
    assert_eq!(result.exit_code, None);
    assert!(
        elapsed.as_secs() < 5,
        "timeout should fire quickly, took {:?}",
        elapsed
    );
}

// ============================================================
// Security log file
// ============================================================

#[tokio::test]
async fn test_security_log_created_on_blocked_command() {
    let ws = setup_workspace();
    let security_log = ws.path().join("security.log");
    let config = test_config(ws.path(), security_log.clone(), 5);
    let layer = SafetyLayer::new(&config).unwrap();

    // Execute a blocked command.
    let _ = layer.execute("sudo ls").await.unwrap();

    // Security log should exist.
    assert!(security_log.exists(), "security log should be created");

    // Read and verify the log content.
    let contents = std::fs::read_to_string(&security_log).unwrap();
    let lines: Vec<&str> = contents.lines().collect();
    assert_eq!(lines.len(), 1, "should have exactly one log entry");

    // Each line should be valid JSON.
    let entry: serde_json::Value = serde_json::from_str(lines[0])
        .expect("security log entry should be valid JSON");
    assert_eq!(entry["blocked"], true);
    assert!(entry["timestamp"].is_number());
    assert_eq!(entry["command"], "sudo ls");
}

#[tokio::test]
async fn test_security_log_appends_multiple_entries() {
    let ws = setup_workspace();
    let security_log = ws.path().join("security.log");
    let config = test_config(ws.path(), security_log.clone(), 5);
    let layer = SafetyLayer::new(&config).unwrap();

    // Execute multiple blocked commands.
    let _ = layer.execute("sudo ls").await.unwrap();
    let _ = layer.execute("sudo rm foo").await.unwrap();
    let _ = layer.execute("reboot").await.unwrap();

    let contents = std::fs::read_to_string(&security_log).unwrap();
    let lines: Vec<&str> = contents.lines().collect();
    assert_eq!(lines.len(), 3, "should have three log entries");

    // All lines should be valid JSON.
    for line in &lines {
        let _: serde_json::Value = serde_json::from_str(line)
            .expect("each security log entry should be valid JSON");
    }
}

#[tokio::test]
async fn test_no_security_log_for_allowed_commands() {
    let ws = setup_workspace();
    let security_log = ws.path().join("security.log");
    let config = test_config(ws.path(), security_log.clone(), 5);
    let layer = SafetyLayer::new(&config).unwrap();

    // Execute an allowed command.
    let _ = layer.execute("echo hello").await.unwrap();

    // Security log should NOT exist (no blocked commands).
    assert!(
        !security_log.exists(),
        "security log should not be created for allowed commands"
    );
}

// ============================================================
// workspace_root accessor
// ============================================================

#[tokio::test]
async fn test_workspace_root_matches_config() {
    let ws = setup_workspace();
    let canonical = std::fs::canonicalize(ws.path()).unwrap();
    let security_log = ws.path().join("security.log");
    let config = test_config(ws.path(), security_log, 5);
    let layer = SafetyLayer::new(&config).unwrap();

    assert_eq!(layer.workspace_root(), canonical.as_path());
}
