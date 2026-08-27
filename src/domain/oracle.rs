//! Pure correctness report types.
//!
//! The report is a projection over `CheckRun` events from the event log. It
//! is deliberately dumb data: running checks (which needs Lua) is the
//! application layer's job; the report is what remains.

use crate::domain::event::{CheckKind, ExperimentEvent};

/// A single failing check, carrying structural context for the caller.
#[derive(Clone, Debug)]
pub struct OracleFailure {
    pub check_type: String,
    pub name: String,
    pub round: Option<usize>,
    pub fault: Option<String>,
    pub subject: Option<String>,
    pub error: String,
}

/// Aggregate of check results for a run.
#[derive(Clone, Debug)]
pub struct OracleReport {
    pub passed: bool,
    pub total_checks: usize,
    pub passed_checks: usize,
    pub failed_checks: usize,
    pub failures: Vec<OracleFailure>,
}

impl Default for OracleReport {
    fn default() -> Self {
        Self {
            passed: true,
            total_checks: 0,
            passed_checks: 0,
            failed_checks: 0,
            failures: Vec::new(),
        }
    }
}

impl OracleReport {
    pub fn new() -> Self {
        Self::default()
    }

    fn add_failure(&mut self, failure: OracleFailure) {
        self.failures.push(failure);
        self.failed_checks += 1;
        self.total_checks += 1;
        self.passed = false;
    }

    fn add_pass(&mut self) {
        self.passed_checks += 1;
        self.total_checks += 1;
    }

    fn record(
        &mut self,
        kind: CheckKind,
        name: String,
        passed: bool,
        failure: Option<OracleFailure>,
    ) {
        if passed {
            self.add_pass();
        } else {
            let failure = failure.unwrap_or(OracleFailure {
                check_type: match kind {
                    CheckKind::Predicate => "predicate".to_string(),
                    CheckKind::Invariant => "invariant".to_string(),
                },
                name,
                round: None,
                fault: None,
                subject: None,
                error: "check failed".to_string(),
            });
            self.add_failure(failure);
        }
    }

    /// Fold `CheckRun` events into a report. Pure projection over the log.
    pub fn from_events(events: &[ExperimentEvent]) -> Self {
        let mut report = OracleReport::new();
        for ev in events {
            if let ExperimentEvent::CheckRun {
                kind,
                name,
                passed,
                error,
                fault,
                subject,
                round,
            } = ev
            {
                let failure = if !*passed {
                    Some(OracleFailure {
                        check_type: match kind {
                            CheckKind::Predicate => "predicate".to_string(),
                            CheckKind::Invariant => "invariant".to_string(),
                        },
                        name: name.clone(),
                        round: *round,
                        fault: fault.clone(),
                        subject: subject.clone(),
                        error: error.clone().unwrap_or_else(|| "check failed".to_string()),
                    })
                } else {
                    None
                };
                report.record(*kind, name.clone(), *passed, failure);
            }
        }
        report
    }
}
