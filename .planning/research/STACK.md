# Stack Research

**Domain:** Autonomous AI agent harness (local LLM, infinite-loop exploration, TUI monitoring)
**Researched:** 2026-02-03
**Confidence:** HIGH (core stack verified via official sources; supporting libraries verified via crates.io)

## Recommended Stack

### Core Technologies

| Technology | Version | Purpose | Why Recommended | Confidence |
|------------|---------|---------|-----------------|------------|
| **Rust** (Edition 2024) | 1.93.0+ (stable) | Language & toolchain | Memory safety without GC is non-negotiable for a long-running autonomous agent that spawns sub-processes and manages context windows. Edition 2024 (stable since 1.85.0) gives us `let_chains`, resolver v3, and modern ergonomics. No runtime crashes from null pointers or data races in the agent loop. | HIGH |
| **genai** | 0.5.3 | LLM driver (multi-provider, Ollama-native) | User contributes to this crate. Single ergonomic API across Ollama, OpenAI, Anthropic, Gemini, and 10+ other providers. Tool/function calling landed in v0.1.11, solidified in v0.4.0, with an Ollama tool fix in v0.5.2. Streaming via `exec_chat_stream()`. Uses reqwest 0.13 internally. The "lowest common denominator" philosophy matches our need: we want chat + tool calls, not provider-specific bells and whistles. | HIGH |
| **tokio** | 1.49 | Async runtime | The Rust async runtime. Work-stealing scheduler, async process spawning (`tokio::process`), timers, channels, `select!` for the agent loop. Every other crate in our stack (genai, reqwest, ratatui event streams) assumes tokio. There is no viable alternative for this project. | HIGH |
| **ratatui** | 0.30 | Terminal UI framework | The standard Rust TUI library (successor to tui-rs). v0.30 modularized into a workspace, added `no_std` support, and remains the community default. Sub-millisecond immediate-mode rendering, constraint-based layouts, rich widget library (tables, sparklines, gauges, scrollable lists). Uses crossterm backend by default. | HIGH |
| **crossterm** | 0.29 | Terminal backend | Cross-platform terminal manipulation (Linux, macOS, Windows). Default backend for ratatui. Handles raw mode, alternate screen, mouse capture, and event reading. The `event-stream` feature provides async `EventStream` for tokio integration. | HIGH |

### Supporting Libraries

| Library | Version | Purpose | When to Use | Confidence |
|---------|---------|---------|-------------|------------|
| **serde** | 1.0 | Serialization framework | Always. Every config, message, tool schema, and persistence format goes through serde. Use `features = ["derive"]` for `#[derive(Serialize, Deserialize)]`. | HIGH |
| **serde_json** | 1.0 | JSON serialization | Always. LLM messages, tool call schemas, agent state snapshots, and Ollama API payloads are all JSON. genai uses serde_json internally for tool definitions (tools take `serde_json::Value` schemas). | HIGH |
| **reqwest** | 0.13 | HTTP client | Web tool implementation. Fetch URLs, scrape pages, call APIs. genai already depends on reqwest 0.13 internally, so this is a zero-cost addition. Use `features = ["json"]` for ergonomic `.json()` deserialization. | HIGH |
| **scraper** | 0.25 | HTML parsing & CSS selectors | Web tool: extract text content from fetched HTML pages. Built on Servo's html5ever engine (browser-grade parsing). The agent needs to read web pages, not just fetch raw HTML. | MEDIUM |
| **tokio** (process feature) | 1.49 | Async child process management | Shell tool: `tokio::process::Command` for async spawn, stdout/stderr capture, timeout, and kill-on-drop. Already part of tokio with `features = ["process"]`. Use `kill_on_drop(true)` to prevent zombie processes from runaway agent commands. | HIGH |
| **tracing** | 0.1.41 | Structured logging / diagnostics | Always. Structured spans and events for debugging the agent loop, tool execution, sub-agent lifecycle. Integrates with tokio. Use `tracing-subscriber` for formatting output. The TUI can subscribe to tracing events to display live logs. | HIGH |
| **tracing-subscriber** | 0.3 | Log formatting & filtering | Always alongside tracing. Provides `FmtSubscriber`, layer composition, and `EnvFilter` for runtime log level control. | HIGH |
| **anyhow** | 1.1 | Application error handling | Top-level error propagation. `anyhow::Result<T>` as the return type for agent operations, tool executions, and the main loop. Provides `.context("doing X")` for rich error chains. | HIGH |
| **thiserror** | 1.6 | Typed error definitions | Library-layer error types. Define `AgentError`, `ToolError`, `LlmError` enums with `#[derive(thiserror::Error)]`. Use at module boundaries where callers need to match on error variants. | HIGH |
| **clap** | 4.5 | CLI argument parsing | Binary entry point. Parse workspace path, model name, config file, verbosity, and sub-commands (e.g., `ouro run`, `ouro resume`). Use `features = ["derive"]` for `#[derive(Parser)]`. | HIGH |
| **toml** | 0.8 | TOML config parsing | Configuration file loading. TOML is the Rust ecosystem standard for config (Cargo itself uses it). Parse `ouro.toml` for agent settings, model preferences, tool permissions. v0.9 is in development but 0.8 is the current stable release. | HIGH |
| **uuid** | 1.18 | Unique identifiers | Agent IDs, session IDs, message IDs. Use `features = ["v4"]` for random UUIDs or `features = ["v7"]` for time-sortable UUIDs (better for logs and persistence). | HIGH |
| **chrono** | 0.4.42 | Date/time handling | Timestamps on agent messages, session logs, and persistence files. Use `features = ["serde"]` for serialization. | HIGH |
| **directories** | 5 | Platform config/data paths | Locate `~/.config/ouro/` and `~/.local/share/ouro/` (XDG-compliant) for config and data directories. Mid-level API with `ProjectDirs` for app-specific paths. | MEDIUM |
| **notify** | 8.2 | Filesystem watching | Watch the agent's workspace directory for changes. Useful for the TUI to detect when the agent creates or modifies files. Cross-platform (inotify/kqueue/ReadDirectoryChanges). | LOW |

