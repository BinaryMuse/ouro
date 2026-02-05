# Phase 5: Sub-Agent Orchestration - Research

**Researched:** 2026-02-04
**Domain:** Concurrent LLM session management, background process orchestration, hierarchical task trees
**Confidence:** HIGH

## Summary

Phase 5 adds the ability for the parent agent to spawn and manage child LLM chat sessions and background shell processes. The existing codebase already has all the foundational pieces: `genai::Client` (confirmed `Clone` + `Arc`-based, safe for concurrent use), `tokio::process` for shell execution with process-group management via `nix`, an mpsc-based event channel for TUI updates, and a `tui-tree-widget` dependency already in `Cargo.toml` (v0.24). The primary work is building the orchestration layer -- a shared registry (`SubAgentManager`) that tracks all spawned agents/processes in a tree, exposes tool schemas for the agent to spawn/query/kill them, integrates with the existing TUI panel placeholder, and ensures clean shutdown with no orphan processes.

The key technical decisions are: using `tokio-util::sync::CancellationToken` with hierarchical `child_token()` for cascading shutdown, a `DashMap`-backed (or `Arc<Mutex<HashMap>>`-backed) registry keyed by UUID, separate `SessionLogger` instances per sub-agent for independent log streams, and reusing the existing `run_agent_session` function (with modified parameters) for LLM sub-agents. Background processes extend the existing `exec::shell` module with long-lived process handles that support stdin writes and on-demand output retrieval.

**Primary recommendation:** Build a shared `SubAgentManager` (wrapped in `Arc`) that owns the agent/process tree, provides the tool dispatch surface, and integrates with both the TUI event channel and the shutdown signal chain. Use `tokio-util` `CancellationToken` for hierarchical cancellation rather than hand-rolling shutdown propagation.

## Standard Stack

The established libraries/tools for this domain:

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| genai | git main (v0.6.0-alpha.2-WIP) | LLM chat for sub-agents | Already in use; `Client` is `Clone` + `Arc`-based, safe for concurrent spawns |
| tokio | 1.x | Async runtime, process spawning, channels | Already in use with process, time, fs, rt-multi-thread, macros, io-util, signal features |
| tokio-util | 0.7.18 | `CancellationToken` with hierarchical child tokens | De facto standard for structured cancellation in tokio; `child_token()` cascades cancel downward |
| nix | 0.29 | Process group management, SIGKILL/SIGTERM | Already in use for shell process cleanup |
| tui-tree-widget | 0.24 | Hierarchical tree rendering in TUI | Already in Cargo.toml; `TreeItem<Identifier>` + `TreeState` for sub-agent tree panel |
| uuid | 4 | Unique IDs for sub-agents/processes | Lightweight, standard for opaque identifiers |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| serde / serde_json | 1.0 | Structured result serialization | Already in use; sub-agent results are JSON objects |
| chrono | 0.4 | Timestamps for sub-agent events | Already in use for logging |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| tokio-util CancellationToken | Manual AtomicBool per agent | CancellationToken has built-in tree hierarchy; AtomicBool requires manual parent-child wiring |
| uuid | Sequential u64 IDs | UUID is collision-free across restarts; u64 requires centralized counter |
| DashMap for registry | Arc<Mutex<HashMap>> | DashMap avoids holding lock during async operations; HashMap+Mutex is simpler but may contend |

**New dependencies to add:**
```toml
# In Cargo.toml [dependencies]
tokio-util = { version = "0.7", features = ["sync"] }
uuid = { version = "4", features = ["v4"] }
```

## Architecture Patterns

### Recommended Project Structure
```
src/
  orchestration/
    mod.rs              # Module exports
    manager.rs          # SubAgentManager -- the central registry
    types.rs            # SubAgentId, SubAgentStatus, SubAgentInfo, SubAgentResult, BackgroundProcessInfo
    llm_agent.rs        # Spawn and run an LLM sub-agent session as a tokio task
    background_proc.rs  # Spawn and manage long-lived background shell processes
  agent/
    tools.rs            # Extended with spawn_llm_session, spawn_background_task, agent_status, agent_result, kill_agent, write_stdin
  tui/
    tabs/agent_tab.rs   # Replace placeholder with real tree rendering from SubAgentManager state
    event.rs            # New AgentEvent variants for sub-agent lifecycle events
    app_state.rs        # New fields for sub-agent tree data
```

