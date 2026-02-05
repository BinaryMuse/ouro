//! Context management: token tracking, graduated threshold evaluation,
//! and observation masking for extending effective context lifetime.
//!
//! The ContextManager is the core engine that the agent loop consults after
//! every turn to decide whether to continue normally, mask old observations,
//! tell the agent to wind down, or trigger a session restart.
//!
//! Key design points:
//! - prompt_tokens from Ollama IS the total context size (not additive across turns)
//! - Character-count fallback (1 token ~ 4 chars) when token data unavailable
//! - Observation masking replaces tool output content with summary placeholders
//!   while preserving message structure (never removes messages)

use genai::chat::{ChatMessage, ChatRole, MessageContent, ToolResponse};

/// Substring used to detect already-masked observations in tool response content.
const MASKED_MARKER: &str = "masked";

/// Default number of observations to mask per evaluation round.
const DEFAULT_MASK_BATCH_SIZE: usize = 3;

// ---------------------------------------------------------------------------
// ContextAction
// ---------------------------------------------------------------------------

/// Action the agent loop should take based on current context pressure.
#[derive(Debug, PartialEq)]
pub enum ContextAction {
    /// Context is healthy -- proceed normally.
    Continue,
    /// Soft threshold hit -- mask `count` oldest unmasked observations.
    Mask { count: usize },
    /// Hard threshold hit for the first time -- tell the agent to wrap up.
    WindDown,
    /// Hard threshold hit again after wind-down -- restart session.
    Restart,
}

// ---------------------------------------------------------------------------
// MaskResult
// ---------------------------------------------------------------------------

/// Outcome of a mask_oldest_observations call.
#[derive(Debug, PartialEq)]
pub struct MaskResult {
    /// Number of observations masked in this round.
    pub masked_count: usize,
    /// Total observations masked across the entire session.
    pub total_masked: usize,
}

// ---------------------------------------------------------------------------
// ContextManager
// ---------------------------------------------------------------------------

/// Tracks token usage, evaluates context pressure thresholds, and drives
/// observation masking decisions.
#[allow(dead_code)]
pub struct ContextManager {
    /// Model's context window size in tokens.
    context_limit: usize,
    /// Fraction (0.0..1.0) at which soft masking begins.
    soft_threshold_pct: f64,
    /// Fraction (0.0..1.0) at which wind-down / restart triggers.
    hard_threshold_pct: f64,
    /// Number of recent turns to preserve during restart carryover.
    carryover_turns: usize,
    /// Latest prompt_tokens from Ollama response (already the full context size).
    prompt_tokens: usize,
    /// Cumulative completion tokens across the session (for logging).
    completion_tokens_total: usize,
    /// Fallback character count when token data is unavailable.
    total_chars: usize,
    /// Total observations masked so far this session.
    masked_count: usize,
    /// 1-based session number, incremented on restart.
    session_number: u32,
    /// Turns completed in the current session.
    turn_count: u64,
    /// Whether a WindDown action has already been sent.
    wind_down_sent: bool,
}

#[allow(dead_code)]
impl ContextManager {
    /// Create a new ContextManager with the given thresholds.
    ///
    /// `session_number` starts at 1. All counters start at zero.
    pub fn new(
        context_limit: usize,
        soft_threshold_pct: f64,
        hard_threshold_pct: f64,
        carryover_turns: usize,
    ) -> Self {
        Self {
            context_limit,
            soft_threshold_pct,
            hard_threshold_pct,
            carryover_turns,
            prompt_tokens: 0,
            completion_tokens_total: 0,
            total_chars: 0,
            masked_count: 0,
            session_number: 1,
            turn_count: 0,
            wind_down_sent: false,
        }
    }

    // -- Token tracking -----------------------------------------------------

    /// Update token usage from the latest Ollama response.
    ///
    /// `prompt_tokens` is SET (not added) because Ollama's prompt_tokens
    /// already reflects the full conversation context size. `completion_tokens`
    /// is added to the cumulative total for logging purposes.
    pub fn update_token_usage(&mut self, prompt_tokens: usize, completion_tokens: usize) {
        self.prompt_tokens = prompt_tokens;
        self.completion_tokens_total += completion_tokens;
    }

