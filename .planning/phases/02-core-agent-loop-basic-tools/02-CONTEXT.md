# Phase 2: Core Agent Loop & Basic Tools - Context

**Gathered:** 2026-02-04
**Status:** Ready for planning

<domain>
## Phase Boundary

The harness runs an infinite conversation loop against a local Ollama model via genai, calling tools (shell, file read, file write) and logging everything to structured files on disk. The workspace's SYSTEM_PROMPT.md is loaded as the system prompt. The loop runs until the user stops it or context fills up. Creating sub-agents, context management, and the TUI are separate phases.

</domain>

<decisions>
## Implementation Decisions

### Conversation flow
- Primarily LLM-autonomous: system prompt sets goals, model decides actions, harness relays tool results
- Harness may inject minimal nudges to keep the loop intact (e.g., if the model stalls), but does not drive decision-making
- When the model responds without a tool call, treat it as "thinking out loud" — add the text to the conversation and prompt again for the next action
- Configurable delay between turns (default 0) — useful for GPU throttling or observation
- Harness wraps the user's SYSTEM_PROMPT.md with context: available tools, workspace path, constraints

### Tool interface design
- Use genai's native tool/function calling protocol — model receives tool schemas, returns structured tool calls
- Tool errors returned as structured tool results with error details — model sees them in conversation and can react
- File operations (read/write) validated through the same safety layer as shell commands (WorkspaceGuard from Phase 1)
- Tool results returned in full — no truncation. Context management (Phase 3) will handle pressure later

### Logging & observability
- JSONL format (one JSON object per line)
- Log everything: model responses, tool calls, tool results, errors, system messages — full replay capability
- Log files stored alongside the workspace directory (sibling), not inside it — keeps workspace clean for the agent
- New timestamped log file per session (e.g., session-2026-02-04T10-30.jsonl)

### Session lifecycle
- Validate Ollama connectivity and model availability at startup — fail early with clear error if not ready
- Ctrl+C finishes current turn, second Ctrl+C force-kills immediately
- When context window fills (pre-Phase 3): stop the loop with a clear message, user restarts manually
- Stream model text responses to stdout in real time (pre-TUI experience) — user can watch the agent think

### Claude's Discretion
- Exact tool schema definitions and parameter naming
- Internal message format between harness and model
- How the harness detects "context full" condition
- Turn counter and status details

</decisions>

<specifics>
## Specific Ideas

- Use genai crate from Git (main branch), not a published version
- Streaming model output to stdout should feel like watching the agent work — conversational text flows in real time, tool calls are shown as they happen

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 02-core-agent-loop-basic-tools*
*Context gathered: 2026-02-04*
