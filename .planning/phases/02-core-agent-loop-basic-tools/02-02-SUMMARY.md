---
phase: 02-core-agent-loop-basic-tools
plan: 02
subsystem: agent
tags: [genai, tool-calling, system-prompt, dispatch, workspace-guard, safety-layer]

# Dependency graph
requires:
  - phase: 01-safety-and-configuration
    provides: "SafetyLayer.execute(), WorkspaceGuard, ExecResult, AppConfig"
  - phase: 02-core-agent-loop-basic-tools (plan 01)
    provides: "genai dependency, AgentError type, agent module skeleton"
provides:
  - "build_system_prompt() for loading SYSTEM_PROMPT.md with harness context wrapping"
  - "define_tools() returning 3 genai Tool schemas (shell_exec, file_read, file_write)"
  - "tool_descriptions() for human-readable tool listing in system prompt"
  - "dispatch_tool_call() routing tool calls through safety layer to implementations"
affects: [02-core-agent-loop-basic-tools plan 03, 03-context-management]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Tool errors as structured JSON strings (never panics) for model observability"
    - "Workspace path validation via canonicalization in dispatch layer"
    - "System prompt layering: harness context wraps user SYSTEM_PROMPT.md"

key-files:
  created:
    - "src/agent/system_prompt.rs"
    - "src/agent/tools.rs"
  modified:
    - "src/agent/mod.rs"

key-decisions:
  - "Tool dispatch uses safety.workspace_root() + canonicalization for write validation (mirrors WorkspaceGuard logic)"
  - "file_read returns raw content string; file_write returns JSON with written_bytes and path"
  - "dispatch_tool_call never returns Err -- all failures are JSON error strings in the result"
  - "file_read accepts both relative and absolute paths; file_write only accepts relative (resolved against workspace)"

patterns-established:
  - "Tool error pattern: all tool errors returned as {\"error\": \"...\"} JSON strings for model consumption"
  - "System prompt structure: harness preamble (environment, tools, constraints) then separator then user content"

# Metrics
duration: 5min
completed: 2026-02-04
---

# Phase 2 Plan 02: System Prompt & Tool System Summary

**System prompt loading from SYSTEM_PROMPT.md with harness context wrapping, plus three tool schemas (shell_exec, file_read, file_write) with dispatch function routing through SafetyLayer**

## Performance

- **Duration:** 5 min
- **Started:** 2026-02-04T20:16:00Z
- **Completed:** 2026-02-04T20:21:00Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- `build_system_prompt()` loads user's SYSTEM_PROMPT.md and wraps it with harness context (model, workspace, tools, constraints)
- Three tool schemas defined using genai's Tool type with proper JSON schemas
- `dispatch_tool_call()` routes shell_exec through SafetyLayer, file_read through tokio::fs, and file_write through workspace-validated tokio::fs::write
- All tool errors returned as structured JSON strings so the model can observe and react
- 17 new tests covering tool schemas, dispatch happy paths, error cases, and workspace escape rejection

## Task Commits

Each task was committed atomically:

1. **Task 1: System prompt loading and wrapping** - `647d52f` (feat)
2. **Task 2: Tool schema definitions and dispatch** - `1d1bcad` (feat)

## Files Created/Modified
- `src/agent/system_prompt.rs` - Loads SYSTEM_PROMPT.md from workspace, wraps with harness context (model, workspace, tools, constraints)
- `src/agent/tools.rs` - Three tool schemas (shell_exec, file_read, file_write), tool_descriptions() for system prompt, dispatch_tool_call() routing to implementations
- `src/agent/mod.rs` - Added pub mod system_prompt and pub mod tools

## Decisions Made
- Tool dispatch uses `safety.workspace_root()` plus canonicalization for write path validation, mirroring WorkspaceGuard's logic at the dispatch layer rather than calling `is_write_allowed()` directly (dispatch creates parent dirs first, then canonicalizes)
- `file_read` returns raw file content as a string (not JSON-wrapped), matching what the model expects for reading files
- `file_write` returns JSON `{"written_bytes": N, "path": "..."}` for structured success feedback
- `dispatch_tool_call` never returns `Err` -- all failures become `{"error": "..."}` JSON strings so the model always gets structured feedback
- `file_read` accepts both relative (resolved against workspace) and absolute paths since reads are unrestricted per Phase 1 decisions

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- System prompt and tool system are ready for the agent loop (Plan 03)
- `build_system_prompt()` is the system message source; `define_tools()` provides tool schemas; `dispatch_tool_call()` handles routing
- The agent loop (Plan 03) will combine these with genai's conversation API to run the autonomous loop
- 98 tests passing across all modules

---
*Phase: 02-core-agent-loop-basic-tools*
*Completed: 2026-02-04*
