//! Keyboard event handler for the TUI.
//!
//! Maps key events to [`AppState`] mutations and control signals. Called by the
//! main loop in [`super::runner`] whenever a keyboard event arrives from the
//! crossterm `EventStream`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tokio::sync::mpsc::UnboundedSender;

use super::app_state::AppState;
use super::event::{AgentState, ControlSignal};

/// Process a keyboard event, mutating app state and optionally sending control signals.
///
/// Returns `true` if the application should exit (confirmed quit).
///
/// Only `KeyEventKind::Press` events are processed. This avoids duplicate handling
/// on Windows where key-up events would otherwise trigger actions twice.
pub fn handle_key_event(
    key: KeyEvent,
    state: &mut AppState,
    control_tx: &UnboundedSender<ControlSignal>,
    pause_flag: &Arc<AtomicBool>,
) -> bool {
    // Filter: only handle key press events (not release/repeat).
    if key.kind != KeyEventKind::Press {
        return false;
    }

    // -- Quit confirmation mode: intercept keys before normal handling.
    if state.quit_pending {
        return match key.code {
            KeyCode::Char('q') | KeyCode::Char('y') => {
                // Confirmed quit.
                let _ = control_tx.send(ControlSignal::Quit);
                true
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                // Cancel quit.
                state.quit_pending = false;
                false
            }
            _ => {
                // Any other key cancels quit.
                state.quit_pending = false;
                false
            }
        };
    }

    // -- Normal key handling.
    match key.code {
        KeyCode::Tab => {
            state.active_tab = (state.active_tab + 1) % 2;
        }
        KeyCode::BackTab => {
            // Shift+Tab: cycle tabs backward.
            state.active_tab = (state.active_tab + 2 - 1) % 2;
        }
        KeyCode::Up => {
            state.scroll_up();
        }
        KeyCode::Down => {
            state.scroll_down();
        }
        KeyCode::Char('p') => {
            // Toggle pause/resume.
            if state.agent_state == AgentState::Paused {
                pause_flag.store(false, Ordering::SeqCst);
                let _ = control_tx.send(ControlSignal::Resume);
            } else {
                pause_flag.store(true, Ordering::SeqCst);
                let _ = control_tx.send(ControlSignal::Pause);
            }
        }
        KeyCode::Char('e') => {
            // Toggle expand on entry at current scroll offset.
            state.toggle_expand(state.log_scroll_offset);
        }
        KeyCode::Char('g') | KeyCode::End => {
            state.jump_to_bottom();
        }
        KeyCode::Char('q') => {
            // First press: enter quit confirmation mode.
            state.quit_pending = true;
        }
        KeyCode::Char('r') => {
            // Resume a sleeping agent by clearing the pause flag.
            if state.agent_state == AgentState::Sleeping {
                pause_flag.store(false, Ordering::SeqCst);
                let _ = control_tx.send(ControlSignal::Resume);
            }
        }
        KeyCode::Char('t') => {
            // Toggle sub-agent panel visibility.
            state.sub_agent_panel_visible = !state.sub_agent_panel_visible;
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Ctrl+C: immediate quit (no confirmation needed).
            let _ = control_tx.send(ControlSignal::Quit);
            return true;
        }
        _ => {
            // Unbound key: no action.
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    /// Helper to create a KeyEvent for a regular key press.
    fn key_press(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }
    }

    /// Helper to create a KeyEvent with modifiers.
    fn key_press_with(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }
    }

    /// Helper to create a KeyEvent for a key release (should be ignored).
    fn key_release(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Release,
            state: KeyEventState::empty(),
        }
    }

    fn setup() -> (AppState, UnboundedSender<ControlSignal>, Arc<AtomicBool>) {
        let state = AppState::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let pause = Arc::new(AtomicBool::new(false));
        (state, tx, pause)
    }

    #[test]
    fn release_events_are_ignored() {
        let (mut state, tx, pause) = setup();
        let result = handle_key_event(key_release(KeyCode::Char('q')), &mut state, &tx, &pause);
        assert!(!result);
        assert!(!state.quit_pending);
    }

    #[test]
    fn tab_cycles_forward() {
        let (mut state, tx, pause) = setup();
        assert_eq!(state.active_tab, 0);

        handle_key_event(key_press(KeyCode::Tab), &mut state, &tx, &pause);
        assert_eq!(state.active_tab, 1);

        handle_key_event(key_press(KeyCode::Tab), &mut state, &tx, &pause);
        assert_eq!(state.active_tab, 0);
    }

    #[test]
    fn backtab_cycles_backward() {
        let (mut state, tx, pause) = setup();
        assert_eq!(state.active_tab, 0);

        // BackTab from 0 should wrap to 1.
        handle_key_event(key_press(KeyCode::BackTab), &mut state, &tx, &pause);
        assert_eq!(state.active_tab, 1);

        handle_key_event(key_press(KeyCode::BackTab), &mut state, &tx, &pause);
        assert_eq!(state.active_tab, 0);
    }

    #[test]
    fn up_down_scrolls() {
        let (mut state, tx, pause) = setup();
        // Add entries so scroll has something to move through.
        for i in 0..5 {
            state.apply_event(crate::tui::event::AgentEvent::ThoughtText {
                timestamp: format!("t{i}"),
                turn: i as u64,
                content: format!("entry {i}"),
            });
        }

        let initial = state.log_scroll_offset;
        handle_key_event(key_press(KeyCode::Up), &mut state, &tx, &pause);
        assert!(state.log_scroll_offset < initial || initial == 0);
        assert!(!state.auto_scroll);

        handle_key_event(key_press(KeyCode::Down), &mut state, &tx, &pause);
    }

    #[test]
    fn quit_confirmation_flow() {
        let (mut state, tx, pause) = setup();

        // First 'q': enter quit pending.
        let result = handle_key_event(key_press(KeyCode::Char('q')), &mut state, &tx, &pause);
        assert!(!result);
        assert!(state.quit_pending);

        // 'n': cancel quit.
        let result = handle_key_event(key_press(KeyCode::Char('n')), &mut state, &tx, &pause);
        assert!(!result);
        assert!(!state.quit_pending);

        // 'q' then 'y': confirm quit.
        handle_key_event(key_press(KeyCode::Char('q')), &mut state, &tx, &pause);
        assert!(state.quit_pending);
        let result = handle_key_event(key_press(KeyCode::Char('y')), &mut state, &tx, &pause);
        assert!(result);
    }

    #[test]
    fn quit_pending_second_q_confirms() {
        let (mut state, tx, pause) = setup();
        handle_key_event(key_press(KeyCode::Char('q')), &mut state, &tx, &pause);
        let result = handle_key_event(key_press(KeyCode::Char('q')), &mut state, &tx, &pause);
        assert!(result);
    }

    #[test]
    fn escape_cancels_quit() {
        let (mut state, tx, pause) = setup();
        handle_key_event(key_press(KeyCode::Char('q')), &mut state, &tx, &pause);
        assert!(state.quit_pending);

        let result = handle_key_event(key_press(KeyCode::Esc), &mut state, &tx, &pause);
        assert!(!result);
        assert!(!state.quit_pending);
    }

    #[test]
    fn any_key_cancels_quit_pending() {
        let (mut state, tx, pause) = setup();
        handle_key_event(key_press(KeyCode::Char('q')), &mut state, &tx, &pause);
        assert!(state.quit_pending);

        // 'x' is not a confirming key; should cancel.
        let result = handle_key_event(key_press(KeyCode::Char('x')), &mut state, &tx, &pause);
        assert!(!result);
        assert!(!state.quit_pending);
    }

    #[test]
    fn toggle_expand_at_scroll_offset() {
        let (mut state, tx, pause) = setup();
        state.apply_event(crate::tui::event::AgentEvent::ToolCallStarted {
            timestamp: "t".into(),
            turn: 1,
            call_id: "c".into(),
            fn_name: "f".into(),
            args_summary: "a".into(),
        });
        assert!(!state.log_entries[0].expanded);

        handle_key_event(key_press(KeyCode::Char('e')), &mut state, &tx, &pause);
        assert!(state.log_entries[0].expanded);

        handle_key_event(key_press(KeyCode::Char('e')), &mut state, &tx, &pause);
        assert!(!state.log_entries[0].expanded);
    }

    #[test]
    fn g_jumps_to_bottom() {
        let (mut state, tx, pause) = setup();
        for i in 0..10 {
            state.apply_event(crate::tui::event::AgentEvent::ThoughtText {
                timestamp: format!("t{i}"),
                turn: i as u64,
                content: format!("entry {i}"),
            });
        }
        state.scroll_up();
        state.scroll_up();
        assert!(!state.auto_scroll);

        handle_key_event(key_press(KeyCode::Char('g')), &mut state, &tx, &pause);
        assert!(state.auto_scroll);
        assert_eq!(state.log_scroll_offset, 9);
    }

    #[test]
    fn end_jumps_to_bottom() {
        let (mut state, tx, pause) = setup();
        for i in 0..5 {
            state.apply_event(crate::tui::event::AgentEvent::ThoughtText {
                timestamp: format!("t{i}"),
                turn: i as u64,
                content: format!("entry {i}"),
            });
        }
        state.scroll_up();

        handle_key_event(key_press(KeyCode::End), &mut state, &tx, &pause);
        assert!(state.auto_scroll);
    }

    #[test]
    fn t_toggles_sub_agent_panel() {
        let (mut state, tx, pause) = setup();
        assert!(state.sub_agent_panel_visible);

        handle_key_event(key_press(KeyCode::Char('t')), &mut state, &tx, &pause);
        assert!(!state.sub_agent_panel_visible);

        handle_key_event(key_press(KeyCode::Char('t')), &mut state, &tx, &pause);
        assert!(state.sub_agent_panel_visible);
    }

    #[test]
    fn pause_sends_control_signal() {
        let (mut state, _tx, pause) = setup();
        let (real_tx, mut real_rx) = tokio::sync::mpsc::unbounded_channel();

        handle_key_event(key_press(KeyCode::Char('p')), &mut state, &real_tx, &pause);
        assert!(pause.load(Ordering::SeqCst));

        let signal = real_rx.try_recv().unwrap();
        assert!(matches!(signal, ControlSignal::Pause));
    }

    #[test]
    fn resume_when_paused() {
        let (mut state, _tx, pause) = setup();
        state.agent_state = AgentState::Paused;
        pause.store(true, Ordering::SeqCst);

        let (real_tx, mut real_rx) = tokio::sync::mpsc::unbounded_channel();
        handle_key_event(key_press(KeyCode::Char('p')), &mut state, &real_tx, &pause);
        assert!(!pause.load(Ordering::SeqCst));

        let signal = real_rx.try_recv().unwrap();
        assert!(matches!(signal, ControlSignal::Resume));
    }

    #[test]
    fn ctrl_c_quits_immediately() {
        let (mut state, _tx, pause) = setup();
        let (real_tx, mut real_rx) = tokio::sync::mpsc::unbounded_channel();

        let result = handle_key_event(
            key_press_with(KeyCode::Char('c'), KeyModifiers::CONTROL),
            &mut state,
            &real_tx,
            &pause,
        );
        assert!(result);

        let signal = real_rx.try_recv().unwrap();
        assert!(matches!(signal, ControlSignal::Quit));
    }
}
