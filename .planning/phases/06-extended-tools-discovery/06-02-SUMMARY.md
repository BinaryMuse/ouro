---
phase: 06-extended-tools-discovery
plan: 02
subsystem: agent-modules
tags: [discovery, sleep, jsonl, state-machine, persistence]
completed: 2026-02-05
duration: 3 min
dependency-graph:
  requires: [phase-01-config, phase-02-tools]
  provides: [discovery-persistence, sleep-state-machine]
  affects: [06-03-tool-dispatch, 06-04-agent-loop-integration]
tech-stack:
  added: []
  patterns: [jsonl-append-persistence, state-machine-types]
key-files:
  created:
    - src/agent/discovery.rs
    - src/agent/sleep.rs
  modified:
    - src/agent/mod.rs
decisions:
  - "Discovery uses JSONL file at workspace/.ouro-discoveries.jsonl (matches SessionLogger pattern)"
  - "Lenient JSONL reader silently skips unparseable lines for crash resilience"
  - "SleepState uses std::time::Instant for elapsed tracking (no async dependency)"
  - "parse_sleep_args clamps requested duration to config max_sleep_duration_secs"
  - "Event mode uses max_sleep_duration_secs as safety timeout"
  - "format_duration produces h/m/s display (e.g., 2m 34s)"
metrics:
  tasks: 2/2
  tests-added: 27
  commits: 2
---

# Phase 6 Plan 02: Discovery Persistence & Sleep State Machine Summary

Standalone discovery JSONL persistence and sleep state machine types for agent-initiated pause with timer/event/manual modes.

## What Was Done

### Task 1: Discovery Persistence Module (`c764fb7`)

Created `src/agent/discovery.rs` with complete JSONL-backed discovery storage:

- **Discovery struct**: timestamp, title, description fields with Serialize/Deserialize
- **discovery_file_path()**: Returns `workspace/.ouro-discoveries.jsonl`
- **append_discovery()**: Opens with `OpenOptions::create(true).append(true)`, writes via BufWriter with explicit flush
- **load_discoveries()**: Lenient JSONL reader -- returns empty Vec if file missing, skips unparseable lines silently

5 unit tests: roundtrip, nonexistent file, corrupt line tolerance, multiple appends, path construction.

### Task 2: Sleep State Machine Module (`39ee743`)

Created `src/agent/sleep.rs` with the sleep types and argument parser:

- **SleepMode enum**: Timer(Duration), Event { agent_id }, Manual
- **SleepState struct**: active, mode, started_at (Instant), max_duration, wake_reason
- **SleepState methods**: new(), elapsed(), remaining_display(), is_expired(), wake()
- **parse_sleep_args()**: Validates JSON args, extracts mode-specific fields, clamps duration to max
- **format_duration()**: Human-readable display ("2m 34s", "1h 0m 30s")

22 unit tests covering: construction, display for all modes, expiry, wake, parse valid/invalid args, clamping, format_duration edge cases.

## Deviations from Plan

None -- plan executed exactly as written.

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| Lenient JSONL reader (skip corrupt lines) | Crash resilience: partial writes only corrupt last line, rest loads fine |
| std::time::Instant for SleepState.started_at | No async dependency needed for elapsed tracking; Instant is monotonic |
| Duration clamping in parse_sleep_args | Prevents indefinite dormancy; config max_sleep_duration_secs is the safety bound |
| Event mode defaults to max duration as timeout | Events complete naturally; timeout is only a safety fallback |
| Empty agent_id rejected in event mode | Prevents silent no-op sleep with no wake condition |

## Verification Results

| Check | Result |
|-------|--------|
| `cargo check` | Pass (only dead_code warnings for unwired modules -- expected) |
| `cargo test -- discovery` | 6 passed (5 new + 1 pre-existing TUI test) |
| `cargo test -- sleep` | 22 passed |
| discovery.rs exports | Discovery, append_discovery, load_discoveries, discovery_file_path |
| sleep.rs exports | SleepMode, SleepState, parse_sleep_args |

## Next Phase Readiness

Plan 03 (tool dispatch wiring) can now import:
- `agent::discovery::{Discovery, append_discovery, load_discoveries, discovery_file_path}`
- `agent::sleep::{SleepMode, SleepState, parse_sleep_args}`

Both modules are standalone with no dependency on tools.rs dispatch -- ready for wiring.
