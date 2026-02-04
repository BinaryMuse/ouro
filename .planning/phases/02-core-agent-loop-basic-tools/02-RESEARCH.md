# Phase 02: Core Agent Loop & Basic Tools - Research

**Researched:** 2026-02-04
**Domain:** LLM agent loop with tool calling via Ollama, structured logging, signal handling
**Confidence:** HIGH

## Summary

This phase implements the core conversation loop that drives the autonomous agent: connecting to a local Ollama model via the `genai` crate, sustaining multi-turn conversations with tool calling (shell execution, file read, file write), streaming model output to stdout, and logging all actions to JSONL files.

The `genai` crate v0.5.3 (latest on crates.io) supports tool calling via `Tool`, `ToolCall`, and `ToolResponse` types, along with streaming via `exec_chat_stream`. Ollama models that support tools (e.g., llama3.1, qwen2.5, qwen3) work through genai's native tool calling protocol. The conversation is managed via `ChatRequest` which accumulates a `Vec<ChatMessage>` history. For Ollama, the genai crate acts as the default adapter -- any model name not matching a known provider prefix (gpt, claude, gemini, etc.) routes to Ollama at `localhost:11434`.

**Primary recommendation:** Use `genai` v0.5.3 with its native tool calling and streaming APIs. Build the agent loop as an async function that accumulates `ChatRequest` messages, dispatches tool calls through the existing `SafetyLayer`, and streams text to stdout. Use JSONL with `serde_json` for session logging. Use `tokio::signal::ctrl_c()` with an `AtomicBool` flag for two-phase Ctrl+C shutdown.

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| genai | 0.5.3 | Multi-provider LLM client with Ollama support | Native tool calling, streaming, multi-turn conversation. Already uses tokio/reqwest internally. Ollama is the default adapter. |
| serde_json | 1.0 (already in Cargo.toml) | JSON serialization for JSONL logging, tool schemas, tool arguments | Already a dependency, standard Rust JSON library |
| tokio | 1.x (already in Cargo.toml) | Async runtime for agent loop, signal handling, streaming | Already a dependency, add "signal" feature |
| chrono | 0.4 | Timestamp formatting for session log filenames | Standard Rust datetime library; needed for ISO 8601 session-YYYY-MM-DDTHH-MM.jsonl filenames |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| futures | 0.3 | `StreamExt` trait for iterating over `ChatStream` | Required to call `.next()` on the genai `ChatStream` type |
| reqwest | 0.12+ | HTTP client for Ollama health check | genai already depends on reqwest; may reuse or make direct calls for /api/tags and /api/show |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| genai | ollama-rs 0.3.3 | ollama-rs has mature tool calling with `#[function]` macro and `Coordinator` pattern, but is Ollama-only. genai provides multi-provider abstraction and is already the project's chosen library per context decisions. |
| genai | ollama-sdk | Newer crate with idiomatic Rust API and tool calling, but less battle-tested than genai. |
| chrono | time-format | Lighter weight but less ecosystem support. chrono is the standard choice for Rust datetime. |
| chrono | manual SystemTime math | Zero dependencies but error-prone calendar math. Not worth the complexity. |

**Installation (additions to Cargo.toml):**
```toml
# LLM client (Ollama + multi-provider)
genai = "0.5"

# Stream iteration
futures = "0.3"

# Timestamps for log filenames
chrono = "0.4"
```

**Tokio feature addition:**
```toml
tokio = { version = "1", features = ["process", "time", "fs", "rt-multi-thread", "macros", "io-util", "signal"] }
```

## Architecture Patterns

### Recommended Module Structure
```
src/
  agent/
    mod.rs            # pub mod loop, tools, logging, system_prompt
    loop.rs           # Core agent conversation loop
    tools.rs          # Tool definitions (shell, file_read, file_write) and dispatch
    logging.rs        # JSONL session logger
    system_prompt.rs  # SYSTEM_PROMPT.md loading and wrapping
  config/             # (existing) Config loading
  exec/               # (existing) Shell execution
  safety/             # (existing) SafetyLayer, WorkspaceGuard, CommandFilter
  cli.rs              # (existing) CLI definitions
  error.rs            # (existing) Error types
  lib.rs              # (existing) Module re-exports
  main.rs             # (existing) Entry point
```

