//! Tool schema definitions and dispatch for the agent loop.
//!
//! Defines the nine agent tools (3 core + 6 sub-agent orchestration) as
//! [`genai::chat::Tool`] schemas and provides a dispatch function that routes
//! tool calls to their implementations.
//!
//! Tool errors are always returned as structured JSON strings (never panics or
//! `Err` variants) so the model can observe the error and react.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use genai::chat::Tool;
use serde_json::json;

use crate::config::AppConfig;
use crate::orchestration::background_proc::spawn_background_process;
use crate::orchestration::llm_agent::spawn_llm_sub_agent;
use crate::orchestration::manager::SubAgentManager;
use crate::safety::SafetyLayer;

/// Define the agent tool schemas.
///
/// Returns a `Vec<Tool>` suitable for passing to
/// [`genai::chat::ChatRequest::with_tools`].
///
/// When `filter` is `None`, returns all 9 tools (3 core + 6 orchestration).
/// When `filter` is `Some(names)`, returns only tools whose names appear in
/// the filter list. This enables sub-agents to have a customized tool set.
///
/// Tools:
/// 1. `shell_exec` -- Execute a shell command in the workspace directory
/// 2. `file_read` -- Read the contents of a file (unrestricted)
/// 3. `file_write` -- Write content to a file (workspace-restricted)
/// 4. `spawn_llm_session` -- Spawn a child LLM chat session
/// 5. `spawn_background_task` -- Spawn a background shell process
/// 6. `agent_status` -- Query sub-agent/process status
/// 7. `agent_result` -- Retrieve a completed sub-agent's result
/// 8. `kill_agent` -- Terminate a running sub-agent or process
/// 9. `write_stdin` -- Write data to a background process's stdin
pub fn define_tools(filter: Option<&[String]>) -> Vec<Tool> {
    let all = vec![
        // -- Core tools (1-3) --
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
        // -- Sub-agent orchestration tools (4-9) --
        Tool::new("spawn_llm_session")
            .with_description(
                "Spawn a child LLM chat session that runs concurrently. \
                 Returns the sub-agent ID for status tracking.",
            )
            .with_schema(json!({
                "type": "object",
                "properties": {
                    "goal": {
                        "type": "string",
                        "description": "What the sub-agent should accomplish"
                    },
                    "model": {
                        "type": "string",
                        "description": "Ollama model name (defaults to current model)"
                    },
                    "context": {
                        "type": "object",
                        "description": "Key-value context injected into the sub-agent's prompt",
                        "additionalProperties": { "type": "string" }
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Maximum runtime in seconds"
                    },
                    "tools": {
                        "type": "array",
                        "description": "Tool names to enable (default: all your tools)",
                        "items": { "type": "string" }
                    }
                },
                "required": ["goal"]
            })),
        Tool::new("spawn_background_task")
            .with_description(
                "Spawn a background shell process that runs independently. \
                 Returns the process ID for monitoring.",
            )
            .with_schema(json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to run"
                    }
                },
                "required": ["command"]
            })),
        Tool::new("agent_status")
            .with_description(
                "Query the status of all sub-agents and background processes, \
                 or a specific one by ID.",
            )
            .with_schema(json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "Specific agent ID to query (omit for all)"
                    }
                }
            })),
        Tool::new("agent_result")
            .with_description(
                "Retrieve the structured result of a completed sub-agent. \
                 Returns error if agent is still running.",
            )
            .with_schema(json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "The agent ID to retrieve results for"
                    }
                },
                "required": ["agent_id"]
            })),
        Tool::new("kill_agent")
            .with_description(
                "Terminate a running sub-agent or background process.",
            )
            .with_schema(json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "The agent ID to terminate"
                    }
                },
                "required": ["agent_id"]
            })),
        Tool::new("write_stdin")
            .with_description(
                "Write data to a running background process's stdin. \
                 Useful for interactive programs.",
            )
            .with_schema(json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "The background process ID"
                    },
                    "data": {
                        "type": "string",
                        "description": "Data to write (newline appended automatically)"
                    }
                },
                "required": ["agent_id", "data"]
            })),
    ];

    match filter {
        None => all,
        Some(names) => all
            .into_iter()
            .filter(|t| names.iter().any(|n| n == &t.name))
            .collect(),
    }
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
/// - `spawn_llm_session` -> [`spawn_llm_sub_agent`]
/// - `spawn_background_task` -> [`spawn_background_process`]
/// - `agent_status` -> [`SubAgentManager::get_status`] / [`SubAgentManager::list_all`]
/// - `agent_result` -> [`SubAgentManager::get_result`]
/// - `kill_agent` -> [`SubAgentManager::cancel_agent`]
/// - `write_stdin` -> [`SubAgentManager::write_to_stdin`]
///
/// The `manager` and `config` parameters are `Option` so that existing callers
/// (tests, sub-agents without manager access) still work. When `None` and a
/// sub-agent tool is called, returns `{"error": "Sub-agent tools not available"}`.
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
    manager: Option<&SubAgentManager>,
    config: Option<&AppConfig>,
) -> String {
    match call.fn_name.as_str() {
        "shell_exec" => dispatch_shell_exec(call, safety).await,
        "file_read" => dispatch_file_read(call, workspace).await,
        "file_write" => dispatch_file_write(call, safety, workspace).await,
        "spawn_llm_session" => dispatch_spawn_llm_session(call, manager, config).await,
        "spawn_background_task" => dispatch_spawn_background_task(call, manager, config).await,
        "agent_status" => dispatch_agent_status(call, manager),
        "agent_result" => dispatch_agent_result(call, manager),
        "kill_agent" => dispatch_kill_agent(call, manager),
        "write_stdin" => dispatch_write_stdin(call, manager).await,
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

// ---------------------------------------------------------------------------
// Sub-agent tool dispatch helpers
// ---------------------------------------------------------------------------

/// Helper: return early if manager/config are not available for sub-agent tools.
macro_rules! require_manager {
    ($manager:expr) => {
        match $manager {
            Some(m) => m,
            None => {
                return json!({"error": "Sub-agent tools not available"}).to_string();
            }
        }
    };
}

/// Extract spawn_llm_session arguments from a tool call.
///
/// Returns `(goal, model, context, timeout, tool_filter)` or an error string.
fn extract_spawn_llm_args(
    call: &genai::chat::ToolCall,
) -> Result<
    (
        String,
        Option<String>,
        HashMap<String, String>,
        Option<Duration>,
        Option<Vec<String>>,
    ),
    String,
> {
    let goal = call
        .fn_arguments
        .get("goal")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            json!({"error": "spawn_llm_session: missing or invalid 'goal' argument"}).to_string()
        })?;

    let model = call
        .fn_arguments
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let context: HashMap<String, String> = call
        .fn_arguments
        .get("context")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    let timeout = call
        .fn_arguments
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .map(Duration::from_secs);

    let tool_filter = call
        .fn_arguments
        .get("tools")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<String>>()
        });

    Ok((goal, model, context, timeout, tool_filter))
}

