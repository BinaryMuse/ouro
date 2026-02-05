//! Central registry for sub-agents and background processes.
//!
//! [`SubAgentManager`] is the single source of truth for all spawned sub-agents
//! and background processes. It wraps a `HashMap` behind `Arc<Mutex<..>>` for
//! thread-safe access from the agent loop, tool dispatch, and TUI.
//!
//! **Concurrency model:** `Arc<Mutex<HashMap>>` is chosen over `DashMap` to avoid
//! an extra dependency. Contention is negligible -- the registry is accessed
//! infrequently (spawn, status query, shutdown) with <20 concurrent agents.
//!
//! **Cancellation model:** Each entry holds a [`CancellationToken`] created as a
//! child of its parent's token (or the root token for top-level agents). Cancelling
//! the root token cascades shutdown to all entries.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use tokio::task::JoinHandle;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use super::types::{SubAgentId, SubAgentInfo, SubAgentKind, SubAgentResult, SubAgentStatus};
use crate::tui::event::AgentEvent;

/// Internal entry stored in the registry. Not exposed publicly -- callers
/// see [`SubAgentInfo`] snapshots via `get_status` / `list_all` / etc.
struct SubAgentEntry {
    /// The read-only view returned by status queries.
    info: SubAgentInfo,
    /// Cancellation token for this entry (child of parent's or root token).
    cancel_token: CancellationToken,
    /// JoinHandle for the spawned tokio task, retained for cleanup.
    join_handle: Option<JoinHandle<()>>,
    /// Structured result set when the agent completes.
    result: Option<SubAgentResult>,
    /// Stdin handle for background processes (taken on first write).
    stdin_handle: Option<tokio::process::ChildStdin>,
    /// Ring buffer of captured stdout/stderr lines for background processes.
    output_buffer: Option<Arc<Mutex<VecDeque<String>>>>,
}

/// Central registry for all sub-agents and background processes.
///
/// The manager is designed to be wrapped in `Arc` for shared ownership across
/// the agent loop, tool dispatch, and TUI. All fields are already behind `Arc`
/// or are `Clone`, so the struct itself derives `Clone`.
///
/// # Example
///
/// ```ignore
/// let token = CancellationToken::new();
/// let manager = SubAgentManager::new(token, None, 3, 10);
/// // Pass manager.clone() to agent loop, tool dispatch, TUI...
/// ```
#[derive(Clone)]
pub struct SubAgentManager {
    entries: Arc<Mutex<HashMap<SubAgentId, SubAgentEntry>>>,
    root_cancel_token: CancellationToken,
    event_tx: Option<UnboundedSender<AgentEvent>>,
    max_depth: usize,
    max_total: usize,
}