### Pattern 1: Shared Manager with Arc
**What:** A `SubAgentManager` struct wrapped in `Arc` that is passed to the agent loop, tool dispatch, and TUI. It owns a `DashMap<SubAgentId, SubAgentEntry>` mapping IDs to metadata + JoinHandle + CancellationToken.
**When to use:** Always -- this is the single source of truth for all spawned agents and processes.
**Example:**
```rust
// Source: Architecture pattern derived from codebase analysis
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub type SubAgentId = String; // UUID string

#[derive(Debug, Clone)]
pub enum SubAgentKind {
    LlmSession { model: String, goal: String },
    BackgroundProcess { command: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubAgentStatus {
    Running,
    Completed,
    Failed(String),
    Killed,
}

#[derive(Debug, Clone)]
pub struct SubAgentInfo {
    pub id: SubAgentId,
    pub kind: SubAgentKind,
    pub parent_id: Option<SubAgentId>,
    pub status: SubAgentStatus,
    pub spawned_at: String,
    pub completed_at: Option<String>,
}

pub struct SubAgentManager {
    entries: Arc<DashMap<SubAgentId, SubAgentEntry>>,
    root_cancel_token: CancellationToken,
    event_tx: Option<tokio::sync::mpsc::UnboundedSender<AgentEvent>>,
    max_depth: usize,
    max_total: usize,
}
```

### Pattern 2: CancellationToken Hierarchy for Shutdown
**What:** Each sub-agent gets a `CancellationToken` created as a `child_token()` of its parent's token. The root token is owned by the harness shutdown path. Cancelling the root cascades to all sub-agents.
**When to use:** All sub-agent and background process spawning.
**Example:**
```rust
// Source: tokio-util docs (https://docs.rs/tokio-util/0.7.18/tokio_util/sync/struct.CancellationToken.html)
let root_token = CancellationToken::new();

// Spawning a sub-agent:
let agent_token = root_token.child_token();

tokio::spawn(async move {
    tokio::select! {
        result = run_sub_agent_session(/* ... */) => {
            // Agent completed naturally
            handle_completion(result);
        }
        _ = agent_token.cancelled() => {
            // Parent (or harness) cancelled us
            handle_cancellation();
        }
    }
});

// On harness shutdown:
root_token.cancel(); // Cancels ALL sub-agents recursively
```

### Pattern 3: Sub-Agent LLM Session Reusing Existing Agent Loop
**What:** Sub-agent LLM sessions reuse the existing `run_agent_session` function with different parameters: a sub-agent-specific system prompt (goal + context injected), optionally a different model, and a dedicated event channel or event prefixing.
**When to use:** When spawning LLM sub-agents.
**Example:**
```rust
// Reuse the existing agent loop for sub-agents
async fn run_llm_sub_agent(
    sub_agent_id: SubAgentId,
    model: String,
    goal: String,
    context: HashMap<String, String>,
    safety: SafetyLayer,
    config: AppConfig,
    cancel_token: CancellationToken,
    result_tx: oneshot::Sender<SubAgentResult>,
) {
    // Build sub-agent-specific config (possibly different model)
    let mut sub_config = config.clone();
    sub_config.model = model;

    // Create separate SessionLogger for this sub-agent
    // Run the agent session with cancellation awareness
    // Capture the result and send via oneshot
}
```

### Pattern 4: Background Process with Stdin Handle
**What:** Background shell processes are spawned with piped stdin/stdout/stderr. The manager retains the `ChildStdin` handle for writing, and captures stdout/stderr into ring buffers that can be queried on demand.
**When to use:** When spawning background shell processes.
**Example:**
```rust
// Source: tokio::process docs
use tokio::process::Command;
use std::process::Stdio;

let mut child = Command::new("sh")
    .arg("-c")
    .arg(&command)
    .current_dir(workspace)
    .process_group(0)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true) // Safety net for background processes
    .spawn()?;

let stdin = child.stdin.take(); // Retained by manager for write_stdin tool
let stdout = child.stdout.take(); // Spawned reader task fills ring buffer
let stderr = child.stderr.take(); // Spawned reader task fills ring buffer
```

