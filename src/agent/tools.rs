//! Tool schema definitions and dispatch for the agent loop.
//!
//! Defines the three core tools (`shell_exec`, `file_read`, `file_write`) as
//! [`genai::chat::Tool`] schemas and provides a dispatch function that routes
//! tool calls to their implementations.
//!
//! Tool errors are always returned as structured JSON strings (never panics or
//! `Err` variants) so the model can observe the error and react.

use std::path::Path;

use genai::chat::Tool;
use serde_json::json;

use crate::safety::SafetyLayer;

/// Define the three core tool schemas for the agent.
///
/// Returns a `Vec<Tool>` suitable for passing to
/// [`genai::chat::ChatRequest::with_tools`].
///
/// Tools:
/// 1. `shell_exec` -- Execute a shell command in the workspace directory
/// 2. `file_read` -- Read the contents of a file (unrestricted)
/// 3. `file_write` -- Write content to a file (workspace-restricted)
pub fn define_tools() -> Vec<Tool> {
    vec![
        Tool::new("shell_exec")
            .with_description(
                "Execute a shell command in the workspace directory. \
                 The command runs via `sh -c` with the workspace as the working directory. \
                 Returns a JSON object with fields: stdout, stderr, exit_code, timed_out.",
            )
            .with_schema(json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    }
                },
                "required": ["command"]
            })),
        Tool::new("file_read")
            .with_description(
                "Read the contents of a file. The path can be relative to the workspace \
                 root or an absolute path. Read access is unrestricted -- any file on \
                 the filesystem can be read.",
            )
            .with_schema(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path, relative to workspace or absolute"
                    }
                },
                "required": ["path"]
            })),
        Tool::new("file_write")
            .with_description(
                "Write content to a file within the workspace directory. The path must \
                 be relative to the workspace root. Parent directories are created \
                 automatically if they do not exist. Writes outside the workspace \
                 directory are rejected.",
            )
            .with_schema(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to the workspace root"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write to the file"
                    }
                },
                "required": ["path", "content"]
            })),
    ]
}

/// Return a human-readable description of all available tools.
///
/// This string is embedded in the system prompt so the model understands
/// what tools are available before it sees the formal JSON schemas.
pub fn tool_descriptions() -> String {
    "\
### shell_exec
Execute a shell command in the workspace directory.
- **command** (string, required): The shell command to execute
- Returns: JSON with stdout, stderr, exit_code, timed_out fields
- Commands run via `sh -c` with the workspace as the working directory
- Commands are filtered against a security blocklist

### file_read
Read the contents of a file.
- **path** (string, required): File path, relative to workspace or absolute
- Returns: The file contents as a string
- Read access is unrestricted (can read any file on the filesystem)

### file_write
Write content to a file within the workspace directory.
- **path** (string, required): File path relative to the workspace root
- **content** (string, required): Content to write to the file
- Returns: JSON with written_bytes and path fields
- Parent directories are created automatically
- Writes outside the workspace directory are rejected"
        .to_string()
}

/// Dispatch a tool call to its implementation.
///
/// Routes based on `call.fn_name`:
/// - `shell_exec` -> [`SafetyLayer::execute`]
/// - `file_read` -> [`tokio::fs::read_to_string`]
/// - `file_write` -> workspace-validated [`tokio::fs::write`]
///
/// # Returns
///
/// Always returns a `String` -- either a JSON success payload or a JSON error
/// object `{"error": "..."}`. Never panics or returns `Err`.
///
/// This design ensures the model always receives structured feedback about
/// tool execution outcomes and can react accordingly.
pub async fn dispatch_tool_call(
    call: &genai::chat::ToolCall,
    safety: &SafetyLayer,
    workspace: &Path,
) -> String {
    match call.fn_name.as_str() {
        "shell_exec" => dispatch_shell_exec(call, safety).await,
        "file_read" => dispatch_file_read(call, workspace).await,
        "file_write" => dispatch_file_write(call, safety, workspace).await,
        unknown => {
            json!({"error": format!("Unknown tool: {}", unknown)}).to_string()
        }
    }
}

