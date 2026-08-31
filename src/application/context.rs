//! The binding context: everything a Lua binding needs to reach the app.
//!
//! This is a single, owned handle into the application state rather than a
//! grab-bag of unrelated mutexes. The substrate is resolved at runtime from
//! the script's declared `substrate` field via the injected
//! [`SubstrateResolver`], so the engine never knows which concrete substrate
//! runs until the script says so. The check runner and the composed
//! [`AppState`] all live behind it.

use std::sync::{Arc, Mutex, RwLock};

use mlua::Lua;

use crate::application::oracle::CheckRunner;
use crate::application::state::AppState;
use crate::ports::{Substrate, SubstrateResolver};

pub struct BindingContext {
    pub state: Arc<Mutex<AppState>>,
    pub oracle: Arc<Mutex<CheckRunner>>,
    /// Runtime substrate resolver injected by the composition root. Only
    /// used to build a substrate once the script declares one.
    pub resolver: Arc<dyn SubstrateResolver>,
    /// The resolved substrate (once `dstest.config()` declares one). Held in
    /// an `RwLock` because `config` runs at script time, after registration.
    pub substrate: Arc<RwLock<Option<Arc<dyn Substrate>>>>,
    /// Seeded workload RNG for `dstest.random.*` (separate stream from the
    /// fault tree's RNG, so workload draws don't affect the fault schedule).
    pub workload_rng: Arc<Mutex<Option<rand::rngs::StdRng>>>,
    /// Shared HTTP client reused across `dstest.net.http` and the HTTP
    /// workload. `reqwest::Client` pools connections internally; building one
    /// per request discards that pool and pays TLS/connection setup each call.
    /// Per-request timeouts are applied on the `RequestBuilder`, so the
    /// client itself carries no global timeout.
    pub http: reqwest::Client,
    pub lua: Lua,
}

impl BindingContext {
    pub fn new(resolver: Arc<dyn SubstrateResolver>) -> Self {
        Self {
            state: Arc::new(Mutex::new(AppState::default())),
            oracle: Arc::new(Mutex::new(CheckRunner::new())),
            resolver,
            substrate: Arc::new(RwLock::new(None)),
            workload_rng: Arc::new(Mutex::new(None)),
            http: reqwest::Client::new(),
            lua: Lua::new(),
        }
    }

    /// Get the currently resolved substrate, or an error if no
    /// `dstest.config()` with a supported `substrate` has run yet.
    pub fn substrate(&self) -> mlua::Result<Arc<dyn Substrate>> {
        self.substrate
            .read()
            .expect("poisoned substrate lock")
            .clone()
            .ok_or_else(|| {
                mlua::Error::RuntimeError(
                    "no substrate resolved: call dstest.config({ substrate = \"...\" }) with a supported substrate first"
                        .to_string(),
                )
            })
    }
}

/// Read the resolved substrate from a captured slot inside a Lua closure
/// that does not hold the whole [`BindingContext`], failing if none has been
/// resolved yet.
pub fn locked_substrate(
    slot: &Arc<RwLock<Option<Arc<dyn Substrate>>>>,
) -> mlua::Result<Arc<dyn Substrate>> {
    slot.read()
        .expect("poisoned substrate lock")
        .clone()
        .ok_or_else(|| {
            mlua::Error::RuntimeError(
                "no substrate resolved: call dstest.config({ substrate = \"...\" }) with a supported substrate first"
                    .to_string(),
            )
        })
}

/// Resolve and cache a substrate into a captured slot, from a captured
/// resolver. Used by Lua closures that do not own the whole
/// [`BindingContext`]. The first resolution fixes the substrate for the whole
/// run; a later call declaring a different substrate is an error.
pub fn resolve_substrate(
    resolver: &Arc<dyn SubstrateResolver>,
    slot: &Arc<RwLock<Option<Arc<dyn Substrate>>>>,
    name: &str,
) -> mlua::Result<Arc<dyn Substrate>> {
    let mut s = slot.write().expect("poisoned substrate lock");
    if let Some(existing) = s.as_ref() {
        if existing.name() != name {
            return Err(mlua::Error::RuntimeError(format!(
                "substrate already resolved to '{}'; cannot use '{}' in the same run",
                existing.name(),
                name
            )));
        }
        return Ok(Arc::clone(existing));
    }
    let substrate = resolver.resolve(name).map_err(mlua::Error::RuntimeError)?;
    *s = Some(Arc::clone(&substrate));
    Ok(substrate)
}
