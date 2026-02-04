//! Application state accumulator for the TUI.
//!
//! [`AppState`] is the single source of truth for all TUI-visible state.
//! Agent events are applied via [`AppState::apply_event`] which pushes log
//! entries and updates counters/status fields. Each render frame reads from
//! `AppState` to produce the UI (immediate-mode rendering).

use super::event::{AgentEvent, AgentState};

/// Categorizes log entries for color-coding and icon selection during rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogEntryKind {
    /// Agent thinking/reasoning text.
    Thought,
    /// A tool call was initiated.
    ToolCall,
    /// A tool call completed with a result.
    ToolResult,
    /// An error occurred.
    Error,
    /// Visual separator for session restarts.
    SessionSeparator,
    /// System-level message (startup, shutdown, etc.).
    System,
}

/// A single entry in the TUI log stream.
///
/// This is a TUI-local type, distinct from the JSONL [`crate::agent::logging::LogEntry`]
/// used for persistent session replay. The TUI log entry carries display-oriented
/// fields (summary vs. full content, expanded state) rather than serialization tags.
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// Wall-clock timestamp for display (e.g., "14:32:07").
    pub timestamp: String,
    /// Entry classification for rendering.
    pub kind: LogEntryKind,
    /// One-line summary always visible in the log stream.
    pub summary: String,
    /// Full content visible when the entry is expanded.
    pub full_content: String,
    /// Whether this entry is currently expanded to show full content.
    pub expanded: bool,
}

/// All TUI-visible state, accumulated from agent events.
///
/// The TUI render loop reads from this struct every frame. Agent events
/// mutate it via [`AppState::apply_event`]. User input mutates it via
/// scroll/tab/toggle methods.
pub struct AppState {
    // -- Log stream --
    /// Ordered list of log entries (newest at end).
    pub log_entries: Vec<LogEntry>,

    // -- Discoveries --
    /// Discovered items: (timestamp, content).
    pub discoveries: Vec<(String, String)>,

    // -- Status bar fields --
    /// Current agent state (Thinking/Executing/Idle/Paused).
    pub agent_state: AgentState,
    /// Context window usage as a fraction (0.0 to 1.0).
    pub context_usage_pct: f64,
    /// Number of prompt tokens used in the current context.
    pub prompt_tokens: usize,
    /// Total context window limit in tokens.
    pub context_limit: usize,
    /// Current session number (1-based).
    pub session_number: u32,
    /// Current turn count within the session.
    pub turn_count: u64,
    /// Total tool calls executed in this session.
    pub tool_call_count: u64,

    // -- Navigation state --
    /// Index of the active tab (0 = Agent, 1 = Discoveries).
    pub active_tab: usize,
    /// Scroll offset into the log entry list.
    pub log_scroll_offset: usize,
    /// When true, new log entries auto-scroll the view to the bottom.
    pub auto_scroll: bool,

    // -- Panel visibility --
    /// Whether the sub-agent tree panel is visible on the Agent tab.
    pub sub_agent_panel_visible: bool,

    // -- Quit confirmation --
    /// True after the first 'q' press; a second 'q' confirms quit.
    pub quit_pending: bool,
}

impl AppState {
    /// Create a new `AppState` with sensible defaults.
    pub fn new() -> Self {
        Self {
            log_entries: Vec::new(),
            discoveries: Vec::new(),
            agent_state: AgentState::Idle,
            context_usage_pct: 0.0,
            prompt_tokens: 0,
            context_limit: 0,
            session_number: 1,
            turn_count: 0,
            tool_call_count: 0,
            active_tab: 0,
            log_scroll_offset: 0,
            auto_scroll: true,
            sub_agent_panel_visible: true,
            quit_pending: false,
        }
    }

    /// Apply an agent event, updating log entries, counters, and status fields.
    ///
    /// This is the sole mutation path for agent-originated state changes.
    /// Each event variant maps to one or more field updates.
    pub fn apply_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::ThoughtText {
                timestamp,
                turn: _,
                content,
            } => {
                self.log_entries.push(LogEntry {
                    timestamp,
                    kind: LogEntryKind::Thought,
                    summary: first_line_or_truncate(&content, 120),
                    full_content: content,
                    expanded: true,
                });
                self.auto_scroll_to_bottom();
            }

