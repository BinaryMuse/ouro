---
phase: 04-tui-dashboard
plan: 04
subsystem: tui
tags: [ratatui, crossterm, tokio-select, event-stream, keyboard-input, main-loop, headless]

# Dependency graph
requires:
  - phase: 04-tui-dashboard
    provides: "AppState, AgentEvent, ControlSignal, render_ui from 04-01/02/03"
  - phase: 03-context-management
    provides: "Agent loop with event_tx/pause_flag parameters, ShutdownReason, session restart"
provides:
  - "handle_key_event: keyboard event handler mapping keys to state mutations and control signals"
  - "run_tui: TUI main loop with tokio::select! multiplexing agent events, keyboard input, and render ticks"
  - "--headless CLI flag: TUI mode (default) vs headless mode (original behavior)"
  - "Complete TUI application compiling and linking end-to-end"
affects: [04-05 sub-agent tree polish]

# Tech tracking
tech-stack:
  added: [crossterm 0.29 (event-stream feature for async EventStream)]
  patterns: [tokio::select! event multiplexing, spawned agent task with mpsc channels, CLI mode branching]

key-files:
  created:
    - src/tui/input.rs
    - src/tui/runner.rs
  modified:
    - src/tui/mod.rs
    - src/main.rs
    - src/cli.rs
    - src/config/mod.rs
    - Cargo.toml

key-decisions:
  - "crossterm 0.29 added as direct dependency for event-stream feature only (EventStream not re-exported by ratatui)"
  - "SafetyLayer recreated inside spawned task (not Clone) rather than adding Clone derive"
  - "Config destructure pattern uses .. for forward-compatible field additions"
  - "Ctrl+C shutdown message only printed in headless mode (TUI handles quit via 'q' key)"

patterns-established:
  - "TUI runner pattern: ratatui::init + tokio::select! loop + ratatui::restore"
  - "Input handler pattern: KeyEventKind::Press filter, quit confirmation state machine, return bool for exit"
  - "CLI mode branching: --headless flag for backward-compatible headless mode, TUI as default"

# Metrics
duration: 4min
completed: 2026-02-05
---

# Phase 4 Plan 04: TUI Input and Main Loop Integration Summary

**TUI main loop with tokio::select! event multiplexing, keyboard input handler for all keybinds, and --headless CLI flag wiring TUI as default launch mode**

## Performance

- **Duration:** 4 min
- **Started:** 2026-02-05T00:03:32Z
- **Completed:** 2026-02-05T00:07:41Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments
- Built keyboard input handler mapping Tab, Shift+Tab, arrows, p/e/g/q/y/n/t keys to state mutations and control signals with Press-only filtering
- Created TUI main loop using tokio::select! to multiplex agent events, crossterm EventStream keyboard input, and 20fps render ticks
- Agent loop runs as a spawned tokio task with full restart loop, sending events through unbounded mpsc channel
- Wired main.rs to launch TUI by default, with --headless flag preserving identical original behavior
- Added 16 input handler tests covering key mapping, quit confirmation flow, pause/resume signals, and Ctrl+C

## Task Commits

Each task was committed atomically:

1. **Task 1: Create input handler and TUI runner** - `73ac200` (feat)
2. **Task 2: Wire main.rs with TUI/headless launch paths** - `a80490b` (feat)

## Files Created/Modified
- `src/tui/input.rs` - Keyboard event handler mapping key events to state mutations and control signals
- `src/tui/runner.rs` - TUI main loop with tokio::select!, terminal init/restore, render tick
- `src/tui/mod.rs` - Updated to include input and runner submodules
- `src/main.rs` - Branches on --headless flag: TUI mode (default) calls run_tui, headless runs restart loop directly
- `src/cli.rs` - Added --headless bool flag to Run command
- `src/config/mod.rs` - Updated destructure pattern with .. for forward compatibility
- `Cargo.toml` - Added crossterm 0.29 direct dependency with event-stream feature

## Decisions Made
- crossterm 0.29 added as direct dependency specifically for the event-stream feature, since ratatui does not re-export EventStream. Version pinned to match ratatui 0.30's internal crossterm to avoid type conflicts.
- SafetyLayer is recreated inside the spawned tokio task rather than making it Clone, since it holds compiled regexes and is cheap to reconstruct.
- Config pattern match uses `..` rest pattern so adding CLI flags doesn't break existing destructure sites.
- Ctrl+C handler only prints shutdown message text in headless mode; in TUI mode the dashboard handles quit flow via the 'q' key confirmation.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Config destructure did not compile with new headless field**
- **Found during:** Task 2 (wire main.rs)
- **Issue:** The `cli_to_partial` function in `src/config/mod.rs` destructures all fields of `Commands::Run` explicitly, so adding `headless` broke compilation
- **Fix:** Changed the destructure pattern to use `..` rest syntax for forward compatibility
- **Files modified:** src/config/mod.rs
- **Verification:** `cargo check` passes
- **Committed in:** a80490b (Task 2 commit)

**2. [Rule 1 - Bug] Test code had unused variables and type inference failure**
- **Found during:** Task 1 (input handler tests)
- **Issue:** Leftover variable bindings and missing type annotations in pause/resume tests
- **Fix:** Removed unused variables and fixed channel setup in tests
- **Files modified:** src/tui/input.rs
- **Verification:** `cargo test` passes all 329 tests
- **Committed in:** 73ac200 (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (1 blocking, 1 bug)
**Impact on plan:** Both fixes were necessary for compilation and test correctness. No scope creep.

## Issues Encountered
None beyond the deviations documented above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Complete TUI application compiles and links end-to-end
- `ouro run` launches TUI mode with agent loop in background task, keyboard controls, real-time rendering
- `ouro run --headless` preserves identical pre-TUI behavior
- Ready for Plan 05 (sub-agent tree polish / final TUI refinements)
- All 329 tests passing across the full project
- No blockers or concerns

---
*Phase: 04-tui-dashboard*
*Completed: 2026-02-05*
