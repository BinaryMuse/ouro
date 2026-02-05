//! Top-level TUI render function.
//!
//! [`render_ui`] is the single entry point called each frame by the main loop.
//! It composes the tab bar, active tab content, and status bar into a complete
//! frame, dispatching to the appropriate tab renderer based on [`AppState::active_tab`].

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Tabs, Widget};
use ratatui::Frame;

use crate::tui::app_state::AppState;
use crate::tui::tabs::{agent_tab, discoveries_tab};
use crate::tui::widgets::status_bar;

/// Tab titles displayed in the tab bar.
const TAB_TITLES: &[&str] = &["Agent", "Discoveries"];

/// Render the complete TUI from the current application state.
///
/// Layout (top to bottom):
/// 1. Tab bar (1 line): shows tab titles with the active tab highlighted
/// 2. Content area (remaining space minus 2 lines): dispatches to the active tab
/// 3. Status bar (2 lines): agent state, context gauge, counters, keybinds
///
/// If `quit_pending` is true, a centered confirmation dialog overlays the content.
pub fn render_ui(state: &AppState, frame: &mut Frame) {
    let area = frame.area();

    let chunks = Layout::vertical([
        Constraint::Length(1),  // Tab bar
        Constraint::Min(0),    // Content area
        Constraint::Length(2), // Status bar
    ])
    .split(area);

    let tab_bar_area = chunks[0];
    let content_area = chunks[1];
    let status_bar_area = chunks[2];

    // -- Tab bar --
    render_tab_bar(state.active_tab, tab_bar_area, frame.buffer_mut());

    // -- Content area: dispatch to active tab --
    match state.active_tab {
        0 => agent_tab::render_agent_tab(state, content_area, frame.buffer_mut()),
        1 => discoveries_tab::render_discoveries_tab(state, content_area, frame.buffer_mut()),
        _ => {} // Unknown tab index, render nothing
    }

    // -- Status bar --
    status_bar::render_status_bar(state, status_bar_area, frame.buffer_mut());

    // -- Quit confirmation overlay --
    if state.quit_pending {
        render_quit_dialog(area, frame.buffer_mut());
    }
}

/// Render the tab bar showing tab titles with the active tab highlighted.
fn render_tab_bar(active_tab: usize, area: Rect, buf: &mut Buffer) {
    let titles: Vec<Line<'_>> = TAB_TITLES.iter().map(|t| Line::from(*t)).collect();

    let tabs = Tabs::new(titles)
        .select(active_tab)
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )
        .divider(Span::styled("|", Style::default().fg(Color::DarkGray)));

    Widget::render(tabs, area, buf);
}

/// Render a centered quit confirmation dialog.
fn render_quit_dialog(area: Rect, buf: &mut Buffer) {
    let dialog_width: u16 = 24;
    let dialog_height: u16 = 3;

    // Center the dialog
    let x = area.x + area.width.saturating_sub(dialog_width) / 2;
    let y = area.y + area.height.saturating_sub(dialog_height) / 2;

    let dialog_area = Rect::new(x, y, dialog_width.min(area.width), dialog_height.min(area.height));

    // Clear the area behind the dialog
    Clear.render(dialog_area, buf);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Confirm ")
        .style(Style::default().fg(Color::Red));

    let inner = block.inner(dialog_area);
    block.render(dialog_area, buf);

    if inner.width > 0 && inner.height > 0 {
        let prompt = Paragraph::new(Line::from(vec![
            Span::raw("  Quit? ("),
            Span::styled("y", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("/"),
            Span::styled("n", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(")"),
        ]));
        prompt.render(inner, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use crate::tui::event::{AgentEvent, AgentState};

    /// Helper to render state to a test terminal and extract buffer content.
    fn render_to_string(state: &AppState, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_ui(state, frame);
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        buf.content().iter().map(|c| c.symbol().to_string()).collect()
    }

    #[test]
    fn render_ui_default_state() {
        let state = AppState::new();
        let content = render_to_string(&state, 80, 24);
        // Tab bar should show both tabs
        assert!(content.contains("Agent"));
        assert!(content.contains("Discoveries"));
        // Status bar should show defaults
        assert!(content.contains("Idle"));
        assert!(content.contains("Session 1"));
    }

    #[test]
    fn render_ui_agent_tab_active() {
        let state = AppState::new();
        // active_tab defaults to 0 (Agent)
        assert_eq!(state.active_tab, 0);
        let content = render_to_string(&state, 80, 24);
        // Should show log and sub-agent panel
        assert!(content.contains("Log"));
        assert!(content.contains("Sub-Agents"));
    }

    #[test]
    fn render_ui_discoveries_tab_active() {
        let mut state = AppState::new();
        state.active_tab = 1;
        let content = render_to_string(&state, 80, 24);
        // Should show discoveries placeholder
        assert!(content.contains("No discoveries flagged yet"));
    }

    #[test]
    fn render_ui_with_discoveries() {
        let mut state = AppState::new();
        state.active_tab = 1;
        state.apply_event(AgentEvent::Discovery {
            timestamp: "15:00:00".into(),
            title: "Found important file".into(),
            description: "An important configuration file".into(),
        });
        let content = render_to_string(&state, 80, 24);
        assert!(content.contains("Found important file"));
    }

    #[test]
    fn render_ui_quit_pending_shows_dialog() {
        let mut state = AppState::new();
        state.quit_pending = true;
        let content = render_to_string(&state, 80, 24);
        assert!(content.contains("Quit?"));
        assert!(content.contains("Confirm"));
    }

    #[test]
    fn render_ui_with_log_entries() {
        let mut state = AppState::new();
        state.apply_event(AgentEvent::ThoughtText {
            timestamp: "14:00:00".into(),
            turn: 1,
            content: "Reading the config file now".into(),
        });
        // Render with only one entry so auto_scroll keeps it in view
        let content = render_to_string(&state, 100, 24);
        // Thought entry should be visible (expanded by default)
        assert!(content.contains("Reading the config file now"));
        // Also verify the thought kind indicator is present
        assert!(content.contains("thought"));
    }

    #[test]
    fn render_ui_status_bar_reflects_state() {
        let mut state = AppState::new();
        state.agent_state = AgentState::Executing;
        state.turn_count = 7;
        state.tool_call_count = 23;
        state.session_number = 2;
        state.context_usage_pct = 0.45;
        let content = render_to_string(&state, 100, 24);
        assert!(content.contains("Executing"));
        assert!(content.contains("Turn 7"));
        assert!(content.contains("Tools: 23"));
        assert!(content.contains("Session 2"));
        assert!(content.contains("45%"));
    }

    #[test]
    fn render_ui_small_terminal() {
        // Should not panic even with tiny terminal
        let state = AppState::new();
        let content = render_to_string(&state, 20, 5);
        // Just verify it doesn't panic -- content may be truncated
        assert!(!content.is_empty());
    }

    #[test]
    fn tab_bar_renders_both_tabs() {
        let area = Rect::new(0, 0, 40, 1);
        let mut buf = Buffer::empty(area);
        render_tab_bar(0, area, &mut buf);
        let content: String = buf.content().iter().map(|c| c.symbol().to_string()).collect();
        assert!(content.contains("Agent"));
        assert!(content.contains("Discoveries"));
    }

    #[test]
    fn quit_dialog_renders_centered() {
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render_quit_dialog(area, &mut buf);
        let content: String = buf.content().iter().map(|c| c.symbol().to_string()).collect();
        assert!(content.contains("Quit?"));
    }
}
