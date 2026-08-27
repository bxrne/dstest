//! Host a subject from a config handle and a setup table.
//!
//! This is the application-layer use case. The Lua binding is a thin shell
//! that hands the raw table through; all orchestration lives here.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use mlua::Table;

use crate::application::state::AppState;
use crate::domain::event::ExperimentEvent;
use crate::domain::subject::{Subject, SubjectStatus};
use crate::ports::Substrate;

/// Default time to wait for a dependency to become ready (60 × 500ms = 30s).
const DEP_WAIT_ATTEMPTS: usize = 120;
const DEP_WAIT_INTERVAL: Duration = Duration::from_millis(500);

/// Host a subject for a config handle. Returns the subject id.
pub async fn setup<S: Substrate>(
    state: &Arc<Mutex<AppState>>,
    substrate: &Arc<S>,
    handle: String,
    config_tbl: Table,
) -> mlua::Result<String> {
    let name = {
        let mut s = state.lock().expect("poisoned engine state lock");
        let cfg = s.configs.get(&handle).ok_or_else(|| {
            mlua::Error::RuntimeError(format!(
                "unknown config '{}' — pass the handle returned by dstest.config()",
                handle
            ))
        })?;

        if cfg.substrate.as_deref() != Some(S::NAME) {
            return Err(mlua::Error::RuntimeError(format!(
                "substrate mismatch: config '{}' wants \"{}\" but the engine was built for \"{}\"",
                handle,
                cfg.substrate.as_deref().unwrap_or("<none>"),
                S::NAME
            )));
        }

        s.subjects.next_name(&handle)
    };

    // Wait for dependencies to be ready before hosting.
    let depends: Vec<String> = config_tbl.get("depends").unwrap_or_default();
    for dep_id in &depends {
        let dep_subject = Subject::new(dep_id.clone());
        let mut ready = false;
        for _attempt in 0..DEP_WAIT_ATTEMPTS {
            match substrate.status(&dep_subject).await {
                Ok(SubjectStatus::Running) => {
                    ready = true;
                    break;
                }
                Ok(SubjectStatus::Terminated) => {
                    return Err(mlua::Error::RuntimeError(format!(
                        "dependency {} has terminated",
                        dep_id
                    )));
                }
                Ok(SubjectStatus::Pending) => {
                    tokio::time::sleep(DEP_WAIT_INTERVAL).await;
                }
                Err(e) => {
                    tracing::debug!("dependency {} status check: {}", dep_id, e);
                    tokio::time::sleep(DEP_WAIT_INTERVAL).await;
                }
            }
        }
        if !ready {
            return Err(mlua::Error::RuntimeError(format!(
                "dependency {} not ready after {}s",
                dep_id,
                DEP_WAIT_ATTEMPTS * DEP_WAIT_INTERVAL.as_millis() as usize / 1000,
            )));
        }
        tracing::debug!("dependency {} ready", dep_id);
    }

    let data = substrate
        .parse_subject(&config_tbl)
        .map_err(mlua::Error::RuntimeError)?;

    let hosted = substrate
        .host(&name, &data)
        .await
        .map_err(mlua::Error::RuntimeError)?;

    let subject_id = format!("{}/{}", S::NAME, hosted.id);

    {
        let mut s = state.lock().expect("poisoned engine state lock");
        let host = hosted.addr.clone();
        s.subjects.add(
            crate::application::state::SubjectRecord {
                id: subject_id.clone(),
                name,
                config: handle,
                active_faults: Vec::new(),
                faulted_at: None,
            },
            host,
        );
        s.log.push(ExperimentEvent::SubjectHosted);
    }

    Ok(subject_id)
}
