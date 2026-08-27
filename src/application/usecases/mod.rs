//! Application use cases: the orchestration of a chaos run.
//!
//! These are the concrete operations a Lua script drives. They own the
//! domain logic and emit [`ExperimentEvent`](crate::domain::event::ExperimentEvent)s
//! into the event log; the Lua bindings are thin shells around them.

pub mod run_step;
pub mod setup;
pub mod teardown;