/// Execute a shell command through the safety layer.
async fn dispatch_shell_exec(call: &genai::chat::ToolCall, safety: &SafetyLayer) -> String {
    let command = match call.fn_arguments.get("command").and_then(|v| v.as_str()) {
        Some(cmd) => cmd,
        None => {
            return json!({"error": "shell_exec: missing or invalid 'command' argument"})
                .to_string();
        }
    };

    match safety.execute(command).await {
        Ok(result) => {
            // ExecResult derives Serialize, so we can serialize it directly.
            serde_json::to_string(&result).unwrap_or_else(|e| {
                json!({"error": format!("Failed to serialize exec result: {}", e)}).to_string()
            })
        }
        Err(e) => json!({"error": format!("shell_exec failed: {}", e)}).to_string(),
    }
}

/// Read a file from the filesystem (unrestricted access).
async fn dispatch_file_read(call: &genai::chat::ToolCall, workspace: &Path) -> String {
    let path_str = match call.fn_arguments.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return json!({"error": "file_read: missing or invalid 'path' argument"}).to_string();
        }
    };

    // Resolve relative paths against workspace; absolute paths used as-is.
    let full_path = if Path::new(path_str).is_absolute() {
        Path::new(path_str).to_path_buf()
    } else {
        workspace.join(path_str)
    };

    match tokio::fs::read_to_string(&full_path).await {
        Ok(content) => content,
        Err(e) => json!({"error": format!("file_read: {}", e)}).to_string(),
    }
}

