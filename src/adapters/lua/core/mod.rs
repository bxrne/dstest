mod opts;
mod setup;

use mlua::Lua;

use crate::adapters::lua::LuaModule;
use crate::application::context::BindingContext;

pub struct Core;

impl LuaModule for Core {
    fn register(lua: &Lua, dstest: &mlua::Table, ctx: &BindingContext) -> mlua::Result<()> {
        opts::register(lua, dstest, ctx)?;
        setup::register(lua, dstest, ctx)?;
        Ok(())
    }
}
