# Phase 4: TUI Dashboard - Research

**Researched:** 2026-02-04
**Domain:** Terminal User Interface with Ratatui, async Tokio integration, real-time agent monitoring
**Confidence:** HIGH

## Summary

This phase adds a real-time terminal UI dashboard to the Ouroboros agent using Ratatui 0.30 with the Crossterm backend. The TUI acts as a read/control layer over the existing async agent loop, displaying a tabbed interface with a log stream, sub-agent tree, discoveries list, and a persistent status bar. The critical architectural challenge is decoupling the agent loop (which runs as a tokio task) from the TUI rendering loop so neither blocks the other.

The standard approach is a **channel-based architecture** where the agent loop sends `AppEvent` messages through a `tokio::sync::mpsc` channel to the TUI main loop, which multiplexes these events with keyboard input and render ticks using `tokio::select!`. The TUI maintains an `AppState` struct that accumulates events into renderable state. Rendering is immediate-mode: every frame, the entire UI is rebuilt from current state.

Ratatui 0.30 provides all needed built-in widgets (Tabs, Paragraph, List, Gauge/LineGauge, Scrollbar) plus the tui-tree-widget crate for the sub-agent tree. The crossterm backend is re-exported from ratatui, so no separate crossterm dependency is needed. Terminal initialization and panic hook setup are handled by `ratatui::init()` and `ratatui::restore()`.

**Primary recommendation:** Use a tokio mpsc channel from the agent loop to the TUI, with `tokio::select!` in the TUI main loop multiplexing agent events, keyboard input (via crossterm EventStream), and render ticks. Keep the agent loop as a spawned tokio task; the TUI owns the terminal and the main thread.

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| ratatui | 0.30.0 | TUI rendering framework | De facto standard for Rust TUIs. 17.3k stars, 14.9M downloads. Immediate-mode rendering with widget library. |
| crossterm | 0.29.x (via ratatui re-export) | Terminal backend | Default backend for ratatui. Cross-platform. Use `ratatui::crossterm` re-export to avoid version conflicts. |
| tokio | 1.x (already in project) | Async runtime | Already used by the agent loop. Provides mpsc channels, select!, spawn, and interval timers. |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tui-tree-widget | 0.24.0 | Tree view widget for sub-agent display | Rendering the sub-agent/background task tree on Tab 1. Provides TreeItem, Tree, and TreeState for collapsible hierarchical data. |
| futures | 0.3 (already in project) | Stream utilities | Already a dependency. Needed for `StreamExt` on crossterm's `EventStream`. |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| tui-tree-widget | Hand-rolled tree with List | tui-tree-widget is small, purpose-built, and handles expand/collapse state. Hand-rolling would duplicate effort. |
| Paragraph for log stream | tui-scrollview | Paragraph + manual scroll state is simpler and sufficient. tui-scrollview adds dependency for a feature we can build with Paragraph.scroll(). |
| LineGauge for context pressure | Gauge | LineGauge takes 1 line of height (ideal for status bar). Gauge takes more vertical space. LineGauge is the right choice for the compact status bar. |

**Installation:**
```bash
cargo add ratatui@0.30.0
cargo add tui-tree-widget@0.24.0
```

**Note on crossterm:** Do NOT add crossterm as a separate dependency. Use `ratatui::crossterm` re-export. Ratatui 0.30 defaults to crossterm 0.29.x via the `crossterm_0_29` feature flag. For the `event-stream` async feature, the ratatui crate re-exports crossterm's `EventStream` when the crossterm feature is active. If the `event-stream` feature is needed explicitly, it may need to be enabled on the crossterm dependency -- but first try using `ratatui::crossterm::event::EventStream` directly.

## Architecture Patterns

### Recommended Project Structure

```
src/
  tui/
    mod.rs             # Module root, re-exports
    app_state.rs       # AppState struct (all TUI-visible state)
    event.rs           # AppEvent enum + EventHandler (crossterm + agent events)
    ui.rs              # Top-level render function dispatching to tab renderers
    tabs/
      mod.rs           # Tab enum and routing
      agent_tab.rs     # Tab 1: log stream + sub-agent tree
      discoveries_tab.rs  # Tab 2: discoveries list
    widgets/
      mod.rs           # Re-exports
      log_stream.rs    # Log entry rendering (structured blocks)
      status_bar.rs    # Two-line status bar widget
      context_gauge.rs # Context pressure gauge for status bar
```

