//! Two-line status bar widget.
//!
//! Renders persistent status information at the bottom of the TUI:
//! - Line 1: Agent state (colored), context pressure gauge, session/turn/tool counters
//! - Line 2: Keybind hints for the current context

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::tui::app_state::AppState;
use crate::tui::event::AgentState;
use crate::tui::widgets::context_gauge;

/// Style for the agent state indicator, colored per state.
fn agent_state_style(state: AgentState) -> Style {
    let color = match state {
        AgentState::Thinking => Color::Yellow,
        AgentState::Executing => Color::Cyan,
        AgentState::Idle => Color::DarkGray,
        AgentState::Paused => Color::Red,
        AgentState::Sleeping => Color::Magenta,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

/// Render the two-line status bar into the given area.
///
/// Line 1: `[AgentState] | [context gauge] | Session N | Turn N | Tools: N`
/// Line 2: `Tab: switch tabs | arrows: scroll | p: pause/resume | e: expand | q: quit`
pub fn render_status_bar(state: &AppState, area: Rect, buf: &mut Buffer) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let sep = Span::styled(" | ", Style::default().fg(Color::DarkGray));

    // -- Line 1: Status indicators --
    let mut line1_spans: Vec<Span<'static>> = Vec::new();

    // Agent state
    line1_spans.push(Span::styled(
        format!(" {}", state.agent_state),
        agent_state_style(state.agent_state),
    ));

    // Sleep display text (shown when sleeping, next to state indicator)
    if !state.sleep_display_text.is_empty() {
        line1_spans.push(Span::styled(
            format!(" ({})", state.sleep_display_text),
            Style::default().fg(Color::Magenta),
        ));
    }

    line1_spans.push(sep.clone());

    // Context gauge
    let gauge_spans = context_gauge::render_context_gauge(state.context_usage_pct);
    line1_spans.extend(gauge_spans);

    line1_spans.push(sep.clone());

    // Session number
    line1_spans.push(Span::raw(format!("Session {}", state.session_number)));

    line1_spans.push(sep.clone());

    // Turn count
    line1_spans.push(Span::raw(format!("Turn {}", state.turn_count)));

    line1_spans.push(sep.clone());

    // Tool call count
    line1_spans.push(Span::raw(format!("Tools: {}", state.tool_call_count)));

    let line1 = Line::from(line1_spans);

    // -- Line 2: Keybind hints --
    let hint_style = Style::default().fg(Color::DarkGray);
    let key_style = Style::default().fg(Color::White);

    let mut line2_spans = vec![
        Span::raw(" "),
        Span::styled("Tab", key_style),
        Span::styled(": switch tabs", hint_style),
        Span::styled(" | ", hint_style),
        Span::styled("\u{2191}\u{2193}", key_style), // "up/down arrows"
        Span::styled(": scroll", hint_style),
        Span::styled(" | ", hint_style),
        Span::styled("p", key_style),
        Span::styled(": pause/resume", hint_style),
        Span::styled(" | ", hint_style),
        Span::styled("e", key_style),
        Span::styled(": expand", hint_style),
        Span::styled(" | ", hint_style),
        Span::styled("q", key_style),
        Span::styled(": quit", hint_style),
    ];

    // Show resume hint when agent is sleeping
    if state.agent_state == AgentState::Sleeping {
        line2_spans.push(Span::styled(" | ", hint_style));
        line2_spans.push(Span::styled("r", key_style));
        line2_spans.push(Span::styled(": resume sleep", hint_style));
    }

    let line2 = Line::from(line2_spans);

    let paragraph = Paragraph::new(vec![line1, line2]);
    paragraph.render(area, buf);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_state_styles_are_distinct() {
        let thinking = agent_state_style(AgentState::Thinking);
        let executing = agent_state_style(AgentState::Executing);
        let idle = agent_state_style(AgentState::Idle);
        let paused = agent_state_style(AgentState::Paused);

        assert_ne!(thinking.fg, executing.fg);
        assert_ne!(idle.fg, paused.fg);
        assert_ne!(thinking.fg, idle.fg);
    }

    #[test]
    fn render_status_bar_does_not_panic_empty_area() {
        let state = AppState::new();
        let mut buf = Buffer::empty(Rect::ZERO);
        render_status_bar(&state, Rect::ZERO, &mut buf);
    }

    #[test]
    fn render_status_bar_default_state() {
        let state = AppState::new();
        let area = Rect::new(0, 0, 80, 2);
        let mut buf = Buffer::empty(area);
        render_status_bar(&state, area, &mut buf);
        // Should render without panic and contain agent state text
        let content: String = buf.content().iter().map(|c| c.symbol().to_string()).collect();
        assert!(content.contains("Idle"));
        assert!(content.contains("Session 1"));
        assert!(content.contains("Turn 0"));
        assert!(content.contains("Tools: 0"));
    }

    #[test]
    fn render_status_bar_with_activity() {
        let mut state = AppState::new();
        state.agent_state = AgentState::Thinking;
        state.session_number = 3;
        state.turn_count = 15;
        state.tool_call_count = 42;
        state.context_usage_pct = 0.65;

        let area = Rect::new(0, 0, 100, 2);
        let mut buf = Buffer::empty(area);
        render_status_bar(&state, area, &mut buf);

        let content: String = buf.content().iter().map(|c| c.symbol().to_string()).collect();
        assert!(content.contains("Thinking"));
        assert!(content.contains("Session 3"));
        assert!(content.contains("Turn 15"));
        assert!(content.contains("Tools: 42"));
        assert!(content.contains("65%"));
    }

    #[test]
    fn keybind_hints_present() {
        let state = AppState::new();
        let area = Rect::new(0, 0, 80, 2);
        let mut buf = Buffer::empty(area);
        render_status_bar(&state, area, &mut buf);

        let content: String = buf.content().iter().map(|c| c.symbol().to_string()).collect();
        assert!(content.contains("Tab"));
        assert!(content.contains("scroll"));
        assert!(content.contains("quit"));
    }
}
