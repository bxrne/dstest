use std::sync::Arc;

use mlua::{Lua, MultiValue, Result, Table, Value};

use crate::adapters::lua::dst::common::render_step;
use crate::application::context::{BindingContext, locked_substrate};
use crate::application::usecases::run_step;

pub fn register(_lua: &Lua, dstest: &Table, ctx: &BindingContext) -> Result<()> {
    let state = Arc::clone(ctx.state());
    let oracle = Arc::clone(ctx.oracle());
    let substrate = Arc::clone(ctx.substrate_slot());

    let run_steps_fn = _lua.create_async_function(move |lua, args: MultiValue| {
        let state = Arc::clone(&state);
        let oracle = Arc::clone(&oracle);
        let substrate = Arc::clone(&substrate);

        async move {
            let substrate = locked_substrate(&substrate)?;

            // run_steps(n) or run_steps(cfg_handle, n)
            let mut args = args.into_iter();
            let (cfg, n) = match (args.next(), args.next()) {
                (Some(Value::Integer(n)), None) => (None, n as usize),
                (Some(Value::String(s)), Some(Value::Integer(n))) => {
                    (Some(s.to_str()?.to_owned()), n as usize)
                }
                _ => {
                    return Err(mlua::Error::RuntimeError(
                        "dstest.dst.run_steps expects (n) or (config_handle, n)".to_string(),
                    ));
                }
            };

            let mut results = Vec::new();
            for _ in 0..n {
                let Some(outcome) =
                    run_step::run_step(&lua, &state, &oracle, &substrate, cfg.clone()).await?
                else {
                    break; // fault schedule exhausted
                };

                results.push(render_step(&lua, &outcome)?);
            }

            let result_table = lua.create_table()?;
            for (i, t) in results.into_iter().enumerate() {
                result_table.set(i + 1, t)?;
            }

            Ok(result_table)
        }
    })?;

    dstest.set("run_steps", run_steps_fn)?;
    Ok(())
}
