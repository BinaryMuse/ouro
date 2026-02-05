# Phase 6: Extended Tools & Discovery - Context

**Gathered:** 2026-02-04
**Status:** Ready for planning

<domain>
## Phase Boundary

The agent gains four new tool capabilities: fetching web content by URL, searching the internet, pausing itself with configurable resume, and flagging noteworthy discoveries for the user. All tools integrate with the existing tool dispatch system and TUI.

</domain>

<decisions>
## Implementation Decisions

### Web fetch behavior
- Agent chooses output format per call: markdown conversion or raw HTML
- HTML pages converted to markdown or returned raw based on agent's parameter choice
- JSON responses returned as-is (no conversion)
- All other content types (PDF, images, binary) not handled by this tool — agent uses bash curl for those
- Simple HTTP GET only — follow redirects, no JavaScript rendering, no cookie management, no headless browser
- Full content returned by default; agent can pass an optional limit parameter to truncate response

### Search integration
- DuckDuckGo as zero-config default provider (no API key needed)
- Brave Search as secondary provider when user sets API key in config
- Results returned as structured list: title, URL, snippet per result
- Agent specifies result count per search call (no fixed default)
- Built-in rate limiting between search requests to avoid getting blocked

### Sleep/pause mechanics
- Three resume modes: timer-based (agent specifies duration), event-based (wait for sub-agent/process completion), and user-controlled (manual resume from TUI)
- Sleep status shown as log entry in main log AND countdown/status in the status bar
- Configurable maximum sleep duration in config file (prevents indefinite dormancy) — user can always manually resume from TUI
- Event-based sleep: if awaited thing fails, agent wakes immediately with failure details and can choose to re-sleep or handle the error

### Discovery system
- Agent flags discoveries via tool call — title and description fields
- System prompt guides the agent on what qualifies as a "discovery" (anything the agent judges would be useful to surface to the user)
- Discoveries persisted to disk in the workspace — survive session restarts and context resets
- TUI shows discoveries as a simple scrollable chronological list, newest at top
- Read-only in TUI — agent manages the discovery list, user observes

### Claude's Discretion
- Rate limiting intervals for search providers
- Discovery file format on disk (JSON, JSONL, etc.)
- Exact markdown conversion approach/library for web fetch
- Status bar formatting for sleep countdown
- Maximum sleep duration default value

</decisions>

<specifics>
## Specific Ideas

- Web fetch tool should feel lightweight — just HTTP GET with content extraction, not a browser
- Search should work out of the box with DDG, Brave is an upgrade path for users who want better results
- Agent can surface discoveries at any time — the system prompt teaches it what's worth flagging

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 06-extended-tools-discovery*
*Context gathered: 2026-02-04*
