//! Log stream rendering widget.
//!
//! Renders a scrollable list of [`LogEntry`] items as structured visual blocks.
//! Each entry has a color-coded header line (icon + timestamp + kind) and an
//! indented content area that can be expanded or collapsed.

use ratatui::buffer::Buffer;
use ratatui::layout::{Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget,
    Widget, Wrap,
};

use crate::tui::app_state::{LogEntry, LogEntryKind};

/// Icon/prefix symbols for each log entry kind.
fn kind_prefix(kind: LogEntryKind) -> &'static str {
    match kind {
        LogEntryKind::Thought => "\u{25CB}", // "○" open circle
        LogEntryKind::ToolCall => "\u{25B6}", // "▶" right-pointing triangle
        LogEntryKind::ToolResult => "\u{25C0}", // "◀" left-pointing triangle
        LogEntryKind::Error => "\u{2716}",     // "✖" heavy multiplication X
        LogEntryKind::SessionSeparator => "\u{2500}", // "─" box drawing horizontal
        LogEntryKind::System => "\u{2605}",    // "★" black star
    }
}

/// Style for the header line based on entry kind.
fn kind_style(kind: LogEntryKind) -> Style {
    match kind {
        LogEntryKind::Thought => Style::default().fg(Color::Cyan),
        LogEntryKind::ToolCall => Style::default().fg(Color::Yellow),
        LogEntryKind::ToolResult => Style::default().fg(Color::Green),
        LogEntryKind::Error => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        LogEntryKind::SessionSeparator => Style::default().fg(Color::DarkGray),
        LogEntryKind::System => Style::default().fg(Color::Magenta),
    }
}

/// Kind label text for the header.
fn kind_label(kind: LogEntryKind) -> &'static str {
    match kind {
        LogEntryKind::Thought => "thought",
        LogEntryKind::ToolCall => "tool-call",
        LogEntryKind::ToolResult => "tool-result",
        LogEntryKind::Error => "error",
        LogEntryKind::SessionSeparator => "",
        LogEntryKind::System => "system",
    }
}

/// Build a Vec<Line> representing all visible log entries.
///
/// This produces the full set of lines for the visible portion of the log,
/// accounting for scroll offset and the available height.
fn build_log_lines<'a>(entries: &'a [LogEntry], area_width: u16) -> Vec<Line<'a>> {
    let mut lines: Vec<Line<'a>> = Vec::new();
    let content_width = (area_width as usize).saturating_sub(4); // indent for content

    for entry in entries {
        if entry.kind == LogEntryKind::SessionSeparator {
            // Render as a dim separator line
            let sep_char = "\u{2500}"; // "─"
            let sep_width = (area_width as usize).saturating_sub(2);
            let separator = sep_char.repeat(sep_width.min(80));
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {} ", entry.summary),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
            lines.push(Line::from(Span::styled(
                separator,
                Style::default().fg(Color::DarkGray),
            )));
            continue;
        }

        let style = kind_style(entry.kind);
        let prefix = kind_prefix(entry.kind);
        let label = kind_label(entry.kind);

        // Header line: icon + timestamp + kind label
        let header_spans = if entry.timestamp.is_empty() {
            vec![
                Span::styled(format!("{prefix} "), style),
                Span::styled(label.to_string(), style),
            ]
        } else {
            vec![
                Span::styled(format!("{prefix} "), style),
                Span::styled(
                    format!("{} ", entry.timestamp),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(label.to_string(), style),
            ]
        };
        lines.push(Line::from(header_spans));

        // Content area (indented)
        if entry.expanded {
            // Show full content, wrapping lines to available width
            for content_line in entry.full_content.lines() {
                let wrapped = wrap_text(content_line, content_width);
                for w in wrapped {
                    lines.push(Line::from(vec![
                        Span::raw("    "),
                        Span::raw(w),
                    ]));
                }
            }
            // Handle empty full_content with non-empty summary
            if entry.full_content.is_empty() && !entry.summary.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(
                        entry.summary.clone(),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
        } else {
            // Show collapsed summary
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(
                    entry.summary.clone(),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }

        // Blank line between entries for visual separation
        lines.push(Line::from(""));
    }

    lines
}

/// Simple word-boundary-unaware text wrapping.
fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    let mut result = Vec::new();
    let mut remaining = text;
    while remaining.len() > max_width {
        result.push(remaining[..max_width].to_string());
        remaining = &remaining[max_width..];
    }
    result.push(remaining.to_string());
    result
}

/// Render log entries into the given area with a bordered block and scrollbar.
///
/// `entries` is the full list of log entries. `scroll_offset` determines which
/// entry is at the conceptual top of the view (used for the scrollbar position
/// indicator, while Paragraph handles the actual text scroll).
pub fn render_log_entries(
    entries: &[LogEntry],
    scroll_offset: usize,
    area: Rect,
    buf: &mut Buffer,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Log ");

    let inner = block.inner(area);
    block.render(area, buf);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let lines = build_log_lines(entries, inner.width);
    let total_lines = lines.len();

    // Calculate a line-based scroll offset from the entry-based scroll_offset.
    // We count how many lines are produced by entries before scroll_offset.
    let line_offset = entry_to_line_offset(entries, scroll_offset, inner.width);

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((line_offset as u16, 0));

    paragraph.render(inner, buf);

    // Scrollbar on the right edge
    if total_lines > inner.height as usize {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("\u{2191}")) // "↑"
            .end_symbol(Some("\u{2193}")); // "↓"

        let mut scrollbar_state = ScrollbarState::new(total_lines.saturating_sub(inner.height as usize))
            .position(line_offset);

        StatefulWidget::render(
            scrollbar,
            inner.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            buf,
            &mut scrollbar_state,
        );
    }
}

