# Phase 6: Extended Tools & Discovery - Research

**Researched:** 2026-02-04
**Domain:** HTTP fetching, web search integration, async sleep/pause mechanics, discovery persistence
**Confidence:** HIGH (codebase patterns well-established; external libraries verified)

## Summary

Phase 6 adds four new tool capabilities to the agent: web content fetching, internet search, self-pause/sleep, and discovery flagging. All four tools follow the established `dispatch_tool_call` pattern from `src/agent/tools.rs`, returning JSON strings (never `Err`). The existing codebase already has `reqwest` as a dependency (used for Ollama health checks), so web fetch and search tools can reuse it. The TUI already has a Discoveries tab with placeholder rendering and an `AgentEvent::Discovery` variant, meaning the discovery tool's TUI integration is partially built.

The sleep/pause tool is the most architecturally novel feature. The agent loop already supports a `pause_flag: Option<Arc<AtomicBool>>` for user-initiated pauses, and the pattern of checking state between turns is well-established. The agent-initiated sleep tool needs a new mechanism: the tool dispatch returns immediately with a sleep ID, and a separate async task manages the countdown/event-wait, waking the agent by injecting a system message or unblocking the loop.

**Primary recommendation:** Build all four tools as new branches in the existing `dispatch_tool_call` match, add their schemas to `define_tools`, and add their descriptions to `tool_descriptions`. Use `reqwest` (already a dependency) for HTTP. Use `htmd` for HTML-to-markdown conversion. Implement DuckDuckGo search via direct HTML scraping with `reqwest` + `scraper`, and Brave Search via their REST API with `reqwest`. Implement sleep as a state machine integrated into the agent loop's between-turn check. Persist discoveries as a JSONL file in the workspace.

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `reqwest` | 0.12 | HTTP client for web fetch and search | Already a dependency; async, follows redirects, cookie-store capable |
| `htmd` | latest | HTML to Markdown conversion | Turndown.js-inspired; lightweight; uses html5ever for correct parsing; thread-safe |
| `scraper` | latest | HTML parsing for DuckDuckGo result extraction | Standard Rust HTML parser; uses html5ever + CSS selectors; widely used |
| `tokio` | 1.x | Async runtime for timers, sleep, futures | Already a dependency with all needed features enabled |
| `serde_json` | 1.0 | Discovery file serialization | Already a dependency |
| `chrono` | 0.4 | Timestamps for discoveries | Already a dependency |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `tokio::time` | (part of tokio) | Timer-based sleep implementation | Agent sleep tool timer mode |
| `tokio::sync::oneshot` | (part of tokio) | Event-based sleep wakeup signal | Agent sleep tool event mode |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `htmd` | `html-to-markdown-rs` | More features (table colspan, hOCR) but much heavier dependency tree; `htmd` is sufficient for web page extraction |
| `htmd` | `html2md` | Older, less maintained; `htmd` has better API and is actively developed |
| `scraper` (for DDG) | `duckduckgo` crate | Ready-made DDG scraping, but adds dependency on unmaintained crate; rolling our own with reqwest+scraper is more reliable and controllable |
| `scraper` (for DDG) | `websearch` crate | Multi-provider support but v0.1.1, early stage; too immature to depend on |
| Custom Brave API | `brave-cli` crate | CLI-only, not a library; we need programmatic access via reqwest |

**New dependencies to add to Cargo.toml:**
```toml
# HTML to Markdown conversion for web_fetch tool
htmd = "0.1"

# HTML parsing for DuckDuckGo search result extraction
scraper = "0.22"
```

**Note:** `reqwest`, `tokio`, `serde_json`, and `chrono` are already dependencies. No new async runtime or HTTP client needed.

## Architecture Patterns

### Recommended Module Structure

New code integrates into existing modules. No new top-level modules needed.

