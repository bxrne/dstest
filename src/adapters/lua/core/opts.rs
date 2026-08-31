use std::sync::Arc;

use mlua::{Lua, Result, Table};

use crate::application::context::{BindingContext, resolve_substrate};
use crate::domain::config::Config;
use crate::domain::event::ExperimentEvent;

pub fn register(lua: &Lua, dstest: &Table, ctx: &BindingContext) -> Result<()> {
    let state = Arc::clone(&ctx.state);
    let resolver = Arc::clone(&ctx.resolver);
    let substrate_slot = Arc::clone(&ctx.substrate);
    let workload_rng = Arc::clone(&ctx.workload_rng);

    let config_fn = lua.create_function(move |lua, tbl: Table| {
        let state = Arc::clone(&state);
        let resolver = Arc::clone(&resolver);
        let substrate_slot = Arc::clone(&substrate_slot);
        let workload_rng = Arc::clone(&workload_rng);
        let mut cfg = Config::default();

        let name: Option<String> = tbl.get("name").ok();

        // Resolve the substrate from the script's declaration. This is the
        // single point where a concrete substrate is chosen at runtime; no
        // backend is touched until `setup` actually uses it.
        let resolved = match tbl.get::<String>("substrate") {
            Ok(s) => {
                let resolved = resolve_substrate(&resolver, &substrate_slot, &s)?;
                cfg.substrate = Some(s);
                Some(resolved)
            }
            Err(_) => None,
        };

        if let Ok(seed) = tbl.get::<u64>("seed") {
            cfg.seed = Some(seed);
            // Seed Lua's own PRNG too, so math.random() is reproducible.
            let globals = lua.globals();
            let math: Table = globals.get("math")?;
            let randomseed: mlua::Function = math.get("randomseed")?;
            randomseed.call::<()>(seed)?;
            if let Some(substrate) = &resolved {
                // Seed the network impairment RNG.
                substrate.network().set_seed(seed);
                // Seed storage fault randomness (e.g. corrupt offsets).
                substrate.storage().set_seed(seed);
            }
            // Seed the workload RNG (separate stream from the fault tree).
            *workload_rng.lock().expect("poisoned rng lock") =
                Some(rand::SeedableRng::seed_from_u64(seed));
        }

        if let Ok(weights) = tbl.get::<Table>("weights") {
            let mut fault_weights = std::collections::BTreeMap::new();
            for (k, v) in weights.pairs::<String, f32>().flatten() {
                fault_weights.insert(k, v);
            }
            cfg.fault_weights = fault_weights;
        }

        if let Ok(mode) = tbl.get::<String>("accumulation") {
            cfg.accumulation_mode = mode
                .parse()
                .map_err(|e: String| mlua::Error::RuntimeError(e))?;
        }

        if let Ok(timeout) = tbl.get::<u64>("http_timeout") {
            cfg.http_timeout_secs = timeout;
        }

        if let Ok(retries) = tbl.get::<u32>("http_retries") {
            cfg.http_retries = retries;
        }

        if let Ok(delay) = tbl.get::<u64>("http_retry_delay") {
            cfg.http_retry_delay_ms = delay;
        }

        if let Ok(delay) = tbl.get::<u64>("step_delay") {
            cfg.step_delay_ms = delay;
        }

        if let Ok(steps) = tbl.get::<usize>("steps") {
            cfg.steps = steps;
        }

        if let Ok(require) = tbl.get::<bool>("require_seed") {
            cfg.require_seed = require;
        }

        if cfg.substrate.is_none() {
            return Err(mlua::Error::RuntimeError(
                "dstest.config requires a `substrate` field".to_string(),
            ));
        }

        cfg.validate()
            .map_err(|e| mlua::Error::RuntimeError(format!("invalid configuration: {}", e)))?;
        cfg.normalize_weights();

        let mut state = state.lock().expect("poisoned engine state lock");

        // Auto-generate a unique handle when none was given.
        let handle = match name {
            Some(n) => n,
            None => state.configs.unique_handle(),
        };

        if state.configs.contains(&handle) {
            return Err(mlua::Error::RuntimeError(format!(
                "config '{}' already exists",
                handle
            )));
        }

        state.configs.register(handle.clone(), cfg);
        state.log.push(ExperimentEvent::ConfigRegistered);

        Ok(handle)
    })?;

    dstest.set("config", config_fn)?;
    Ok(())
}
