//! Sleep state machine types for agent-initiated pauses.
//!
//! The agent can pause itself via a sleep tool with three resume modes:
//! - **Timer**: Sleep for a fixed duration, then auto-wake.
//! - **Event**: Sleep until a specific sub-agent completes.
//! - **Manual**: Sleep until the user resumes from the TUI.
//!
//! This module defines the state types and argument parser. The actual
//! sleep loop integration into `agent_loop.rs` happens in a later plan.

use std::time::{Duration, Instant};

/// The mode determining how the agent wakes from sleep.
#[derive(Debug, Clone)]
pub enum SleepMode {
    /// Sleep for a fixed duration.
    Timer(Duration),
    /// Sleep until a specific sub-agent/process completes.
    Event { agent_id: String },
    /// Sleep until user manually resumes from TUI.
    Manual,
}

/// Tracks the state of an active (or recently completed) sleep.
#[derive(Debug, Clone)]
pub struct SleepState {
    /// Whether the sleep is currently active.
    pub active: bool,
    /// The mode that determines wake condition.
    pub mode: SleepMode,
    /// When the sleep started (for elapsed/remaining calculations).
    pub started_at: Instant,
    /// Maximum allowed sleep duration (clamped from config).
    pub max_duration: Duration,
    /// Set when the sleep ends, describing why the agent woke.
    pub wake_reason: Option<String>,
}

impl SleepState {
    /// Create a new active sleep state.
    pub fn new(mode: SleepMode, max_duration: Duration) -> Self {
        Self {
            active: true,
            mode,
            started_at: Instant::now(),
            max_duration,
            wake_reason: None,
        }
    }

    /// How long the agent has been sleeping.
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Human-readable remaining time or status string for TUI display.
    pub fn remaining_display(&self) -> String {
        match &self.mode {
            SleepMode::Timer(d) => {
                let elapsed = self.started_at.elapsed();
                if elapsed >= *d {
                    "Timer expired".to_string()
                } else {
                    let remaining = *d - elapsed;
                    format_duration(remaining)
                }
            }
            SleepMode::Event { agent_id } => {
                format!("Waiting: agent {agent_id}")
            }
            SleepMode::Manual => "Manual pause".to_string(),
        }
    }

    /// Whether the sleep has exceeded its maximum allowed duration.
    pub fn is_expired(&self) -> bool {
        self.started_at.elapsed() >= self.max_duration
    }

    /// Wake the agent, recording the reason.
    pub fn wake(&mut self, reason: &str) {
        self.active = false;
        self.wake_reason = Some(reason.to_string());
    }
}

