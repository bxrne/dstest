pub mod http;
pub mod pg;

use mlua::{Lua, Table};

use crate::adapters::lua::LuaModule;
use crate::application::context::BindingContext;
use crate::ports::Substrate;

pub struct Workload;

impl<S: Substrate> LuaModule<S> for Workload {
    fn register(lua: &Lua, dstest: &Table, ctx: &BindingContext<S>) -> mlua::Result<()> {
        let workload_table = lua.create_table()?;
        http::register(lua, &workload_table, ctx)?;
        pg::register(lua, &workload_table, ctx)?;
        dstest.set("workload", workload_table)?;
        Ok(())
    }
}