```
src/
├── agent/
│   ├── tools.rs             # Add 4 new tool schemas + dispatch branches
│   ├── system_prompt.rs     # Add discovery guidance to harness prompt
│   └── agent_loop.rs        # Add sleep state machine to between-turn logic
├── config/
│   └── schema.rs            # Add search/sleep config fields
├── tui/
│   ├── app_state.rs         # Add sleep state tracking fields
│   ├── tabs/
│   │   └── discoveries_tab.rs  # Enhance with title+description display
│   └── widgets/
│       └── status_bar.rs    # Add sleep countdown/status display
└── (new files)
    ├── agent/web_fetch.rs   # HTTP fetch + markdown conversion logic
    ├── agent/web_search.rs  # DDG + Brave search providers
    ├── agent/sleep.rs       # Sleep state machine (timer/event/manual)
    └── agent/discovery.rs   # Discovery persistence (JSONL read/write)
```

### Pattern 1: Tool Dispatch Extension

**What:** Add new tool names to the existing `define_tools` Vec and `dispatch_tool_call` match.
**When to use:** Every new tool follows this exact pattern.
**Example from existing code:**

```rust
// In define_tools():
Tool::new("web_fetch")
    .with_description("Fetch a web page by URL and return its content...")
    .with_schema(json!({
        "type": "object",
        "properties": {
            "url": { "type": "string", "description": "The URL to fetch" },
            "format": { "type": "string", "enum": ["markdown", "html"], "description": "Output format" },
            "max_length": { "type": "integer", "description": "Optional truncation limit in characters" }
        },
        "required": ["url"]
    })),

// In dispatch_tool_call():
"web_fetch" => dispatch_web_fetch(call).await,
"web_search" => dispatch_web_search(call, config).await,
"sleep" => dispatch_sleep(call, manager, /* sleep_state */).await,
"flag_discovery" => dispatch_flag_discovery(call, workspace, event_tx).await,
```

### Pattern 2: Sleep State Machine (between-turn integration)

**What:** The sleep tool sets a flag/state that the agent loop checks between turns, blocking until the wake condition is met.
**When to use:** Sleep/pause mechanics.

The existing pause mechanism in `agent_loop.rs` provides the exact template:

```rust
// Existing pause pattern (lines 331-345 of agent_loop.rs):
if let Some(ref pf) = pause_flag
    && pf.load(Ordering::SeqCst)
{
    send_event(AgentEvent::StateChanged(AgentState::Paused));
    while pf.load(Ordering::SeqCst) && !shutdown.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // ...
}
```

The sleep tool extends this with three modes:
1. **Timer-based:** `tokio::time::sleep(duration)` in the spin-wait, checking elapsed time
2. **Event-based:** Check if the awaited agent_id has completed (via `manager.get_status()`)
3. **User-controlled:** Set a flag that the TUI can clear (identical to existing pause)

### Pattern 3: Discovery Persistence (JSONL append)

**What:** Discoveries written as JSONL to a file in the workspace, loaded on session start.
**When to use:** Discovery flagging and persistence.

This follows the exact pattern of `SessionLogger` in `agent/logging.rs`:
- Synchronous `std::fs` for small buffered writes with flush
- JSONL format (one JSON object per line)
- File lives in the workspace directory (not the log directory) so it survives context resets

### Anti-Patterns to Avoid

- **Blocking the agent loop on HTTP requests:** Web fetch and search are async tool dispatches -- they already run in the tokio runtime. Never use `std::thread::sleep()` or blocking HTTP in the tool dispatch.
- **Building a custom HTTP client:** `reqwest` is already configured and available. Do not hand-roll HTTP over `tokio::net::TcpStream`.
- **Using the `duckduckgo` crate for search:** Its dependency tree is heavy and it's a CLI tool primarily. Direct reqwest+scraper gives full control over request headers, rate limiting, and error handling.
- **Storing discoveries in the log directory:** The `.ouro-logs` directory is for session replay logs. Discoveries go in the workspace because they're user-facing data the agent and user both reference.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| HTML parsing | Custom regex parser | `scraper` crate with CSS selectors | HTML is complex; regex cannot handle nested tags, encoding, malformed HTML |
| HTML to Markdown | String replacement rules | `htmd` crate | Edge cases: nested lists, tables, code blocks, entities, whitespace normalization |
| HTTP client | Raw TCP + HTTP/1.1 | `reqwest` | TLS, redirects, encoding, connection pooling, cookie support all handled |
| Rate limiting | Custom timer logic | `tokio::time::Interval` | Accurate, cancellable, works with async runtime |
| JSON line parsing | Manual string splitting | `serde_json::from_str` per line | Handles escaping, unicode, nested objects correctly |

