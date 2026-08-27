//! The pure domain core.
//!
//! These modules depend on nothing else in the crate: they are the value
//! types, the fault model, the experiment event vocabulary, and the pure
//! correctness report. Ports, application, and adapters all build on top.

pub mod config;
pub mod event;
pub mod fault;
pub mod oracle;
pub mod subject;
