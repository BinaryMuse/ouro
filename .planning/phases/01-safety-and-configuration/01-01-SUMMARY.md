---
phase: 01-safety-and-configuration
plan: 01
subsystem: config
tags: [rust, clap, toml, serde, tokio, config-layering, cli]

# Dependency graph
requires:
  - phase: none
    provides: "First plan - no dependencies"
provides:
  - "Compiling Rust project with all Phase 1 dependencies"
  - "CLI argument parsing with Run and Resume subcommands"
  - "Layered config loading (global -> workspace -> CLI -> defaults)"
  - "Typed error enums (ConfigError, GuardrailError, ExecError)"
  - "Module stubs for safety and exec subsystems"
affects: [01-02, 01-03, 01-04, 02-01]

# Tech tracking
tech-stack:
  added: [clap 4.5, toml 0.9, serde 1.0, tokio 1, regex 1.12, directories 5, anyhow 1, thiserror 1, tracing 0.1, nix 0.29]
  patterns: [PartialConfig with_fallback merge, ConfigFile.to_partial conversion, CLI-to-PartialConfig bridge]

key-files:
  created:
    - Cargo.toml
    - src/main.rs
    - src/cli.rs
    - src/error.rs
    - src/config/mod.rs
    - src/config/schema.rs
    - src/config/merge.rs
    - src/safety/mod.rs
    - src/safety/command_filter.rs
    - src/safety/workspace.rs
    - src/safety/defaults.rs
    - src/exec/mod.rs
    - src/exec/shell.rs
  modified: []

key-decisions:
  - "PartialConfig with Option fields for merge-friendly config layering"
  - "Replace semantics for blocked_patterns (workspace replaces global entirely)"
  - "Security log defaults to workspace/security.log when not explicitly set"
  - "Missing config files logged at debug level, not treated as errors"

patterns-established:
  - "PartialConfig.with_fallback(fallback) for layered config merge"
  - "ConfigFile.to_partial() to convert TOML structs to merge-friendly format"
  - "cli_to_partial(cli) to convert CLI args into config merge layer"
  - "tracing::info for loaded configs, tracing::debug for missing configs"

# Metrics
duration: 4min
completed: 2026-02-04
---

# Phase 1 Plan 01: Project Scaffold and Config Summary

**Rust project scaffold with clap CLI (Run/Resume subcommands), layered TOML config system (CLI > workspace > global > defaults), and typed error enums for all Phase 1 modules**

## Performance

- **Duration:** 3 min 39 sec
- **Started:** 2026-02-04T19:08:06Z
- **Completed:** 2026-02-04T19:11:45Z
- **Tasks:** 2
- **Files modified:** 13

## Accomplishments
- Initialized ouro binary crate with 12 dependencies covering CLI, config, async, safety, and logging
- Full clap derive CLI with `ouro run` (--model, --workspace, --timeout, --config) and `ouro resume` (--workspace)
- Layered config system: global (~/.config/ouro/ouro.toml) -> workspace (workspace/ouro.toml) -> CLI args -> defaults
- 7 unit tests validating merge precedence, replace semantics for blocklists, and default resolution
- Typed error enums (ConfigError, GuardrailError, ExecError) ready for all Phase 1 modules
- Module stubs for safety (command_filter, workspace, defaults) and exec (shell) ready for subsequent plans

## Task Commits

Each task was committed atomically:

1. **Task 1: Scaffold Rust project with all dependencies and module stubs** - `cd0106e` (feat)
2. **Task 2: Implement config loading with layered merge and TOML parsing** - `8c21fdb` (feat)

## Files Created/Modified
- `Cargo.toml` - Project manifest with all Phase 1 dependencies
- `src/main.rs` - Entry point: tokio main, tracing init, CLI parse, config load
- `src/cli.rs` - Clap derive CLI with Run and Resume subcommands
- `src/error.rs` - Typed error enums (ConfigError, GuardrailError, ExecError)
- `src/config/mod.rs` - Config loading public API: load_config(), TOML file loading, CLI conversion
- `src/config/schema.rs` - ConfigFile, AppConfig, PartialConfig, BlocklistEntry structs
- `src/config/merge.rs` - with_fallback() merge logic, finalize() default resolution, 7 unit tests
- `src/safety/mod.rs` - Safety module declarations
- `src/safety/command_filter.rs` - CommandFilter with RegexSet, check() method
- `src/safety/workspace.rs` - WorkspaceGuard with canonical path validation
- `src/safety/defaults.rs` - Default blocklist (20 patterns: sudo, rm root, system writes, etc.)
- `src/exec/mod.rs` - Exec module declaration
- `src/exec/shell.rs` - ExecResult struct definition

## Decisions Made
- PartialConfig uses Option fields for all config values, enabling clean `or()` merge semantics
- blocked_patterns uses REPLACE semantics (if workspace specifies patterns, global is fully replaced, not merged)
- Security log path defaults to `workspace/security.log` when not explicitly configured
- Missing config files are handled gracefully with debug-level logging, never treated as errors
- Edition 2024 used (Rust 1.93 available)

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Project compiles and all config infrastructure is in place
- Safety module stubs (command_filter, workspace, defaults) are ready for plans 01-02 and 01-03
- Exec module stub (shell) is ready for plan 01-04
- Config system is complete and tested, providing AppConfig to all downstream modules

---
*Phase: 01-safety-and-configuration*
*Completed: 2026-02-04*