### Development Tools

| Tool | Purpose | Notes |
|------|---------|-------|
| **cargo** | Build system & package manager | Use workspace layout: root `Cargo.toml` with members for `ouro-core`, `ouro-tui`, `ouro-tools`, `ouro-cli`. |
| **cargo-watch** | Auto-rebuild on save | `cargo install cargo-watch && cargo watch -x 'run -- --model llama3.2'` |
| **cargo-nextest** | Faster test runner | Drop-in replacement for `cargo test` with better output, parallelism, and retries. |
| **clippy** | Lint | Run with `cargo clippy -- -W clippy::all`. Essential for catching Rust anti-patterns. |
| **rustfmt** | Code formatting | Standard Rust formatting. Add `style_edition = "2024"` to `rustfmt.toml` if migrating. |

## Installation

```bash
# Create project
cargo init ouro --edition 2024
cd ouro

# Or add to existing Cargo.toml:
```

```toml
[package]
name = "ouro"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"

[dependencies]
# LLM Driver
genai = "0.5"

# Async Runtime
tokio = { version = "1.49", features = ["full"] }

# TUI
ratatui = "0.30"
crossterm = { version = "0.29", features = ["event-stream"] }

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
toml = "0.8"

# HTTP & Web
reqwest = { version = "0.13", features = ["json"] }
scraper = "0.25"

# Error Handling
anyhow = "1.1"
thiserror = "1.6"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# CLI
clap = { version = "4.5", features = ["derive"] }

# Utilities
uuid = { version = "1.18", features = ["v7", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
directories = "5"
```

## Alternatives Considered

| Recommended | Alternative | When to Use Alternative |
|-------------|-------------|-------------------------|
| **genai** | `ollama-rs` | If you need Ollama-specific features beyond what genai normalizes (e.g., direct model management, pull/push). ollama-rs has a richer Ollama-specific API and its own `#[function]` macro for tool definitions. But genai is the project constraint and multi-provider support is valuable for testing against cloud models. |
| **genai** | `rig-core` | If you want a batteries-included agent framework with built-in RAG, tool orchestration, and provider routing. Rig is more opinionated and heavier. Ouro needs to own its agent loop, not delegate it to a framework. |
| **genai** | `async-openai` | If you only target OpenAI-compatible APIs. More complete API coverage for OpenAI specifically, but locks you to one provider's schema. genai's author recommends it alongside ollama-rs for "complete client API" needs. |
| **ratatui** | `tui-realm` | If you want a React/Elm-inspired stateful component model on top of ratatui. Adds a layer of abstraction that may be useful for complex UIs but increases learning curve and dependency weight. Start with raw ratatui; consider tui-realm if the TUI grows complex. |
| **tokio** (process) | `duct` | If you want a simpler, synchronous process pipeline API. duct is excellent for shell pipelines but doesn't integrate with async. Since the agent loop is async, tokio::process is the right fit. |
| **anyhow** | `eyre` / `color-eyre` | If you want colorized, Span-aware error reports. eyre is a fork of anyhow with better formatting. Reasonable choice, but anyhow is more established and the TUI will handle error display formatting anyway. |
| **chrono** | `time` | If you want a lighter datetime library. The `time` crate is smaller and avoids some of chrono's historical soundness issues (now fixed). chrono remains more widely used and has richer timezone support. Either works; chrono's ecosystem integration (serde, diesel, etc.) tips the scale. |

