//! The experiment event log vocabulary.
//!
//! A dstest run is a deterministic log of a scripted experiment. Everything a
//! run does is expressed as an [`ExperimentEvent`] appended to an [`EventLog`]
//! (application layer). Read models — metrics, blast radius, the subject
//! registry — are projections over this log, so there is a single source of
//! truth and no hand-synced duplicate state.
//!
//! The vocabulary is the forward-looking contract: use cases emit a subset of
//! variants today, and projections read a subset of fields, with the rest
//! landing as scenarios and observability features are wired. Dead-code is
//! allowed module-wide so the designed surface is not mistaken for cruft.

#![allow(dead_code)]

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
#[derive(Clone, Debug)]
pub enum ExperimentEvent {
    /// A config was registered under a handle.
    ConfigRegistered { handle: String },
    /// A scenario (the main script body) started.
    ScenarioStarted,
    /// A scenario completed successfully.
    ScenarioCompleted,
    /// A subject was hosted.
    SubjectHosted {
        id: String,
        config: String,
        name: String,
    },
    /// A subject was torn down.
    SubjectTornDown { id: String },
    /// A new unique engine state was enumerated during a scenario.
    StateEnumerated { unique: bool },
    /// A new unique interleaving was enumerated during a scenario.
    InterleavingEnumerated { unique: bool },
    /// A fault was applied to a subject on a config.
    FaultApplied {
        step: usize,
        subject: String,
        config: String,
        fault: Fault,
    },
    /// A fault was cleared from a subject.
    FaultCleared { subject: String },
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
