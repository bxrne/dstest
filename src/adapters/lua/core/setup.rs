use std::sync::Arc;

use mlua::{Lua, Result, Table};

use crate::application::context::BindingContext;
use crate::application::usecases::setup;
use crate::ports::Substrate;

pub fn register<S: Substrate>(_lua: &Lua, dstest: &Table, ctx: &BindingContext<S>) -> Result<()> {
    let state = Arc::clone(&ctx.state);
    let substrate = Arc::clone(&ctx.substrate);

    let setup_fn =
        _lua.create_async_function(move |_, (handle, config_tbl): (String, Table)| {
            let state = Arc::clone(&state);
            let substrate = Arc::clone(&substrate);
            async move { setup::setup(&state, &substrate, handle, config_tbl).await }
        })?;

    dstest.set("setup", setup_fn)?;
    Ok(())
}
