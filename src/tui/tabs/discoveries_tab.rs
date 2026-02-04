//! Discoveries tab rendering (Tab 2).
//!
//! Displays a scrollable list of agent-flagged discoveries in reverse
//! chronological order (most recent at top).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Widget};

use crate::tui::app_state::AppState;

/// Render the Discoveries tab into the given area.
///
/// Shows a bordered block titled "Discoveries" containing either:
/// - A list of timestamped discovery entries (most recent first)
/// - A placeholder message if no discoveries exist
pub fn render_discoveries_tab(state: &AppState, area: Rect, buf: &mut Buffer) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Discoveries ");

    if state.discoveries.is_empty() {
        let inner = block.inner(area);
        block.render(area, buf);

        if inner.width == 0 || inner.height == 0 {
            return;
        }

        let placeholder = Paragraph::new("No discoveries flagged yet")
            .style(Style::default().fg(Color::DarkGray));
        placeholder.render(inner, buf);
        return;
    }

    // Build list items in reverse chronological order (most recent first)
    let items: Vec<ListItem<'_>> = state
        .discoveries
        .iter()
        .rev()
        .map(|(timestamp, content)| {
            let line = Line::from(format!("[{timestamp}] {content}"));
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .style(Style::default().fg(Color::White));

    Widget::render(list, area, buf);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::event::AgentEvent;

    #[test]
    fn render_empty_discoveries() {
        let state = AppState::new();
        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);
        render_discoveries_tab(&state, area, &mut buf);

        let content: String = buf.content().iter().map(|c| c.symbol().to_string()).collect();
        assert!(content.contains("No discoveries flagged yet"));
        assert!(content.contains("Discoveries"));
    }

    #[test]
    fn render_with_discoveries() {
        let mut state = AppState::new();
        state.apply_event(AgentEvent::Discovery {
            timestamp: "14:00:00".into(),
            content: "Found a Makefile".into(),
        });
        state.apply_event(AgentEvent::Discovery {
            timestamp: "14:01:00".into(),
            content: "Found a README".into(),
        });

        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);
        render_discoveries_tab(&state, area, &mut buf);

        let content: String = buf.content().iter().map(|c| c.symbol().to_string()).collect();
        // Most recent should appear first
        assert!(content.contains("14:01:00"));
        assert!(content.contains("README"));
        assert!(content.contains("14:00:00"));
        assert!(content.contains("Makefile"));
    }

    #[test]
    fn render_zero_area_no_panic() {
        let state = AppState::new();
        let mut buf = Buffer::empty(Rect::ZERO);
        render_discoveries_tab(&state, Rect::ZERO, &mut buf);
    }

    #[test]
    fn render_discoveries_reverse_order() {
        let mut state = AppState::new();
        state.apply_event(AgentEvent::Discovery {
            timestamp: "10:00".into(),
            content: "First discovery".into(),
        });
        state.apply_event(AgentEvent::Discovery {
            timestamp: "11:00".into(),
            content: "Second discovery".into(),
        });
        state.apply_event(AgentEvent::Discovery {
            timestamp: "12:00".into(),
            content: "Third discovery".into(),
        });

        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);
        render_discoveries_tab(&state, area, &mut buf);

        let content: String = buf.content().iter().map(|c| c.symbol().to_string()).collect();
        // Third (most recent) should appear before First (oldest) in the rendered output
        let pos_third = content.find("Third discovery").expect("Third should be present");
        let pos_first = content.find("First discovery").expect("First should be present");
        assert!(pos_third < pos_first, "Most recent should appear first");
    }
}