### Pattern 1: Channel-Based Agent-TUI Decoupling

**What:** The agent loop runs as a spawned tokio task and sends structured events through an `mpsc::unbounded_channel()` to the TUI main loop. The TUI never calls into the agent directly; it only reads events and sends control signals (pause/resume/quit) via a separate control channel.

**When to use:** Always -- this is the core integration pattern.

**Example:**
```rust
// Source: Ratatui async template + Tokio channel patterns

/// Events the agent loop sends to the TUI
pub enum AgentEvent {
    /// Agent produced thinking text
    ThoughtText { timestamp: String, turn: u64, content: String },
    /// Agent requested a tool call
    ToolCallStarted { timestamp: String, turn: u64, call_id: String, fn_name: String, args_summary: String },
    /// Tool call completed with result
    ToolCallCompleted { timestamp: String, turn: u64, call_id: String, fn_name: String, result_summary: String, full_result: String },
    /// Agent state changed
    StateChanged(AgentState),
    /// Context pressure updated
    ContextPressure { usage_pct: f64, prompt_tokens: usize, context_limit: usize },
    /// Session restarted
    SessionRestarted { session_number: u32 },
    /// Error occurred
    Error { timestamp: String, turn: u64, message: String },
    /// Agent flagged a discovery
    Discovery { timestamp: String, content: String },
    /// Turn/tool counters updated
    CountersUpdated { turn: u64, tool_calls: u64 },
}

#[derive(Clone, Copy, PartialEq)]
pub enum AgentState {
    Thinking,
    Executing,
    Idle,
    Paused,
}

/// Control signals the TUI sends to the agent
pub enum ControlSignal {
    Pause,
    Resume,
    Quit,
}
```

### Pattern 2: TUI Main Loop with tokio::select!

**What:** The TUI main loop uses `tokio::select!` to multiplex three event sources: (1) keyboard/terminal events from crossterm EventStream, (2) agent events from the mpsc channel, and (3) periodic render ticks.

**When to use:** This IS the main loop structure.

**Example:**
```rust
// Source: ratatui async template pattern + official tutorials

use ratatui::crossterm::event::{EventStream, KeyCode, KeyEventKind};
use futures::StreamExt;
use tokio::sync::mpsc;

pub async fn run_tui(
    agent_rx: mpsc::UnboundedReceiver<AgentEvent>,
    control_tx: mpsc::UnboundedSender<ControlSignal>,
) -> anyhow::Result<()> {
    let mut terminal = ratatui::init();
    let mut app_state = AppState::new();
    let mut agent_rx = agent_rx;
    let mut event_stream = EventStream::new();
    let mut render_interval = tokio::time::interval(std::time::Duration::from_millis(50)); // 20 FPS

    loop {
        // Render current state
        terminal.draw(|frame| ui::render(frame, &mut app_state))?;

        tokio::select! {
            // Branch 1: Terminal/keyboard events
            maybe_event = event_stream.next() => {
                if let Some(Ok(event)) = maybe_event {
                    handle_terminal_event(&mut app_state, &control_tx, event)?;
                }
            }
            // Branch 2: Agent events
            maybe_agent_event = agent_rx.recv() => {
                match maybe_agent_event {
                    Some(event) => app_state.apply_agent_event(event),
                    None => break, // Agent channel closed, agent is done
                }
            }
            // Branch 3: Render tick (forces redraw for animations, clock updates)
            _ = render_interval.tick() => {
                // Just triggers the next loop iteration for redraw
            }
        }

        if app_state.should_quit {
            break;
        }
    }

    ratatui::restore();
    Ok(())
}
```

### Pattern 3: Immediate-Mode Rendering with AppState

**What:** All TUI-visible state lives in a single `AppState` struct. Agent events are applied to mutate this state. Each frame, the entire UI is rendered from `AppState`. This follows ratatui's immediate-mode rendering model.

**When to use:** Always -- ratatui requires re-rendering all widgets every frame.

