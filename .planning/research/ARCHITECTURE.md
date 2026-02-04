# Architecture Research

**Domain:** Autonomous AI agent harness (local LLM loop with TUI)
**Researched:** 2026-02-03
**Confidence:** HIGH (architecture patterns from official Tokio docs, ratatui docs, genai crate API; verified across multiple authoritative sources)

## Standard Architecture

### System Overview

```
┌──────────────────────────────────────────────────────────────────────────┐
│                           TUI Layer (ratatui)                            │
│  ┌────────────┐  ┌────────────┐  ┌────────────┐  ┌────────────┐        │
│  │ Agent Log  │  │ Sub-Agent  │  │ Discoveries│  │  Progress  │        │
│  │   Panel    │  │   Tree     │  │   Panel    │  │   Panel    │        │
│  └─────┬──────┘  └─────┬──────┘  └─────┬──────┘  └─────┬──────┘        │
│        └────────────────┴───────────────┴───────────────┘              │
│                            ▲ (watch channels)                          │
├──────────────────────────────────────────────────────────────────────────┤
│                      Coordinator Actor (hub)                            │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │  mpsc::Receiver<CoordinatorMsg> — central message bus           │    │
│  │  Owns: AgentState, SubAgentRegistry, DiscoveryLog, PauseState   │    │
│  └────────┬──────────────┬──────────────┬──────────────┬───────────┘    │
│           │              │              │              │                │
│           ▼              ▼              ▼              ▼                │
│  ┌──────────────┐ ┌────────────┐ ┌────────────┐ ┌──────────────┐      │
│  │ Agent Loop   │ │ Tool       │ │ Sub-Agent  │ │ Pause/Resume │      │
│  │ Actor        │ │ Executor   │ │ Supervisor │ │ Controller   │      │
│  └──────────────┘ └────────────┘ └────────────┘ └──────────────┘      │
│           │              │              │                               │
├───────────┴──────────────┴──────────────┴───────────────────────────────┤
│                       External I/O Layer                                │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐               │
│  │ Ollama   │  │ Shell    │  │ File I/O │  │ HTTP     │               │
│  │ (genai)  │  │ (tokio   │  │ (tokio   │  │ (reqwest)│               │
│  │          │  │ process) │  │ fs)      │  │          │               │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘               │
└──────────────────────────────────────────────────────────────────────────┘
```

### Component Responsibilities

| Component | Responsibility | Communicates With | Implementation |
|-----------|----------------|-------------------|----------------|
| **TUI Renderer** | Renders terminal UI, captures keyboard input, dispatches user actions | Coordinator (via watch channels for state, mpsc for user input) | ratatui + crossterm `EventStream`, dedicated tokio task |
| **Coordinator Actor** | Central hub: routes messages, owns canonical state, enforces pause/resume | All components via mpsc channels | Single tokio task with `mpsc::Receiver` run loop |
| **Agent Loop Actor** | Infinite LLM conversation cycle: build request, call Ollama, parse response, dispatch tool calls | Coordinator (results, status), Ollama (via genai) | Dedicated tokio task, owns chat history `Vec<ChatMessage>` |
| **Tool Executor** | Executes tool calls (shell, file, web, search, sleep) with guardrails | Coordinator (receives tool requests, returns results) | Pool of tokio tasks, one per concurrent tool execution |
| **Sub-Agent Supervisor** | Spawns, tracks, and terminates sub-agents (LLM sessions + background processes) | Coordinator (lifecycle events), child Agent Loop actors | Owns `HashMap<SubAgentId, JoinHandle>`, manages child mpsc senders |
| **Pause/Resume Controller** | Timer-based, event-based, and user-controlled pause logic | Coordinator (pause state), Agent Loop (gate signal) | `tokio::sync::watch` channel broadcasting pause state |
| **Context Manager** | Tracks token usage, triggers context window restart when limit approached | Agent Loop (token counts, restart signal) | Embedded in Agent Loop actor, not a separate task |

## Recommended Project Structure