/// Write content to a file within the workspace directory.
async fn dispatch_file_write(
    call: &genai::chat::ToolCall,
    safety: &SafetyLayer,
    workspace: &Path,
) -> String {
    let path_str = match call.fn_arguments.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return json!({"error": "file_write: missing or invalid 'path' argument"}).to_string();
        }
    };

    let content = match call.fn_arguments.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => {
            return json!({"error": "file_write: missing or invalid 'content' argument"})
                .to_string();
        }
    };

    // Resolve path relative to workspace.
    let full_path = workspace.join(path_str);

    // Validate the write target is within the workspace.
    // We canonicalize the parent (which must exist or be created) and check
    // it starts_with the workspace root, matching WorkspaceGuard logic.
    let ws_root = safety.workspace_root();

    // Ensure parent directories exist so we can canonicalize.
    if let Some(parent) = full_path.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return json!({"error": format!("file_write: failed to create directories: {}", e)})
                .to_string();
        }
    }

    // Canonicalize the parent to resolve symlinks and check containment.
    let canonical_parent = match full_path.parent() {
        Some(parent) => match tokio::fs::canonicalize(parent).await {
            Ok(p) => p,
            Err(e) => {
                return json!({"error": format!("file_write: failed to resolve path: {}", e)})
                    .to_string();
            }
        },
        None => {
            return json!({"error": "file_write: path has no parent directory"}).to_string();
        }
    };

    let canonical_target = canonical_parent.join(
        full_path
            .file_name()
            .unwrap_or_default(),
    );

    if !canonical_target.starts_with(ws_root) {
        return json!({
            "error": format!(
                "file_write: path '{}' is outside the workspace directory",
                path_str
            )
        })
        .to_string();
    }

    // Write the file.
    match tokio::fs::write(&full_path, content).await {
        Ok(()) => {
            json!({
                "written_bytes": content.len(),
                "path": path_str
            })
            .to_string()
        }
        Err(e) => json!({"error": format!("file_write: {}", e)}).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use genai::chat::ToolCall;
    use tempfile::TempDir;

    #[test]
    fn define_tools_returns_three_tools() {
        let tools = define_tools();
        assert_eq!(tools.len(), 3);
    }

    #[test]
    fn define_tools_has_correct_names() {
        let tools = define_tools();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["shell_exec", "file_read", "file_write"]);
    }

    #[test]
    fn define_tools_all_have_descriptions() {
        let tools = define_tools();
        for tool in &tools {
            assert!(
                tool.description.is_some(),
                "Tool '{}' should have a description",
                tool.name
            );
        }
    }

    #[test]
    fn define_tools_all_have_schemas() {
        let tools = define_tools();
        for tool in &tools {
            assert!(
                tool.schema.is_some(),
                "Tool '{}' should have a schema",
                tool.name
            );
        }
    }

    #[test]
    fn tool_descriptions_contains_all_tools() {
        let desc = tool_descriptions();
        assert!(desc.contains("### shell_exec"));
        assert!(desc.contains("### file_read"));
        assert!(desc.contains("### file_write"));
    }

    /// Create a SafetyLayer with a temporary workspace for testing.
    fn make_safety(tmp: &TempDir) -> SafetyLayer {
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let config = AppConfig {
            model: "test-model".to_string(),
            workspace,
            shell_timeout_secs: 10,
            context_limit: 8192,
            blocked_patterns: vec![],
            security_log_path: tmp.path().join("security.log"),
        };

        SafetyLayer::new(&config).unwrap()
    }

    fn make_tool_call(fn_name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            call_id: "test-call-1".to_string(),
            fn_name: fn_name.to_string(),
            fn_arguments: args,
            thought_signatures: None,
        }
    }

    #[tokio::test]
    async fn dispatch_shell_exec_runs_command() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        let call = make_tool_call("shell_exec", json!({"command": "echo hello"}));
        let result = dispatch_tool_call(&call, &safety, &workspace).await;

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["stdout"].as_str().unwrap().trim(), "hello");
        assert_eq!(parsed["exit_code"], 0);
        assert_eq!(parsed["timed_out"], false);
    }

    #[tokio::test]
    async fn dispatch_shell_exec_missing_command() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        let call = make_tool_call("shell_exec", json!({}));
        let result = dispatch_tool_call(&call, &safety, &workspace).await;

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed["error"].as_str().unwrap().contains("missing"));
    }

    #[tokio::test]
    async fn dispatch_file_read_existing_file() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        // Write a test file
        std::fs::write(workspace.join("test.txt"), "file contents here").unwrap();

        let call = make_tool_call("file_read", json!({"path": "test.txt"}));
        let result = dispatch_tool_call(&call, &safety, &workspace).await;

        // file_read returns raw content, not JSON
        assert_eq!(result, "file contents here");
    }

    #[tokio::test]
    async fn dispatch_file_read_nonexistent_file() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        let call = make_tool_call("file_read", json!({"path": "no_such_file.txt"}));
        let result = dispatch_tool_call(&call, &safety, &workspace).await;

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed["error"].as_str().unwrap().contains("file_read"));
    }

    #[tokio::test]
    async fn dispatch_file_read_absolute_path() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        // Write file outside workspace
        let outside = tmp.path().join("outside.txt");
        std::fs::write(&outside, "outside content").unwrap();

        let call = make_tool_call(
            "file_read",
            json!({"path": outside.to_str().unwrap()}),
        );
        let result = dispatch_tool_call(&call, &safety, &workspace).await;

        // Reads are unrestricted
        assert_eq!(result, "outside content");
    }

    #[tokio::test]
    async fn dispatch_file_write_within_workspace() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        let call = make_tool_call(
            "file_write",
            json!({"path": "output.txt", "content": "written content"}),
        );
        let result = dispatch_tool_call(&call, &safety, &workspace).await;

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["written_bytes"], 15);
        assert_eq!(parsed["path"], "output.txt");

        // Verify file was actually written
        let content = std::fs::read_to_string(workspace.join("output.txt")).unwrap();
        assert_eq!(content, "written content");
    }

    #[tokio::test]
    async fn dispatch_file_write_creates_parent_dirs() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        let call = make_tool_call(
            "file_write",
            json!({"path": "sub/dir/file.txt", "content": "nested"}),
        );
        let result = dispatch_tool_call(&call, &safety, &workspace).await;

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["written_bytes"], 6);

        let content = std::fs::read_to_string(workspace.join("sub/dir/file.txt")).unwrap();
        assert_eq!(content, "nested");
    }

    #[tokio::test]
    async fn dispatch_file_write_outside_workspace_rejected() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        let call = make_tool_call(
            "file_write",
            json!({"path": "../escape.txt", "content": "should fail"}),
        );
        let result = dispatch_tool_call(&call, &safety, &workspace).await;

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(
            parsed["error"]
                .as_str()
                .unwrap()
                .contains("outside the workspace"),
            "Expected workspace violation error, got: {}",
            result
        );

        // Verify file was NOT written
        assert!(!tmp.path().join("escape.txt").exists());
    }

    #[tokio::test]
    async fn dispatch_file_write_missing_content() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        let call = make_tool_call("file_write", json!({"path": "file.txt"}));
        let result = dispatch_tool_call(&call, &safety, &workspace).await;

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed["error"].as_str().unwrap().contains("content"));
    }

    #[tokio::test]
    async fn dispatch_unknown_tool() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        let call = make_tool_call("nonexistent_tool", json!({}));
        let result = dispatch_tool_call(&call, &safety, &workspace).await;

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed["error"]
            .as_str()
            .unwrap()
            .contains("Unknown tool: nonexistent_tool"));
    }
}
