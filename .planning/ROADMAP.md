# Roadmap: Ouroboros

## Overview

Ouroboros delivers an autonomous AI research harness that runs local Ollama models in an infinite exploration loop. The roadmap progresses from sandboxed execution foundations through the core agent loop, context resilience, real-time TUI monitoring, sub-agent orchestration, and finally extended tools (web, search, discovery). Each phase delivers a coherent, verifiable capability -- the agent gets safer, smarter, more observable, and more capable as phases complete.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [x] **Phase 1: Safety & Configuration** - Workspace-scoped execution guardrails and user-configurable runtime parameters
- [x] **Phase 2: Core Agent Loop & Basic Tools** - Infinite LLM conversation loop with shell, file, and tool-calling support
- [x] **Phase 3: Context Management & Resilience** - Token-aware context window management and graceful session restart
- [x] **Phase 4: TUI Dashboard** - Real-time four-panel terminal interface for monitoring and controlling the agent
- [ ] **Phase 5: Sub-Agent Orchestration** - Agent-controlled child LLM sessions and background processes with lifecycle management
- [ ] **Phase 6: Extended Tools & Discovery** - Web fetching, internet search, sleep/pause, and agent-flagged discovery system

## Phase Details

### Phase 1: Safety & Configuration
**Goal**: The harness enforces workspace-scoped execution boundaries and loads user-specified configuration before any agent code runs
**Depends on**: Nothing (first phase)
**Requirements**: SAFE-01, SAFE-02, SAFE-03, SAFE-04, CONF-01, CONF-02, CONF-03
**Success Criteria** (what must be TRUE):
  1. Shell commands executed through the harness cannot read or write files outside the workspace directory
  2. Shell commands that attempt sudo or other privilege escalation are rejected before execution
  3. Destructive shell patterns (rm -rf /, writes to system paths) are blocked with a clear error
  4. Shell commands that exceed the configured timeout are killed and return an error
  5. User can launch the harness specifying an Ollama model name, workspace path, and operational parameters (timeout, context limit) via CLI or config file
**Plans**: 4 plans

Plans:
- [x] 01-01-PLAN.md -- Scaffold Rust project, config module, and CLI parsing
- [x] 01-02-PLAN.md -- Command blocklist filter (TDD)
- [x] 01-03-PLAN.md -- Workspace boundary guard (TDD)
- [x] 01-04-PLAN.md -- Shell execution with timeout and integration wiring

### Phase 2: Core Agent Loop & Basic Tools
**Goal**: The agent runs an infinite conversation loop against a local Ollama model, calling tools to execute shell commands and read/write files in its workspace
**Depends on**: Phase 1
**Requirements**: LOOP-01, LOOP-02, LOOP-05, TOOL-01, TOOL-02, TOOL-03, LOG-02
**Success Criteria** (what must be TRUE):
  1. The harness connects to a local Ollama model via genai and sustains an ongoing conversation across multiple turns without manual intervention
  2. The agent can call tools (shell, file read, file write) and receive results back in the conversation
  3. SYSTEM_PROMPT.md from the workspace is loaded as the system prompt when the agent session starts
  4. All agent actions, tool calls, and results are written to structured append-only log files on disk
  5. The agent loop runs continuously until the user stops it or the context window fills
**Plans**: 3 plans

Plans:
- [x] 02-01-PLAN.md -- Dependencies, agent module scaffold, and JSONL session logger
- [x] 02-02-PLAN.md -- System prompt loading and tool definitions with dispatch
- [x] 02-03-PLAN.md -- Core agent conversation loop, Ollama health check, and main wiring

### Phase 3: Context Management & Resilience
**Goal**: The harness detects context window pressure and restarts the agent session cleanly, preserving the agent's ability to bootstrap from its workspace
**Depends on**: Phase 2
**Requirements**: LOOP-03, LOOP-04
**Success Criteria** (what must be TRUE):
  1. The harness tracks token usage and applies observation masking (replacing old tool output with compact placeholders) when context approaches the model's limit
  2. When context is exhausted, the harness restarts the agent session with SYSTEM_PROMPT.md and the agent can resume work from workspace files it previously wrote
  3. The agent makes cumulative progress across multiple context window restarts (does not repeat the same work each cycle)
**Plans**: 3 plans