#[allow(dead_code)]
impl SubAgentManager {
    /// Create a new manager with the given cancellation root and limits.
    ///
    /// - `root_cancel_token`: Top-level token; cancelling it shuts down all agents.
    /// - `event_tx`: Optional channel for TUI event emission.
    /// - `max_depth`: Maximum nesting depth (root = 0). Default recommendation: 3.
    /// - `max_total`: Maximum total registered entries. Default recommendation: 10.
    pub fn new(
        root_cancel_token: CancellationToken,
        event_tx: Option<UnboundedSender<AgentEvent>>,
        max_depth: usize,
        max_total: usize,
    ) -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
            root_cancel_token,
            event_tx,
            max_depth,
            max_total,
        }
    }

    /// Register a new sub-agent or background process.
    ///
    /// Validates depth (by walking the parent chain) and total count limits
    /// before inserting. Returns `Err` with a descriptive message if limits
    /// are exceeded or if the `id` is already registered.
    pub fn register(
        &self,
        id: SubAgentId,
        kind: SubAgentKind,
        parent_id: Option<SubAgentId>,
        cancel_token: CancellationToken,
    ) -> Result<(), String> {
        let mut entries = self.entries.lock().unwrap();

        // Check total count limit
        if entries.len() >= self.max_total {
            return Err(format!(
                "max total agents reached ({}/{})",
                entries.len(),
                self.max_total
            ));
        }

        // Check for duplicate ID
        if entries.contains_key(&id) {
            return Err(format!("agent id already registered: {id}"));
        }

        // Calculate depth by walking parent chain
        let depth = if let Some(ref pid) = parent_id {
            match entries.get(pid) {
                Some(parent) => parent.info.depth + 1,
                None => return Err(format!("parent agent not found: {pid}")),
            }
        } else {
            0
        };

        // Check depth limit
        if depth > self.max_depth {
            return Err(format!(
                "max nesting depth exceeded ({depth} > {})",
                self.max_depth
            ));
        }

        let now = Utc::now().to_rfc3339();

        let info = SubAgentInfo {
            id: id.clone(),
            kind,
            parent_id,
            status: SubAgentStatus::Running,
            depth,
            spawned_at: now,
            completed_at: None,
        };

        entries.insert(
            id,
            SubAgentEntry {
                info,
                cancel_token,
                join_handle: None,
                result: None,
                stdin_handle: None,
                output_buffer: None,
            },
        );

        Ok(())
    }

    /// Attach a JoinHandle for later cleanup/await.
    pub fn set_join_handle(&self, id: &SubAgentId, handle: JoinHandle<()>) {
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.get_mut(id) {
            entry.join_handle = Some(handle);
        }
    }

    /// Attach a stdin handle for a background process.
    pub fn set_stdin(&self, id: &SubAgentId, stdin: tokio::process::ChildStdin) {
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.get_mut(id) {
            entry.stdin_handle = Some(stdin);
        }
    }

    /// Attach an output ring buffer for a background process.
    pub fn set_output_buffer(&self, id: &SubAgentId, buf: Arc<Mutex<VecDeque<String>>>) {
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.get_mut(id) {
            entry.output_buffer = Some(buf);
        }
    }

    /// Update the lifecycle status of a registered entry.
    ///
    /// Automatically sets `completed_at` when the status transitions to a
    /// terminal state (Completed, Failed, Killed). Emits a
    /// [`AgentEvent::SubAgentStatusChanged`] via the event channel if available.
    pub fn update_status(&self, id: &SubAgentId, status: SubAgentStatus) {
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.get_mut(id) {
            // Set completed_at for terminal states
            match &status {
                SubAgentStatus::Completed
                | SubAgentStatus::Failed(_)
                | SubAgentStatus::Killed => {
                    entry.info.completed_at = Some(Utc::now().to_rfc3339());
                }
                SubAgentStatus::Running => {}
            }

            let status_str = match &status {
                SubAgentStatus::Running => "running".to_string(),
                SubAgentStatus::Completed => "completed".to_string(),
                SubAgentStatus::Failed(msg) => format!("failed: {msg}"),
                SubAgentStatus::Killed => "killed".to_string(),
            };

            let kind_str = match &entry.info.kind {
                SubAgentKind::LlmSession { model, .. } => format!("llm:{model}"),
                SubAgentKind::BackgroundProcess { command } => {
                    format!("proc:{}", truncate_str(command, 30))
                }
            };

            entry.info.status = status;

            // Emit TUI event if channel is available
            if let Some(tx) = &self.event_tx {
                let _ = tx.send(AgentEvent::SubAgentStatusChanged {
                    agent_id: id.clone(),
                    status: status_str,
                    kind: kind_str,
                });
            }
        }
    }

    /// Store the structured result for a completed agent.
    pub fn set_result(&self, id: &SubAgentId, result: SubAgentResult) {
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.get_mut(id) {
            entry.result = Some(result);
        }
    }

    /// Get a snapshot of a sub-agent's info. Returns `None` if not found.
    pub fn get_status(&self, id: &SubAgentId) -> Option<SubAgentInfo> {
        let entries = self.entries.lock().unwrap();
        entries.get(id).map(|e| e.info.clone())
    }

    /// Get the structured result for a completed agent. Returns `None` if
    /// not found or not yet completed.
    pub fn get_result(&self, id: &SubAgentId) -> Option<SubAgentResult> {
        let entries = self.entries.lock().unwrap();
        entries.get(id).and_then(|e| e.result.clone())
    }

    /// Return info snapshots for all registered entries.
    pub fn list_all(&self) -> Vec<SubAgentInfo> {
        let entries = self.entries.lock().unwrap();
        entries.values().map(|e| e.info.clone()).collect()
    }

    /// Return info snapshots for children of a given parent.
    pub fn children_of(&self, parent_id: &SubAgentId) -> Vec<SubAgentInfo> {
        let entries = self.entries.lock().unwrap();
        entries
            .values()
            .filter(|e| e.info.parent_id.as_ref() == Some(parent_id))
            .map(|e| e.info.clone())
            .collect()
    }

    /// Return info snapshots for root-level entries (no parent).
    pub fn root_agents(&self) -> Vec<SubAgentInfo> {
        let entries = self.entries.lock().unwrap();
        entries
            .values()
            .filter(|e| e.info.parent_id.is_none())
            .map(|e| e.info.clone())
            .collect()
    }

    /// Cancel a specific agent by cancelling its token and setting status to Killed.
    ///
    /// Returns `true` if the agent was found (regardless of prior status).
    pub fn cancel_agent(&self, id: &SubAgentId) -> bool {
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.get_mut(id) {
            entry.cancel_token.cancel();
            entry.info.status = SubAgentStatus::Killed;
            entry.info.completed_at = Some(Utc::now().to_rfc3339());
            true
        } else {
            false
        }
    }

    /// Shut down all agents: cancel the root token, then await all JoinHandles
    /// with a per-handle timeout of 5 seconds.
    pub async fn shutdown_all(&self) {
        // Cancel root token -- cascades to all children
        self.root_cancel_token.cancel();

        // Collect JoinHandles (take them out of entries)
        let handles: Vec<JoinHandle<()>> = {
            let mut entries = self.entries.lock().unwrap();
            entries
                .values_mut()
                .filter_map(|e| e.join_handle.take())
                .collect()
        };

        // Await each handle with a 5-second timeout
        for handle in handles {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), handle).await;
        }

        // Mark all remaining as Killed
        {
            let mut entries = self.entries.lock().unwrap();
            let now = Utc::now().to_rfc3339();
            for entry in entries.values_mut() {
                if entry.info.status == SubAgentStatus::Running {
                    entry.info.status = SubAgentStatus::Killed;
                    entry.info.completed_at = Some(now.clone());
                }
            }
        }
    }

    /// Create a child cancellation token for a new sub-agent.
    ///
    /// If `parent_id` is `Some` and found, the token is a child of that agent's
    /// token. Otherwise, it is a child of the root token.
    pub fn create_child_token(&self, parent_id: Option<&SubAgentId>) -> CancellationToken {
        if let Some(pid) = parent_id {
            let entries = self.entries.lock().unwrap();
            if let Some(entry) = entries.get(pid) {
                return entry.cancel_token.child_token();
            }
        }
        self.root_cancel_token.child_token()
    }

    /// Take (remove) the stdin handle for a background process.
    ///
    /// Returns `None` if the entry is not found or stdin was already taken.
    pub fn take_stdin(&self, id: &SubAgentId) -> Option<tokio::process::ChildStdin> {
        let mut entries = self.entries.lock().unwrap();
        entries.get_mut(id).and_then(|e| e.stdin_handle.take())
    }

    /// Write data to a background process's stdin without consuming the handle.
    ///
    /// Temporarily takes the `ChildStdin` handle, writes the data, and puts
    /// the handle back. This allows multiple writes to the same process.
    ///
    /// Returns the number of bytes written on success, or an error string.
    pub async fn write_to_stdin(&self, id: &SubAgentId, data: &[u8]) -> Result<usize, String> {
        // Take the stdin handle out of the entry (requires brief lock).
        let stdin_opt = {
            let mut entries = self.entries.lock().unwrap();
            entries.get_mut(id).and_then(|e| e.stdin_handle.take())
        };

        let mut stdin = stdin_opt.ok_or_else(|| {
            "No stdin handle available (process not found or stdin already closed)".to_string()
        })?;

        // Write data outside the lock to avoid holding it during async I/O.
        use tokio::io::AsyncWriteExt;
        let write_result = stdin.write_all(data).await;

        // Put the handle back regardless of write outcome.
        {
            let mut entries = self.entries.lock().unwrap();
            if let Some(entry) = entries.get_mut(id) {
                entry.stdin_handle = Some(stdin);
            }
            // If the entry was removed while we were writing, the handle is dropped.
        }

        match write_result {
            Ok(()) => Ok(data.len()),
            Err(e) => Err(format!("Failed to write to stdin: {e}")),
        }
    }

    /// Read the last N lines from a background process's output buffer.
    ///
    /// Returns `None` if the entry is not found or has no output buffer.
    pub fn read_output(&self, id: &SubAgentId, tail_lines: usize) -> Option<Vec<String>> {
        let entries = self.entries.lock().unwrap();
        entries.get(id).and_then(|e| {
            e.output_buffer.as_ref().map(|buf| {
                let buf = buf.lock().unwrap();
                let len = buf.len();
                let start = len.saturating_sub(tail_lines);
                buf.iter().skip(start).cloned().collect()
            })
        })
    }

    /// Return the total number of registered entries.
    pub fn total_count(&self) -> usize {
        self.entries.lock().unwrap().len()
    }

    /// Return the nesting depth of a specific entry, or `None` if not found.
    pub fn depth_of(&self, id: &SubAgentId) -> Option<usize> {
        let entries = self.entries.lock().unwrap();
        entries.get(id).map(|e| e.info.depth)
    }
}