**Example:**
```rust
pub struct AppState {
    // Tab navigation
    pub active_tab: Tab,

    // Log stream (Tab 1)
    pub log_entries: Vec<LogDisplayEntry>,
    pub log_scroll_offset: usize,
    pub auto_scroll: bool,
    pub expanded_entries: HashSet<usize>, // indices of expanded tool results

    // Sub-agent tree (Tab 1)
    pub sub_agent_tree_visible: bool,
    pub sub_agents: Vec<SubAgentInfo>,

    // Discoveries (Tab 2)
    pub discoveries: Vec<Discovery>,
    pub discovery_scroll_offset: usize,

    // Status bar (always visible)
    pub agent_state: AgentState,
    pub context_pressure_pct: f64,
    pub session_number: u32,
    pub turn_count: u64,
    pub tool_call_count: u64,

    // Control state
    pub should_quit: bool,
    pub quit_confirmation_pending: bool,
}
```

### Pattern 4: Structured Log Entries

**What:** Each log entry in the stream is a typed enum rendered as a distinct visual block with header line, icon, color, and content area. Tool results are collapsed by default.

**When to use:** Rendering the log stream on Tab 1.

**Example:**
```rust
pub enum LogDisplayEntry {
    Thought {
        timestamp: String,
        turn: u64,
        content: String,
    },
    ToolCall {
        timestamp: String,
        turn: u64,
        fn_name: String,
        args_summary: String,
        result_summary: Option<String>,
        full_result: Option<String>,
        line_count: Option<usize>,
    },
    Error {
        timestamp: String,
        turn: u64,
        message: String,
    },
    SessionSeparator {
        session_number: u32,
    },
    SystemMessage {
        timestamp: String,
        content: String,
    },
}
```

### Anti-Patterns to Avoid

- **Calling agent functions from the render loop:** The TUI must never call into the agent loop directly. All communication goes through channels. Direct calls would block the render loop.
- **Multiple terminal.draw() calls per frame:** Ratatui uses double-buffering. Only the last `draw()` call per frame actually renders. Combine all widgets in one closure.
- **Storing widget state in widgets:** Ratatui widgets are ephemeral (created and consumed each frame). State must live in `AppState`, not in widget structs.
- **Blocking on agent events in the render loop:** Use `tokio::select!` or `try_recv()` -- never block. The render loop must always remain responsive to keyboard input.
- **Forgetting terminal restoration on panic:** Use `ratatui::init()` which installs a panic hook, or manually install one. A panic without restoration leaves the terminal in raw mode.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Terminal raw mode / alternate screen | Custom terminal setup/teardown | `ratatui::init()` / `ratatui::restore()` | Handles panic hooks, raw mode, alternate screen. Forgetting any step corrupts terminal state. |
| Tree view widget | Custom recursive List rendering | `tui-tree-widget` (TreeItem, Tree, TreeState) | Handles expand/collapse, indentation, selection, scrolling. Non-trivial to get right. |
| Scrollbar state management | Manual scroll position tracking | Ratatui's `Scrollbar` + `ScrollbarState` | Handles thumb size, position relative to content length, and rendering correctly. |
| Crossterm event filtering | Custom key event dedup | Filter `KeyEventKind::Press` only | Windows sends both Press and Release events. Without filtering, every keypress registers twice. |
| Context pressure visualization | Custom character-by-character progress bar | `LineGauge` widget | Single-line gauge with filled/unfilled styling, ratio input, automatic percentage label. |
| Async event multiplexing | Custom polling loop with sleep | `tokio::select!` with EventStream + mpsc + interval | Race-free, efficient multiplexing. No busy-waiting. Proper cancellation. |

**Key insight:** Ratatui's widget library and tokio's async primitives already solve the hard problems (terminal state management, event multiplexing, layout calculation, scrolling). The implementation work is in wiring data flow and rendering logic, not building infrastructure.

## Common Pitfalls

### Pitfall 1: Terminal State Corruption on Panic/Error

**What goes wrong:** If the application panics or exits without restoring the terminal, the user's shell is left in raw mode (no echo, no line editing, no Ctrl+C).
**Why it happens:** Raw mode and alternate screen are process-level terminal state. Any exit path that skips restoration corrupts the terminal.
**How to avoid:** Use `ratatui::init()` which automatically installs a panic hook. For the manual path, call `ratatui::restore()` in a panic hook AND in all exit paths (including error returns).
**Warning signs:** Terminal appears "frozen" after a crash. User has to type `reset` to recover.

