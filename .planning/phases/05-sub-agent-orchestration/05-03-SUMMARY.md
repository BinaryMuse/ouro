---
phase: 05-sub-agent-orchestration
plan: 03
subsystem: orchestration
tags: [tool-dispatch, sub-agent, genai-tools, pin-box-future, send-safety]

# Dependency graph
requires:
  - phase: 05-01
    provides: "SubAgentManager registry with types, CancellationToken hierarchy, status/result/cancel APIs"
  - phase: 05-02
    provides: "spawn_llm_sub_agent and spawn_background_process functions"
  - phase: 02-02
    provides: "Core tool schemas (shell_exec, file_read, file_write) and dispatch_tool_call"
provides:
  - "Six new tool schemas: spawn_llm_session, spawn_background_task, agent_status, agent_result, kill_agent, write_stdin"
  - "define_tools(filter) with optional name-based filtering for sub-agent tool customization"
  - "dispatch_tool_call routes all 9 tools including sub-agent orchestration tools"
  - "write_to_stdin method on SubAgentManager for non-consuming stdin writes"
  - "Updated tool descriptions in system prompt covering all 9 tools"
affects: [05-04-tui-integration, 05-05-integration-testing]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Pin<Box<dyn Future + Send>> return type for async dispatch helpers to break opaque type cycles"
    - "tokio::spawn indirection for sub-agent dispatch to satisfy Send + 'static bounds"
    - "Macro-based early return (require_manager!) for consistent sub-agent tool error handling"
    - "Argument extraction separated from async dispatch for cycle-free type resolution"

key-files:
  created: []
  modified:
    - src/agent/tools.rs
    - src/orchestration/manager.rs
    - src/agent/agent_loop.rs
    - src/main.rs

key-decisions:
  - "Pin<Box<dyn Future + Send>> return type to break opaque type cycle between dispatch_tool_call -> spawn_llm_sub_agent -> run_agent_session -> dispatch_tool_call"
  - "tokio::spawn indirection for spawn dispatch functions -- owned clones in spawned task satisfy Send + 'static"
  - "write_to_stdin takes-writes-puts-back ChildStdin handle instead of consuming it (enables multiple writes)"
  - "define_tools accepts Option<&[String]> filter for sub-agent tool customization per user decision"
  - "dispatch_tool_call takes Option parameters for manager/config -- returns error JSON when None"

patterns-established:
  - "Async dispatch with type erasure: separate argument extraction (sync) from execution (async with boxed future)"
  - "Optional capability injection: manager/config as Option allows same dispatch function for root agent and sub-agents"

# Metrics
duration: 9min
completed: 2026-02-05
---

# Phase 5 Plan 3: Tool Dispatch Wiring Summary

**Six sub-agent tool schemas with dispatch routing to orchestration layer, Pin<Box<dyn Future + Send>> type erasure for cycle-free async, and tool-filtered define_tools for sub-agent customization**

## Performance

- **Duration:** 9 min
- **Started:** 2026-02-05T01:47:27Z
- **Completed:** 2026-02-05T01:56:24Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Extended define_tools from 3 to 9 tools with optional name-based filtering for sub-agent tool customization
- dispatch_tool_call routes all 6 new sub-agent tools to the orchestration module with proper argument extraction
- Solved async opaque type cycle (dispatch -> spawn -> session -> dispatch) using Pin<Box<dyn Future + Send>> and tokio::spawn indirection
- Added write_to_stdin to SubAgentManager for non-consuming stdin writes (takes handle, writes, puts back)
- Tool descriptions updated with markdown documentation for all 9 tools, automatically embedded in system prompt

## Task Commits

Each task was committed atomically:

1. **Task 1: Add tool schemas and dispatch for all six sub-agent tools** - `5b9ff1b` (feat)
2. **Task 2: Update tool descriptions in system prompt** - `5bc8b7e` (feat)

## Files Created/Modified
- `src/agent/tools.rs` - Six new tool schemas, dispatch routing, Pin<Box<dyn Future>> helpers, argument extractors, tool descriptions, updated tests (18 total)
- `src/orchestration/manager.rs` - Added write_to_stdin method for non-consuming stdin writes
- `src/agent/agent_loop.rs` - Updated define_tools(None) and dispatch_tool_call with new parameters
- `src/main.rs` - Added mod orchestration for binary crate module resolution

## Decisions Made
- **Pin<Box<dyn Future + Send>> for type erasure:** The async dispatch chain creates a cyclic opaque type dependency (dispatch_tool_call -> spawn_llm_sub_agent -> run_agent_session -> dispatch_tool_call). Returning boxed futures from the sub-agent dispatch functions breaks this cycle while maintaining Send safety for tokio::spawn contexts.
- **tokio::spawn indirection:** Sub-agent spawn dispatch functions clone manager/config into owned values and spawn a tokio task to execute. This ensures the spawned future is Send + 'static without requiring the spawning functions themselves to change.
- **write_to_stdin take-write-put-back pattern:** Rather than consuming the ChildStdin handle (take_stdin), the new write_to_stdin method temporarily removes the handle, writes data, and puts it back. This enables multiple writes to the same process over time.
- **Optional filter on define_tools:** Accepts `Option<&[String]>` to enable sub-agent tool customization (per plan's user decision). None returns all 9 tools; Some filters to named tools only.
- **mod orchestration in main.rs:** The binary crate needed the orchestration module declaration to resolve crate::orchestration imports from tools.rs.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Opaque type cycle between dispatch and spawn async functions**
- **Found during:** Task 1 (dispatch implementation)
- **Issue:** `dispatch_tool_call` (async) calls `spawn_llm_sub_agent` (async) which calls `run_agent_session` (async) which calls `dispatch_tool_call` -- creating a circular opaque type dependency that prevents Rust from resolving the future's Send bound
- **Fix:** Changed sub-agent dispatch helpers from `async fn` to regular `fn` returning `Pin<Box<dyn Future<Output = String> + Send>>`, and used `tokio::spawn` with owned clones for the actual async work
- **Files modified:** src/agent/tools.rs
- **Verification:** cargo check passes, all tests pass
- **Committed in:** 5b9ff1b (Task 1 commit)

**2. [Rule 3 - Blocking] Missing mod orchestration in binary crate**
- **Found during:** Task 1 (dispatch implementation)
- **Issue:** Binary crate (main.rs) did not declare `mod orchestration`, so `crate::orchestration::*` imports in tools.rs failed when compiling as binary
- **Fix:** Added `mod orchestration;` to main.rs
- **Files modified:** src/main.rs
- **Verification:** cargo check passes for both lib and bin targets
- **Committed in:** 5b9ff1b (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (2 blocking -- async type cycle and missing module declaration)
**Impact on plan:** Both fixes were necessary for compilation. The Pin<Box<dyn Future>> approach is the standard Rust pattern for breaking async type cycles. No scope creep.

## Issues Encountered
None beyond the deviations documented above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All 9 tools are schema-defined, dispatch-routed, and described in the system prompt
- The root agent can now spawn sub-agents and background processes through tool calls
- Plan 05-04 (TUI integration) can wire sub-agent status events to the dashboard panel
- Plan 05-05 (integration testing) can exercise the full tool dispatch -> orchestration pipeline

---
*Phase: 05-sub-agent-orchestration*
*Completed: 2026-02-05*
