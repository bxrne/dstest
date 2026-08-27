mod opts;
mod setup;

use mlua::Lua;

use crate::adapters::lua::LuaModule;
use crate::application::context::BindingContext;
use crate::ports::Substrate;

pub struct Core;

impl<S: Substrate> LuaModule<S> for Core {
    fn register(lua: &Lua, dstest: &mlua::Table, ctx: &BindingContext<S>) -> mlua::Result<()> {
        opts::register(lua, dstest, ctx)?;
        setup::register(lua, dstest, ctx)?;
        Ok(())
    }
}