/// Format a duration as a human-readable string (e.g. "2m 34s").
fn format_duration(d: Duration) -> String {
    let total_secs = d.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

/// Parse sleep tool arguments into a ready-to-use `SleepState`.
///
/// Expected JSON args:
/// - `mode`: `"timer"` | `"event"` | `"manual"` (required)
/// - `duration_secs`: integer > 0 (required for timer mode)
/// - `agent_id`: string (required for event mode)
///
/// The effective duration is clamped to `min(requested, max_sleep_duration_secs)`.
pub fn parse_sleep_args(
    args: &serde_json::Value,
    max_sleep_duration_secs: u64,
) -> Result<SleepState, String> {
    let mode_str = args
        .get("mode")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "sleep: missing required 'mode' field".to_string())?;

    let (mode, requested_secs) = match mode_str {
        "timer" => {
            let secs = args
                .get("duration_secs")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| {
                    "sleep: timer mode requires 'duration_secs' (positive integer)".to_string()
                })?;
            if secs == 0 {
                return Err("sleep: duration_secs must be greater than 0".to_string());
            }
            (SleepMode::Timer(Duration::from_secs(secs)), secs)
        }
        "event" => {
            let agent_id = args
                .get("agent_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    "sleep: event mode requires 'agent_id' (string)".to_string()
                })?;
            if agent_id.is_empty() {
                return Err("sleep: agent_id must not be empty".to_string());
            }
            // For event mode, use max as the default duration (event completes or timeout)
            (
                SleepMode::Event {
                    agent_id: agent_id.to_string(),
                },
                max_sleep_duration_secs,
            )
        }
        "manual" => {
            // Manual mode uses max duration as safety timeout
            (SleepMode::Manual, max_sleep_duration_secs)
        }
        other => {
            return Err(format!(
                "sleep: unknown mode '{other}'. Expected 'timer', 'event', or 'manual'"
            ));
        }
    };

    // Clamp effective duration to the configured maximum
    let effective_secs = requested_secs.min(max_sleep_duration_secs);
    let max_duration = Duration::from_secs(effective_secs);

    Ok(SleepState::new(mode, max_duration))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn new_sets_active_true() {
        let state = SleepState::new(SleepMode::Manual, Duration::from_secs(60));
        assert!(state.active);
        assert!(state.wake_reason.is_none());
    }

    #[test]
    fn remaining_display_timer_mode() {
        // Create a timer sleep with 120 seconds
        let state = SleepState::new(SleepMode::Timer(Duration::from_secs(120)), Duration::from_secs(120));
        let display = state.remaining_display();
        // Should show roughly "2m 0s" or "1m 59s" depending on timing
        assert!(
            display.contains('m'),
            "Timer display should contain minutes: got '{display}'"
        );
    }

    #[test]
    fn remaining_display_event_mode() {
        let state = SleepState::new(
            SleepMode::Event {
                agent_id: "abc123".to_string(),
            },
            Duration::from_secs(3600),
        );
        assert_eq!(state.remaining_display(), "Waiting: agent abc123");
    }

    #[test]
    fn remaining_display_manual_mode() {
        let state = SleepState::new(SleepMode::Manual, Duration::from_secs(3600));
        assert_eq!(state.remaining_display(), "Manual pause");
    }

    #[test]
    fn is_expired_returns_true_after_max_duration() {
        // Use a very short max_duration so it expires immediately
        let state = SleepState::new(SleepMode::Manual, Duration::from_millis(1));
        // Small sleep to ensure expiration
        std::thread::sleep(Duration::from_millis(5));
        assert!(state.is_expired());
    }

    #[test]
    fn is_expired_returns_false_before_max_duration() {
        let state = SleepState::new(SleepMode::Manual, Duration::from_secs(3600));
        assert!(!state.is_expired());
    }

    #[test]
    fn wake_sets_active_false_and_reason() {
        let mut state = SleepState::new(SleepMode::Manual, Duration::from_secs(60));
        assert!(state.active);

        state.wake("user_resumed");
        assert!(!state.active);
        assert_eq!(state.wake_reason.as_deref(), Some("user_resumed"));
    }

    #[test]
    fn parse_sleep_args_valid_timer() {
        let args = json!({"mode": "timer", "duration_secs": 30});
        let state = parse_sleep_args(&args, 3600).expect("should parse");
        assert!(state.active);
        assert!(matches!(state.mode, SleepMode::Timer(d) if d == Duration::from_secs(30)));
        assert_eq!(state.max_duration, Duration::from_secs(30));
    }

    #[test]
    fn parse_sleep_args_valid_event() {
        let args = json!({"mode": "event", "agent_id": "sub-001"});
        let state = parse_sleep_args(&args, 3600).expect("should parse");
        assert!(state.active);
        assert!(
            matches!(&state.mode, SleepMode::Event { agent_id } if agent_id == "sub-001")
        );
    }

    #[test]
    fn parse_sleep_args_valid_manual() {
        let args = json!({"mode": "manual"});
        let state = parse_sleep_args(&args, 3600).expect("should parse");
        assert!(state.active);
        assert!(matches!(state.mode, SleepMode::Manual));
    }

    #[test]
    fn parse_sleep_args_missing_mode_returns_error() {
        let args = json!({"duration_secs": 30});
        let result = parse_sleep_args(&args, 3600);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing required 'mode'"));
    }

    #[test]
    fn parse_sleep_args_timer_missing_duration_returns_error() {
        let args = json!({"mode": "timer"});
        let result = parse_sleep_args(&args, 3600);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("duration_secs"));
    }

    #[test]
    fn parse_sleep_args_timer_zero_duration_returns_error() {
        let args = json!({"mode": "timer", "duration_secs": 0});
        let result = parse_sleep_args(&args, 3600);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("greater than 0"));
    }

    #[test]
    fn parse_sleep_args_event_missing_agent_id_returns_error() {
        let args = json!({"mode": "event"});
        let result = parse_sleep_args(&args, 3600);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("agent_id"));
    }

    #[test]
    fn parse_sleep_args_event_empty_agent_id_returns_error() {
        let args = json!({"mode": "event", "agent_id": ""});
        let result = parse_sleep_args(&args, 3600);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("must not be empty"));
    }

    #[test]
    fn parse_sleep_args_unknown_mode_returns_error() {
        let args = json!({"mode": "hibernate"});
        let result = parse_sleep_args(&args, 3600);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown mode"));
    }

    #[test]
    fn parse_sleep_args_clamps_to_max_duration() {
        // Request 7200s but max is 3600s
        let args = json!({"mode": "timer", "duration_secs": 7200});
        let state = parse_sleep_args(&args, 3600).expect("should parse");

        // The max_duration should be clamped to 3600
        assert_eq!(state.max_duration, Duration::from_secs(3600));

        // The timer mode keeps the original requested duration for display purposes
        // but max_duration is what controls expiry
        assert!(matches!(state.mode, SleepMode::Timer(d) if d == Duration::from_secs(7200)));
    }

    #[test]
    fn format_duration_hours() {
        assert_eq!(format_duration(Duration::from_secs(3661)), "1h 1m 1s");
    }

    #[test]
    fn format_duration_minutes() {
        assert_eq!(format_duration(Duration::from_secs(154)), "2m 34s");
    }

    #[test]
    fn format_duration_seconds_only() {
        assert_eq!(format_duration(Duration::from_secs(42)), "42s");
    }

    #[test]
    fn format_duration_zero() {
        assert_eq!(format_duration(Duration::from_secs(0)), "0s");
    }

    #[test]
    fn elapsed_increases_over_time() {
        let state = SleepState::new(SleepMode::Manual, Duration::from_secs(3600));
        let e1 = state.elapsed();
        std::thread::sleep(Duration::from_millis(5));
        let e2 = state.elapsed();
        assert!(e2 > e1);
    }
}