/// Spawn a child LLM session via the orchestration layer.
///
/// Extracts arguments synchronously, clones the manager and config into owned
/// values, then spawns the work as a `tokio::spawn` task with an explicit
/// `Pin<Box<dyn Future + Send>>` return type. This type erasure breaks the
/// opaque type cycle between `dispatch_tool_call -> spawn_llm_sub_agent ->
/// run_agent_session -> dispatch_tool_call`.
fn dispatch_spawn_llm_session(
    call: &genai::chat::ToolCall,
    manager: Option<&SubAgentManager>,
    config: Option<&AppConfig>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = String> + Send>> {
    // Early returns for validation (no async needed).
    let mgr = match manager {
        Some(m) => m,
        None => {
            return Box::pin(std::future::ready(
                json!({"error": "Sub-agent tools not available"}).to_string(),
            ));
        }
    };
    let cfg = match config {
        Some(c) => c,
        None => {
            return Box::pin(std::future::ready(
                json!({"error": "Sub-agent tools not available (no config)"}).to_string(),
            ));
        }
    };

    let (goal, model, context, timeout, tool_filter) = match extract_spawn_llm_args(call) {
        Ok(args) => args,
        Err(e) => return Box::pin(std::future::ready(e)),
    };

    // Clone into owned values for the spawned task.
    let mgr_owned = mgr.clone();
    let cfg_owned = cfg.clone();

    Box::pin(async move {
        // Spawn a task that owns all values -- its future IS Send + 'static.
        let handle = tokio::spawn(async move {
            let parent_id = None; // Root agent has no parent_id.
            match spawn_llm_sub_agent(
                &mgr_owned,
                goal,
                model,
                context,
                timeout,
                tool_filter,
                parent_id,
                &cfg_owned,
            )
            .await
            {
                Ok(agent_id) => json!({"agent_id": agent_id, "status": "spawned"}).to_string(),
                Err(e) => json!({"error": format!("spawn_llm_session: {e}")}).to_string(),
            }
        });

        match handle.await {
            Ok(result) => result,
            Err(e) => json!({"error": format!("spawn_llm_session task failed: {e}")}).to_string(),
        }
    })
}

/// Spawn a background shell process via the orchestration layer.
fn dispatch_spawn_background_task(
    call: &genai::chat::ToolCall,
    manager: Option<&SubAgentManager>,
    config: Option<&AppConfig>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = String> + Send>> {
    let mgr = match manager {
        Some(m) => m,
        None => {
            return Box::pin(std::future::ready(
                json!({"error": "Sub-agent tools not available"}).to_string(),
            ));
        }
    };
    let cfg = match config {
        Some(c) => c,
        None => {
            return Box::pin(std::future::ready(
                json!({"error": "Sub-agent tools not available (no config)"}).to_string(),
            ));
        }
    };

    let command = match call.fn_arguments.get("command").and_then(|v| v.as_str()) {
        Some(cmd) => cmd.to_string(),
        None => {
            return Box::pin(std::future::ready(
                json!({"error": "spawn_background_task: missing or invalid 'command' argument"})
                    .to_string(),
            ));
        }
    };

    // Clone into owned values for the spawned task.
    let mgr_owned = mgr.clone();
    let cfg_owned = cfg.clone();

    Box::pin(async move {
        let handle = tokio::spawn(async move {
            let parent_id = None; // Root agent has no parent_id.
            match spawn_background_process(&mgr_owned, command, parent_id, &cfg_owned).await {
                Ok(agent_id) => json!({"agent_id": agent_id, "status": "spawned"}).to_string(),
                Err(e) => json!({"error": format!("spawn_background_task: {e}")}).to_string(),
            }
        });

        match handle.await {
            Ok(result) => result,
            Err(e) => {
                json!({"error": format!("spawn_background_task task failed: {e}")}).to_string()
            }
        }
    })
}

/// Query sub-agent status (single or all).
fn dispatch_agent_status(
    call: &genai::chat::ToolCall,
    manager: Option<&SubAgentManager>,
) -> String {
    let mgr = require_manager!(manager);

    let agent_id = call
        .fn_arguments
        .get("agent_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if let Some(id) = agent_id {
        match mgr.get_status(&id) {
            Some(info) => serde_json::to_string(&info).unwrap_or_else(|e| {
                json!({"error": format!("Failed to serialize status: {e}")}).to_string()
            }),
            None => json!({"error": format!("Agent not found: {id}")}).to_string(),
        }
    } else {
        let all = mgr.list_all();
        serde_json::to_string(&all).unwrap_or_else(|e| {
            json!({"error": format!("Failed to serialize status list: {e}")}).to_string()
        })
    }
}

/// Retrieve a completed sub-agent's structured result.
fn dispatch_agent_result(
    call: &genai::chat::ToolCall,
    manager: Option<&SubAgentManager>,
) -> String {
    let mgr = require_manager!(manager);

    let agent_id = match call.fn_arguments.get("agent_id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => {
            return json!({"error": "agent_result: missing or invalid 'agent_id' argument"})
                .to_string();
        }
    };

    match mgr.get_result(&agent_id) {
        Some(result) => serde_json::to_string(&result).unwrap_or_else(|e| {
            json!({"error": format!("Failed to serialize result: {e}")}).to_string()
        }),
        None => {
            json!({"error": "No result available (agent may still be running or not found)"})
                .to_string()
        }
    }
}

/// Terminate a running sub-agent or background process.
fn dispatch_kill_agent(
    call: &genai::chat::ToolCall,
    manager: Option<&SubAgentManager>,
) -> String {
    let mgr = require_manager!(manager);

    let agent_id = match call.fn_arguments.get("agent_id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => {
            return json!({"error": "kill_agent: missing or invalid 'agent_id' argument"})
                .to_string();
        }
    };

    if mgr.cancel_agent(&agent_id) {
        json!({"killed": true}).to_string()
    } else {
        json!({"error": "Agent not found"}).to_string()
    }
}

