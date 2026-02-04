//! JSONL session logger for full session replay.
//!
//! Writes structured events to timestamped JSONL files stored alongside the
//! workspace directory (not inside it). Each session produces a file named
//! `session-{ISO8601}.jsonl` in `{workspace_parent}/.ouro-logs/`.
//!
//! Uses synchronous `std::fs` since writes are small, buffered, and flushed
//! after each event -- no async complexity needed for append-only logging.

use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Serialize;

/// Returns the current UTC time as an ISO 8601 string with milliseconds.
fn now_iso() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// A structured log entry serialized as a single JSON line.
///
/// Tagged with `event_type` so each line is self-describing for replay.
#[derive(Debug, Serialize)]
#[serde(tag = "event_type")]
pub enum LogEntry {
    /// Marks the beginning of an agent session.
    #[serde(rename = "session_start")]
    SessionStart {
        timestamp: String,
        model: String,
        workspace: String,
    },

    /// An assistant text response (thinking out loud or final answer).
    #[serde(rename = "assistant_text")]
    AssistantText {
        timestamp: String,
        turn: u64,
        content: String,
    },

    /// A tool call requested by the model.
    #[serde(rename = "tool_call")]
    ToolCall {
        timestamp: String,
        turn: u64,
        call_id: String,
        fn_name: String,
        fn_arguments: serde_json::Value,
    },

    /// The result of a tool call execution.
    #[serde(rename = "tool_result")]
    ToolResult {
        timestamp: String,
        turn: u64,
        call_id: String,
        fn_name: String,
        result: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },

    /// A system-injected message (e.g., nudges, context warnings).
    #[serde(rename = "system_message")]
    SystemMessage {
        timestamp: String,
        content: String,
    },

    /// An error encountered during the session.
    #[serde(rename = "error")]
    Error {
        timestamp: String,
        turn: u64,
        message: String,
    },

    /// Marks the end of an agent session.
    #[serde(rename = "session_end")]
    SessionEnd {
        timestamp: String,
        total_turns: u64,
        reason: String,
    },
}

/// Append-only JSONL logger for agent sessions.
///
/// Creates a timestamped log file in `{workspace_parent}/.ouro-logs/` and
/// writes one JSON object per line. Flushes after each event for durability.
pub struct SessionLogger {
    writer: BufWriter<fs::File>,
    log_path: PathBuf,
}

impl SessionLogger {
    /// Create a new session logger for the given workspace path.
    ///
    /// Log directory is `{workspace_parent}/.ouro-logs/`. The session file
    /// is named `session-{ISO8601}.jsonl` with colons replaced by dashes
    /// for filesystem safety.
    pub fn new(workspace_path: &Path) -> anyhow::Result<Self> {
        let log_dir = Self::log_dir_for(workspace_path)?;
        fs::create_dir_all(&log_dir)?;

        let session_id = Utc::now()
            .format("%Y-%m-%dT%H-%M-%S")
            .to_string();
        let filename = format!("session-{session_id}.jsonl");
        let log_path = log_dir.join(filename);

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;

        Ok(Self {
            writer: BufWriter::new(file),
            log_path,
        })
    }

    /// Compute the log directory for a given workspace path.
    ///
    /// Returns `{workspace_parent}/.ouro-logs/`.
    fn log_dir_for(workspace_path: &Path) -> anyhow::Result<PathBuf> {
        let parent = workspace_path.parent().ok_or_else(|| {
            anyhow::anyhow!(
                "Workspace path '{}' has no parent directory",
                workspace_path.display()
            )
        })?;
        Ok(parent.join(".ouro-logs"))
    }

    /// Serialize a log entry as a single JSON line and flush.
    pub fn log_event(&mut self, event: &LogEntry) -> anyhow::Result<()> {
        serde_json::to_writer(&mut self.writer, event)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        Ok(())
    }

