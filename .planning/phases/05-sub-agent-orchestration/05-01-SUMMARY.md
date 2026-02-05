---
phase: 05-sub-agent-orchestration
plan: 01
subsystem: orchestration
tags: [tokio, cancellation-token, sub-agent, registry, uuid, arc-mutex]

# Dependency graph
requires:
  - phase: 04-tui-dashboard
    provides: "AgentEvent enum, AppState apply_event, TUI event channel infrastructure"
provides:
  - "SubAgentManager registry with thread-safe registration, status, query, cancel, shutdown"
  - "SubAgentId, SubAgentKind, SubAgentStatus, SubAgentInfo, SubAgentResult type definitions"
  - "CancellationToken hierarchy for cascading sub-agent shutdown"
  - "AgentEvent::SubAgentStatusChanged variant for TUI integration"
affects: [05-02, 05-03, 05-04, 05-05]

# Tech tracking
tech-stack:
  added: [tokio-util 0.7, uuid 1.x]
  patterns: [arc-mutex-hashmap registry, cancellation-token hierarchy, clone-friendly manager]

key-files:
  created:
    - src/orchestration/mod.rs
    - src/orchestration/types.rs
    - src/orchestration/manager.rs
  modified:
    - Cargo.toml
    - src/lib.rs
    - src/tui/event.rs
    - src/tui/app_state.rs

key-decisions:
  - "Arc<Mutex<HashMap>> over DashMap -- negligible contention for <20 agents, avoids extra dependency"
  - "uuid version 1.x (not 4.x as plan stated) -- actual crate versioning corrected"
  - "tokio-util default features (no sync feature) -- CancellationToken available without feature flag"
  - "SubAgentManager is Clone (all fields Arc/Clone) rather than requiring Arc wrapper"
  - "Terminal status states (Completed/Failed/Killed) auto-set completed_at timestamp"

patterns-established:
  - "Registry pattern: Arc<Mutex<HashMap<Id, Entry>>> with public Info snapshots and private Entry internals"
  - "Cancellation hierarchy: create_child_token(parent_id) for cascading shutdown"
  - "Event emission: manager emits AgentEvent variants through optional UnboundedSender"

# Metrics
duration: 4min
completed: 2026-02-05
---

# Phase 5 Plan 1: Orchestration Types and Manager Summary

**SubAgentManager registry with Arc<Mutex<HashMap>> backend, CancellationToken hierarchy, depth/count limits, and 14 unit tests**

## Performance

- **Duration:** 4 min
- **Started:** 2026-02-05T01:33:06Z
- **Completed:** 2026-02-05T01:37:04Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments
- Created orchestration module with complete type definitions (SubAgentId, SubAgentKind, SubAgentStatus, SubAgentInfo, SubAgentResult) all deriving Serialize
- Implemented SubAgentManager with 19 public methods covering registration, status management, queries, cancellation, shutdown, stdin/output buffer handling
- CancellationToken hierarchy enables cascading shutdown from root to all nested sub-agents
- Depth and total count limits enforced at registration time with descriptive error messages
- 14 unit tests covering all core registry operations

## Task Commits

Each task was committed atomically:

1. **Task 1: Add dependencies and create orchestration types** - `a5a5830` (feat)
2. **Task 2: Implement SubAgentManager registry** - `69fab4b` (feat)

## Files Created/Modified
- `Cargo.toml` - Added tokio-util and uuid dependencies
- `src/lib.rs` - Added pub mod orchestration
- `src/orchestration/mod.rs` - Module exports for types and manager
- `src/orchestration/types.rs` - SubAgentId, SubAgentKind, SubAgentStatus, SubAgentInfo, SubAgentResult
- `src/orchestration/manager.rs` - SubAgentManager with full registry API and 14 unit tests
- `src/tui/event.rs` - Added AgentEvent::SubAgentStatusChanged variant
- `src/tui/app_state.rs` - No-op handler for SubAgentStatusChanged (TUI wiring in later plan)

## Decisions Made
- Used `Arc<Mutex<HashMap>>` instead of `DashMap` per research recommendation -- contention negligible for <20 concurrent agents
- Corrected uuid dependency to version 1.x (plan referenced version "4" which is the UUID version, not the crate version)
- Removed `sync` feature from tokio-util (does not exist in 0.7.x; CancellationToken is in default features)
- SubAgentManager derives Clone directly (all fields are Arc/Clone) rather than requiring external Arc wrapping
- Terminal status transitions automatically set completed_at timestamp

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Corrected tokio-util feature flag**
- **Found during:** Task 1 (dependency setup)
- **Issue:** Plan specified `tokio-util = { version = "0.7", features = ["sync"] }` but tokio-util 0.7.x has no `sync` feature
- **Fix:** Changed to `tokio-util = "0.7"` (CancellationToken available in default features)
- **Files modified:** Cargo.toml
- **Verification:** cargo check passes
- **Committed in:** a5a5830 (Task 1 commit)

**2. [Rule 3 - Blocking] Corrected uuid crate version**
- **Found during:** Task 1 (dependency setup)
- **Issue:** Plan specified `uuid = { version = "4", features = ["v4"] }` but the crate is at version 1.x (the "4" refers to UUID v4 format, not the crate version)
- **Fix:** Changed to `uuid = { version = "1", features = ["v4"] }`
- **Files modified:** Cargo.toml
- **Verification:** cargo check passes, uuid 1.20.0 resolved
- **Committed in:** a5a5830 (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (2 blocking -- incorrect dependency specifications)
**Impact on plan:** Both fixes were necessary for compilation. No scope creep.

## Issues Encountered
None beyond the dependency specification corrections documented above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Orchestration types and manager are ready for use by subsequent plans
- Plan 05-02 (LLM sub-agent spawning) can build on SubAgentManager.register() and create_child_token()
- Plan 05-03 (background processes) can use set_stdin/take_stdin/set_output_buffer/read_output
- Plan 05-04 (tool dispatch) can use get_status/get_result/list_all/cancel_agent
- Plan 05-05 (TUI integration) can wire SubAgentStatusChanged events

---
*Phase: 05-sub-agent-orchestration*
*Completed: 2026-02-05*
