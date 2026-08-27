//! dstest — distributed systems fault testing harness. Composition root.
//!
//! This module is the only place that knows every layer: it selects the
//! Docker substrate adapter, builds the application engine, registers the
//! Lua bindings against it, and drives the script from stdin.

mod adapters;
mod application;
mod domain;
mod ports;

use std::io::{Read, stdin};
use tracing::{debug, error, info};

fn main() {
    tracing_subscriber::fmt::init();

    debug!("Starting dstest");

    let docker = adapters::substrate::docker::Docker::new().unwrap_or_else(|e| {
        error!("Failed to connect to Docker daemon: {e}");
        std::process::exit(3);
    });

    let engine = application::Engine::new(docker);

    // Wire the Lua bindings onto the engine (composition root; the engine
    // itself knows nothing about concrete adapters).
    if let Err(e) = adapters::lua::register_all(engine.lua(), engine.context()) {
        error!("Failed to register Lua bindings error=\"{e}\"");
        std::process::exit(1);
    }

    debug!("Reading scripts from stdin");
    let mut script = String::new();
    if stdin().read_to_string(&mut script).is_err() {
        error!("Failed to read stdin");
        std::process::exit(1);
    }

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");

    let code = rt.block_on(async {
        let result = engine.execute(&script).await;
        engine.shutdown().await;

        match result {
            Ok(()) => {
                let report = engine.oracle_report();
                let m = engine.metrics();
                let blast = engine.blast_radius();
                info!(
                    "scenarios={} unique_states={} unique_interleavings={} simulated_time_ms={} \
                     faults={} max_fault_depth={} fault_classes={} recoveries={} failures={} \
                     recovery_time_ms={} checks_passed={} checks_failed={}",
                    m.scenarios,
                    m.unique_states,
                    m.unique_interleavings,
                    m.simulated_time.as_millis(),
                    m.faults_injected,
                    m.max_fault_depth,
                    m.classes_seen,
                    m.recoveries,
                    m.failures,
                    m.total_recovery_time.as_millis(),
                    report.passed_checks,
                    report.failed_checks,
                );
                let fmt_ratio =
                    |t: &crate::application::log::Totals| match t.ratio() {
                        Some(r) => format!("{:.2}", r),
                        None => "-".to_string(),
                    };
                info!(
                    "blast_radius nodes={}/{} ({}) services={}/{} ({}) clients={}/{} ({}) requests={}/{} ({})",
                    blast.nodes.affected,
                    blast.nodes.total,
                    fmt_ratio(&blast.nodes),
                    blast.services.affected,
                    blast.services.total,
                    fmt_ratio(&blast.services),
                    blast.clients.affected,
                    blast.clients.total,
                    fmt_ratio(&blast.clients),
                    blast.requests.affected,
                    blast.requests.total,
                    fmt_ratio(&blast.requests),
                );
                if report.total_checks > 0 && !report.passed {
                    error!(
                        "oracle failures detected: {} of {} checks failed",
                        report.failed_checks, report.total_checks
                    );
                    2
                } else {
                    debug!("Experiment complete");
                    0
                }
            }
            Err(e) => {
                error!("Failed to execute script error=\"{e}\"");
                1
            }
        }
    });

    drop(engine);
    drop(rt);
    debug!("Exiting dstest");
    std::process::exit(code);
}
