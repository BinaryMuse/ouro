//! Core agent conversation loop with Ollama health check, streaming,
//! tool dispatch, and graceful shutdown handling.
//!
//! This is the capstone module that brings together the session logger, system
//! prompt, tool definitions, and the genai client into a working infinite
//! conversation loop. The loop:
//!
//! 1. Validates Ollama connectivity and model availability
//! 2. Loads the system prompt and tool schemas
//! 3. Streams model text to stdout in real time
//! 4. Dispatches tool calls through the safety layer
//! 5. Logs all events to a JSONL session file
//! 6. Handles Ctrl+C for graceful shutdown

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use genai::chat::{
    ChatMessage, ChatOptions, ChatRequest, ChatStreamEvent, ToolCall, ToolResponse,
};
use genai::Client;

use crate::agent::logging::{LogEntry, SessionLogger};
use crate::agent::system_prompt::build_system_prompt;
use crate::agent::tools::{define_tools, dispatch_tool_call, tool_descriptions};
use crate::config::AppConfig;
use crate::error::AgentError;
use crate::safety::SafetyLayer;

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

/// Run the core agent conversation loop.
///
/// This function blocks until the user sends Ctrl+C or the estimated context
/// window fills up. It validates Ollama connectivity, loads the system prompt,
/// and enters an infinite loop that streams model text to stdout, dispatches
/// tool calls, and logs all events to a JSONL session file.
///
/// # Arguments
///
/// * `config` - Resolved application configuration (model, workspace, limits)
/// * `safety` - Safety layer for command filtering and workspace enforcement
pub async fn run_agent_loop(config: &AppConfig, safety: &SafetyLayer) -> anyhow::Result<()> {
    // -- Startup: validate Ollama and model
    check_ollama_ready(&config.model).await?;

    // -- Create session logger
    let mut logger = SessionLogger::new(&config.workspace)?;

    // -- Build system prompt with harness context
    let system_prompt = build_system_prompt(
        &config.workspace,
        &config.model,
        &tool_descriptions(),
    )
    .await?;

    // -- Create genai client (defaults to Ollama for non-prefixed model names)
    let client = Client::default();

    // -- Build initial chat request with system prompt and tools
    let mut chat_req = ChatRequest::from_system(&system_prompt).with_tools(define_tools());

    // -- Configure streaming capture options
    let chat_options = ChatOptions::default()
        .with_capture_content(true)
        .with_capture_tool_calls(true);

    // -- Log session start
    logger.log_session_start(&config.model, &config.workspace)?;

    // -- Print startup info to stderr (not stdout, which is for model output)
    eprintln!(
        "Ouroboros agent started.\n  Model: {}\n  Workspace: {}\n  Log: {}",
        config.model,
        config.workspace.display(),
        logger.log_path().display(),
    );

    // -- Set up two-phase Ctrl+C shutdown
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    tokio::spawn(async move {
        // First Ctrl+C: set graceful shutdown flag.
        tokio::signal::ctrl_c().await.ok();
        shutdown_clone.store(true, Ordering::SeqCst);
        eprintln!(
            "\nShutting down after current turn... (Ctrl+C again to force quit)"
        );

        // Second Ctrl+C: force exit.
        tokio::signal::ctrl_c().await.ok();
        eprintln!("\nForce quitting.");
        std::process::exit(1);
    });

    // -- Main loop state
    let mut turn: u64 = 0;
    let mut total_chars: usize = system_prompt.len();
    let shutdown_reason;

    loop {
        // Check shutdown flag between turns.
        if shutdown.load(Ordering::SeqCst) {
            shutdown_reason = "user_shutdown";
            break;
        }

        turn += 1;

        // -- Stream model response
        let stream_res = match client
            .exec_chat_stream(&config.model, chat_req.clone(), Some(&chat_options))
            .await
        {
            Ok(res) => res,
            Err(e) => {
                let msg = format!("LLM stream error: {e}");
                eprintln!("[error] {msg}");
                logger.log_event(&LogEntry::Error {
                    timestamp: chrono::Utc::now()
                        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                        .to_string(),
                    turn,
                    message: msg,
                })?;
                shutdown_reason = "error";
                break;
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
            total_chars += text.len();
            logger.log_event(&LogEntry::AssistantText {
                timestamp: chrono::Utc::now()
                    .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                    .to_string(),
                turn,
                content: text.clone(),
            })?;
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
                    timestamp: chrono::Utc::now()
                        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                        .to_string(),
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
                    args_summary
                };
                eprintln!("[tool] {}({})", call.fn_name, args_display);

                // Dispatch tool call through safety layer
                let result =
                    dispatch_tool_call(call, safety, &config.workspace).await;

                // Log tool result
                logger.log_event(&LogEntry::ToolResult {
                    timestamp: chrono::Utc::now()
                        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                        .to_string(),
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

                // Track character count for context estimation
                total_chars += result.len();

                // Append tool response to conversation
                chat_req = chat_req.append_message(ToolResponse::new(
                    call_id.clone(),
                    result,
                ));
            }
        }

        // -- Context full detection (heuristic: 1 token ~ 4 chars)
        let estimated_tokens = total_chars / 4;
        if estimated_tokens > config.context_limit {
            eprintln!(
                "\n[warning] Context window estimated full after {turn} turns. \
                 Restart the session."
            );
            logger.log_event(&LogEntry::SystemMessage {
                timestamp: chrono::Utc::now()
                    .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                    .to_string(),
                content: format!(
                    "Context window estimated full (~{estimated_tokens} tokens) \
                     after {turn} turns"
                ),
            })?;
            shutdown_reason = "context_full";
            break;
        }
    }

    // -- Log session end
    logger.log_session_end(turn, shutdown_reason)?;

    eprintln!(
        "Session ended: {shutdown_reason}. {turn} turns completed. Log: {}",
        logger.log_path().display(),
    );

    Ok(())
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
}
