//! Fault-injectable storage for subjects.
//!
//! A substrate implementing `StorageControl` gives subjects a virtual disk
//! that can be errored, slowed, corrupted, filled, snapshotted, and restored —
//! the FoundationDB-style crash-consistency toolbox.

use std::future::Future;
use std::pin::Pin;

use crate::domain::subject::Subject;

pub const NOT_SUPPORTED: &str = "storage control not supported by this substrate";

/// Options for attaching a virtual disk to a subject.
#[derive(Clone, Debug)]
pub struct StorageOpts {
    /// Disk capacity in mebibytes; reaching it yields ENOSPC in the subject.
    pub size_mb: u64,
    /// Mount point inside the subject (substrate-specific interpretation).
    pub mount: String,
}

impl StorageOpts {
    pub fn validate(&self) -> Result<(), String> {
        if self.size_mb == 0 {
            return Err("storage: size_mb must be > 0".to_string());
        }
        if !self.mount.starts_with('/') {
            return Err("storage: mount must be an absolute path".to_string());
        }
        Ok(())
    }
}

/// Fault-injectable virtual disk control, per subject.
pub trait StorageControl: Send + Sync + 'static {
    /// Set the root seed for deterministic operations (e.g. `corrupt` offsets).
    /// Called by the engine when a config with a seed is registered.
    fn set_seed(&self, _seed: u64) {}

    /// Attach a virtual disk to a subject. Must be called while the subject
    /// is being set up or while it tolerates a new mount appearing.
    fn attach<'a>(
        &'a self,
        _subject: &'a Subject,
        _opts: StorageOpts,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move { Err(NOT_SUPPORTED.to_string()) })
    }

    /// Toggle deterministic I/O errors (EIO) on the subject's virtual disk.
    fn error<'a>(
        &'a self,
        _subject: &'a Subject,
        _on: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move { Err(NOT_SUPPORTED.to_string()) })
    }

    /// Toggle dropping of writes (acknowledged but never persisted).
    fn drop_writes<'a>(
        &'a self,
        _subject: &'a Subject,
        _on: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move { Err(NOT_SUPPORTED.to_string()) })
    }

    /// Impose per-operation latency on the virtual disk.
    fn slow<'a>(
        &'a self,
        _subject: &'a Subject,
        _delay_ms: u64,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move { Err(NOT_SUPPORTED.to_string()) })
    }

    /// Corrupt `n` bytes of the virtual disk (bit rot).
    fn corrupt<'a>(
        &'a self,
        _subject: &'a Subject,
        _n: u64,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move { Err(NOT_SUPPORTED.to_string()) })
    }

    /// Snapshot the virtual disk; returns an opaque snapshot id.
    fn snapshot<'a>(
        &'a self,
        _subject: &'a Subject,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>> {
        Box::pin(async move { Err(NOT_SUPPORTED.to_string()) })
    }

    /// Restore a previously taken snapshot (e.g. after a kill, for
    /// crash-consistency testing).
    fn restore<'a>(
        &'a self,
        _subject: &'a Subject,
        _snapshot_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move { Err(NOT_SUPPORTED.to_string()) })
    }
}