### Pattern 1: Accumulating Conversation Loop
**What:** The agent loop maintains a `ChatRequest` that accumulates all messages (system, assistant, user/tool responses). Each iteration either streams the model response or processes tool calls, then appends results to the conversation history.
**When to use:** Always -- this is the core loop pattern.
**Example:**
```rust
// Source: genai docs.rs + examples/c01-conv.rs, c08-tooluse.rs
use genai::chat::{ChatMessage, ChatRequest, ChatOptions, Tool, ToolCall, ToolResponse};
use genai::Client;
use futures::StreamExt;

async fn run_agent_loop(
    client: &Client,
    model: &str,
    system_prompt: &str,
    tools: Vec<Tool>,
    safety: &SafetyLayer,
    logger: &mut SessionLogger,
) -> Result<()> {
    let mut chat_req = ChatRequest::from_system(system_prompt)
        .with_tools(tools);

    let chat_options = ChatOptions::default()
        .with_capture_content(true)
        .with_capture_tool_calls(true);

    loop {
        // Stream the model response
        let stream_res = client
            .exec_chat_stream(model, chat_req.clone(), Some(&chat_options))
            .await?;

        let mut stream = stream_res.stream;
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        while let Some(event) = stream.next().await {
            match event? {
                ChatStreamEvent::Chunk(chunk) => {
                    // Print text to stdout in real time
                    print!("{}", chunk.content);
                }
                ChatStreamEvent::ToolCallChunk(_) => {
                    // Tool call chunks are accumulated internally by genai
                    // when capture_tool_calls is enabled
                }
                ChatStreamEvent::End(end) => {
                    // Extract captured tool calls
                    if let Some(calls) = end.captured_into_tool_calls() {
                        tool_calls = calls;
                    }
                    // Extract captured text content
                    if let Some(text) = end.captured_first_text() {
                        // Log the assistant text response
                        logger.log_assistant_text(&text)?;
                    }
                }
                _ => {} // Start, ReasoningChunk, ThoughtSignatureChunk
            }
        }

        if tool_calls.is_empty() {
            // Model responded with text only (thinking out loud)
            // Append assistant message and prompt again
            chat_req = chat_req.append_message(
                ChatMessage::assistant(/* captured text */)
            );
        } else {
            // Process each tool call
            for call in &tool_calls {
                let result = dispatch_tool(call, safety).await?;
                logger.log_tool_call(call, &result)?;

                // Append assistant tool call + tool response
                chat_req = chat_req
                    .append_message(ChatMessage::assistant(/* tool call message */))
                    .append_message(ToolResponse::new(
                        call.call_id.clone(),
                        result,
                    ));
            }
        }
    }
}
```

### Pattern 2: Tool Dispatch Table
**What:** A central dispatch function that maps tool call function names to their implementations. Each tool receives structured JSON arguments and returns a string result.
**When to use:** For routing tool calls from the model to actual implementations.
**Example:**
```rust
async fn dispatch_tool(
    call: &ToolCall,
    safety: &SafetyLayer,
    workspace: &Path,
) -> Result<String> {
    match call.fn_name.as_str() {
        "shell_exec" => {
            let command = call.fn_arguments["command"]
                .as_str()
                .ok_or_else(|| anyhow!("shell_exec: missing 'command' argument"))?;
            let result = safety.execute(command).await?;
            Ok(serde_json::to_string(&result)?)
        }
        "file_read" => {
            let path_str = call.fn_arguments["path"]
                .as_str()
                .ok_or_else(|| anyhow!("file_read: missing 'path' argument"))?;
            let full_path = workspace.join(path_str);
            // Reads are unrestricted per Phase 1 decision
            let content = tokio::fs::read_to_string(&full_path).await
                .map_err(|e| anyhow!("file_read error: {}", e))?;
            Ok(content)
        }
        "file_write" => {
            let path_str = call.fn_arguments["path"]
                .as_str()
                .ok_or_else(|| anyhow!("file_write: missing 'path' argument"))?;
            let content = call.fn_arguments["content"]
                .as_str()
                .ok_or_else(|| anyhow!("file_write: missing 'content' argument"))?;
            let full_path = workspace.join(path_str);
            // Validate write is within workspace using WorkspaceGuard
            // (accessed via SafetyLayer or directly)
            tokio::fs::write(&full_path, content).await
                .map_err(|e| anyhow!("file_write error: {}", e))?;
            Ok(format!("Written {} bytes to {}", content.len(), path_str))
        }
        unknown => {
            Ok(format!("Unknown tool: {}", unknown))
        }
    }
}
```

