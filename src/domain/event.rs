//! The experiment event log vocabulary.
//!
//! A dstest run is a deterministic log of a scripted experiment. Everything a
//! run does is expressed as an [`ExperimentEvent`] appended to an [`EventLog`]
//! (application layer). Read models — metrics, blast radius, the subject
//! registry — are projections over this log, so there is a single source of
//! truth and no hand-synced duplicate state.
//!
//! Every variant has a producer in the application use cases: the projections
//! ([`crate::application::log::Metrics`],
//! [`crate::application::log::BlastRadius`],
//! [`crate::domain::oracle::OracleReport`]) fold exactly these events into
//! their read models, so the log is the single source of truth.

use std::time::Duration;

use crate::domain::fault::Fault;

/// How a correctness check was classified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckKind {
    Predicate,
    Invariant,
}

/// Classification of a fault by the concrete layer it targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultClass {
    Pause,
    Kill,
    Deprive,
}

impl From<Fault> for FaultClass {
    fn from(fault: Fault) -> Self {
        match fault {
            Fault::Pause => FaultClass::Pause,
            Fault::Kill => FaultClass::Kill,
            Fault::Deprive(_) => FaultClass::Deprive,
        }
    }
}

/// One observed fact about the experiment run.
///
/// Each variant carries exactly the fields its read models consume, so the
/// vocabulary stays lean and every field is consumed by a projection
/// ([`crate::application::log::Metrics`],
/// [`crate::application::log::BlastRadius`],
/// [`crate::domain::oracle::OracleReport`]).
#[derive(Clone, Debug)]
pub enum ExperimentEvent {
    /// A config was registered (marker: the handle is not consumed by any
    /// projection, so it is not carried).
    ConfigRegistered,
    /// A scenario (the main script body) started.
    ScenarioStarted,
    /// A scenario completed successfully.
    ScenarioCompleted,
    /// A subject was hosted.
    SubjectHosted,
    /// A subject was torn down.
    SubjectTornDown,
    /// A new unique engine state was enumerated during a scenario.
    StateEnumerated { unique: bool },
    /// A new unique interleaving was enumerated during a scenario.
    InterleavingEnumerated { unique: bool },
    /// A fault was applied to a subject.
    FaultApplied { fault: Fault },
    /// A fault was cleared from a subject.
    FaultCleared,
    /// A subject was recovered (or failed to recover) after a fault.
    Recovery { took: Duration, ok: bool },
    /// Blast radius: how many targets of a class were affected out of total.
    BlastAffected {
        class: &'static str,
        affected: u64,
        total: u64,
    },
    /// A correctness check ran. Failure detail is carried inline so the
    /// report projection can be rebuilt purely from the log.
    CheckRun {
        kind: CheckKind,
        name: String,
        passed: bool,
        /// Human-readable failure reason, present when the check did not pass.
        error: Option<String>,
        /// Fault the check was about, when applicable (predicates).
        fault: Option<String>,
        /// Subject the check was about, when applicable (predicates).
        subject: Option<String>,
        /// Fault round the check was about, when applicable (predicates).
        round: Option<usize>,
    },
}