### Pattern 5: Tool Schema Design
**What:** Six new tools exposed to the agent: `spawn_llm_session`, `spawn_background_task`, `agent_status`, `agent_result`, `kill_agent`, `write_stdin`.
**When to use:** All sub-agent interaction from the parent.
**Example schemas:**
```rust
Tool::new("spawn_llm_session")
    .with_description("Spawn a child LLM chat session...")
    .with_schema(json!({
        "type": "object",
        "properties": {
            "goal": { "type": "string", "description": "What the sub-agent should accomplish" },
            "model": { "type": "string", "description": "Ollama model name (optional, defaults to parent model)" },
            "context": {
                "type": "object",
                "description": "Key-value context injected into sub-agent system prompt",
                "additionalProperties": { "type": "string" }
            },
            "timeout_secs": { "type": "integer", "description": "Optional timeout in seconds" },
            "tools": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Tool names to enable (default: all parent tools)"
            }
        },
        "required": ["goal"]
    }))
```

### Anti-Patterns to Avoid
- **Shared conversation history:** Sub-agents must start clean. Passing parent conversation history creates unbounded context and coupling.
- **Blocking the parent loop on sub-agent completion:** All sub-agent work is async and non-blocking. The parent queries status/results via tools.
- **Global mutable state for process tracking:** Use the `SubAgentManager` as the single owner. No statics, no global registries.
- **Spawning without JoinHandle retention:** Every `tokio::spawn` must store its `JoinHandle` in the manager for cleanup.

## Don't Hand-Roll

Problems that look simple but have existing solutions:

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Hierarchical cancellation | Manual AtomicBool chain per agent | `tokio-util::CancellationToken::child_token()` | Built-in tree structure, cancel cascades automatically, thread-safe |
| Process tree cleanup | Walk PIDs manually | `nix::sys::signal::killpg()` (already used) + `CancellationToken` | Process groups handle child process cleanup; CancellationToken handles task cleanup |
| Unique agent IDs | Sequential counter with mutex | `uuid::Uuid::new_v4()` | No contention, collision-free, works across restarts |
| Tree widget rendering | Custom tree drawing code | `tui-tree-widget` (already in deps) | `TreeItem` + `TreeState` handles nesting, selection, scrolling |
| Output ring buffer | Custom circular buffer | `VecDeque<String>` with capacity check | Simple, standard, good enough for on-demand output retrieval |

**Key insight:** The existing codebase already has 80% of the infrastructure needed. The `genai::Client` is `Clone` + `Arc`-based (confirmed from source: `struct Client { inner: Arc<ClientInner> }`). The shell executor already manages process groups. The TUI already has a tree widget dependency and a placeholder panel. The work is composing these pieces with a management layer, not building from scratch.

## Common Pitfalls

### Pitfall 1: Orphan Processes on Harness Shutdown
**What goes wrong:** Sub-agent background processes continue running after the harness exits, consuming resources.
**Why it happens:** `tokio::spawn` tasks are not automatically cancelled when the runtime shuts down if the process exits via `std::process::exit()`. Also, `Child` handles dropped without kill leave processes running.
**How to avoid:**
1. Use `kill_on_drop(true)` on all background `Command` spawns as a safety net.
2. Use `CancellationToken` hierarchy: root cancel triggers all sub-agent cancellation.
3. In the shutdown path, explicitly iterate the manager registry and kill remaining processes.
4. Use process groups (`process_group(0)`) so `killpg` reaches all children of background processes.
**Warning signs:** Processes still running after `ouro` exits (check with `ps aux | grep`).

