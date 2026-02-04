# Requirements: Ouroboros

**Defined:** 2026-02-04
**Core Value:** A local AI agent can autonomously explore, build its own tools, develop its own memory/persistence, and sustain itself across context window restarts — with minimal human scaffolding.

## v1 Requirements

Requirements for initial release. Each maps to roadmap phases.

### Agent Loop

- [ ] **LOOP-01**: Harness runs an infinite agent loop calling a configurable local Ollama model via genai crate
- [ ] **LOOP-02**: Harness loads SYSTEM_PROMPT.md from the agent's workspace as the system prompt on each session start/restart
- [ ] **LOOP-03**: Harness tracks token usage and applies observation masking when context approaches the model's limit
- [ ] **LOOP-04**: When context window fills, harness restarts the agent session with SYSTEM_PROMPT.md — agent must bootstrap its own persistence from workspace files
- [ ] **LOOP-05**: Agent can call tools via genai's tool calling interface; harness dispatches and returns results

### Tools

- [ ] **TOOL-01**: Agent can execute shell commands scoped to its workspace directory with configurable timeout and output size limits
- [ ] **TOOL-02**: Agent can read files within its workspace
- [ ] **TOOL-03**: Agent can write/create files within its workspace
- [ ] **TOOL-04**: Agent can fetch web pages via HTTP and receive extracted content
- [ ] **TOOL-05**: Agent can search the internet via the websearch crate
- [ ] **TOOL-06**: Agent can pause itself via a sleep/wait tool (timer-based resume, event-based resume, or user-controlled resume)

### Sub-Agents

- [ ] **AGENT-01**: Agent can spawn child LLM chat sessions (sub-agents) that run concurrently via Ollama
- [ ] **AGENT-02**: Agent can spawn background shell processes that run independently
- [ ] **AGENT-03**: Harness tracks all sub-agents and background processes with status (running, completed, failed)
- [ ] **AGENT-04**: Harness cleans up sub-agents and background processes on shutdown (no orphan processes)

### TUI Dashboard

- [ ] **TUI-01**: Ratatui-based terminal UI displays a scrollable main agent log (thoughts, tool calls, results)
- [ ] **TUI-02**: TUI displays a tree view of active sub-agents and background tasks with status
- [ ] **TUI-03**: TUI displays a panel of agent-flagged discoveries (interesting findings, unexpected results, promising leads)
- [ ] **TUI-04**: TUI displays high-level progress overview
- [ ] **TUI-05**: User can pause/resume the agent loop from the TUI
- [ ] **TUI-06**: User can scroll, navigate, and inspect agent state via keyboard controls
- [ ] **TUI-07**: TUI runs independently of the agent loop (neither blocks the other)

### Discovery & Logging

- [ ] **LOG-01**: Agent can flag findings as noteworthy via a discovery tool, with title and description
- [ ] **LOG-02**: All agent actions, tool calls, and results are written to structured append-only log files
- [ ] **LOG-03**: Sub-agent and background task output is captured in separate log streams

### Safety & Guardrails

- [ ] **SAFE-01**: All file operations are restricted to the agent's workspace directory (path traversal blocked)
- [ ] **SAFE-02**: Shell commands cannot use sudo or other privilege escalation
- [ ] **SAFE-03**: Destructive shell patterns are blocked (e.g., rm -rf /, writes outside workspace)
- [ ] **SAFE-04**: Shell commands enforce a configurable timeout (kill on timeout)

### Configuration

- [ ] **CONF-01**: User can specify the Ollama model name via CLI argument or config file
- [ ] **CONF-02**: User can specify the workspace directory path
- [ ] **CONF-03**: User can configure shell timeout, context window limits, and other operational parameters

## v2 Requirements

Deferred to future release. Tracked but not in current roadmap.

### Loop Safety

- **LOOP-V2-01**: Repetitive action loop detection (hash recent actions, break degenerate patterns)
- **LOOP-V2-02**: Configurable per-cycle iteration cap

### Resource Management

- **RES-01**: Cap on concurrent sub-agents (configurable)
- **RES-02**: Web request rate limiting
- **RES-03**: Process memory and output size limits

### Enhanced Context

- **CTX-01**: Sliding window and priority-based context retention strategies
- **CTX-02**: Agent-accessible token count tool (agent can query remaining context budget)

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| Built-in memory/RAG/vector DB | Agent bootstraps its own persistence — that's the core experiment |
| Pre-built tool library beyond core tools | Agent builds its own tools from shell + file access |
| Goal decomposition / task planning engine | Agent develops its own exploration strategy |
| Container/Docker sandboxing | Adds complexity; workspace scoping sufficient for v1 |
| Multi-model orchestration | Single configurable model for v1 |
| Web dashboard or GUI | TUI only — simplicity via ratatui |
| Human-in-the-loop approval gates | Autonomous exploration; safety via sandboxing, not interruption |
| Structured output schemas | Let the agent communicate however it wants |
| Token/cost tracking dashboard | Running locally on Ollama, not a cost concern |
| Pre-defined agent roles/personas | Agent decides its own organizational structure |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| LOOP-01 | — | Pending |
| LOOP-02 | — | Pending |
| LOOP-03 | — | Pending |
| LOOP-04 | — | Pending |
| LOOP-05 | — | Pending |
| TOOL-01 | — | Pending |
| TOOL-02 | — | Pending |
| TOOL-03 | — | Pending |
| TOOL-04 | — | Pending |
| TOOL-05 | — | Pending |
| TOOL-06 | — | Pending |
| AGENT-01 | — | Pending |
| AGENT-02 | — | Pending |
| AGENT-03 | — | Pending |
| AGENT-04 | — | Pending |
| TUI-01 | — | Pending |
| TUI-02 | — | Pending |
| TUI-03 | — | Pending |
| TUI-04 | — | Pending |
| TUI-05 | — | Pending |
| TUI-06 | — | Pending |
| TUI-07 | — | Pending |
| LOG-01 | — | Pending |
| LOG-02 | — | Pending |
| LOG-03 | — | Pending |
| SAFE-01 | — | Pending |
| SAFE-02 | — | Pending |
| SAFE-03 | — | Pending |
| SAFE-04 | — | Pending |
| CONF-01 | — | Pending |
| CONF-02 | — | Pending |
| CONF-03 | — | Pending |

**Coverage:**
- v1 requirements: 32 total
- Mapped to phases: 0
- Unmapped: 32 ⚠️

---
*Requirements defined: 2026-02-04*
*Last updated: 2026-02-04 after initial definition*