/// Write data to a running background process's stdin.
///
/// Returns `Pin<Box<dyn Future + Send>>` for consistency with other async
/// sub-agent dispatch functions (avoids Send issues in tokio::spawn chains).
fn dispatch_write_stdin(
    call: &genai::chat::ToolCall,
    manager: Option<&SubAgentManager>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = String> + Send>> {
    let mgr = match manager {
        Some(m) => m,
        None => {
            return Box::pin(std::future::ready(
                json!({"error": "Sub-agent tools not available"}).to_string(),
            ));
        }
    };

    let agent_id = match call.fn_arguments.get("agent_id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => {
            return Box::pin(std::future::ready(
                json!({"error": "write_stdin: missing or invalid 'agent_id' argument"}).to_string(),
            ));
        }
    };

    let data = match call.fn_arguments.get("data").and_then(|v| v.as_str()) {
        Some(d) => d.to_string(),
        None => {
            return Box::pin(std::future::ready(
                json!({"error": "write_stdin: missing or invalid 'data' argument"}).to_string(),
            ));
        }
    };

    let mgr_owned = mgr.clone();

    Box::pin(async move {
        let handle = tokio::spawn(async move {
            let data_with_newline = format!("{data}\n");
            match mgr_owned
                .write_to_stdin(&agent_id, data_with_newline.as_bytes())
                .await
            {
                Ok(n) => json!({"written_bytes": n}).to_string(),
                Err(e) => json!({"error": format!("write_stdin: {e}")}).to_string(),
            }
        });

        match handle.await {
            Ok(result) => result,
            Err(e) => json!({"error": format!("write_stdin task failed: {e}")}).to_string(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use genai::chat::ToolCall;
    use tempfile::TempDir;

    #[test]
    fn define_tools_returns_nine_tools() {
        let tools = define_tools(None);
        assert_eq!(tools.len(), 9);
    }

    #[test]
    fn define_tools_has_correct_names() {
        let tools = define_tools(None);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "shell_exec",
                "file_read",
                "file_write",
                "spawn_llm_session",
                "spawn_background_task",
                "agent_status",
                "agent_result",
                "kill_agent",
                "write_stdin",
            ]
        );
    }

    #[test]
    fn define_tools_filter_returns_subset() {
        let filter = vec!["shell_exec".to_string(), "file_read".to_string()];
        let tools = define_tools(Some(&filter));
        assert_eq!(tools.len(), 2);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["shell_exec", "file_read"]);
    }

    #[test]
    fn define_tools_filter_empty_returns_empty() {
        let filter: Vec<String> = vec![];
        let tools = define_tools(Some(&filter));
        assert_eq!(tools.len(), 0);
    }

    #[test]
    fn define_tools_all_have_descriptions() {
        let tools = define_tools(None);
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
        let tools = define_tools(None);
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
        // Core tools
        assert!(desc.contains("### shell_exec"));
        assert!(desc.contains("### file_read"));
        assert!(desc.contains("### file_write"));
        // Sub-agent tools (descriptions added in Task 2)
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
            soft_threshold_pct: 0.70,
            hard_threshold_pct: 0.90,
            carryover_turns: 5,
            max_restarts: None,
            auto_restart: true,
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
        let result = dispatch_tool_call(&call, &safety, &workspace, None, None).await;

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
        let result = dispatch_tool_call(&call, &safety, &workspace, None, None).await;

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
        let result = dispatch_tool_call(&call, &safety, &workspace, None, None).await;

        // file_read returns raw content, not JSON
        assert_eq!(result, "file contents here");
    }

    #[tokio::test]
    async fn dispatch_file_read_nonexistent_file() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        let call = make_tool_call("file_read", json!({"path": "no_such_file.txt"}));
        let result = dispatch_tool_call(&call, &safety, &workspace, None, None).await;

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
        let result = dispatch_tool_call(&call, &safety, &workspace, None, None).await;

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
        let result = dispatch_tool_call(&call, &safety, &workspace, None, None).await;

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
        let result = dispatch_tool_call(&call, &safety, &workspace, None, None).await;

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
        let result = dispatch_tool_call(&call, &safety, &workspace, None, None).await;

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
        let result = dispatch_tool_call(&call, &safety, &workspace, None, None).await;

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed["error"].as_str().unwrap().contains("content"));
    }

    #[tokio::test]
    async fn dispatch_unknown_tool() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        let call = make_tool_call("nonexistent_tool", json!({}));
        let result = dispatch_tool_call(&call, &safety, &workspace, None, None).await;

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed["error"]
            .as_str()
            .unwrap()
            .contains("Unknown tool: nonexistent_tool"));
    }

    #[tokio::test]
    async fn dispatch_sub_agent_tools_without_manager_returns_error() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        let sub_agent_tools = [
            "spawn_llm_session",
            "spawn_background_task",
            "agent_status",
            "agent_result",
            "kill_agent",
            "write_stdin",
        ];

        for tool_name in &sub_agent_tools {
            let call = make_tool_call(tool_name, json!({"goal": "test", "agent_id": "x", "data": "y", "command": "echo"}));
            let result = dispatch_tool_call(&call, &safety, &workspace, None, None).await;

            let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
            assert!(
                parsed["error"]
                    .as_str()
                    .unwrap()
                    .contains("not available"),
                "Tool '{}' should return 'not available' without manager, got: {}",
                tool_name,
                result
            );
        }
    }
}
