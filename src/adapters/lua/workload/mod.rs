pub mod http;
pub mod pg;

use mlua::{Lua, Table};

use crate::adapters::lua::LuaModule;
use crate::application::context::BindingContext;

pub struct Workload;

impl LuaModule for Workload {
    fn register(lua: &Lua, dstest: &Table, ctx: &BindingContext) -> mlua::Result<()> {
        let workload_table = lua.create_table()?;
        http::register(lua, &workload_table, ctx)?;
        pg::register(lua, &workload_table, ctx)?;
        dstest.set("workload", workload_table)?;
        Ok(())
    }
}