```
src/
├── main.rs                 # Entry point: init tokio runtime, wire channels, spawn actors
├── coordinator/
│   ├── mod.rs              # Coordinator actor: message loop, state ownership
│   ├── messages.rs         # CoordinatorMsg enum (all message types)
│   └── state.rs            # Canonical AppState struct
├── agent/
│   ├── mod.rs              # Agent loop actor: LLM call cycle
│   ├── context.rs          # Context window management, observation masking
│   └── history.rs          # Chat history builder, system prompt loading
├── tools/
│   ├── mod.rs              # Tool dispatch: match ToolCall -> executor
│   ├── shell.rs            # Workspace-scoped shell execution
│   ├── file_io.rs          # File read/write with path validation
│   ├── web_fetch.rs        # HTTP fetch via reqwest
│   ├── search.rs           # Internet search integration
│   ├── sub_agent.rs        # Spawn sub-agent tool
│   └── sleep.rs            # Pause/sleep tool
├── sub_agent/
│   ├── mod.rs              # Sub-agent supervisor actor
│   └── registry.rs         # Sub-agent tracking, lifecycle management
├── tui/
│   ├── mod.rs              # TUI setup, terminal init/shutdown
│   ├── app.rs              # Top-level render function, layout
│   ├── events.rs           # Keyboard/mouse event handler (crossterm EventStream)
│   ├── panels/
│   │   ├── agent_log.rs    # Main agent conversation panel
│   │   ├── sub_agents.rs   # Sub-agent tree panel
│   │   ├── discoveries.rs  # Discoveries/findings panel
│   │   └── progress.rs     # Progress/status panel
│   └── actions.rs          # UserAction enum for TUI -> Coordinator
├── pause/
│   ├── mod.rs              # Pause controller: timer, event, user triggers
│   └── strategy.rs         # Pause strategy types
├── config/
│   ├── mod.rs              # Configuration loading
│   └── guardrails.rs       # Shell execution guardrails, path validation
└── error.rs                # Unified error types (thiserror)
```

### Structure Rationale