    /// Add characters to the fallback counter.
    ///
    /// Used when token data is unavailable; the heuristic 1 token ~ 4 chars
    /// converts this to an estimated token count.
    pub fn add_chars(&mut self, chars: usize) {
        self.total_chars += chars;
    }

    /// Current context usage as a fraction of the context limit.
    ///
    /// Uses real token data when available (prompt_tokens > 0), otherwise
    /// falls back to the character heuristic (total_chars / 4).
    pub fn usage_percentage(&self) -> f64 {
        if self.prompt_tokens > 0 {
            self.prompt_tokens as f64 / self.context_limit as f64
        } else {
            (self.total_chars / 4) as f64 / self.context_limit as f64
        }
    }

    // -- Threshold evaluation -----------------------------------------------

    /// Evaluate current context pressure and return the appropriate action.
    ///
    /// Decision logic (checked in priority order):
    /// 1. >= hard_threshold AND wind_down already sent => Restart
    /// 2. >= hard_threshold AND wind_down NOT sent => WindDown (sets flag)
    /// 3. >= soft_threshold => Mask { count: DEFAULT_MASK_BATCH_SIZE }
    /// 4. Otherwise => Continue
    pub fn evaluate(&mut self) -> ContextAction {
        let pct = self.usage_percentage();

        if pct >= self.hard_threshold_pct {
            if self.wind_down_sent {
                return ContextAction::Restart;
            }
            self.wind_down_sent = true;
            return ContextAction::WindDown;
        }

        if pct >= self.soft_threshold_pct {
            return ContextAction::Mask {
                count: DEFAULT_MASK_BATCH_SIZE,
            };
        }

        ContextAction::Continue
    }

    // -- Turn and session management ----------------------------------------

    /// Increment the turn counter.
    pub fn increment_turn(&mut self) {
        self.turn_count += 1;
    }

    /// Prepare for a session restart: increment session_number, reset all
    /// per-session counters.
    pub fn prepare_restart(&mut self) {
        self.session_number += 1;
        self.prompt_tokens = 0;
        self.completion_tokens_total = 0;
        self.total_chars = 0;
        self.turn_count = 0;
        self.masked_count = 0;
        self.wind_down_sent = false;
    }

    // -- Getters ------------------------------------------------------------

    /// Current session number (1-based).
    pub fn session_number(&self) -> u32 {
        self.session_number
    }

    /// Turns completed in the current session.
    pub fn turn_count(&self) -> u64 {
        self.turn_count
    }

    /// Total observations masked this session.
    pub fn masked_count(&self) -> usize {
        self.masked_count
    }

    /// Latest prompt_tokens value from Ollama.
    pub fn prompt_tokens(&self) -> usize {
        self.prompt_tokens
    }

    /// Number of recent turns to carry over on restart.
    pub fn carryover_turns(&self) -> usize {
        self.carryover_turns
    }
}

// ---------------------------------------------------------------------------
// Observation masking helpers
// ---------------------------------------------------------------------------

/// Generate a descriptive summary placeholder for a masked tool observation.
///
/// The placeholder preserves enough information for the agent to understand
/// what the original output contained without the full content.
pub fn generate_placeholder(fn_name: &str, original_content: &str) -> String {
    match fn_name {
        "file_read" => {
            let line_count = original_content.lines().count();
            let first_line = original_content.lines().next().unwrap_or("");
            let first_line_display = if first_line.len() > 60 {
                format!("{}...", &first_line[..60])
            } else {
                first_line.to_string()
            };
            format!(
                "[file_read result masked -- {} lines, starts with: {}]",
                line_count, first_line_display
            )
        }
        "shell_exec" => {
            // Try to parse as JSON to extract structured info.
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(original_content) {
                let exit_code = val
                    .get("exit_code")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(-1);
                let stdout_len = val
                    .get("stdout")
                    .and_then(|v| v.as_str())
                    .map(|s| s.len())
                    .unwrap_or(0);
                format!(
                    "[shell_exec result masked -- exit_code={}, stdout={} bytes]",
                    exit_code, stdout_len
                )
            } else {
                format!(
                    "[shell_exec result masked -- {} bytes of output]",
                    original_content.len()
                )
            }
        }
        "file_write" => {
            format!(
                "[file_write result masked -- {} bytes]",
                original_content.len()
            )
        }
        _ => {
            format!(
                "[{} result masked -- {} bytes of output]",
                fn_name,
                original_content.len()
            )
        }
    }
}

