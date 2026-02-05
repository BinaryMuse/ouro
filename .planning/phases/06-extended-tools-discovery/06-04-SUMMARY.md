---
phase: 06-extended-tools-discovery
plan: 04
subsystem: agent-loop-tui
tags: [sleep-integration, tui-sleeping-state, discovery-display, wake-notification]
dependency_graph:
  requires: [06-01, 06-02, 06-03]
  provides: [sleep-loop-integration, tui-sleep-display, enhanced-discoveries]
  affects: []
tech_stack:
  added: []
  patterns: [sleep-state-machine-between-turns, pause-flag-reuse-for-manual-sleep]
key_files:
  created: []
  modified:
    - src/agent/agent_loop.rs
    - src/tui/event.rs
    - src/tui/app_state.rs
    - src/tui/tabs/discoveries_tab.rs
    - src/tui/widgets/status_bar.rs
    - src/tui/input.rs
    - src/tui/ui.rs
    - src/agent/tools.rs
    - src/agent/discovery.rs
decisions:
  - id: 06-04-pause-flag-reuse
    description: "Manual sleep mode reuses existing pause_flag for TUI resume signal -- r key clears flag, sleep loop detects cleared flag as wake"
  - id: 06-04-sleep-between-turns
    description: "Sleep state machine executes between turns after tool dispatch, before context evaluation -- no LLM calls during sleep"
  - id: 06-04-wake-as-system-message
    description: "Wake notification injected as ChatMessage::system with reason and elapsed time -- counted in context char tracking"
metrics:
  duration: 7 min
  completed: 2026-02-05
---

# Phase 6 Plan 4: Agent Loop Sleep Integration and TUI Enhancements Summary

Sleep state machine integrated into agent loop between turns with timer/event/manual wake modes; TUI enhanced with Sleeping state, sleep countdown display, two-line discovery rendering, and r-key resume.

## Tasks Completed

| # | Task | Commit | Key Files |
|---|------|--------|-----------|
| 1 | Add Sleeping state to TUI and enhance discovery data model | 256a0c9 | event.rs, app_state.rs, discoveries_tab.rs, status_bar.rs, input.rs, ui.rs, tools.rs, agent_loop.rs |
| 2 | Integrate sleep state machine into agent loop | d648a3d | agent_loop.rs, discovery.rs, tools.rs |
| 3 | Build verification | (verify only) | clippy clean, all tests pass, release build succeeds |

## What Was Built

### TUI Enhancements

- **AgentState::Sleeping** variant added with Magenta color in status bar
- **Discovery event** changed from single `content` field to `title` + `description` fields
- **AppState.discoveries** updated to 3-tuple `(timestamp, title, description)`
- **sleep_display_text** field added to AppState for status bar display
- **Discoveries tab** renders each discovery as two lines: yellow title + gray indented description
- **Status bar** shows sleep display text next to state indicator when sleeping
- **r key** handler added to resume sleeping agent by clearing pause_flag
- **Keybind hint** "r: resume sleep" appears in status bar line 2 when Sleeping

### Agent Loop Sleep Integration

- **pending_sleep** state variable tracks active sleep between turns
- **Sleep detection**: After tool dispatch, parses sleep tool response for `sleep_requested: true`
- **Sleep state machine**: Blocks the loop after tool dispatch, before context evaluation
  - Polls every 500ms checking wake conditions
  - Emits `AgentState::Sleeping` to TUI on entry
  - Restores `AgentState::Idle` on wake
- **Timer mode**: Compares elapsed time against requested duration
- **Event mode**: Polls `SubAgentManager.get_status()` for Completed/Failed/Killed
- **Manual mode**: Sets pause_flag on entry; TUI r-key clears it; loop detects cleared flag
- **Safety timeout**: `max_duration` (from config) applies to all modes
- **Wake notification**: Injected as `ChatMessage::system` with reason and elapsed seconds
- **Context tracking**: Wake message length added to char-based context estimation

### Cross-Plan Fixes (06-03 Merge)

