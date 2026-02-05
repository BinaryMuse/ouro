# Ouroboros

## What This Is

Ouroboros (`ouro`) is an autonomous AI research harness that runs local Ollama models in an infinite exploration loop. The agent gets a workspace on disk, shell access, web tools, and the ability to spawn sub-agents — then free reign to explore AI architecture, data processing patterns, creative generation, simulation, and philosophy. The harness provides the infrastructure (agent loop, TUI monitoring, tool execution); the agent provides the curiosity.

## Core Value

A local AI agent can autonomously explore, build its own tools, develop its own memory and persistence systems, and sustain itself across context window restarts — with minimal human scaffolding.

## Requirements

### Validated

- Harness runs an infinite agent loop against a configurable local Ollama model via genai crate — v1.0
- Agent has a workspace directory it fully owns and can organize however it wants — v1.0
- Agent can execute shell commands scoped to its workspace (no sudo, destructive command blocking) — v1.0
- Agent can read/write files within its workspace — v1.0
- Agent can fetch web documents and search the internet — v1.0
- Agent can spawn sub-agents (both additional LLM chat sessions and background shell processes) — v1.0
- Agent can pause itself via a sleep/wait tool (timer-based, event-based, or user-controlled resume) — v1.0
- Harness loads SYSTEM_PROMPT.md from the workspace as the system prompt on each agent restart — v1.0
- When context window fills, harness restarts the agent session with SYSTEM_PROMPT.md — agent must bootstrap its own persistence — v1.0
- Ratatui TUI displays: main agent log, sub-agent/task tree, flagged discoveries panel, high-level progress — v1.0
- TUI allows user to pause/resume the agent loop, inspect state, and debug — v1.0
- Well-formatted structured logs for the main loop and all background tasks — v1.0
- Basic guardrails: workspace-scoped execution, no sudo, destructive command blocking — v1.0

### Active

(None — v1.0 shipped. See /gsd:new-milestone for next iteration.)

### Out of Scope

- Container/Docker sandboxing — full shell access within workspace for v1; container isolation can wrap the existing tool interface later
- Token/cost tracking — running locally on Ollama, not a concern
- Multi-model orchestration — single configurable model for v1
- Pre-built memory or knowledge systems — the agent builds its own; that's the experiment
- GUI or web dashboard — TUI only

## Context

The core hypothesis is that a local LLM, given enough freedom and persistence mechanisms, can develop its own exploration patterns, memory systems, and tooling. The agent's first survival challenge is bootstrapping: it must figure out how to persist knowledge across context window restarts using only SYSTEM_PROMPT.md (which the harness guarantees to load) and its workspace.

OpenClaw-style agents have shown that LLMs can pattern-match toward developing "interests" when persistence exists on disk. This project tests that hypothesis with a purpose-built harness.

The genai crate is preferred as the LLM driver — the user contributes to this project. Ratatui handles the TUI. The agent's workspace is a single directory; the agent decides its own organizational structure.

The monitoring TUI should surface what matters: what the agent is doing (log), what's running in the background (sub-agent tree), and what the agent thinks is interesting (discoveries panel). The user wants to observe the experiment, not babysit it.

## Current State (v1.0 Shipped)

**Version:** v1.0 Initial Release (2026-02-05)
**Codebase:** 11,617 lines of Rust across 124 files
**Tech stack:** Rust, genai (Ollama), ratatui, tokio, reqwest, htmd, scraper

**Capabilities:**
- Infinite agent loop with streaming LLM responses
- 13 tools: shell, file read/write, web fetch, web search, sleep, discovery, sub-agent management
- Context management with observation masking and automatic session restart
- Four-panel TUI dashboard with real-time updates
- Sub-agent orchestration with hierarchical lifecycle management

**Next:** Run the agent and observe autonomous exploration patterns

## Constraints

- **Runtime**: Rust — performance, safety, suitable for long-running daemon processes
- **LLM driver**: genai crate (user contributes to this project)
- **TUI**: ratatui — rich terminal UI without web framework complexity
- **LLM backend**: Ollama running locally — no cloud API dependencies
- **Execution model**: Agent loop runs indefinitely; harness manages lifecycle
- **Security**: Workspace-scoped shell access, no sudo, rate limiting, destructive command blocking

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Full shell access (not container) for v1 | Simplicity; container adds build complexity. Guardrails via workspace scoping and command filtering | Good |
| Agent bootstraps its own persistence | Core experiment: see if the agent can design its own memory. Harness only guarantees SYSTEM_PROMPT.md loading | Good |
| Single workspace directory (no imposed structure) | Minimize scaffolding; see what organizational patterns the agent develops on its own | Good |
| genai crate for LLM communication | User contributes to the project; ensures tight integration and ability to fix issues | Good |
| Timer + events + user-controlled pause | Maximum flexibility for the agent (timer/events) and the human (manual override) | Good |
| Two-phase Ctrl+C shutdown | First signal graceful (finish current turn), second signal force-exit | Good |
| Session-based architecture | Outer restart loop in main.rs, inner turn loop in run_agent_session | Good |
| Sub-agent CancellationToken hierarchy | Clean cascading shutdown without orphan processes | Good |

---
*Last updated: 2026-02-05 after v1.0 milestone*