Plans:
- [x] 03-01-PLAN.md -- Config fields for context management and new JSONL log entry types
- [x] 03-02-PLAN.md -- ContextManager module with token tracking, threshold evaluation, and observation masking
- [x] 03-03-PLAN.md -- Agent loop integration with restart loop, carryover, and wind-down

### Phase 4: TUI Dashboard
**Goal**: The user can observe and control the running agent through a rich terminal interface that never blocks agent execution
**Depends on**: Phase 2
**Requirements**: TUI-01, TUI-02, TUI-03, TUI-04, TUI-05, TUI-06, TUI-07
**Success Criteria** (what must be TRUE):
  1. The TUI displays a scrollable log of agent thoughts, tool calls, and results updating in real time
  2. The TUI displays a tree view of active sub-agents and background tasks with their current status
  3. The TUI displays a panel of agent-flagged discoveries and a high-level progress overview
  4. The user can pause and resume the agent loop, scroll through logs, and navigate panels using keyboard controls
  5. The TUI renders smoothly while the agent is actively executing tool calls (neither blocks the other)
**Plans**: 5 plans

Plans:
- [x] 04-01-PLAN.md -- TUI type foundation (events, state, control signals, dependencies)
- [x] 04-02-PLAN.md -- Agent loop refactoring for event emission and pause control
- [x] 04-03-PLAN.md -- TUI rendering widgets (log stream, status bar, tabs, context gauge)
- [x] 04-04-PLAN.md -- TUI main loop, keyboard input, and main.rs launch wiring
- [x] 04-05-PLAN.md -- Human verification of TUI functionality

### Phase 5: Sub-Agent Orchestration
**Goal**: The agent can spawn and manage child LLM sessions and background shell processes, with the harness enforcing lifecycle management and cleanup
**Depends on**: Phase 2, Phase 3
**Requirements**: AGENT-01, AGENT-02, AGENT-03, AGENT-04, LOG-03
**Success Criteria** (what must be TRUE):
  1. The agent can spawn a child LLM chat session that runs concurrently and returns results
  2. The agent can spawn a background shell process that runs independently of the main conversation loop
  3. The harness tracks all sub-agents and background processes, reporting their status (running, completed, failed)
  4. When the harness shuts down, all sub-agents and background processes are terminated cleanly with no orphan processes remaining
  5. Sub-agent and background task output is captured in separate log streams accessible after completion
**Plans**: 5 plans

Plans:
- [ ] 05-01-PLAN.md -- Orchestration types, SubAgentManager registry, and new dependencies
- [ ] 05-02-PLAN.md -- LLM sub-agent spawner and background process spawner
- [ ] 05-03-PLAN.md -- Six new agent tools (spawn, status, result, kill, stdin) with dispatch
- [ ] 05-04-PLAN.md -- Harness wiring (main.rs, runner.rs, agent_loop.rs) and TUI tree panel
- [ ] 05-05-PLAN.md -- Build verification and human verification of TUI and lifecycle

### Phase 6: Extended Tools & Discovery
**Goal**: The agent can fetch web content, search the internet, pause itself, and flag noteworthy findings for the user
**Depends on**: Phase 2
**Requirements**: TOOL-04, TOOL-05, TOOL-06, LOG-01
**Success Criteria** (what must be TRUE):
  1. The agent can fetch a web page by URL and receive extracted text content in the conversation
  2. The agent can search the internet and receive a list of results with titles, URLs, and snippets
  3. The agent can pause itself with a timer-based, event-based, or user-controlled resume mechanism
  4. The agent can flag a finding as noteworthy (with title and description) and it appears in the discoveries panel
**Plans**: TBD

Plans:
- [ ] 06-01: TBD
- [ ] 06-02: TBD
- [ ] 06-03: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 1 -> 2 -> 3 -> 4 -> 5 -> 6
Note: Phase 4 and Phase 6 depend only on Phase 2, so they could execute after Phase 2 in parallel with Phase 3/5 if desired.

| Phase | Plans Complete | Status | Completed |
|-------|---------------|--------|-----------|
| 1. Safety & Configuration | 4/4 | Complete | 2026-02-04 |
| 2. Core Agent Loop & Basic Tools | 3/3 | Complete | 2026-02-04 |
| 3. Context Management & Resilience | 3/3 | Complete | 2026-02-04 |
| 4. TUI Dashboard | 5/5 | Complete | 2026-02-05 |
| 5. Sub-Agent Orchestration | 0/5 | Not started | - |
| 6. Extended Tools & Discovery | 0/3 | Not started | - |