### Pitfall 2: Ollama Resource Exhaustion from Concurrent Requests
**What goes wrong:** Spawning many LLM sub-agents simultaneously overwhelms Ollama, causing extreme slowdowns or OOM.
**Why it happens:** Each concurrent LLM session consumes VRAM/RAM proportional to its context. Ollama's `OLLAMA_NUM_PARALLEL` defaults to 4 but context size scales linearly with parallel requests.
**How to avoid:**
1. Document that Ollama's natural resource limits are the constraint (per user decision: "no hard concurrency limit").
2. Consider a soft warning in the system prompt about resource awareness.
3. The `max_total` field in `SubAgentManager` provides an optional safety valve.
**Warning signs:** Ollama responses become extremely slow; system runs out of memory.

### Pitfall 3: Deadlock on Background Process Stdin/Stdout
**What goes wrong:** Writing to stdin while the process's stdout buffer is full causes deadlock.
**Why it happens:** If stdout pipe buffer fills and nobody is reading, the process blocks on write, which blocks the stdin consumer.
**How to avoid:**
1. Always spawn separate reader tasks for stdout and stderr that drain into ring buffers continuously.
2. Never hold stdin write and await stdout read in the same task.
3. Drop stdin handle to signal EOF when the agent is done writing.
**Warning signs:** Background process appears hung; write_stdin tool hangs.

### Pitfall 4: Sub-Agent Survival Across Parent Restart
**What goes wrong:** Parent context restarts (session restart due to context exhaustion) and loses track of running sub-agents.
**Why it happens:** If the `SubAgentManager` is created per-session instead of per-harness-run, state is lost on restart.
**How to avoid:**
1. The `SubAgentManager` must live at the harness level (created in `main.rs` or `runner.rs`), not inside `run_agent_session`.
2. Pass `Arc<SubAgentManager>` through to each session.
3. The `agent_status` tool allows the restarted parent to rediscover running sub-agents.
**Warning signs:** After context restart, agent cannot see previously spawned sub-agents.

### Pitfall 5: Nested Spawning Depth Bomb
**What goes wrong:** A sub-agent spawns a sub-agent which spawns a sub-agent, exhausting system resources recursively.
**Why it happens:** No depth or count constraint on nested spawning.
**How to avoid:**
1. Track depth in the manager: each entry knows its parent, depth = parent.depth + 1.
2. Enforce `max_depth` (recommend default 3) and `max_total` (recommend default 10) in `SubAgentManager`.
3. Return an error from spawn tools when limits are exceeded.
**Warning signs:** System becomes unresponsive; many `ollama` processes in `ps`.

### Pitfall 6: TUI Event Channel Flooding from Many Sub-Agents
**What goes wrong:** Many concurrent sub-agents all sending events through the same channel overwhelms the TUI render loop.
**Why it happens:** Each sub-agent may generate ThoughtText, ToolCall, ToolResult events at high frequency.
**How to avoid:**
1. Sub-agent events should be batched or summarized, not streamed raw to TUI.
2. Use dedicated sub-agent log streams (separate files) for full detail.
3. TUI receives only lifecycle events (spawned, status change, completed, failed) for the sub-agent panel.
**Warning signs:** TUI becomes laggy; event channel backs up.

## Code Examples

Verified patterns from official sources:

### CancellationToken Hierarchy
```rust
// Source: https://docs.rs/tokio-util/0.7.18/tokio_util/sync/struct.CancellationToken.html
use tokio_util::sync::CancellationToken;

// Root token for the entire harness
let root = CancellationToken::new();

// Sub-agent gets a child token
let agent_token = root.child_token();

// Nested sub-agent gets a grandchild token
let nested_token = agent_token.child_token();

// Cancel root -> cancels agent_token and nested_token
root.cancel();
assert!(agent_token.is_cancelled());
assert!(nested_token.is_cancelled());

// But cancelling a child does NOT cancel parent:
let root2 = CancellationToken::new();
let child2 = root2.child_token();
child2.cancel();
assert!(!root2.is_cancelled()); // Parent unaffected
```