            AgentEvent::ToolCallStarted {
                timestamp,
                turn: _,
                call_id: _,
                fn_name,
                args_summary,
            } => {
                let summary = format!("{fn_name}({args_summary})");
                self.log_entries.push(LogEntry {
                    timestamp,
                    kind: LogEntryKind::ToolCall,
                    summary,
                    full_content: String::new(),
                    expanded: false,
                });
                self.auto_scroll_to_bottom();
            }

            AgentEvent::ToolCallCompleted {
                timestamp,
                turn: _,
                call_id: _,
                fn_name,
                result_summary: _,
                full_result,
            } => {
                let line_count = full_result.lines().count();
                let summary = format!("{fn_name}: {line_count} lines of output");
                self.log_entries.push(LogEntry {
                    timestamp,
                    kind: LogEntryKind::ToolResult,
                    summary,
                    full_content: full_result,
                    expanded: false,
                });
                self.auto_scroll_to_bottom();
            }

            AgentEvent::StateChanged(state) => {
                self.agent_state = state;
            }

            AgentEvent::ContextPressure {
                usage_pct,
                prompt_tokens,
                context_limit,
            } => {
                self.context_usage_pct = usage_pct;
                self.prompt_tokens = prompt_tokens;
                self.context_limit = context_limit;
            }

            AgentEvent::SessionRestarted { session_number } => {
                self.session_number = session_number;
                self.log_entries.push(LogEntry {
                    timestamp: String::new(),
                    kind: LogEntryKind::SessionSeparator,
                    summary: format!("--- Session {session_number} started ---"),
                    full_content: String::new(),
                    expanded: false,
                });
                self.auto_scroll_to_bottom();
            }

            AgentEvent::Error {
                timestamp,
                turn: _,
                message,
            } => {
                self.log_entries.push(LogEntry {
                    timestamp,
                    kind: LogEntryKind::Error,
                    summary: first_line_or_truncate(&message, 120),
                    full_content: message,
                    expanded: true,
                });
                self.auto_scroll_to_bottom();
            }

            AgentEvent::Discovery { timestamp, content } => {
                self.discoveries.push((timestamp, content));
            }