    /// Return the path to the current session log file.
    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    /// Convenience: log a session_start event.
    pub fn log_session_start(&mut self, model: &str, workspace: &Path) -> anyhow::Result<()> {
        self.log_event(&LogEntry::SessionStart {
            timestamp: now_iso(),
            model: model.to_string(),
            workspace: workspace.display().to_string(),
        })
    }

    /// Convenience: log a session_end event.
    pub fn log_session_end(&mut self, total_turns: u64, reason: &str) -> anyhow::Result<()> {
        self.log_event(&LogEntry::SessionEnd {
            timestamp: now_iso(),
            total_turns,
            reason: reason.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use tempfile::TempDir;

    /// Create a SessionLogger pointing at a temporary workspace.
    fn make_logger() -> (SessionLogger, TempDir) {
        let tmp = TempDir::new().expect("tempdir");
        // workspace_path = tmp/workspace (doesn't need to exist itself)
        let workspace = tmp.path().join("workspace");
        let logger = SessionLogger::new(&workspace).expect("SessionLogger::new");
        (logger, tmp)
    }

    #[test]
    fn creates_log_file_in_sibling_dir() {
        let (logger, tmp) = make_logger();
        let log_path = logger.log_path().to_owned();

        // Log file exists
        assert!(log_path.exists(), "log file should exist at {log_path:?}");

        // Log dir is .ouro-logs sibling of workspace
        let log_dir = tmp.path().join(".ouro-logs");
        assert!(log_dir.is_dir(), ".ouro-logs dir should exist");
        assert!(
            log_path.starts_with(&log_dir),
            "log file should be inside .ouro-logs"
        );

        // Filename format: session-YYYY-MM-DDTHH-MM-SS.jsonl
        let name = log_path.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("session-"), "filename should start with 'session-'");
        assert!(name.ends_with(".jsonl"), "filename should end with '.jsonl'");
    }

    #[test]
    fn log_session_start_writes_valid_jsonl() {
        let (mut logger, _tmp) = make_logger();
        let workspace = PathBuf::from("/tmp/test-workspace");

        logger
            .log_session_start("qwen2.5:7b", &workspace)
            .expect("log_session_start");

        // Read the log file
        let file = fs::File::open(logger.log_path()).expect("open log");
        let lines: Vec<String> = std::io::BufReader::new(file)
            .lines()
            .collect::<Result<_, _>>()
            .expect("read lines");

        assert_eq!(lines.len(), 1, "should have exactly one line");

        // Parse as JSON
        let entry: serde_json::Value =
            serde_json::from_str(&lines[0]).expect("valid JSON");

        assert_eq!(entry["event_type"], "session_start");
        assert_eq!(entry["model"], "qwen2.5:7b");
        assert_eq!(entry["workspace"], "/tmp/test-workspace");
        assert!(entry["timestamp"].is_string());
    }

    #[test]
    fn log_session_end_writes_valid_jsonl() {
        let (mut logger, _tmp) = make_logger();

        logger
            .log_session_end(42, "context_full")
            .expect("log_session_end");

        let file = fs::File::open(logger.log_path()).expect("open log");
        let lines: Vec<String> = std::io::BufReader::new(file)
            .lines()
            .collect::<Result<_, _>>()
            .expect("read lines");

        assert_eq!(lines.len(), 1);

        let entry: serde_json::Value =
            serde_json::from_str(&lines[0]).expect("valid JSON");

        assert_eq!(entry["event_type"], "session_end");
        assert_eq!(entry["total_turns"], 42);
        assert_eq!(entry["reason"], "context_full");
    }

    #[test]
    fn multiple_events_produce_multiple_lines() {
        let (mut logger, _tmp) = make_logger();
        let workspace = PathBuf::from("/tmp/ws");

        logger.log_session_start("test-model", &workspace).unwrap();
        logger
            .log_event(&LogEntry::AssistantText {
                timestamp: now_iso(),
                turn: 1,
                content: "Hello, I will start working.".to_string(),
            })
            .unwrap();
        logger
            .log_event(&LogEntry::ToolCall {
                timestamp: now_iso(),
                turn: 1,
                call_id: "call_001".to_string(),
                fn_name: "shell_exec".to_string(),
                fn_arguments: serde_json::json!({"command": "ls -la"}),
            })
            .unwrap();
        logger
            .log_event(&LogEntry::ToolResult {
                timestamp: now_iso(),
                turn: 1,
                call_id: "call_001".to_string(),
                fn_name: "shell_exec".to_string(),
                result: "total 0\ndrwxr-xr-x 2 user user 64 Feb 4 10:00 .".to_string(),
                error: None,
            })
            .unwrap();
        logger.log_session_end(1, "user_stopped").unwrap();

        let file = fs::File::open(logger.log_path()).expect("open log");
        let lines: Vec<String> = std::io::BufReader::new(file)
            .lines()
            .collect::<Result<_, _>>()
            .expect("read lines");

        assert_eq!(lines.len(), 5, "should have 5 events");

        // Every line should be valid JSON
        for (i, line) in lines.iter().enumerate() {
            let _: serde_json::Value =
                serde_json::from_str(line).unwrap_or_else(|e| panic!("line {i} invalid JSON: {e}"));
        }
    }

    #[test]
    fn tool_result_with_error_field() {
        let (mut logger, _tmp) = make_logger();

        logger
            .log_event(&LogEntry::ToolResult {
                timestamp: now_iso(),
                turn: 3,
                call_id: "call_err".to_string(),
                fn_name: "file_read".to_string(),
                result: String::new(),
                error: Some("file not found: /no/such/file".to_string()),
            })
            .unwrap();

        let file = fs::File::open(logger.log_path()).expect("open log");
        let line = std::io::BufReader::new(file)
            .lines()
            .next()
            .unwrap()
            .unwrap();

        let entry: serde_json::Value = serde_json::from_str(&line).expect("valid JSON");
        assert_eq!(entry["event_type"], "tool_result");
        assert_eq!(entry["error"], "file not found: /no/such/file");
    }

    #[test]
    fn tool_result_without_error_omits_field() {
        let (mut logger, _tmp) = make_logger();

        logger
            .log_event(&LogEntry::ToolResult {
                timestamp: now_iso(),
                turn: 1,
                call_id: "call_ok".to_string(),
                fn_name: "shell_exec".to_string(),
                result: "ok".to_string(),
                error: None,
            })
            .unwrap();

        let file = fs::File::open(logger.log_path()).expect("open log");
        let line = std::io::BufReader::new(file)
            .lines()
            .next()
            .unwrap()
            .unwrap();

        let entry: serde_json::Value = serde_json::from_str(&line).expect("valid JSON");
        assert_eq!(entry["event_type"], "tool_result");
        // "error" field should be absent (skip_serializing_if = None)
        assert!(entry.get("error").is_none(), "error field should be absent when None");
    }

    #[test]
    fn system_message_and_error_events() {
        let (mut logger, _tmp) = make_logger();

        logger
            .log_event(&LogEntry::SystemMessage {
                timestamp: now_iso(),
                content: "Context window at 80% capacity".to_string(),
            })
            .unwrap();

        logger
            .log_event(&LogEntry::Error {
                timestamp: now_iso(),
                turn: 5,
                message: "Ollama connection lost".to_string(),
            })
            .unwrap();

        let file = fs::File::open(logger.log_path()).expect("open log");
        let lines: Vec<String> = std::io::BufReader::new(file)
            .lines()
            .collect::<Result<_, _>>()
            .expect("read lines");

        assert_eq!(lines.len(), 2);

        let sys: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(sys["event_type"], "system_message");
        assert_eq!(sys["content"], "Context window at 80% capacity");

        let err: serde_json::Value = serde_json::from_str(&lines[1]).unwrap();
        assert_eq!(err["event_type"], "error");
        assert_eq!(err["turn"], 5);
        assert_eq!(err["message"], "Ollama connection lost");
    }
}
