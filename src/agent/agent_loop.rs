//! Core agent conversation loop with Ollama health check, streaming,
//! tool dispatch, context management, and graceful shutdown handling.
//!
//! This is the capstone module that brings together the session logger, system
//! prompt, tool definitions, context manager, and the genai client into a
//! working conversation loop. The loop:
//!
//! 1. Validates Ollama connectivity and model availability
//! 2. Loads the system prompt (re-read from disk each session)
//! 3. Streams model text to stdout in real time
//! 4. Dispatches tool calls through the safety layer
//! 5. Tracks token usage from StreamEnd and evaluates context pressure
//! 6. Masks old observations when soft threshold is reached
//! 7. Injects wind-down message at hard threshold
//! 8. Returns carryover messages for session restart at context exhaustion
//! 9. Logs all events to a JSONL session file

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use genai::chat::{
    ChatMessage, ChatOptions, ChatRequest, ChatStreamEvent, ToolCall, ToolResponse,
};
use genai::Client;

use crate::agent::context_manager::{
    mask_oldest_observations, generate_mask_notification, ContextAction, ContextManager,
};
use crate::agent::logging::{LogEntry, SessionLogger};
use crate::agent::system_prompt::build_system_prompt;
use crate::agent::tools::{define_tools, dispatch_tool_call, tool_descriptions};
use crate::config::AppConfig;
use crate::error::AgentError;
use crate::safety::SafetyLayer;
use crate::tui::event::{AgentEvent, AgentState};

// ---------------------------------------------------------------------------
// ShutdownReason / SessionResult
// ---------------------------------------------------------------------------

/// Why a session ended. Returned to the outer restart loop in main.rs.
pub enum ShutdownReason {
    /// User pressed Ctrl+C (graceful shutdown).
    UserShutdown,
    /// Context window exhausted -- carry over recent messages to next session.
    ContextFull {
        carryover_messages: Vec<ChatMessage>,
    },
    /// Maximum turns reached, unrecoverable error, or other termination.
    MaxTurnsOrError(String),
}

/// Result of a single agent session.
pub struct SessionResult {
    /// Why this session ended.
    pub shutdown_reason: ShutdownReason,
    /// Number of turns completed in this session.
    pub turns_completed: u64,
    /// The session number (1-based) that just ran.
    pub session_number: u32,
}

// ---------------------------------------------------------------------------
// Ollama health check
// ---------------------------------------------------------------------------