            AgentEvent::CountersUpdated { turn, tool_calls } => {
                self.turn_count = turn;
                self.tool_call_count = tool_calls;
            }
        }
    }

    /// Toggle the expanded state of a log entry by index.
    ///
    /// No-op if `index` is out of bounds.
    pub fn toggle_expand(&mut self, index: usize) {
        if let Some(entry) = self.log_entries.get_mut(index) {
            entry.expanded = !entry.expanded;
        }
    }

    /// Scroll the log view up by one entry.
    ///
    /// Disables auto-scroll so the user can read history without being
    /// yanked back to the bottom on each new event.
    pub fn scroll_up(&mut self) {
        self.log_scroll_offset = self.log_scroll_offset.saturating_sub(1);
        self.auto_scroll = false;
    }

    /// Scroll the log view down by one entry.
    pub fn scroll_down(&mut self) {
        self.log_scroll_offset = self
            .log_scroll_offset
            .saturating_add(1)
            .min(self.log_entries.len().saturating_sub(1));
    }

    /// Jump to the bottom of the log and re-enable auto-scroll.
    pub fn jump_to_bottom(&mut self) {
        self.log_scroll_offset = self.log_entries.len().saturating_sub(1);
        self.auto_scroll = true;
    }

    /// If auto-scroll is enabled, move the scroll offset to the latest entry.
    fn auto_scroll_to_bottom(&mut self) {
        if self.auto_scroll {
            self.log_scroll_offset = self.log_entries.len().saturating_sub(1);
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract the first line of `text`, truncating to `max_len` characters if needed.
fn first_line_or_truncate(text: &str, max_len: usize) -> String {
    let first_line = text.lines().next().unwrap_or("");
    if first_line.len() > max_len {
        format!("{}...", &first_line[..max_len])
    } else {
        first_line.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::event::{AgentEvent, AgentState};

    #[test]
    fn new_state_has_correct_defaults() {
        let state = AppState::new();
        assert_eq!(state.agent_state, AgentState::Idle);
        assert!(state.auto_scroll);
        assert!(state.sub_agent_panel_visible);
        assert_eq!(state.active_tab, 0);
        assert_eq!(state.session_number, 1);
        assert_eq!(state.turn_count, 0);
        assert_eq!(state.tool_call_count, 0);
        assert!(state.log_entries.is_empty());
        assert!(state.discoveries.is_empty());
        assert!(!state.quit_pending);
    }

    #[test]
    fn apply_thought_pushes_expanded_entry() {
        let mut state = AppState::new();
        state.apply_event(AgentEvent::ThoughtText {
            timestamp: "14:32:07".into(),
            turn: 1,
            content: "I should read the config file first.".into(),
        });

        assert_eq!(state.log_entries.len(), 1);
        let entry = &state.log_entries[0];
        assert_eq!(entry.kind, LogEntryKind::Thought);
        assert!(entry.expanded);
        assert_eq!(entry.timestamp, "14:32:07");
    }

    #[test]
    fn apply_tool_call_started_pushes_collapsed_entry() {
        let mut state = AppState::new();
        state.apply_event(AgentEvent::ToolCallStarted {
            timestamp: "14:32:08".into(),
            turn: 1,
            call_id: "call_001".into(),
            fn_name: "shell_exec".into(),
            args_summary: "ls -la".into(),
        });

        assert_eq!(state.log_entries.len(), 1);
        let entry = &state.log_entries[0];
        assert_eq!(entry.kind, LogEntryKind::ToolCall);
        assert!(!entry.expanded);
        assert_eq!(entry.summary, "shell_exec(ls -la)");
    }

    #[test]
    fn apply_tool_call_completed_counts_lines() {
        let mut state = AppState::new();
        let full_result = "line1\nline2\nline3\nline4\nline5".to_string();
        state.apply_event(AgentEvent::ToolCallCompleted {
            timestamp: "14:32:09".into(),
            turn: 1,
            call_id: "call_001".into(),
            fn_name: "shell_exec".into(),
            result_summary: "ok".into(),
            full_result: full_result.clone(),
        });

        assert_eq!(state.log_entries.len(), 1);
        let entry = &state.log_entries[0];
        assert_eq!(entry.kind, LogEntryKind::ToolResult);
        assert!(!entry.expanded);
        assert_eq!(entry.summary, "shell_exec: 5 lines of output");
        assert_eq!(entry.full_content, full_result);
    }

    #[test]
    fn apply_state_changed_updates_agent_state() {
        let mut state = AppState::new();
        assert_eq!(state.agent_state, AgentState::Idle);

        state.apply_event(AgentEvent::StateChanged(AgentState::Thinking));
        assert_eq!(state.agent_state, AgentState::Thinking);

        state.apply_event(AgentEvent::StateChanged(AgentState::Executing));
        assert_eq!(state.agent_state, AgentState::Executing);

        state.apply_event(AgentEvent::StateChanged(AgentState::Paused));
        assert_eq!(state.agent_state, AgentState::Paused);
    }

    #[test]
    fn apply_context_pressure_updates_fields() {
        let mut state = AppState::new();
        state.apply_event(AgentEvent::ContextPressure {
            usage_pct: 0.73,
            prompt_tokens: 3000,
            context_limit: 4096,
        });

        assert!((state.context_usage_pct - 0.73).abs() < f64::EPSILON);
        assert_eq!(state.prompt_tokens, 3000);
        assert_eq!(state.context_limit, 4096);
    }

    #[test]
    fn apply_session_restarted_pushes_separator() {
        let mut state = AppState::new();
        state.apply_event(AgentEvent::SessionRestarted { session_number: 3 });

        assert_eq!(state.session_number, 3);
        assert_eq!(state.log_entries.len(), 1);
        let entry = &state.log_entries[0];
        assert_eq!(entry.kind, LogEntryKind::SessionSeparator);
        assert_eq!(entry.summary, "--- Session 3 started ---");
    }

    #[test]
    fn apply_error_pushes_expanded_entry() {
        let mut state = AppState::new();
        state.apply_event(AgentEvent::Error {
            timestamp: "14:33:00".into(),
            turn: 5,
            message: "Connection refused".into(),
        });

        assert_eq!(state.log_entries.len(), 1);
        let entry = &state.log_entries[0];
        assert_eq!(entry.kind, LogEntryKind::Error);
        assert!(entry.expanded);
        assert_eq!(entry.full_content, "Connection refused");
    }

    #[test]
    fn apply_discovery_appends_to_list() {
        let mut state = AppState::new();
        state.apply_event(AgentEvent::Discovery {
            timestamp: "14:34:00".into(),
            content: "Found a Makefile in the project root".into(),
        });

        assert_eq!(state.discoveries.len(), 1);
        assert_eq!(state.discoveries[0].1, "Found a Makefile in the project root");
    }

    #[test]
    fn apply_counters_updated() {
        let mut state = AppState::new();
        state.apply_event(AgentEvent::CountersUpdated {
            turn: 12,
            tool_calls: 47,
        });

        assert_eq!(state.turn_count, 12);
        assert_eq!(state.tool_call_count, 47);
    }

    #[test]
    fn toggle_expand_flips_state() {
        let mut state = AppState::new();
        state.apply_event(AgentEvent::ToolCallStarted {
            timestamp: "t".into(),
            turn: 1,
            call_id: "c".into(),
            fn_name: "f".into(),
            args_summary: "a".into(),
        });

        assert!(!state.log_entries[0].expanded);
        state.toggle_expand(0);
        assert!(state.log_entries[0].expanded);
        state.toggle_expand(0);
        assert!(!state.log_entries[0].expanded);
    }

    #[test]
    fn toggle_expand_out_of_bounds_is_noop() {
        let mut state = AppState::new();
        state.toggle_expand(99); // no panic
    }

    #[test]
    fn scroll_up_disables_auto_scroll() {
        let mut state = AppState::new();
        // Push a few entries to have something to scroll
        for i in 0..5 {
            state.apply_event(AgentEvent::ThoughtText {
                timestamp: format!("t{i}"),
                turn: i as u64,
                content: format!("entry {i}"),
            });
        }

        assert!(state.auto_scroll);
        state.scroll_up();
        assert!(!state.auto_scroll);
    }

    #[test]
    fn jump_to_bottom_re_enables_auto_scroll() {
        let mut state = AppState::new();
        for i in 0..5 {
            state.apply_event(AgentEvent::ThoughtText {
                timestamp: format!("t{i}"),
                turn: i as u64,
                content: format!("entry {i}"),
            });
        }

        state.scroll_up();
        assert!(!state.auto_scroll);

        state.jump_to_bottom();
        assert!(state.auto_scroll);
        assert_eq!(state.log_scroll_offset, 4); // last index
    }

    #[test]
    fn scroll_down_clamps_to_last_entry() {
        let mut state = AppState::new();
        state.apply_event(AgentEvent::ThoughtText {
            timestamp: "t".into(),
            turn: 1,
            content: "only entry".into(),
        });

        state.scroll_down();
        state.scroll_down();
        state.scroll_down();
        // Should be clamped, not panicking
        assert_eq!(state.log_scroll_offset, 0);
    }

    #[test]
    fn auto_scroll_moves_offset_on_new_entries() {
        let mut state = AppState::new();
        assert_eq!(state.log_scroll_offset, 0);

        state.apply_event(AgentEvent::ThoughtText {
            timestamp: "t1".into(),
            turn: 1,
            content: "first".into(),
        });
        assert_eq!(state.log_scroll_offset, 0); // first entry is index 0

        state.apply_event(AgentEvent::ThoughtText {
            timestamp: "t2".into(),
            turn: 2,
            content: "second".into(),
        });
        assert_eq!(state.log_scroll_offset, 1);

        state.apply_event(AgentEvent::ThoughtText {
            timestamp: "t3".into(),
            turn: 3,
            content: "third".into(),
        });
        assert_eq!(state.log_scroll_offset, 2);
    }

    #[test]
    fn auto_scroll_disabled_does_not_move_offset() {
        let mut state = AppState::new();
        state.apply_event(AgentEvent::ThoughtText {
            timestamp: "t1".into(),
            turn: 1,
            content: "first".into(),
        });

        state.scroll_up(); // disables auto_scroll, offset stays at 0
        let offset_before = state.log_scroll_offset;

        state.apply_event(AgentEvent::ThoughtText {
            timestamp: "t2".into(),
            turn: 2,
            content: "second".into(),
        });

        assert_eq!(state.log_scroll_offset, offset_before);
    }

    #[test]
    fn agent_state_display() {
        assert_eq!(format!("{}", AgentState::Thinking), "Thinking");
        assert_eq!(format!("{}", AgentState::Executing), "Executing");
        assert_eq!(format!("{}", AgentState::Idle), "Idle");
        assert_eq!(format!("{}", AgentState::Paused), "Paused");
    }

    #[test]
    fn first_line_truncation() {
        let mut state = AppState::new();
        let long_content = "x".repeat(200);
        state.apply_event(AgentEvent::ThoughtText {
            timestamp: "t".into(),
            turn: 1,
            content: long_content,
        });

        let entry = &state.log_entries[0];
        // Summary should be truncated to 120 chars + "..."
        assert!(entry.summary.len() <= 123);
        assert!(entry.summary.ends_with("..."));
    }
}
