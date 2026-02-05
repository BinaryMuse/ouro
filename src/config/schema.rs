use serde::Deserialize;
use std::path::PathBuf;

/// The TOML file structure for ouro.toml.
#[derive(Debug, Deserialize, Default)]
pub struct ConfigFile {
    pub general: Option<GeneralConfig>,
    pub safety: Option<SafetyConfig>,
    pub context: Option<ContextConfig>,
}

#[derive(Debug, Deserialize)]
pub struct GeneralConfig {
    pub model: Option<String>,
    pub workspace: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SafetyConfig {
    pub shell_timeout_secs: Option<u64>,
    pub context_limit: Option<usize>,
    /// If specified, fully replaces the default blocklist.
    pub blocked_patterns: Option<Vec<BlocklistEntry>>,
    pub security_log: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlocklistEntry {
    pub pattern: String,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub struct ContextConfig {
    pub soft_threshold_pct: Option<f64>,
    pub hard_threshold_pct: Option<f64>,
    pub carryover_turns: Option<usize>,
    pub max_restarts: Option<u32>,
    pub auto_restart: Option<bool>,
}

/// Fully-resolved runtime configuration. All fields have values.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub model: String,
    pub workspace: PathBuf,
    pub shell_timeout_secs: u64,
    pub context_limit: usize,
    pub blocked_patterns: Vec<(String, String)>,
    pub security_log_path: PathBuf,
    pub soft_threshold_pct: f64,
    pub hard_threshold_pct: f64,
    pub carryover_turns: usize,
    pub max_restarts: Option<u32>,
    pub auto_restart: bool,
}

/// Partial config used during merge. All fields are Option so that
/// missing fields don't override lower-priority values.
#[derive(Debug, Clone, Default)]
pub struct PartialConfig {
    pub model: Option<String>,
    pub workspace: Option<PathBuf>,
    pub shell_timeout_secs: Option<u64>,
    pub context_limit: Option<usize>,
    pub blocked_patterns: Option<Vec<(String, String)>>,
    pub security_log_path: Option<PathBuf>,
    pub soft_threshold_pct: Option<f64>,
    pub hard_threshold_pct: Option<f64>,
    pub carryover_turns: Option<usize>,
    pub max_restarts: Option<Option<u32>>,
    pub auto_restart: Option<bool>,
}

impl ConfigFile {
    /// Convert a parsed TOML config file into a PartialConfig for merging.
    #[allow(clippy::wrong_self_convention)]
    pub fn to_partial(self) -> PartialConfig {
        let mut partial = PartialConfig::default();

        if let Some(general) = self.general {
            partial.model = general.model;
            partial.workspace = general.workspace.map(PathBuf::from);
        }

        if let Some(safety) = self.safety {
            partial.shell_timeout_secs = safety.shell_timeout_secs;
            partial.context_limit = safety.context_limit;
            partial.blocked_patterns = safety.blocked_patterns.map(|entries| {
                entries
                    .into_iter()
                    .map(|e| (e.pattern, e.reason))
                    .collect()
            });
            partial.security_log_path = safety.security_log.map(PathBuf::from);
        }

        if let Some(context) = self.context {
            partial.soft_threshold_pct = context.soft_threshold_pct;
            partial.hard_threshold_pct = context.hard_threshold_pct;
            partial.carryover_turns = context.carryover_turns;
            partial.max_restarts = context.max_restarts.map(Some);
            partial.auto_restart = context.auto_restart;
        }

        partial
    }
}