/// Truncate a string to `max_len` characters, appending "..." if truncated.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}...", &s[..max_len])
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a manager with standard test limits.
    fn test_manager() -> SubAgentManager {
        let token = CancellationToken::new();
        SubAgentManager::new(token, None, 3, 10)
    }

    #[test]
    fn register_succeeds_within_limits() {
        let mgr = test_manager();
        let token = mgr.create_child_token(None);
        let result = mgr.register(
            "agent-1".to_string(),
            SubAgentKind::LlmSession {
                model: "qwen2.5:3b".into(),
                goal: "test".into(),
            },
            None,
            token,
        );
        assert!(result.is_ok());
        assert_eq!(mgr.total_count(), 1);
    }

    #[test]
    fn register_fails_when_max_total_exceeded() {
        let token = CancellationToken::new();
        let mgr = SubAgentManager::new(token, None, 3, 2); // max 2

        for i in 0..2 {
            let ct = mgr.create_child_token(None);
            mgr.register(
                format!("agent-{i}"),
                SubAgentKind::BackgroundProcess {
                    command: "sleep 10".into(),
                },
                None,
                ct,
            )
            .unwrap();
        }

        let ct = mgr.create_child_token(None);
        let result = mgr.register(
            "agent-overflow".to_string(),
            SubAgentKind::BackgroundProcess {
                command: "sleep 10".into(),
            },
            None,
            ct,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("max total"));
    }

    #[test]
    fn register_fails_when_max_depth_exceeded() {
        let token = CancellationToken::new();
        let mgr = SubAgentManager::new(token, None, 1, 10); // max depth 1

        // Register root (depth 0)
        let ct = mgr.create_child_token(None);
        mgr.register(
            "root".to_string(),
            SubAgentKind::LlmSession {
                model: "m".into(),
                goal: "g".into(),
            },
            None,
            ct,
        )
        .unwrap();

        // Register child (depth 1) -- should succeed
        let ct = mgr.create_child_token(Some(&"root".to_string()));
        mgr.register(
            "child".to_string(),
            SubAgentKind::LlmSession {
                model: "m".into(),
                goal: "g".into(),
            },
            Some("root".to_string()),
            ct,
        )
        .unwrap();

        // Register grandchild (depth 2) -- should fail (max_depth=1)
        let ct = mgr.create_child_token(Some(&"child".to_string()));
        let result = mgr.register(
            "grandchild".to_string(),
            SubAgentKind::LlmSession {
                model: "m".into(),
                goal: "g".into(),
            },
            Some("child".to_string()),
            ct,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("depth"));
    }

    #[test]
    fn update_status_changes_info() {
        let mgr = test_manager();
        let ct = mgr.create_child_token(None);
        mgr.register(
            "a1".to_string(),
            SubAgentKind::BackgroundProcess {
                command: "echo hi".into(),
            },
            None,
            ct,
        )
        .unwrap();

        assert_eq!(
            mgr.get_status(&"a1".to_string()).unwrap().status,
            SubAgentStatus::Running
        );

        mgr.update_status(&"a1".to_string(), SubAgentStatus::Completed);

        let info = mgr.get_status(&"a1".to_string()).unwrap();
        assert_eq!(info.status, SubAgentStatus::Completed);
        assert!(info.completed_at.is_some());
    }

    #[test]
    fn list_all_returns_all_registered() {
        let mgr = test_manager();
        for i in 0..3 {
            let ct = mgr.create_child_token(None);
            mgr.register(
                format!("a{i}"),
                SubAgentKind::BackgroundProcess {
                    command: "true".into(),
                },
                None,
                ct,
            )
            .unwrap();
        }

        let all = mgr.list_all();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn children_of_returns_correct_children() {
        let mgr = test_manager();

        // Register parent
        let ct = mgr.create_child_token(None);
        mgr.register(
            "parent".to_string(),
            SubAgentKind::LlmSession {
                model: "m".into(),
                goal: "g".into(),
            },
            None,
            ct,
        )
        .unwrap();

        // Register two children of parent
        for i in 0..2 {
            let ct = mgr.create_child_token(Some(&"parent".to_string()));
            mgr.register(
                format!("child-{i}"),
                SubAgentKind::LlmSession {
                    model: "m".into(),
                    goal: "g".into(),
                },
                Some("parent".to_string()),
                ct,
            )
            .unwrap();
        }

        // Register an unrelated root agent
        let ct = mgr.create_child_token(None);
        mgr.register(
            "other".to_string(),
            SubAgentKind::BackgroundProcess {
                command: "true".into(),
            },
            None,
            ct,
        )
        .unwrap();

        let children = mgr.children_of(&"parent".to_string());
        assert_eq!(children.len(), 2);
        for child in &children {
            assert_eq!(child.parent_id.as_deref(), Some("parent"));
        }
    }

    #[test]
    fn root_agents_returns_only_parentless_entries() {
        let mgr = test_manager();

        // Two root agents
        for i in 0..2 {
            let ct = mgr.create_child_token(None);
            mgr.register(
                format!("root-{i}"),
                SubAgentKind::BackgroundProcess {
                    command: "true".into(),
                },
                None,
                ct,
            )
            .unwrap();
        }

        // One child
        let ct = mgr.create_child_token(Some(&"root-0".to_string()));
        mgr.register(
            "child".to_string(),
            SubAgentKind::LlmSession {
                model: "m".into(),
                goal: "g".into(),
            },
            Some("root-0".to_string()),
            ct,
        )
        .unwrap();

        let roots = mgr.root_agents();
        assert_eq!(roots.len(), 2);
        for root in &roots {
            assert!(root.parent_id.is_none());
        }
    }

    #[test]
    fn cancel_agent_sets_status_to_killed() {
        let mgr = test_manager();
        let ct = mgr.create_child_token(None);
        mgr.register(
            "victim".to_string(),
            SubAgentKind::BackgroundProcess {
                command: "sleep 999".into(),
            },
            None,
            ct,
        )
        .unwrap();

        assert!(mgr.cancel_agent(&"victim".to_string()));

        let info = mgr.get_status(&"victim".to_string()).unwrap();
        assert_eq!(info.status, SubAgentStatus::Killed);
        assert!(info.completed_at.is_some());
    }

    #[test]
    fn cancel_nonexistent_returns_false() {
        let mgr = test_manager();
        assert!(!mgr.cancel_agent(&"ghost".to_string()));
    }

    #[test]
    fn create_child_token_creates_proper_hierarchy() {
        let root_token = CancellationToken::new();
        let mgr = SubAgentManager::new(root_token.clone(), None, 3, 10);

        // Register a parent agent
        let parent_ct = mgr.create_child_token(None);
        mgr.register(
            "parent".to_string(),
            SubAgentKind::LlmSession {
                model: "m".into(),
                goal: "g".into(),
            },
            None,
            parent_ct,
        )
        .unwrap();

        // Create a child token under the parent
        let child_ct = mgr.create_child_token(Some(&"parent".to_string()));

        // Cancelling parent should NOT cancel child's token (only root cancel cascades through parent)
        // But cancelling root should cascade to both
        assert!(!child_ct.is_cancelled());
        root_token.cancel();
        assert!(child_ct.is_cancelled());
    }

    #[test]
    fn depth_of_returns_correct_depth() {
        let mgr = test_manager();

        let ct = mgr.create_child_token(None);
        mgr.register(
            "root".to_string(),
            SubAgentKind::LlmSession {
                model: "m".into(),
                goal: "g".into(),
            },
            None,
            ct,
        )
        .unwrap();

        let ct = mgr.create_child_token(Some(&"root".to_string()));
        mgr.register(
            "child".to_string(),
            SubAgentKind::LlmSession {
                model: "m".into(),
                goal: "g".into(),
            },
            Some("root".to_string()),
            ct,
        )
        .unwrap();

        assert_eq!(mgr.depth_of(&"root".to_string()), Some(0));
        assert_eq!(mgr.depth_of(&"child".to_string()), Some(1));
        assert_eq!(mgr.depth_of(&"missing".to_string()), None);
    }

    #[test]
    fn set_and_get_result() {
        let mgr = test_manager();
        let ct = mgr.create_child_token(None);
        mgr.register(
            "a1".to_string(),
            SubAgentKind::LlmSession {
                model: "m".into(),
                goal: "g".into(),
            },
            None,
            ct,
        )
        .unwrap();

        assert!(mgr.get_result(&"a1".to_string()).is_none());

        let result = SubAgentResult {
            agent_id: "a1".to_string(),
            status: "completed".to_string(),
            summary: "did the thing".to_string(),
            output: "detailed output".to_string(),
            files_modified: vec!["foo.rs".to_string()],
            elapsed_secs: 3.14,
        };
        mgr.set_result(&"a1".to_string(), result.clone());

        let got = mgr.get_result(&"a1".to_string()).unwrap();
        assert_eq!(got.agent_id, "a1");
        assert_eq!(got.summary, "did the thing");
        assert_eq!(got.files_modified, vec!["foo.rs"]);
    }

    #[test]
    fn output_buffer_read_returns_tail_lines() {
        let mgr = test_manager();
        let ct = mgr.create_child_token(None);
        mgr.register(
            "proc".to_string(),
            SubAgentKind::BackgroundProcess {
                command: "echo".into(),
            },
            None,
            ct,
        )
        .unwrap();

        let buf = Arc::new(Mutex::new(VecDeque::new()));
        {
            let mut b = buf.lock().unwrap();
            for i in 0..10 {
                b.push_back(format!("line {i}"));
            }
        }
        mgr.set_output_buffer(&"proc".to_string(), buf);

        let lines = mgr.read_output(&"proc".to_string(), 3).unwrap();
        assert_eq!(lines, vec!["line 7", "line 8", "line 9"]);
    }

    #[test]
    fn register_rejects_duplicate_id() {
        let mgr = test_manager();
        let ct = mgr.create_child_token(None);
        mgr.register(
            "dup".to_string(),
            SubAgentKind::BackgroundProcess {
                command: "true".into(),
            },
            None,
            ct,
        )
        .unwrap();

        let ct = mgr.create_child_token(None);
        let result = mgr.register(
            "dup".to_string(),
            SubAgentKind::BackgroundProcess {
                command: "true".into(),
            },
            None,
            ct,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already registered"));
    }
}