### TreeItem Construction for Sub-Agent Panel
```rust
// Source: https://docs.rs/tui-tree-widget/0.24.0/tui_tree_widget/struct.TreeItem.html
use tui_tree_widget::{Tree, TreeItem, TreeState};
use ratatui::style::{Color, Style};

fn build_sub_agent_tree(manager: &SubAgentManager) -> Vec<TreeItem<'static, String>> {
    // Build tree from manager's hierarchical data
    let mut root_items = Vec::new();

    for entry in manager.root_agents() {
        let status_icon = match entry.status {
            SubAgentStatus::Running => "[*]",
            SubAgentStatus::Completed => "[+]",
            SubAgentStatus::Failed(_) => "[!]",
            SubAgentStatus::Killed => "[x]",
        };

        let children: Vec<TreeItem<'static, String>> = manager
            .children_of(&entry.id)
            .map(|child| TreeItem::new_leaf(
                child.id.clone(),
                format!("{} {} ({})", status_icon, child.kind_label(), child.status_label()),
            ))
            .collect();

        let parent_item = TreeItem::new(
            entry.id.clone(),
            format!("{} {} ({})", status_icon, entry.kind_label(), entry.status_label()),
            children,
        ).unwrap_or_else(|_| TreeItem::new_leaf(entry.id.clone(), "error".to_string()));

        root_items.push(parent_item);
    }

    root_items
}
```

### Background Process with Stdin and Output Capture
```rust
// Source: https://docs.rs/tokio/latest/tokio/process/index.html
use tokio::process::Command;
use tokio::io::AsyncWriteExt;
use std::process::Stdio;
use std::collections::VecDeque;

async fn spawn_background_process(
    command: &str,
    workspace: &Path,
) -> anyhow::Result<BackgroundProcessHandle> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(workspace)
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let pid = child.id().unwrap_or(0);
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    // Ring buffer for captured output (shared with reader tasks)
    let output_buf = Arc::new(Mutex::new(VecDeque::with_capacity(1000)));

    // Spawn stdout reader
    let buf_clone = output_buf.clone();
    tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(stdout);
        let mut line = String::new();
        use tokio::io::AsyncBufReadExt;
        while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
            let mut buf = buf_clone.lock().unwrap();
            if buf.len() >= 1000 {
                buf.pop_front();
            }
            buf.push_back(line.clone());
            line.clear();
        }
    });

    Ok(BackgroundProcessHandle { pid, child, stdin, output_buf })
}
```

