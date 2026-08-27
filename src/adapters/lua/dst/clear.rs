use std::sync::Arc;
use std::time::Duration;

use mlua::{Lua, Result, Table};

use crate::application::context::BindingContext;
use crate::domain::event::ExperimentEvent;
use crate::domain::subject::{Subject, SubjectStatus};
use crate::ports::Substrate;

pub fn register<S: Substrate>(_lua: &Lua, dstest: &Table, ctx: &BindingContext<S>) -> Result<()> {
    let state = Arc::clone(&ctx.state);
    let substrate = Arc::clone(&ctx.substrate);

    let clear_fn = _lua.create_async_function(move |_, subject_id: String| {
        let state = Arc::clone(&state);
        let substrate = Arc::clone(&substrate);

        async move {
            let subject = Subject::new(subject_id.clone());

            // Clear the faults, then verify the subject is running again so a
            // failed recovery is recorded as such.
            substrate
                .clear_faults(&subject)
                .await
                .map_err(mlua::Error::RuntimeError)?;
            let ok = matches!(
                substrate.status(&subject).await,
                Ok(SubjectStatus::Running)
            );

            // Measure how long the fault was in place until now.
            let faulted_at = {
                let s = state.lock().expect("poisoned engine state lock");
                s.subjects.faulted_at(&subject_id)
            };
            let took = faulted_at.map_or(Duration::ZERO, |at| at.elapsed());

            let mut s = state.lock().expect("poisoned engine state lock");
            s.subjects.clear_faults(&subject_id);
            s.log.push(ExperimentEvent::FaultCleared);
            s.log.push(ExperimentEvent::Recovery { ok, took });

            Ok(())
        }
    })?;

    dstest.set("clear", clear_fn)?;
    Ok(())
}
