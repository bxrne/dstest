use std::sync::Arc;

use mlua::{Lua, Result, Table};

use crate::adapters::lua::dst::common::render_step;
use crate::application::context::BindingContext;
use crate::application::usecases::run_step;
use crate::ports::Substrate;

pub fn register<S: Substrate>(_lua: &Lua, dstest: &Table, ctx: &BindingContext<S>) -> Result<()> {
    let state = Arc::clone(&ctx.state);
    let oracle = Arc::clone(&ctx.oracle);
    let substrate = Arc::clone(&ctx.substrate);

    let step_fn = _lua.create_async_function(move |lua, cfg: Option<String>| {
        let state = Arc::clone(&state);
        let oracle = Arc::clone(&oracle);
        let substrate = Arc::clone(&substrate);

        async move {
            match run_step::run_step(&lua, &state, &oracle, &substrate, cfg).await? {
                Some(outcome) => render_step(&lua, &outcome),
                None => {
                    let t = lua.create_table()?;
                    t.set("more", false)?;
                    Ok(t)
                }
            }
        }
    })?;

    dstest.set("step", step_fn)?;
    Ok(())
}
