//! Discovery persistence module for JSONL-backed discovery storage.
//!
//! The agent flags noteworthy discoveries via tool calls. Each discovery is
//! appended as a single JSON line to `{workspace}/.ouro-discoveries.jsonl`.
//! The file survives context resets since it lives in the workspace directory,
//! not the log directory.
//!
//! Uses synchronous `std::fs` for small buffered writes with flush -- same
//! pattern as `SessionLogger` in `agent/logging.rs`.

use std::fs::OpenOptions;
use std::io::{BufRead, BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A single discovery flagged by the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Discovery {
    /// ISO 8601 timestamp when the discovery was flagged.
    pub timestamp: String,
    /// Short title summarizing the discovery.
    pub title: String,
    /// Longer description with context and details.
    pub description: String,
}

/// Returns the path to the discoveries JSONL file for a given workspace.
pub fn discovery_file_path(workspace: &Path) -> PathBuf {
    workspace.join(".ouro-discoveries.jsonl")
}

/// Append a single discovery to the JSONL file.
///
/// Creates the file if it does not exist. Each discovery is serialized as a
/// single JSON line followed by a newline, then flushed for durability.
///
/// Returns `Err(String)` on any I/O or serialization error, matching the
/// codebase convention of error-as-string for tool dispatch results.
pub fn append_discovery(workspace: &Path, discovery: &Discovery) -> Result<(), String> {
    let path = discovery_file_path(workspace);
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("Failed to open discoveries file: {e}"))?;

    let mut writer = BufWriter::new(file);
    serde_json::to_writer(&mut writer, discovery)
        .map_err(|e| format!("Failed to serialize discovery: {e}"))?;
    writer
        .write_all(b"\n")
        .map_err(|e| format!("Failed to write newline: {e}"))?;
    writer
        .flush()
        .map_err(|e| format!("Failed to flush discoveries file: {e}"))?;
    Ok(())
}

/// Load all discoveries from the JSONL file.
///
/// Returns an empty `Vec` if the file does not exist. Unparseable lines are
/// silently skipped (lenient reader) so that partial writes from crashes do
/// not prevent loading the rest of the file.
#[allow(dead_code)]
pub fn load_discoveries(workspace: &Path) -> Vec<Discovery> {
    let path = discovery_file_path(workspace);
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };

    std::io::BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<Discovery>(&line).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Create a temporary workspace directory for testing.
    fn make_workspace() -> TempDir {
        TempDir::new().expect("tempdir")
    }

    #[test]
    fn append_and_load_roundtrip() {
        let ws = make_workspace();
        let d = Discovery {
            timestamp: "2026-02-04T10:00:00Z".to_string(),
            title: "Found Makefile".to_string(),
            description: "Project root contains build targets".to_string(),
        };

        append_discovery(ws.path(), &d).expect("append");
        let loaded = load_discoveries(ws.path());

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].title, "Found Makefile");
        assert_eq!(loaded[0].description, "Project root contains build targets");
        assert_eq!(loaded[0].timestamp, "2026-02-04T10:00:00Z");
    }

    #[test]
    fn load_from_nonexistent_file_returns_empty() {
        let ws = make_workspace();
        let loaded = load_discoveries(ws.path());
        assert!(loaded.is_empty());
    }

    #[test]
    fn lenient_parsing_skips_corrupt_lines() {
        let ws = make_workspace();
        let path = discovery_file_path(ws.path());

        // Write a mix of valid and corrupt lines
        let content = concat!(
            r#"{"timestamp":"2026-01-01T00:00:00Z","title":"Good","description":"Valid line"}"#,
            "\n",
            "this is not json\n",
            r#"{"timestamp":"2026-01-02T00:00:00Z","title":"Also Good","description":"Another valid line"}"#,
            "\n",
            "{broken json\n",
        );
        std::fs::write(&path, content).expect("write test file");

        let loaded = load_discoveries(ws.path());
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].title, "Good");
        assert_eq!(loaded[1].title, "Also Good");
    }

    #[test]
    fn multiple_appends_produce_correct_jsonl() {
        let ws = make_workspace();

        for i in 0..5 {
            let d = Discovery {
                timestamp: format!("2026-02-04T10:0{i}:00Z"),
                title: format!("Discovery {i}"),
                description: format!("Description for discovery {i}"),
            };
            append_discovery(ws.path(), &d).expect("append");
        }

        let loaded = load_discoveries(ws.path());
        assert_eq!(loaded.len(), 5);

        for (i, d) in loaded.iter().enumerate() {
            assert_eq!(d.title, format!("Discovery {i}"));
            assert_eq!(d.description, format!("Description for discovery {i}"));
        }

        // Verify the raw file has exactly 5 lines
        let raw = std::fs::read_to_string(discovery_file_path(ws.path())).expect("read");
        let lines: Vec<&str> = raw.lines().collect();
        assert_eq!(lines.len(), 5);

        // Each line should be valid JSON
        for line in &lines {
            let _: serde_json::Value =
                serde_json::from_str(line).expect("each line should be valid JSON");
        }
    }

    #[test]
    fn discovery_file_path_is_in_workspace() {
        let path = discovery_file_path(Path::new("/tmp/my-workspace"));
        assert_eq!(path, PathBuf::from("/tmp/my-workspace/.ouro-discoveries.jsonl"));
    }
}