/// Check whether a tool response has already been masked.
///
/// A masked observation starts with `[` and contains the masked marker.
pub fn is_already_masked(content: &str) -> bool {
    content.starts_with('[') && content.contains(MASKED_MARKER)
}

/// Walk `messages` from oldest to newest, masking up to `count` unmasked
/// tool response observations by replacing their content with summary
/// placeholders.
///
/// Returns a `MaskResult` describing how many were masked this round and
/// the running total. The `context_manager` masked_count is updated.
///
/// IMPORTANT: Messages are never removed -- only their content is replaced.
/// This preserves the tool call/response chain that providers expect.
pub fn mask_oldest_observations(
    messages: &mut [ChatMessage],
    count: usize,
    context_manager: &mut ContextManager,
) -> MaskResult {
    let mut masked_this_round = 0;

    // First pass: collect (index, call_id) pairs for Tool-role messages
    // that are not yet masked.
    let tool_msg_indices: Vec<(usize, String)> = messages
        .iter()
        .enumerate()
        .filter_map(|(i, msg)| {
            if msg.role != ChatRole::Tool {
                return None;
            }
            // Extract the ToolResponse content from the message.
            let tool_responses = msg.content.tool_responses();
            if let Some(tr) = tool_responses.first() {
                if is_already_masked(&tr.content) {
                    return None;
                }
                Some((i, tr.call_id.clone()))
            } else {
                // Fallback: check if it's a text-content tool message (unlikely
                // but defensively handle it).
                if let Some(text) = msg.content.first_text()
                    && is_already_masked(text)
                {
                    return None;
                }
                Some((i, String::new()))
            }
        })
        .collect();

    for (msg_idx, call_id) in tool_msg_indices {
        if masked_this_round >= count {
            break;
        }

        // Try to find the fn_name from the preceding assistant message's
        // tool calls by matching call_id.
        let fn_name = if !call_id.is_empty() {
            find_fn_name_for_call_id(messages, msg_idx, &call_id)
                .unwrap_or_else(|| "unknown".to_string())
        } else {
            "unknown".to_string()
        };

        // Extract current content, generate placeholder, and replace.
        let msg = &messages[msg_idx];
        let original_content = extract_tool_content(msg);
        let placeholder = generate_placeholder(&fn_name, &original_content);

        // Replace the message content with a ToolResponse containing the
        // placeholder text, preserving the call_id.
        let effective_call_id = if call_id.is_empty() {
            "masked".to_string()
        } else {
            call_id.clone()
        };
        messages[msg_idx] = ChatMessage {
            role: ChatRole::Tool,
            content: MessageContent::from(ToolResponse::new(
                effective_call_id,
                placeholder,
            )),
            options: None,
        };

        masked_this_round += 1;
        context_manager.masked_count += 1;
    }

    MaskResult {
        masked_count: masked_this_round,
        total_masked: context_manager.masked_count,
    }
}

