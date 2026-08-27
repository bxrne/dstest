use mlua::Lua;

use crate::adapters::lua::LuaModule;
use crate::application::context::BindingContext;
use crate::ports::Substrate;

mod exec;
mod inspect;
mod logs;

pub struct Subs;

impl<S: Substrate> LuaModule<S> for Subs {
    fn register(lua: &Lua, dstest: &mlua::Table, ctx: &BindingContext<S>) -> mlua::Result<()> {
        exec::register(lua, dstest, ctx)?;
        inspect::register(lua, dstest, ctx)?;
        logs::register(lua, dstest, ctx)?;
        Ok(())
    }
}
