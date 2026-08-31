//! The substrate port.
//!
//! A substrate hosts subjects and virtualises their environment. Everything
//! async here runs on the engine's single tokio runtime; the engine and Lua
//! adapters contain no substrate-specific code. Virtualised components
//! (clock, network, storage) are exposed through the
//! [`ClockControl`]/[`NetworkControl`]/[`StorageControl`] traits — never
//! through substrate-specific types leaking into the engine — so new
//! substrates plug in by implementing this trait plus whichever component
//! traits they can support.
//!
//! # Runtime dispatch
//!
//! This trait is deliberately object-safe (`dyn Substrate`): the engine holds
//! an `Arc<dyn Substrate>` rather than a monomorphised concrete type, so the
//! substrate is chosen at runtime from the script's declared `substrate`
//! field. Async methods therefore return boxed pinned futures (the standard
//! object-safe representation), and substrate-typed data crosses the `dyn`
//! boundary as a boxed `Any` that the producing substrate downcasts again.

use std::any::Any;
use std::future::Future;
use std::pin::Pin;

use crate::domain::fault::Fault;
use crate::domain::subject::{ExecResult, HostedSubject, LogEntry, Subject, SubjectStatus};
use crate::ports::components::{ClockControl, NetworkControl, StorageControl};

/// Render a substrate-specific value onto a Lua value. Each substrate owns
/// the shape of its `inspect` result (and any other associated type it
/// surfaces to Lua) and implements this trait so the bindings stay substrate
/// agnostic. Consumes `self` by value; implemented for boxed trait objects so
/// an `Arc<dyn Substrate>` can hand it to Lua directly.
pub trait ToLua {
    fn to_lua(self: Box<Self>, lua: &mlua::Lua) -> mlua::Result<mlua::Value>;
}

/// A substrate hosts subjects and virtualises their environment.
pub trait Substrate: Send + Sync + 'static {
    /// Human-readable substrate name (e.g. `"docker"`), matched against the
    /// `substrate` field of `dstest.config()`. Also the subject-id prefix.
    fn name(&self) -> &'static str;

    /// Parse the Lua table from `dstest.setup()` into this substrate's
    /// `SubjectData`. Each substrate owns its own config schema — Docker
    /// reads `image`/`ports`/`volumes`/`env`/`cmd`, other substrates can
    /// read whatever fields they need. The parsed data is returned opaque
    /// and handed back to [`Substrate::host`], which downcasts it.
    fn parse_subject(&self, table: &mlua::Table) -> Result<Box<dyn Any + Send + Sync>, String>;

    /// Parse the optional Lua table from `dstest.logs` into this substrate's
    /// `LogOpts`. `None` means no options table was passed. Returned opaque
    /// and handed back to [`Substrate::logs`].
    fn parse_log_opts(
        &self,
        table: Option<&mlua::Table>,
    ) -> Result<Box<dyn Any + Send + Sync>, String>;

    /// Pull/create/start a subject and return its instance id plus an
    /// optional reachable address. `name` is the engine-assigned subject
    /// name (unique per experiment); substrates should use it to name
    /// their resources so leaked resources are identifiable and cleanable.
    ///
    /// The `data` reference is `dyn Any + Sync` so the returned future can
    /// be `Send` while borrowing the parsed (already `Send + Sync`) data.
    fn host<'a>(
        &'a self,
        name: &'a str,
        data: &'a (dyn Any + Sync),
    ) -> Pin<Box<dyn Future<Output = Result<HostedSubject, String>> + Send + 'a>>;

    fn affect<'a>(
        &'a self,
        subject: &'a Subject,
        fault: &'a Fault,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

    fn clear_faults<'a>(
        &'a self,
        subject: &'a Subject,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

    fn teardown<'a>(
        &'a self,
        subject: Subject,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

    fn logs<'a>(
        &'a self,
        subject: &'a Subject,
        opts: Box<dyn Any + Send + Sync>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<LogEntry>, String>> + Send + 'a>>;

    fn inspect<'a>(
        &'a self,
        subject: &'a Subject,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn ToLua + Send>, String>> + Send + 'a>>;

    fn exec<'a>(
        &'a self,
        subject: &'a Subject,
        cmd: &'a [String],
    ) -> Pin<Box<dyn Future<Output = Result<ExecResult, String>> + Send + 'a>>;

    fn status<'a>(
        &'a self,
        subject: &'a Subject,
    ) -> Pin<Box<dyn Future<Output = Result<SubjectStatus, String>> + Send + 'a>>;

    fn clock(&self) -> &dyn ClockControl;
    fn network(&self) -> &dyn NetworkControl;
    fn storage(&self) -> &dyn StorageControl;
}

/// A runtime-resolvable substrate constructor. Each concrete substrate
/// exposes a factory (e.g. [`Docker::factory`]) so the composition root can
/// register it for runtime dispatch without knowing its concrete type.
pub trait SubstrateFactory: Send + Sync + 'static {
    /// The substrate's config name, as declared in `dstest.config()`.
    fn name(&self) -> &'static str;
    /// Build a fresh object-safe substrate. `Docker::new()` connects lazily,
    /// so building a substrate does not touch the backend until first use.
    fn build(&self) -> Result<std::sync::Arc<dyn Substrate>, String>;
}

/// Resolves a substrate by its config-declared name.
pub trait SubstrateResolver: Send + Sync + 'static {
    fn resolve(&self, name: &str) -> Result<std::sync::Arc<dyn Substrate>, String>;
}
