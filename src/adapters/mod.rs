//! Adapters: infrastructure implementations behind the ports.
//!
//! Nothing in the application or domain layer knows these exist. Concrete
//! substrate implementations and the Lua bindings live here.

pub mod lua;
pub mod substrate;
