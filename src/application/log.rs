//! The experiment event log and its projections.
//!
//! The log is the single source of truth for what happened in a run. The
//! [`EventLog`] appends [`ExperimentEvent`]s and feeds a small fixed set of
//! projections inline on `push`, so hot counters are available in O(1)
//! without a pub/sub bus.

use std::time::Duration;

use crate::domain::event::{ExperimentEvent, FaultClass};

/// Scope/execution metrics derived from the event log.
#[derive(Default, Debug, Clone)]
pub struct Metrics {
    // Execution
    pub scenarios: u64,
    pub unique_states: u64,
    pub unique_interleavings: u64,
    /// Sum of recovery durations across all `Recovery` events, whether or not
    /// the recovery succeeded. Represents the time a run spent in fault
    /// handling.
    pub simulated_time: Duration,

    // Faults
    pub faults_injected: u64,
    pub max_fault_depth: u32,
    pub classes_seen: u64,

    // Recovery
    pub recoveries: u64,
    pub failures: u64,
    pub total_recovery_time: Duration,

    // Internal running aggregates (not part of the public projection).
    active_faults: u32,
    classes_mask: u8,
}

impl Metrics {
    #[cfg(test)]
    pub fn from_events(events: &[ExperimentEvent]) -> Self {
        let mut m = Self::default();
        for ev in events {
            m.apply(ev);
        }
        m
    }

    /// Apply a single event to the running aggregate.
    fn apply(&mut self, ev: &ExperimentEvent) {
        match ev {
            ExperimentEvent::ScenarioStarted => self.scenarios += 1,
            ExperimentEvent::StateEnumerated { unique } => {
                if *unique {
                    self.unique_states += 1;
                }
            }
            ExperimentEvent::InterleavingEnumerated { unique } => {
                if *unique {
                    self.unique_interleavings += 1;
                }
            }
            ExperimentEvent::FaultApplied { fault, .. } => {
                self.faults_injected += 1;
                self.active_faults += 1;
                self.max_fault_depth = self.max_fault_depth.max(self.active_faults);
                let bit = match FaultClass::from(*fault) {
                    FaultClass::Pause => 0b001,
                    FaultClass::Kill => 0b010,
                    FaultClass::Deprive => 0b100,
                };
                self.classes_mask |= bit;
                self.classes_seen = self.classes_mask.count_ones() as u64;
            }
            ExperimentEvent::FaultCleared => {
                self.active_faults = self.active_faults.saturating_sub(1);
            }
            ExperimentEvent::Recovery { ok, took } => {
                self.simulated_time += *took;
                if *ok {
                    self.recoveries += 1;
                    self.total_recovery_time += *took;
                } else {
                    self.failures += 1;
                }
            }
            _ => {}
        }
    }
}

/// Per-target-class blast radius (affected out of total), derived from
/// `BlastAffected` events.
#[derive(Default, Debug, Clone)]
pub struct BlastRadius {
    pub nodes: Totals,
    pub services: Totals,
    pub clients: Totals,
    pub requests: Totals,
}

impl BlastRadius {
    #[cfg(test)]
    pub fn from_events(events: &[ExperimentEvent]) -> Self {
        let mut b = Self::default();
        for ev in events {
            b.apply(ev);
        }
        b
    }

    fn apply(&mut self, ev: &ExperimentEvent) {
        if let ExperimentEvent::BlastAffected {
            class,
            affected,
            total,
        } = ev
        {
            let t = match *class {
                "node" => &mut self.nodes,
                "service" => &mut self.services,
                "client" => &mut self.clients,
                "request" => &mut self.requests,
                _ => return,
            };
            t.affected = t.affected.max(*affected);
            t.total = t.total.max(*total);
        }
    }
}

/// A count and a whole, with a derived ratio.
#[derive(Default, Debug, Clone, Copy)]
pub struct Totals {
    pub affected: u64,
    pub total: u64,
}

impl Totals {
    pub fn ratio(&self) -> Option<f64> {
        if self.total == 0 {
            None
        } else {
            Some(self.affected as f64 / self.total as f64)
        }
    }
}

/// Append-only event log with inline projections.
#[derive(Default)]
pub struct EventLog {
    events: Vec<ExperimentEvent>,
    metrics: Metrics,
    blast: BlastRadius,
}

impl EventLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append an event, feeding the inline projections.
    pub fn push(&mut self, ev: ExperimentEvent) -> &mut Self {
        self.metrics.apply(&ev);
        self.blast.apply(&ev);
        self.events.push(ev);
        self
    }

    pub fn events(&self) -> &[ExperimentEvent] {
        &self.events
    }

    pub fn metrics(&self) -> &Metrics {
        &self.metrics
    }

    /// Blast-radius projection over the log.
    pub fn blast(&self) -> &BlastRadius {
        &self.blast
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::event::{CheckKind, ExperimentEvent};
    use crate::domain::fault::Fault;

    #[test]
    fn metrics_folds_events() {
        let events = [
            ExperimentEvent::ScenarioStarted,
            ExperimentEvent::StateEnumerated { unique: true },
            ExperimentEvent::StateEnumerated { unique: false },
            ExperimentEvent::FaultApplied { fault: Fault::Kill },
            ExperimentEvent::FaultApplied {
                fault: Fault::Pause,
            },
            ExperimentEvent::Recovery {
                took: Duration::from_millis(5),
                ok: true,
            },
            ExperimentEvent::Recovery {
                took: Duration::ZERO,
                ok: false,
            },
        ];
        let m = Metrics::from_events(&events);
        assert_eq!(m.scenarios, 1);
        assert_eq!(m.unique_states, 1);
        assert_eq!(m.faults_injected, 2);
        assert_eq!(m.max_fault_depth, 2);
        assert_eq!(m.recoveries, 1);
        assert_eq!(m.failures, 1);
        assert_eq!(m.total_recovery_time, Duration::from_millis(5));
        assert_eq!(m.simulated_time, Duration::from_millis(5));
    }

    #[test]
    fn blast_radius_tracks_affected_and_total() {
        let events = [
            ExperimentEvent::BlastAffected {
                class: "node",
                affected: 2,
                total: 5,
            },
            ExperimentEvent::BlastAffected {
                class: "service",
                affected: 1,
                total: 3,
            },
        ];
        let b = BlastRadius::from_events(&events);
        assert_eq!(b.nodes.affected, 2);
        assert_eq!(b.nodes.total, 5);
        assert_eq!(b.nodes.ratio(), Some(0.4));
        assert_eq!(b.services.affected, 1);
        assert_eq!(b.requests.ratio(), None);
    }

    #[test]
    fn event_log_pushes_and_exposes_metrics() {
        let mut log = EventLog::new();
        log.push(ExperimentEvent::ScenarioStarted)
            .push(ExperimentEvent::CheckRun {
                kind: CheckKind::Invariant,
                name: "i1".into(),
                passed: true,
                error: None,
                fault: None,
                subject: None,
                round: None,
            });
        assert_eq!(log.events().len(), 2);
        assert_eq!(log.metrics().scenarios, 1);
    }
}
