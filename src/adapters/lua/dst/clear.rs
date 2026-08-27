use std::sync::Arc;

use mlua::{Lua, Result, Table};

use crate::application::context::BindingContext;
use crate::domain::event::ExperimentEvent;
use crate::domain::subject::Subject;
use crate::ports::Substrate;

pub fn register<S: Substrate>(_lua: &Lua, dstest: &Table, ctx: &BindingContext<S>) -> Result<()> {
    let state = Arc::clone(&ctx.state);
    let substrate = Arc::clone(&ctx.substrate);

    let clear_fn = _lua.create_async_function(move |_, subject_id: String| {
        let state = Arc::clone(&state);
        let substrate = Arc::clone(&substrate);

        async move {
            let subject = Subject::new(subject_id.clone());
            substrate
                .clear_faults(&subject)
                .await
                .map_err(mlua::Error::RuntimeError)?;

            let mut s = state.lock().expect("poisoned engine state lock");
            s.subjects.clear_faults(&subject_id);
            s.log.push(ExperimentEvent::FaultCleared {
                subject: subject_id,
            });

            Ok(())
        }
    })?;

    dstest.set("clear", clear_fn)?;
    Ok(())
}
