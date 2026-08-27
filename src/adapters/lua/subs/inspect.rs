use std::sync::Arc;

use mlua::{Lua, Result, Table, Value};

use crate::application::context::BindingContext;
use crate::domain::subject::Subject;
use crate::ports::{Substrate, ToLua};

pub fn register<S: Substrate>(lua: &Lua, dstest: &Table, ctx: &BindingContext<S>) -> Result<()> {
    let substrate = Arc::clone(&ctx.substrate);
    let state = Arc::clone(&ctx.state);

    let inspect_fn = lua.create_async_function(move |lua, id: String| {
        let substrate = Arc::clone(&substrate);
        let state = Arc::clone(&state);

        async move {
            let subject = Subject::new(id.clone());

            let info = substrate
                .inspect(&subject)
                .await
                .map_err(mlua::Error::RuntimeError)?;

            let value = info.to_lua(&lua)?;

            // Surface the host-reachable address (host:port) when the substrate
            // mapped container ports to the host. On setups where the bridge IP
            // is not directly reachable from the host (e.g. a podman/docker
            // machine VM), this is the only address the host process can dial.
            if let Value::Table(ref t) = value
                && let Some(host) = state
                    .lock()
                    .expect("poisoned engine state lock")
                    .subjects
                    .host_for(&id)
                    .map(str::to_owned)
            {
                t.set("host", host)?;
            }

            Ok(value)
        }
    })?;

    dstest.set("inspect", inspect_fn)?;
    Ok(())
}
