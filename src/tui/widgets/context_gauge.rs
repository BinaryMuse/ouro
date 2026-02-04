//! Context pressure visualization widget.
//!
//! Renders a small inline gauge showing context window usage as a colored
//! bar with percentage text. Color transitions from green (low) through
//! yellow (moderate) to red (high pressure).

use ratatui::style::{Color, Style};
use ratatui::text::Span;

/// Width of the bar portion of the gauge (number of block characters).
const BAR_WIDTH: usize = 10;

/// Filled block character for the bar.
const FILLED: &str = "\u{2588}"; // Full block: "█"

/// Empty block character for the bar.
const EMPTY: &str = "\u{2591}"; // Light shade: "░"

/// Render a context pressure gauge as a vector of styled spans.
///
/// Returns something like: `[████░░░░░░] 48%`
///
/// Color thresholds:
/// - Green: 0% to 50%
/// - Yellow: 50% to 70%
/// - Red: 70%+
///
/// `usage_pct` should be in the range 0.0..=1.0 (fraction, not percentage).
pub fn render_context_gauge(usage_pct: f64) -> Vec<Span<'static>> {
    let pct = (usage_pct * 100.0).clamp(0.0, 100.0);
    let filled_count = ((usage_pct * BAR_WIDTH as f64).round() as usize).min(BAR_WIDTH);
    let empty_count = BAR_WIDTH - filled_count;

    let color = gauge_color(usage_pct);
    let bar_style = Style::default().fg(color);
    let dim_style = Style::default().fg(Color::DarkGray);

    vec![
        Span::raw("["),
        Span::styled(FILLED.repeat(filled_count), bar_style),
        Span::styled(EMPTY.repeat(empty_count), dim_style),
        Span::raw(format!("] {:.0}%", pct)),
    ]
}

/// Select gauge color based on usage fraction.
fn gauge_color(usage_pct: f64) -> Color {
    if usage_pct < 0.5 {
        Color::Green
    } else if usage_pct < 0.7 {
        Color::Yellow
    } else {
        Color::Red
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_usage_is_green_and_empty() {
        let spans = render_context_gauge(0.0);
        // Should have 4 spans: "[", filled, empty, "] 0%"
        assert_eq!(spans.len(), 4);
        // Filled portion should be empty string (0 repeats)
        assert_eq!(spans[1].content, "");
        // Empty portion should be BAR_WIDTH blocks
        assert_eq!(spans[2].content.chars().count(), BAR_WIDTH);
        assert!(spans[3].content.contains("0%"));
    }

    #[test]
    fn full_usage_is_red() {
        let spans = render_context_gauge(1.0);
        assert_eq!(spans[1].content.chars().count(), BAR_WIDTH);
        assert_eq!(spans[2].content, "");
        assert!(spans[3].content.contains("100%"));
        // Check color is red
        assert_eq!(spans[1].style.fg, Some(Color::Red));
    }

    #[test]
    fn half_usage_is_yellow() {
        let spans = render_context_gauge(0.5);
        assert_eq!(spans[1].style.fg, Some(Color::Yellow));
    }

    #[test]
    fn low_usage_is_green() {
        let spans = render_context_gauge(0.3);
        assert_eq!(spans[1].style.fg, Some(Color::Green));
    }

    #[test]
    fn high_usage_is_red() {
        let spans = render_context_gauge(0.85);
        assert_eq!(spans[1].style.fg, Some(Color::Red));
    }

    #[test]
    fn clamps_above_one() {
        let spans = render_context_gauge(1.5);
        // Should not panic, should clamp to 100%
        assert!(spans[3].content.contains("100%"));
    }

    #[test]
    fn clamps_below_zero() {
        let spans = render_context_gauge(-0.1);
        assert!(spans[3].content.contains("0%"));
    }
}