### Pattern 3: JSONL Session Logger
**What:** An append-only JSONL logger that writes one JSON object per line for every event in the session. Uses `BufWriter` for efficiency and flushes after each write for durability.
**When to use:** For all session logging (tool calls, model responses, errors, system messages).
**Example:**
```rust
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};

struct SessionLogger {
    writer: BufWriter<std::fs::File>,
}

impl SessionLogger {
    fn new(log_dir: &Path, session_id: &str) -> Result<Self> {
        std::fs::create_dir_all(log_dir)?;
        let filename = format!("session-{}.jsonl", session_id);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_dir.join(filename))?;
        Ok(Self { writer: BufWriter::new(file) })
    }

    fn log_event(&mut self, event: &LogEvent) -> Result<()> {
        serde_json::to_writer(&mut self.writer, event)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        Ok(())
    }
}

#[derive(serde::Serialize)]
struct LogEvent {
    timestamp: String,
    turn: u64,
    event_type: String,
    #[serde(flatten)]
    data: serde_json::Value,
}
```

### Pattern 4: Two-Phase Ctrl+C Shutdown
**What:** First Ctrl+C sets a flag that the loop checks between turns (graceful). Second Ctrl+C force-exits via `std::process::exit(1)`.
**When to use:** Always -- required by the context decisions.
**Example:**
```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

let shutdown = Arc::new(AtomicBool::new(false));
let shutdown_clone = shutdown.clone();

tokio::spawn(async move {
    tokio::signal::ctrl_c().await.ok();
    // First Ctrl+C: set graceful shutdown flag
    shutdown_clone.store(true, Ordering::SeqCst);
    eprintln!("\nShutting down after current turn... (Ctrl+C again to force quit)");

    tokio::signal::ctrl_c().await.ok();
    // Second Ctrl+C: force exit
    eprintln!("\nForce quitting.");
    std::process::exit(1);
});

// In the agent loop:
loop {
    if shutdown.load(Ordering::SeqCst) {
        break; // Graceful shutdown between turns
    }
    // ... process next turn ...
}
```

### Anti-Patterns to Avoid
- **Building a custom HTTP client for Ollama:** Use genai, which handles the Ollama API protocol internally. Do not call Ollama REST endpoints directly for chat -- only for health checks and model info.
- **Mutable shared state for conversation history:** The `ChatRequest` should be owned by the loop, not shared across threads. Clone it when passing to `exec_chat_stream`.
- **Blocking I/O in the async loop:** Use `tokio::fs` for file operations, not `std::fs`, within the async agent loop. The JSONL logger can use synchronous `std::fs` since writes are small and buffered.
- **Silently dropping tool call errors:** Always return tool errors as structured results to the model so it can react and try alternatives.
- **Truncating tool results prematurely:** Per context decisions, return tool results in full. Context management (Phase 3) will handle pressure later.

## Don't Hand-Roll

Problems that look simple but have existing solutions:

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| LLM API protocol | Custom HTTP/JSON for Ollama chat API | genai crate | Handles streaming, tool calling protocol, message formatting, provider abstraction |
| Tool schema generation | Manual JSON schema strings | `serde_json::json!()` macro | Easier to maintain, less error-prone than hand-written JSON strings |
| Timestamp formatting | Manual calendar math from SystemTime | chrono crate | Leap years, timezone handling, ISO 8601 formatting are non-trivial |
| Stream iteration | Manual poll loop | futures `StreamExt` | Provides `.next().await` pattern that works naturally with genai's `ChatStream` |
| Signal handling | Raw libc signal handlers | `tokio::signal::ctrl_c()` | Cross-platform, async-safe, integrates with tokio runtime |
| JSON line serialization | Manual string formatting | `serde_json::to_writer()` + newline | Handles escaping, nested objects, special characters correctly |

**Key insight:** The genai crate handles the complex parts (Ollama API protocol, streaming SSE parsing, tool call serialization/deserialization). Focus implementation effort on the loop logic, tool dispatch, and logging -- not on the LLM communication layer.

## Common Pitfalls

