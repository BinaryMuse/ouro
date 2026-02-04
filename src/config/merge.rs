use super::schema::{AppConfig, PartialConfig};
use crate::safety::defaults::default_blocklist;
use std::path::PathBuf;

impl PartialConfig {
    /// Merge self with a lower-priority fallback.
    /// Self's non-None values take precedence.
    /// For blocked_patterns: REPLACE semantics (if self has Some, use it entirely).
    pub fn with_fallback(self, fallback: PartialConfig) -> PartialConfig {
        PartialConfig {
            model: self.model.or(fallback.model),
            workspace: self.workspace.or(fallback.workspace),
            shell_timeout_secs: self.shell_timeout_secs.or(fallback.shell_timeout_secs),
            context_limit: self.context_limit.or(fallback.context_limit),
            blocked_patterns: self.blocked_patterns.or(fallback.blocked_patterns),
            security_log_path: self.security_log_path.or(fallback.security_log_path),
        }
    }

    /// Convert to AppConfig, filling any remaining gaps with defaults.
    pub fn finalize(self) -> AppConfig {
        let workspace = self
            .workspace
            .unwrap_or_else(|| PathBuf::from("./workspace"));
        let security_log_path = self
            .security_log_path
            .unwrap_or_else(|| workspace.join("security.log"));

        AppConfig {
            model: self.model.unwrap_or_else(|| "llama3.2".to_string()),
            workspace,
            shell_timeout_secs: self.shell_timeout_secs.unwrap_or(30),
            context_limit: self.context_limit.unwrap_or(32768),
            blocked_patterns: self.blocked_patterns.unwrap_or_else(default_blocklist),
            security_log_path,
        }
    }
}
