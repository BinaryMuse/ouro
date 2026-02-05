//! Sub-agent orchestration subsystem.
//!
//! Provides the [`manager::SubAgentManager`] registry for tracking spawned
//! sub-agents and background processes, along with shared [`types`] used
//! across the orchestration layer.

pub mod llm_agent;
pub mod manager;
pub mod types;
