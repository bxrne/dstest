//! Tear down every live subject.
//!
//! Extracted from the engine's shutdown path so the orchestration is a use
//! case rather than inline loop in the engine shell.

use std::sync::{Arc, Mutex};

use tracing::{debug, warn};

use crate::application::state::AppState;
use crate::domain::event::ExperimentEvent;
use crate::domain::subject::Subject;
use crate::ports::Substrate;

/// Drain and tear down every subject, awaiting each. Idempotent: subsequent
/// calls tear down nothing.
pub async fn teardown_all(state: &Arc<Mutex<AppState>>, substrate: &Arc<dyn Substrate>) {
    let records: Vec<(String, String)> = {
        let mut s = state.lock().expect("poisoned engine state lock");
        s.subjects.drain()
    };

    for (id, name) in records {
        if let Err(e) = substrate.teardown(Subject::new(id.clone())).await {
            warn!("teardown failed for subject {} ({}): {}", name, id, e);
        } else {
            debug!("teardown complete for subject {} ({})", name, id);
            let mut s = state.lock().expect("poisoned engine state lock");
            s.log.push(ExperimentEvent::SubjectTornDown);
        }
    }
}
