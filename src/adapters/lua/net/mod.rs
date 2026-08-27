use mlua::{Lua, Table};

use crate::adapters::lua::LuaModule;
use crate::application::context::BindingContext;
use crate::ports::Substrate;

pub mod http;
pub mod link;
pub mod tcp;

pub struct Net;

impl<S: Substrate> LuaModule<S> for Net {
    fn register(lua: &Lua, dstest: &Table, ctx: &BindingContext<S>) -> mlua::Result<()> {
        let net_table = lua.create_table()?;
        http::register(lua, &net_table, ctx)?;
        link::register(lua, &net_table, ctx)?;
        tcp::register(lua, &net_table, ctx)?;
        dstest.set("net", net_table)?;
        Ok(())
    }
}