## What NOT to Use

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| **tui-rs** | Abandoned since 2023. ratatui is the community fork and active successor. All development, examples, and community support have moved to ratatui. | `ratatui` |
| **log** crate (as primary) | `tracing` is the successor to `log` for async Rust. It provides structured spans (not just events), integrates with tokio, and supports the same log levels. The `tracing-log` compatibility layer lets libraries that use `log` still work. | `tracing` + `tracing-subscriber` |
| **rustformers/llm** | Archived and abandoned. Was a local inference runtime, not an API client. Does not work with Ollama's HTTP API. | `genai` for API access, Ollama for local inference |
| **termion** | Linux/macOS only. crossterm is cross-platform and the ratatui default backend. No reason to use termion unless you explicitly cannot use crossterm. | `crossterm` |
| **reqwest** `blocking` | Blocks the tokio runtime thread. In an async agent loop, use the async reqwest client. The blocking API exists for simple scripts, not async applications. | `reqwest` (async, default) |
| **serde_yaml** | Unmaintained since 2023 (the original `serde_yaml` by dtolnay is archived). If you need YAML, use `serde_yml` (community fork). But prefer TOML for config and JSON for data exchange -- no reason to introduce YAML. | `toml` for config, `serde_json` for data |
| **AutoAgents** / **Anda** / **AutoGPT-rs** | These are full agent frameworks with their own opinions on agent loops, memory, and orchestration. Ouro's core value is that the agent owns its own persistence and exploration strategy. Using a framework would defeat the purpose. Build the agent loop from primitives. | Custom agent loop with `genai` + `tokio` |

## Stack Patterns by Variant

**If the agent needs to execute untrusted shell commands:**
- Wrap `tokio::process::Command` with timeout (`tokio::time::timeout`), output size limits, and `kill_on_drop(true)`
- Consider running in a subprocess group (`pre_exec` with `setpgid`) so the entire process tree can be killed
- Never pass unsanitized LLM output directly to a shell -- use `.arg()` not `.args(&["-c", llm_output])`

**If the TUI needs to display real-time agent output:**
- Use `tokio::sync::mpsc` channels to send events from the agent loop to the TUI render loop
- ratatui uses immediate-mode rendering: the TUI redraws every frame based on current state
- Keep the agent state in a shared `Arc<RwLock<AgentState>>` or use message passing

**If sub-agents need their own context windows:**
- Each sub-agent gets its own `ChatRequest` with its own message history
- Use `tokio::spawn` for concurrent sub-agents with their own genai `Client` instances
- Parent tracks sub-agent handles via `tokio::task::JoinHandle` for lifecycle management

**If the agent needs to bootstrap its own persistence:**
- The SYSTEM_PROMPT.md pattern means the agent reads its own prompt from disk on startup
- Agent state serializes to JSON via serde in the workspace directory
- On context window restart, the agent reads its previous state from disk
- Use `chrono` timestamps + `uuid` session IDs to version state files

## Version Compatibility

| Package | Compatible With | Notes |
|---------|-----------------|-------|
| genai 0.5.x | reqwest 0.13, tokio 1.x | genai internally uses reqwest 0.13. Your own reqwest dependency should match to avoid duplicate versions in the dependency tree. |
| ratatui 0.30.x | crossterm 0.29 (default), crossterm 0.28 (via feature flag) | ratatui 0.30 modularized crossterm support. Default is `crossterm_0_29`. |
| tokio 1.49.x | Rust 1.71+ (MSRV) | LTS versions: 1.43.x (until March 2026), 1.47.x (until September 2026). Use latest stable for new projects. |
| tracing 0.1.41 | tokio 1.x, tracing-subscriber 0.3.x | tracing 0.2 is in development but not released. Stick with 0.1.x. |
| serde 1.0.228 | All crates in ecosystem | serde 1.x is semver-stable. Pin to `"1.0"` not a specific patch. |
| clap 4.5.x | Rust 1.74+ (MSRV) | clap 4.x is the current major. No clap 5 on the horizon. |
| Rust Edition 2024 | Rust 1.85.0+ | Edition 2024 implies resolver v3. Use `rust-version = "1.85"` in Cargo.toml for MSRV enforcement. |

