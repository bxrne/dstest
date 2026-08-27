//! Core subject value types.
//!
//! A subject is the hosted entity the harness runs chaos against. These types
//! are pure domain values: they depend on nothing else in the crate.

use std::fmt::{self, Display};

/// A hosted subject identifier.
pub struct Subject {
    pub id: String,
}

impl Subject {
    pub fn new(id: String) -> Self {
        Subject { id }
    }
}

impl Display for Subject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Subject {{ id: {} }}", self.id)
    }
}

/// Log stream classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    StdOut,
    StdErr,
}

impl Display for Stream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Stream::StdOut => write!(f, "stdout"),
            Stream::StdErr => write!(f, "stderr"),
        }
    }
}

/// One log line from a subject.
#[derive(Clone, Debug)]
pub struct LogEntry {
    pub stream: Stream,
    pub message: String,
}

/// Result of a command executed inside a subject.
#[derive(Clone, Debug)]
pub struct ExecResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Substrate-agnostic liveness status used for dependency readiness waits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubjectStatus {
    /// Subject is up and running.
    Running,
    /// Subject exists but is not currently running (paused, starting, …).
    Pending,
    /// Subject has terminated — waiting is pointless.
    Terminated,
}

/// The result of hosting a subject: its instance id and an optional
/// reachable address (e.g. `"localhost:8080"`). Each substrate decides
/// what address format makes sense — Docker returns a host port mapping,
/// a future k8s substrate could return a service DNS name.
pub struct HostedSubject {
    pub id: String,
    pub addr: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subject_new() {
        let subject = Subject::new("docker/abc123".to_string());
        assert_eq!(subject.id, "docker/abc123");
    }

    #[test]
    fn test_subject_display() {
        let subject = Subject::new("docker/abc123".to_string());
        assert_eq!(format!("{subject}"), "Subject { id: docker/abc123 }");
    }
}
