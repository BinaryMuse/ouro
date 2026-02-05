---
phase: 06-extended-tools-discovery
plan: 01
subsystem: agent-tools
tags: [web-fetch, web-search, config, dependencies]
dependency_graph:
  requires: [phase-05]
  provides: [web-fetch-module, web-search-module, search-sleep-config]
  affects: [06-02, 06-03, 06-04]
tech_stack:
  added: [htmd-0.1, scraper-0.22]
  patterns: [rate-limiting-mutex-instant, html-scraping-css-selectors]
key_files:
  created:
    - src/agent/web_fetch.rs
    - src/agent/web_search.rs
  modified:
    - Cargo.toml
    - Cargo.lock
    - src/config/schema.rs
    - src/config/merge.rs
    - src/agent/mod.rs
    - src/agent/tools.rs
    - tests/integration_tests.rs
decisions:
  - id: 06-01-rate-limit-pattern
    description: "Mutex<Option<Instant>> static for rate limiting -- simple, lock-free during sleep (only held briefly to read/write timestamp)"
  - id: 06-01-ddg-lite-scraping
    description: "DDG lite endpoint with scraper CSS selectors (a.result-link, td.result-snippet) for search result extraction"
  - id: 06-01-brave-api-key-double-option
    description: "brave_api_key uses Option<Option<String>> in PartialConfig for merge layering (None=unset, Some(None)=explicitly no key, Some(Some(k))=key set)"
metrics:
  duration: 5 min
  completed: 2026-02-05
---

# Phase 6 Plan 1: Dependencies and Foundation Modules Summary

**One-liner:** htmd/scraper deps added, config schema extended with search/sleep fields, web_fetch (reqwest+htmd) and web_search (DDG scraping + Brave API + rate limiting) modules created as standalone async functions.

## Tasks Completed

| # | Task | Commit | Key Files |
|---|------|--------|-----------|
| 1 | Add dependencies and extend config schema | c764fb7 | Cargo.toml, schema.rs, merge.rs, tools.rs tests, integration_tests.rs |
| 2 | Create web_fetch and web_search modules | 628fee1 | web_fetch.rs, web_search.rs, mod.rs |

## What Was Built

### Config Schema Extensions

- `SearchConfig` struct: `ddg_rate_limit_secs`, `brave_api_key`, `brave_rate_limit_secs`
- `SleepConfig` struct: `max_sleep_duration_secs`
- Full merge chain: `ConfigFile` -> `PartialConfig` -> `AppConfig`
- Defaults: DDG 2.0s rate limit, Brave 1.0s rate limit, no API key, 3600s max sleep

### web_fetch Module

- `fetch_url(url, format, max_length)` -- async HTTP GET with:
  - 30s timeout, 10-redirect limit, Ouro user-agent
  - Content-type detection: JSON returned as-is, HTML converted to markdown via htmd (when format="markdown"), otherwise raw
  - Optional truncation with summary suffix
  - All errors returned as JSON strings (never Err)

### web_search Module

- `search_duckduckgo(query, count)` -- scrapes DDG lite HTML with CSS selectors (`a.result-link`, `td.result-snippet`)
- `search_brave(query, count, api_key)` -- REST API with auth header, 401/429 error handling
- `SearchResult` struct: title, url, snippet (Serialize)
- Rate-limited wrappers: `rate_limited_ddg_search()`, `rate_limited_brave_search()` using `Mutex<Option<Instant>>` static tracking
- `enforce_rate_limit()` reads timestamp under brief lock, sleeps outside lock, updates timestamp

## Decisions Made

1. **Rate limiting via Mutex<Option<Instant>>** -- Static globals for DDG and Brave last-request tracking. Lock held only briefly to read/update timestamp; sleep happens outside the lock. Simple and correct for the expected low-concurrency use case.

2. **DDG lite scraping with CSS selectors** -- `a.result-link` for links/titles, `td.result-snippet` for snippets. Resilient to minor HTML changes; returns empty results if selectors fail (graceful degradation).

3. **brave_api_key as Option<Option<String>> in PartialConfig** -- Follows the same double-Option pattern as `max_restarts` for correct merge semantics (distinguish "not set" from "explicitly set to None").

## Deviations from Plan

### Pre-existing Work

The Task 1 and Task 2 changes were found already committed by a prior agent session (commits `c764fb7` and `628fee1`). These commits contained the exact changes specified in the plan. Rather than re-creating identical commits, the existing commits were verified and adopted.

The prior session also committed `discovery.rs` and `sleep.rs` modules (from Plan 06-02) alongside the Plan 06-01 work. These extra modules are not part of this plan but do not conflict.

## Verification

- `cargo check`: Compiles cleanly (21 dead_code warnings for not-yet-wired code, expected)
- `cargo test`: All suites pass (192 lib + 36 + 9 + 8 + 14 = 259 tests, 1 doc-test ignored)
- web_fetch.rs: `pub async fn fetch_url` present
- web_search.rs: `pub async fn search_duckduckgo`, `pub async fn search_brave`, rate-limited wrappers present
- AppConfig: All 4 new fields present with merge and defaults

## Next Phase Readiness

Plan 06-01 deliverables are ready for Plans 02-04:
- **Plan 02** can use discovery.rs (already committed)
- **Plan 03** can wire web_fetch and web_search into tool dispatch
- **Plan 04** can integrate sleep into agent loop

No blockers or concerns.
