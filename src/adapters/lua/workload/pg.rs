//! `dstest.workload.pg` — generates sustained PostgreSQL traffic against a pool.
//!
//! Runs a loop of queries with configurable duration and rate. Reports
//! aggregate stats (queries, failures, latencies).

use std::time::{Duration, Instant};

use mlua::{Lua, Result, Table};

use crate::adapters::lua::pg::pool::LuaPgPool;
use crate::application::context::BindingContext;

pub fn register(lua: &Lua, workload: &Table, _ctx: &BindingContext) -> Result<()> {
    let run_fn = lua.create_async_function(
        move |lua, (wrapped_pool, opts): (LuaPgPool, Table)| async move {
            let duration_secs: u64 = opts.get("duration_secs").unwrap_or(10);
            let rate: u64 = opts.get("rate").unwrap_or(10);
            let queries: Vec<String> = opts
                .get("queries")
                .unwrap_or_else(|_| vec!["SELECT 1".to_string()]);

            let pool = wrapped_pool.0.clone();

            let start = Instant::now();
            let end = start + Duration::from_secs(duration_secs);
            let interval = Duration::from_millis(1000 / rate.max(1));

            let mut total = 0u64;
            let mut ok = 0u64;
            let mut fail = 0u64;
            let mut total_latency_ms = 0u64;
            let mut max_latency_ms = 0u64;

            while Instant::now() < end {
                let req_start = Instant::now();
                let sql = &queries[total as usize % queries.len()];

                let success = sqlx::query(sql).execute(&pool).await.is_ok();

                let elapsed = req_start.elapsed().as_millis() as u64;
                total += 1;
                if success {
                    ok += 1;
                } else {
                    fail += 1;
                }
                total_latency_ms += elapsed;
                if elapsed > max_latency_ms {
                    max_latency_ms = elapsed;
                }

                tokio::time::sleep(interval.saturating_sub(req_start.elapsed())).await;
            }

            let avg_latency = total_latency_ms.checked_div(total).unwrap_or(0);

            let t = lua.create_table()?;
            t.set("total_queries", total)?;
            t.set("ok", ok)?;
            t.set("failed", fail)?;
            t.set("avg_latency_ms", avg_latency)?;
            t.set("max_latency_ms", max_latency_ms)?;
            t.set("duration_secs", duration_secs)?;
            t.set("rate", rate)?;
            Ok(t)
        },
    )?;

    workload.set("pg", run_fn)?;
    Ok(())
}
