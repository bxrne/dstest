//! `dstest.workload.http` — generates sustained HTTP traffic against a subject.
//!
//! Supports manual path/method lists or loading from an OpenAPI spec:
//!
//! ```lua
//! -- Manual
//! dstest.workload.http(s, {
//!     duration_secs = 10,
//!     rate = 20,
//!     requests = {
//!         { method = "GET",  path = "/get" },
//!         { method = "POST", path = "/post", body = '{"key":"val"}' },
//!     },
//! })
//!
//! -- From OpenAPI spec
//! dstest.workload.http(s, {
//!     duration_secs = 10,
//!     rate = 20,
//!     openapi = "/workspace/api/openapi.json",
//! })
//! ```

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mlua::{Lua, Result, Table};

use crate::adapters::lua::net::http::resolve_subject_http;
use crate::application::context::BindingContext;

/// Cache of parsed OpenAPI request lists, keyed by the spec path. Re-parsing
/// a spec from disk on every `dstest.workload.http` call is avoidable work;
/// a given spec path is immutable and its parsed request list never changes.
static OPENAPI_CACHE: std::sync::LazyLock<Mutex<HashMap<String, Arc<Vec<HttpReq>>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Debug)]
struct HttpReq {
    method: String,
    path: String,
    body: Option<String>,
    content_type: Option<String>,
}

/// Parse an OpenAPI 3.x spec (JSON or YAML) into a list of HTTP requests.
fn parse_openapi(spec: &str) -> std::result::Result<Vec<HttpReq>, mlua::Error> {
    let root: serde_json::Value = if spec.trim().starts_with('{') {
        serde_json::from_str(spec)
            .map_err(|e| mlua::Error::RuntimeError(format!("invalid OpenAPI JSON: {}", e)))?
    } else {
        let yaml_val: serde_yaml::Value = serde_yaml::from_str(spec)
            .map_err(|e| mlua::Error::RuntimeError(format!("invalid OpenAPI YAML: {}", e)))?;
        serde_yaml::to_string(&yaml_val)
            .map_err(|e| mlua::Error::RuntimeError(format!("failed to convert YAML: {}", e)))?;
        serde_json::from_str(&serde_json::to_string(&yaml_val).map_err(|e| {
            mlua::Error::RuntimeError(format!("YAML to JSON conversion failed: {}", e))
        })?)
        .map_err(|e| mlua::Error::RuntimeError(format!("invalid OpenAPI YAML: {}", e)))?
    };

    let paths = root
        .get("paths")
        .and_then(|v| v.as_object())
        .ok_or_else(|| mlua::Error::RuntimeError("OpenAPI spec missing 'paths'".to_string()))?;

    let mut reqs = Vec::new();
    for (path, methods) in paths {
        if let Some(methods_obj) = methods.as_object() {
            for (method, _op) in methods_obj {
                let method_upper = method.to_uppercase();
                if matches!(
                    method_upper.as_str(),
                    "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS"
                ) {
                    reqs.push(HttpReq {
                        method: method_upper,
                        path: path.clone(),
                        body: None,
                        content_type: None,
                    });
                }
            }
        }
    }

    if reqs.is_empty() {
        return Err(mlua::Error::RuntimeError(
            "no HTTP methods found in OpenAPI spec".to_string(),
        ));
    }

    Ok(reqs)
}

