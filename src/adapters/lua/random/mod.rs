//! `dstest.random` — seeded reproducible randomness for workload generation.
//!
//! All draws come from a single `StdRng` seeded by the first config's seed,
//! independent of the fault tree's RNG stream (so workload draws don't
//! perturb the fault schedule). Lua is single-threaded, so the stream is
//! deterministic for a given seed + call order.

use std::sync::Arc;

use mlua::{Lua, Result, Table, Value};
use rand::Rng;

use crate::adapters::lua::LuaModule;
use crate::application::context::BindingContext;

pub struct Random;

impl LuaModule for Random {
    fn register(lua: &Lua, dstest: &Table, ctx: &BindingContext) -> Result<()> {
        let rng = Arc::clone(ctx.workload_rng());

        let random_table = lua.create_table()?;

        let rng1 = Arc::clone(&rng);
        let int_fn = lua.create_function(move |_, (min, max): (i64, i64)| {
            let mut guard = rng1.lock().expect("poisoned rng lock");
            match guard.as_mut() {
                Some(r) => Ok(r.r#gen_range(min..max)),
                None => Err(mlua::Error::RuntimeError(
                    "dstest.random requires a seed: call dstest.config({ seed = n }) first".into(),
                )),
            }
        })?;
        random_table.set("int", int_fn)?;

        let rng2 = Arc::clone(&rng);
        let float_fn = lua.create_function(move |_, _: ()| {
            let mut guard = rng2.lock().expect("poisoned rng lock");
            match guard.as_mut() {
                Some(r) => Ok(r.r#gen::<f64>()),
                None => Err(mlua::Error::RuntimeError(
                    "dstest.random requires a seed: call dstest.config({ seed = n }) first".into(),
                )),
            }
        })?;
        random_table.set("float", float_fn)?;

        let rng3 = Arc::clone(&rng);
        let bool_fn = lua.create_function(move |_, p: Option<f64>| {
            let p = p.unwrap_or(0.5);
            if !(0.0..=1.0).contains(&p) {
                return Err(mlua::Error::RuntimeError(format!(
                    "probability must be 0.0–1.0, got {}",
                    p
                )));
            }
            let mut guard = rng3.lock().expect("poisoned rng lock");
            match guard.as_mut() {
                Some(r) => Ok(r.r#gen::<f64>() < p),
                None => Err(mlua::Error::RuntimeError(
                    "dstest.random requires a seed: call dstest.config({ seed = n }) first".into(),
                )),
            }
        })?;
        random_table.set("bool", bool_fn)?;

        let rng4 = Arc::clone(&rng);
        let choice_fn = lua.create_function(move |_, tbl: Table| {
            let len = tbl.raw_len();
            if len == 0 {
                return Ok(Value::Nil);
            }
            let idx = {
                let mut guard = rng4.lock().expect("poisoned rng lock");
                match guard.as_mut() {
                    Some(r) => r.r#gen_range(1..=len),
                    None => {
                        return Err(mlua::Error::RuntimeError(
                            "dstest.random requires a seed: call dstest.config({ seed = n }) first"
                                .into(),
                        ));
                    }
                }
            };
            let val: Value = tbl.get(idx)?;
            Ok(val)
        })?;
        random_table.set("choice", choice_fn)?;

        let rng5 = Arc::clone(&rng);
        let shuffle_fn = lua.create_function(move |_, tbl: Table| {
            let len = tbl.raw_len();
            if len < 2 {
                return Ok(());
            }
            let mut guard = rng5.lock().expect("poisoned rng lock");
            let rng = match guard.as_mut() {
                Some(r) => r,
                None => {
                    return Err(mlua::Error::RuntimeError(
                        "dstest.random requires a seed: call dstest.config({ seed = n }) first"
                            .into(),
                    ));
                }
            };
            for i in (1..len).rev() {
                let j = rng.r#gen_range(1..=i);
                let a: Value = tbl.get(i)?;
                let b: Value = tbl.get(j)?;
                tbl.set(i, b)?;
                tbl.set(j, a)?;
            }
            Ok(())
        })?;
        random_table.set("shuffle", shuffle_fn)?;

        dstest.set("random", random_table)?;
        Ok(())
    }
}