### Pitfall 1: Ollama Silent Context Truncation
**What goes wrong:** Ollama's default `num_ctx` is 2048 tokens. When the conversation exceeds this, Ollama silently truncates from the start -- losing the system prompt and early context without any error.
**Why it happens:** Ollama does not return an error when context is truncated. The response just degrades.
**How to avoid:** At startup, query `/api/show` for the model to get its `model_info.<family>.context_length` (max supported) and the configured `num_ctx`. Track token usage from `ChatResponse.usage` (or `StreamEnd.captured_usage`) to estimate when the context is filling up. Note: Ollama does not reliably emit token counts when streaming via the OpenAI compatibility layer that genai uses, so non-streaming `exec_chat` calls may be more reliable for usage tracking, or track message count/size heuristically.
**Warning signs:** Model responses become incoherent, lose track of goals, or repeat earlier patterns.

### Pitfall 2: Tool Call Message Ordering
**What goes wrong:** The conversation history must maintain strict ordering: user message -> assistant message (with tool calls) -> tool response messages (one per call, matched by call_id). If this ordering is broken, the model or the API rejects the conversation.
**Why it happens:** When processing multiple parallel tool calls, it's easy to append messages out of order or miss the assistant's tool-call message before the tool responses.
**How to avoid:** After streaming completes and tool calls are captured, first append the full assistant message (including tool calls), then append each `ToolResponse` in order. Use `ChatRequest::append_tool_use_from_stream_end()` which genai provides specifically for this purpose.
**Warning signs:** API errors about message ordering, or model confusion about tool results.

### Pitfall 3: genai Ollama Streaming Token Count Limitation
**What goes wrong:** Ollama does not emit `prompt_eval_count` / `eval_count` (input/output token counts) when streaming through the OpenAI compatibility layer that genai uses.
**Why it happens:** This is a known Ollama limitation documented in the genai README.
**How to avoid:** For context-full detection, use a heuristic approach: estimate token count from message content length (rough approximation: 1 token per 4 chars for English text), or periodically make a non-streaming `exec_chat` call to get accurate usage. Alternatively, make a direct Ollama API call to `/api/show` at startup to get `num_ctx` and track cumulative message sizes.
**Warning signs:** `Usage` fields showing 0 or None for token counts during streaming.

### Pitfall 4: Forgetting to Re-prompt on Text-Only Responses
**What goes wrong:** When the model responds with text but no tool calls (thinking out loud), the harness must add the text as an assistant message and send the conversation back so the model can take its next action. Without this, the loop stalls.
**Why it happens:** It's natural to expect every model response to contain a tool call in an agentic loop, but models often "think out loud" first.
**How to avoid:** After each model response, check `tool_calls()`. If empty, append the text as `ChatMessage::assistant(text)` and immediately loop back to `exec_chat_stream`. The context decisions explicitly state: "treat it as thinking out loud -- add the text to the conversation and prompt again."
**Warning signs:** Agent loop hangs after model produces text without tool calls.

### Pitfall 5: Ctrl+C During Streaming Leaves Partial State
**What goes wrong:** If Ctrl+C fires while streaming a model response, the stream may be partially consumed. The conversation history could be in an inconsistent state (e.g., tool calls received but not dispatched).
**Why it happens:** The graceful shutdown flag is only checked between turns, but `tokio::select!` can interrupt mid-stream.
**How to avoid:** Check the shutdown flag after each turn completes (after all tool calls are dispatched and logged). If a turn is interrupted mid-stream, either discard the partial turn entirely or log it as incomplete. The logger should always flush after each event so partial sessions are recoverable.
**Warning signs:** Corrupted log files, inconsistent conversation state on resume.

### Pitfall 6: System Prompt Not Wrapping User Content Properly
**What goes wrong:** The system prompt must include both the user's SYSTEM_PROMPT.md content AND harness-injected context (available tools, workspace path, constraints). If these are concatenated poorly, the model may ignore tools or misunderstand its environment.
**Why it happens:** Mixing user-authored and harness-generated system prompt content requires careful formatting.
**How to avoid:** Build the system prompt in layers: (1) harness preamble with tool descriptions and constraints, (2) separator, (3) user's SYSTEM_PROMPT.md content. Use `ChatRequest::with_system()` to set the combined prompt.
**Warning signs:** Model doesn't use tools, asks user for information it should know, or ignores workspace constraints.

## Code Examples

