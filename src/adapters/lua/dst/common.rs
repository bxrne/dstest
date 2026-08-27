//! Shared fault-step execution for `dstest.dst.step` and
//! `dstest.dst.run_steps`.
//!
//! The orchestration itself lives in the application layer
//! (`application::usecases::run_step`); this module only renders the
//! structured [`StepOutcome`] to a Lua table.

use mlua::{Lua, Result, Table};

use crate::application::usecases::run_step::StepOutcome;

/// Render a completed step into the Lua table a script sees.
pub fn render_step(lua: &Lua, step: &StepOutcome) -> Result<Table> {
    let t = lua.create_table()?;
    t.set("fault", step.fault.to_string())?;
    t.set("subject", step.subject.clone())?;
    t.set("config", step.config.clone())?;
    t.set("round", step.round)?;
    t.set("total_rounds", step.total_rounds)?;
    t.set("remaining", step.remaining)?;
    t.set("more", step.more)?;

    if let Some(report) = &step.oracle {
        let ot = lua.create_table()?;
        ot.set("passed", report.passed)?;
        ot.set("total_checks", report.total_checks)?;
        ot.set("passed_checks", report.passed_checks)?;
        ot.set("failed_checks", report.failed_checks)?;
        t.set("oracle", ot)?;
    }

    Ok(t)
}
