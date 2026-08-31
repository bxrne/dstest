//! Lua bindings: the adapter layer that renders the application API to a
//! Lua script surface.
//!
//! These are deliberately thin. Each binding calls an application use case or
//! reaches a port, then renders the structured result to a Lua table. They
//! never mutate raw application state or implement domain logic.

pub mod clock;
pub mod core;
pub mod dst;
pub mod log;
pub mod net;
pub mod pg;
pub mod random;
pub mod storage;
pub mod subs;
pub mod workload;

use mlua::{Lua, Table};

use crate::application::BindingContext;

pub trait LuaModule {
    fn register(lua: &Lua, dstest: &Table, ctx: &BindingContext) -> mlua::Result<()>;
}

/// Register every Lua module against the `dstest` global created by the
/// engine. Called from the composition root.
pub fn register_all(lua: &Lua, ctx: &BindingContext) -> mlua::Result<()> {
    let globals = lua.globals();
    let dstest: Table = globals.get("dstest")?;

    net::Net::register(lua, &dstest, ctx)?;
    dst::Dst::register(lua, &dstest, ctx)?;
    subs::Subs::register(lua, &dstest, ctx)?;
    log::Log::register(lua, &dstest, ctx)?;
    clock::Clock::register(lua, &dstest, ctx)?;
    core::Core::register(lua, &dstest, ctx)?;
    pg::Pg::register(lua, &dstest, ctx)?;
    storage::Storage::register(lua, &dstest, ctx)?;
    random::Random::register(lua, &dstest, ctx)?;
    workload::Workload::register(lua, &dstest, ctx)?;

    Ok(())
}
