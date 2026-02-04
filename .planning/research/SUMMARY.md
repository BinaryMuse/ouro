# Project Research Summary

**Project:** Ouroboros (Autonomous AI Agent Harness)
**Domain:** Local LLM-powered autonomous agent system with infinite exploration loop
**Researched:** 2026-02-03
**Confidence:** HIGH

## Executive Summary

Ouroboros is an autonomous AI agent harness built on Rust, Ollama (local LLM inference), and ratatui (terminal UI). It inverts the typical agent framework model: instead of shipping with memory infrastructure, planning engines, and tool libraries, it provides only the bare minimum (system prompt loading, LLM integration, basic tools, and a monitoring TUI). The agent must bootstrap its own persistence, memory strategies, and task decomposition patterns by exploring open-endedly and self-organizing. This "infinite exploration loop" design is philosophically distinct from task-oriented frameworks like AutoGPT, CrewAI, and LangGraph.

The recommended approach leverages Rust's memory safety and async runtime (tokio) for rock-solid long-running operation, the genai crate for multi-provider LLM integration (focused on Ollama), and ratatui for zero-cost real-time monitoring. The architecture follows an actor-per-component pattern with message-passing via tokio channels, ensuring zero shared mutable state and clean shutdown behavior. Context window management is critical: local models degrade before hitting token limits, requiring observation masking and periodic context refresh. Sub-agent orchestration demands hard resource limits to prevent VRAM exhaustion on consumer hardware.

The primary risks are repetitive action loops (LLM gets stuck), uncontained shell access (security), and context rot (quality degradation). Mitigations include loop detection with action hashing, OS-level sandboxing (Seatbelt/Bubblewrap) from day one, and observation masking for context management. The system must handle ungraceful shutdowns and context window restarts as first-class concerns, not edge cases.

## Key Findings

### Recommended Stack

Rust Edition 2024 with tokio async runtime provides the foundation for a long-running autonomous system that spawns sub-processes and manages context windows. The genai crate (0.5.3+) offers ergonomic multi-provider LLM integration with native Ollama support and tool calling (function calling works with Llama 3.1+, Mistral, Qwen2.5). Ratatui (0.30) delivers sub-millisecond immediate-mode terminal rendering for the real-time monitoring dashboard.

**Core technologies:**
- **Rust (Edition 2024, 1.85.0+)**: Memory safety without GC for long-running agent loop with sub-processes
- **genai (0.5.3+)**: Multi-provider LLM client with Ollama-native support and tool calling API
- **tokio (1.49)**: Async runtime with work-stealing scheduler, process spawning, timers, channels
- **ratatui (0.30)**: Terminal UI framework with constraint-based layouts and rich widget library
- **crossterm (0.29)**: Cross-platform terminal backend with async event streams for tokio

**Supporting libraries:**
- serde/serde_json (1.0): Serialization for configs, tool schemas, state persistence
- reqwest (0.13): HTTP client for web tools (reused by genai internally)
- tracing/tracing-subscriber (0.1/0.3): Structured logging for debugging agent loop and tool execution
- anyhow (1.1): Top-level error propagation for agent operations
- thiserror (1.6): Typed error definitions for library-layer boundaries
- clap (4.5): CLI argument parsing with derive macros

**Critical version requirement:** Ollama models default to 4096-token context regardless of trained capacity. Must explicitly set `num_ctx` to 32K-64K for agent workloads. For tool calling, use Ollama with models that support it (Llama 3.1 8B+, Qwen2.5, Mistral 7B-Instruct).

### Expected Features

**Must have (table stakes):**
- **Agent loop (plan-act-observe-refine)** — Every agent framework implements this. Core execution cycle. Without it, there is no agent.
- **Shell/command execution** — Agent's hands. Every coding agent provides this (OpenHands bash, Claude Code terminal, SWE-agent ACI).
- **File system access (read/write)** — Workspace on disk is fundamental to Ouroboros's design. Agent persists artifacts.
- **Context window management** — LLMs have finite context. Harness must manage what's in context, summarize/drop old content, or the agent degrades after a few iterations. Critical for infinite loop operation.
- **System prompt loading (SYSTEM_PROMPT.md)** — The one guarantee. Bootstrapping contract.
- **Ollama model API integration** — Interface to the brain. Must handle streaming, tool calls, errors, timeout/retry.
- **Error recovery and restart** — Agents crash. Infinite loop demands checkpoint/restore. Salesforce and Anthropic harness research emphasizes lifecycle management.
- **Conversation/session management** — Flow of messages between harness and model. Backbone of agent loop.
- **Tool calling interface** — Mechanism for agent to invoke tools. Ollama follows OpenAI function calling format.
- **Basic logging** — Foundation for TUI. Log agent thoughts, tool calls, results, errors.