## Critical Gap: genai Tool Calling + Ollama

**Status (as of v0.5.3):** Tool/function calling is implemented in genai for OpenAI, Anthropic, and Gemini. Ollama support exists and received a bug fix in v0.5.2. However, this is a relatively new feature path.

**Implications for Ouro:**
1. Tool schemas are defined as `serde_json::Value` objects (JSON Schema), not strongly typed. Use the `schemars` crate if you want to derive schemas from Rust types.
2. Ollama's tool calling depends on the model supporting it. Recommended models: **Llama 3.1 8B+** (best overall tool calling), **Qwen2.5** (strong tool use), **Mistral 7B-Instruct** (lightweight option).
3. Ollama does NOT support streaming tool calls or `tool_choice` parameter (as of late 2025). Tool calls come back in the non-streaming response. Plan the agent loop accordingly.
4. Since the user contributes to genai, gaps in Ollama tool support can be fixed upstream. This is an advantage, not a risk.

**Recommendation:** Start with genai's tool calling API. If Ollama-specific edge cases appear, contribute fixes upstream to genai. Fall back to prompt-engineering JSON output as a temporary workaround for models that don't support native tool calling.

## Sources

- [genai crate - GitHub](https://github.com/jeremychone/rust-genai) -- Verified v0.5.3 features, tool calling status, Ollama support. CHANGELOG.md confirms tool calling timeline. HIGH confidence.
- [genai Issue #24: Function calling / tool use](https://github.com/jeremychone/rust-genai/issues/24) -- Confirmed tool calling implemented and merged. HIGH confidence.
- [genai crate - crates.io](https://crates.io/crates/genai) -- Version verification. HIGH confidence.
- [ratatui - GitHub](https://github.com/ratatui/ratatui) -- Verified v0.30 release, modular workspace, crossterm backend. HIGH confidence.
- [ratatui.rs](https://ratatui.rs/) -- Official documentation. HIGH confidence.
- [tokio - GitHub](https://github.com/tokio-rs/tokio) -- Verified v1.49, LTS policy, process module. HIGH confidence.
- [tokio::process docs](https://docs.rs/tokio/latest/tokio/process/index.html) -- Command API, kill_on_drop, process groups. HIGH confidence.
- [crossterm - crates.io](https://crates.io/crates/crossterm) -- Verified v0.29.0. HIGH confidence.
- [reqwest - GitHub](https://github.com/seanmonstar/reqwest) -- Verified v0.13, rustls default, TLS changes. HIGH confidence.
- [scraper - crates.io](https://crates.io/crates/scraper) -- Verified v0.25. MEDIUM confidence (version from search, not official docs).
- [serde - crates.io](https://crates.io/crates/serde) -- Verified v1.0.228. HIGH confidence.
- [tracing - crates.io](https://crates.io/crates/tracing) -- Verified v0.1.41. HIGH confidence.
- [clap - crates.io](https://crates.io/crates/clap) -- Verified v4.5.54. HIGH confidence.
- [Rust 1.85.0 / Edition 2024 announcement](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/) -- Edition 2024 stable since Feb 2025. HIGH confidence.
- [Rust releases](https://releases.rs/) -- Current stable Rust 1.93.0. HIGH confidence.
- [Ollama tool calling docs](https://docs.ollama.com/capabilities/tool-calling) -- Ollama native tool calling API, supported models. MEDIUM confidence.
- [ollama-rs - GitHub](https://github.com/pepperoni21/ollama-rs) -- Function calling system, Coordinator pattern. MEDIUM confidence (evaluated as alternative).
- [notify crate - crates.io](https://crates.io/crates/notify) -- Verified v8.2.0. HIGH confidence.

---
*Stack research for: Autonomous AI agent harness (Rust/Ollama/ratatui)*
*Researched: 2026-02-03*