**Key insight:** The HTTP/HTML stack has deep edge cases (encoding, redirects, malformed HTML, entity decoding, connection reuse) that existing crates handle correctly. Custom solutions will break on real-world web pages.

## Common Pitfalls

### Pitfall 1: DuckDuckGo Rate Limiting / Blocking

**What goes wrong:** Sending too many requests too quickly causes DDG to return CAPTCHAs or empty results.
**Why it happens:** DDG has anti-scraping measures, though they are relatively lenient compared to Google.
**How to avoid:**
- Built-in delay between search requests (recommended: 1-2 seconds minimum between DDG requests)
- Set a realistic User-Agent header (e.g., "Mozilla/5.0 ...")
- Use the DDG lite HTML endpoint (`https://lite.duckduckgo.com/lite/`) which is simpler to parse and more tolerant of programmatic access
**Warning signs:** Empty results, HTML containing CAPTCHA form, HTTP 429 responses

### Pitfall 2: Brave Search API Key Not Set

**What goes wrong:** Agent tries to use Brave search when no API key is configured.
**Why it happens:** Brave is an optional provider requiring `X-Subscription-Token` header.
**How to avoid:**
- Config schema has an optional `brave_api_key` field
- `dispatch_web_search` checks config for key presence before attempting Brave
- Return clear error JSON: `{"error": "Brave Search requires API key in config"}`
- Tool description tells the agent that DDG is always available, Brave requires config
**Warning signs:** HTTP 401 from Brave API

### Pitfall 3: Web Fetch Returning Enormous Content

**What goes wrong:** Fetching a large web page (e.g., documentation page, Wikipedia article) produces tens of thousands of characters, consuming context window.
**Why it happens:** No content truncation applied.
**How to avoid:**
- `max_length` optional parameter in the tool schema (agent specifies truncation limit)
- Default behavior: return full content (agent is responsible for using limit parameter wisely)
- System prompt guidance: "Use the max_length parameter for large pages"
**Warning signs:** Context pressure spikes after web_fetch calls

### Pitfall 4: Sleep Tool Blocking Context Consumption

**What goes wrong:** Agent sleeps but the conversation includes the full sleep duration wait, consuming turns.
**Why it happens:** Sleep is implemented as repeated polling in the agent loop.
**How to avoid:**
- Sleep tool dispatch returns immediately with a sleep confirmation JSON
- The agent loop's between-turn check blocks (like pause does now)
- No LLM calls happen during sleep -- the turn counter does not increment
- Wake event injects a system message ("You slept for X seconds. Reason: timer expired / agent completed / user resumed")
**Warning signs:** Turn count incrementing during sleep; LLM calls being made while sleeping

### Pitfall 5: Event-Based Sleep on Non-Existent Agent