### Pitfall 2: Crossterm Version Conflicts

**What goes wrong:** Pulling in crossterm as a direct dependency alongside ratatui's re-exported crossterm causes two different crossterm versions. Different versions maintain separate event queues and raw mode state, leading to lost events and broken terminal restoration.
**Why it happens:** Ratatui 0.30 depends on crossterm 0.29. Adding `crossterm = "0.28"` directly creates a conflict.
**How to avoid:** Use `ratatui::crossterm` for all crossterm types. Do not add crossterm to Cargo.toml. If event-stream feature is needed, check if ratatui re-exports it.
**Warning signs:** Compile errors about mismatched types between `crossterm::event::KeyEvent` and `ratatui::crossterm::event::KeyEvent`.

### Pitfall 3: Blocking the Render Loop

**What goes wrong:** The TUI freezes and stops responding to keyboard input while waiting for an agent event.
**Why it happens:** Using `agent_rx.recv().await` without `tokio::select!` blocks until an event arrives. If the agent is busy with a long tool call, the UI is unresponsive.
**How to avoid:** Always use `tokio::select!` to race agent events against keyboard input and render ticks. The render tick ensures the UI redraws at a minimum FPS even when no events arrive.
**Warning signs:** UI freezes during long shell executions or model inference.

### Pitfall 4: Rendering Outside Buffer Bounds

**What goes wrong:** Panic with "index out of bounds" when rendering to coordinates outside the terminal area.
**Why it happens:** Terminal can be resized at any time. Layout calculations from the previous frame may reference areas larger than the current terminal.
**How to avoid:** Use `area.intersection(buf.area)` when calculating render areas. Use ratatui's `Layout` system which inherently respects the given `Rect`. Handle `Event::Resize` to trigger a full re-render.
**Warning signs:** Panics on terminal resize, especially when making the terminal smaller.

### Pitfall 5: Auto-Scroll Fighting User Scroll

**What goes wrong:** User scrolls up to read earlier log entries, but the next agent event auto-scrolls back to the bottom, making it impossible to read history.
**Why it happens:** Naive auto-scroll implementation always sets scroll offset to the bottom on new entries.
**How to avoid:** Track an `auto_scroll: bool` flag. Set it to `false` when user scrolls up. Set it back to `true` when user explicitly "jumps to bottom" (e.g., presses End or a shortcut key). Only auto-scroll when the flag is true.
**Warning signs:** Users cannot read log history while the agent is active.

### Pitfall 6: Forgetting Immediate-Mode Rendering Rules

**What goes wrong:** Only part of the UI updates, or old content remains on screen from previous frames.
**Why it happens:** Developer assumes retained-mode behavior where only changed widgets need re-rendering. Ratatui uses immediate mode: every frame must render ALL widgets.
**How to avoid:** Always render all visible widgets in every `terminal.draw()` call. The draw closure must produce the complete UI, not just "updates."
**Warning signs:** Stale content visible after tab switches, or missing widgets after resize.

### Pitfall 7: Duplicate Key Events on Windows

**What goes wrong:** Every keypress is processed twice (once on press, once on release).
**Why it happens:** Crossterm on Windows emits both `KeyEventKind::Press` and `KeyEventKind::Release` events. macOS/Linux only emit Press.
**How to avoid:** Filter to only process `KeyEventKind::Press` events: `if key.kind == KeyEventKind::Press { ... }`.
**Warning signs:** Pause toggles on and off immediately. Tab switches flash.

## Code Examples

### Terminal Initialization with Panic Safety
```rust
// Source: ratatui 0.30 official docs
// ratatui::init() enters raw mode, alternate screen, and installs panic hook.
// ratatui::restore() reverses everything.
// ratatui::run() does both and runs a closure.

use ratatui::DefaultTerminal;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Option A: Manual init/restore (needed for async main loop)
    let mut terminal = ratatui::init();

    let result = run_app(&mut terminal).await;

    ratatui::restore();
    result
}
```