**Should have (competitive differentiators):**
- **Agent-bootstrapped persistence** (THE core differentiator) — No other framework makes the agent build its own memory. AutoGPT ships vector DB, CrewAI ships 4 memory types, LangGraph ships cross-thread stores. Ouroboros ships NOTHING — agent must figure out how to survive context restarts. Philosophical core: harness provides blank workspace, agent invents its own memory. Complexity is LOW for harness (deliberately NOT building infrastructure).
- **Infinite exploration loop** — Most frameworks are task-oriented. Ouroboros runs forever exploring open-endedly. AutoGPT is closest (autonomous goal pursuit) but still goal-bounded.
- **Sub-agent spawning (agent-controlled)** — While Claude Code, CrewAI, LangGraph support sub-agents, they're framework-orchestrated. In Ouroboros, the agent itself decides when to spawn and what to delegate. Agent builds its own organizational structure.
- **Ratatui TUI (real-time multi-panel dashboard)** — Four panels: main agent log, sub-agent tree visualization, flagged discoveries, progress overview. Most frameworks are headless or basic web UIs. Terminal-native is unique.
- **Discovery flagging system** — No other framework has "agent found something interesting and flags it for human." Turns passive monitoring into active curation.
- **Workspace-as-memory philosophy** — Instead of vector DBs or knowledge graphs, workspace IS memory. Files on disk. Agent uses same tools for persistence as for everything else. Mirrors how humans use notebooks/files/folders.
- **Local-only / privacy-first operation** — No data leaves machine. Most frameworks default to cloud APIs. Ouroboros is local-first by design.

**Defer (v2+):**
- **Multi-model support** — Different Ollama models for different tasks. Defer because single-model is simpler to debug.
- **Remote/distributed operation** — Harness on one machine, Ollama on another. Defer because local-only is the v1 story.
- **Agent marketplace / sharing** — Share configs, agent-built tools, discoveries. Way too early.

### Anti-Features (Deliberately NOT Building)

Critical distinction: The agent bootstraps its own tools and memory. Ouroboros should explicitly NOT provide:

- **Built-in vector database / RAG memory** — If harness provides memory, agent never learns to build its own. The entire point is agent figures out persistence.
- **Built-in knowledge graph** — Agent should discover need for structured knowledge representation and build it.
- **Pre-built tool library** — Provide MINIMAL tools (shell, file I/O, web access, sub-agent spawn). Agent builds additional tools as needed.
- **Pre-defined agent roles / personas** — Agent should discover what roles it needs and define them itself.
- **Goal decomposition / task planning engine** — Agent should learn to decompose its own exploration goals. Built-in planner constrains emergent behavior.
- **Conversation memory summarization (automatic)** — Agent should discover context window limitation and figure out how to cope. If harness auto-summarizes, agent never learns context management.
- **Human-in-the-loop approval gates** — Designed for autonomous exploration. Safety from sandboxing, not interrupting the loop.

### Architecture Approach

The architecture follows an **actor-per-component pattern** with message-passing via tokio mpsc/oneshot/watch channels. One Coordinator actor owns all mutable state; every other component communicates through it. This eliminates shared mutable state and prevents data races at the architecture level. The agent loop, tool executors, sub-agent supervisor, TUI renderer, and pause controller each run as independent tokio tasks. The Coordinator routes messages and publishes state snapshots via watch channels for the TUI.

**Major components:**
1. **Coordinator Actor** — Central hub: routes messages, owns canonical state (AgentState, SubAgentRegistry, DiscoveryLog), enforces pause/resume. Single tokio task with mpsc::Receiver run loop.
2. **Agent Loop Actor** — Infinite LLM conversation cycle: build ChatRequest, call Ollama via genai, parse ChatResponse, dispatch tool calls. Owns chat history Vec<ChatMessage>. Communicates with Coordinator for tool requests (oneshot response) and status updates.
3. **Tool Executor** — Executes tool calls (shell, file, web, search, sleep) with guardrails. Pool of tokio tasks, one per concurrent tool execution. Uses tokio::process for shell (non-blocking), tokio::fs for file I/O, reqwest for HTTP.
4. **Sub-Agent Supervisor** — Spawns, tracks, terminates sub-agents (LLM sessions + background processes). Owns HashMap<SubAgentId, JoinHandle>. Manages child mpsc senders and lifecycle events.
5. **TUI Renderer** — Renders four panels (agent log, sub-agent tree, discoveries, progress), captures keyboard input, dispatches user actions. Uses ratatui + crossterm EventStream. Subscribes to Coordinator state via watch channels.
6. **Context Manager** — Tracks token usage, triggers context window restart when limit approached. Implements observation masking (replace old tool output with placeholders while preserving reasoning). Embedded in Agent Loop actor, not a separate task.
7. **Pause/Resume Controller** — Timer-based, event-based, user-controlled pause logic. Uses tokio::sync::watch channel broadcasting pause state.

