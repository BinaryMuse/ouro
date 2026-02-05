---
phase: 05-sub-agent-orchestration
plan: 02
subsystem: orchestration
tags: [tokio, sub-agent, process-management, ring-buffer, cancellation-token]

# Dependency graph
requires:
  - phase: 05-01
    provides: SubAgentManager registry with types, CancellationToken hierarchy, depth/count limits
  - phase: 02-03
    provides: run_agent_session conversation loop for LLM sub-agent reuse
  - phase: 01-04
    provides: SafetyLayer for workspace-scoped command execution
provides:
  - spawn_llm_sub_agent function for child LLM sessions with goal-directed prompts
  - spawn_background_process function for long-lived shell processes with piped I/O
  - SessionLogger::new_in_dir for sub-agent-specific log directories
affects: [05-03-tool-dispatch, 05-04-tui-integration, 05-05-integration-testing]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "CancellationToken-to-AtomicBool bridge for run_agent_session integration"
    - "Ring buffer output capture via Arc<Mutex<VecDeque<String>>> with 1000-line capacity"
    - "Process group spawning with kill_on_drop safety net for background processes"
    - "Goal-directed sub-agent system prompt (no SYSTEM_PROMPT.md dependency)"

key-files:
  created:
    - src/orchestration/llm_agent.rs
    - src/orchestration/background_proc.rs
  modified:
    - src/orchestration/mod.rs
    - src/agent/logging.rs

key-decisions:
  - "Sub-agent prompt injected as carryover system message to run_agent_session (avoids modifying workspace SYSTEM_PROMPT.md)"
  - "CancellationToken bridged to AtomicBool via small spawned task (run_agent_session expects AtomicBool shutdown flag)"
  - "Background processes spawn via tokio::process::Command directly (bypass SafetyLayer.execute which is designed for fire-and-forget)"
  - "Output ring buffer capped at 1000 lines with pop_front eviction on overflow"
  - "Stderr lines prefixed with [stderr] in shared buffer to distinguish from stdout"

patterns-established:
  - "CancellationToken bridge: spawn small task that sets AtomicBool when token cancelled"
  - "Ring buffer I/O: separate reader tasks per stream, shared VecDeque with mutex"
  - "Process group management: process_group(0) + killpg for clean multi-process shutdown"

# Metrics
duration: 4min
completed: 2026-02-05
---

# Phase 5 Plan 2: Sub-Agent Spawning Functions Summary

**LLM sub-agent spawner reusing run_agent_session with goal-directed prompts, and background process spawner with piped stdin/ring-buffer output capture and process-group cancellation**

## Performance

- **Duration:** 4 min
- **Started:** 2026-02-05T01:39:56Z
- **Completed:** 2026-02-05T01:43:57Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- LLM sub-agent spawner that runs full conversation loops via run_agent_session with custom goal-based system prompts
- Background process spawner with piped stdin/stdout/stderr and 1000-line ring buffer output capture
- Both spawners integrate with SubAgentManager for lifecycle tracking, CancellationToken for shutdown, and SubAgentResult for completion reporting
- Added SessionLogger::new_in_dir constructor for sub-agent-specific log directories

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement LLM sub-agent spawner** - `94b4c13` (feat)
2. **Task 2: Implement background process spawner** - `ed6ac1a` (feat)

## Files Created/Modified
- `src/orchestration/llm_agent.rs` - spawn_llm_sub_agent function with goal-directed system prompt, CancellationToken bridge, and run_agent_session integration
- `src/orchestration/background_proc.rs` - spawn_background_process function with piped I/O, ring buffer capture, process group management, and cancel support
- `src/orchestration/mod.rs` - Module declarations for llm_agent and background_proc
- `src/agent/logging.rs` - Added SessionLogger::new_in_dir for custom log directories

## Decisions Made
- **Sub-agent prompt as carryover:** Rather than modifying workspace SYSTEM_PROMPT.md or bypassing run_agent_session entirely, the sub-agent's goal prompt is injected as a system carryover message. This preserves run_agent_session's full feature set (streaming, tool dispatch, context management) while giving the sub-agent a distinct mission.
- **CancellationToken-to-AtomicBool bridge:** run_agent_session expects an `Arc<AtomicBool>` for shutdown signaling, but the orchestration system uses CancellationTokens. A small bridging task converts between the two by watching the token and setting the flag.
- **Direct process spawning for background processes:** Background processes bypass SafetyLayer.execute() (designed for fire-and-forget with timeout) and use tokio::process::Command directly, since they need persistent stdin/stdout handles and long-lived execution. Workspace scoping is enforced via current_dir.
- **1000-line ring buffer capacity:** A reasonable default that captures sufficient context for monitoring while bounding memory usage. Lines evicted FIFO via pop_front.
- **Stderr prefixed with [stderr]:** Merged into the same ring buffer as stdout for unified reading, but prefixed for source identification.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added SessionLogger::new_in_dir constructor**
- **Found during:** Task 1 (LLM sub-agent spawner)
- **Issue:** SessionLogger only had `new(workspace_path)` which computes the log directory internally. Sub-agents need a custom log directory (`sub-{id}/`).
- **Fix:** Added `pub fn new_in_dir(log_dir: &Path)` that creates a logger writing to an arbitrary directory.
- **Files modified:** src/agent/logging.rs
- **Verification:** All 10 existing logging tests pass, cargo check clean
- **Committed in:** 94b4c13 (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Minimal - added a necessary public constructor to enable the planned sub-agent logging feature. No scope creep.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Both spawning functions are ready for tool dispatch integration (05-03)
- The manager API (register, set_result, update_status, set_stdin, set_output_buffer) is exercised by both spawners
- TUI integration (05-04) can wire SubAgentStatusChanged events to the sub-agent panel
- Integration testing (05-05) can exercise the full spawn-to-completion lifecycle

---
*Phase: 05-sub-agent-orchestration*
*Completed: 2026-02-05*