pub fn register(lua: &Lua, workload: &Table, ctx: &BindingContext) -> Result<()> {
    let state = Arc::clone(&ctx.state);
    let client = ctx.http.clone();

    let run_fn = lua.create_async_function(move |lua, (id, opts): (String, Table)| {
        let state = Arc::clone(&state);
        let client = client.clone();

        async move {
            let duration_secs: u64 = opts.get("duration_secs").unwrap_or(10);
            let rate: u64 = opts.get("rate").unwrap_or(10);

            // Build request list from either OpenAPI spec or manual requests table
            let reqs: Arc<Vec<HttpReq>> =
                if let Ok(openapi_path) = opts.get::<String>("openapi") {
                    if let Some(cached) = OPENAPI_CACHE
                        .lock()
                        .expect("poisoned openapi cache")
                        .get(&openapi_path)
                    {
                        Arc::clone(cached)
                    } else {
                        let spec = std::fs::read_to_string(&openapi_path).map_err(|e| {
                            mlua::Error::RuntimeError(format!(
                                "cannot read OpenAPI spec '{}': {}",
                                openapi_path, e
                            ))
                        })?;
                        let parsed = Arc::new(parse_openapi(&spec)?);
                        OPENAPI_CACHE
                            .lock()
                            .expect("poisoned openapi cache")
                            .insert(openapi_path, Arc::clone(&parsed));
                        parsed
                    }
                } else if let Ok(reqs_tbl) = opts.get::<Table>("requests") {
                    let mut reqs = Vec::new();
                    let len = reqs_tbl.raw_len();
                    for i in 1..=len {
                        let entry: Table = reqs_tbl.raw_get(i as i64).map_err(|_| {
                            mlua::Error::RuntimeError(format!("requests[{}] is not a table", i))
                        })?;
                        let method: String =
                            entry.get("method").unwrap_or_else(|_| "GET".to_string());
                        let path: String = entry.get("path").map_err(|_| {
                            mlua::Error::RuntimeError("each request needs a 'path' field".to_string())
                        })?;
                        let body: Option<String> = entry.get("body").ok();
                        let content_type: Option<String> = entry.get("content_type").ok();
                        reqs.push(HttpReq {
                            method,
                            path,
                            body,
                            content_type,
                        });
                    }
                    if reqs.is_empty() {
                        return Err(mlua::Error::RuntimeError(
                            "requests list is empty".to_string(),
                        ));
                    }
                    Arc::new(reqs)
                } else {
                    // Default: GET /get
                    Arc::new(vec![HttpReq {
                        method: "GET".to_string(),
                        path: "/get".to_string(),
                        body: None,
                        content_type: None,
                    }])
                };

            let (host, timeout, retries, delay) = {
                let state = state.lock().expect("poisoned engine state lock");
                resolve_subject_http(&state, &id)?
            };

            let start = Instant::now();
            let end = start + Duration::from_secs(duration_secs);
            let interval = Duration::from_millis(1000 / rate.max(1));

            let mut total = 0u64;
            let mut ok = 0u64;
            let mut fail = 0u64;
            let mut total_latency_ms = 0u64;
            let mut max_latency_ms = 0u64;

            // Per-method counters
            let mut method_ok: HashMap<String, u64> = HashMap::new();
            let mut method_fail: HashMap<String, u64> = HashMap::new();

            while Instant::now() < end {
                let req_start = Instant::now();
                let req = &reqs[total as usize % reqs.len()];
                let url = format!("http://{host}{}", req.path);

                let mut success = false;
                for attempt in 0..retries {
                    let mut builder = client
                        .request(
                            req.method.parse().map_err(|e| {
                                mlua::Error::RuntimeError(format!(
                                    "invalid method '{}': {}",
                                    req.method, e
                                ))
                            })?,
                            &url,
                        )
                        .timeout(Duration::from_secs(timeout));

                    if let Some(ref ct) = req.content_type {
                        builder = builder.header("content-type", ct.as_str());
                    }
                    if let Some(ref body) = req.body {
                        builder = builder.body(body.clone());
                    }

                    match builder.send().await {
                        Ok(resp) if resp.status().is_success() => {
                            success = true;
                            let _ = resp.text().await;
                            break;
                        }
                        Ok(resp) => {
                            let _ = resp.text().await;
                            break;
                        }
                        Err(_) => {
                            if attempt < retries - 1 {
                                tokio::time::sleep(Duration::from_millis(delay)).await;
                            }
                        }
                    }
                }

                let elapsed = req_start.elapsed().as_millis() as u64;
                total += 1;
                if success {
                    ok += 1;
                    *method_ok.entry(req.method.clone()).or_insert(0) += 1;
                } else {
                    fail += 1;
                    *method_fail.entry(req.method.clone()).or_insert(0) += 1;
                }
                total_latency_ms += elapsed;
                if elapsed > max_latency_ms {
                    max_latency_ms = elapsed;
                }

                tokio::time::sleep(interval.saturating_sub(req_start.elapsed())).await;
            }

            let avg_latency = total_latency_ms.checked_div(total).unwrap_or(0);

            let t = lua.create_table()?;
            t.set("total_requests", total)?;
            t.set("ok", ok)?;
            t.set("failed", fail)?;
            t.set("avg_latency_ms", avg_latency)?;
            t.set("max_latency_ms", max_latency_ms)?;
            t.set("duration_secs", duration_secs)?;
            t.set("rate", rate)?;

            // Per-method breakdown
            let breakdown = lua.create_table()?;
            for req in reqs.iter() {
                let mt = lua.create_table()?;
                mt.set("ok", method_ok.get(&req.method).copied().unwrap_or(0))?;
                mt.set("failed", method_fail.get(&req.method).copied().unwrap_or(0))?;
                breakdown.set(format!("{} {}", req.method, req.path), mt)?;
            }
            t.set("breakdown", breakdown)?;

            Ok(t)
        }
    })?;

    workload.set("http", run_fn)?;
    Ok(())
}
