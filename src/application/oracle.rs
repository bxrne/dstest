//! The correctness check runner.
//!
//! This is the application-layer component that *runs* the Lua-defined
//! predicates and invariants. It emits [`ExperimentEvent::CheckRun`] into the
//! [`EventLog`]; the report (domain data) is a pure projection over those
//! events, rebuilt on demand. The runner holds no accumulated report of its
//! own — the log is the single source of truth.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use mlua::Lua;

use crate::application::log::EventLog;
use crate::domain::event::{CheckKind, ExperimentEvent};
use crate::domain::oracle::OracleReport;

type PredicateFut<'a> =
    Pin<Box<dyn Future<Output = Result<(bool, Option<String>), mlua::Error>> + 'a>>;
type InvariantFut<'a> =
    Pin<Box<dyn Future<Output = Result<(bool, Option<String>), mlua::Error>> + 'a>>;
pub type PredicateFn =
    Box<dyn for<'a> Fn(&'a Lua, String, String, usize) -> PredicateFut<'a> + Send + Sync>;
pub type InvariantFn = Box<dyn for<'a> Fn(&'a Lua) -> InvariantFut<'a> + Send + Sync>;

pub struct CheckRunner {
    predicates: HashMap<String, PredicateFn>,
    invariants: HashMap<String, InvariantFn>,
    enabled: bool,
}

impl Default for CheckRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl CheckRunner {
    pub fn new() -> Self {
        Self {
            predicates: HashMap::new(),
            invariants: HashMap::new(),
            enabled: false,
        }
    }

    pub fn add_predicate(&mut self, name: String, func: PredicateFn) {
        self.predicates.insert(name, func);
    }

    pub fn add_invariant(&mut self, name: String, func: InvariantFn) {
        self.invariants.insert(name, func);
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) -> &mut Self {
        self.enabled = enabled;
        self
    }

    /// Run every check (predicates then invariants), emitting a `CheckRun`
    /// event per check into `log`, and return the report projected from the
    /// events this call appended.
    pub async fn check_all(
        &self,
        lua: &Lua,
        subject_id: &str,
        fault: &str,
        round: usize,
        log: &mut EventLog,
    ) -> OracleReport {
        let start = log.events().len();

        for (name, predicate) in &self.predicates {
            let (passed, error) =
                match predicate(lua, subject_id.to_string(), fault.to_string(), round).await {
                    Ok((passed, msg)) => (passed, msg),
                    Err(e) => (false, Some(format!("predicate error: {}", e))),
                };
            let error = if passed { None } else { error };
            log.push(ExperimentEvent::CheckRun {
                kind: CheckKind::Predicate,
                name: name.clone(),
                passed,
                error,
                fault: Some(fault.to_string()),
                subject: Some(subject_id.to_string()),
                round: Some(round),
            });
        }

        for (name, invariant) in &self.invariants {
            let (passed, error) = match invariant(lua).await {
                Ok((passed, msg)) => (passed, msg),
                Err(e) => (false, Some(format!("invariant error: {}", e))),
            };
            let error = if passed { None } else { error };
            log.push(ExperimentEvent::CheckRun {
                kind: CheckKind::Invariant,
                name: name.clone(),
                passed,
                error,
                fault: None,
                subject: None,
                round: None,
            });
        }

        OracleReport::from_events(&log.events()[start..])
    }
}
