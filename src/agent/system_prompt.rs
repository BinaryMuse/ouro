//! System prompt loading and harness context wrapping.
//!
//! Loads the user's `SYSTEM_PROMPT.md` from the workspace directory and wraps
//! it with harness context (model name, workspace path, available tools,
//! constraints). The combined prompt is used as the system message for the
//! LLM conversation.
//!
//! The system prompt is re-read from disk on every session start. This supports
//! the Ouroboros self-modification philosophy: the agent may have modified
//! `SYSTEM_PROMPT.md` during a previous session, and those changes should take
//! effect on restart.

use std::path::Path;

use crate::error::AgentError;

/// Build the full system prompt by loading `SYSTEM_PROMPT.md` from the
/// workspace and wrapping it with harness-injected context.
///
/// The resulting prompt has this structure:
/// 1. Harness preamble (role, environment, tools, constraints)
/// 2. Session continuity section (if session_number > 1)
/// 3. Separator
/// 4. User's system prompt content from `SYSTEM_PROMPT.md`
///
/// The prompt is always re-read from disk (never cached) so that agent
/// modifications to `SYSTEM_PROMPT.md` are picked up on restart.
///
/// # Arguments
///
/// * `workspace` - Path to the workspace directory containing `SYSTEM_PROMPT.md`
/// * `model` - Model identifier (e.g., "qwen2.5:7b") shown to the agent
/// * `tool_descriptions` - Pre-formatted human-readable tool listing
/// * `session_number` - 1-based session number; values > 1 add a continuity section
///
/// # Errors
///
/// Returns [`AgentError::SystemPromptNotFound`] if `SYSTEM_PROMPT.md` does not
/// exist in the workspace directory.
pub async fn build_system_prompt(
    workspace: &Path,
    model: &str,
    tool_descriptions: &str,
    session_number: u32,
) -> Result<String, AgentError> {
    let prompt_path = workspace.join("SYSTEM_PROMPT.md");

    // Always re-read from disk -- the agent may have modified this file.
    let user_content = tokio::fs::read_to_string(&prompt_path)
        .await
        .map_err(|_| AgentError::SystemPromptNotFound {
            path: prompt_path.clone(),
        })?;

    let workspace_display = workspace.display();

    let session_continuity = if session_number > 1 {
        format!(
            "\n\n## Session Continuity\n\
             This is session #{session_number}. You have been restarted due to context window limits.\n\
             Your SYSTEM_PROMPT.md is reloaded from disk each restart -- if you modified it, your changes are active.\n\
             Check your workspace for any state files you wrote in previous sessions."
        )
    } else {
        String::new()
    };

    Ok(format!(
        "\
You are an autonomous AI agent running in the Ouroboros research harness.

## Environment
- Model: {model}
- Workspace: {workspace_display} (you own this directory)
- Shell commands execute in the workspace directory

## Available Tools
{tool_descriptions}

## Constraints
- File writes are restricted to the workspace directory
- Shell commands are filtered against a security blocklist
- Shell commands have a configurable timeout
- Read access is unrestricted\
{session_continuity}

## Your System Prompt
The following is your system prompt, provided by your operator:

---

{user_content}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn build_system_prompt_includes_harness_context_and_user_content() {
        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path().join("workspace");
        tokio::fs::create_dir_all(&workspace).await.unwrap();

        let user_prompt = "You are a helpful coding assistant.\nFocus on Rust code.";
        tokio::fs::write(workspace.join("SYSTEM_PROMPT.md"), user_prompt)
            .await
            .unwrap();

        let tool_desc = "- shell_exec: Execute a shell command\n- file_read: Read a file";
        let result = build_system_prompt(&workspace, "qwen2.5:7b", tool_desc, 1)
            .await
            .unwrap();

        // Harness context present
        assert!(result.contains("Ouroboros research harness"));
        assert!(result.contains("qwen2.5:7b"));
        assert!(result.contains(&workspace.display().to_string()));
        assert!(result.contains("shell_exec: Execute a shell command"));
        assert!(result.contains("file_read: Read a file"));

        // Constraints present
        assert!(result.contains("File writes are restricted to the workspace directory"));
        assert!(result.contains("Shell commands are filtered against a security blocklist"));
        assert!(result.contains("Read access is unrestricted"));

        // User content present
        assert!(result.contains("You are a helpful coding assistant."));
        assert!(result.contains("Focus on Rust code."));

        // Structure: harness context comes before user content
        let harness_pos = result.find("Ouroboros research harness").unwrap();
        let user_pos = result.find("You are a helpful coding assistant.").unwrap();
        assert!(harness_pos < user_pos);

        // Session 1: no continuity section
        assert!(!result.contains("Session Continuity"));
    }

    #[tokio::test]
    async fn build_system_prompt_returns_error_when_file_missing() {
        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path().join("workspace");
        tokio::fs::create_dir_all(&workspace).await.unwrap();

        let result = build_system_prompt(&workspace, "test-model", "tools", 1).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        match &err {
            AgentError::SystemPromptNotFound { path } => {
                assert!(path.ends_with("SYSTEM_PROMPT.md"));
            }
            other => panic!("Expected SystemPromptNotFound, got: {other}"),
        }
    }

    #[tokio::test]
    async fn build_system_prompt_includes_continuity_on_restart() {
        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path().join("workspace");
        tokio::fs::create_dir_all(&workspace).await.unwrap();

        tokio::fs::write(workspace.join("SYSTEM_PROMPT.md"), "My prompt.")
            .await
            .unwrap();

        let result = build_system_prompt(&workspace, "test-model", "tools", 3)
            .await
            .unwrap();

        // Session continuity section present
        assert!(result.contains("Session Continuity"));
        assert!(result.contains("session #3"));
        assert!(result.contains("restarted due to context window limits"));
        assert!(result.contains("SYSTEM_PROMPT.md is reloaded from disk"));
        assert!(result.contains("Check your workspace"));

        // User content still present
        assert!(result.contains("My prompt."));
    }
}