**What goes wrong:** Agent sleeps waiting for an agent_id that doesn't exist or has already completed.
**Why it happens:** Race condition or typo in agent_id.
**How to avoid:**
- Validate agent_id exists in `SubAgentManager` before entering sleep
- If agent is already completed at sleep entry time, return immediately with the result
- If agent_id not found, return error JSON immediately (don't enter sleep)
**Warning signs:** Agent stuck in sleep with no way to wake (mitigated by max duration and manual resume)

### Pitfall 6: Discovery File Corruption

**What goes wrong:** Discovery JSONL file has corrupted lines due to partial writes.
**Why it happens:** Process crash mid-write.
**How to avoid:**
- Use `BufWriter` with explicit `flush()` after each line (same as SessionLogger)
- On read, skip unparseable lines (lenient JSONL reader)
- Each line is self-contained (JSONL property)
**Warning signs:** `serde_json::from_str` errors when loading discoveries

## Code Examples

### Web Fetch Tool Implementation Pattern

```rust
// Source: Codebase pattern from dispatch_file_read + reqwest docs
async fn dispatch_web_fetch(call: &genai::chat::ToolCall) -> String {
    let url = match call.fn_arguments.get("url").and_then(|v| v.as_str()) {
        Some(u) => u,
        None => return json!({"error": "web_fetch: missing 'url' argument"}).to_string(),
    };

    // Default format: markdown
    let format = call.fn_arguments.get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("markdown");

    let max_length = call.fn_arguments.get("max_length")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10))
        .user_agent("Mozilla/5.0 (compatible; Ouro/0.1)")
        .build()
        .unwrap_or_default();

    let response = match client.get(url).send().await {
        Ok(r) => r,
        Err(e) => return json!({"error": format!("web_fetch: {e}")}).to_string(),
    };

    let status = response.status();
    if !status.is_success() {
        return json!({"error": format!("web_fetch: HTTP {status}")}).to_string();
    }

    let content_type = response.headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let body = match response.text().await {
        Ok(t) => t,
        Err(e) => return json!({"error": format!("web_fetch: failed to read body: {e}")}).to_string(),
    };

    // JSON responses: return as-is
    if content_type.contains("application/json") {
        return maybe_truncate(&body, max_length);
    }

    // HTML: convert based on format parameter
    let output = if content_type.contains("text/html") && format == "markdown" {
        htmd::convert(&body).unwrap_or(body)
    } else {
        body
    };

    maybe_truncate(&output, max_length)
}

fn maybe_truncate(content: &str, max_length: Option<usize>) -> String {
    match max_length {
        Some(limit) if content.len() > limit => {
            format!("{}...\n[truncated at {} chars, total {}]",
                &content[..limit], limit, content.len())
        }
        _ => content.to_string(),
    }
}
```

### DuckDuckGo Search via HTML Scraping

```rust
// Source: Codebase pattern + scraper crate docs + DDG lite endpoint
use scraper::{Html, Selector};

async fn search_duckduckgo(query: &str, count: usize) -> Result<Vec<SearchResult>, String> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:120.0) Gecko/20100101 Firefox/120.0")
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    let resp = client
        .get("https://lite.duckduckgo.com/lite/")
        .query(&[("q", query)])
        .send()
        .await
        .map_err(|e| format!("DuckDuckGo request failed: {e}"))?;

    let html = resp.text().await
        .map_err(|e| format!("Failed to read DDG response: {e}"))?;

    let document = Html::parse_document(&html);
    // Parse result links, titles, and snippets from the lite HTML
    // DDG lite uses a simple table layout with result rows
    let mut results = Vec::new();
    // ... CSS selector parsing logic ...

    results.truncate(count);
    Ok(results)
}
```

### Brave Search via REST API

```rust
// Source: Brave Search API docs (https://api-dashboard.search.brave.com)
async fn search_brave(
    query: &str,
    count: usize,
    api_key: &str,
) -> Result<Vec<SearchResult>, String> {
    let client = reqwest::Client::new();

    let resp = client
        .get("https://api.search.brave.com/res/v1/web/search")
        .header("X-Subscription-Token", api_key)
        .header("Accept", "application/json")
        .query(&[
            ("q", query),
            ("count", &count.to_string()),
        ])
        .send()
        .await
        .map_err(|e| format!("Brave Search request failed: {e}"))?;

    if resp.status() == 429 {
        return Err("Brave Search rate limit exceeded. Try again later.".to_string());
    }

    let body: serde_json::Value = resp.json().await
        .map_err(|e| format!("Failed to parse Brave response: {e}"))?;

    // Extract results from body.web.results array
    let results = body["web"]["results"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .take(count)
        .filter_map(|r| {
            Some(SearchResult {
                title: r["title"].as_str()?.to_string(),
                url: r["url"].as_str()?.to_string(),
                snippet: r["description"].as_str().unwrap_or("").to_string(),
            })
        })
        .collect();

    Ok(results)
}
```

### Sleep State Machine Integration

```rust
// Source: Codebase pattern from agent_loop.rs pause mechanism (lines 331-345)

// New AgentState variant for sleep:
pub enum AgentState {
    Thinking,
    Executing,
    Idle,
    Paused,
    Sleeping { reason: String, until: Option<String> },  // NEW
}

// Sleep state stored alongside pause_flag in agent loop:
struct SleepState {
    active: bool,
    mode: SleepMode,
    started_at: std::time::Instant,
    max_duration: Duration,
    wake_reason: Option<String>,
}

enum SleepMode {
    Timer(Duration),
    Event { agent_id: String },
    Manual,
}

// In agent_loop.rs between-turn check (after pause check):
if let Some(ref sleep) = sleep_state && sleep.active {
    send_event(AgentEvent::StateChanged(AgentState::Sleeping { ... }));

    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Check shutdown
        if shutdown.load(Ordering::SeqCst) { break; }

        // Check max duration
        if sleep.started_at.elapsed() >= sleep.max_duration {
            wake_reason = "max_duration_exceeded";
            break;
        }

        match &sleep.mode {
            SleepMode::Timer(d) => {
                if sleep.started_at.elapsed() >= *d {
                    wake_reason = "timer_expired";
                    break;
                }
            }
            SleepMode::Event { agent_id } => {
                if let Some(info) = manager.get_status(agent_id) {
                    match info.status {
                        SubAgentStatus::Completed => { wake_reason = "agent_completed"; break; }
                        SubAgentStatus::Failed(msg) => { wake_reason = format!("agent_failed: {msg}"); break; }
                        SubAgentStatus::Killed => { wake_reason = "agent_killed"; break; }
                        SubAgentStatus::Running => {} // keep sleeping
                    }
                }
            }
            SleepMode::Manual => {} // only manual resume or shutdown wakes
        }

        // Check if user manually resumed from TUI
        if manual_resume_flag.load(Ordering::SeqCst) {
            wake_reason = "user_resumed";
            break;
        }
    }

    // Inject wake notification as system message
    chat_req = chat_req.append_message(ChatMessage::system(
        &format!("[Sleep ended. Reason: {wake_reason}. You slept for {:.1}s]",
            sleep.started_at.elapsed().as_secs_f64())
    ));
    sleep_state.active = false;
}
```

### Discovery Persistence (JSONL)

```rust
// Source: Codebase pattern from agent/logging.rs SessionLogger

use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Discovery {
    timestamp: String,
    title: String,
    description: String,
}

fn discovery_file_path(workspace: &Path) -> PathBuf {
    workspace.join(".ouro-discoveries.jsonl")
}

fn append_discovery(workspace: &Path, discovery: &Discovery) -> Result<(), String> {
    let path = discovery_file_path(workspace);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("Failed to open discoveries file: {e}"))?;

    let mut writer = std::io::BufWriter::new(file);
    serde_json::to_writer(&mut writer, discovery)
        .map_err(|e| format!("Failed to serialize discovery: {e}"))?;
    writer.write_all(b"\n").map_err(|e| format!("{e}"))?;
    writer.flush().map_err(|e| format!("{e}"))?;
    Ok(())
}

fn load_discoveries(workspace: &Path) -> Vec<Discovery> {
    let path = discovery_file_path(workspace);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(), // File doesn't exist yet
    };

    content.lines()
        .filter_map(|line| serde_json::from_str::<Discovery>(line).ok()) // Lenient: skip bad lines
        .collect()
}
```

## Discretion Recommendations

These are areas marked as "Claude's Discretion" in the CONTEXT.md. Here are research-informed recommendations:

### Rate Limiting Intervals

**Recommendation:**
- DuckDuckGo: 2.0 second minimum delay between requests (configurable in config)
- Brave Search free tier: 1.0 second minimum delay (matches their 1 req/sec free limit)
- Brave Search paid tier: 0.1 second minimum delay
- Implementation: `tokio::time::Instant` tracking last request time; if delta < interval, `tokio::time::sleep(remaining)` before sending

**Rationale:** DDG is lenient but a 2-second gap avoids CAPTCHAs reliably. Brave's free tier explicitly enforces 1 req/sec. Both intervals should be configurable in the config file.

### Discovery File Format

**Recommendation:** JSONL (one JSON object per line) stored at `{workspace}/.ouro-discoveries.jsonl`

**Rationale:**
- Consistent with the existing `SessionLogger` pattern (JSONL in `.ouro-logs/`)
- Append-only is crash-safe (partial writes only corrupt the last line)
- Each line is self-contained for lenient parsing
- Human-readable with `cat` or `jq`
- Easy to load: read lines, `serde_json::from_str` each, skip failures

**Schema per line:**
```json
{"timestamp":"2026-02-04T14:32:07.123Z","title":"Found Makefile","description":"Project root contains a Makefile with build, test, and deploy targets"}
```

### Markdown Conversion Library

**Recommendation:** `htmd` crate

**Rationale:**
- Lightweight: just html5ever + markup5ever_rcdom + phf
- Simple API: `htmd::convert(html_str)` returns `Result<String, ...>`
- Inspired by turndown.js (the de facto JS standard for HTML-to-markdown)
- Thread-safe for concurrent use
- Actively maintained
- `html-to-markdown-rs` (v2.24) is more feature-rich but overkill for web page extraction and has a much larger dependency tree

### Status Bar Sleep Formatting

**Recommendation:** Add a sleep segment to status bar line 1, displayed only when sleeping:

```
[Sleeping] | Timer: 2m 34s remaining | Context: 45% | Session 1 | Turn 12 | Tools: 37
[Sleeping] | Waiting: agent abc123 | Context: 45% | Session 1 | Turn 12 | Tools: 37
[Sleeping] | Manual pause (r to resume) | Context: 45% | Session 1 | Turn 12 | Tools: 37
```

- Yellow color for "Sleeping" state label (matches Thinking)
- Countdown updates every render tick (50ms) for smooth display
- When not sleeping, this segment is absent (status bar unchanged from Phase 4/5)

### Maximum Sleep Duration Default

**Recommendation:** 3600 seconds (1 hour)

**Rationale:**
- Long enough for meaningful background tasks (builds, tests, long-running processes)
- Short enough to prevent indefinite dormancy if something goes wrong
- User can always override in config or resume manually from TUI
- Config field: `max_sleep_duration_secs` (optional, defaults to 3600)

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| DuckDuckGo Instant Answer API | DDG Lite HTML scraping | DDG deprecated free API | Must scrape HTML; lite endpoint is the standard approach |
| Browser automation for JS pages | Simple HTTP GET | N/A (decision: no JS rendering) | Much simpler, but some modern SPAs will return empty content |
| Separate discovery database | Workspace JSONL file | Phase 6 design | Survives context resets; no external database dependency |

**Deprecated/outdated:**
- DuckDuckGo Instant Answer API: Was limited to instant answers only (no web results), and is not reliable for programmatic access
- `html2md` crate: Older, less maintained than `htmd`; `htmd` is the current standard

## Config Schema Additions

The following fields need to be added to the config hierarchy:

```toml
[search]
# DuckDuckGo rate limit (seconds between requests)
ddg_rate_limit_secs = 2.0
# Brave Search API key (optional; enables Brave as search provider)
brave_api_key = ""
# Brave rate limit (seconds between requests)
brave_rate_limit_secs = 1.0

[sleep]
# Maximum sleep duration in seconds (prevents indefinite dormancy)
max_sleep_duration_secs = 3600
```

These follow the existing pattern: `ConfigFile` -> `PartialConfig` -> `AppConfig` merge chain.

## Open Questions

1. **DuckDuckGo Lite HTML structure stability**
   - What we know: The DDG lite endpoint (`lite.duckduckgo.com/lite/`) uses a simple table-based HTML layout
   - What's unclear: How stable is this HTML structure? DDG could change it without notice
   - Recommendation: Build the scraper with CSS selectors that are resilient to minor changes; log a warning if expected selectors find no results (suggests page structure changed)

2. **reqwest Client reuse vs. per-request**
   - What we know: Creating a `reqwest::Client` per request wastes connection pool resources; reusing is better
   - What's unclear: How to thread a shared client through the tool dispatch without changing the dispatch signature significantly
   - Recommendation: Create a `reqwest::Client` at the `dispatch_web_fetch` / `dispatch_web_search` call site (one per tool call is acceptable; the connection pool matters more for rapid sequential calls). Alternatively, store a `reqwest::Client` in a new tool context struct passed through dispatch.

3. **Sleep tool interaction with context management**
   - What we know: Sleep blocks between turns; no LLM calls happen during sleep; turn count should not increment
   - What's unclear: Should the context manager's char/token tracking account for the wake-up system message?
   - Recommendation: Yes -- the wake system message is like any other system message and contributes to context usage. This is already the pattern for restart markers and wind-down messages.

## Sources

### Primary (HIGH confidence)
- Codebase: `src/agent/tools.rs` -- Established tool dispatch pattern (9 tools, define_tools + dispatch_tool_call)
- Codebase: `src/agent/agent_loop.rs` -- Pause mechanism pattern (lines 331-345), context management integration
- Codebase: `src/tui/app_state.rs` -- Discovery state already exists as `Vec<(String, String)>`
- Codebase: `src/tui/event.rs` -- `AgentEvent::Discovery` variant already defined
- Codebase: `src/tui/tabs/discoveries_tab.rs` -- Rendering already implemented with reverse-chronological order
- Codebase: `src/agent/logging.rs` -- JSONL persistence pattern (SessionLogger)
- Codebase: `src/config/schema.rs` -- Config merge chain pattern (ConfigFile -> PartialConfig -> AppConfig)
- Codebase: `Cargo.toml` -- reqwest 0.12, tokio 1.x, serde_json 1.0 already present
- [htmd crate docs](https://docs.rs/htmd/latest/htmd/) -- HTML-to-Markdown API
- [Brave Search API docs](https://api-dashboard.search.brave.com/app/documentation/web-search/get-started) -- REST API format

### Secondary (MEDIUM confidence)
- [htmd GitHub](https://github.com/letmutex/htmd) -- Turndown.js-inspired design, thread-safe, html5ever-based
- [Brave Search API pricing](https://brave.com/search/api/) -- Free tier: 1 req/sec, 2000/month
- [scraper crate](https://crates.io/crates/scraper) -- Standard Rust HTML parser with CSS selectors
- [websearch crate](https://lib.rs/crates/websearch) -- Multi-provider search (evaluated but not recommended due to v0.1.1 immaturity)
- [duckduckgo crate](https://lib.rs/crates/duckduckgo) -- DDG search library (evaluated but direct scraping preferred for control)

### Tertiary (LOW confidence)
- WebSearch: DuckDuckGo rate limiting behavior (community reports suggest 2-second gaps are safe; no official documentation)
- WebSearch: DDG lite HTML structure details (would need to fetch and inspect actual page during implementation)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- All libraries verified on crates.io/docs.rs; reqwest/tokio already in use
- Architecture: HIGH -- All patterns derived from existing codebase (tools.rs, agent_loop.rs, logging.rs, app_state.rs)
- Pitfalls: MEDIUM -- DDG rate limiting behavior is community knowledge; Brave API limits documented officially
- Discretion recommendations: MEDIUM -- Based on research + codebase patterns; reasonable defaults that user can override

**Research date:** 2026-02-04
**Valid until:** 2026-03-04 (30 days -- libraries are stable; DDG lite endpoint could change)
