//! TUI main loop: terminal lifecycle, event multiplexing, and render tick.
//!
//! [`run_tui`] is the entry point for TUI mode. It initializes the terminal,
//! spawns the agent loop as a background task, and runs a `tokio::select!` loop
//! that multiplexes agent events, keyboard input, and render ticks.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::EventStream;
use futures::StreamExt;
use genai::chat::ChatMessage;

use crate::agent::agent_loop::{run_agent_session, ShutdownReason};
use crate::config::AppConfig;
use crate::safety::SafetyLayer;
use crate::tui::app_state::AppState;
use crate::tui::event::{AgentEvent, ControlSignal};
use crate::tui::input::handle_key_event;
use crate::tui::ui::render_ui;

/// Run the TUI dashboard.
///
/// Initializes the terminal, spawns the agent loop as a background tokio task,
/// and enters the main loop that multiplexes:
/// 1. Agent events (from the mpsc channel)
/// 2. Keyboard input (from crossterm EventStream)
/// 3. Render ticks (~20fps)
///
/// The terminal is properly restored on both normal exit and panic.
pub async fn run_tui(
    config: &AppConfig,
    _safety: &SafetyLayer,
    shutdown: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    // -- Initialize terminal (raw mode + alternate screen + panic hook).
    let mut terminal = ratatui::init();

    // -- Create channels for agent -> TUI communication.
    let (event_tx, mut event_rx) =
        tokio::sync::mpsc::unbounded_channel::<AgentEvent>();
    let (control_tx, _control_rx) =
        tokio::sync::mpsc::unbounded_channel::<ControlSignal>();
    let pause_flag = Arc::new(AtomicBool::new(false));

    // -- Create application state.
    let mut app_state = AppState::new();

    // -- Create async keyboard event stream.
    let mut key_stream = EventStream::new();

    // -- Spawn the agent loop as a background task.
    let config_clone = config.clone();
    let shutdown_clone = shutdown.clone();
    let pause_clone = pause_flag.clone();
    let event_tx_clone = event_tx.clone();

    tokio::spawn(async move {
        // Create a fresh SafetyLayer for the spawned task (SafetyLayer is not Clone).
        let safety = match SafetyLayer::new(&config_clone) {
            Ok(s) => s,
            Err(e) => {
                let _ = event_tx_clone.send(AgentEvent::Error {
                    timestamp: String::new(),
                    turn: 0,
                    message: format!("Failed to initialize safety layer: {e}"),
                });
                return;
            }
        };

        let mut session_number: u32 = 1;
        let mut carryover_messages: Vec<ChatMessage> = Vec::new();

        loop {
            let result = run_agent_session(
                &config_clone,
                &safety,
                session_number,
                &carryover_messages,
                shutdown_clone.clone(),
                Some(event_tx_clone.clone()),
                Some(pause_clone.clone()),
            )
            .await;

            match result {
                Ok(session_result) => match session_result.shutdown_reason {
                    ShutdownReason::ContextFull {
                        carryover_messages: carry,
                    } => {
                        // Check max_restarts.
                        if let Some(max) = config_clone.max_restarts {
                            if session_number >= max {
                                break;
                            }
                        }
                        if shutdown_clone.load(Ordering::SeqCst) {
                            break;
                        }
                        session_number += 1;
                        carryover_messages = carry;
                    }
                    ShutdownReason::UserShutdown
                    | ShutdownReason::MaxTurnsOrError(_) => break,
                },
                Err(_) => break,
            }
        }
    });

    // -- Main render/event loop.
    let tick_rate = Duration::from_millis(50); // ~20fps
    let mut tick_interval = tokio::time::interval(tick_rate);

    loop {
        tokio::select! {
            // Agent events from the background task.
            Some(event) = event_rx.recv() => {
                app_state.apply_event(event);
                if app_state.auto_scroll {
                    app_state.jump_to_bottom();
                }
            }

            // Keyboard events from crossterm.
            Some(Ok(crossterm_event)) = key_stream.next() => {
                if let crossterm::event::Event::Key(key) = crossterm_event {
                    let should_quit = handle_key_event(
                        key,
                        &mut app_state,
                        &control_tx,
                        &pause_flag,
                    );
                    if should_quit {
                        shutdown.store(true, Ordering::SeqCst);
                        break;
                    }
                }
                // Resize events are handled automatically by ratatui on next draw.
            }

            // Render tick.
            _ = tick_interval.tick() => {
                terminal.draw(|frame| {
                    render_ui(&app_state, frame);
                })?;
            }
        }
    }

    // -- Cleanup: restore terminal state.
    ratatui::restore();

    Ok(())
}
