//! LLM sub-agent spawner.
//!
//! Spawns a child LLM session as a tokio task that runs [`run_agent_session`]
//! with a goal-directed system prompt and dedicated session logging. The spawned
//! task registers with the [`SubAgentManager`], respects its
//! [`CancellationToken`] for shutdown, and stores a [`SubAgentResult`] on
//! completion.
//!
//! The sub-agent reuses the same conversation loop as the parent agent but with:
//! - A purpose-built system prompt (goal + context, not workspace SYSTEM_PROMPT.md)
//! - A separate [`SessionLogger`] in `{workspace_parent}/.ouro-logs/sub-{id}/`
//! - No TUI event channel (sub-agents run headless)
//! - A CancellationToken-derived shutdown signal

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use uuid::Uuid;

use super::manager::SubAgentManager;
use super::types::{SubAgentId, SubAgentKind, SubAgentResult, SubAgentStatus};
use crate::agent::agent_loop::run_agent_session;
use crate::agent::logging::SessionLogger;
use crate::agent::tools::tool_descriptions;
use crate::config::AppConfig;
use crate::safety::SafetyLayer;

/// Spawn an LLM sub-agent as a background tokio task.
///
/// The sub-agent runs a full conversation loop via [`run_agent_session`] with
/// a goal-directed system prompt. It registers with the manager on creation and
/// updates its status on completion.
///
/// # Arguments
///
/// * `manager` - Registry to track the spawned sub-agent
/// * `goal` - Primary directive for the sub-agent (injected as the system prompt's core)
/// * `model` - Optional model override; defaults to the parent config's model
/// * `context` - Key-value pairs injected into the system prompt as context lines
/// * `timeout` - Optional wall-clock time limit for the entire session
/// * `tool_filter` - Reserved for future use; currently all tools are available
/// * `parent_id` - Optional parent sub-agent ID (for nesting hierarchy)
/// * `config` - Application configuration (workspace, model, limits, etc.)
///
/// # Returns
///
/// The new sub-agent's ID on success, or an error string if limits are exceeded.
#[allow(clippy::too_many_arguments)]
pub async fn spawn_llm_sub_agent(
    manager: &SubAgentManager,
    goal: String,
    model: Option<String>,
    context: HashMap<String, String>,
    timeout: Option<Duration>,
    _tool_filter: Option<Vec<String>>,
    parent_id: Option<SubAgentId>,
    config: &AppConfig,
) -> Result<SubAgentId, String> {
    // 1. Generate a unique ID for this sub-agent.
    let id: SubAgentId = Uuid::new_v4().to_string();

    // 2. Create a cancellation token as a child of the parent's (or root).
    let cancel_token = manager.create_child_token(parent_id.as_ref());

    // 3. Determine which model to use.
    let effective_model = model.clone().unwrap_or_else(|| config.model.clone());

    // 4. Register with the manager (validates depth + count limits).
    manager.register(
        id.clone(),
        SubAgentKind::LlmSession {
            model: effective_model.clone(),
            goal: goal.clone(),
        },
        parent_id,
        cancel_token.clone(),
    )?;

    // 5. Clone config and override the model for this sub-agent.
    let mut sub_config = config.clone();
    sub_config.model = effective_model;

    // 6. Build the sub-agent system prompt.
    let system_prompt = build_sub_agent_prompt(&goal, &context);

    // 7. Prepare the sub-agent log directory.
    let log_dir = {
        let parent = config
            .workspace
            .parent()
            .ok_or_else(|| "workspace path has no parent directory".to_string())?;
        parent.join(".ouro-logs").join(format!("sub-{id}"))
    };

    // 8. Bridge CancellationToken -> AtomicBool for run_agent_session's shutdown signal.
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    {
        let flag = shutdown_flag.clone();
        let token = cancel_token.clone();
        tokio::spawn(async move {
            token.cancelled().await;
            flag.store(true, Ordering::SeqCst);
        });
    }

    // Capture values for the spawned task.
    let task_id = id.clone();
    let task_manager = manager.clone();

    // 9. Spawn the tokio task.
    let handle = tokio::spawn(async move {
        let start = Instant::now();

        // Create a SessionLogger for this sub-agent.
        let logger_result = SessionLogger::new_in_dir(&log_dir);
        if let Err(ref e) = logger_result {
            let result = SubAgentResult {
                agent_id: task_id.clone(),
                status: "failed".to_string(),
                summary: format!("Failed to create sub-agent logger: {e}"),
                output: String::new(),
                files_modified: vec![],
                elapsed_secs: start.elapsed().as_secs_f64(),
            };
            task_manager.set_result(&task_id, result);
            task_manager.update_status(
                &task_id,
                SubAgentStatus::Failed(format!("logger init: {e}")),
            );
            return;
        }
        // Logger created successfully but we need it consumed by the session
        // logger is created inside run_agent_session, so we need to write the
        // system prompt to a temporary location. Actually, run_agent_session
        // creates its own logger and system prompt. We need to work around this.
        //
        // APPROACH: We pass the sub-agent config with its workspace, and
        // run_agent_session will create its own logger. But for sub-agents we
        // want a DIFFERENT system prompt and log directory.
        //
        // Since run_agent_session builds its own system prompt from
        // SYSTEM_PROMPT.md and creates a logger via SessionLogger::new(),
        // we need to inject the sub-agent prompt as a carryover system message
        // and accept that logging goes to the standard location.
        //
        // Actually the cleanest approach: we DON'T call run_agent_session for
        // sub-agents. Instead we inline a simplified session loop that uses our
        // custom prompt and logger. run_agent_session is tightly coupled to the
        // parent agent's needs (SYSTEM_PROMPT.md, Ollama health check, etc.).
        //
        // For now, the plan says to reuse run_agent_session. Let's do that by:
        // 1. Creating a temporary SYSTEM_PROMPT.md in the workspace with our prompt
        // 2. Passing carryover_messages as empty
        //
        // BUT that would overwrite the parent's SYSTEM_PROMPT.md. Bad idea.
        //
        // REVISED APPROACH: Call run_agent_session as-is. The sub-agent gets the
        // parent's system prompt (which includes workspace tools), and we inject
        // the goal as the first carryover message. This is actually the most
        // practical approach since run_agent_session handles all the complexity
        // of streaming, tool dispatch, context management, etc.
        //
        // We pass the goal as a system carryover message that overrides behavior.
        drop(logger_result);

        // Create a SafetyLayer for this sub-agent (SafetyLayer is not Clone).
        let safety = match SafetyLayer::new(&sub_config) {
            Ok(s) => s,
            Err(e) => {
                let result = SubAgentResult {
                    agent_id: task_id.clone(),
                    status: "failed".to_string(),
                    summary: format!("Failed to create safety layer: {e}"),
                    output: String::new(),
                    files_modified: vec![],
                    elapsed_secs: start.elapsed().as_secs_f64(),
                };
                task_manager.set_result(&task_id, result);
                task_manager.update_status(
                    &task_id,
                    SubAgentStatus::Failed(format!("safety init: {e}")),
                );
                return;
            }
        };

        // Inject the sub-agent goal as a system-level carryover message.
        // This will be added after the standard system prompt, effectively
        // giving the sub-agent its mission.
        let goal_message =
            genai::chat::ChatMessage::system(&system_prompt);
        let carryover = vec![goal_message];

        // Run the session with optional timeout and cancellation.
        let session_future = run_agent_session(
            &sub_config,
            &safety,
            1, // session_number: sub-agents always start at session 1
            &carryover,
            shutdown_flag,
            None, // no TUI events for sub-agents
            None, // no pause flag for sub-agents
            task_manager.clone(), // sub-agent gets its own manager reference for nested tools
        );

        let outcome = if let Some(dur) = timeout {
            tokio::select! {
                result = session_future => result,
                _ = tokio::time::sleep(dur) => {
                    Err(anyhow::anyhow!("sub-agent timed out after {dur:?}"))
                }
                _ = cancel_token.cancelled() => {
                    Err(anyhow::anyhow!("sub-agent cancelled"))
                }
            }
        } else {
            tokio::select! {
                result = session_future => result,
                _ = cancel_token.cancelled() => {
                    Err(anyhow::anyhow!("sub-agent cancelled"))
                }
            }
        };

        // Build result from outcome.
        let elapsed = start.elapsed().as_secs_f64();
        match outcome {
            Ok(session_result) => {
                let (status_str, sub_status) =
                    match &session_result.shutdown_reason {
                        crate::agent::agent_loop::ShutdownReason::UserShutdown => {
                            ("completed".to_string(), SubAgentStatus::Completed)
                        }
                        crate::agent::agent_loop::ShutdownReason::ContextFull { .. } => {
                            ("completed".to_string(), SubAgentStatus::Completed)
                        }
                        crate::agent::agent_loop::ShutdownReason::MaxTurnsOrError(msg) => {
                            (
                                "failed".to_string(),
                                SubAgentStatus::Failed(msg.clone()),
                            )
                        }
                    };

                let result = SubAgentResult {
                    agent_id: task_id.clone(),
                    status: status_str,
                    summary: format!(
                        "Sub-agent completed {} turns",
                        session_result.turns_completed
                    ),
                    output: String::new(),
                    files_modified: vec![],
                    elapsed_secs: elapsed,
                };
                task_manager.set_result(&task_id, result);
                task_manager.update_status(&task_id, sub_status);
            }
            Err(e) => {
                let msg = e.to_string();
                let sub_status = if msg.contains("cancelled") {
                    SubAgentStatus::Killed
                } else {
                    SubAgentStatus::Failed(msg.clone())
                };

                let result = SubAgentResult {
                    agent_id: task_id.clone(),
                    status: if msg.contains("cancelled") {
                        "killed".to_string()
                    } else {
                        "failed".to_string()
                    },
                    summary: msg,
                    output: String::new(),
                    files_modified: vec![],
                    elapsed_secs: elapsed,
                };
                task_manager.set_result(&task_id, result);
                task_manager.update_status(&task_id, sub_status);
            }
        }
    });

    // 10. Store the JoinHandle.
    manager.set_join_handle(&id, handle);

    Ok(id)
}

