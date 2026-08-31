use std::sync::Arc;

use mlua::{Lua, Result, Table};

use crate::application::context::{BindingContext, locked_substrate};
use crate::domain::subject::{Stream, Subject};

pub fn register(lua: &Lua, dstest: &Table, ctx: &BindingContext) -> Result<()> {
    let substrate = Arc::clone(ctx.substrate_slot());

    let logs_fn = lua.create_async_function(move |lua, (id, opts): (String, Option<Table>)| {
        let substrate = Arc::clone(&substrate);

        async move {
            let substrate = locked_substrate(&substrate)?;
            let subject = Subject::new(id);

            let log_opts = substrate
                .parse_log_opts(opts.as_ref())
                .map_err(mlua::Error::RuntimeError)?;

            let entries = substrate
                .logs(&subject, log_opts)
                .await
                .map_err(mlua::Error::RuntimeError)?;

            let result = lua.create_table()?;
            for (i, entry) in entries.into_iter().enumerate() {
                let t = lua.create_table()?;
                t.set(
                    "stream",
                    match entry.stream {
                        Stream::StdOut => "stdout",
                        Stream::StdErr => "stderr",
                    },
                )?;
                t.set("message", entry.message)?;
                result.set(i + 1, t)?;
            }

            Ok(result)
        }
    })?;

    dstest.set("logs", logs_fn)?;
    Ok(())
}