### genai Client Concurrent Usage
```rust
// Source: genai source (confirmed Client is #[derive(Clone)] with Arc<ClientInner>)
use genai::Client;
use genai::chat::{ChatMessage, ChatOptions, ChatRequest};

// Client is Clone + Arc-based -- cheap to share across tasks
let client = Client::default();

// Each sub-agent gets a cloned client
let sub_client = client.clone();
tokio::spawn(async move {
    let chat_req = ChatRequest::from_system("You are a sub-agent...")
        .with_tools(define_sub_agent_tools());
    let stream_res = sub_client
        .exec_chat_stream("qwen2.5:3b", chat_req, Some(&chat_options))
        .await;
    // Process stream...
});
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Manual AtomicBool shutdown flags | `tokio-util::CancellationToken` with child tokens | tokio-util 0.7+ | Hierarchical cancellation built-in, no manual wiring |
| Ollama single-request | Ollama 0.2+ parallel requests | 2024 | Concurrent sub-agent LLM sessions now viable with single Ollama instance |
| `kill_on_drop` only for child cleanup | Process group + explicit killpg | Stable pattern | Reliable cleanup of entire process trees, not just direct children |

**Deprecated/outdated:**
- Ollama versions before 0.2 did not support concurrent requests natively.
- Tokio's `task::scope` RFC (structured concurrency) is still not merged; use `CancellationToken` hierarchy instead.

## Open Questions

Things that couldn't be fully resolved:

1. **Failure notification: polling vs event injection**
   - What we know: The user left this to Claude's discretion. Both patterns are viable.
   - What's unclear: Whether injecting a system message into the parent's conversation on sub-agent failure could disrupt the parent's current tool-call sequence.
   - Recommendation: Use status polling (agent_status tool) as the primary mechanism. Additionally, emit an AgentEvent for TUI notification. Avoid injecting messages into the parent conversation mid-turn -- the parent discovers failures on its next status check. This is simpler and avoids race conditions with the genai chat message sequence.

2. **Sub-agent tool set customization**
   - What we know: User decided parent can customize tool set per spawn. The `define_tools()` function returns a static Vec.
   - What's unclear: How to filter tools at the genai ChatRequest level (the `with_tools` call).
   - Recommendation: `define_tools()` should accept an optional filter list. If `tools` parameter is provided in spawn, filter to only those tools. Default is all tools the parent has. This is straightforward -- just filter the Vec before passing to `with_tools`.

3. **Log stream format for sub-agents**
   - What we know: Each sub-agent needs a separate log stream (requirement LOG-03). The existing `SessionLogger` creates timestamped JSONL files.
   - What's unclear: Whether sub-agent logs should be in a subdirectory or flat alongside parent logs.
   - Recommendation: Use `{workspace_parent}/.ouro-logs/sub-{agent_id}/session-{timestamp}.jsonl` subdirectory structure. This keeps sub-agent logs grouped and easily identifiable.

4. **Exact structured result format**
   - What we know: Sub-agents return a structured result object (user decision).
   - Recommendation: Use a simple JSON structure:
   ```json
   {
     "agent_id": "uuid",
     "status": "completed|failed",
     "summary": "Brief description of what was accomplished",
     "output": "Detailed output or error message",
     "files_modified": ["list", "of", "paths"],
     "elapsed_secs": 42
   }
   ```
   The sub-agent writes this as its final action before ending. The harness captures it.

## Sources

### Primary (HIGH confidence)
- genai source code (local: `~/.cargo/git/checkouts/rust-genai-*/ce7feec/src/client/client_types.rs`) -- Confirmed `Client` is `#[derive(Debug, Clone)]` with `inner: Arc<ClientInner>`
- [tokio-util CancellationToken docs](https://docs.rs/tokio-util/0.7.18/tokio_util/sync/struct.CancellationToken.html) -- child_token() hierarchy, cancel propagation, DropGuard
- [tokio::process module docs](https://docs.rs/tokio/latest/tokio/process/index.html) -- Command, Child, ChildStdin, kill_on_drop, process group management
- [tui-tree-widget docs](https://docs.rs/tui-tree-widget/0.24.0/tui_tree_widget/) -- TreeItem, TreeState, Tree rendering API
- Existing codebase: `src/agent/agent_loop.rs`, `src/agent/tools.rs`, `src/exec/shell.rs`, `src/safety/mod.rs`, `src/tui/` -- Complete architecture review

### Secondary (MEDIUM confidence)
- [Ollama concurrency](https://www.glukhov.org/post/2025/05/how-ollama-handles-parallel-requests/) -- OLLAMA_NUM_PARALLEL, queuing behavior, memory scaling
- [Ollama 0.2 announcement](https://medium.com/@simeon.emanuilov/ollama-0-2-revolutionizing-local-model-management-with-concurrency-2318115ce961) -- Concurrent requests enabled by default
- [Tokio task cancellation patterns](https://cybernetist.com/2024/04/19/rust-tokio-task-cancellation-patterns/) -- CancellationToken usage patterns
- [Tokio structured concurrency discussion](https://github.com/tokio-rs/tokio/issues/2592) -- Why CancellationToken is the current solution (task::scope not yet available)

### Tertiary (LOW confidence)
- [kill_tree crate](https://lib.rs/crates/kill_tree) -- Recursive process tree killing; evaluated but not recommended (nix killpg + process groups already covers this use case)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- All core libraries verified from source code and official docs. genai Client clone-safety confirmed from actual source.
- Architecture: HIGH -- Patterns derived from thorough analysis of existing 4-phase codebase. Extension points are clear (tools.rs dispatch, TUI placeholder, event channel).
- Pitfalls: HIGH -- Process management and Ollama concurrency pitfalls verified against official docs and community reports.
- Open questions: MEDIUM -- Discretionary decisions have clear recommendations but may need user validation.

**Research date:** 2026-02-04
**Valid until:** 2026-03-06 (30 days -- stable domain, no fast-moving dependencies)
