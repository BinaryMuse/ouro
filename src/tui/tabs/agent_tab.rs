//! Agent tab rendering (Tab 1).
//!
//! Displays the log stream as the primary content, with an optional
//! sub-agent tree panel at the bottom showing a hierarchical view of
//! spawned sub-agents and background processes.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Text;
use ratatui::widgets::{Block, Borders, Paragraph, StatefulWidget, Widget};
use tui_tree_widget::{Tree, TreeItem, TreeState};

use crate::orchestration::types::{SubAgentInfo, SubAgentKind, SubAgentStatus};
use crate::tui::app_state::AppState;
use crate::tui::widgets::log_stream;

/// Render the Agent tab into the given area.
///
/// Layout:
/// - If `sub_agent_panel_visible`: 70% log stream (top), 30% sub-agent panel (bottom)
/// - Otherwise: log stream takes the full area
pub fn render_agent_tab(state: &AppState, area: Rect, buf: &mut Buffer) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    if state.sub_agent_panel_visible {
        let chunks = Layout::vertical([
            Constraint::Percentage(70),
            Constraint::Percentage(30),
        ])
        .split(area);

        // Log stream in top portion
        log_stream::render_log_entries(
            &state.log_entries,
            state.log_scroll_offset,
            chunks[0],
            buf,
        );

        // Sub-agent tree panel in bottom portion
        render_sub_agent_panel(&state.sub_agent_entries, chunks[1], buf);
    } else {
        // Log stream takes full area
        log_stream::render_log_entries(
            &state.log_entries,
            state.log_scroll_offset,
            area,
            buf,
        );
    }
}

/// Render the sub-agent tree panel.
///
/// Shows a hierarchical tree of all sub-agents and background processes,
/// or a "(No sub-agents running)" message when the list is empty.
fn render_sub_agent_panel(entries: &[SubAgentInfo], area: Rect, buf: &mut Buffer) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Sub-Agents ");

    let inner = block.inner(area);
    block.render(area, buf);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    if entries.is_empty() {
        let placeholder = Paragraph::new("(No sub-agents running)")
            .style(Style::default().fg(Color::DarkGray));
        placeholder.render(inner, buf);
        return;
    }

    // Build tree items from the flat list of entries.
    let tree_items = build_sub_agent_tree_items(entries);

    if let Ok(tree) = Tree::new(&tree_items) {
        let mut tree_state = TreeState::default();
        // Open all nodes by default so the full hierarchy is visible.
        for entry in entries {
            if entry.parent_id.is_none() {
                tree_state.open(vec![entry.id.clone()]);
            }
        }
        StatefulWidget::render(tree, inner, buf, &mut tree_state);
    }
}

/// Build a hierarchical list of [`TreeItem`]s from the flat sub-agent entries.
///
/// Root entries (`parent_id == None`) become top-level items. Their children
/// are nested underneath. Each item shows a status icon, kind label,
/// description, and status text.
fn build_sub_agent_tree_items(entries: &[SubAgentInfo]) -> Vec<TreeItem<'static, String>> {
    // Collect root-level entries (no parent).
    let roots: Vec<&SubAgentInfo> = entries.iter().filter(|e| e.parent_id.is_none()).collect();

    roots
        .iter()
        .map(|root| build_tree_item(root, entries))
        .collect()
}

/// Recursively build a [`TreeItem`] for a single entry and its children.
fn build_tree_item(entry: &SubAgentInfo, all_entries: &[SubAgentInfo]) -> TreeItem<'static, String> {
    let label = format_entry_label(entry);
    let style = status_style(&entry.status);
    let text = Text::styled(label, style);

    // Find children of this entry.
    let children: Vec<TreeItem<'static, String>> = all_entries
        .iter()
        .filter(|e| e.parent_id.as_ref() == Some(&entry.id))
        .map(|child| build_tree_item(child, all_entries))
        .collect();

    if children.is_empty() {
        TreeItem::new_leaf(entry.id.clone(), text)
    } else {
        TreeItem::new(entry.id.clone(), text, children).unwrap_or_else(|_| {
            // Fallback if duplicate IDs (should not happen, but be safe).
            TreeItem::new_leaf(entry.id.clone(), Text::raw("(error)"))
        })
    }
}

