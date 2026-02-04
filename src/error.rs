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

/// Errors related to the agent loop and its subsystems.
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("Ollama not reachable at {url}: {message}")]
    OllamaUnavailable { url: String, message: String },

    #[error("Model '{model}' not available in Ollama: {message}")]
    ModelNotAvailable { model: String, message: String },

    #[error("System prompt not found at {path}")]
    SystemPromptNotFound { path: PathBuf },

    #[error("LLM error: {0}")]
    LlmError(String),

    #[error("Tool execution error: {0}")]
    ToolError(String),

    #[error("Session logging error: {0}")]
    LoggingError(String),

    #[error("Context window full after {turns} turns")]
    ContextFull { turns: u64 },
}
