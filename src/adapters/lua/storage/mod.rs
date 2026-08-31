//! `dstest.storage` — fault-injectable virtual disk control.
//!
//! Dispatches through the substrate's `StorageControl` implementation.
//! Substrates without virtual storage return "not supported" errors.
//!
//! # Setup (Docker)
//!
//! Opt in at subject creation — the disk is prepared before the container
//! starts and bind-mounted at `mount`:
//!
//! ```lua
//! local s = dstest.setup(cfg, {
//!     image = "alpine:3.20",
//!     cmd = { "sleep", "300" },
//!     storage = { flaky = true, mount = "/data", size_mb = 64 },
//! })
//! ```
//!
//! Requires root on the host (loop devices + device-mapper `dm-flakey`).
//!
//! # Runtime API
//!
//! | Function | Effect |
//! |----------|--------|
//! | `dstest.storage.error(id, on)` | Toggle EIO on all I/O |
//! | `dstest.storage.drop_writes(id, on)` | ACK writes but discard them |
//! | `dstest.storage.corrupt(id, n)` | Flip `n` bytes (seeded) |
//! | `dstest.storage.snapshot(id)` | Snapshot backing store → id |
//! | `dstest.storage.restore(id, snap)` | Restore a snapshot |
//! | `dstest.storage.slow(id, ms)` | Not supported on dm-flakey |
//! | `dstest.storage.attach(id, opts)` | Rejected — configure at setup |
//!
//! `corrupt` offsets are deterministic given the experiment `seed`.

use std::sync::Arc;

use mlua::{Lua, Result, Table};

use crate::adapters::lua::LuaModule;
use crate::application::context::{BindingContext, locked_substrate};
use crate::domain::subject::Subject;
use crate::ports::components::StorageOpts;

pub struct Storage;

impl LuaModule for Storage {
    fn register(lua: &Lua, dstest: &Table, ctx: &BindingContext) -> Result<()> {
        let storage_table = lua.create_table()?;

        let substrate_slot = Arc::clone(ctx.substrate_slot());
        let attach_fn = lua.create_async_function(move |_, (id, opts): (String, Table)| {
            let substrate = Arc::clone(&substrate_slot);
            async move {
                let substrate = locked_substrate(&substrate)?;
                let size_mb: u64 = opts.get("size_mb").unwrap_or(512);
                let mount: String = opts.get("mount").map_err(|_| {
                    mlua::Error::RuntimeError("storage.attach requires a `mount` field".to_string())
                })?;
                let opts = StorageOpts { size_mb, mount };
                opts.validate().map_err(mlua::Error::RuntimeError)?;
                substrate
                    .storage()
                    .attach(&Subject::new(id), opts)
                    .await
                    .map_err(mlua::Error::RuntimeError)
            }
        })?;
        storage_table.set("attach", attach_fn)?;

        let substrate_slot = Arc::clone(ctx.substrate_slot());
        let error_fn = lua.create_async_function(move |_, (id, on): (String, bool)| {
            let substrate = Arc::clone(&substrate_slot);
            async move {
                let substrate = locked_substrate(&substrate)?;
                substrate
                    .storage()
                    .error(&Subject::new(id), on)
                    .await
                    .map_err(mlua::Error::RuntimeError)
            }
        })?;
        storage_table.set("error", error_fn)?;

        let substrate_slot = Arc::clone(ctx.substrate_slot());
        let drop_fn = lua.create_async_function(move |_, (id, on): (String, bool)| {
            let substrate = Arc::clone(&substrate_slot);
            async move {
                let substrate = locked_substrate(&substrate)?;
                substrate
                    .storage()
                    .drop_writes(&Subject::new(id), on)
                    .await
                    .map_err(mlua::Error::RuntimeError)
            }
        })?;
        storage_table.set("drop_writes", drop_fn)?;

        let substrate_slot = Arc::clone(ctx.substrate_slot());
        let slow_fn = lua.create_async_function(move |_, (id, delay_ms): (String, u64)| {
            let substrate = Arc::clone(&substrate_slot);
            async move {
                let substrate = locked_substrate(&substrate)?;
                substrate
                    .storage()
                    .slow(&Subject::new(id), delay_ms)
                    .await
                    .map_err(mlua::Error::RuntimeError)
            }
        })?;
        storage_table.set("slow", slow_fn)?;

        let substrate_slot = Arc::clone(ctx.substrate_slot());
        let corrupt_fn = lua.create_async_function(move |_, (id, n): (String, u64)| {
            let substrate = Arc::clone(&substrate_slot);
            async move {
                let substrate = locked_substrate(&substrate)?;
                substrate
                    .storage()
                    .corrupt(&Subject::new(id), n)
                    .await
                    .map_err(mlua::Error::RuntimeError)
            }
        })?;
        storage_table.set("corrupt", corrupt_fn)?;

        let substrate_slot = Arc::clone(ctx.substrate_slot());
        let snapshot_fn = lua.create_async_function(move |_, id: String| {
            let substrate = Arc::clone(&substrate_slot);
            async move {
                let substrate = locked_substrate(&substrate)?;
                substrate
                    .storage()
                    .snapshot(&Subject::new(id))
                    .await
                    .map_err(mlua::Error::RuntimeError)
            }
        })?;
        storage_table.set("snapshot", snapshot_fn)?;

        let substrate_slot = Arc::clone(ctx.substrate_slot());
        let restore_fn = lua.create_async_function(move |_, (id, snap): (String, String)| {
            let substrate = Arc::clone(&substrate_slot);
            async move {
                let substrate = locked_substrate(&substrate)?;
                substrate
                    .storage()
                    .restore(&Subject::new(id), &snap)
                    .await
                    .map_err(mlua::Error::RuntimeError)
            }
        })?;
        storage_table.set("restore", restore_fn)?;

        dstest.set("storage", storage_table)?;
        Ok(())
    }
}