### Layout: Tabbed Interface with Status Bar
```rust
// Source: ratatui 0.30 docs - Layout, Tabs, Block

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::{Block, Borders, Tabs};
use ratatui::style::{Style, Stylize};
use ratatui::Frame;

fn render(frame: &mut Frame, state: &mut AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Tab bar
            Constraint::Min(0),    // Content area
            Constraint::Length(2), // Status bar (2 lines)
        ])
        .split(frame.area());

    // Tab bar
    let tab_titles = vec!["Agent", "Discoveries"];
    let tabs = Tabs::new(tab_titles)
        .select(state.active_tab as usize)
        .style(Style::default().white())
        .highlight_style(Style::default().yellow().bold())
        .divider("|")
        .block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(tabs, chunks[0]);

    // Content area (dispatch to active tab)
    match state.active_tab {
        Tab::Agent => render_agent_tab(frame, state, chunks[1]),
        Tab::Discoveries => render_discoveries_tab(frame, state, chunks[1]),
    }

    // Status bar (always visible)
    render_status_bar(frame, state, chunks[2]);
}
```

### Layout: Agent Tab with Toggleable Sub-Agent Panel
```rust
// Source: ratatui 0.30 docs - Layout splitting

fn render_agent_tab(frame: &mut Frame, state: &mut AppState, area: Rect) {
    if state.sub_agent_tree_visible {
        // 70% log, 30% sub-agent tree
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(70),
                Constraint::Percentage(30),
            ])
            .split(area);

        render_log_stream(frame, state, chunks[0]);
        render_sub_agent_tree(frame, state, chunks[1]);
    } else {
        // Log takes full height
        render_log_stream(frame, state, area);
    }
}
```

### Status Bar with Context Pressure Gauge
```rust
// Source: ratatui 0.30 docs - LineGauge, Layout, Span

use ratatui::widgets::LineGauge;
use ratatui::text::{Line, Span};

fn render_status_bar(frame: &mut Frame, state: &AppState, area: Rect) {
    let lines = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(area);

    // Line 1: state | context gauge | session | counters
    let status_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(12),  // Agent state
            Constraint::Length(20),  // Context pressure
            Constraint::Length(12),  // Session number
            Constraint::Min(0),     // Turn/tool counters
        ])
        .split(lines[0]);

    // Agent state indicator
    let state_style = match state.agent_state {
        AgentState::Thinking  => Style::default().yellow(),
        AgentState::Executing => Style::default().green(),
        AgentState::Idle      => Style::default().dim(),
        AgentState::Paused    => Style::default().red().bold(),
    };
    let state_label = match state.agent_state {
        AgentState::Thinking  => " Thinking ",
        AgentState::Executing => " Executing",
        AgentState::Idle      => " Idle     ",
        AgentState::Paused    => " PAUSED   ",
    };
    frame.render_widget(Span::styled(state_label, state_style), status_chunks[0]);

    // Context pressure gauge (green -> yellow -> red)
    let pressure_color = if state.context_pressure_pct < 0.5 {
        ratatui::style::Color::Green
    } else if state.context_pressure_pct < 0.8 {
        ratatui::style::Color::Yellow
    } else {
        ratatui::style::Color::Red
    };
    let gauge = LineGauge::default()
        .ratio(state.context_pressure_pct.clamp(0.0, 1.0))
        .filled_style(Style::default().fg(pressure_color))
        .label(format!("Ctx:{:.0}%", state.context_pressure_pct * 100.0));
    frame.render_widget(gauge, status_chunks[1]);

    // Line 2: Keybind hints
    let hints = Line::from(vec![
        Span::styled(" Tab", Style::default().bold()),
        Span::raw(":switch "),
        Span::styled("p", Style::default().bold()),
        Span::raw(":pause "),
        Span::styled("q", Style::default().bold()),
        Span::raw(":quit "),
        Span::styled("Up/Dn", Style::default().bold()),
        Span::raw(":scroll "),
        Span::styled("t", Style::default().bold()),
        Span::raw(":tree "),
        Span::styled("End", Style::default().bold()),
        Span::raw(":bottom"),
    ]);
    frame.render_widget(hints, lines[1]);
}
```

