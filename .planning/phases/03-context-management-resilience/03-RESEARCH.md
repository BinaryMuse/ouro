# Phase 03: Context Management & Resilience - Research

**Researched:** 2026-02-04
**Domain:** LLM context window management, observation masking, token tracking, session restart/continuity
**Confidence:** HIGH

## Summary

This phase adds context-aware lifecycle management to the agent loop: tracking real token usage from Ollama response metadata, applying graduated observation masking when context pressure rises, and restarting the session gracefully when context is exhausted while preserving continuity across restarts.

The critical technical discovery is the **token usage chain**: Ollama's OpenAI-compatible `/v1/chat/completions` endpoint now supports `stream_options.include_usage` (issue #4448 resolved, confirmed working by June 2025). The genai crate's `ChatOptions::with_capture_usage(true)` triggers this -- it sends `stream_options: {"include_usage": true}` in the request body. The `StreamEnd` event then carries `captured_usage: Option<Usage>` with `prompt_tokens` and `completion_tokens`. This replaces the Phase 2 character-count heuristic (`total_chars / 4`) with actual token counts.

For observation masking, recent research (JetBrains 2025, Anthropic context engineering guide) confirms that **simple observation masking -- replacing old tool outputs with compact placeholders while preserving action/reasoning history -- matches or outperforms LLM-based summarization** for coding agents. The user's decision to include summaries in placeholders (e.g. `[file_read: src/main.rs -- 142 lines, Rust source]`) is well-aligned with this research. The conversation history lives in `ChatRequest.messages: Vec<ChatMessage>` which is a public field, making in-place modification straightforward.

