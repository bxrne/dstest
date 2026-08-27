//! The application layer.
//!
//! This is the engine: it owns the composed state, the event log and its
//! projections, the check runner, and the use cases a Lua script drives. It
//! depends only on the domain and the ports; it knows nothing about concrete
//! adapters.

pub mod context;
pub mod engine;
pub mod log;
pub mod oracle;
pub mod state;
pub mod usecases;

// The context is the owned handle bindings reach into; `Engine` is what the
// composition root drives. Other modules are reached through their own paths.
pub use context::BindingContext;
pub use engine::Engine;
