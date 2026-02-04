# Phase 4: TUI Dashboard - Context

**Gathered:** 2026-02-04
**Status:** Ready for planning

<domain>
## Phase Boundary

Real-time four-panel terminal interface for monitoring and controlling the running agent. The user can observe agent thoughts, tool calls, results, sub-agent status, and flagged discoveries without blocking agent execution. Keyboard controls allow pause/resume, scrolling, and panel navigation. The TUI is a read/control layer over the existing agent loop -- it does not change agent behavior, only how the user observes and controls it.

</domain>

<decisions>
## Implementation Decisions

### Panel layout & arrangement
- Tabbed interface, not all-panels-visible-at-once
- Tab 1 (main): Log stream (70%) stacked above sub-agent tree (30%)
- Tab 2: Discoveries list (simple scrollable list, most recent at top)
- Persistent two-line status bar below all tabs (always visible regardless of active tab)
- Tab bar visible at top of content area with tab names; active tab highlighted
- Bordered panels with title labels for clear section separation
- Sub-agent panel visibility is user-toggleable (not auto-collapse) -- when hidden, log expands to full height
- Graceful degradation on small terminals: collapse panels, truncate content, hide less important elements progressively

### Log stream presentation
- Structured blocks: each log entry is a distinct visual block with a header line showing type, then indented content
- Color-coded content per type (thoughts, tool calls, results, errors)
- Icons/symbols as prefixes for each entry type
- Tool results collapsed by default: one-line summary (e.g., "shell: 42 lines of output"), user can expand to see full output
- Auto-scroll follows latest entry by default; pauses when user scrolls up; "jump to bottom" action resumes auto-scroll
- Absolute wall-clock timestamps on each log entry (e.g., "14:32:07")

### Keyboard controls & navigation
- Arrow keys + single-letter shortcuts (not vim-style)
- Tab/Shift-Tab to switch between tabs
- Arrow keys for scrolling within panels
- Single-letter shortcuts for actions (p=pause, q=quit, etc.)
- Pause behavior: let current tool finish executing, then pause before next LLM call
- Quit requires confirmation (second q press or y/n prompt) to prevent accidental exit
- Keybind hints always visible in bottom status bar line

### Status indicators & visual feedback
- Agent state shown in status bar: Thinking / Executing / Idle / Paused (four distinct states)
- Context window pressure: small colored progress bar + percentage text, color transitions green -> yellow -> red as pressure increases
- Session restart shown as log separator line only ("--- Session N started ---"), status bar updates session count quietly
- Running counters in status bar: turn count + tool call count for current session (e.g., "Turn 12 | Tools: 47")
- Status bar line 1: agent state, context pressure bar, session number, turn/tool counters
- Status bar line 2: keybind hints for current context

### Claude's Discretion
- Exact color palette and theme
- Icon/symbol choices for log entry types
- Specific Ratatui widget selection and composition
- Graceful degradation breakpoints and what gets hidden first
- Exact formatting of collapsed tool result summaries
- Animation/transition behavior (if any)

</decisions>

<specifics>
## Specific Ideas

- Tab 1 as "Agent" tab, Tab 2 as "Discoveries" tab (naming)
- Sub-agent tree panel should feel like a compact process list, not a full dashboard
- Collapsed tool results should indicate size ("42 lines") so user can judge whether to expand
- Status bar should feel information-dense but scannable -- like a tmux or vim status line

</specifics>

<deferred>
## Deferred Ideas

None -- discussion stayed within phase scope

</deferred>

---

*Phase: 04-tui-dashboard*
*Context gathered: 2026-02-04*