/// Validate that Ollama is running and the configured model is available.
///
/// Step 1: HTTP GET to `http://localhost:11434/` with 5-second timeout.
/// Step 2: HTTP POST to `http://localhost:11434/api/show` to verify the model.
///
/// Returns `Ok(())` if both checks pass. Returns an appropriate `AgentError`
/// if Ollama is unreachable or the model is not found.
async fn check_ollama_ready(model: &str) -> Result<(), AgentError> {
    let http = reqwest::Client::new();

    // Step 1: Check Ollama is running.
    let base_url = "http://localhost:11434/";
    http.get(base_url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| AgentError::OllamaUnavailable {
            url: base_url.to_string(),
            message: format!("Is Ollama running? {e}"),
        })?;

    // Step 2: Check model is available.
    let show_url = "http://localhost:11434/api/show";
    let resp = http
        .post(show_url)
        .json(&serde_json::json!({ "model": model }))
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| AgentError::ModelNotAvailable {
            model: model.to_string(),
            message: format!("Failed to query model info: {e}"),
        })?;

    if !resp.status().is_success() {
        return Err(AgentError::ModelNotAvailable {
            model: model.to_string(),
            message: format!(
                "Model not found (HTTP {}). Run `ollama pull {model}` to download it.",
                resp.status()
            ),
        });
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Timestamp helper
// ---------------------------------------------------------------------------

/// Return the current UTC time as an ISO 8601 string with milliseconds.
fn now_iso_timestamp() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

// ---------------------------------------------------------------------------
// Carryover extraction
// ---------------------------------------------------------------------------

/// Extract the last `n_turns` complete interaction cycles from the message
/// history for carryover to the next session.
///
/// A "turn boundary" is after an assistant text response (not a tool call),
/// before the next user/system message. Tool call/response pairs are never
/// split -- if the last messages are assistant(tool_calls) -> tool_responses,
/// the full sequence is included.
fn extract_carryover(messages: &[ChatMessage], n_turns: usize) -> Vec<ChatMessage> {
    if n_turns == 0 || messages.is_empty() {
        return Vec::new();
    }

    // Walk backward to find turn boundaries.
    // A turn boundary is at position i when:
    //   messages[i].role == Assistant AND messages[i] has no tool_calls
    //   (meaning it's a text-only assistant response, ending a turn)
    let mut boundaries: Vec<usize> = Vec::new();

    for i in (0..messages.len()).rev() {
        let msg = &messages[i];
        if msg.role == genai::chat::ChatRole::Assistant {
            // Check if this is a text-only response (no tool calls).
            let tool_calls = msg.content.tool_calls();
            if tool_calls.is_empty() {
                // This is a turn boundary (end of a complete turn).
                boundaries.push(i);
                if boundaries.len() >= n_turns {
                    break;
                }
            }
        }
    }

    if boundaries.is_empty() {
        // No clean turn boundaries found. Fall back: take the last few messages.
        let start = messages.len().saturating_sub(n_turns * 3);
        return messages[start..].to_vec();
    }

    // boundaries are in reverse order; the last one is the earliest start point.
    let start_idx = *boundaries.last().unwrap();

    // Find the actual start: go back to include any preceding user/system
    // message that initiated this turn.
    let adjusted_start = if start_idx > 0 {
        let prev = &messages[start_idx - 1];
        if prev.role == genai::chat::ChatRole::User
            || prev.role == genai::chat::ChatRole::System
        {
            start_idx - 1
        } else {
            start_idx
        }
    } else {
        start_idx
    };

    messages[adjusted_start..].to_vec()
}

// ---------------------------------------------------------------------------
// run_agent_session
// ---------------------------------------------------------------------------

/// Run a single agent session with context management.
///
/// This function blocks until one of:
/// - The user sends Ctrl+C (graceful shutdown)
/// - Context pressure triggers a restart (ContextFull)
/// - An unrecoverable error occurs
///
/// The caller (outer restart loop in main.rs) handles the `SessionResult` to
/// decide whether to start a new session with carryover messages.
///
/// # Arguments
///
/// * `config` - Resolved application configuration (model, workspace, limits)
/// * `safety` - Safety layer for command filtering and workspace enforcement
/// * `session_number` - 1-based session counter (incremented by outer loop)
/// * `carryover_messages` - Messages from previous session to seed context
/// * `shutdown` - Shared shutdown flag (owned by outer loop, shared across sessions)
/// * `event_tx` - Optional TUI event channel. When `Some`, agent events are
///   sent for real-time TUI rendering. When `None`, headless mode (no events).
/// * `pause_flag` - Optional pause control. When `Some(true)`, the loop blocks
///   between turns until unpaused. When `None`, pause is never checked.
pub async fn run_agent_session(
    config: &AppConfig,
    safety: &SafetyLayer,
    session_number: u32,
    carryover_messages: &[ChatMessage],
    shutdown: Arc<AtomicBool>,
    event_tx: Option<tokio::sync::mpsc::UnboundedSender<AgentEvent>>,
    pause_flag: Option<Arc<AtomicBool>>,
) -> anyhow::Result<SessionResult> {
    // -- Helper: send event if TUI channel exists, ignore send errors (TUI may have closed)
    let send_event = {
        let tx = event_tx.clone();
        move |event: AgentEvent| {
            if let Some(ref tx) = tx {
                let _ = tx.send(event);
            }
        }
    };

    // -- Startup: validate Ollama and model
    check_ollama_ready(&config.model).await?;

    // -- Create session logger
    let mut logger = SessionLogger::new(&config.workspace)?;

    // -- Build system prompt with harness context (re-read from disk each session)
    let system_prompt = build_system_prompt(
        &config.workspace,
        &config.model,
        &tool_descriptions(),
        session_number,
    )
    .await?;

    // -- Create ContextManager for this session
    let mut context_manager = ContextManager::new(
        config.context_limit,
        config.soft_threshold_pct,
        config.hard_threshold_pct,
        config.carryover_turns,
    );

    // -- Create genai client (defaults to Ollama for non-prefixed model names)
    let client = Client::default();

    // -- Build initial chat request with system prompt and tools
    let mut chat_req = ChatRequest::from_system(&system_prompt).with_tools(define_tools());

    // -- Add carryover messages from previous session
    if !carryover_messages.is_empty() {
        for msg in carryover_messages {
            chat_req = chat_req.append_message(msg.clone());
        }
        eprintln!(
            "[context] Loaded {} carryover messages from previous session",
            carryover_messages.len()
        );
    }

    // -- Inject restart marker if this is a restarted session
    if session_number > 1 {
        let restart_marker = format!(
            "[Session restarted. Session #{session_number}. Previous session context was full. \
             Check your workspace files for progress state.]"
        );
        chat_req = chat_req.append_message(ChatMessage::system(&restart_marker));
    }

    // -- Configure streaming capture options (with usage tracking)
    let chat_options = ChatOptions::default()
        .with_capture_content(true)
        .with_capture_tool_calls(true)
        .with_capture_usage(true);

    // -- Seed the char-based fallback counter with the system prompt size
    context_manager.add_chars(system_prompt.len());

    // -- Log session start
    logger.log_session_start(&config.model, &config.workspace)?;

    // -- Print startup info to stderr (not stdout, which is for model output)
    eprintln!(
        "Ouroboros agent started (session #{session_number}).\n  Model: {}\n  Workspace: {}\n  Log: {}",
        config.model,
        config.workspace.display(),
        logger.log_path().display(),
    );

    // -- Main loop state
    let mut turn: u64 = 0;
    let mut tool_call_count: u64 = 0;
    let shutdown_reason;

    loop {
        // Check shutdown flag between turns.
        if shutdown.load(Ordering::SeqCst) {
            shutdown_reason = "user_shutdown";
            break;
        }

        // Check pause flag between turns (let current tool finish, pause before next LLM call).
        if let Some(ref pf) = pause_flag {
            if pf.load(Ordering::SeqCst) {
                send_event(AgentEvent::StateChanged(AgentState::Paused));
                // Spin-wait with small sleep until unpaused or shutdown.
                while pf.load(Ordering::SeqCst) && !shutdown.load(Ordering::SeqCst) {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                if shutdown.load(Ordering::SeqCst) {
                    // User quit while paused.
                    shutdown_reason = "user_shutdown";
                    break;
                }
                send_event(AgentEvent::StateChanged(AgentState::Idle));
            }
        }

        turn += 1;

        // -- Emit Thinking state before streaming
        send_event(AgentEvent::StateChanged(AgentState::Thinking));

        // -- Stream model response
        let stream_res = match client
            .exec_chat_stream(&config.model, chat_req.clone(), Some(&chat_options))
            .await
        {
            Ok(res) => res,
            Err(e) => {
                let msg = format!("LLM stream error: {e}");
                eprintln!("[error] {msg}");
                send_event(AgentEvent::Error {
                    timestamp: now_iso_timestamp(),
                    turn,
                    message: msg.clone(),
                });
                logger.log_event(&LogEntry::Error {
                    timestamp: now_iso_timestamp(),
                    turn,
                    message: msg.clone(),
                })?;
                logger.log_session_end(turn, "error")?;
                return Ok(SessionResult {
                    shutdown_reason: ShutdownReason::MaxTurnsOrError(msg),
                    turns_completed: turn,
                    session_number,
                });
            }
        };

        let mut stream = stream_res.stream;
        let mut captured_text: Option<String> = None;
        let mut captured_tool_calls: Vec<ToolCall> = Vec::new();

        while let Some(event) = stream.next().await {
            match event {
                Ok(ChatStreamEvent::Chunk(chunk)) => {
                    // Print text to stdout in real time.
                    print!("{}", chunk.content);
                    std::io::stdout().flush().ok();
                }
                Ok(ChatStreamEvent::End(end)) => {
                    // Extract captured text content.
                    if let Some(text) = end.captured_first_text() {
                        captured_text = Some(text.to_string());
                    }
                    // Extract captured tool calls.
                    if let Some(calls) = end.captured_tool_calls() {
                        captured_tool_calls =
                            calls.into_iter().cloned().collect();
                    }

                    // Extract token usage from StreamEnd.
                    if let Some(usage) = &end.captured_usage {
                        let prompt_toks =
                            usage.prompt_tokens.unwrap_or(0) as usize;
                        let completion_toks =
                            usage.completion_tokens.unwrap_or(0) as usize;
                        context_manager.update_token_usage(
                            prompt_toks,
                            completion_toks,
                        );
                        logger.log_event(&LogEntry::TokenUsage {
                            timestamp: now_iso_timestamp(),
                            turn,
                            prompt_tokens: prompt_toks,
                            completion_tokens: completion_toks,
                            total_tokens: prompt_toks + completion_toks,
                            context_used_pct: context_manager
                                .usage_percentage(),
                        })?;
                        // Emit context pressure event for TUI.
                        send_event(AgentEvent::ContextPressure {
                            usage_pct: context_manager.usage_percentage(),
                            prompt_tokens: prompt_toks,
                            context_limit: config.context_limit,
                        });
                    }
                }
                Ok(_) => {
                    // Start, ReasoningChunk, ThoughtSignatureChunk, ToolCallChunk -- ignore.
                }
                Err(e) => {
                    eprintln!("\n[stream error] {e}");
                    // Continue -- the End event may still arrive.
                }
            }
        }

        // -- Log assistant text if produced
        if let Some(ref text) = captured_text {
            context_manager.add_chars(text.len());
            logger.log_event(&LogEntry::AssistantText {
                timestamp: now_iso_timestamp(),
                turn,
                content: text.clone(),
            })?;
            // Emit thought text event for TUI.
            send_event(AgentEvent::ThoughtText {
                timestamp: now_iso_timestamp(),
                turn,
                content: text.clone(),
            });
        }

        if captured_tool_calls.is_empty() {
            // -- Text-only response (thinking out loud): append and re-prompt
            println!(); // newline after streamed text
            if let Some(text) = captured_text {
                chat_req = chat_req.append_message(ChatMessage::assistant(text));
            }
            // Continue to next iteration (re-prompt).
        } else {
            // -- Tool calls: dispatch each one
            println!(); // newline after any streamed text

            // Append the assistant message with tool calls to the conversation.
            // Convert captured tool calls into a proper assistant message.
            let assistant_msg: ChatMessage =
                ChatMessage::from(captured_tool_calls.clone());
            chat_req = chat_req.append_message(assistant_msg);

            for call in &captured_tool_calls {
                let call_id = &call.call_id;

                // Log tool call
                logger.log_event(&LogEntry::ToolCall {
                    timestamp: now_iso_timestamp(),
                    turn,
                    call_id: call_id.clone(),
                    fn_name: call.fn_name.clone(),
                    fn_arguments: call.fn_arguments.clone(),
                })?;

                // Print tool call info to stderr
                let args_summary = serde_json::to_string(&call.fn_arguments)
                    .unwrap_or_else(|_| "{}".to_string());
                let args_display = if args_summary.len() > 100 {
                    format!("{}...", &args_summary[..100])
                } else {
                    args_summary.clone()
                };
                eprintln!("[tool] {}({})", call.fn_name, args_display);

                // Emit Executing state and ToolCallStarted event for TUI.
                send_event(AgentEvent::StateChanged(AgentState::Executing));
                send_event(AgentEvent::ToolCallStarted {
                    timestamp: now_iso_timestamp(),
                    turn,
                    call_id: call_id.clone(),
                    fn_name: call.fn_name.clone(),
                    args_summary: args_summary.clone(),
                });

                // Track tool call count
                tool_call_count += 1;

                // Dispatch tool call through safety layer
                let result =
                    dispatch_tool_call(call, safety, &config.workspace).await;

                // Log tool result
                logger.log_event(&LogEntry::ToolResult {
                    timestamp: now_iso_timestamp(),
                    turn,
                    call_id: call_id.clone(),
                    fn_name: call.fn_name.clone(),
                    result: result.clone(),
                    error: None,
                })?;

                // Print abbreviated result to stderr
                let result_display = if result.len() > 200 {
                    format!("{}...", &result[..200])
                } else {
                    result.clone()
                };
                eprintln!("[result] {result_display}");

                // Emit ToolCallCompleted event for TUI.
                send_event(AgentEvent::ToolCallCompleted {
                    timestamp: now_iso_timestamp(),
                    turn,
                    call_id: call_id.clone(),
                    fn_name: call.fn_name.clone(),
                    result_summary: result_display,
                    full_result: result.clone(),
                });

                // Track character count for fallback context estimation
                context_manager.add_chars(result.len());

                // Append tool response to conversation
                chat_req = chat_req.append_message(ToolResponse::new(
                    call_id.clone(),
                    result,
                ));
            }
        }

        // -- Emit counters and transition to Idle between turns
        send_event(AgentEvent::CountersUpdated {
            turn,
            tool_calls: tool_call_count,
        });
        send_event(AgentEvent::StateChanged(AgentState::Idle));

        // -- Evaluate context pressure after each turn
        context_manager.increment_turn();
        match context_manager.evaluate() {
            ContextAction::Continue => { /* context is healthy */ }
            ContextAction::Mask { count } => {
                // Mask oldest unmasked observations to reclaim context.
                let pct_before = context_manager.usage_percentage();
                let mask_result = mask_oldest_observations(
                    &mut chat_req.messages,
                    count,
                    &mut context_manager,
                );
                let pct_after = context_manager.usage_percentage();
                let reclaimed_pct = (pct_before - pct_after) * 100.0;

                // Log masking event
                logger.log_event(&LogEntry::ContextMask {
                    timestamp: now_iso_timestamp(),
                    observations_masked: mask_result.masked_count,
                    total_masked: mask_result.total_masked,
                    context_reclaimed_pct: reclaimed_pct.max(0.0),
                })?;

                // Inject system notification
                let notification = generate_mask_notification(
                    mask_result.masked_count,
                    mask_result.total_masked,
                    reclaimed_pct.max(0.0),
                );
                chat_req = chat_req
                    .append_message(ChatMessage::system(&notification));

                eprintln!(
                    "[context] Masked {} observations ({} total), ~{:.0}% reclaimed",
                    mask_result.masked_count,
                    mask_result.total_masked,
                    reclaimed_pct.max(0.0),
                );
            }
            ContextAction::WindDown => {
                // Inject wind-down message -- let the agent have one more turn.
                let msg = format!(
                    "[Context window {:.0}% full. Please wrap up your current task and \
                     write any important state to workspace files. The session will restart shortly.]",
                    context_manager.usage_percentage() * 100.0
                );
                chat_req =
                    chat_req.append_message(ChatMessage::system(&msg));
                logger.log_event(&LogEntry::SystemMessage {
                    timestamp: now_iso_timestamp(),
                    content: msg,
                })?;
                eprintln!("[context] Wind-down message sent");
            }
            ContextAction::Restart => {
                // Extract carryover messages for the next session.
                let carryover =
                    extract_carryover(&chat_req.messages, config.carryover_turns);

                // Emit session restart event for TUI.
                send_event(AgentEvent::SessionRestarted { session_number });

                // Log restart event
                logger.log_event(&LogEntry::SessionRestart {
                    timestamp: now_iso_timestamp(),
                    session_number,
                    previous_turns: turn,
                    carryover_messages: carryover.len(),
                    reason: "hard_threshold_exceeded".to_string(),
                })?;
                logger.log_session_end(turn, "context_full_restart")?;

                eprintln!(
                    "[context] Session #{session_number} restarting. {turn} turns, {} carryover messages.",
                    carryover.len()
                );

                return Ok(SessionResult {
                    shutdown_reason: ShutdownReason::ContextFull {
                        carryover_messages: carryover,
                    },
                    turns_completed: turn,
                    session_number,
                });
            }
        }
    }

    // -- Log session end (normal shutdown)
    logger.log_session_end(turn, shutdown_reason)?;

    eprintln!(
        "Session ended: {shutdown_reason}. {turn} turns completed. Log: {}",
        logger.log_path().display(),
    );

    Ok(SessionResult {
        shutdown_reason: ShutdownReason::UserShutdown,
        turns_completed: turn,
        session_number,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that check_ollama_ready returns a sensible error when Ollama is
    /// not running (which is the expected state in CI / test environments).
    #[tokio::test]
    async fn health_check_returns_error_when_ollama_unavailable() {
        let result = check_ollama_ready("test-model").await;

        // In test environments Ollama is typically not running, so we expect
        // an OllamaUnavailable error. If Ollama happens to be running, the
        // test still passes (the model check may or may not succeed).
        match result {
            Err(AgentError::OllamaUnavailable { url, message }) => {
                assert!(url.contains("11434"));
                assert!(!message.is_empty());
            }
            // Ollama is running but model not found -- also acceptable.
            Err(AgentError::ModelNotAvailable { model, message }) => {
                assert_eq!(model, "test-model");
                assert!(!message.is_empty());
            }
            // Ollama is running AND the model exists -- unlikely but fine.
            Ok(()) => {}
            // Any other error variant is unexpected.
            Err(other) => panic!("Unexpected error variant: {other}"),
        }
    }

    #[test]
    fn extract_carryover_returns_empty_for_zero_turns() {
        let messages = vec![ChatMessage::system("hello")];
        let result = extract_carryover(&messages, 0);
        assert!(result.is_empty());
    }

    #[test]
    fn extract_carryover_returns_empty_for_empty_messages() {
        let result = extract_carryover(&[], 3);
        assert!(result.is_empty());
    }

    #[test]
    fn extract_carryover_preserves_complete_turns() {
        // Simulate a conversation: system -> user-like -> assistant text -> assistant text
        let messages = vec![
            ChatMessage::system("system prompt"),
            ChatMessage::assistant("I will help."),
            ChatMessage::assistant("Working on it..."),
            ChatMessage::assistant("Done with the task."),
        ];

        // Request 1 turn -- should get at least the last assistant text response.
        let result = extract_carryover(&messages, 1);
        assert!(!result.is_empty());

        // The last message should be the last assistant text.
        let last = result.last().unwrap();
        assert_eq!(last.role, genai::chat::ChatRole::Assistant);
    }

    #[test]
    fn extract_carryover_does_not_split_tool_call_pairs() {
        use genai::chat::ChatRole;

        let tool_call = ToolCall {
            call_id: "c1".to_string(),
            fn_name: "file_read".to_string(),
            fn_arguments: serde_json::json!({"path": "test.txt"}),
            thought_signatures: None,
        };

        let messages = vec![
            ChatMessage::system("system prompt"),
            ChatMessage::assistant("First text response."),
            ChatMessage::from(vec![tool_call]),
            ToolResponse::new("c1", "file contents").into(),
            ChatMessage::assistant("Got the file."),
        ];

        // Request 1 turn -- should include from the assistant text before tools
        // through to the final "Got the file." response.
        let result = extract_carryover(&messages, 1);
        assert!(!result.is_empty());

        // The result should contain the tool response (not split from its call).
        let has_tool = result.iter().any(|m| m.role == ChatRole::Tool);
        let has_assistant_tool_call = result
            .iter()
            .any(|m| m.role == ChatRole::Assistant && !m.content.tool_calls().is_empty());

        // If tool messages are in carryover, their assistant call must also be present.
        if has_tool {
            assert!(
                has_assistant_tool_call,
                "Tool response in carryover without its tool call"
            );
        }
    }
}
