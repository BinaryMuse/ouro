use std::path::PathBuf;

/// Errors related to configuration loading and parsing.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Failed to parse config at {path}: {message}")]
    ParseError { path: PathBuf, message: String },

    #[error("Config merge error: {0}")]
    MergeError(String),
}

/// Errors related to safety guardrails (command filtering, workspace enforcement).
#[derive(Debug, thiserror::Error)]
pub enum GuardrailError {
    #[error("Command blocked: `{command}` - {reason}")]
    CommandBlocked { command: String, reason: String },

    #[error("Write outside workspace: `{path}` is not within `{workspace}`")]
    WriteOutsideWorkspace { path: PathBuf, workspace: PathBuf },
}

/// Errors related to shell command execution.
#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("Failed to spawn shell process: {0}")]
    SpawnFailed(String),

    #[error("Command timed out after {timeout_secs}s; partial output: {partial_output}")]
    TimedOut {
        timeout_secs: u64,
        partial_output: String,
    },

    #[error("Process execution failed: {0}")]
    ProcessFailed(String),
}
