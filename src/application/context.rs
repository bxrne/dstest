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
    state: Arc<Mutex<AppState>>,
    oracle: Arc<Mutex<CheckRunner>>,
    /// Runtime substrate resolver injected by the composition root. Only
    /// used to build a substrate once the script declares one.
    resolver: Arc<dyn SubstrateResolver>,
    /// The resolved substrate (once `dstest.config()` declares one). Held in
    /// an `RwLock` because `config` runs at script time, after registration.
    substrate: Arc<RwLock<Option<Arc<dyn Substrate>>>>,
    /// Seeded workload RNG for `dstest.random.*` (separate stream from the
    /// fault tree's RNG, so workload draws don't affect the fault schedule).
    workload_rng: Arc<Mutex<Option<rand::rngs::StdRng>>>,
    /// Shared HTTP client reused across `dstest.net.http` and the HTTP
    /// workload. `reqwest::Client` pools connections internally; building one
    /// per request discards that pool and pays TLS/connection setup each call.
    /// Per-request timeouts are applied on the `RequestBuilder`, so the
    /// client itself carries no global timeout.
    http: reqwest::Client,
    lua: Lua,
}

impl BindingContext {
    /// The application state handle.
    pub fn state(&self) -> &Arc<Mutex<AppState>> {
        &self.state
    }

    /// The oracle check runner handle.
    pub fn oracle(&self) -> &Arc<Mutex<CheckRunner>> {
        &self.oracle
    }

    /// The injected substrate resolver (composition root).
    pub fn resolver(&self) -> &Arc<dyn SubstrateResolver> {
        &self.resolver
    }

    /// The substrate resolution slot (a captured `Arc` clone for Lua
    /// closures).
    pub fn substrate_slot(&self) -> &Arc<RwLock<Option<Arc<dyn Substrate>>>> {
        &self.substrate
    }

    /// The seeded workload RNG slot (a captured `Arc` clone for Lua
    /// closures).
    pub fn workload_rng(&self) -> &Arc<Mutex<Option<rand::rngs::StdRng>>> {
        &self.workload_rng
    }

    /// The shared HTTP client (cloned cheaply into closures).
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// The backing Lua state.
    pub fn lua(&self) -> &Lua {
        &self.lua
    }

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
