//! `dstest.net.link` — deterministic, impairable links between subjects.
//!
//! Dispatches through the substrate's `NetworkControl` implementation.
//! Substrates without link support return "not supported" errors.

use std::sync::Arc;

use mlua::{Lua, Result, Table, UserData, UserDataMethods};

use crate::application::context::{BindingContext, locked_substrate};
use crate::domain::subject::Subject;
use crate::ports::Substrate;
use crate::ports::components::{Direction, LinkId, PartitionMode};

/// Handle to an established link, obtained via `dstest.net.link(a, b, port)`.
struct Link {
    substrate: Arc<dyn Substrate>,
    id: LinkId,
}

impl UserData for Link {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method("addr", |_, this, ()| {
            let substrate = Arc::clone(&this.substrate);
            let id = this.id.clone();
            async move {
                substrate
                    .network()
                    .link_addr(&id)
                    .await
                    .map_err(mlua::Error::RuntimeError)
            }
        });

        methods.add_async_method("latency", |_, this, (delay_ms, jitter_ms): (u64, u64)| {
            let substrate = Arc::clone(&this.substrate);
            let id = this.id.clone();
            async move {
                substrate
                    .network()
                    .set_latency(&id, delay_ms, jitter_ms)
                    .await
                    .map_err(mlua::Error::RuntimeError)
            }
        });

        methods.add_async_method("loss", |_, this, pct: f64| {
            let substrate = Arc::clone(&this.substrate);
            let id = this.id.clone();
            async move {
                substrate
                    .network()
                    .set_loss(&id, pct)
                    .await
                    .map_err(mlua::Error::RuntimeError)
            }
        });

        methods.add_async_method("partition", |_, this, opts: Table| {
            let substrate = Arc::clone(&this.substrate);
            let id = this.id.clone();
            async move {
                let direction: Direction = opts
                    .get::<String>("direction")
                    .unwrap_or_else(|_| "both".to_string())
                    .parse()
                    .map_err(mlua::Error::RuntimeError)?;
                let mode: PartitionMode = opts
                    .get::<String>("mode")
                    .unwrap_or_else(|_| "blackhole".to_string())
                    .parse()
                    .map_err(mlua::Error::RuntimeError)?;
                substrate
                    .network()
                    .partition(&id, direction, mode)
                    .await
                    .map_err(mlua::Error::RuntimeError)
            }
        });

        methods.add_async_method("heal", |_, this, ()| {
            let substrate = Arc::clone(&this.substrate);
            let id = this.id.clone();
            async move {
                substrate
                    .network()
                    .heal(&id)
                    .await
                    .map_err(mlua::Error::RuntimeError)
            }
        });
    }
}

pub fn register(lua: &Lua, dstest: &Table, ctx: &BindingContext) -> Result<()> {
    let substrate = Arc::clone(&ctx.substrate);

    let link_fn = lua.create_async_function(move |lua, (a, b, port): (String, String, u16)| {
        let substrate = Arc::clone(&substrate);
        async move {
            let substrate = locked_substrate(&substrate)?;
            let id = substrate
                .network()
                .link(&Subject::new(a), &Subject::new(b), port)
                .await
                .map_err(mlua::Error::RuntimeError)?;
            lua.create_userdata(Link { substrate, id })
        }
    })?;

    dstest.set("link", link_fn)?;
    Ok(())
}
