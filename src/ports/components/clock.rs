//! Virtualised clock control for subjects.
//!
//! A substrate implementing `ClockControl` can place a subject on a virtual
//! clock: pinned to a chosen epoch, offset from wall time, run at a scaled
//! rate, frozen, or released back to real time. The default method bodies
//! return "not supported" errors, so a substrate only overrides what it can
//! actually virtualise.

use std::future::Future;
use std::pin::Pin;
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
/// different subjects may run on different clocks. Async methods return boxed
/// pinned futures so the trait stays object-safe (`&dyn ClockControl`), since
/// `Substrate::clock()` hands out trait objects.
pub trait ClockControl: Send + Sync + 'static {
    /// Current time as seen by the subject, unix epoch millis. The default
    /// implementation reports real wall-clock time and never fails.
    fn now<'a>(
        &'a self,
        subject: &'a Subject,
    ) -> Pin<Box<dyn Future<Output = Result<i64, String>> + Send + 'a>> {
        Box::pin(async move {
            let _ = subject;
            Ok(SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_millis() as i64)
        })
    }

    /// Set an absolute offset from real time.
    fn set_offset<'a>(
        &'a self,
        _subject: &'a Subject,
        _offset_ms: i64,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move { Err(NOT_SUPPORTED.to_string()) })
    }

    /// Advance the clock relative to its current position.
    fn advance<'a>(
        &'a self,
        _subject: &'a Subject,
        _delta_ms: i64,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move { Err(NOT_SUPPORTED.to_string()) })
    }

    /// Scale the rate at which virtual time passes (1.0 = real time).
    fn set_rate<'a>(
        &'a self,
        _subject: &'a Subject,
        _rate: f64,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move { Err(NOT_SUPPORTED.to_string()) })
    }

    /// Freeze the clock at its current value.
    fn freeze<'a>(
        &'a self,
        _subject: &'a Subject,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move { Err(NOT_SUPPORTED.to_string()) })
    }

    /// Return the subject to real wall-clock time.
    fn release<'a>(
        &'a self,
        _subject: &'a Subject,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move { Err(NOT_SUPPORTED.to_string()) })
    }

    /// Inspect the subject's clock configuration.
    fn state<'a>(
        &'a self,
        _subject: &'a Subject,
    ) -> Pin<Box<dyn Future<Output = Result<ClockState, String>> + Send + 'a>> {
        Box::pin(async move { Err(NOT_SUPPORTED.to_string()) })
    }
}
