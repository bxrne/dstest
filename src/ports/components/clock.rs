//! Virtualised clock control for subjects.
//!
//! A substrate implementing `ClockControl` can place a subject on a virtual
//! clock: pinned to a chosen epoch, offset from wall time, run at a scaled
//! rate, frozen, or released back to real time. The default method bodies
//! return "not supported" errors, so a substrate only overrides what it can
//! actually virtualise.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::domain::subject::Subject;

pub const NOT_SUPPORTED: &str = "virtual clocks not supported by this substrate";

/// Snapshot of a subject's virtual clock configuration.
#[derive(Clone, Copy, Debug)]
pub struct ClockState {
    /// Whether the subject is currently on a virtual clock at all.
    pub virtualised: bool,
    /// Current virtual time as unix epoch millis.
    pub epoch_millis: i64,
    /// Offset from real time, in milliseconds.
    pub offset_millis: i64,
    /// Rate multiplier relative to real time (1.0 = real time).
    pub rate: f64,
    /// Whether the clock is frozen (time does not advance).
    pub frozen: bool,
}

/// Virtual clock control for a single subject. All methods are per-subject:
/// different subjects may run on different clocks.
pub trait ClockControl: Send + Sync + 'static {
    /// Current time as seen by the subject, unix epoch millis. The default
    /// implementation reports real wall-clock time and never fails.
    async fn now(&self, subject: &Subject) -> Result<i64, String> {
        let _ = subject;
        Ok(SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis() as i64)
    }

    /// Set an absolute offset from real time.
    async fn set_offset(&self, _subject: &Subject, _offset_ms: i64) -> Result<(), String> {
        Err(NOT_SUPPORTED.to_string())
    }

    /// Advance the clock relative to its current position.
    async fn advance(&self, _subject: &Subject, _delta_ms: i64) -> Result<(), String> {
        Err(NOT_SUPPORTED.to_string())
    }

    /// Scale the rate at which virtual time passes (1.0 = real time).
    async fn set_rate(&self, _subject: &Subject, _rate: f64) -> Result<(), String> {
        Err(NOT_SUPPORTED.to_string())
    }

    /// Freeze the clock at its current value.
    async fn freeze(&self, _subject: &Subject) -> Result<(), String> {
        Err(NOT_SUPPORTED.to_string())
    }

    /// Return the subject to real wall-clock time.
    async fn release(&self, _subject: &Subject) -> Result<(), String> {
        Err(NOT_SUPPORTED.to_string())
    }

    /// Inspect the subject's clock configuration.
    async fn state(&self, _subject: &Subject) -> Result<ClockState, String> {
        Err(NOT_SUPPORTED.to_string())
    }
}
