# Ouroboros

An autonomous AI research harness that runs local Ollama models in an infinite exploration loop.

> **Status:** Under active development. Phase 1 (safety & configuration) is complete; Phase 2 (core agent loop) is in progress.

## What is this?

Most AI agent frameworks ship with built-in memory systems, planning engines, and tool libraries. Ouroboros inverts that model. It provides a blank workspace and forces the agent to bootstrap its own persistence, memory, and organizational structure using the same tools it uses for everything else.

The agent runs indefinitely, exploring open-ended topics -- AI architecture, data processing, philosophy, simulation, creative generation -- while building and maintaining its own knowledge systems through files on disk.

### Key ideas

- **Workspace-as-memory** -- No vector databases or RAG pipelines. The agent's workspace directory *is* its memory. It reads and writes files to persist knowledge across context window restarts.
- **Inverse framework design** -- Instead of providing infrastructure, the harness provides constraints (safety boundaries, execution sandboxing) and lets the agent figure out the rest.
- **Local-first** -- All inference runs on local Ollama models. No cloud API dependencies. Data never leaves your machine.
- **Agent-controlled sub-agents** -- The agent itself decides when to spawn sub-agents and what to delegate, rather than following a framework-imposed orchestration pattern.

## Tech stack

| Component | Technology |
|-----------|-----------|
| Language | Rust (2024 edition) |
| Async runtime | tokio |
| LLM client | [genai](https://github.com/jeremychone/rust-genai) |
| LLM backend | Ollama (local) |
| Terminal UI | ratatui |

## Architecture

Ouroboros uses an actor-per-component model with tokio channels for message passing. A central Coordinator actor owns all mutable state; every other component (agent loop, tool executor, TUI renderer, sub-agent supervisor) communicates through typed channels. This eliminates shared mutable state and keeps the agent loop decoupled from the UI.

## Building

Requires Rust 1.85.0+.

```
cargo build
```

## Project status

Ouroboros is pre-release software under active development.

| Phase | Description | Status |
|-------|-------------|--------|
| 1 | Safety & Configuration | Complete |
| 2 | Core Agent Loop & Basic Tools | In progress |
| 3 | Context Management & Resilience | Not started |
| 4 | TUI Dashboard | Not started |
| 5 | Sub-Agent Orchestration | Not started |
| 6 | Extended Tools & Discovery | Not started |

## License

TBD