/// Format a human-readable label for a sub-agent entry.
///
/// Format: `{status_icon} {kind_label}: {description} ({status})`
fn format_entry_label(entry: &SubAgentInfo) -> String {
    let icon = status_icon(&entry.status);
    let kind_label = kind_label(&entry.kind);
    let description = kind_description(&entry.kind);
    let status_text = status_text(&entry.status);

    format!("{icon} {kind_label}: {description} ({status_text})")
}

/// Return a text icon for the given status.
fn status_icon(status: &SubAgentStatus) -> &'static str {
    match status {
        SubAgentStatus::Running => "[*]",
        SubAgentStatus::Completed => "[+]",
        SubAgentStatus::Failed(_) => "[!]",
        SubAgentStatus::Killed => "[x]",
    }
}

/// Return a short kind label.
fn kind_label(kind: &SubAgentKind) -> &'static str {
    match kind {
        SubAgentKind::LlmSession { .. } => "LLM",
        SubAgentKind::BackgroundProcess { .. } => "Proc",
    }
}

/// Return a description string for the kind (goal or command, truncated).
fn kind_description(kind: &SubAgentKind) -> String {
    match kind {
        SubAgentKind::LlmSession { goal, .. } => truncate(goal, 40),
        SubAgentKind::BackgroundProcess { command } => truncate(command, 40),
    }
}

/// Return a human-readable status text.
fn status_text(status: &SubAgentStatus) -> String {
    match status {
        SubAgentStatus::Running => "running".to_string(),
        SubAgentStatus::Completed => "completed".to_string(),
        SubAgentStatus::Failed(msg) => format!("failed: {}", truncate(msg, 30)),
        SubAgentStatus::Killed => "killed".to_string(),
    }
}

/// Return a style colored by status.
fn status_style(status: &SubAgentStatus) -> Style {
    match status {
        SubAgentStatus::Running => Style::default().fg(Color::Yellow),
        SubAgentStatus::Completed => Style::default().fg(Color::Green),
        SubAgentStatus::Failed(_) => Style::default().fg(Color::Red),
        SubAgentStatus::Killed => Style::default().fg(Color::DarkGray),
    }
}

