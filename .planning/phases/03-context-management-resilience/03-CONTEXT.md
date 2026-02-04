# Phase 3: Context Management & Resilience - Context

**Gathered:** 2026-02-04
**Status:** Ready for planning

<domain>
## Phase Boundary

The harness detects context window pressure and manages the agent's conversation history so it can run indefinitely without losing progress. This includes token tracking, observation masking (compressing old tool output), graceful session restart when context is exhausted, and continuity mechanisms so the agent doesn't repeat work across restarts.

</domain>

<decisions>
## Implementation Decisions

### Observation masking
- Placeholders include a **summary** of what was there, not just a size marker (e.g. "[file_read: src/main.rs -- 142 lines, Rust source with main() entry point]")
- Masking is **incremental, oldest first** -- each time pressure increases, mask the next oldest unmasked observation
- Agent receives a **system notification** when masking occurs (e.g. "[Context compressed: 12 observations masked, 40% context reclaimed]")

### Restart behavior
- **Graceful wind-down**: agent gets a "context running low, wrap up" message before session ends, giving it a chance to write state to disk
- On restart, context contains: **system prompt + last N turns from previous session** (carried over as seed context)
- Restart mode is **configurable, default fully automatic** -- harness detects exhaustion, ends session, starts new one. Config option to require pause/confirmation before restart
- **Configurable max restarts** -- config option for max session count, default unlimited

### Progress continuity
- Agent is responsible for writing its **own state/progress files** to the workspace -- no harness-managed state file
- SYSTEM_PROMPT.md is **fully writable** by the agent -- it can modify its own bootstrap instructions for future sessions (self-modification)
- Harness injects a **restart marker** message (e.g. "[Session restarted. Session #3. Previous session ran 47 turns.]") so the agent knows it restarted
- Number of **carryover turns is configurable** (how many turns from previous session seed the new one)

### Token tracking
- Use **Ollama response metadata** (prompt_eval_count, eval_count) for actual token usage -- replaces the Phase 2 character-count heuristic
- **Graduated thresholds**: soft threshold starts masking, hard threshold triggers wind-down and restart
- Threshold percentages are **configurable with sensible defaults** (e.g. 70% soft, 90% hard)
- Token counts per turn **logged to JSONL session log** -- also designed to be consumed by TUI in Phase 4

### Claude's Discretion
- Which parts of conversation are eligible for masking (tool outputs only vs also long agent responses)
- Exact placeholder format and summary generation approach
- Default values for carryover turns and threshold percentages
- How the "wrap up" wind-down message is worded

</decisions>

<specifics>
## Specific Ideas

- Agent can "hack" its own bootstrap by modifying SYSTEM_PROMPT.md -- this is intentional and core to the Ouroboros philosophy of self-sustaining agents
- Token usage should be surfaced in the TUI dashboard (Phase 4 will consume the logged data)
- The graduated threshold model: masking is a soft response, restart is a hard response -- two different levels of intervention

</specifics>

<deferred>
## Deferred Ideas

None -- discussion stayed within phase scope

</deferred>

---

*Phase: 03-context-management-resilience*
*Context gathered: 2026-02-04*
