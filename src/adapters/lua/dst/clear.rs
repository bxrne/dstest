use std::sync::Arc;
use std::time::Duration;

use mlua::{Lua, Result, Table};

use crate::application::context::{BindingContext, locked_substrate};
use crate::domain::event::ExperimentEvent;
use crate::domain::subject::{Subject, SubjectStatus};

pub fn register(_lua: &Lua, dstest: &Table, ctx: &BindingContext) -> Result<()> {
    let state = Arc::clone(&ctx.state);
    let substrate = Arc::clone(&ctx.substrate);

    let clear_fn = _lua.create_async_function(move |_, subject_id: String| {
        let state = Arc::clone(&state);
        let substrate = Arc::clone(&substrate);

        async move {
            let substrate = locked_substrate(&substrate)?;
            let subject = Subject::new(subject_id.clone());
            let had_faults = {
                let s = state.lock().expect("poisoned engine state lock");
                s.subjects
                    .find(&subject_id)
                    .is_some_and(|r| !r.active_faults.is_empty())
            };

            // Nothing to recover: this is a no-op clear, so emit no events.
            if !had_faults {
                return Ok(());
            }

            // Clear the faults, then verify the subject is running again so a
            // failed recovery is recorded as such.
            substrate
                .clear_faults(&subject)
                .await
                .map_err(mlua::Error::RuntimeError)?;
            let ok = matches!(substrate.status(&subject).await, Ok(SubjectStatus::Running));

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