/// Convert an entry-based scroll offset to a line offset.
///
/// Counts how many rendered lines entries[0..scroll_offset] produce, so the
/// Paragraph can scroll to the correct position.
fn entry_to_line_offset(entries: &[LogEntry], scroll_offset: usize, area_width: u16) -> usize {
    let content_width = (area_width as usize).saturating_sub(4);
    let mut line_count = 0;

    for entry in entries.iter().take(scroll_offset) {
        if entry.kind == LogEntryKind::SessionSeparator {
            line_count += 2; // separator text + separator line
            continue;
        }

        // Header line
        line_count += 1;

        // Content lines
        if entry.expanded {
            if entry.full_content.is_empty() && !entry.summary.is_empty() {
                line_count += 1;
            } else {
                for content_line in entry.full_content.lines() {
                    line_count += wrap_text(content_line, content_width).len();
                }
            }
        } else {
            line_count += 1; // collapsed summary
        }

        // Blank separator
        line_count += 1;
    }

    line_count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app_state::LogEntry;

    fn make_entry(kind: LogEntryKind, summary: &str, full_content: &str, expanded: bool) -> LogEntry {
        LogEntry {
            timestamp: "14:00:00".to_string(),
            kind,
            summary: summary.to_string(),
            full_content: full_content.to_string(),
            expanded,
        }
    }

    #[test]
    fn build_lines_empty_entries() {
        let lines = build_log_lines(&[], 80);
        assert!(lines.is_empty());
    }

    #[test]
    fn build_lines_thought_expanded() {
        let entry = make_entry(
            LogEntryKind::Thought,
            "I should read config",
            "I should read config first.\nThen process it.",
            true,
        );
        let entries = [entry];
        let lines = build_log_lines(&entries, 80);
        // Header + 2 content lines + blank separator = 4
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn build_lines_tool_result_collapsed() {
        let entry = make_entry(
            LogEntryKind::ToolResult,
            "shell_exec: 5 lines of output",
            "line1\nline2\nline3\nline4\nline5",
            false,
        );
        let entries = [entry];
        let lines = build_log_lines(&entries, 80);
        // Header + collapsed summary + blank = 3
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn build_lines_session_separator() {
        let entry = LogEntry {
            timestamp: String::new(),
            kind: LogEntryKind::SessionSeparator,
            summary: "--- Session 2 started ---".to_string(),
            full_content: String::new(),
            expanded: false,
        };
        let entries = [entry];
        let lines = build_log_lines(&entries, 80);
        // Separator text + separator line = 2
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn entry_to_line_offset_zero() {
        let entries = vec![make_entry(LogEntryKind::Thought, "test", "test content", true)];
        assert_eq!(entry_to_line_offset(&entries, 0, 80), 0);
    }

    #[test]
    fn entry_to_line_offset_skips_entries() {
        let entries = vec![
            make_entry(LogEntryKind::Thought, "first", "first content", true),
            make_entry(LogEntryKind::ToolCall, "second", "", false),
        ];
        // First entry: header(1) + content(1) + blank(1) = 3
        assert_eq!(entry_to_line_offset(&entries, 1, 80), 3);
    }

    #[test]
    fn wrap_text_short() {
        let result = wrap_text("hello", 80);
        assert_eq!(result, vec!["hello"]);
    }

    #[test]
    fn wrap_text_long() {
        let result = wrap_text("abcdef", 3);
        assert_eq!(result, vec!["abc", "def"]);
    }

    #[test]
    fn kind_styles_are_distinct() {
        // Ensure each kind has a different color/style
        let thought = kind_style(LogEntryKind::Thought);
        let tool_call = kind_style(LogEntryKind::ToolCall);
        let error = kind_style(LogEntryKind::Error);
        assert_ne!(thought.fg, tool_call.fg);
        assert_ne!(thought.fg, error.fg);
    }

    #[test]
    fn render_empty_log_does_not_panic() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 10));
        render_log_entries(&[], 0, Rect::new(0, 0, 40, 10), &mut buf);
        // Should not panic, just render empty bordered block
    }
}
