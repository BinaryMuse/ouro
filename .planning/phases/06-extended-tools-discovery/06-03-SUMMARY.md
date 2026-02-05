---
phase: 06-extended-tools-discovery
plan: 03
subsystem: agent-tools
tags: [tool-dispatch, web-fetch, web-search, sleep, discovery, system-prompt]

# Dependency graph
requires:
  - phase: 06-01
    provides: web_fetch and web_search modules with fetch_url, rate_limited_ddg_search, rate_limited_brave_search functions
  - phase: 06-02
    provides: discovery and sleep modules with append_discovery, load_discoveries, parse_sleep_args functions
provides:
  - 13-tool define_tools() with schemas for web_fetch, web_search, sleep, flag_discovery
  - dispatch_tool_call routing for all 4 new tools with event_tx parameter
  - tool_descriptions covering all 13 tools for system prompt injection
  - Discovery guidance in system prompt teaching agent what qualifies as a discovery
affects:
  - 06-04 (TUI integration -- sleep state machine consumes dispatch_sleep return)

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "dispatch_tool_call event_tx parameter for TUI event emission from tools"
    - "Tool dispatch returns JSON signal (sleep_requested) for agent loop state machine"
    - "Discovery guidance section in system prompt for model instruction"

key-files:
  created: []
  modified:
    - "src/agent/tools.rs"
    - "src/agent/system_prompt.rs"

key-decisions:
  - "dispatch_sleep returns JSON signal rather than blocking -- agent loop reads sleep_requested to enter sleep state machine"
  - "web_search defaults to duckduckgo, falls back gracefully when brave_api_key missing"
  - "flag_discovery emits AgentEvent::Discovery with title+description fields for TUI rendering"
  - "Discovery guidance placed after Constraints and before Session Continuity in system prompt"

patterns-established:
  - "event_tx: Option<&UnboundedSender<AgentEvent>> parameter pattern for tool dispatch"
  - "Sleep tool returns immediately with signal JSON, not blocking -- state machine handled by caller"

# Metrics
duration: 12min
completed: 2026-02-05
---

# Phase 06 Plan 03: Tool Dispatch Wiring Summary

**13 agent tools wired with schemas, dispatch routing, descriptions, and discovery guidance in system prompt**

## Performance

- **Duration:** ~12 min
- **Started:** 2026-02-05T03:25:22Z
- **Completed:** 2026-02-05T03:37:36Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- Extended define_tools() from 9 to 13 tools with proper JSON schemas for web_fetch, web_search, sleep, flag_discovery
- Wired all 4 dispatch helpers: web_fetch delegates to fetch_url, web_search handles provider selection with rate limits, sleep returns signal JSON, flag_discovery persists to disk and emits AgentEvent
- Added event_tx parameter to dispatch_tool_call for TUI event emission
- Added discovery guidance section to system prompt teaching the agent about discoveries
- Comprehensive test coverage: 9 new tests for extended tools, updated descriptions test, tool count/names tests

## Task Commits

Each task was committed atomically:

1. **Task 1: Add 4 new tool schemas and dispatch branches to tools.rs** - `f600a13` (feat)
2. **Task 2: Add discovery guidance to system prompt** - `3373ac4` (feat)

**Plan metadata:** (pending)

## Files Created/Modified
- `src/agent/tools.rs` - 13 tool schemas, dispatch routing, descriptions, 4 dispatch helpers, 9 new tests
- `src/agent/system_prompt.rs` - Discovery guidance section, updated test assertions

## Decisions Made
- **dispatch_sleep returns JSON signal:** The sleep tool returns `{"sleep_requested": true, "mode": ..., "max_duration_secs": ...}` immediately. The agent loop (plan 06-04) reads this signal to enter the sleep state machine. This avoids blocking in the tool dispatcher.
- **Default provider is duckduckgo:** The `_` wildcard match arm catches both "duckduckgo" explicitly and any unknown provider string, defaulting to DDG which requires no API key.
- **event_tx as Option reference:** Using `Option<&UnboundedSender<AgentEvent>>` keeps the existing callers (tests, sub-agents) working with `None` while enabling TUI event emission.
- **Discovery guidance placement:** Positioned after Constraints and before Session Continuity to ensure model sees it early but after operational rules.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed indentation/brace error in agent_loop.rs sleep state machine**
- **Found during:** Task 1 verification (cargo check)
- **Issue:** Plan 06-04 (executing in parallel) introduced a sleep state machine block in agent_loop.rs with inconsistent indentation that caused a compilation error (`unexpected closing delimiter: }`)
- **Fix:** Corrected indentation of the sleep state machine body to match the `if let` block's expected nesting level
- **Files modified:** `src/agent/agent_loop.rs` (not staged, belongs to plan 06-04)
- **Verification:** `cargo check` passes cleanly, `cargo test` -- all 201 unit tests pass
- **Committed in:** Not committed separately (fix applied to plan 06-04's working tree changes)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Fix was necessary for compilation. No scope creep. Agent_loop.rs changes belong to plan 06-04's scope.

## Issues Encountered
- Plan 06-04 running in parallel modified `AgentEvent::Discovery` variant from `{ timestamp, content }` to `{ timestamp, title, description }` fields. Our dispatch_flag_discovery code was updated to match the new field structure.
- Plan 06-04's linter pass changed `"duckduckgo" | _ =>` to `_ =>` in tools.rs (removing redundant pattern arm). This is correct behavior and reduces a compiler warning.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All 13 tools are fully wired: schemas, dispatch, descriptions
- System prompt teaches the agent about discoveries
- Sleep dispatch returns signal JSON ready for agent loop consumption (plan 06-04 wires the state machine)
- Ready for plan 06-04 to complete TUI integration and end-to-end verification

---
*Phase: 06-extended-tools-discovery*
*Completed: 2026-02-05*
