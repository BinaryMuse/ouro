pub mod command_filter;
pub mod defaults;
pub mod workspace;

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use command_filter::{BlockedCommand, CommandFilter};
use workspace::WorkspaceGuard;

use crate::config::AppConfig;
use crate::exec::{execute_shell, ExecResult};

/// Combined safety layer: checks commands against the blocklist, enforces
/// workspace boundaries, and delegates allowed commands to the shell executor
/// with timeout enforcement.
///
/// This is the single entry point for all command execution. No code should
/// call [`execute_shell`] directly -- always go through `SafetyLayer::execute`.
pub struct SafetyLayer {
    command_filter: CommandFilter,
    workspace_guard: WorkspaceGuard,
    timeout_secs: u64,
    security_log_path: PathBuf,
}

impl SafetyLayer {
    /// Build a SafetyLayer from the resolved application configuration.
    ///
    /// Constructs the [`CommandFilter`] from `config.blocked_patterns` and the
    /// [`WorkspaceGuard`] from `config.workspace`. Stores timeout and security
    /// log path for runtime use.
    pub fn new(config: &AppConfig) -> anyhow::Result<Self> {
        let command_filter = CommandFilter::new(&config.blocked_patterns)
            .map_err(|e| anyhow::anyhow!("Failed to compile command filter patterns: {}", e))?;

        let workspace_guard = WorkspaceGuard::new(&config.workspace)
            .map_err(|e| anyhow::anyhow!("Failed to initialize workspace guard: {}", e))?;

        Ok(Self {
            command_filter,
            workspace_guard,
            timeout_secs: config.shell_timeout_secs,
            security_log_path: config.security_log_path.clone(),
        })
    }

    /// Execute a shell command through the safety pipeline.
    ///
    /// 1. Check command against the blocklist.
    /// 2. If blocked: log to security file, return an [`ExecResult`] with the
    ///    blocked JSON in `stderr` and `exit_code` 126 ("cannot execute").
    /// 3. If allowed: delegate to [`execute_shell`] with workspace root and timeout.
    pub async fn execute(&self, command: &str) -> anyhow::Result<ExecResult> {
        // Step 1: Check against blocklist.
        if let Some(blocked) = self.command_filter.check(command) {
            // Log the blocked command to the security log.
            self.log_blocked_command(&blocked);

            // Return structured result (not an error) so the agent gets JSON.
            return Ok(ExecResult {
                stdout: String::new(),
                stderr: blocked.to_json(),
                exit_code: Some(126), // standard "cannot execute" code
                timed_out: false,
            });
        }

        // Step 2: Execute allowed command in workspace with timeout.
        execute_shell(command, self.workspace_guard.canonical_root(), self.timeout_secs).await
    }

    /// Get the canonical workspace root path.
    pub fn workspace_root(&self) -> &Path {
        self.workspace_guard.canonical_root()
    }

    /// Append a JSON line to the security log for a blocked command.
    ///
    /// Each entry is a single JSON line with timestamp, blocked flag, reason, and command.
    /// Uses [`std::time::SystemTime`] for timestamps (no chrono dependency).
    /// If the log file cannot be written, a warning is logged via tracing but the
    /// command check is not affected.
    fn log_blocked_command(&self, blocked: &BlockedCommand) {
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let log_entry = format!(
            "{{\"timestamp\":{},\"blocked\":true,\"reason\":{},\"command\":{}}}\n",
            timestamp,
            serde_json::to_string(&blocked.reason).unwrap_or_else(|_| "\"unknown\"".into()),
            serde_json::to_string(&blocked.command).unwrap_or_else(|_| "\"unknown\"".into()),
        );

        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.security_log_path)
        {
            Ok(mut file) => {
                if let Err(e) = file.write_all(log_entry.as_bytes()) {
                    tracing::warn!(
                        "Failed to write to security log at {}: {}",
                        self.security_log_path.display(),
                        e
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to open security log at {}: {}",
                    self.security_log_path.display(),
                    e
                );
            }
        }
    }
}
