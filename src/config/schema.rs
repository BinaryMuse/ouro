use serde::Deserialize;
use std::path::PathBuf;

/// The TOML file structure for ouro.toml.
#[derive(Debug, Deserialize, Default)]
pub struct ConfigFile {
    pub general: Option<GeneralConfig>,
    pub safety: Option<SafetyConfig>,
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

/// Fully-resolved runtime configuration. All fields have values.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub model: String,
    pub workspace: PathBuf,
    pub shell_timeout_secs: u64,
    pub context_limit: usize,
    pub blocked_patterns: Vec<(String, String)>,
    pub security_log_path: PathBuf,
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
}
