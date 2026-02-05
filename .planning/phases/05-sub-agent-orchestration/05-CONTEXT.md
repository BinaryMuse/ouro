# Phase 5: Sub-Agent Orchestration - Context

**Gathered:** 2026-02-04
**Status:** Ready for planning

<domain>
## Phase Boundary

The agent can spawn and manage child LLM sessions and background shell processes, with the harness enforcing lifecycle management and cleanup. This phase delivers the orchestration infrastructure — tools for spawning, status tracking, result retrieval, and clean shutdown. Extended tool capabilities (web, search, discovery) are Phase 6.

</domain>

<decisions>
## Implementation Decisions

### Spawning model
- Separate tools for different sub-agent types: `spawn_llm_session` and `spawn_background_task` (distinct schemas)
- Parent can specify a different Ollama model per sub-agent spawn — allows using smaller/faster models for subtasks
- No hard concurrency limit on sub-agents — Ollama and system resources are the natural constraint
- LLM sub-agents receive a goal string plus optional key-value context (files to read, facts to know) injected into the sub-agent's system prompt
- Sub-agents start fresh — no parent conversation history. Parent provides any needed context explicitly at spawn time

### Lifecycle & cleanup
- No timeout by default — sub-agents run until they declare done, hit context limit, or are killed
- Parent can optionally set a timeout per spawn when it wants time-bounded work
- Sub-agents survive parent context restart — parent re-discovers running sub-agents via status tool after restart
- Sub-agents have access to the same tools as parent (shell, file read/write) through the shared safety layer, but parent can customize the tool set per spawn based on the sub-agent's task
- Nested spawning allowed — sub-agents can spawn their own children. The harness tracks the full tree. May need depth or total count constraints for system resource safety

### Communication pattern
- Sub-agents return a structured summary/result object when complete, in addition to any workspace files written — necessary because sub-agents may interact with systems beyond the filesystem
- Parent can query a running sub-agent's partial output/status while it's still running — useful for long-running tasks
- Sub-agents start with a clean slate: system prompt + goal + optional provided context. No inherited conversation history

### Background processes
- Background shell tasks use the existing safety layer only — no additional resource constraints beyond workspace scope, blocked commands, and timeout
- Background process output is captured but on-demand only — not streamed to TUI in real time. Keeps the TUI focused on the main agent's activity
- Agent can write to a running background process's stdin — enables interactive programs and pipelines
- TUI sub-agent panel displays a hierarchical tree view: parent → children with status indicators per node

### Claude's Discretion
- Failure notification mechanism for sub-agents (status polling vs event injection into parent's conversation)
- Internal data structures for the agent/process tree
- Exact format of the structured result object
- How depth/count constraints for nested spawning are enforced
- Log stream format and storage for sub-agent output

</decisions>

<specifics>
## Specific Ideas

- Sub-agents should be able to interact with systems other than the filesystem (hence structured result objects, not just file-based communication)
- The parent providing context at spawn time is intentional — the agent is the orchestrator and decides what each sub-agent needs to know
- Background processes with stdin support enables the agent to run interactive tools and pipelines

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 05-sub-agent-orchestration*
*Context gathered: 2026-02-04*