- Fixed `dispatch_tool_call` call site to pass 6th `event_tx` parameter (06-03 added it)
- Updated `AgentEvent::Discovery` emission in tools.rs to use title/description
- Fixed clippy issues from 06-03: wildcard-in-or-patterns, lines-filter-map-ok
- Added `#[allow(dead_code)]` for `load_discoveries` (not yet called from binary)

## Decisions Made

1. **pause_flag reuse for manual sleep** -- Rather than adding a new flag, manual sleep mode sets the existing `pause_flag` to true on entry. The TUI r-key clears it (same mechanism as p-key for pause/resume). The sleep loop detects the cleared flag as the wake signal. This reuses existing infrastructure with zero new synchronization primitives.

2. **Sleep between turns, not mid-tool** -- The sleep state machine runs after all tool calls in a turn complete and before context evaluation. This means:
   - No LLM calls happen during sleep
   - Turn counter does not increment during sleep
   - Context evaluation runs after wake (wake message is counted)
   - All tool results are already appended to conversation before sleeping

3. **Wake notification as system message** -- The wake message is injected as `ChatMessage::system` (not user or assistant). This follows the established pattern for harness-generated notifications (restart markers, context mask notifications, wind-down messages). The message includes both the wake reason and elapsed time so the agent has full context.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed dispatch_tool_call 6th parameter**

- **Found during:** Task 1 compilation
- **Issue:** Plan 06-03 (executing in parallel) added a 6th `event_tx` parameter to `dispatch_tool_call`. The call site in agent_loop.rs needed updating.
- **Fix:** Added `event_tx.as_ref()` as 6th argument to the dispatch call
- **Files modified:** src/agent/agent_loop.rs
- **Commit:** 256a0c9

**2. [Rule 1 - Bug] Fixed Discovery event format in tools.rs**

- **Found during:** Task 1 compilation
- **Issue:** Plan 06-03 emitted `AgentEvent::Discovery` with old `content` field. Our changes to event.rs split it into `title` + `description`.
- **Fix:** Updated the emission to pass title and description separately
- **Files modified:** src/agent/tools.rs
- **Commit:** 256a0c9

**3. [Rule 1 - Bug] Fixed clippy warnings from 06-03 code**

- **Found during:** Task 2 clippy check
- **Issue:** `"duckduckgo" | _` wildcard pattern and `filter_map(|line| line.ok())` on Lines iterator
- **Fix:** Changed to `_` and `map_while(Result::ok)` respectively
- **Files modified:** src/agent/tools.rs, src/agent/discovery.rs
- **Commit:** d648a3d

## Verification Results

| Check | Result |
|-------|--------|
| `cargo clippy -- -D warnings` | Clean (0 warnings, 0 errors) |
| `cargo test` | 201 lib + 201 bin + 36 + 9 + 8 + 14 integration = all pass |
| `cargo build --release` | Success (13.4s) |
| AgentState::Sleeping exists | Yes -- event.rs |
| Discovery events carry title + description | Yes -- event.rs, tools.rs |
| Sleep state machine blocks agent loop | Yes -- agent_loop.rs (500ms poll loop) |
| Manual sleep reuses pause_flag | Yes -- sets true on entry, TUI r-key clears |
| Wake notification injected | Yes -- ChatMessage::system with reason + elapsed |

## Next Phase Readiness

This is the final plan in Phase 6. All phase deliverables are complete:
- Plan 01: Dependencies, config schema, web_fetch/web_search modules
- Plan 02: Discovery persistence (JSONL) and sleep state machine types
- Plan 03: Tool dispatch wiring (13 tools) and system prompt discovery guidance
- Plan 04: Agent loop sleep integration and TUI enhancements

Phase 6 success criteria are met:
1. Agent can fetch web pages by URL (web_fetch tool)
2. Agent can search the internet (web_search with DDG + Brave)
3. Agent can pause itself with timer/event/manual resume (sleep tool + loop integration)
4. Agent can flag discoveries with title + description (flag_discovery tool + TUI display)

No blockers. The project is feature-complete for v1.
