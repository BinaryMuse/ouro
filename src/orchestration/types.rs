//! Type definitions for the sub-agent orchestration subsystem.
//!
//! These types form the shared vocabulary between the [`super::manager::SubAgentManager`],
//! tool dispatch (spawn/status/kill tools), and the TUI sub-agent panel.
//! All types derive [`serde::Serialize`] for JSON tool responses.

use serde::{Deserialize, Serialize};

/// Unique identifier for a sub-agent or background process.
///
/// Uses UUID v4 strings for collision-free IDs that are readable in logs and tool output.
pub type SubAgentId = String;

/// Classifies what kind of work a sub-agent entry represents.
#[derive(Clone, Debug, Serialize)]
pub enum SubAgentKind {
    /// A child LLM chat session with its own conversation and tools.
    LlmSession {
        /// The Ollama model used for this session.
        model: String,
        /// The goal/task description given to the sub-agent.
        goal: String,
    },

    /// A long-lived background shell process with optional stdin interaction.
    BackgroundProcess {
        /// The shell command that was spawned.
        command: String,
    },
}

/// Lifecycle status of a sub-agent or background process.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum SubAgentStatus {
    /// Currently executing.
    Running,
    /// Finished successfully.
    Completed,
    /// Terminated with an error.
    Failed(String),
    /// Explicitly cancelled via kill_agent or cascading shutdown.
    Killed,
}

/// Read-only view of a sub-agent entry, returned by status queries.
///
/// This is a snapshot -- the actual entry may change after this clone is returned.
/// Cheap to clone since all fields are small strings/enums.
#[derive(Clone, Debug, Serialize)]
pub struct SubAgentInfo {
    /// Unique identifier (UUID v4 string).
    pub id: SubAgentId,
    /// What kind of work this entry represents.
    pub kind: SubAgentKind,
    /// Parent sub-agent ID, or `None` for root-level entries.
    pub parent_id: Option<SubAgentId>,
    /// Current lifecycle status.
    pub status: SubAgentStatus,
    /// Nesting depth in the sub-agent tree (root = 0).
    pub depth: usize,
    /// ISO 8601 timestamp when the entry was registered.
    pub spawned_at: String,
    /// ISO 8601 timestamp when the entry completed/failed/was killed.
    pub completed_at: Option<String>,
}

/// Structured result returned when a sub-agent completes its work.
///
/// Captured by the manager and retrievable via the `agent_result` tool.
/// Derives both Serialize and Deserialize since sub-agents produce this
/// as their final output and the parent consumes it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubAgentResult {
    /// ID of the sub-agent that produced this result.
    pub agent_id: SubAgentId,
    /// Outcome: "completed" or "failed".
    pub status: String,
    /// Brief human-readable summary of what was accomplished.
    pub summary: String,
    /// Detailed output or error message.
    pub output: String,
    /// Files created or modified during execution.
    pub files_modified: Vec<String>,
    /// Wall-clock duration of execution in seconds.
    pub elapsed_secs: f64,
}