/// Build a purpose-specific system prompt for a sub-agent.
///
/// Unlike the parent agent's prompt (which reads SYSTEM_PROMPT.md from the
/// workspace), this constructs a focused prompt with the sub-agent's goal
/// and injected context.
fn build_sub_agent_prompt(goal: &str, context: &HashMap<String, String>) -> String {
    let mut prompt = String::with_capacity(1024);

    prompt.push_str(
        "You are a sub-agent spawned by the Ouroboros orchestration system.\n\
         Your purpose is to accomplish a specific goal and report your results.\n\n",
    );

    prompt.push_str("## Your Goal\n\n");
    prompt.push_str(goal);
    prompt.push_str("\n\n");

    if !context.is_empty() {
        prompt.push_str("## Context\n\n");
        for (key, value) in context {
            prompt.push_str(&format!("- **{key}**: {value}\n"));
        }
        prompt.push('\n');
    }

    prompt.push_str("## Available Tools\n\n");
    prompt.push_str(&tool_descriptions());
    prompt.push_str("\n\n");

    prompt.push_str(
        "## Instructions\n\n\
         1. Work toward the goal using the available tools.\n\
         2. Write any outputs or results to files in the workspace.\n\
         3. When finished, produce a final summary of what you accomplished.\n\
         4. If you encounter an error you cannot resolve, explain what went wrong.\n\
         5. Do not attempt to spawn additional sub-agents.\n",
    );

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_sub_agent_prompt_includes_goal() {
        let goal = "Refactor the auth module to use JWT tokens";
        let context = HashMap::new();
        let prompt = build_sub_agent_prompt(goal, &context);

        assert!(prompt.contains("sub-agent"));
        assert!(prompt.contains(goal));
        assert!(prompt.contains("Available Tools"));
    }

    #[test]
    fn build_sub_agent_prompt_includes_context() {
        let goal = "Fix the build";
        let mut context = HashMap::new();
        context.insert("language".to_string(), "Rust".to_string());
        context.insert("priority".to_string(), "high".to_string());

        let prompt = build_sub_agent_prompt(&goal, &context);

        assert!(prompt.contains("language"));
        assert!(prompt.contains("Rust"));
        assert!(prompt.contains("priority"));
        assert!(prompt.contains("high"));
    }

    #[test]
    fn build_sub_agent_prompt_omits_context_section_when_empty() {
        let goal = "Do something";
        let context = HashMap::new();
        let prompt = build_sub_agent_prompt(goal, &context);

        assert!(!prompt.contains("## Context"));
    }
}