**Key patterns:**
- **Request-response via mpsc + oneshot:** Agent Loop sends ToolRequest with oneshot::Sender through Coordinator. Tool executor sends result back via oneshot. Agent awaits exactly one result.
- **Watch channels for TUI state broadcasting:** Coordinator publishes read-only state snapshot. TUI subscribes and re-renders when state changes. Non-blocking for sender, TUI gets latest state.
- **Shutdown via channel drop propagation:** User sends Quit → Coordinator drops state including mpsc::Sender handles → child actors detect closure → Sub-Agent Supervisor aborts tasks → TUI exits and restores terminal → main awaits JoinHandles.

**Context window strategy:** Primary approach is observation masking (JetBrains NeurIPS 2025 research: outperformed LLM summarization in 4/5 configurations, 52% cheaper). Replace old tool output with compact placeholders while preserving agent reasoning. Secondary approach is hard restart: reset history to [SYSTEM_PROMPT.md, "Previous session findings: {discoveries}"] when approaching 80% of limit.

### Critical Pitfalls

Research identified 7 critical pitfalls based on real-world failures in agent systems:

1. **Agent stuck in repetitive action loops** — Local models prone to repeating same tool call indefinitely (neural text degeneration). MAST taxonomy found this in 41-87% of multi-agent traces across 7 frameworks. **Prevention:** Implement repetition detector (hash recent actions, flag if same action repeats 3x), hard iteration cap per cycle (max 50 tool calls), progress metric (compare workspace state before/after). Plan-then-Execute architecture reduces tight loop that enables degeneration. Must be present from first prototype.

2. **Uncontained shell access leading to system damage** — Agent executes commands that escape workspace boundary or consume unbounded resources. Documented: writing system configs, spawning orphan processes, exhausting disk, rm -rf outside workspace, network exfiltration. **Prevention:** OS-level sandboxing is MANDATORY (macOS: Seatbelt/sandbox-exec, Linux: Bubblewrap/Firejail/namespaces). Filesystem allowlist (block writes outside workspace, block ~/.ssh, ~/.bashrc, ~/.config). Network isolation. Process limits (cgroups/ulimits). Command denylist as defense-in-depth. Must be implemented Phase 1, before agent loop runs.

3. **Ollama default context window silently cripples agent** — Ollama defaults to 4096 tokens regardless of model's trained capacity. Llama 3.1 trained on 128K gets limited to 4K. Truncation is silent — agent loses system prompt, prior findings, task context. Agent exhibits amnesia: forgets instructions, repeats work, hallucinates. **Prevention:** Explicitly set `num_ctx` in every Ollama model config/API call (never rely on defaults). For agent workloads: minimum 16K, preferably 32K-64K. Monitor token usage per request. Implement context budgeting (reserve portions for system prompt, recent tool output, historical context). Set OLLAMA_CONTEXT_LENGTH environment variable system-wide.

4. **Sub-agent spawning without resource limits causes exhaustion** — Agent spawns sub-agents to parallelize, but each loads its own Ollama model instance, exhausting VRAM/RAM. On 16GB machine, two concurrent 8B models with 32K context can consume all memory. Orphaned sub-agents (PPID=1) persist after harness crash, consuming ~200MB each. **Prevention:** Hard cap on concurrent sub-agents (2-3 max for consumer hardware). Use Ollama's OLLAMA_NUM_PARALLEL for concurrency instead of loading separate model instances. Proper process lifecycle: track child PIDs, SIGCHLD handlers, cleanup-on-exit (SIGTERM → SIGKILL after timeout). Use process groups (setpgid/killpg) to prevent orphans.

5. **Context rot degrades agent quality before hitting token limit** — Agent output quality degrades silently as context fills, even within technical limit. Chroma research: effective context window is often <256K for frontier models, far less for local 7B-13B models. "Lost in the Middle" effect: LLMs miss information in middle of context. No error signal — model continues generating confident but degraded output. **Prevention:** Define "pre-rot threshold" for each model (assume effective context is ~50-60% of configured num_ctx). Implement context compaction hierarchy: keep recent tool calls raw, compress older to summaries, extract key findings to structured memory. Pin critical info (system prompt at start, key findings at end for highest attention). Periodic "context refresh" (write findings to disk, clear context, re-read from disk — the Ralph Loop pattern).

