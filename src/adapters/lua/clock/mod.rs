use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mlua::{Lua, Result, Table, UserData, UserDataMethods};

use crate::adapters::lua::LuaModule;
use crate::application::context::{BindingContext, locked_substrate};
use crate::domain::subject::Subject;
use crate::ports::Substrate;

/// Handle to a subject's virtual clock, obtained via
/// `dstest.clock.virtual(subject_id)`. All methods dispatch through the
/// substrate's `ClockControl` implementation; substrates without virtual
/// clock support return "not supported" errors.
struct VirtualClock {
    substrate: Arc<dyn Substrate>,
    subject_id: String,
}

impl VirtualClock {
    fn subject(&self) -> Subject {
        Subject::new(self.subject_id.clone())
    }
}

impl UserData for VirtualClock {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method("now", |lua, this, ()| async move {
            let millis = this
                .substrate
                .clock()
                .now(&this.subject())
                .await
                .map_err(mlua::Error::RuntimeError)?;
            let t = lua.create_table()?;
            t.set("millis", millis)?;
            t.set("secs", millis / 1000)?;
            t.set("nanos", millis * 1_000_000)?;
            t.set("micros", millis * 1_000)?;
            Ok(t)
        });

        methods.add_async_method("advance", |_, this, delta_ms: i64| async move {
            this.substrate
                .clock()
                .advance(&this.subject(), delta_ms)
                .await
                .map_err(mlua::Error::RuntimeError)
        });

        methods.add_async_method("set_offset", |_, this, offset_ms: i64| async move {
            this.substrate
                .clock()
                .set_offset(&this.subject(), offset_ms)
                .await
                .map_err(mlua::Error::RuntimeError)
        });

        methods.add_async_method("set_rate", |_, this, rate: f64| async move {
            this.substrate
                .clock()
                .set_rate(&this.subject(), rate)
                .await
                .map_err(mlua::Error::RuntimeError)
        });

        methods.add_async_method("freeze", |_, this, ()| async move {
            this.substrate
                .clock()
                .freeze(&this.subject())
                .await
                .map_err(mlua::Error::RuntimeError)
        });

        methods.add_async_method("release", |_, this, ()| async move {
            this.substrate
                .clock()
                .release(&this.subject())
                .await
                .map_err(mlua::Error::RuntimeError)
        });

        methods.add_async_method("state", |lua, this, ()| async move {
            let state: crate::ports::components::ClockState = this
                .substrate
                .clock()
                .state(&this.subject())
                .await
                .map_err(mlua::Error::RuntimeError)?;
            let t = lua.create_table()?;
            t.set("virtualised", state.virtualised)?;
            t.set("epoch_millis", state.epoch_millis)?;
            t.set("offset_millis", state.offset_millis)?;
            t.set("rate", state.rate)?;
            t.set("frozen", state.frozen)?;
            Ok(t)
        });
    }
}

pub struct Clock;

impl LuaModule for Clock {
    fn register(lua: &Lua, dstest: &Table, ctx: &BindingContext) -> Result<()> {
        let clock_table = lua.create_table()?;

        // dstest.clock() — real wall-clock time (the harness clock).
        let now_fn = lua.create_function(move |lua, _: ()| {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO);

            let t = lua.create_table()?;
            t.set("nanos", now.as_nanos() as u64)?;
            t.set("micros", now.as_micros() as u64)?;
            t.set("millis", now.as_millis() as u64)?;
            t.set("secs", now.as_secs())?;

            Ok(t)
        })?;
        clock_table.set("now", now_fn)?;

        // dstest.clock.virtual(subject_id) -> per-subject virtual clock handle.
        let substrate_slot = Arc::clone(&ctx.substrate);
        let virtual_fn = lua.create_function(move |lua, subject_id: String| {
            let substrate = locked_substrate(&substrate_slot)?;
            lua.create_userdata(VirtualClock {
                substrate,
                subject_id,
            })
        })?;
        clock_table.set("virtual", virtual_fn)?;

        // Keep the table callable: dstest.clock() == dstest.clock.now().
        let meta = lua.create_table()?;
        let now_ref: mlua::Function = clock_table.get("now")?;
        meta.set("__call", now_ref)?;
        clock_table.set_metatable(Some(meta))?;

        dstest.set("clock", clock_table)?;
        Ok(())
    }
}
