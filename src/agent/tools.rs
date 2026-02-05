//! Tool schema definitions and dispatch for the agent loop.
//!
//! Defines the thirteen agent tools (3 core + 6 sub-agent orchestration + 4
//! extended) as [`genai::chat::Tool`] schemas and provides a dispatch function
//! that routes tool calls to their implementations.
//!
//! Tool errors are always returned as structured JSON strings (never panics or
//! `Err` variants) so the model can observe the error and react.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use genai::chat::Tool;
use serde_json::json;
use tokio::sync::mpsc::UnboundedSender;

use crate::agent::discovery;
use crate::agent::sleep;
use crate::agent::web_fetch;
use crate::agent::web_search;
use crate::config::AppConfig;
use crate::orchestration::background_proc::spawn_background_process;
use crate::orchestration::llm_agent::spawn_llm_sub_agent;
use crate::orchestration::manager::SubAgentManager;
use crate::safety::SafetyLayer;
use crate::tui::event::AgentEvent;

/// Define the agent tool schemas.
///
/// Returns a `Vec<Tool>` suitable for passing to
/// [`genai::chat::ChatRequest::with_tools`].
///
/// When `filter` is `None`, returns all 13 tools (3 core + 6 orchestration +
/// 4 extended). When `filter` is `Some(names)`, returns only tools whose names
/// appear in the filter list. This enables sub-agents to have a customized
/// tool set.
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
/// 10. `web_fetch` -- Fetch a web page by URL
/// 11. `web_search` -- Search the internet
/// 12. `sleep` -- Pause the agent loop
/// 13. `flag_discovery` -- Flag a noteworthy finding
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
        // -- Extended tools (10-13) --
        Tool::new("web_fetch")
            .with_description(
                "Fetch a web page by URL and return its content as markdown, \
                 raw HTML, or JSON. JSON responses are returned as-is. HTML pages \
                 are converted to markdown by default.",
            )
            .with_schema(json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch"
                    },
                    "format": {
                        "type": "string",
                        "enum": ["markdown", "html"],
                        "description": "Output format for HTML pages (default: markdown)"
                    },
                    "max_length": {
                        "type": "integer",
                        "description": "Optional truncation limit in characters"
                    }
                },
                "required": ["url"]
            })),
        Tool::new("web_search")
            .with_description(
                "Search the internet and return a list of results with titles, \
                 URLs, and snippets. Uses DuckDuckGo by default. Set provider to \
                 'brave' to use Brave Search (requires API key in config).",
            )
            .with_schema(json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    },
                    "count": {
                        "type": "integer",
                        "description": "Maximum number of results to return (default: 5)"
                    },
                    "provider": {
                        "type": "string",
                        "enum": ["duckduckgo", "brave"],
                        "description": "Search provider (default: duckduckgo)"
                    }
                },
                "required": ["query"]
            })),
        Tool::new("sleep")
            .with_description(
                "Pause the agent loop. Three modes: 'timer' sleeps for a fixed \
                 duration, 'event' waits for a sub-agent to complete, 'manual' \
                 waits for user resume from TUI. No LLM calls occur during sleep.",
            )
            .with_schema(json!({
                "type": "object",
                "properties": {
                    "mode": {
                        "type": "string",
                        "enum": ["timer", "event", "manual"],
                        "description": "Sleep mode"
                    },
                    "duration_secs": {
                        "type": "integer",
                        "description": "Sleep duration in seconds (required for timer mode)"
                    },
                    "agent_id": {
                        "type": "string",
                        "description": "Sub-agent ID to wait for (required for event mode)"
                    }
                },
                "required": ["mode"]
            })),
        Tool::new("flag_discovery")
            .with_description(
                "Flag a noteworthy finding for the user. Discoveries persist to \
                 disk and appear in the TUI discoveries panel. Use for interesting \
                 patterns, unexpected results, useful resources, or any insight worth \
                 surfacing.",
            )
            .with_schema(json!({
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "Short title summarizing the discovery"
                    },
                    "description": {
                        "type": "string",
                        "description": "Detailed description with context"
                    }
                },
                "required": ["title", "description"]
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
- Writes outside the workspace directory are rejected

### spawn_llm_session
Spawn a child LLM chat session that runs concurrently.
- **goal** (string, required): What the sub-agent should accomplish
- **model** (string, optional): Ollama model name (defaults to your model)
- **context** (object, optional): Key-value context injected into the sub-agent's prompt
- **timeout_secs** (integer, optional): Maximum runtime in seconds
- **tools** (array of strings, optional): Tool names to enable (default: all your tools)
- Returns: JSON with agent_id and status fields
- The sub-agent runs independently and you can check its progress via agent_status

### spawn_background_task
Spawn a background shell process that runs independently.
- **command** (string, required): The shell command to run
- Returns: JSON with agent_id and status fields
- The process runs in the background; use agent_status to check progress
- Use write_stdin to send input to the process

### agent_status
Query the status of sub-agents and background processes.
- **agent_id** (string, optional): Specific agent ID to query (omit for all)
- Returns: JSON with agent info including status (running/completed/failed/killed)

### agent_result
Retrieve the structured result of a completed sub-agent.
- **agent_id** (string, required): The agent ID to retrieve results for
- Returns: JSON with summary, output, files_modified, elapsed_secs
- Returns error if the agent is still running

### kill_agent
Terminate a running sub-agent or background process.
- **agent_id** (string, required): The agent ID to terminate
- Returns: JSON with killed status

### write_stdin
Write data to a running background process's stdin.
- **agent_id** (string, required): The background process ID
- **data** (string, required): Data to write (newline appended automatically)
- Returns: JSON with written_bytes

### web_fetch
Fetch a web page by URL and return its content.
- **url** (string, required): The URL to fetch
- **format** (string, optional): Output format for HTML pages: 'markdown' (default) or 'html'
- **max_length** (integer, optional): Truncation limit in characters (for large pages)
- Returns: The page content as text (markdown, HTML, or JSON depending on content type)
- JSON responses are returned as-is regardless of format parameter
- Uses HTTP GET with 30-second timeout and redirect following

### web_search
Search the internet and return structured results.
- **query** (string, required): The search query
- **count** (integer, optional): Maximum results to return (default: 5)
- **provider** (string, optional): 'duckduckgo' (default, always available) or 'brave' (requires API key in config)
- Returns: JSON array of objects with title, url, snippet fields
- Rate-limited to avoid being blocked by search providers

### sleep
Pause the agent loop with configurable resume.
- **mode** (string, required): 'timer', 'event', or 'manual'
- **duration_secs** (integer, required for timer): How long to sleep in seconds
- **agent_id** (string, required for event): Sub-agent ID to wait for completion
- Returns: JSON confirmation with sleep_requested, mode, and duration fields
- Timer: auto-wake after duration. Event: wake when agent completes/fails. Manual: wake on user resume from TUI
- No LLM calls occur during sleep; turn counter does not increment

### flag_discovery
Flag a noteworthy finding for the user.
- **title** (string, required): Short title summarizing the discovery
- **description** (string, required): Detailed description with context
- Returns: JSON confirmation with flagged status
- Discoveries persist to disk (survive session restarts) and appear in the TUI discoveries panel
- Use for interesting patterns, unexpected results, useful resources, or insights worth surfacing"
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
/// - `web_fetch` -> [`web_fetch::fetch_url`]
/// - `web_search` -> [`web_search::rate_limited_ddg_search`] / [`web_search::rate_limited_brave_search`]
/// - `sleep` -> [`sleep::parse_sleep_args`]
/// - `flag_discovery` -> [`discovery::append_discovery`]
///
/// The `manager` and `config` parameters are `Option` so that existing callers
/// (tests, sub-agents without manager access) still work. When `None` and a
/// sub-agent tool is called, returns `{"error": "Sub-agent tools not available"}`.
///
/// The `event_tx` parameter is `Option` for TUI event emission. When `Some`,
/// discovery events are broadcast to the TUI.
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
    event_tx: Option<&UnboundedSender<AgentEvent>>,
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
        "web_fetch" => dispatch_web_fetch(call).await,
        "web_search" => dispatch_web_search(call, config).await,
        "sleep" => dispatch_sleep(call, config),
        "flag_discovery" => dispatch_flag_discovery(call, workspace, event_tx),
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
    if let Some(parent) = full_path.parent()
        && let Err(e) = tokio::fs::create_dir_all(parent).await
    {
        return json!({"error": format!("file_write: failed to create directories: {}", e)})
            .to_string();
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

/// Parsed arguments for spawn_llm_session: (goal, model, context, timeout, tool_filter).
type SpawnLlmArgs = (
    String,
    Option<String>,
    HashMap<String, String>,
    Option<Duration>,
    Option<Vec<String>>,
);

/// Extract spawn_llm_session arguments from a tool call.
///
/// Returns `(goal, model, context, timeout, tool_filter)` or an error string.
fn extract_spawn_llm_args(
    call: &genai::chat::ToolCall,
) -> Result<SpawnLlmArgs, String> {
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

// ---------------------------------------------------------------------------
// Extended tool dispatch helpers (web_fetch, web_search, sleep, flag_discovery)
// ---------------------------------------------------------------------------

/// Fetch a web page by URL.
///
/// Extracts `url`, `format` (default `"markdown"`), and optional `max_length`
/// from the tool call arguments, then delegates to [`web_fetch::fetch_url`].
async fn dispatch_web_fetch(call: &genai::chat::ToolCall) -> String {
    let url = match call.fn_arguments.get("url").and_then(|v| v.as_str()) {
        Some(u) => u,
        None => {
            return json!({"error": "web_fetch: missing or invalid 'url' argument"}).to_string();
        }
    };

    let format = call
        .fn_arguments
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("markdown");

    let max_length = call
        .fn_arguments
        .get("max_length")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);

    web_fetch::fetch_url(url, format, max_length).await
}

/// Search the internet via DuckDuckGo or Brave.
///
/// Uses the `provider` argument to select the backend. DuckDuckGo is the
/// default (zero-config). Brave requires an API key in the config.
///
/// Rate limits are enforced per-provider using config values.
async fn dispatch_web_search(
    call: &genai::chat::ToolCall,
    config: Option<&AppConfig>,
) -> String {
    let query = match call.fn_arguments.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => {
            return json!({"error": "web_search: missing or invalid 'query' argument"}).to_string();
        }
    };

    let count = call
        .fn_arguments
        .get("count")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(5);

    let provider = call
        .fn_arguments
        .get("provider")
        .and_then(|v| v.as_str())
        .unwrap_or("duckduckgo");

    // Get rate limit settings from config (use defaults if no config available).
    let ddg_rate = config.map(|c| c.ddg_rate_limit_secs).unwrap_or(2.0);
    let brave_rate = config.map(|c| c.brave_rate_limit_secs).unwrap_or(1.0);
    let brave_key = config.and_then(|c| c.brave_api_key.as_deref());

    match provider {
        "brave" => {
            match brave_key {
                Some(key) => {
                    web_search::rate_limited_brave_search(query, count, key, brave_rate).await
                }
                None => {
                    json!({"error": "web_search: Brave Search requires 'brave_api_key' in config. Use provider 'duckduckgo' (default) or set the key."}).to_string()
                }
            }
        }
        _ => {
            web_search::rate_limited_ddg_search(query, count, ddg_rate).await
        }
    }
}

/// Parse sleep arguments and return a confirmation JSON.
///
/// The sleep tool does not block here -- it returns immediately with a
/// `sleep_requested` JSON payload. The agent loop's between-turn check
/// reads this result to enter the sleep state machine.
fn dispatch_sleep(
    call: &genai::chat::ToolCall,
    config: Option<&AppConfig>,
) -> String {
    let max_sleep = config.map(|c| c.max_sleep_duration_secs).unwrap_or(3600);

    match sleep::parse_sleep_args(&call.fn_arguments, max_sleep) {
        Ok(state) => {
            let mode_str = match &state.mode {
                sleep::SleepMode::Timer(d) => format!("timer ({}s)", d.as_secs()),
                sleep::SleepMode::Event { agent_id } => format!("event (agent: {agent_id})"),
                sleep::SleepMode::Manual => "manual".to_string(),
            };
            json!({
                "sleep_requested": true,
                "mode": mode_str,
                "max_duration_secs": state.max_duration.as_secs(),
            })
            .to_string()
        }
        Err(e) => json!({"error": e}).to_string(),
    }
}

/// Flag a discovery and persist it to disk.
///
/// Creates a [`discovery::Discovery`] with the current timestamp, appends it
/// to the workspace JSONL file, and emits an [`AgentEvent::Discovery`] for
/// the TUI if `event_tx` is available.
fn dispatch_flag_discovery(
    call: &genai::chat::ToolCall,
    workspace: &Path,
    event_tx: Option<&UnboundedSender<AgentEvent>>,
) -> String {
    let title = match call.fn_arguments.get("title").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => {
            return json!({"error": "flag_discovery: missing or invalid 'title' argument"})
                .to_string();
        }
    };

    let description = match call
        .fn_arguments
        .get("description")
        .and_then(|v| v.as_str())
    {
        Some(d) => d.to_string(),
        None => {
            return json!({"error": "flag_discovery: missing or invalid 'description' argument"})
                .to_string();
        }
    };

    let timestamp = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();

    let disc = discovery::Discovery {
        timestamp: timestamp.clone(),
        title: title.clone(),
        description: description.clone(),
    };

    if let Err(e) = discovery::append_discovery(workspace, &disc) {
        return json!({"error": format!("flag_discovery: {e}")}).to_string();
    }

    // Emit discovery event for TUI.
    if let Some(tx) = event_tx {
        let _ = tx.send(AgentEvent::Discovery {
            timestamp: timestamp.clone(),
            title: title.to_string(),
            description: description.to_string(),
        });
    }

    json!({
        "flagged": true,
        "title": title,
        "timestamp": timestamp,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use genai::chat::ToolCall;
    use tempfile::TempDir;

    #[test]
    fn define_tools_returns_thirteen_tools() {
        let tools = define_tools(None);
        assert_eq!(tools.len(), 13);
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
                "web_fetch",
                "web_search",
                "sleep",
                "flag_discovery",
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
        // Sub-agent orchestration tools
        assert!(desc.contains("### spawn_llm_session"));
        assert!(desc.contains("### spawn_background_task"));
        assert!(desc.contains("### agent_status"));
        assert!(desc.contains("### agent_result"));
        assert!(desc.contains("### kill_agent"));
        assert!(desc.contains("### write_stdin"));
        // Extended tools
        assert!(desc.contains("### web_fetch"));
        assert!(desc.contains("### web_search"));
        assert!(desc.contains("### sleep"));
        assert!(desc.contains("### flag_discovery"));
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
            ddg_rate_limit_secs: 2.0,
            brave_api_key: None,
            brave_rate_limit_secs: 1.0,
            max_sleep_duration_secs: 3600,
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
        let result = dispatch_tool_call(&call, &safety, &workspace, None, None, None).await;

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
        let result = dispatch_tool_call(&call, &safety, &workspace, None, None, None).await;

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
        let result = dispatch_tool_call(&call, &safety, &workspace, None, None, None).await;

        // file_read returns raw content, not JSON
        assert_eq!(result, "file contents here");
    }

    #[tokio::test]
    async fn dispatch_file_read_nonexistent_file() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        let call = make_tool_call("file_read", json!({"path": "no_such_file.txt"}));
        let result = dispatch_tool_call(&call, &safety, &workspace, None, None, None).await;

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
        let result = dispatch_tool_call(&call, &safety, &workspace, None, None, None).await;

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
        let result = dispatch_tool_call(&call, &safety, &workspace, None, None, None).await;

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
        let result = dispatch_tool_call(&call, &safety, &workspace, None, None, None).await;

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
        let result = dispatch_tool_call(&call, &safety, &workspace, None, None, None).await;

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
        let result = dispatch_tool_call(&call, &safety, &workspace, None, None, None).await;

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed["error"].as_str().unwrap().contains("content"));
    }

    #[tokio::test]
    async fn dispatch_unknown_tool() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        let call = make_tool_call("nonexistent_tool", json!({}));
        let result = dispatch_tool_call(&call, &safety, &workspace, None, None, None).await;

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
            let result = dispatch_tool_call(&call, &safety, &workspace, None, None, None).await;

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

    // -- Extended tool dispatch tests --

    #[tokio::test]
    async fn dispatch_web_fetch_missing_url() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        let call = make_tool_call("web_fetch", json!({}));
        let result = dispatch_tool_call(&call, &safety, &workspace, None, None, None).await;

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed["error"].as_str().unwrap().contains("web_fetch"));
        assert!(parsed["error"].as_str().unwrap().contains("url"));
    }

    #[tokio::test]
    async fn dispatch_web_search_missing_query() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        let call = make_tool_call("web_search", json!({}));
        let result = dispatch_tool_call(&call, &safety, &workspace, None, None, None).await;

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed["error"].as_str().unwrap().contains("web_search"));
        assert!(parsed["error"].as_str().unwrap().contains("query"));
    }

    #[tokio::test]
    async fn dispatch_web_search_brave_without_key() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        // Config without brave key
        let config = AppConfig {
            model: "test-model".to_string(),
            workspace: workspace.clone(),
            shell_timeout_secs: 10,
            context_limit: 8192,
            blocked_patterns: vec![],
            security_log_path: tmp.path().join("security.log"),
            soft_threshold_pct: 0.70,
            hard_threshold_pct: 0.90,
            carryover_turns: 5,
            max_restarts: None,
            auto_restart: true,
            ddg_rate_limit_secs: 2.0,
            brave_api_key: None,
            brave_rate_limit_secs: 1.0,
            max_sleep_duration_secs: 3600,
        };

        let call = make_tool_call(
            "web_search",
            json!({"query": "test", "provider": "brave"}),
        );
        let result =
            dispatch_tool_call(&call, &safety, &workspace, None, Some(&config), None).await;

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed["error"].as_str().unwrap().contains("brave_api_key"));
    }

    #[tokio::test]
    async fn dispatch_sleep_valid_timer() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        let config = AppConfig {
            model: "test-model".to_string(),
            workspace: workspace.clone(),
            shell_timeout_secs: 10,
            context_limit: 8192,
            blocked_patterns: vec![],
            security_log_path: tmp.path().join("security.log"),
            soft_threshold_pct: 0.70,
            hard_threshold_pct: 0.90,
            carryover_turns: 5,
            max_restarts: None,
            auto_restart: true,
            ddg_rate_limit_secs: 2.0,
            brave_api_key: None,
            brave_rate_limit_secs: 1.0,
            max_sleep_duration_secs: 3600,
        };

        let call = make_tool_call(
            "sleep",
            json!({"mode": "timer", "duration_secs": 30}),
        );
        let result =
            dispatch_tool_call(&call, &safety, &workspace, None, Some(&config), None).await;

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["sleep_requested"], true);
        assert!(parsed["mode"].as_str().unwrap().contains("timer"));
    }

    #[tokio::test]
    async fn dispatch_sleep_invalid_mode() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        let call = make_tool_call("sleep", json!({"mode": "hibernate"}));
        let result =
            dispatch_tool_call(&call, &safety, &workspace, None, None, None).await;

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed["error"].as_str().unwrap().contains("unknown mode"));
    }

    #[tokio::test]
    async fn dispatch_flag_discovery_persists_and_returns_confirmation() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        let call = make_tool_call(
            "flag_discovery",
            json!({"title": "Found Makefile", "description": "Project has build targets"}),
        );
        let result =
            dispatch_tool_call(&call, &safety, &workspace, None, None, None).await;

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["flagged"], true);
        assert_eq!(parsed["title"], "Found Makefile");
        assert!(parsed["timestamp"].as_str().is_some());

        // Verify discovery was persisted to disk
        let discoveries = discovery::load_discoveries(&workspace);
        assert_eq!(discoveries.len(), 1);
        assert_eq!(discoveries[0].title, "Found Makefile");
        assert_eq!(discoveries[0].description, "Project has build targets");
    }

    #[tokio::test]
    async fn dispatch_flag_discovery_emits_event() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();

        let call = make_tool_call(
            "flag_discovery",
            json!({"title": "Test Discovery", "description": "Testing event emission"}),
        );
        let result =
            dispatch_tool_call(&call, &safety, &workspace, None, None, Some(&tx)).await;

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["flagged"], true);

        // Check that an event was emitted
        let event = rx.try_recv().expect("should receive discovery event");
        match event {
            AgentEvent::Discovery { title, description, .. } => {
                assert_eq!(title, "Test Discovery");
                assert_eq!(description, "Testing event emission");
            }
            other => panic!("Expected Discovery event, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_flag_discovery_missing_title() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        let call = make_tool_call(
            "flag_discovery",
            json!({"description": "Missing title"}),
        );
        let result =
            dispatch_tool_call(&call, &safety, &workspace, None, None, None).await;

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed["error"].as_str().unwrap().contains("title"));
    }

    #[tokio::test]
    async fn dispatch_flag_discovery_missing_description() {
        let tmp = TempDir::new().unwrap();
        let safety = make_safety(&tmp);
        let workspace = tmp.path().join("workspace");

        let call = make_tool_call(
            "flag_discovery",
            json!({"title": "No description"}),
        );
        let result =
            dispatch_tool_call(&call, &safety, &workspace, None, None, None).await;

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed["error"].as_str().unwrap().contains("description"));
    }
}
