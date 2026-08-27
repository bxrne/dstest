//! The Lua host engine.
//!
//! The engine owns the [`BindingContext`] and drives a script through it.
//! It deliberately does NOT know about the Lua bindings (adapters): the
//! adapters register themselves against the engine's context from the
//! composition root (main), keeping the dependency direction one-way.

use mlua::Lua;

use crate::application::context::BindingContext;
use crate::application::log::Metrics;
use crate::application::usecases::teardown;
use crate::domain::oracle::OracleReport;
use crate::ports::Substrate;

pub struct Engine<S: Substrate> {
    ctx: BindingContext<S>,
}

impl<S: Substrate> Engine<S> {
    pub fn new(substrate: S) -> Self {
        let ctx = BindingContext::new(substrate);
        set_up_lua(&ctx);
        Engine { ctx }
    }

    /// The context, for the composition root to register Lua bindings and for
    /// tests to reach the application state.
    pub fn context(&self) -> &BindingContext<S> {
        &self.ctx
    }

    /// The Lua state backing this engine.
    pub fn lua(&self) -> &Lua {
        &self.ctx.lua
    }

    pub async fn execute(&self, script: &str) -> mlua::Result<()> {
        self.ctx.lua.load(script).call_async::<()>(()).await
    }

    /// Tear down every live subject, awaiting each. Call this while a tokio
    /// runtime is still alive; `Drop` remains as a last-resort fallback.
    pub async fn shutdown(&self) {
        teardown::teardown_all(&self.ctx.state, &self.ctx.substrate).await;
    }

    /// Final oracle report for the run (used for the process exit code).
    pub fn oracle_report(&self) -> OracleReport {
        let lock = self.ctx.state.lock().expect("poisoned engine state lock");
        OracleReport::from_events(lock.log.events())
    }

    /// Scope/execution metrics projected from the event log.
    pub fn metrics(&self) -> Metrics {
        self.ctx
            .state
            .lock()
            .expect("poisoned engine state lock")
            .log
            .metrics()
            .clone()
    }
}

/// Publish the `dstest` global table and a `print` helper into the Lua state.
fn set_up_lua<S: Substrate>(ctx: &BindingContext<S>) {
    let globals = ctx.lua.globals();
    let dstest = ctx
        .lua
        .create_table()
        .expect("failed to create dstest table");

    let _ = globals.set(
        "print",
        ctx.lua
            .create_function(|_, msg: String| {
                tracing::info!("lua: {}", msg);
                Ok(())
            })
            .expect("failed to create print function"),
    );

    globals
        .set("dstest", dstest)
        .expect("failed to set global dstest");
}
