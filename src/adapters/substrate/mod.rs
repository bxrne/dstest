//! Substrate adapters.
//!
//! Each concrete substrate registers a [`SubstrateFactory`] into the
//! [`SubstrateRegistry`], which the composition root injects as a
//! [`SubstrateResolver`]. The runtime dispatches on the script's declared
//! `substrate` field, so no substrate is built (or its backend connected)
//! until the script actually asks for it.

pub mod docker;

use std::sync::Arc;

use crate::ports::{Substrate, SubstrateFactory, SubstrateResolver};

/// A name-keyed registry of substrate factories for runtime dispatch. The
/// only place that knows every concrete substrate.
#[derive(Default)]
pub struct SubstrateRegistry {
    factories: Vec<Box<dyn SubstrateFactory>>,
}

impl SubstrateRegistry {
    pub fn new() -> Self {
        Self {
            factories: Vec::new(),
        }
    }

    /// Register a substrate factory for runtime dispatch by its name.
    pub fn register(mut self, factory: Box<dyn SubstrateFactory>) -> Self {
        self.factories.push(factory);
        self
    }

    /// Names of every registered substrate, for error messages.
    pub fn registered_names(&self) -> Vec<&'static str> {
        self.factories.iter().map(|f| f.name()).collect()
    }
}

impl SubstrateResolver for SubstrateRegistry {
    fn resolve(&self, name: &str) -> Result<Arc<dyn Substrate>, String> {
        self.factories
            .iter()
            .find(|f| f.name() == name)
            .ok_or_else(|| {
                format!(
                    "unknown substrate '{}' (registered: {})",
                    name,
                    self.registered_names().join(", ")
                )
            })
            .and_then(|f| f.build())
    }
}
