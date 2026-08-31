use mlua::Lua;

use crate::adapters::lua::LuaModule;
use crate::application::context::BindingContext;

pub mod command;
pub mod pool;

pub struct Pg;

impl LuaModule for Pg {
    fn register(lua: &Lua, dstest: &mlua::Table, ctx: &BindingContext) -> mlua::Result<()> {
        let pg_table = lua.create_table()?;
        command::register(lua, &pg_table, ctx)?;
        dstest.set("pg", pg_table)?;
        Ok(())
    }
}
