//! Ports: the interfaces the application and adapters build against.
//!
//! These are the seams. They depend only on the domain (value types, fault
//! model) and carry no implementation. Concrete substrates implement them.

pub mod components;
pub mod substrate;

pub use substrate::{Substrate, SubstrateFactory, SubstrateResolver, ToLua};
