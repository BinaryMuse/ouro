---
phase: 02-core-agent-loop-basic-tools
plan: 03
subsystem: agent
tags: [genai, ollama, streaming, tool-calling, signal-handling, reqwest, jsonl]

# Dependency graph
requires:
  - phase: 02-core-agent-loop-basic-tools
    plan: 01
    provides: "SessionLogger for JSONL event logging, AgentError types"
  - phase: 02-core-agent-loop-basic-tools
    plan: 02
    provides: "System prompt builder, tool schemas, dispatch_tool_call"
  - phase: 01-safety-and-configuration
    plan: 04
    provides: "SafetyLayer for command execution and workspace enforcement"
provides:
  - "Core agent conversation loop (run_agent_loop) with streaming and tool dispatch"
  - "Ollama health check (check_ollama_ready) with connectivity and model validation"
  - "Two-phase Ctrl+C shutdown handling"
  - "Context-full heuristic detection"
  - "Working `ouro run` command that connects to local Ollama"
affects:
  - 03-context-management
  - 04-self-modification
  - 05-multi-session-continuity

# Tech tracking
tech-stack:
  added: [reqwest]
  patterns: [streaming-chat-loop, atomic-bool-shutdown, heuristic-context-tracking]

key-files:
  created:
    - src/agent/agent_loop.rs
  modified:
    - src/agent/mod.rs
    - src/main.rs
    - Cargo.toml

key-decisions:
  - "reqwest added as direct dependency for health check HTTP calls (already transitive via genai)"
  - "ChatMessage::from(Vec<ToolCall>) for assistant tool-call message construction (uses genai's built-in conversion)"
  - "Context-full heuristic: total_chars / 4 > context_limit (conservative, replaced by proper tracking in Phase 3)"
  - "Shutdown flag checked only between turns, not mid-stream, to avoid partial conversation state"
  - "Stream errors logged but not fatal -- End event may still arrive after partial errors"

patterns-established:
  - "Streaming loop: exec_chat_stream -> iterate events -> handle End -> dispatch tools -> re-prompt"
  - "Two-phase Ctrl+C: AtomicBool flag + tokio::spawn for graceful-then-force shutdown"
  - "stderr for harness messages (tool info, status), stdout for model text only"

# Metrics
duration: 4min
completed: 2026-02-04
---

# Phase 2 Plan 3: Agent Loop & Ollama Integration Summary

**Core conversation loop connecting genai streaming to Ollama with tool dispatch, JSONL logging, and two-phase Ctrl+C shutdown**

## Performance

- **Duration:** 4 min
- **Started:** 2026-02-04T20:23:46Z
- **Completed:** 2026-02-04T20:27:24Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Ollama health check validates connectivity and model availability at startup with clear error messages
- Core agent loop streams model text to stdout in real time, dispatches tool calls, and re-prompts on text-only responses
- All events (session start/end, assistant text, tool calls, tool results, errors) logged to JSONL
- Two-phase Ctrl+C: first signal sets graceful shutdown between turns, second force-exits
- `ouro run` is now a working command that starts a real agent session against a local Ollama model

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement Ollama health check and agent loop** - `0d812c5` (feat)
2. **Task 2: Wire agent loop into main.rs** - `b754604` (feat)

**Plan metadata:** [pending] (docs: complete plan)

## Files Created/Modified
- `src/agent/agent_loop.rs` - Core agent conversation loop (362 lines): health check, streaming, tool dispatch, shutdown handling, context estimation
- `src/agent/mod.rs` - Added `pub mod agent_loop` export
- `src/main.rs` - Replaced placeholder with `run_agent_loop` call in Run command
- `Cargo.toml` - Added reqwest as direct dependency for health check

## Decisions Made
- **reqwest as direct dependency:** Added explicitly even though it's a transitive dep via genai, because we call it directly for health check HTTP requests. Using `features = ["json"]` for the `.json()` request builder.
- **ChatMessage::from(Vec<ToolCall>) for assistant messages:** Uses genai's built-in conversion which handles thought_signatures correctly, rather than the `append_tool_use_from_stream_end` method (which takes a single ToolResponse and would need multiple calls).
- **Context heuristic: chars/4 vs context_limit:** Conservative approximation that will be replaced by proper token tracking in Phase 3. Overestimates tokens for code/structured content but prevents silent Ollama truncation.
- **Shutdown between turns only:** The AtomicBool flag is only checked at the start of each turn, never mid-stream. This ensures no partial conversation state (e.g., tool calls received but not dispatched).
- **Stream errors non-fatal:** If an individual stream event errors, we log and continue. The End event may still arrive with captured content.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added reqwest as direct Cargo.toml dependency**
- **Found during:** Task 1 (Ollama health check implementation)
- **Issue:** Plan referenced `reqwest::Client::new()` for health checks but reqwest was only a transitive dependency through genai, not directly declared
- **Fix:** Added `reqwest = { version = "0.12", features = ["json"] }` to Cargo.toml dependencies
- **Files modified:** Cargo.toml
- **Verification:** `cargo check` passes, health check compiles and runs
- **Committed in:** 0d812c5 (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Necessary for compilation. No scope creep.

## Issues Encountered
None -- plan executed cleanly. The genai streaming API matched the research findings. The `ChatMessage::from(Vec<ToolCall>)` conversion worked as documented.

## User Setup Required
None -- no external service configuration required. Ollama must be running locally (the health check validates this at startup).

## Next Phase Readiness
- Phase 2 is complete: all three plans delivered
- The full agent loop is functional: health check, streaming, tool dispatch, logging, shutdown
- Ready for Phase 3 (Context Management): the loop already tracks character counts and has a heuristic context-full check that Phase 3 will replace with proper token tracking
- The `captured_usage` field from StreamEnd is available but not yet used (Ollama may not populate it during streaming)

---
*Phase: 02-core-agent-loop-basic-tools*
*Plan: 03*
*Completed: 2026-02-04*
