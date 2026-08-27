//! The binding context: everything a Lua binding needs to reach the app.
//!
//! This is a single, owned handle into the application state rather than a
//! grab-bag of unrelated mutexes. The substrate port, the check runner, and
//! the composed [`AppState`] all live behind it.

use std::sync::{Arc, Mutex};

use mlua::Lua;

use crate::application::oracle::CheckRunner;
use crate::application::state::AppState;
use crate::ports::Substrate;

pub struct BindingContext<S: Substrate> {
    pub state: Arc<Mutex<AppState<S>>>,
    pub oracle: Arc<Mutex<CheckRunner>>,
    pub substrate: Arc<S>,
    /// Seeded workload RNG for `dstest.random.*` (separate stream from the
    /// fault tree's RNG, so workload draws don't affect the fault schedule).
    pub workload_rng: Arc<Mutex<Option<rand::rngs::StdRng>>>,
    pub lua: Lua,
}

impl<S: Substrate> BindingContext<S> {
    pub fn new(substrate: S) -> Self {
        let substrate = Arc::new(substrate);
        Self {
            state: Arc::new(Mutex::new(AppState::default())),
            oracle: Arc::new(Mutex::new(CheckRunner::new())),
            substrate,
            workload_rng: Arc::new(Mutex::new(None)),
            lua: Lua::new(),
        }
    }
}
