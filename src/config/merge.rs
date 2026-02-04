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
            soft_threshold_pct: self.soft_threshold_pct.or(fallback.soft_threshold_pct),
            hard_threshold_pct: self.hard_threshold_pct.or(fallback.hard_threshold_pct),
            carryover_turns: self.carryover_turns.or(fallback.carryover_turns),
            max_restarts: self.max_restarts.or(fallback.max_restarts),
            auto_restart: self.auto_restart.or(fallback.auto_restart),
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
            soft_threshold_pct: self.soft_threshold_pct.unwrap_or(0.70),
            hard_threshold_pct: self.hard_threshold_pct.unwrap_or(0.90),
            carryover_turns: self.carryover_turns.unwrap_or(5),
            max_restarts: self.max_restarts.unwrap_or(None),
            auto_restart: self.auto_restart.unwrap_or(true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_overrides_workspace() {
        let cli = PartialConfig {
            model: Some("gpt4".to_string()),
            shell_timeout_secs: Some(60),
            ..Default::default()
        };
        let workspace = PartialConfig {
            model: Some("llama3".to_string()),
            shell_timeout_secs: Some(10),
            context_limit: Some(16384),
            ..Default::default()
        };

        let merged = cli.with_fallback(workspace);
        assert_eq!(merged.model.as_deref(), Some("gpt4"), "CLI model should override workspace");
        assert_eq!(merged.shell_timeout_secs, Some(60), "CLI timeout should override workspace");
        assert_eq!(merged.context_limit, Some(16384), "Workspace context_limit should be preserved");
    }

    #[test]
    fn test_workspace_overrides_global() {
        let workspace = PartialConfig {
            model: Some("mistral".to_string()),
            workspace: Some(PathBuf::from("/tmp/ws")),
            ..Default::default()
        };
        let global = PartialConfig {
            model: Some("llama3.2".to_string()),
            workspace: Some(PathBuf::from("/home/user/workspace")),
            shell_timeout_secs: Some(45),
            ..Default::default()
        };

        let merged = workspace.with_fallback(global);
        assert_eq!(merged.model.as_deref(), Some("mistral"), "Workspace model should override global");
        assert_eq!(merged.workspace, Some(PathBuf::from("/tmp/ws")), "Workspace path should override global");
        assert_eq!(merged.shell_timeout_secs, Some(45), "Global timeout should be preserved");
    }

    #[test]
    fn test_defaults_apply_when_no_config() {
        let empty = PartialConfig::default();
        let config = empty.finalize();

        assert_eq!(config.model, "llama3.2");
        assert_eq!(config.workspace, PathBuf::from("./workspace"));
        assert_eq!(config.shell_timeout_secs, 30);
        assert_eq!(config.context_limit, 32768);
        assert!(!config.blocked_patterns.is_empty(), "Default blocklist should be non-empty");
        assert_eq!(config.security_log_path, PathBuf::from("./workspace/security.log"));
    }

    #[test]
    fn test_blocked_patterns_replace_semantics() {
        let workspace = PartialConfig {
            blocked_patterns: Some(vec![
                ("custom_pattern".to_string(), "custom reason".to_string()),
            ]),
            ..Default::default()
        };
        let global = PartialConfig {
            blocked_patterns: Some(vec![
                ("global_pattern_1".to_string(), "global reason 1".to_string()),
                ("global_pattern_2".to_string(), "global reason 2".to_string()),
            ]),
            ..Default::default()
        };

        let merged = workspace.with_fallback(global);
        let patterns = merged.blocked_patterns.unwrap();
        assert_eq!(patterns.len(), 1, "Workspace blocklist should replace global entirely");
        assert_eq!(patterns[0].0, "custom_pattern");
    }

    #[test]
    fn test_three_layer_merge() {
        let cli = PartialConfig {
            model: Some("cli-model".to_string()),
            ..Default::default()
        };
        let workspace = PartialConfig {
            shell_timeout_secs: Some(20),
            context_limit: Some(8192),
            ..Default::default()
        };
        let global = PartialConfig {
            model: Some("global-model".to_string()),
            workspace: Some(PathBuf::from("/global/ws")),
            shell_timeout_secs: Some(60),
            ..Default::default()
        };

        let config = cli
            .with_fallback(workspace)
            .with_fallback(global)
            .finalize();

        assert_eq!(config.model, "cli-model", "CLI should win for model");
        assert_eq!(config.workspace, PathBuf::from("/global/ws"), "Global workspace should apply");
        assert_eq!(config.shell_timeout_secs, 20, "Workspace timeout should win over global");
        assert_eq!(config.context_limit, 8192, "Workspace context_limit should apply");
    }

    #[test]
    fn test_security_log_defaults_to_workspace() {
        let partial = PartialConfig {
            workspace: Some(PathBuf::from("/my/workspace")),
            ..Default::default()
        };
        let config = partial.finalize();
        assert_eq!(
            config.security_log_path,
            PathBuf::from("/my/workspace/security.log"),
            "Security log should default to workspace/security.log"
        );
    }

    #[test]
    fn test_explicit_security_log_overrides_default() {
        let partial = PartialConfig {
            workspace: Some(PathBuf::from("/my/workspace")),
            security_log_path: Some(PathBuf::from("/var/log/ouro.log")),
            ..Default::default()
        };
        let config = partial.finalize();
        assert_eq!(
            config.security_log_path,
            PathBuf::from("/var/log/ouro.log"),
            "Explicit security log should override workspace default"
        );
    }

    #[test]
    fn test_context_config_defaults() {
        let empty = PartialConfig::default();
        let config = empty.finalize();

        assert!((config.soft_threshold_pct - 0.70).abs() < f64::EPSILON);
        assert!((config.hard_threshold_pct - 0.90).abs() < f64::EPSILON);
        assert_eq!(config.carryover_turns, 5);
        assert_eq!(config.max_restarts, None, "max_restarts should default to None (unlimited)");
        assert!(config.auto_restart, "auto_restart should default to true");
    }

    #[test]
    fn test_context_config_override() {
        let partial = PartialConfig {
            soft_threshold_pct: Some(0.60),
            hard_threshold_pct: Some(0.85),
            carryover_turns: Some(10),
            max_restarts: Some(Some(3)),
            auto_restart: Some(false),
            ..Default::default()
        };
        let config = partial.finalize();

        assert!((config.soft_threshold_pct - 0.60).abs() < f64::EPSILON);
        assert!((config.hard_threshold_pct - 0.85).abs() < f64::EPSILON);
        assert_eq!(config.carryover_turns, 10);
        assert_eq!(config.max_restarts, Some(3));
        assert!(!config.auto_restart);
    }

    #[test]
    fn test_context_config_merge_layers() {
        let cli = PartialConfig {
            soft_threshold_pct: Some(0.50),
            ..Default::default()
        };
        let workspace = PartialConfig {
            soft_threshold_pct: Some(0.60),
            hard_threshold_pct: Some(0.85),
            carryover_turns: Some(10),
            ..Default::default()
        };
        let global = PartialConfig {
            soft_threshold_pct: Some(0.75),
            hard_threshold_pct: Some(0.95),
            carryover_turns: Some(3),
            max_restarts: Some(Some(5)),
            auto_restart: Some(false),
            ..Default::default()
        };

        let config = cli
            .with_fallback(workspace)
            .with_fallback(global)
            .finalize();

        assert!((config.soft_threshold_pct - 0.50).abs() < f64::EPSILON, "CLI soft_threshold should win");
        assert!((config.hard_threshold_pct - 0.85).abs() < f64::EPSILON, "Workspace hard_threshold should win over global");
        assert_eq!(config.carryover_turns, 10, "Workspace carryover_turns should win over global");
        assert_eq!(config.max_restarts, Some(5), "Global max_restarts should apply");
        assert!(!config.auto_restart, "Global auto_restart should apply");
    }
}
