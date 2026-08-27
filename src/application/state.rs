//! Application state: the composed, owned registries behind the engine.
//!
//! This replaces the previous bare `EngineState` with a small set of named,
//! encapsulated components: the event log (source of truth for what
//! happened), the subject registry (what is live), the config registry, and
//! the fault tree registry. Each owns its own invariants; callers go through
//! methods instead of mutating raw maps.

use std::collections::BTreeMap;
use std::time::Instant;

use crate::application::log::EventLog;
use crate::domain::config::Config;
use crate::domain::fault::{Fault, FaultTree};

/// A hosted subject and everything the engine needs to remember about it.
pub struct SubjectRecord {
    /// Fully-qualified subject id, e.g. `"docker/<container_id>"`.
    pub id: String,
    /// Engine-assigned name passed to the substrate (e.g. container name).
    pub name: String,
    /// Handle of the `dstest.config` this subject was created under.
    pub config: String,
    /// Faults currently applied to this subject (for observability).
    pub active_faults: Vec<Fault>,
    /// Instant the most recent fault was applied to this subject, used to
    /// measure recovery duration when the fault is cleared.
    pub faulted_at: Option<Instant>,
}

/// Owns the set of live subjects and the address map. Single owner of the
/// "what is running" invariant, instead of several hand-kept collections.
#[derive(Default)]
pub struct SubjectRegistry {
    subjects: Vec<SubjectRecord>,
    hosts: BTreeMap<String, String>,
    name_counter: usize,
}

impl SubjectRegistry {
    /// Generate the next monotonic subject name for a config handle.
    pub fn next_name(&mut self, handle: &str) -> String {
        self.name_counter += 1;
        format!(
            "dstest-{}-{}",
            handle
                .chars()
                .map(
                    |c| if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-' {
                        c
                    } else {
                        '-'
                    }
                )
                .collect::<String>(),
            self.name_counter
        )
    }

    /// Register a hosted subject, recording the id -> address mapping.
    pub fn add(&mut self, subject: SubjectRecord, host: Option<String>) {
        if let Some(addr) = host {
            self.hosts.insert(subject.id.clone(), addr);
        }
        self.subjects.push(subject);
    }

    pub fn find(&self, id: &str) -> Option<&SubjectRecord> {
        self.subjects.iter().find(|r| r.id == id)
    }

    pub fn find_mut(&mut self, id: &str) -> Option<&mut SubjectRecord> {
        self.subjects.iter_mut().find(|r| r.id == id)
    }

    pub fn ids_for_config(&self, config: &str) -> Vec<String> {
        self.subjects
            .iter()
            .filter(|r| r.config == config)
            .map(|r| r.id.clone())
            .collect()
    }

    pub fn host_for(&self, id: &str) -> Option<&str> {
        self.hosts.get(id).map(|s| s.as_str())
    }

    pub fn config_for(&self, id: &str) -> Option<&str> {
        self.find(id).map(|r| r.config.as_str())
    }

    pub fn push_fault(&mut self, id: &str, fault: Fault) {
        if let Some(rec) = self.find_mut(id) {
            rec.active_faults.push(fault);
        }
    }

    /// Record the moment a fault was applied to a subject.
    pub fn mark_faulted(&mut self, id: &str) {
        if let Some(rec) = self.find_mut(id) {
            rec.faulted_at = Some(Instant::now());
        }
    }

    /// When this subject's most recent fault was applied, if any.
    pub fn faulted_at(&self, id: &str) -> Option<Instant> {
        self.find(id).and_then(|r| r.faulted_at)
    }

    pub fn clear_faults(&mut self, id: &str) {
        if let Some(rec) = self.find_mut(id) {
            rec.active_faults.clear();
            rec.faulted_at = None;
        }
    }

    /// Drain the registry for teardown, returning the id/name pairs.
    pub fn drain(&mut self) -> Vec<(String, String)> {
        self.subjects.drain(..).map(|r| (r.id, r.name)).collect()
    }
}

/// Registered experiment configs, keyed by the handle from `dstest.config`.
#[derive(Default)]
pub struct ConfigRegistry {
    configs: BTreeMap<String, Config>,
}

impl ConfigRegistry {
    pub fn register(&mut self, handle: String, cfg: Config) {
        self.configs.insert(handle, cfg);
    }

    pub fn contains(&self, handle: &str) -> bool {
        self.configs.contains_key(handle)
    }

    pub fn get(&self, handle: &str) -> Option<&Config> {
        self.configs.get(handle)
    }

    pub fn unique_handle(&self) -> String {
        let mut n = self.configs.len() + 1;
        loop {
            let candidate = format!("config_{}", n);
            if !self.configs.contains_key(&candidate) {
                return candidate;
            }
            n += 1;
        }
    }

    /// Resolve an explicit or implicit config handle (used by dst step calls).
    pub fn resolve_handle(&self, arg: Option<String>) -> Result<String, mlua::Error> {
        match arg {
            Some(h) => {
                if self.configs.contains_key(&h) {
                    Ok(h)
                } else {
                    Err(mlua::Error::RuntimeError(format!(
                        "unknown config '{}' — pass the handle returned by dstest.config()",
                        h
                    )))
                }
            }
            None => match self.configs.len() {
                0 => Err(mlua::Error::RuntimeError(
                    "no configs registered: call dstest.config({...}) first".into(),
                )),
                1 => Ok(self.configs.keys().next().unwrap().clone()),
                _ => Err(mlua::Error::RuntimeError(
                    "multiple configs registered; pass a config handle, e.g. dstest.dst.step(cfg)"
                        .into(),
                )),
            },
        }
    }
}

/// Fault trees kept separate from the rest of state: each is the deterministic
/// generator for one config, created lazily on first step and owned here so
/// the schedule persists across `run_step` calls.
#[derive(Default)]
pub struct FaultTreeRegistry {
    trees: BTreeMap<String, FaultTree>,
}

impl FaultTreeRegistry {
    pub fn contains(&self, handle: &str) -> bool {
        self.trees.contains_key(handle)
    }

    pub fn insert(&mut self, handle: String, tree: FaultTree) {
        self.trees.insert(handle, tree);
    }

    pub fn step(&mut self, handle: &str) -> Option<crate::domain::fault::StepResult> {
        self.trees.get_mut(handle).and_then(|t| t.step())
    }
}

/// The composed application state behind the engine.
pub struct AppState {
    pub log: EventLog,
    pub subjects: SubjectRegistry,
    pub configs: ConfigRegistry,
    pub fault_trees: FaultTreeRegistry,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            log: EventLog::new(),
            subjects: SubjectRegistry::default(),
            configs: ConfigRegistry::default(),
            fault_trees: FaultTreeRegistry::default(),
        }
    }
}