### Connecting to Ollama via genai
```rust
// Source: docs.rs/genai/0.5.3 + examples/c00-readme.rs
use genai::Client;

// Default client -- Ollama is the fallback adapter for unknown model names
let client = Client::default();

// Model names like "llama3.2", "qwen2.5:7b" auto-route to Ollama
// (anything not matching gpt*, claude*, gemini*, etc.)
let model = "qwen2.5:7b";
```

### Defining Tool Schemas
```rust
// Source: docs.rs/genai/0.5.3/genai/chat/struct.Tool.html
use genai::chat::Tool;
use serde_json::json;

fn define_tools() -> Vec<Tool> {
    vec![
        Tool::new("shell_exec")
            .with_description(
                "Execute a shell command in the workspace directory. \
                 Returns stdout, stderr, exit_code, and timed_out fields."
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
                "Read the contents of a file. Path is relative to the workspace root. \
                 Can read any file on the filesystem."
            )
            .with_schema(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path (relative to workspace, or absolute)"
                    }
                },
                "required": ["path"]
            })),
        Tool::new("file_write")
            .with_description(
                "Write content to a file. Path is relative to the workspace root. \
                 Can only write to files within the workspace directory."
            )
            .with_schema(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to workspace root"
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
```

### Streaming Model Output to stdout
```rust
// Source: docs.rs/genai/0.5.3/genai/chat/enum.ChatStreamEvent.html
use genai::chat::{ChatStreamEvent, ChatOptions};
use futures::StreamExt;

let opts = ChatOptions::default()
    .with_capture_content(true)
    .with_capture_tool_calls(true);

let stream_res = client
    .exec_chat_stream(model, chat_req.clone(), Some(&opts))
    .await?;

let mut stream = stream_res.stream;
while let Some(event) = stream.next().await {
    match event? {
        ChatStreamEvent::Chunk(chunk) => {
            print!("{}", chunk.content);
            std::io::Write::flush(&mut std::io::stdout())?;
        }
        ChatStreamEvent::End(end) => {
            println!(); // newline after streaming
            // Access captured content and tool calls
            let tool_calls = end.captured_into_tool_calls()
                .unwrap_or_default();
            let text = end.captured_first_text();
            break;
        }
        _ => {}
    }
}
```

### Ollama Health Check at Startup
```rust
// Source: Ollama API docs (https://docs.ollama.com/api-reference)
// Direct HTTP call since genai doesn't expose health check API

async fn check_ollama_ready(model: &str) -> Result<()> {
    let client = reqwest::Client::new();

    // Step 1: Check Ollama is running
    let resp = client.get("http://localhost:11434/")
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| anyhow!(
            "Cannot connect to Ollama at localhost:11434. \
             Is Ollama running? Error: {}", e
        ))?;

    if !resp.status().is_success() {
        anyhow::bail!("Ollama returned status {}", resp.status());
    }

    // Step 2: Check model is available
    let show_resp = client.post("http://localhost:11434/api/show")
        .json(&serde_json::json!({ "model": model }))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| anyhow!("Failed to query model info: {}", e))?;

    if !show_resp.status().is_success() {
        anyhow::bail!(
            "Model '{}' not found in Ollama. \
             Run 'ollama pull {}' to download it.", model, model
        );
    }

    Ok(())
}
```

