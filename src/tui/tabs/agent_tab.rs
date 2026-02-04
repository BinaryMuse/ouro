//! Agent tab rendering (Tab 1).
//!
//! Displays the log stream as the primary content, with an optional
//! sub-agent tree panel at the bottom (placeholder for Phase 5).

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

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

        // Sub-agent tree placeholder in bottom portion
        render_sub_agent_placeholder(chunks[1], buf);
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

/// Render a placeholder for the sub-agent tree panel.
///
/// This will be replaced with real sub-agent tree rendering in Phase 5.
fn render_sub_agent_placeholder(area: Rect, buf: &mut Buffer) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Sub-Agents ");

    let inner = block.inner(area);
    block.render(area, buf);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let placeholder = Paragraph::new("(No sub-agents -- Phase 5)")
        .style(Style::default().fg(Color::DarkGray));
    placeholder.render(inner, buf);
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
    fn render_agent_tab_with_sub_agent_panel() {
        let state = AppState::new();
        // sub_agent_panel_visible defaults to true
        assert!(state.sub_agent_panel_visible);
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render_agent_tab(&state, area, &mut buf);

        let content: String = buf.content().iter().map(|c| c.symbol().to_string()).collect();
        assert!(content.contains("Sub-Agents"));
        assert!(content.contains("Phase 5"));
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
    fn sub_agent_placeholder_renders() {
        let area = Rect::new(0, 0, 40, 5);
        let mut buf = Buffer::empty(area);
        render_sub_agent_placeholder(area, &mut buf);
        let content: String = buf.content().iter().map(|c| c.symbol().to_string()).collect();
        assert!(content.contains("Phase 5"));
    }
}
