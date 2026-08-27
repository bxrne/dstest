//! Execute one fault step: pick the next fault from a config's tree, apply
//! it, run the oracle, and record everything as events.
//!
//! This is the heart of the application layer, extracted out of the Lua
//! bindings. The caller (a Lua binding) is responsible only for rendering the
//! returned [`StepOutcome`] to a Lua table.

use std::sync::{Arc, Mutex};

use mlua::Lua;

use crate::application::oracle::{run_checks, CheckRunner};
use crate::application::state::AppState;
use crate::domain::config::AccumulationMode;
use crate::domain::event::ExperimentEvent;
use crate::domain::fault::{Fault, StepResult};
use crate::domain::oracle::OracleReport;
use crate::domain::subject::Subject;
use crate::ports::Substrate;

/// Structured result of a fault step, before Lua rendering.
pub struct StepOutcome {
    pub fault: Fault,
    pub subject: String,
    pub config: String,
    pub round: usize,
    pub total_rounds: usize,
    pub remaining: usize,
    pub more: bool,
    pub oracle: Option<OracleReport>,
}

/// Execute a single fault step. Returns `None` when the config's fault
/// schedule is exhausted.
pub async fn run_step<S: Substrate>(
    lua: &Lua,
    state: &Arc<Mutex<AppState>>,
    oracle: &Arc<Mutex<CheckRunner>>,
    substrate: &Arc<S>,
    cfg_arg: Option<String>,
) -> mlua::Result<Option<StepOutcome>> {
    // Resolve the config handle and clone the config out.
    let (cfg, handle) = {
        let s = state.lock().expect("poisoned engine state lock");
        let h = s.configs.resolve_handle(cfg_arg)?;
        let cfg = s
            .configs
            .get(&h)
            .expect("resolved config must exist")
            .clone();
        (cfg, h)
    };

    if cfg.require_seed && cfg.seed.is_none() {
        return Err(mlua::Error::RuntimeError(format!(
            "config '{}' has no seed: set seed = n in dstest.config()",
            handle
        )));
    }

    // Lazily create this config's fault tree from its current subjects.
    // Subjects set up after the first step for a config are not faulted.
    let step_result: Option<StepResult> = {
        let mut s = state.lock().expect("poisoned engine state lock");
        if !s.fault_trees.contains(&handle) {
            let subject_ids = s.subjects.ids_for_config(&handle);
            if subject_ids.is_empty() {
                return Err(mlua::Error::RuntimeError(format!(
                    "no subjects for config '{}' — call dstest.setup({}, {{...}}) first",
                    handle, handle
                )));
            }
            let seed = cfg.seed.ok_or_else(|| {
                mlua::Error::RuntimeError(format!("config '{}' has no seed", handle))
            })?;
            s.fault_trees.insert(
                handle.clone(),
                crate::domain::fault::FaultTree::new(seed, subject_ids, &cfg),
            );
        }
        s.fault_trees.step(&handle)
    };

    let Some(step_result) = step_result else {
        return Ok(None);
    };

    let subject = Subject::new(step_result.subject_id.clone());

    // Accumulation mode: clear prior faults on this subject before applying.
    match cfg.accumulation_mode {
        AccumulationMode::Single => {
            substrate
                .clear_faults(&subject)
                .await
                .map_err(mlua::Error::RuntimeError)?;
            {
                let mut s = state.lock().expect("poisoned engine state lock");
                s.subjects.clear_faults(&step_result.subject_id);
                s.log.push(ExperimentEvent::FaultCleared);
            }
            tokio::time::sleep(std::time::Duration::from_millis(cfg.step_delay_ms)).await;
        }
        AccumulationMode::Accumulate => {}
    }

    substrate
        .affect(&subject, &step_result.fault)
        .await
        .map_err(mlua::Error::RuntimeError)?;

    let fault_str = step_result.fault.to_string();
    {
        let mut s = state.lock().expect("poisoned engine state lock");
            s.subjects.push_fault(&step_result.subject_id, step_result.fault);
        s.subjects.mark_faulted(&step_result.subject_id);
        s.log.push(ExperimentEvent::FaultApplied {
            fault: step_result.fault,
        });

        // State enumeration: each fault round visits a distinct engine state.
        s.log.push(ExperimentEvent::StateEnumerated { unique: true });

        // Interleaving enumeration: a config hosting several subjects offers
        // multiple schedules, so each round on one is a distinct interleaving.
        let subject_count = s.subjects.ids_for_config(&handle).len();
        if subject_count > 1 {
            s.log.push(ExperimentEvent::InterleavingEnumerated { unique: true });
        }

        // Blast radius: nodes of the config currently under fault.
        let faulted = s
            .subjects
            .ids_for_config(&handle)
            .iter()
            .filter(|id| {
                !s.subjects
                    .find(id)
                    .is_none_or(|r| r.active_faults.is_empty())
            })
            .count();
        s.log.push(ExperimentEvent::BlastAffected {
            class: "node",
            affected: faulted as u64,
            total: subject_count as u64,
        });
    }

    // Run the oracle if it is enabled. Predicates/invariants are collected
    // under a short lock and run against a scratch log, so no mutex guard is
    // held across an `.await`; the resulting events are then replayed into the
    // shared log under a brief, non-async lock.
    let oracle_report: Option<OracleReport> = {
        let checks = {
            let o = oracle.lock().expect("poisoned oracle lock");
            if o.enabled() {
                Some(o.collect_checks())
            } else {
                None
            }
        };

        let Some((preds, invs)) = checks else {
            return Ok(Some(StepOutcome {
                fault: step_result.fault,
                subject: step_result.subject_id,
                config: handle,
                round: step_result.round,
                total_rounds: step_result.total_rounds,
                remaining: step_result.remaining,
                more: step_result.more,
                oracle: None,
            }));
        };

        let mut scratch = crate::application::log::EventLog::new();
        let report = run_checks(
            lua,
            &preds,
            &invs,
            &step_result.subject_id,
            &fault_str,
            step_result.round,
            &mut scratch,
        )
        .await;
        let mut s = state.lock().expect("poisoned engine state lock");
        for ev in scratch.events() {
            s.log.push(ev.clone());
        }
        Some(report)
    };

    Ok(Some(StepOutcome {
        fault: step_result.fault,
        subject: step_result.subject_id,
        config: handle,
        round: step_result.round,
        total_rounds: step_result.total_rounds,
        remaining: step_result.remaining,
        more: step_result.more,
        oracle: oracle_report,
    }))
}