- **coordinator/**: Single actor that owns all mutable state. Every other component communicates through it. This eliminates shared mutable state and prevents data races at the architecture level, not just at the type level.
- **agent/**: Separated from coordinator because the LLM call cycle has its own complex state (chat history, context tracking). It runs as its own actor with a clean message interface.
- **tools/**: Each tool is an independent module with a uniform `async fn execute(params) -> ToolResult` interface. New tools are added by implementing the pattern and adding a match arm in the dispatcher.
- **tui/**: Panel-per-file mirrors ratatui's component model. The `app.rs` file handles layout; each panel file handles rendering its own area.
- **sub_agent/**: Separated because sub-agent lifecycle management is complex (spawn, track, cancel, collect output) and orthogonal to tool execution.
- **pause/**: Isolated because pause logic crosscuts multiple components and benefits from a single source of truth.

## Architectural Patterns

### Pattern 1: Actor-per-Component with Channel Mesh

**What:** Each major component runs as an independent tokio task (actor) communicating via typed mpsc channels. The Coordinator is the central hub; no component-to-component direct channels.

**When to use:** Always in this system. Every component has its own async lifecycle and internal state.

**Trade-offs:**
- PRO: No shared mutable state. Rust's ownership model naturally enforces message passing.
- PRO: Components can be developed and tested independently.
- PRO: Clean shutdown via channel drop propagation.
- CON: Slight message routing overhead through coordinator. Acceptable at the message volumes in an LLM agent loop (messages per second, not per microsecond).

**Example:**
```rust
// messages.rs
enum CoordinatorMsg {
    // From Agent Loop
    AgentStatus(AgentStatusUpdate),
    ToolRequest { call: ToolCall, respond_to: oneshot::Sender<ToolResult> },
    ContextLimitReached,

    // From Tool Executor
    ToolCompleted { call_id: String, result: ToolResult },

    // From Sub-Agent Supervisor
    SubAgentEvent(SubAgentEvent),

    // From TUI
    UserAction(UserAction),

    // From Pause Controller
    PauseStateChanged(bool),
}

// coordinator/mod.rs
async fn run_coordinator(mut rx: mpsc::Receiver<CoordinatorMsg>, state: AppState) {
    while let Some(msg) = rx.recv().await {
        match msg {
            CoordinatorMsg::ToolRequest { call, respond_to } => {
                // Dispatch to tool executor, which will send result via respond_to
                tool_executor.execute(call, respond_to).await;
            }
            CoordinatorMsg::ContextLimitReached => {
                // Signal agent loop to restart with system prompt
                agent_handle.restart().await;
            }
            // ... other message handlers
        }
        // After each message, update watch channels for TUI
        tui_state_tx.send(state.snapshot()).ok();
    }
}
```

### Pattern 2: Request-Response via mpsc + oneshot

**What:** When the Agent Loop needs a tool result before continuing, it sends a `ToolRequest` containing a `oneshot::Sender` through the coordinator. The tool executor sends the result back directly via the oneshot channel.

**When to use:** Whenever the agent loop must block on a tool result (which is the normal case: the LLM needs tool output to generate its next response).

**Trade-offs:**
- PRO: Agent loop awaits exactly one result. No complex state machine for "waiting for tool."
- PRO: Tool execution is naturally async; multiple tools could execute concurrently if the LLM emits multiple tool calls.
- CON: Must avoid deadlock cycles. Since coordinator dispatches and agent awaits, the coordinator must never await the agent in the same code path.

**Example:**
```rust
// agent/mod.rs
async fn agent_loop(
    client: genai::Client,
    coordinator_tx: mpsc::Sender<CoordinatorMsg>,
    mut pause_rx: watch::Receiver<bool>,
) {
    let mut history: Vec<ChatMessage> = vec![load_system_prompt()];

    loop {
        // Check pause state
        while *pause_rx.borrow() {
            pause_rx.changed().await.ok();
        }

        // Call Ollama
        let request = ChatRequest::from_msgs(history.clone());
        let response = client.exec_chat("model_name", request, None).await?;

        // Append assistant message
        history.push(ChatMessage::assistant(&response.content_text_as_str()));

        // Handle tool calls
        if let Some(tool_calls) = response.tool_calls {
            for call in tool_calls {
                let (tx, rx) = oneshot::channel();
                coordinator_tx.send(CoordinatorMsg::ToolRequest {
                    call: call.clone(),
                    respond_to: tx,
                }).await?;
                let result = rx.await?;
                history.push(ChatMessage::tool(result.to_string(), &call.call_id));
            }
        }

        // Check context limits
        if context_manager.approaching_limit(&history) {
            coordinator_tx.send(CoordinatorMsg::ContextLimitReached).await?;
            // Restart: compress or reset history
            history = context_manager.restart(history);
        }
    }
}
```

### Pattern 3: Watch Channels for TUI State Broadcasting

**What:** The coordinator publishes a read-only state snapshot via `tokio::sync::watch` channels. The TUI renderer subscribes and re-renders when state changes.

**When to use:** For all TUI state updates. The TUI only needs the latest state (not a queue of updates), making watch channels ideal.

**Trade-offs:**
- PRO: TUI never blocks the coordinator. Watch channels are non-blocking for senders.
- PRO: TUI naturally gets the latest state even if it falls behind on renders.
- PRO: Multiple TUI panels can subscribe to the same watch channel.
- CON: If intermediate states matter (e.g., streaming tokens), use mpsc for that specific data flow.

**Example:**
```rust
// TUI render loop
async fn run_tui(
    mut state_rx: watch::Receiver<AppStateSnapshot>,
    action_tx: mpsc::Sender<CoordinatorMsg>,
) {
    let mut terminal = setup_terminal()?;
    let mut event_stream = crossterm::event::EventStream::new();

    loop {
        tokio::select! {
            // Keyboard/mouse events
            Some(Ok(event)) = event_stream.next() => {
                if let Some(action) = map_event_to_action(event) {
                    action_tx.send(CoordinatorMsg::UserAction(action)).await?;
                }
            }
            // State changed -> re-render
            Ok(()) = state_rx.changed() => {
                let state = state_rx.borrow().clone();
                terminal.draw(|frame| render_app(frame, &state))?;
            }
        }
    }
}
```

## Data Flow

### Primary Agent Loop Flow

```
                         ┌──────────────────┐
                         │  SYSTEM_PROMPT.md │
                         └────────┬─────────┘
                                  │ (load once, reload on restart)
                                  ▼
┌─────────┐  ChatRequest  ┌────────────┐  ChatResponse  ┌─────────────┐
│  Agent   │─────────────→│   Ollama   │───────────────→│   Agent     │
│  Loop    │              │  (genai)   │                │   Loop      │
│          │←─────────────│            │                │  (parse)    │
└────┬─────┘              └────────────┘                └──────┬──────┘
     │                                                         │
     │ Has tool calls?                                         │
     │ YES                                                     │
     ▼                                                         │
┌─────────────┐  ToolRequest+oneshot  ┌─────────────┐         │
│  Agent Loop │──────────────────────→│ Coordinator │         │
│  (await     │                       │  (dispatch) │         │
│   result)   │                       └──────┬──────┘         │
│             │                              │                │
│             │                              ▼                │
│             │                       ┌─────────────┐         │
│             │                       │ Tool        │         │
│             │  ToolResult (oneshot) │ Executor    │         │
│             │←──────────────────────│             │         │
└─────────────┘                       └─────────────┘         │
     │                                                         │
     │ Append tool result to history                           │
     │ Loop back to Ollama call ◄──────────────────────────────┘
     │
     │ Context approaching limit?
     │ YES → Restart with SYSTEM_PROMPT.md
     │        (observation masking + optional summary of key findings)
     │
     ▼ (continues indefinitely)
```

### Sub-Agent Spawn Flow

```
Agent Loop (main)
     │
     │ ToolCall: spawn_sub_agent(task, model)
     ▼
Coordinator
     │
     │ CoordinatorMsg::SpawnSubAgent { config }
     ▼
Sub-Agent Supervisor
     │
     ├── Spawn new tokio task with its own Agent Loop
     │   (own chat history, own genai::Client, own tool access)
     │
     ├── Register in SubAgentRegistry { id, handle, status }
     │
     └── Forward sub-agent events to Coordinator
         (status updates, discoveries, completion)
```

### TUI Data Flow

```
Coordinator
     │
     │ watch::Sender<AppStateSnapshot>
     │ (updated after every message handled)
     ▼
TUI Renderer
     │
     ├── Agent Log Panel   ← state.agent_log (Vec<LogEntry>)
     ├── Sub-Agent Tree     ← state.sub_agents (tree structure)
     ├── Discoveries Panel  ← state.discoveries (Vec<Discovery>)
     └── Progress Panel     ← state.progress (metrics, status)

User Keyboard Input
     │
     │ crossterm::EventStream
     ▼
TUI Event Handler
     │
     │ mpsc::Sender<CoordinatorMsg::UserAction>
     ▼
Coordinator (processes user actions: pause, resume, focus, quit)
```

### Key Data Flows

1. **LLM Call Cycle:** Agent Loop builds `ChatRequest` from history -> sends to Ollama via `genai::Client::exec_chat()` -> parses `ChatResponse` -> extracts tool calls or text -> appends to history -> loops.

2. **Tool Execution:** Agent Loop sends `ToolRequest` with `oneshot::Sender` through coordinator -> Coordinator dispatches to appropriate tool executor -> Tool executor runs async operation (shell, file, HTTP) -> sends `ToolResult` back via oneshot -> Agent Loop receives and appends to history.

3. **State Broadcasting:** Every time the coordinator handles a message that changes observable state, it publishes a new `AppStateSnapshot` on the watch channel. The TUI re-renders on change.

4. **Context Restart:** When the context manager detects the token count approaching the model's limit, the agent loop resets its chat history to `[SYSTEM_PROMPT, summary_of_key_findings]`, preserving critical discoveries while clearing verbose tool output.

## Concurrency Model

### Tokio Runtime Configuration

Use `#[tokio::main]` with the multi-threaded runtime (default). The system will have 5-15 concurrent tokio tasks at typical operation:

| Task | Lifetime | Blocking? |
|------|----------|-----------|
| Coordinator | Application lifetime | No (pure message routing) |
| Agent Loop (main) | Application lifetime, restarts on context reset | No (async I/O to Ollama) |
| TUI Renderer | Application lifetime | No (async EventStream + watch) |
| Tool Executor (per call) | Per tool invocation, seconds to minutes | Shell: uses `tokio::process` (non-blocking). File I/O: `tokio::fs`. HTTP: `reqwest`. |
| Sub-Agent Loop (per agent) | Variable, controlled by supervisor | No (same pattern as main agent) |
| Sub-Agent Background Process | Variable | Uses `tokio::process`, non-blocking |
| Pause Controller | Application lifetime | No (timer + watch) |

### Channel Architecture

```
                    ┌──────────────────────────┐
                    │      Coordinator         │
                    │  mpsc::Receiver<Msg>     │
                    │  watch::Sender<State>    │
                    └──────────┬───────────────┘
                               │
              ┌────────────────┼────────────────────┐
              │                │                     │
   mpsc::Sender         mpsc::Sender          mpsc::Sender
   (Agent Loop)          (TUI)             (Sub-Agent Supervisor)
              │                │                     │
              │         watch::Receiver        watch::Receiver
              │          (TUI state)           (TUI state)
              │
        oneshot per tool call
        (Agent Loop ←── Tool Executor)
```

**Channel sizing guidance:**
- Coordinator mpsc: bounded(256) -- handles bursts from multiple tool completions
- Tool request oneshot: unbounded by nature (single value)
- Watch channels: latest-value semantics, no sizing needed
- Sub-agent mpsc: bounded(64) per sub-agent

### Shutdown Protocol

Shutdown follows channel drop propagation (the "Tokio actor" pattern):

1. User sends Quit action via TUI
2. Coordinator receives `UserAction::Quit`
3. Coordinator drops its state, including all `mpsc::Sender` handles to child actors
4. Child actors detect channel closure (`recv()` returns `None`) and exit their loops
5. Sub-Agent Supervisor aborts child tasks via `JoinHandle::abort()`
6. TUI task exits, restores terminal
7. `main()` awaits all `JoinHandle`s, then exits

### Avoiding Deadlocks

Critical constraint from the actor pattern: **no circular bounded-channel dependencies**.

- Agent Loop -> Coordinator -> Tool Executor -> (oneshot back to Agent Loop): This is safe because the oneshot is unbounded and the agent loop awaits the oneshot, not the coordinator's channel.
- Coordinator MUST NOT await the agent loop's channel in any code path that the agent loop triggers. The coordinator is fire-and-forget toward child actors (sends messages, does not await responses through the coordinator channel).

## Scaling Considerations

This is a local, single-user application. "Scaling" means handling more sub-agents and longer sessions, not more users.

| Concern | At 1 agent | At 5 sub-agents | At 20+ sub-agents |
|---------|-----------|-----------------|-------------------|
| Memory | Minimal: one chat history in memory | Moderate: 5 chat histories | Watch channel cloning becomes relevant; consider compressing older sub-agent histories |
| Ollama throughput | Single serial call chain | 5 parallel inference streams; Ollama queues internally | Ollama GPU saturation; implement request queue with backpressure |
| TUI rendering | Trivial | Sub-agent tree grows; still fast | Consider virtualizing the sub-agent panel (render only visible entries) |
| Channel backpressure | No concern | Monitor coordinator queue depth | Add metrics; bounded channels provide natural backpressure |

### Scaling Priorities

1. **First bottleneck: Ollama inference throughput.** GPU memory and compute are the ceiling. Mitigate by: queuing sub-agent requests, prioritizing main agent, allowing sub-agents to use smaller models.
2. **Second bottleneck: Context window size.** Long sessions accumulate large histories. Mitigate by: observation masking (replace old tool output with placeholders, keep reasoning), periodic summaries of key findings, hard restart with SYSTEM_PROMPT.md when approaching limit.

## Anti-Patterns

### Anti-Pattern 1: Shared Mutable AppState Behind Arc<Mutex>

**What people do:** Put all application state in `Arc<Mutex<AppState>>` and pass clones to every task.
**Why it's wrong:** Creates lock contention between the agent loop, TUI renderer, and tool executors. Mutex poisoning on panic crashes the whole application. Violates Rust's "share memory by communicating" principle.
**Do this instead:** Use the Coordinator actor pattern. One task owns the state; others communicate via channels. The watch channel provides lock-free reads for the TUI.

### Anti-Pattern 2: Blocking Ollama Calls on the Tokio Runtime

**What people do:** Use synchronous HTTP calls or `block_on()` inside a tokio task to call Ollama.
**Why it's wrong:** Starves the tokio runtime's thread pool, freezing the TUI and all other async tasks.
**Do this instead:** Use `genai::Client::exec_chat()` which is natively async. If using raw HTTP, use `reqwest` async client. If a library requires blocking, use `tokio::task::spawn_blocking()`.

### Anti-Pattern 3: Unbounded Channels Everywhere

**What people do:** Use `mpsc::unbounded_channel()` for all communication to avoid thinking about backpressure.
**Why it's wrong:** If the agent loop emits tool calls faster than they can be processed (unlikely but possible with sub-agents), memory grows without bound. No natural flow control.
**Do this instead:** Use bounded channels with reasonable capacity. The only exception is oneshot (inherently bounded to 1) and watch (inherently latest-value).

### Anti-Pattern 4: Context Window as an Afterthought

**What people do:** Append every message to chat history without tracking token count. Eventually hit the model's context limit and get truncated or errored responses.
**Why it's wrong:** LLM quality degrades before hitting the hard limit (lost-in-the-middle effect). Truncation from the provider side loses critical system prompt or early context.
**Do this instead:** Track token count after every exchange. Implement observation masking first (replace old tool output with `[output truncated -- see discovery log]`), then fall back to full restart with SYSTEM_PROMPT.md + key findings summary.

### Anti-Pattern 5: Tight Coupling Between TUI and Business Logic

**What people do:** Put LLM call logic inside TUI event handlers, or make tool executors directly update TUI widgets.
**Why it's wrong:** Makes the TUI untestable without a live Ollama instance. Makes the agent loop untestable without a terminal. Prevents running headless for CI or batch mode.
**Do this instead:** TUI communicates only via channels to the Coordinator. The agent loop has no knowledge of the TUI. The system should be runnable headless by replacing the TUI task with a logger.

## Integration Points

### External Services

| Service | Integration Pattern | Notes |
|---------|---------------------|-------|
| Ollama | `genai::Client::exec_chat()` / `exec_chat_stream()` async | Model name passed as string (e.g., `"llama3:70b"`). Client auto-detects Ollama as provider for non-prefixed model names. Reuse one `Client` instance. |
| Shell (workspace) | `tokio::process::Command::new("sh").arg("-c").arg(cmd).current_dir(workspace)` | Set `kill_on_drop(true)`. Capture stdout+stderr. Enforce timeout via `tokio::time::timeout`. Validate paths against workspace root. |
| File system | `tokio::fs::read_to_string()` / `tokio::fs::write()` | Validate all paths resolve within workspace directory. Reject path traversal (`..`). |
| HTTP fetch | `reqwest::Client::get(url).send().await` | Reuse one `reqwest::Client`. Set timeout. Respect robots.txt optionally. Return body as text/markdown. |
| Internet search | Via search API (e.g., SearXNG, Brave Search API) through `reqwest` | API key management. Rate limiting via semaphore. |

### Internal Boundaries

| Boundary | Communication | Notes |
|----------|---------------|-------|
| Agent Loop <-> Coordinator | mpsc + oneshot | Agent sends status + tool requests; coordinator sends control signals (restart, pause) |
| Coordinator <-> TUI | watch (state down) + mpsc (actions up) | Unidirectional data flow: state flows down, user actions flow up |
| Coordinator <-> Sub-Agent Supervisor | mpsc (both directions) | Supervisor reports lifecycle events; coordinator sends spawn/kill commands |
| Tool Executor <-> Agent Loop | oneshot (result) | Direct response path bypasses coordinator for latency; coordinator only dispatches |
| Agent Loop <-> Context Manager | In-process function calls | Context manager is not a separate task; it's a module called by the agent loop |

## Build Order (Dependency Graph)

Build order follows data flow dependencies. Each phase can only start after its dependencies are complete.

```
Phase 1: Foundation
├── config/ (configuration, guardrails)
├── error.rs (unified error types)
└── coordinator/messages.rs (message enum -- defines all interfaces)

Phase 2: Core Loop (depends on Phase 1)
├── coordinator/mod.rs (coordinator actor skeleton)
├── agent/history.rs (system prompt loading, chat history)
└── agent/mod.rs (agent loop: call Ollama, parse response)
    └── Minimal genai integration: exec_chat, no tools yet

Phase 3: Tool System (depends on Phase 2)
├── tools/mod.rs (tool dispatcher)
├── tools/shell.rs (workspace-scoped shell)
├── tools/file_io.rs (file read/write)
├── tools/web_fetch.rs (HTTP fetch)
├── tools/search.rs (internet search)
├── tools/sleep.rs (pause tool)
└── Wire tool dispatch into coordinator + agent loop

Phase 4: Context Management (depends on Phase 2)
├── agent/context.rs (token counting, observation masking)
└── Restart logic in agent loop

Phase 5: TUI (depends on Phase 2, parallel with 3-4)
├── tui/mod.rs (terminal setup/teardown)
├── tui/events.rs (crossterm EventStream)
├── tui/app.rs (layout, render dispatch)
├── tui/panels/*.rs (individual panels)
└── tui/actions.rs (user action enum)

Phase 6: Sub-Agents (depends on Phase 3)
├── sub_agent/mod.rs (supervisor actor)
├── sub_agent/registry.rs (tracking)
├── tools/sub_agent.rs (spawn tool)
└── Wire into coordinator

Phase 7: Pause/Resume (depends on Phase 2, parallel with 3-6)
├── pause/mod.rs (controller)
├── pause/strategy.rs (timer, event, user triggers)
└── Wire watch channel into agent loop gate

Phase 8: Integration & Polish
├── Wire all components together in main.rs
├── Headless mode (replace TUI with logger)
└── End-to-end testing
```

**Key dependency insight:** The Coordinator message enum (Phase 1) defines ALL interfaces upfront. This is the single most important file in the project -- it is the contract between every component. Design it carefully before writing any actor logic.

## Context Window Management Strategy

Based on JetBrains NeurIPS 2025 research (observation masking outperformed LLM summarization in 4/5 configurations while being 52% cheaper), the recommended approach is:

### Primary: Observation Masking

Replace old tool output (shell stdout, file contents, HTTP responses) with compact placeholders while preserving the agent's reasoning and action history in full.

```
Before masking:
  [user] Run `find . -name "*.rs" | head -50`
  [tool_result] src/main.rs\nsrc/lib.rs\nsrc/agent/mod.rs\n... (48 more lines)
  [assistant] I found 50 Rust files. The project has modules for...

After masking:
  [user] Run `find . -name "*.rs" | head -50`
  [tool_result] [output masked -- 50 lines, listed Rust source files]
  [assistant] I found 50 Rust files. The project has modules for...
```

### Secondary: Hard Restart with Findings Carry-Over

When observation masking is insufficient (approaching 80% of context limit), restart the entire conversation:

1. Save current `discoveries` list (key findings the agent has logged)
2. Reset history to: `[SYSTEM_PROMPT.md, "Previous session findings: {discoveries}"]`
3. Agent continues with fresh context but retained knowledge

### Token Counting

Estimate token count per message using a simple heuristic (chars / 4 for English text). Precise counting requires tokenizer integration which adds complexity; the heuristic is sufficient for triggering thresholds with margin.

## Sources

- [genai crate (GitHub)](https://github.com/jeremychone/rust-genai) -- Multi-provider Rust LLM client, Ollama support, tool call API (HIGH confidence)
- [genai chat module API (docs.rs)](https://docs.rs/genai/latest/genai/chat/index.html) -- ChatMessage, ChatRequest, ChatResponse, Tool, ToolCall, ToolResponse types (HIGH confidence)
- [Tokio channels tutorial](https://tokio.rs/tokio/tutorial/channels) -- mpsc, oneshot, broadcast, watch channel patterns (HIGH confidence)
- [Actors with Tokio (Alice Ryhl)](https://ryhl.io/blog/actors-with-tokio/) -- Actor + Handle pattern, shutdown via channel drop, deadlock avoidance (HIGH confidence)
- [Ratatui async event stream tutorial](https://ratatui.rs/tutorials/counter-async-app/async-event-stream/) -- EventHandler, tokio::select!, crossterm EventStream (HIGH confidence)
- [TUI Core component framework (GitHub)](https://github.com/AstekGroup/tui-core) -- Component trait, action pattern, lifecycle management (MEDIUM confidence)
- [JetBrains Research: Efficient Context Management](https://blog.jetbrains.com/research/2025/12/efficient-context-management/) -- Observation masking vs LLM summarization, NeurIPS 2025 (HIGH confidence)
- [Context Window Management Strategies (Maxim)](https://www.getmaxim.ai/articles/context-window-management-strategies-for-long-context-ai-agents-and-chatbots/) -- Sliding window, summarization, truncation strategies (MEDIUM confidence)
- [tokio::process::Command (docs.rs)](https://docs.rs/tokio/latest/tokio/process/struct.Command.html) -- Async process execution, kill_on_drop (HIGH confidence)
- [reqwest crate (docs.rs)](https://docs.rs/reqwest/) -- Async HTTP client, connection pooling (HIGH confidence)
- [Red Hat: Agentic AI Developers Moving to Rust](https://developers.redhat.com/articles/2025/09/15/why-some-agentic-ai-developers-are-moving-code-python-rust) -- Rust advantages for concurrent agent systems (MEDIUM confidence)

---
*Architecture research for: Autonomous AI agent harness (Ouroboros/ouro)*
*Researched: 2026-02-03*
