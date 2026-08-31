use mlua::Lua;

use crate::adapters::lua::LuaModule;
use crate::application::context::BindingContext;

mod exec;
mod inspect;
mod logs;

pub struct Subs;

impl LuaModule for Subs {
    fn register(lua: &Lua, dstest: &mlua::Table, ctx: &BindingContext) -> mlua::Result<()> {
        exec::register(lua, dstest, ctx)?;
        inspect::register(lua, dstest, ctx)?;
        logs::register(lua, dstest, ctx)?;
        Ok(())
    }
}
