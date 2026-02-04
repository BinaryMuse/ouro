/// Result of a shell command execution.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
}
