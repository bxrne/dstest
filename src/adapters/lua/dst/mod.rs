use mlua::{Lua, Table};

use crate::adapters::lua::LuaModule;
use crate::application::BindingContext;

mod clear;
mod common;
mod oracle;
mod run_steps;
mod step;

pub struct Dst;

impl LuaModule for Dst {
    fn register(lua: &Lua, dstest: &Table, ctx: &BindingContext) -> mlua::Result<()> {
        let dst_table = lua.create_table()?;
        clear::register(lua, &dst_table, ctx)?;
        oracle::register(lua, &dst_table, ctx)?;
        step::register(lua, &dst_table, ctx)?;
        run_steps::register(lua, &dst_table, ctx)?;
        dstest.set("dst", dst_table)?;
        Ok(())
    }
}