/// Generate the system notification text for a masking round.
pub fn generate_mask_notification(
    masked_count: usize,
    total_masked: usize,
    reclaimed_pct: f64,
) -> String {
    format!(
        "[Context compressed: {} observations masked this round, {} total, ~{:.0}% context reclaimed]",
        masked_count, total_masked, reclaimed_pct
    )
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Search backwards from `tool_msg_idx` to find the assistant message whose
/// tool calls contain a ToolCall with the given `call_id`, and return its
/// `fn_name`.
fn find_fn_name_for_call_id(
    messages: &[ChatMessage],
    tool_msg_idx: usize,
    call_id: &str,
) -> Option<String> {
    for i in (0..tool_msg_idx).rev() {
        if messages[i].role != ChatRole::Assistant {
            continue;
        }
        let tool_calls = messages[i].content.tool_calls();
        for tc in tool_calls {
            if tc.call_id == call_id {
                return Some(tc.fn_name.clone());
            }
        }
        // Only check the immediately preceding assistant message.
        break;
    }
    None
}

/// Extract the textual content from a Tool-role message, whether it is stored
/// as a ToolResponse part or a plain Text part.
fn extract_tool_content(msg: &ChatMessage) -> String {
    let tool_responses = msg.content.tool_responses();
    if let Some(tr) = tool_responses.first() {
        return tr.content.clone();
    }
    msg.content.first_text().unwrap_or("").to_string()
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Task 1 tests: threshold evaluation ---------------------------------

    #[test]
    fn test_continue_when_under_soft_threshold() {
        // 50% usage, soft at 70% => Continue
        let mut cm = ContextManager::new(1000, 0.70, 0.90, 5);
        cm.update_token_usage(500, 10);
        assert_eq!(cm.evaluate(), ContextAction::Continue);
    }

    #[test]
    fn test_mask_when_at_soft_threshold() {
        // 75% usage, soft at 70% => Mask
        let mut cm = ContextManager::new(1000, 0.70, 0.90, 5);
        cm.update_token_usage(750, 10);
        let action = cm.evaluate();
        assert!(
            matches!(action, ContextAction::Mask { count } if count == DEFAULT_MASK_BATCH_SIZE),
            "Expected Mask, got {:?}",
            action
        );
    }

    #[test]
    fn test_winddown_when_at_hard_threshold() {
        // 92% usage, hard at 90% => WindDown, flag set
        let mut cm = ContextManager::new(1000, 0.70, 0.90, 5);
        cm.update_token_usage(920, 10);
        assert_eq!(cm.evaluate(), ContextAction::WindDown);
        assert!(cm.wind_down_sent);
    }

    #[test]
    fn test_restart_after_winddown() {
        // First eval at hard threshold => WindDown
        // Second eval at hard threshold => Restart
        let mut cm = ContextManager::new(1000, 0.70, 0.90, 5);
        cm.update_token_usage(920, 10);
        assert_eq!(cm.evaluate(), ContextAction::WindDown);
        assert_eq!(cm.evaluate(), ContextAction::Restart);
    }

    #[test]
    fn test_fallback_char_heuristic() {
        // No prompt_tokens set, add 2000 chars => 2000/4 = 500 tokens
        // With context_limit 1000 => 50% usage
        let mut cm = ContextManager::new(1000, 0.70, 0.90, 5);
        cm.add_chars(2000);
        let pct = cm.usage_percentage();
        assert!(
            (pct - 0.50).abs() < 0.001,
            "Expected ~0.50, got {}",
            pct
        );
        // Should be Continue at 50%
        assert_eq!(cm.evaluate(), ContextAction::Continue);
    }

    #[test]
    fn test_prompt_tokens_not_additive() {
        // Call update_token_usage twice -- prompt_tokens should be latest, not summed
        let mut cm = ContextManager::new(1000, 0.70, 0.90, 5);
        cm.update_token_usage(300, 10);
        cm.update_token_usage(500, 20);
        assert_eq!(cm.prompt_tokens(), 500);
        // completion_tokens should be cumulative
        assert_eq!(cm.completion_tokens_total, 30);
    }

    // -- Task 2 tests: observation masking ----------------------------------

    #[test]
    fn test_generate_placeholder_file_read() {
        let content = "line one of the file\nline two\nline three\n";
        let placeholder = generate_placeholder("file_read", content);
        assert!(placeholder.starts_with('['));
        assert!(placeholder.contains("masked"));
        assert!(placeholder.contains("3 lines"));
        assert!(placeholder.contains("starts with: line one of the file"));
    }

    #[test]
    fn test_generate_placeholder_shell_exec_json() {
        let content = r#"{"exit_code":0,"stdout":"hello world","stderr":""}"#;
        let placeholder = generate_placeholder("shell_exec", content);
        assert!(placeholder.starts_with('['));
        assert!(placeholder.contains("masked"));
        assert!(placeholder.contains("exit_code=0"));
        assert!(placeholder.contains("stdout=11 bytes"));
    }

    #[test]
    fn test_generate_placeholder_shell_exec_plain() {
        let content = "some plain text output that is not json";
        let placeholder = generate_placeholder("shell_exec", content);
        assert!(placeholder.starts_with('['));
        assert!(placeholder.contains("masked"));
        assert!(placeholder.contains("bytes of output"));
    }

    #[test]
    fn test_is_already_masked() {
        assert!(is_already_masked("[file_read result masked -- 10 lines, starts with: foo]"));
        assert!(is_already_masked("[shell_exec result masked -- exit_code=0, stdout=5 bytes]"));
        assert!(!is_already_masked("some normal tool output"));
        assert!(!is_already_masked("this contains masked but no bracket"));
    }

    #[test]
    fn test_mask_oldest_observations_walks_oldest_first() {
        use genai::chat::ToolCall;

        let mut cm = ContextManager::new(1000, 0.70, 0.90, 5);

        // Build a mini conversation: assistant tool call -> tool response x2
        let tool_call_1 = ToolCall {
            call_id: "call_1".to_string(),
            fn_name: "file_read".to_string(),
            fn_arguments: serde_json::json!({"path": "foo.txt"}),
            thought_signatures: None,
        };
        let tool_call_2 = ToolCall {
            call_id: "call_2".to_string(),
            fn_name: "shell_exec".to_string(),
            fn_arguments: serde_json::json!({"command": "ls"}),
            thought_signatures: None,
        };

        let assistant_msg = ChatMessage::from(vec![tool_call_1, tool_call_2]);
        let tool_resp_1: ChatMessage =
            ToolResponse::new("call_1", "line1\nline2\nline3\n").into();
        let tool_resp_2: ChatMessage = ToolResponse::new(
            "call_2",
            r#"{"exit_code":0,"stdout":"file.txt","stderr":""}"#,
        )
        .into();

        let mut messages = vec![assistant_msg, tool_resp_1, tool_resp_2];

        // Mask 1 observation
        let result = mask_oldest_observations(&mut messages, 1, &mut cm);
        assert_eq!(result.masked_count, 1);
        assert_eq!(result.total_masked, 1);
        assert_eq!(cm.masked_count(), 1);

        // First tool response should now be masked
        let first_tool = &messages[1];
        let content = extract_tool_content(first_tool);
        assert!(is_already_masked(&content), "Expected masked, got: {}", content);
        assert!(content.contains("file_read"));
        assert!(content.contains("3 lines"));

        // Second tool response should still be unmasked
        let second_tool = &messages[2];
        let content2 = extract_tool_content(second_tool);
        assert!(!is_already_masked(&content2));

        // Mask 1 more -- should get the second one
        let result2 = mask_oldest_observations(&mut messages, 1, &mut cm);
        assert_eq!(result2.masked_count, 1);
        assert_eq!(result2.total_masked, 2);

        let content2_after = extract_tool_content(&messages[2]);
        assert!(is_already_masked(&content2_after));
        assert!(content2_after.contains("shell_exec"));
    }

    #[test]
    fn test_mask_skips_already_masked() {
        let mut cm = ContextManager::new(1000, 0.70, 0.90, 5);

        // Create a tool response that is already masked
        let already_masked: ChatMessage = ToolResponse::new(
            "call_1",
            "[file_read result masked -- 5 lines, starts with: foo]",
        )
        .into();

        let mut messages = vec![already_masked];

        // Try to mask -- should skip it
        let result = mask_oldest_observations(&mut messages, 1, &mut cm);
        assert_eq!(result.masked_count, 0);
        assert_eq!(result.total_masked, 0);
    }

    #[test]
    fn test_generate_mask_notification() {
        let notification = generate_mask_notification(3, 10, 15.0);
        assert_eq!(
            notification,
            "[Context compressed: 3 observations masked this round, 10 total, ~15% context reclaimed]"
        );
    }
}