6. **TUI render loop blocks agent execution** — TUI and agent loop compete for event loop/process resources. Synchronous or too-frequent TUI rendering blocks agent tool calls. Agent operations blocking event loop freeze TUI. Uncontrolled state updates from agent cause excessive re-renders. **Prevention:** Separate agent loop and TUI into different processes (or worker threads). Communicate via IPC, not shared state. Throttle TUI updates (batch state changes, render at fixed 100ms interval = ~10fps). Implement scrollback buffer (only render visible portion). Graceful shutdown (register handlers for SIGINT/SIGTERM/uncaughtException, restore terminal state). Log to file as primary, TUI as secondary.

7. **Agent fails to bootstrap persistence after context reset** — Agent designed to bootstrap from workspace files, but bootstrap consumes significant context tokens. If workspace files are disorganized/too large, agent either fails to load critical context or fills context during bootstrap, leaving no room for work. Agent starts from scratch every cycle, making no cumulative progress. **Prevention:** Design structured persistence format from day one (single well-known STATE.md or state.json with fixed schema, <2K tokens). Separate hot state (current objective, recent findings, next steps) from cold storage (full logs, raw output). Implement bootstrap budget (reserve 4K tokens out of 32K for bootstrap). Test bootstrap path explicitly (run, kill, restart, verify agent picks up where it left off). Version state file (cycle counter + timestamp).

## Implications for Roadmap

Based on research, suggested phase structure follows dependency graph and risk mitigation:

### Phase 1: Foundation & Security (Sandboxing + Core Infrastructure)
**Rationale:** Pitfall #2 (uncontained shell access) is CRITICAL and must be addressed before agent loop runs. Establishing unsafe patterns in development is dangerous. The sandbox is the foundation everything else is built on. Additionally, the Coordinator message enum defines all interfaces upfront — this is the contract between every component and must be designed carefully before writing any actor logic.

**Delivers:**
- OS-level sandboxing (Seatbelt on macOS, Bubblewrap/Firejail on Linux)
- Filesystem allowlist (workspace-only writes)
- Network isolation configuration
- Process limits (cgroups/ulimits)
- Configuration module with guardrails
- Error type definitions (thiserror)
- Coordinator message enum (defines all actor interfaces)

**Addresses:** Pitfall #2 (uncontained shell access)

**Research flag:** Needs specific platform research for macOS Seatbelt profiles vs Linux namespace configuration. Sandbox testing patterns.

---

