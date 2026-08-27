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

use crate::domain::fault::Fault;
use crate::domain::subject::{ExecResult, HostedSubject, LogEntry, Subject, SubjectStatus};
use crate::ports::components::{ClockControl, NetworkControl, StorageControl};

/// Render a substrate-specific value onto a Lua value. Each substrate owns
/// the shape of its `inspect` result (and any other associated type it
/// surfaces to Lua) and implements this trait so the bindings stay generic
/// over `S: Substrate`.
pub trait ToLua {
    fn to_lua(self, lua: &mlua::Lua) -> mlua::Result<mlua::Value>;
}

/// A substrate hosts subjects and virtualises their environment.
pub trait Substrate: Send + Sync + 'static {
    /// Human-readable substrate name (e.g. `"docker"`), matched against the
    /// `substrate` field of `dstest.config()`.
    const NAME: &'static str;

    /// Substrate-specific data describing how to host a subject.
    type SubjectData: Clone + Send + Sync + 'static;

    /// Substrate-specific inspect result, surfaced to Lua via [`ToLua`].
    type Inspect: ToLua + Send + 'static;

    /// Substrate-specific log-query options, parsed from the optional Lua
    /// table passed to `dstest.logs`.
    type LogOpts: Default + Send + Sync + 'static;

    /// Virtualised clock implementation.
    type Clock: ClockControl;
    /// Network link control implementation.
    type Network: NetworkControl;
    /// Virtual storage implementation.
    type Storage: StorageControl;

    /// Parse the Lua table from `dstest.setup()` into this substrate's
    /// `SubjectData`. Each substrate owns its own config schema — Docker
    /// reads `image`/`ports`/`volumes`/`env`/`cmd`, other substrates can
    /// read whatever fields they need.
    fn parse_subject(&self, table: &mlua::Table) -> Result<Self::SubjectData, String>;

    /// Parse the optional Lua table from `dstest.logs` into this
    /// substrate's `LogOpts`. `None` means no options table was passed;
    /// the substrate should fall back to [`Default`].
    fn parse_log_opts(&self, table: Option<&mlua::Table>) -> Result<Self::LogOpts, String>;

    /// Pull/create/start a subject and return its instance id plus an
    /// optional reachable address. `name` is the engine-assigned subject
    /// name (unique per experiment); substrates should use it to name
    /// their resources so leaked resources are identifiable and cleanable.
    async fn host(&self, name: &str, data: &Self::SubjectData) -> Result<HostedSubject, String>;

    async fn affect(&self, subject: &Subject, fault: &Fault) -> Result<(), String>;
    async fn clear_faults(&self, subject: &Subject) -> Result<(), String>;
    async fn teardown(&self, subject: Subject) -> Result<(), String>;

    async fn logs(&self, subject: &Subject, opts: Self::LogOpts) -> Result<Vec<LogEntry>, String>;
    async fn inspect(&self, subject: &Subject) -> Result<Self::Inspect, String>;
    async fn exec(&self, subject: &Subject, cmd: &[String]) -> Result<ExecResult, String>;
    async fn status(&self, subject: &Subject) -> Result<SubjectStatus, String>;

    fn clock(&self) -> &Self::Clock;
    fn network(&self) -> &Self::Network;
    fn storage(&self) -> &Self::Storage;
}