### JSONL Log Event Types
```rust
use serde::Serialize;
use chrono::Utc;

#[derive(Serialize)]
#[serde(tag = "event_type")]
enum LogEntry {
    #[serde(rename = "session_start")]
    SessionStart {
        timestamp: String,
        model: String,
        workspace: String,
    },
    #[serde(rename = "assistant_text")]
    AssistantText {
        timestamp: String,
        turn: u64,
        content: String,
    },
    #[serde(rename = "tool_call")]
    ToolCallEntry {
        timestamp: String,
        turn: u64,
        call_id: String,
        fn_name: String,
        fn_arguments: serde_json::Value,
    },
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
    #[serde(rename = "system_message")]
    SystemMessage {
        timestamp: String,
        content: String,
    },
    #[serde(rename = "error")]
    Error {
        timestamp: String,
        turn: u64,
        message: String,
    },
    #[serde(rename = "session_end")]
    SessionEnd {
        timestamp: String,
        total_turns: u64,
        reason: String,
    },
}

fn now_iso() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Manual Ollama REST API calls | genai crate with native Ollama adapter | genai 0.5.x (2026) | Unified API across providers; tool calling and streaming handled internally |
| Custom SSE parsing for streaming | genai `ChatStream` with `StreamExt` | genai 0.5.x | Reliable streaming with event types (Chunk, ToolCallChunk, End) |
| No tool calling in genai | Full tool calling support (Tool, ToolCall, ToolResponse) | genai 0.5.x | Can define tools with JSON schemas, receive structured tool calls, send results back |

**Deprecated/outdated:**
- genai 0.3.x: Latest search results may reference 0.3.5 but 0.5.3 is the actual latest on docs.rs/crates.io. The 0.3.x line did NOT have tool calling. Ensure Cargo.toml specifies `genai = "0.5"`.

## Open Questions

1. **Ollama token count reliability during streaming**
   - What we know: genai README states Ollama does not emit input/output tokens when streaming via the OpenAI compatibility layer. The `Usage` fields may be zero.
   - What's unclear: Whether this has been fixed in recent Ollama versions (0.6+), and whether the `captured_usage` from `StreamEnd` provides any data.
   - Recommendation: Implement a heuristic context-size tracker (message count + character count estimate) as the primary mechanism. If `captured_usage` returns real values, use those preferentially. This is mostly a Phase 3 concern but the data collection should start in Phase 2.

2. **genai `append_tool_use_from_stream_end` exact usage**
   - What we know: This method exists on `ChatRequest` and is designed for appending assistant tool-use turns with responses from streaming capture.
   - What's unclear: Exact parameter types and whether it handles all the thought_signatures correctly.
   - Recommendation: Prefer manual message construction (append assistant message with tool calls, then append ToolResponse messages) for clarity and control. Fall back to `append_tool_use_from_stream_end` if manual approach proves complex.

3. **File read path resolution (absolute vs relative)**
   - What we know: Phase 1 decision says "Read anywhere, write workspace only." The `file_read` tool should accept both relative (to workspace) and absolute paths.
   - What's unclear: How to resolve relative paths consistently when the workspace is the shell's working directory.
   - Recommendation: If the path is absolute, use it directly. If relative, resolve against workspace root. This matches the shell execution behavior (which runs with `current_dir` set to workspace).

## Sources

### Primary (HIGH confidence)
- [docs.rs/genai/0.5.3](https://docs.rs/genai/0.5.3/genai/) - Full API documentation for Chat, Tool, ToolCall, ToolResponse, ChatStreamEvent, Client
- [docs.rs/genai/0.5.3/genai/chat](https://docs.rs/genai/0.5.3/genai/chat/index.html) - Chat module types including tool support
- [Ollama API docs](https://docs.ollama.com/api-reference/show-model-details) - /api/show endpoint for model info and context length
- [Ollama tool calling docs](https://docs.ollama.com/capabilities/tool-calling) - Tool calling API format and supported models
- [Tokio graceful shutdown](https://tokio.rs/tokio/topics/shutdown) - Official guide for signal handling and shutdown patterns

### Secondary (MEDIUM confidence)
- [genai GitHub repo](https://github.com/jeremychone/rust-genai) - README, examples (c01-conv.rs, c08-tooluse.rs, c10-tooluse-streaming.rs, c11-tooluse-deterministic.rs)
- [genai tool calling issue #24](https://github.com/jeremychone/rust-genai/issues/24) - Confirms tool calling implemented for OpenAI, Anthropic, and (via OpenAI compat) Ollama
- [Ollama context length docs](https://docs.ollama.com/context-length) - Default num_ctx, silent truncation behavior
- [Rust CLI signal handling](https://rust-cli.github.io/book/in-depth/signals.html) - Cross-platform signal handling patterns

### Tertiary (LOW confidence)
- WebSearch findings on genai version history (conflicting reports between 0.3.5 and 0.5.3 -- docs.rs confirms 0.5.3 is latest published)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - genai 0.5.3 API verified via docs.rs, tool calling types confirmed, streaming API documented
- Architecture: HIGH - Patterns derived from genai official examples and existing codebase structure (Phase 1)
- Pitfalls: MEDIUM - Ollama streaming token limitation documented in genai README; context truncation behavior from Ollama docs; message ordering from genai examples
- Signal handling: HIGH - tokio::signal::ctrl_c() is the standard well-documented approach

**Research date:** 2026-02-04
**Valid until:** 2026-03-06 (30 days -- genai and Ollama are actively developed but core APIs are stable)
