use std::sync::Arc;

use mlua::{Lua, Result, Table};

use crate::application::context::{BindingContext, locked_substrate};
use crate::application::usecases::setup;

pub fn register(_lua: &Lua, dstest: &Table, ctx: &BindingContext) -> Result<()> {
    let state = Arc::clone(ctx.state());
    let substrate = Arc::clone(ctx.substrate_slot());

    let setup_fn =
        _lua.create_async_function(move |_, (handle, config_tbl): (String, Table)| {
            let state = Arc::clone(&state);
            let substrate = Arc::clone(&substrate);
            async move {
                let substrate = locked_substrate(&substrate)?;
                setup::setup(&state, &substrate, handle, config_tbl).await
            }
        })?;

    dstest.set("setup", setup_fn)?;
    Ok(())
}