### Phase 2: Core Agent Loop (Ollama Integration + Basic Tools)
**Rationale:** The agent loop is the heart of the system. Must be built with context window awareness from the start (Pitfall #3). This phase establishes the actor pattern, Ollama API integration with explicit num_ctx configuration, and the minimal tool set (shell, file I/O). System prompt loading is the one guarantee Ouroboros provides.

**Delivers:**
- Coordinator actor skeleton (mpsc message routing)
- Agent Loop actor (call Ollama, parse response)
- System prompt loading (SYSTEM_PROMPT.md)
- Ollama integration via genai (with explicit num_ctx=32K minimum)
- Tool dispatcher module
- Shell execution tool (workspace-scoped, within sandbox)
- File read/write tools (path validation)
- Tool calling interface (genai tool definitions)
- Basic logging infrastructure (tracing)

**Uses:** genai (0.5.3), tokio (1.49), serde/serde_json, tracing, anyhow

**Implements:** Agent Loop Actor, Tool Executor, partial Coordinator

**Addresses:** Pitfalls #3 (Ollama context window), table stakes features (agent loop, shell, file access, model API, tool calling, logging)

**Research flag:** Standard patterns. Skip research-phase. Reference genai docs and Ollama API docs during implementation.

---

### Phase 3: Loop Safety & Context Management
**Rationale:** Pitfall #1 (repetitive action loops) and Pitfall #5 (context rot) will surface as soon as the agent runs for extended periods. These must be designed into the core loop, not retrofitted. Context management is identified in research as HIGH complexity table stakes feature.

**Delivers:**
- Repetition detector (action hashing, 3x repetition threshold)
- Hard iteration cap per cycle (configurable, default 50)
- Progress metric tracking (workspace state comparison)
- Context Manager module (token counting, observation masking)
- Context budgeting (reserved portions for system prompt, tools, history)
- Agent loop restart logic (context refresh with findings carry-over)
- Error recovery with checkpoint/restart

**Implements:** Context Manager (embedded in Agent Loop), loop safety mechanisms

**Addresses:** Pitfalls #1 (repetitive loops), #5 (context rot), table stakes (context window management, error recovery)

**Research flag:** Context management strategies are well-documented in research. Reference JetBrains NeurIPS 2025 paper on observation masking. Standard implementation pattern.

---

### Phase 4: Agent-Bootstrapped Persistence
**Rationale:** Pitfall #7 (bootstrap failure) is directly related to the core differentiator (agent-bootstrapped persistence). The persistence format must be designed before the agent runs multi-cycle sessions. This is architectural, not a later optimization. The workspace-as-memory philosophy depends on this working correctly.

**Delivers:**
- Structured persistence format (STATE.md or state.json schema)
- Hot state vs cold storage separation
- Bootstrap budget tracking (4K token limit for state loading)
- State file versioning (cycle counter + timestamp)
- Bootstrap path testing infrastructure
- Workspace discovery log for flagged findings

**Implements:** Persistence layer (agent reads/writes via file tools)

**Addresses:** Pitfall #7 (bootstrap failure), differentiator features (agent-bootstrapped persistence, workspace-as-memory)

**Research flag:** Skip research-phase. This is novel to Ouroboros, so less about research and more about design/implementation. Reference FEATURES.md anti-features section.

---

### Phase 5: TUI Dashboard
**Rationale:** Pitfall #6 (TUI blocking agent) requires architectural separation decided upfront. The TUI is built on the logging/state infrastructure from Phase 2-3. This is HIGH complexity table stakes feature and HIGH value differentiator. Can be developed in parallel with Phase 4 after Phase 3 completes.

**Delivers:**
- TUI module structure (mod.rs, app.rs, events.rs, actions.rs)
- Terminal setup/teardown with graceful restoration
- Four-panel layout (agent log, sub-agent tree, discoveries, progress)
- Watch channel subscription for state updates
- Keyboard event handling (crossterm EventStream)
- Throttled rendering (100ms interval, ~10fps)
- Scrollback buffer for logs (render only visible portion)
- Process separation (TUI in separate task, communicates via channels)
- Graceful shutdown handlers (SIGINT/SIGTERM, restore terminal)

**Uses:** ratatui (0.30), crossterm (0.29), tokio::sync::watch

**Implements:** TUI Renderer actor

**Addresses:** Pitfall #6 (TUI blocking agent), differentiator features (ratatui TUI dashboard), table stakes (basic logging → visualization)

**Research flag:** Ratatui patterns are well-documented. Reference official docs and async event stream tutorial. Standard implementation.

---

### Phase 6: Sub-Agent Orchestration
**Rationale:** Pitfall #4 (resource exhaustion) is specific to sub-agent spawning. This phase builds on the core loop (Phase 2-3) and requires resource management designed from the start. Sub-agent spawning is a HIGH complexity differentiator feature. Process lifecycle management patterns from Phase 1 (sandboxing) inform this phase.

**Delivers:**
- Sub-Agent Supervisor actor
- Sub-agent registry (HashMap<SubAgentId, JoinHandle>)
- Hard cap on concurrent sub-agents (2-3 max, configurable)
- Process lifecycle management (track child PIDs, SIGCHLD handlers)
- Process groups (setpgid/killpg) to prevent orphans
- Cleanup-on-exit (SIGTERM → SIGKILL after timeout)
- Memory budget checking (query Ollama requirements before spawn)
- Circuit breaker (stop spawning after 3 consecutive failures)
- Spawn sub-agent tool (agent calls to create children)
- Sub-agent tree visualization panel (TUI)

**Uses:** tokio::process, tokio::task::JoinHandle

**Implements:** Sub-Agent Supervisor actor, sub-agent spawn tool

**Addresses:** Pitfall #4 (resource exhaustion), differentiator features (sub-agent spawning agent-controlled, sub-agent tree panel)

**Research flag:** Needs research on process group management patterns and Ollama memory estimation. Resource limits and cleanup are critical. Medium complexity.

---

### Phase 7: Enhanced Tools & Discovery System
**Rationale:** After core loop, persistence, TUI, and sub-agents are stable, add the remaining differentiator features (discovery flagging, web fetch) and enhanced tools (search). These are MEDIUM value, MEDIUM/LOW complexity.

**Delivers:**
- Web fetch tool (reqwest + scraper for HTML parsing)
- Internet search tool integration (API selection: SearXNG, Brave Search)
- Discovery flagging tool (agent calls to mark noteworthy findings)
- Discovery panel in TUI (flagged items display)
- Sleep/pause tool (agent-controlled pausing)
- Improved context strategies (sliding window, selective injection, priority-based retention)

**Uses:** reqwest (0.13), scraper (0.25)

**Implements:** Additional tools, discovery storage

**Addresses:** Differentiator features (discovery flagging, web fetch), deferred table stakes (web fetch tool, advanced context strategies)

**Research flag:** Search API integration needs research on available options and rate limiting. Web scraping patterns are standard.

---

### Phase 8: Pause/Resume & Advanced Control
**Rationale:** This is the final polish phase. Pause/Resume Controller is LOW complexity and can be built after core functionality is proven. User experience improvements (pause command, resource usage display) round out the system.

**Delivers:**
- Pause/Resume Controller actor
- Pause strategy types (timer-based, event-based, user-triggered)
- watch channel for pause state broadcasting
- Agent loop gating (check pause state before each cycle)
- TUI status bar with resource usage (memory, tokens, cycle count)
- User action handlers (pause, resume, focus, quit)
- Headless mode (replace TUI with logger for CI/batch)

**Implements:** Pause/Resume Controller actor

**Addresses:** UX pitfalls (no way to interrupt agent, no resource usage visibility)

**Research flag:** Skip research-phase. Standard patterns for timer-based control and user input handling.

---

### Phase Ordering Rationale

- **Phase 1 first:** Security cannot be compromised. Sandbox before any shell access. Coordinator message enum defines all interfaces.
- **Phase 2 depends on 1:** Agent loop needs sandboxed shell tool. Ollama integration with explicit num_ctx is foundational.
- **Phase 3 depends on 2:** Loop safety and context management require a working agent loop to protect.
- **Phase 4 depends on 3:** Persistence bootstrapping requires context management and restart logic.
- **Phase 5 depends on 2-3, parallel with 4:** TUI visualizes agent state and logs from Phase 2-3. Can be built in parallel with persistence.
- **Phase 6 depends on 2-3:** Sub-agents are instances of the agent loop with resource management on top.
- **Phase 7 depends on 2-6:** Enhanced tools and discovery system build on stable core infrastructure.
- **Phase 8 depends on all:** Pause/resume and polish touch all components.

**Grouping logic:** Phases 1-3 are the critical path (security, core loop, safety). Phases 4-5 can partially parallelize (persistence + TUI). Phase 6 (sub-agents) is the most complex orchestration piece. Phases 7-8 are enhancements and polish.

**Pitfall avoidance:** The ordering directly addresses pitfalls in priority: #2 (Phase 1), #3 (Phase 2), #1 and #5 (Phase 3), #7 (Phase 4), #6 (Phase 5), #4 (Phase 6).

### Research Flags

**Phases needing deeper research during planning:**
- **Phase 1 (Sandboxing):** Platform-specific sandbox configuration (Seatbelt profiles on macOS, namespace/cgroup setup on Linux). Testing patterns for verifying sandbox effectiveness.
- **Phase 6 (Sub-Agents):** Process group management, Ollama memory estimation formulas, resource limit tuning for different hardware configurations.
- **Phase 7 (Search API):** Available search API options, rate limiting strategies, cost/quota management.

**Phases with standard patterns (skip research-phase):**
- **Phase 2 (Core Loop):** Well-documented patterns in genai docs, Ollama API docs, tokio actor tutorials.
- **Phase 3 (Context Management):** Research already provides clear guidance (observation masking, Ralph Loop pattern).
- **Phase 4 (Persistence):** Novel to Ouroboros, but design-driven rather than research-driven.
- **Phase 5 (TUI):** Ratatui has comprehensive docs and async event stream tutorials.
- **Phase 8 (Pause/Resume):** Standard timer and control patterns.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | Core technologies verified via official sources (genai GitHub, ratatui docs, tokio docs). Version compatibility cross-referenced with crates.io. genai tool calling status confirmed in CHANGELOG.md and issue #24. Only MEDIUM confidence on scraper (version from search, not official source). |
| Features | MEDIUM-HIGH | Table stakes synthesized across many well-documented frameworks (AutoGPT, CrewAI, LangGraph, OpenHands, Claude Code, Aider). Differentiators are philosophically distinct but grounded in real framework capabilities. LOW confidence on some frontier self-evolution patterns (still research-stage). MVP definition is clear. |
| Architecture | HIGH | Architecture patterns from official Tokio docs (channels tutorial, Alice Ryhl's actors article), ratatui async event stream tutorial, genai crate API docs. Actor-per-component, request-response via oneshot, watch channels for state broadcasting are all documented patterns. Context management strategy grounded in JetBrains NeurIPS 2025 research (observation masking). |
| Pitfalls | HIGH | Multiple sources cross-referenced: MAST taxonomy (Cemri et al., 2025 arXiv), NVIDIA sandboxing guidance, Anthropic Claude Code sandboxing, Chroma context rot research, documented real-world issues (KiloCode #2936, Gastown #29, SuperAGI #542). Each pitfall has multiple confirming sources. Prevention strategies are concrete and actionable. |

**Overall confidence:** HIGH

Research is comprehensive and grounded in authoritative sources. The domain is well-explored (many agent frameworks to learn from), and the technical stack is mature (Rust ecosystem, Ollama, ratatui). The main unknowns are in novel aspects (agent-bootstrapped persistence philosophy) which are intentional design choices rather than knowledge gaps.

### Gaps to Address

- **Ollama model memory requirements:** Need specific formulas for calculating VRAM/RAM requirements per model size, quantization level, and context window size. This affects Phase 6 (sub-agent resource limits). Formula exists (KV cache = ~4.5GB for 8B model at 32K context FP16), but needs validation across quantization levels and hardware configurations.

- **Platform-specific sandbox configuration:** Seatbelt (macOS) and Bubblewrap/Firejail (Linux) have different APIs and capabilities. Phase 1 needs platform-specific implementation research. Cross-platform abstraction may not be feasible — accept platform-specific code paths.

- **Search API selection:** Multiple options (SearXNG self-hosted, Brave Search API, others). Phase 7 needs evaluation of rate limits, cost, ease of integration. Deferred to phase planning.

- **Context window "pre-rot threshold" calibration:** Research suggests ~50-60% of configured num_ctx for local models, but this likely varies by model size and architecture. Phase 3 should include empirical testing with target models (Llama 3.1 8B, Mistral 7B, Qwen2.5) to calibrate thresholds.

- **Agent-bootstrapped persistence format:** No prior art for this approach. Phase 4 is design-driven. Key questions: JSON vs Markdown for STATE file? Fixed schema vs flexible? How to version schema evolution? These are implementation decisions, not research gaps, but warrant explicit design review.

## Sources

### Primary (HIGH confidence)
- [genai crate - GitHub](https://github.com/jeremychone/rust-genai) — Multi-provider Rust LLM client, Ollama support, tool calling status, version verification
- [genai crate - crates.io](https://crates.io/crates/genai) — Version 0.5.3 verification
- [genai chat module API - docs.rs](https://docs.rs/genai/latest/genai/chat/index.html) — ChatMessage, ChatRequest, Tool, ToolCall types
- [ratatui - GitHub](https://github.com/ratatui/ratatui) — Version 0.30 release, modular workspace, crossterm backend
- [ratatui.rs](https://ratatui.rs/) — Official documentation
- [ratatui async event stream tutorial](https://ratatui.rs/tutorials/counter-async-app/async-event-stream/) — EventHandler, tokio::select!, crossterm EventStream
- [tokio - GitHub](https://github.com/tokio-rs/tokio) — Version 1.49, LTS policy, process module
- [tokio::process docs](https://docs.rs/tokio/latest/tokio/process/index.html) — Command API, kill_on_drop, process groups
- [Tokio channels tutorial](https://tokio.rs/tokio/tutorial/channels) — mpsc, oneshot, broadcast, watch channel patterns
- [Actors with Tokio - Alice Ryhl](https://ryhl.io/blog/actors-with-tokio/) — Actor + Handle pattern, shutdown via channel drop, deadlock avoidance
- [Rust 1.85.0 / Edition 2024 announcement](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/) — Edition 2024 stable since Feb 2025
- [LangGraph 1.0 release](https://www.blog.langchain.com/langchain-langgraph-1dot0/) — LangGraph features
- [LangGraph workflows and agents docs](https://docs.langchain.com/oss/python/langgraph/workflows-agents) — Official docs
- [Claude Code sub-agents documentation](https://code.claude.com/docs/en/sub-agents) — Official docs
- [Anthropic: enabling autonomous Claude Code](https://www.anthropic.com/news/enabling-claude-code-to-work-more-autonomously) — Official
- [Anthropic: effective context engineering for agents](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents) — Official
- [Anthropic: effective harnesses for long-running agents](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents) — Official
- [Anthropic: Claude Code sandboxing](https://www.anthropic.com/engineering/claude-code-sandboxing) — Production-tested Seatbelt + Bubblewrap approach
- [OpenHands (OpenDevin) platform paper](https://arxiv.org/abs/2407.16741) — arXiv paper
- [Ollama GitHub and documentation](https://github.com/ollama/ollama) — Official
- [Ollama Context Length Documentation](https://docs.ollama.com/context-length) — Official documentation
- [Why Do Multi-Agent LLM Systems Fail? (MAST taxonomy, Cemri et al., 2025)](https://arxiv.org/abs/2503.13657) — Peer-reviewed research with 1600+ annotated traces
- [Practical Security Guidance for Sandboxing Agentic Workflows (NVIDIA, 2025)](https://developer.nvidia.com/blog/practical-security-guidance-for-sandboxing-agentic-workflows-and-managing-execution-risk/) — Vendor guidance with technical recommendations
- [JetBrains Research: Efficient Context Management](https://blog.jetbrains.com/research/2025/12/efficient-context-management/) — Observation masking vs LLM summarization, NeurIPS 2025
- [Orphan AI agent process leak (Gastown issue #29, 2026)](https://github.com/steveyegge/gastown/issues/29) — Documented real-world issue
- [LLM Agent Loop Stuck (SuperAGI issue #542)](https://github.com/TransformerOptimus/SuperAGI/issues/542) — Documented real-world issue

### Secondary (MEDIUM confidence)
- [AutoGPT architecture and features](https://builtin.com/artificial-intelligence/autogpt) — Built on multiple 2025 reviews
- [CrewAI framework review](https://latenode.com/blog/ai-frameworks-technical-infrastructure/crewai-framework/crewai-framework-2025-complete-review-of-the-open-source-multi-agent-ai-platform) — 2025 review
- [CrewAI memory systems deep dive](https://sparkco.ai/blog/deep-dive-into-crewai-memory-systems) — Memory types analysis
- [Aider features and architecture](https://www.blott.com/blog/post/aider-review-a-developers-month-with-this-terminal-based-code-assistant) — Practitioner review
- [Ollama tool calling and agent integration](https://www.byteplus.com/en/topic/418052) — Integration guide
- [Context Window Management Strategies (Maxim)](https://www.getmaxim.ai/articles/context-window-management-strategies-for-long-context-ai-agents-and-chatbots/) — Sliding window, summarization, truncation strategies
- [Context Rot research (Chroma/Hong et al., 2025)](https://www.getmaxim.ai/articles/context-window-management-strategies-for-long-context-ai-agents-and-chatbots/) — Measurements across 18 LLMs
- [The Context Window Problem (Factory.ai)](https://factory.ai/news/context-window-problem) — Practitioner insights
- [Ollama Memory Management (DeepWiki)](https://deepwiki.com/ollama/ollama/5.4-memory-management-and-gpu-allocation) — Technical analysis of Ollama internals
- [How Ollama Handles Parallel Requests (Glukhov, 2025)](https://www.glukhov.org/post/2025/05/how-ollama-handles-parallel-requests/) — Practitioner analysis
- [From ReAct to Ralph Loop (Alibaba, 2025)](https://www.alibabacloud.com/blog/from-react-to-ralph-loop-a-continuous-iteration-paradigm-for-ai-agents_602799) — Production-informed architectural pattern
- [Why Your Multi-Agent System is Failing (Towards Data Science, 2025)](https://towardsdatascience.com/why-your-multi-agent-system-is-failing-escaping-the-17x-error-trap-of-the-bag-of-agents/) — Practitioner analysis
- [AI Agents Deleting Home Folders? Run Your Agent in Firejail (SES, 2025)](https://softwareengineeringstandard.com/2025/12/15/ai-agents-firejail-sandbox/) — Practical guide
- [Reliability for Unreliable LLMs (Stack Overflow, 2025)](https://stackoverflow.blog/2025/06/30/reliability-for-unreliable-llms/) — Practitioner guidance
- [Salesforce: what is an agent harness](https://www.salesforce.com/agentforce/ai-agents/agent-harness/?bc=OTH) — Conceptual overview
- [AI agent safety guardrails 2025](https://skywork.ai/blog/agentic-ai-safety-best-practices-2025-enterprise/) — Best practices
- [METR task horizon findings](https://ajithp.com/2025/06/30/ai-native-memory-persistent-agents-second-me/) — References METR March 2025 report
- [Top agentic AI frameworks 2026](https://www.alphamatch.ai/blog/top-agentic-ai-frameworks-2026) — Framework comparison
- [AI observability tools buyer's guide 2026](https://www.braintrust.dev/articles/best-ai-observability-tools-2026) — Observability patterns
- [Red Hat: Agentic AI Developers Moving to Rust](https://developers.redhat.com/articles/2025/09/15/why-some-agentic-ai-developers-are-moving-code-python-rust) — Rust advantages for concurrent agent systems
- [TUI Core component framework (GitHub)](https://github.com/AstekGroup/tui-core) — Component trait, action pattern
- [Snyk: From SKILL.md to Shell Access in Three Lines of Markdown](https://snyk.io/articles/skill-md-shell-access/) — Security vendor with attack demonstration
- [Self-evolving AI agents survey](https://arxiv.org/abs/2508.07407) — arXiv survey paper
- [Hierarchical multi-agent systems](https://arxiv.org/html/2506.12508v1) — arXiv paper
- Various crates.io pages (crossterm, reqwest, scraper, serde, tracing, clap, notify, etc.) — Version verification

### Tertiary (LOW confidence)
- [Agent scaffolding concepts](https://zbrain.ai/agent-scaffolding/) — Single source conceptual overview

---
*Research completed: 2026-02-03*
*Ready for roadmap: yes*