### Keyboard Event Handling with Windows Safety
```rust
// Source: ratatui FAQ, crossterm docs

use ratatui::crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEventKind};

fn handle_terminal_event(
    state: &mut AppState,
    control_tx: &mpsc::UnboundedSender<ControlSignal>,
    event: CrosstermEvent,
) -> anyhow::Result<()> {
    match event {
        CrosstermEvent::Key(key) if key.kind == KeyEventKind::Press => {
            match key.code {
                KeyCode::Tab => state.next_tab(),
                KeyCode::BackTab => state.prev_tab(),
                KeyCode::Char('p') => {
                    if state.agent_state == AgentState::Paused {
                        control_tx.send(ControlSignal::Resume)?;
                    } else {
                        control_tx.send(ControlSignal::Pause)?;
                    }
                }
                KeyCode::Char('q') => {
                    if state.quit_confirmation_pending {
                        state.should_quit = true;
                        control_tx.send(ControlSignal::Quit)?;
                    } else {
                        state.quit_confirmation_pending = true;
                        // Reset after a timeout or on next non-q key
                    }
                }
                KeyCode::Up => state.scroll_up(),
                KeyCode::Down => state.scroll_down(),
                KeyCode::End => state.jump_to_bottom(),
                KeyCode::Char('t') => state.toggle_sub_agent_tree(),
                _ => {
                    state.quit_confirmation_pending = false;
                }
            }
        }
        CrosstermEvent::Resize(_, _) => {
            // Ratatui handles resize automatically on next draw
        }
        _ => {}
    }
    Ok(())
}
```

