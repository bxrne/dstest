use std::sync::Arc;

use mlua::{Function, Lua, Result, Table, Value};

use crate::application::context::BindingContext;
use crate::application::oracle::{InvariantFn, PredicateFn};
use crate::domain::oracle::OracleReport;
use crate::ports::Substrate;

/// Render an oracle report into a Lua table (the `oracle` result).
fn report_to_table(lua: &Lua, report: &OracleReport) -> Result<Table> {
    let t = lua.create_table()?;
    t.set("passed", report.passed)?;
    t.set("total_checks", report.total_checks)?;
    t.set("passed_checks", report.passed_checks)?;
    t.set("failed_checks", report.failed_checks)?;

    let failures = lua.create_table()?;
    for (i, f) in report.failures.iter().enumerate() {
        let ft = lua.create_table()?;
        ft.set("type", f.check_type.clone())?;
        ft.set("name", f.name.clone())?;
        if let Some(r) = f.round {
            ft.set("round", r)?;
        }
        if let Some(ref fault) = f.fault {
            ft.set("fault", fault.clone())?;
        }
        if let Some(ref s) = f.subject {
            ft.set("subject", s.clone())?;
        }
        ft.set("error", f.error.clone())?;
        failures.set(i + 1, ft)?;
    }
    t.set("failures", failures)?;

    Ok(t)
}

pub fn register<S: Substrate>(lua: &Lua, dstest: &Table, ctx: &BindingContext<S>) -> Result<()> {
    let oracle = Arc::clone(&ctx.oracle);
    let oracle_table = lua.create_table()?;

    let oracle_clone = Arc::clone(&oracle);
    let predicate_fn =
        lua.create_async_function(move |lua, (name, func): (String, Function)| {
            let oracle = Arc::clone(&oracle_clone);
            async move {
                let func_ref = lua.create_registry_value(func)?;
                let func_ref = Arc::new(func_ref);

                let predicate: PredicateFn = Box::new(
                    move |lua: &Lua, subject: String, fault: String, round: usize| {
                        let func_ref = Arc::clone(&func_ref);
                        Box::pin(async move {
                            let func: Function = lua
                                .registry_value(&func_ref)
                                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                            let result: Value = func.call_async((subject, fault, round)).await?;

                            match result {
                                Value::Boolean(b) => Ok((b, None)),
                                Value::Table(t) => {
                                    let passed: bool = t.get(1)?;
                                    let msg: Option<String> = t.get(2).ok();
                                    Ok((passed, msg))
                                }
                                other => Err(mlua::Error::RuntimeError(format!(
                                    "predicate must return boolean or {{passed, message?}}, got {:?}",
                                    other.type_name()
                                ))),
                            }
                        })
                    },
                );

                oracle.lock().unwrap().add_predicate(name, predicate);
                Ok(())
            }
        })?;
    oracle_table.set("predicate", predicate_fn)?;

    let oracle_clone = Arc::clone(&oracle);
    let invariant_fn =
        lua.create_async_function(move |lua, (name, func): (String, Function)| {
            let oracle = Arc::clone(&oracle_clone);
            async move {
                let func_ref = lua.create_registry_value(func)?;
                let func_ref = Arc::new(func_ref);

                let invariant: InvariantFn = Box::new(move |lua: &Lua| {
                    let func_ref = Arc::clone(&func_ref);
                    Box::pin(async move {
                        let func: Function = lua
                            .registry_value(&func_ref)
                            .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                        let result: Value = func.call_async(()).await?;

                        match result {
                            Value::Boolean(b) => Ok((b, None)),
                            Value::Table(t) => {
                                let passed: bool = t.get(1)?;
                                let msg: Option<String> = t.get(2).ok();
                                Ok((passed, msg))
                            }
                            other => Err(mlua::Error::RuntimeError(format!(
                                "invariant must return boolean or {{passed, message?}}, got {:?}",
                                other.type_name()
                            ))),
                        }
                    })
                });

                oracle.lock().unwrap().add_invariant(name, invariant);
                Ok(())
            }
        })?;
    oracle_table.set("invariant", invariant_fn)?;

    // Run a function with the oracle enabled, then return the report for that
    // block, projected from the events appended while it ran.
    let state = Arc::clone(&ctx.state);
    let oracle_clone = Arc::clone(&oracle);
    let run_fn = lua.create_async_function(move |lua, func: Function| {
        let state = Arc::clone(&state);
        let oracle = Arc::clone(&oracle_clone);
        async move {
            let start = {
                let s = state.lock().expect("poisoned engine state lock");
                s.log.events().len()
            };
            {
                oracle.lock().unwrap().set_enabled(true);
            }

            let _: Value = func.call_async(()).await?;

            {
                oracle.lock().unwrap().set_enabled(false);
            }

            let report = {
                let s = state.lock().expect("poisoned engine state lock");
                OracleReport::from_events(&s.log.events()[start..])
            };

            // `lua` is owned here; `report_to_table` wants a reference.
            #[allow(clippy::needless_borrow)]
            report_to_table(&lua, &report)
        }
    })?;
    oracle_table.set("run", run_fn)?;

    let oracle_clone = Arc::clone(&oracle);
    let enable_fn = lua.create_function(move |_lua, _: ()| {
        oracle_clone.lock().unwrap().set_enabled(true);
        Ok(())
    })?;
    oracle_table.set("enable", enable_fn)?;

    let oracle_clone = Arc::clone(&oracle);
    let disable_fn = lua.create_function(move |_lua, _: ()| {
        oracle_clone.lock().unwrap().set_enabled(false);
        Ok(())
    })?;
    oracle_table.set("disable", disable_fn)?;

    // Report for the whole run, projected from the log.
    let state = Arc::clone(&ctx.state);
    let report_fn = lua.create_function(move |lua, ()| {
        let s = state.lock().expect("poisoned engine state lock");
        let report = OracleReport::from_events(s.log.events());
        report_to_table(lua, &report)
    })?;
    oracle_table.set("report", report_fn)?;

    // The log is the single source of truth; there is nothing to reset.
    let reset_fn = lua.create_function(|_lua, ()| Ok(()))?;
    oracle_table.set("reset", reset_fn)?;

    dstest.set("oracle", oracle_table)?;
    Ok(())
}