/// Truncate a string to `max_len` characters, appending "..." if truncated.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}...", &s[..max_len])
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_agent_tab_empty_state_no_panic() {
        let state = AppState::new();
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render_agent_tab(&state, area, &mut buf);
    }

    #[test]
    fn render_agent_tab_zero_area_no_panic() {
        let state = AppState::new();
        let mut buf = Buffer::empty(Rect::ZERO);
        render_agent_tab(&state, Rect::ZERO, &mut buf);
    }

    #[test]
    fn render_agent_tab_with_empty_sub_agents_shows_no_sub_agents() {
        let state = AppState::new();
        // sub_agent_panel_visible defaults to true, entries empty
        assert!(state.sub_agent_panel_visible);
        assert!(state.sub_agent_entries.is_empty());
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render_agent_tab(&state, area, &mut buf);

        let content: String = buf.content().iter().map(|c| c.symbol().to_string()).collect();
        assert!(content.contains("Sub-Agents"));
        assert!(content.contains("No sub-agents running"));
    }

    #[test]
    fn render_agent_tab_without_sub_agent_panel() {
        let mut state = AppState::new();
        state.sub_agent_panel_visible = false;
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render_agent_tab(&state, area, &mut buf);

        let content: String = buf.content().iter().map(|c| c.symbol().to_string()).collect();
        // Should not contain sub-agent panel
        assert!(!content.contains("Sub-Agents"));
        // Should still have log block
        assert!(content.contains("Log"));
    }

    #[test]
    fn render_agent_tab_with_sub_agent_entries_shows_tree() {
        let mut state = AppState::new();
        state.sub_agent_entries = vec![
            SubAgentInfo {
                id: "agent-1".to_string(),
                kind: SubAgentKind::LlmSession {
                    model: "qwen2.5:3b".to_string(),
                    goal: "Fix the build".to_string(),
                },
                parent_id: None,
                status: SubAgentStatus::Running,
                depth: 0,
                spawned_at: "2025-01-01T00:00:00Z".to_string(),
                completed_at: None,
            },
            SubAgentInfo {
                id: "agent-2".to_string(),
                kind: SubAgentKind::BackgroundProcess {
                    command: "cargo watch -x check".to_string(),
                },
                parent_id: None,
                status: SubAgentStatus::Completed,
                depth: 0,
                spawned_at: "2025-01-01T00:00:00Z".to_string(),
                completed_at: Some("2025-01-01T00:01:00Z".to_string()),
            },
        ];

        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render_agent_tab(&state, area, &mut buf);

        let content: String = buf.content().iter().map(|c| c.symbol().to_string()).collect();
        assert!(content.contains("Sub-Agents"));
        // Should show the tree content, not the placeholder
        assert!(!content.contains("No sub-agents running"));
        // Should contain status icons and kind labels
        assert!(content.contains("[*]"));   // Running icon
        assert!(content.contains("LLM"));   // Kind label
    }

    #[test]
    fn render_sub_agent_panel_empty_entries() {
        let area = Rect::new(0, 0, 40, 5);
        let mut buf = Buffer::empty(area);
        render_sub_agent_panel(&[], area, &mut buf);
        let content: String = buf.content().iter().map(|c| c.symbol().to_string()).collect();
        assert!(content.contains("No sub-agents running"));
    }

    #[test]
    fn build_tree_items_groups_by_parent() {
        let entries = vec![
            SubAgentInfo {
                id: "root-1".to_string(),
                kind: SubAgentKind::LlmSession {
                    model: "m".to_string(),
                    goal: "root goal".to_string(),
                },
                parent_id: None,
                status: SubAgentStatus::Running,
                depth: 0,
                spawned_at: String::new(),
                completed_at: None,
            },
            SubAgentInfo {
                id: "child-1".to_string(),
                kind: SubAgentKind::BackgroundProcess {
                    command: "sleep 10".to_string(),
                },
                parent_id: Some("root-1".to_string()),
                status: SubAgentStatus::Running,
                depth: 1,
                spawned_at: String::new(),
                completed_at: None,
            },
        ];

        let items = build_sub_agent_tree_items(&entries);
        assert_eq!(items.len(), 1); // One root item
        assert_eq!(items[0].children().len(), 1); // With one child
    }

    #[test]
    fn format_entry_label_llm_running() {
        let entry = SubAgentInfo {
            id: "a1".to_string(),
            kind: SubAgentKind::LlmSession {
                model: "qwen".to_string(),
                goal: "fix stuff".to_string(),
            },
            parent_id: None,
            status: SubAgentStatus::Running,
            depth: 0,
            spawned_at: String::new(),
            completed_at: None,
        };

        let label = format_entry_label(&entry);
        assert!(label.contains("[*]"));
        assert!(label.contains("LLM"));
        assert!(label.contains("fix stuff"));
        assert!(label.contains("running"));
    }

    #[test]
    fn format_entry_label_proc_completed() {
        let entry = SubAgentInfo {
            id: "p1".to_string(),
            kind: SubAgentKind::BackgroundProcess {
                command: "cargo build".to_string(),
            },
            parent_id: None,
            status: SubAgentStatus::Completed,
            depth: 0,
            spawned_at: String::new(),
            completed_at: Some(String::new()),
        };

        let label = format_entry_label(&entry);
        assert!(label.contains("[+]"));
        assert!(label.contains("Proc"));
        assert!(label.contains("cargo build"));
        assert!(label.contains("completed"));
    }

    #[test]
    fn status_icons_are_distinct() {
        assert_ne!(status_icon(&SubAgentStatus::Running), status_icon(&SubAgentStatus::Completed));
        assert_ne!(status_icon(&SubAgentStatus::Running), status_icon(&SubAgentStatus::Failed("err".into())));
        assert_ne!(status_icon(&SubAgentStatus::Running), status_icon(&SubAgentStatus::Killed));
    }

    #[test]
    fn truncate_long_string() {
        let long = "a".repeat(50);
        let result = truncate(&long, 10);
        assert_eq!(result.len(), 13); // 10 + "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn truncate_short_string_unchanged() {
        let short = "hello";
        let result = truncate(short, 10);
        assert_eq!(result, "hello");
    }
}