### Agent Loop Integration (Sending Events to TUI)
```rust
// This shows how the existing agent_loop.rs would be modified
// to send events to the TUI instead of printing to stderr.

// In the agent loop, replace eprintln! calls with channel sends:
// Before: eprintln!("[tool] {}({})", call.fn_name, args_display);
// After:
if let Some(ref tui_tx) = tui_event_sender {
    let _ = tui_tx.send(AgentEvent::ToolCallStarted {
        timestamp: now_iso_timestamp(),
        turn,
        call_id: call_id.clone(),
        fn_name: call.fn_name.clone(),
        args_summary: args_display.clone(),
    });
}

// The tui_event_sender is an Option<mpsc::UnboundedSender<AgentEvent>>
// passed into run_agent_session. When None, the agent falls back to
// eprintln (headless mode). When Some, events go to the TUI.
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `tui-rs` crate | `ratatui` crate (community fork) | 2023 | tui-rs is unmaintained. All new projects use ratatui. |
| Manual terminal init/teardown | `ratatui::init()` / `ratatui::restore()` | ratatui 0.28.1 | Eliminates boilerplate, automatic panic hooks. |
| Separate crossterm dependency | `ratatui::crossterm` re-export | ratatui 0.27+ | Prevents version conflicts between ratatui and crossterm. |
| Monolithic ratatui crate | Workspace: ratatui-core, ratatui-widgets, etc. | ratatui 0.30 | Apps continue using `ratatui` crate. Widget libs can use `ratatui-core`. |
| `std::sync::mpsc` for async events | `tokio::sync::mpsc` | Always for tokio apps | Tokio channels integrate with `select!` and async. |
| Deprecated ratatui async-template repo | `ratatui/templates` (cargo-generate) | 2025 | The old async-template repo is deprecated. |

**Deprecated/outdated:**
- `tui-rs`: Unmaintained since 2023. Use `ratatui` instead.
- `ratatui/async-template` repo: Deprecated in favor of `ratatui/templates`.
- `Title` type for Block titles: Replaced with `Into<Line>` in ratatui 0.30.

## Open Questions

1. **Event-stream feature on re-exported crossterm**
   - What we know: Ratatui re-exports crossterm as `ratatui::crossterm`. The `EventStream` type requires crossterm's `event-stream` feature.
   - What's unclear: Whether ratatui 0.30's default features include event-stream on the re-exported crossterm, or whether we need to explicitly enable it. The `ratatui-crossterm` sub-crate may have its own feature flag.
   - Recommendation: Try `ratatui::crossterm::event::EventStream` first. If it's not available, add `crossterm = { version = "0.29", features = ["event-stream"] }` matching ratatui's version. Check at implementation time.

2. **Pause/resume control signal mechanism**
   - What we know: The agent loop currently checks a `shutdown: Arc<AtomicBool>` between turns. Pause needs a similar mechanism.
   - What's unclear: Whether to use an additional `Arc<AtomicBool>` for pause, a `tokio::sync::watch` channel, or a `tokio::sync::Notify`. The best choice depends on whether we need to block the agent loop (watch channel with `.changed().await`) or just check a flag (AtomicBool).
   - Recommendation: Use `Arc<AtomicBool>` for pause (consistent with existing shutdown pattern). Check it at the top of each turn alongside the shutdown flag. When paused, sleep briefly and re-check in a loop.

3. **tui-tree-widget 0.24 compatibility with ratatui 0.30**
   - What we know: tui-tree-widget 0.24.0 was released January 2026. Ratatui 0.30.0 was released December 2025. The tree widget depends on ratatui-core and ratatui-widgets.
   - What's unclear: Whether 0.24.0 was built against ratatui 0.30 or an earlier version.
   - Recommendation: Add the dependency and check if it compiles. If there's a version conflict, either pin to a compatible version or fall back to building a simple tree with the List widget (indented items with expand/collapse state).

4. **Graceful degradation breakpoints**
   - What we know: The CONTEXT.md specifies graceful degradation on small terminals but leaves breakpoints to Claude's discretion.
   - What's unclear: Exact minimum terminal dimensions and what hides first.
   - Recommendation: Set minimum usable size at 60 columns x 15 rows. Below that, show a "terminal too small" message. Between that and full size: first hide sub-agent tree, then hide tab bar (show only active content), then truncate status bar to essentials (state + context percentage only).

## Sources

### Primary (HIGH confidence)
- [Ratatui official documentation](https://ratatui.rs/) - Installation, FAQ, tutorials, concepts
- [Ratatui v0.30.0 release highlights](https://ratatui.rs/highlights/v030/) - New features, breaking changes, migration
- [docs.rs/ratatui 0.30.0](https://docs.rs/ratatui/latest/ratatui/) - API documentation for all widgets (Tabs, Paragraph, List, Gauge, LineGauge, Scrollbar, Block)
- [Ratatui async event stream tutorial](https://ratatui.rs/tutorials/counter-async-app/async-event-stream/) - EventHandler pattern with tokio::select!
- [Ratatui panic hooks recipe](https://ratatui.rs/recipes/apps/panic-hooks/) - Terminal restoration patterns
- [Ratatui backends documentation](https://ratatui.rs/concepts/backends/) - Crossterm re-export, version management
- [Ratatui FAQ](https://ratatui.rs/faq/) - Async guidance, rendering model, platform issues

### Secondary (MEDIUM confidence)
- [Ratatui async template structure](https://ratatui.github.io/async-template/02-structure.html) - Component architecture (deprecated repo, but patterns are sound)
- [tui-tree-widget on GitHub](https://github.com/EdJoPaTo/tui-rs-tree-widget) - v0.24.0, TreeItem/Tree/TreeState API
- [tui-tree-widget on docs.rs](https://docs.rs/tui-tree-widget/latest/tui_tree_widget/) - API docs (limited detail)
- [Ratatui GitHub releases](https://github.com/ratatui/ratatui/releases) - Version timeline, crossterm compatibility

### Tertiary (LOW confidence)
- [Ratatui forum discussion on async tasks](https://forum.ratatui.rs/t/how-do-i-run-an-async-task-and-update-ui-when-finished/129) - Community patterns
- [async-ratatui project on GitHub](https://github.com/d-holguin/async-ratatui) - TEA-inspired architecture example

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - Ratatui 0.30 is the clear choice, well documented, verified from official sources
- Architecture: HIGH - Channel-based async pattern is well established in ratatui ecosystem, documented in official tutorials and templates
- Pitfalls: HIGH - Documented in official FAQ (Windows key events, terminal restoration, immediate mode) and verified across multiple sources
- Widget selection: HIGH - All needed widgets verified in ratatui 0.30 API docs
- tui-tree-widget compatibility: MEDIUM - Version 0.24.0 exists but compatibility with ratatui 0.30 not confirmed from docs

**Research date:** 2026-02-04
**Valid until:** 2026-03-06 (30 days -- ratatui is stable, no upcoming releases announced)