**Primary recommendation:** Enable `capture_usage` on the genai `ChatOptions`, extract `prompt_tokens + completion_tokens` from each `StreamEnd`, track cumulative token usage, and implement a `ContextManager` that owns the graduated threshold logic (soft threshold triggers oldest-first observation masking, hard threshold triggers wind-down and session restart). The session restart mechanism should save the last N turns, end the current `run_agent_loop` call, and re-invoke it with seed context.

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| genai (already in Cargo.toml) | git main | `ChatOptions::with_capture_usage(true)` enables token tracking via `StreamEnd.captured_usage` | Already the project's LLM client; usage capture is built-in |
| serde_json (already) | 1.0 | Serialize token usage to JSONL log, config deserialization | Already a dependency |
| tokio (already) | 1.x | Async runtime for session restart orchestration | Already a dependency |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| No new dependencies needed | - | All functionality implementable with existing crate features | - |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| genai `capture_usage` | Direct Ollama `/api/chat` native endpoint | Would get `prompt_eval_count`/`eval_count` directly, but requires abandoning genai's streaming abstraction. Not worth it since genai supports the same data via OpenAI compat layer. |
| In-place message mutation for masking | Clone-and-rebuild message Vec | Cloning wastes memory. Direct mutation of `ChatRequest.messages` (public Vec) is simpler and cheaper. |
| Character heuristic as fallback | No fallback | Keep the char/4 heuristic as LOW-priority fallback if `captured_usage` returns None (e.g., provider doesn't support it). Defensive coding. |

**Installation:** No new dependencies required. All functionality uses existing crates.

## Architecture Patterns

### Recommended Module Structure
```
src/
  agent/
    mod.rs                  # Add context_manager module
    agent_loop.rs           # Refactor: integrate ContextManager, session restart
    context_manager.rs      # NEW: token tracking, masking, threshold logic
    logging.rs              # Extend: add token_usage log entry type
    system_prompt.rs        # Extend: add session number, restart marker
    tools.rs                # Unchanged
  config/
    schema.rs               # Extend: add context management config fields
    merge.rs                # Extend: add defaults for new config fields
  cli.rs                    # Unchanged (or minimal: Resume command wiring)
```

### Pattern 1: ContextManager Struct
**What:** A dedicated struct that encapsulates all context pressure logic -- token tracking, threshold checks, masking decisions, and wind-down signaling.
**When to use:** Called after every turn in the agent loop to evaluate context state and take action.

```rust
// Source: Project-specific design based on codebase analysis
pub struct ContextManager {
    // Configuration
    context_limit: usize,           // Model's context window size in tokens
    soft_threshold_pct: f64,        // e.g., 0.70 -- start masking
    hard_threshold_pct: f64,        // e.g., 0.90 -- trigger wind-down
    carryover_turns: usize,         // Turns to carry into next session

    // Runtime state
    prompt_tokens: usize,           // From last Ollama response
    completion_tokens_total: usize, // Cumulative completion tokens
    total_tokens_used: usize,       // Latest prompt_tokens (most accurate)
    masked_count: usize,            // How many observations have been masked
    session_number: u32,            // Current session number (1-based)
    turn_count: u64,                // Turns in current session
}

pub enum ContextAction {
    Continue,                       // Under soft threshold, no action needed
    Mask { reclaimed_pct: f64 },    // Soft threshold hit, masking applied
    WindDown,                       // Hard threshold hit, tell agent to wrap up
    Restart,                        // Context exhausted, restart session
}
```

### Pattern 2: Observation Masking via Message Mutation
**What:** Walk `ChatRequest.messages` from oldest to newest, replacing tool result content with summary placeholders.
**When to use:** When `total_tokens_used` exceeds the soft threshold.

```rust
// Source: Based on genai ChatRequest.messages: Vec<ChatMessage> (public field)
// and JetBrains observation masking research

fn mask_oldest_observations(
    messages: &mut Vec<ChatMessage>,
    count: usize,  // How many to mask this round
) -> MaskResult {
    let mut masked = 0;
    for msg in messages.iter_mut() {
        if masked >= count { break; }
        if msg.role == ChatRole::Tool {
            // Check if already masked (contains placeholder marker)
            if is_already_masked(msg) { continue; }
            // Generate summary placeholder
            let summary = generate_observation_summary(msg);
            // Replace content with placeholder
            msg.content = MessageContent::from(summary);
            masked += 1;
        }
    }
    MaskResult { masked_count: masked, /* ... */ }
}
```

### Pattern 3: Session Restart Loop (Outer Loop)
**What:** Wrap the existing `run_agent_loop` in an outer loop that handles session restarts.
**When to use:** When the agent loop returns with a "restart needed" signal.

```rust
// Source: Project-specific design
pub async fn run_agent_with_restarts(
    config: &AppConfig,
    safety: &SafetyLayer,
) -> anyhow::Result<()> {
    let mut session_number: u32 = 1;
    let mut carryover_messages: Vec<ChatMessage> = Vec::new();

    loop {
        let result = run_agent_session(
            config, safety, session_number, &carryover_messages,
        ).await?;

        match result.shutdown_reason {
            ShutdownReason::ContextFull { last_n_messages } => {
                if config.max_restarts.map_or(false, |max| session_number >= max) {
                    break; // Max restarts reached
                }
                if config.restart_requires_confirmation {
                    // Pause and wait for user confirmation
                    wait_for_confirmation().await?;
                }
                carryover_messages = last_n_messages;
                session_number += 1;
            }
            ShutdownReason::UserShutdown | ShutdownReason::Error => break,
        }
    }
    Ok(())
}
```

### Pattern 4: Graduated Threshold Response
**What:** Two-tier response to context pressure. Soft threshold triggers incremental masking. Hard threshold triggers graceful wind-down then restart.
**When to use:** Evaluated after every model response using actual token counts.

```
Token Usage Zones:
  [0%--------70%]  NORMAL    - No action
  [70%------90%]   SOFT      - Mask oldest observations incrementally
  [90%------100%]  HARD      - Inject wind-down message, then restart
```

### Anti-Patterns to Avoid
- **Masking reasoning/action history:** Only mask tool outputs (observations). Preserving the agent's reasoning chain is critical for coherent behavior. Research confirms masking only observations preserves agent performance.
- **Aggressive bulk masking:** Don't mask all observations at once. Incremental oldest-first masking is gentler and allows the agent to keep using recently retrieved information.
- **Token counting via character heuristic only:** The `total_chars / 4` heuristic is inaccurate (varies by tokenizer, language, code vs prose). Use actual token counts from Ollama.
- **Restarting without wind-down:** The agent needs a chance to write state to disk before the session ends. Always inject a wind-down message before restart.
- **Harness-managed state files:** The user decided the agent writes its own progress files. The harness should NOT maintain a structured state file -- only inject restart markers and carry over recent turns.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Token counting | Custom tokenizer or character-based estimation | genai `capture_usage` -> Ollama `prompt_eval_count`/`eval_count` | Ollama already tokenizes; its counts are exact. No need for a Rust tokenizer crate. |
| Message serialization | Custom message format for carryover | genai `ChatMessage` with serde | ChatMessage already has the right structure for conversation history |
| Observation summarization | LLM-based summarization of tool outputs | Simple heuristic summaries based on tool name + content length + first line | Research shows simple masking matches LLM summarization. Heuristic summaries are cheaper, faster, and avoid an extra LLM call. |
| Session log format | Custom binary format for token logs | Extend existing JSONL `LogEntry` enum with token usage events | Already have JSONL logging infrastructure from Phase 2 |

**Key insight:** The biggest temptation will be to build LLM-powered summarization for observation masking. Research definitively shows this is unnecessary -- simple observation replacement with descriptive placeholders performs equally well for code-centric agents, is cheaper, and avoids recursive context consumption.

## Common Pitfalls

### Pitfall 1: Token Count Unavailability
**What goes wrong:** `StreamEnd.captured_usage` returns `None` because `capture_usage` was not enabled, or the Ollama version is too old.
**Why it happens:** The genai `ChatOptions` default is `capture_usage: false`. Must explicitly enable it. Also, Ollama versions before mid-2025 did not support `stream_options.include_usage`.
**How to avoid:** Always set `ChatOptions::default().with_capture_usage(true)`. Keep the character heuristic as a fallback when `captured_usage` is `None`. Log a warning when falling back.
**Warning signs:** `prompt_tokens` or `completion_tokens` is `None` in the `Usage` struct.

### Pitfall 2: prompt_tokens Already Includes Full Context
**What goes wrong:** Double-counting tokens. `prompt_tokens` from Ollama already reflects the ENTIRE prompt sent (including all conversation history). It is NOT incremental -- it is the total input token count for that request.
**Why it happens:** Misunderstanding what `prompt_eval_count` measures. Each Ollama request re-evaluates the full prompt.
**How to avoid:** Use `prompt_tokens` from the LATEST response as the current context size. Don't sum it across turns -- each new response's `prompt_tokens` already includes all prior messages.
**Warning signs:** Token count growing much faster than expected, exceeding `context_limit` prematurely.

### Pitfall 3: Masking Messages That genai Needs Structurally
**What goes wrong:** Replacing a `Tool` role message's content breaks the assistant->tool call->tool response chain that LLMs expect.
**Why it happens:** LLMs expect tool responses to follow tool calls. If you remove the tool response entirely, the model gets confused.
**How to avoid:** Never remove messages from the Vec -- only replace their content with a shorter placeholder string. The message structure (role, ordering) must remain intact. The placeholder IS the tool response, just compressed.
**Warning signs:** Model starts producing confused or repetitive tool calls after masking.

### Pitfall 4: Restart Carryover Losing Tool Call Structure
**What goes wrong:** Carrying over the "last N turns" splits a tool call/response pair, giving the model an incomplete interaction.
**Why it happens:** A "turn" from the user's perspective might be: user msg -> assistant tool calls -> tool response 1 -> tool response 2 -> assistant text. Naively slicing the last N messages from the Vec can cut mid-sequence.
**How to avoid:** Define carryover boundaries at complete interaction cycles. Scan backward from the end of messages to find clean break points (after an assistant text response, before the next user/system message).
**Warning signs:** Model in restarted session immediately tries to re-call tools it already called.

### Pitfall 5: System Prompt Not Reloaded on Restart
**What goes wrong:** The agent modifies SYSTEM_PROMPT.md during a session (per Ouroboros philosophy), but the restart uses the original cached system prompt.
**Why it happens:** `build_system_prompt()` is called once at session start and the result is used for the entire session.
**How to avoid:** Re-read SYSTEM_PROMPT.md from disk at the start of each new session. The agent may have modified it to improve its own bootstrap.
**Warning signs:** Agent behaves identically after restart despite having written improvements to SYSTEM_PROMPT.md.

### Pitfall 6: Wind-Down Message Timing
**What goes wrong:** The wind-down message is injected but the agent ignores it or doesn't have enough remaining context to act on it.
**Why it happens:** If hard threshold is 90% and the agent's next response pushes past 100%, there was no room to wrap up.
**How to avoid:** Set the hard threshold with enough headroom. 90% gives 10% of context for the wind-down exchange. For a 32K context, that is 3,200 tokens -- enough for a final response. Consider injecting wind-down at 85% if the model tends to produce long responses.
**Warning signs:** Session ends abruptly without the agent saving state.

## Code Examples

### Enabling Token Usage Capture in ChatOptions
```rust
// Source: genai ChatOptions (verified via GitHub source)
let chat_options = ChatOptions::default()
    .with_capture_content(true)
    .with_capture_tool_calls(true)
    .with_capture_usage(true);  // NEW: enables stream_options.include_usage
```

### Extracting Token Counts from StreamEnd
```rust
// Source: genai StreamEnd.captured_usage: Option<Usage>
// Usage { prompt_tokens: Option<i32>, completion_tokens: Option<i32>, total_tokens: Option<i32>, ... }
Ok(ChatStreamEvent::End(end)) => {
    // Extract token usage from this turn
    if let Some(usage) = &end.captured_usage {
        let prompt_toks = usage.prompt_tokens.unwrap_or(0) as usize;
        let completion_toks = usage.completion_tokens.unwrap_or(0) as usize;

        // prompt_tokens IS the current context size (not incremental)
        // It includes system prompt + all conversation history + this prompt
        context_manager.update_token_usage(prompt_toks, completion_toks, turn);

        // Log to JSONL for TUI consumption
        logger.log_event(&LogEntry::TokenUsage {
            timestamp: now_iso(),
            turn,
            prompt_tokens: prompt_toks,
            completion_tokens: completion_toks,
            total_tokens: prompt_toks + completion_toks,
            context_used_pct: context_manager.usage_percentage(),
        })?;
    }

    // ... existing captured_text / captured_tool_calls handling
}
```

### Observation Masking Placeholder Generation
```rust
// Source: Project-specific design, informed by JetBrains research
fn generate_placeholder(fn_name: &str, original_content: &str) -> String {
    let line_count = original_content.lines().count();
    let byte_count = original_content.len();

    // Extract a brief summary based on tool type
    let summary = match fn_name {
        "file_read" => {
            let first_line = original_content.lines().next().unwrap_or("");
            let truncated = if first_line.len() > 60 {
                format!("{}...", &first_line[..60])
            } else {
                first_line.to_string()
            };
            format!("{line_count} lines, starts with: {truncated}")
        }
        "shell_exec" => {
            // Try to parse as JSON to get exit_code
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(original_content) {
                let exit_code = v.get("exit_code").and_then(|v| v.as_i64()).unwrap_or(-1);
                let stdout_len = v.get("stdout").and_then(|v| v.as_str()).map(|s| s.len()).unwrap_or(0);
                format!("exit_code={exit_code}, stdout={stdout_len} bytes")
            } else {
                format!("{byte_count} bytes of output")
            }
        }
        "file_write" => {
            // file_write results are small JSON, usually fine to keep
            format!("{byte_count} bytes")
        }
        _ => format!("{byte_count} bytes of output"),
    };

    format!("[{fn_name} result masked -- {summary}]")
}
```

### New Config Fields for Context Management
```rust
// Addition to AppConfig
pub struct AppConfig {
    // ... existing fields ...
    pub context_limit: usize,           // Already exists

    // New Phase 3 fields
    pub soft_threshold_pct: f64,        // Default: 0.70
    pub hard_threshold_pct: f64,        // Default: 0.90
    pub carryover_turns: usize,         // Default: 5
    pub max_restarts: Option<u32>,      // Default: None (unlimited)
    pub auto_restart: bool,             // Default: true
}
```

### TOML Config Example
```toml
[context]
soft_threshold_pct = 0.70
hard_threshold_pct = 0.90
carryover_turns = 5
max_restarts = 10        # Optional, default unlimited
auto_restart = true      # Default true; false = pause for confirmation
```

### New LogEntry Variant for Token Usage
```rust
// Addition to LogEntry enum
#[serde(rename = "token_usage")]
TokenUsage {
    timestamp: String,
    turn: u64,
    prompt_tokens: usize,
    completion_tokens: usize,
    total_tokens: usize,
    context_used_pct: f64,
}

#[serde(rename = "context_mask")]
ContextMask {
    timestamp: String,
    observations_masked: usize,
    total_masked: usize,
    context_reclaimed_pct: f64,
}

#[serde(rename = "session_restart")]
SessionRestart {
    timestamp: String,
    session_number: u32,
    previous_turns: u64,
    carryover_messages: usize,
    reason: String,
}
```

### Restart Marker Injection
```rust
// Injected as system message at start of restarted session
fn restart_marker(session_number: u32, previous_turns: u64) -> String {
    format!(
        "[Session restarted. Session #{session_number}. \
         Previous session ran {previous_turns} turns. \
         Check your workspace files for progress state.]"
    )
}
```

### Wind-Down Message
```rust
// Injected as system message when hard threshold is hit
fn wind_down_message(context_used_pct: f64, remaining_tokens: usize) -> String {
    format!(
        "[Context window {:.0}% full (~{remaining_tokens} tokens remaining). \
         Please wrap up your current task and write any important state to \
         workspace files. The session will restart shortly.]",
        context_used_pct * 100.0
    )
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Character/4 heuristic for token estimation | Actual token counts from Ollama via `prompt_eval_count` / `eval_count` | Ollama added `stream_options.include_usage` support, mid-2025 | Accurate context tracking, no guesswork |
| LLM-based summarization for context compression | Simple observation masking with descriptive placeholders | JetBrains research Dec 2025 | Equal or better performance, 50%+ cost reduction, no extra LLM calls |
| Full context dump on restart | Carry over last N turns + reload SYSTEM_PROMPT.md from disk | Anthropic context engineering guide 2025 | Agent can self-modify bootstrap, maintains recent context |

**Deprecated/outdated:**
- `total_chars / 4 > context_limit` heuristic from Phase 2: Replace with actual token counts. Keep as fallback only when `captured_usage` is `None`.

## Open Questions

1. **genai Usage field types are `Option<i32>`, not guaranteed**
   - What we know: The `Usage` struct has `prompt_tokens: Option<i32>` and `completion_tokens: Option<i32>`. They could be `None` even with `capture_usage` enabled, depending on the provider/model.
   - What's unclear: Whether current Ollama versions always populate both fields for all models.
   - Recommendation: Treat as optional, fall back to character heuristic when `None`. Log a warning on first fallback occurrence.

2. **Exact token savings from masking**
   - What we know: Each masked observation saves the token count of that observation's content. But the model will re-tokenize the placeholder too.
   - What's unclear: Precise token reduction per masking operation without a tokenizer.
   - Recommendation: After masking, rely on the NEXT response's `prompt_tokens` to see the actual new context size. Don't try to estimate savings -- measure them.

3. **MessageContent mutation API**
   - What we know: `ChatMessage.content` is `MessageContent` (not a plain `String`). Setting it to a new value requires constructing a `MessageContent`.
   - What's unclear: Exact API for constructing `MessageContent` from a string (likely `MessageContent::from(string)` or similar).
   - Recommendation: Verify via compilation. If `MessageContent` doesn't implement `From<String>`, use whatever constructor the genai crate provides. This is a minor implementation detail.

4. **Carryover turn boundary detection**
   - What we know: Must carry complete interaction cycles, not split mid-tool-call.
   - What's unclear: Whether "5 turns" means "5 messages" or "5 complete interaction cycles" (each cycle = potentially many messages).
   - Recommendation: Define a "turn" as one complete cycle: system/user message through all tool calls and responses to the next assistant text completion. Carry over N such cycles. Default to 5 cycles.

## Sources

### Primary (HIGH confidence)
- genai GitHub source: `src/chat/chat_options.rs` -- `capture_usage` field and `with_capture_usage(true)` method verified
- genai GitHub source: `src/chat/chat_stream.rs` -- `StreamEnd.captured_usage: Option<Usage>` verified
- genai GitHub source: `src/chat/usage.rs` -- `Usage { prompt_tokens: Option<i32>, completion_tokens: Option<i32>, total_tokens: Option<i32> }` verified
- genai GitHub source: `src/adapter/adapters/openai/adapter_impl.rs` -- confirms `stream_options: {"include_usage": true}` is sent when `capture_usage` is enabled
- Ollama docs: https://docs.ollama.com/api/openai-compatibility -- `stream_options.include_usage` listed as supported
- Ollama docs: https://docs.ollama.com/api/usage -- native API token fields documented
- Existing codebase: `src/agent/agent_loop.rs`, `src/agent/logging.rs`, `src/config/schema.rs` -- current implementation verified via direct source reading

### Secondary (MEDIUM confidence)
- Ollama GitHub issue #4448 (closed/resolved Feb 2026) -- confirms `stream_options.include_usage` is working in streaming mode
- JetBrains Research blog Dec 2025: "Cutting Through the Noise: Smarter Context Management" -- observation masking research
- Anthropic engineering guide: "Effective context engineering for AI agents" -- compaction and note-taking patterns
- genai GitHub: Ollama adapter delegates to OpenAI adapter, uses `/v1/` endpoint (not native `/api/chat`)

### Tertiary (LOW confidence)
- Exact Ollama version that added `stream_options.include_usage` support -- confirmed working by mid-2025 but exact version number not found
- JetBrains "10 turns window" recommendation -- specific to their tested agents, may need tuning for Ouroboros

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - genai source code verified, Ollama docs confirmed, no new dependencies needed
- Architecture: HIGH - Patterns derived from existing codebase structure plus verified genai APIs
- Token tracking: HIGH - Full chain verified: ChatOptions -> stream_options -> Ollama -> StreamEnd.captured_usage
- Observation masking: HIGH - Research-backed approach, straightforward Vec mutation on public field
- Session restart: MEDIUM - Design pattern is sound but carryover boundary detection needs implementation validation
- Pitfalls: HIGH - Based on verified API behavior and prior phase experience

**Research date:** 2026-02-04
**Valid until:** 2026-03-04 (30 days -- stable domain, genai API unlikely to change significantly)
