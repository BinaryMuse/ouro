# Phase 1: Safety & Configuration - Context

**Gathered:** 2026-02-04
**Status:** Ready for planning

<domain>
## Phase Boundary

The harness enforces workspace-scoped execution boundaries and loads user-specified configuration before any agent code runs. This phase builds the sandbox and config systems that all subsequent phases depend on. It does NOT include the agent loop, tools, or TUI.

</domain>

<decisions>
## Implementation Decisions

### Command filtering strategy
- Blocklist approach: block known-dangerous patterns, allow everything else
- Ship with sensible defaults (sudo, rm -rf /, shutdown, privilege escalation, etc.) that user can fully override
- Blocklist is fully configurable: user can add, remove, or replace any entry via config
- Agent CAN install packages (pip install, cargo add, npm install) without restriction
- Agent CAN make outbound network connections (HTTP, SSH, any protocol) without restriction
- Agent CAN listen on ports (start servers, bind sockets)
- When a command is blocked: reject with structured error (JSON-like: blocked, reason, command)
- Blocked commands logged to a separate security log for user review
- No dry-run mode for v1

### Workspace boundary enforcement
- Read anywhere, write workspace only: agent can read the full filesystem but only write inside its workspace
- Symlinks allowed: agent can create symlinks pointing outside workspace (not blocked)
- Workspace is persistent and can be pre-populated: user puts files in it, they persist across sessions
- Harness creates workspace directory if it doesn't exist
- Path validation: resolve to canonical path on startup, all write operations checked against it

### Config file format & CLI design
- TOML config (ouro.toml) - Rust ecosystem standard
- CLI uses subcommands: `ouro run`, `ouro resume`, etc.
- Precedence: CLI args override workspace config, workspace config overrides global config
- Config search path: global (~/.config/ouro/ouro.toml) then workspace-local (workspace/ouro.toml), workspace overrides global
- Since the config lives in the workspace too, the agent could technically modify its own config

### Error reporting to the agent
- Blocked commands return structured error to the agent: `{ blocked: true, reason: "...", command: "..." }`
- Timed-out commands return partial output captured before the kill, plus a timeout indicator
- Guardrail configuration (what's blocked, timeout values) included in the system prompt so the agent knows its constraints upfront
- Blocked commands also written to a separate security log for the human to review

### Claude's Discretion
- Exact default blocklist entries (should cover obvious dangerous patterns)
- Specific TOML config file schema and field names
- CLI argument names and help text
- How path canonicalization handles edge cases

</decisions>

<specifics>
## Specific Ideas

- Config layering is global -> workspace -> CLI (each layer overrides the previous)
- The agent seeing its own constraints in the system prompt is important for the self-bootstrapping philosophy: the agent should know what it can and can't do
- Security log for blocked commands is separate from the main action log so the user can review guardrail hits without wading through normal activity

</specifics>

<deferred>
## Deferred Ideas

None -- discussion stayed within phase scope

</deferred>

---

*Phase: 01-safety-and-configuration*
*Context gathered: 2026-02-04*
