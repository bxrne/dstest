use std::sync::Arc;

use mlua::{Lua, Result, Table};

use crate::application::context::{BindingContext, locked_substrate};
use crate::domain::subject::Subject;

pub fn register(lua: &Lua, dstest: &Table, ctx: &BindingContext) -> Result<()> {
    let substrate = Arc::clone(&ctx.substrate);

    let exec_fn = lua.create_async_function(move |lua, (id, cmd): (String, Vec<String>)| {
        let substrate = Arc::clone(&substrate);

        async move {
            let substrate = locked_substrate(&substrate)?;
            let subject = Subject::new(id);

            let result = substrate
                .exec(&subject, &cmd)
                .await
                .map_err(mlua::Error::RuntimeError)?;

            let t = lua.create_table()?;
            t.set("exit_code", result.exit_code)?;
            t.set("stdout", result.stdout)?;
            t.set("stderr", result.stderr)?;

            Ok(t)
        }
    })?;

    dstest.set("exec", exec_fn)?;
    Ok(())
}
