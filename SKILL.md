---
name: dstest
description: Deterministic simulation testing for containerized services. Write Lua scripts to inject chaos (pause, kill, resource deprivation) into Docker containers with reproducible, seeded fault injection. Use when writing chaos experiments, testing service resilience, or debugging distributed systems.
license: MIT
metadata:
  author: bxrne
  version: "0.1.12"
---

# dstest

dstest is a deterministic chaos testing framework for Docker containers. Write Lua scripts that inject faults and verify system resilience.

## Quick Start

```bash
# Run an example
cat examples/oracle.lua | cargo run

# Build and install
cargo build --release
cargo install --path .
```

## Key Commands

| Command | Purpose |
|---------|---------|
| `cat examples/oracle.lua \| cargo run` | Run a script via stdin |
| `dstest < script.lua` | Run script (after install) |
| `cargo test` | Run test suite |
| `cargo clippy -- -D warnings` | Lint gate (CI-enforced) |
| `cargo doc --open` | Open API docs |

## Extra Capabilities

- **Virtual clocks**: pin a subject's `CLOCK_REALTIME` to an epoch and advance
  it deterministically via `dstest.clock.virtual(s)` (see `dstest.clock` in
  [DOCS.md](DOCS.md)).
- **Proxied network faults**: `dstest.net.link(a, b, port)` adds latency, loss,
  and partitions between two subjects (see `dstest.net` in [DOCS.md](DOCS.md)).
- **Seeded workloads**: `dstest.random.*` (int, float, bool, choice, shuffle)
  for reproducible non-fault randomness (see `dstest.random` in [DOCS.md](DOCS.md)).
- **Workload generation**: `dstest.workload.http` (manual or OpenAPI-driven) and
  `dstest.workload.pg` for sustained traffic with latency stats (see
  `dstest.workload` in [DOCS.md](DOCS.md)).
- **Startup ordering**: `depends = { ... }` in `dstest.setup` waits for
  upstream subjects to reach a running state as reported by the substrate.

For the complete Lua API surface, run `cargo doc --open` (or read [DOCS.md](DOCS.md)).

## Available Faults

| Fault | Effect |
|-------|--------|
| `pause` | Freeze container (cgroups) |
| `kill` | Kill container (SIGKILL) |
| `deprive:disk` | Throttle disk I/O to 1MB/s |
| `deprive:network` | Disconnect from bridge network |
| `deprive:memory` | Halve memory limit (min 64MB) |
| `deprive:cpu` | Limit CPU to 20% quota |

### Proxied Network Faults (`dstest.net.link`)

Beyond container-level network disconnect, `dstest.net.link(a, b, port)`
establishes a controllable link between two subjects for targeted impairments.
All impairment randomness derives from the experiment seed.

```lua
local link = dstest.net.link(gateway, payment, 8083)
link:latency(50, 20)      -- 50ms delay + 20ms jitter
link:loss(0.05)           -- 5% packet loss
link:partition({ direction = "a->b", mode = "blackhole" })
link:heal()
```

| Method | Description |
|--------|-------------|
| `link:latency(delay_ms, jitter_ms)` | Constant delay plus uniform jitter |
| `link:loss(pct)` | Probabilistic loss (0.0–1.0) |
| `link:partition({ direction, mode })` | Partition; `direction` is `"a->b"`, `"b->a"`, or `"both"` (default), `mode` is `"blackhole"` (default) or `"reset"` |
| `link:heal()` | Remove all impairments |

### Storage Faults (`dstest.storage.*`)

> **Removed:** the `dm-flakey` storage backend required root privileges and has been removed. Storage fault injection may return in a future unprivileged implementation (e.g. FUSE-based).

## Configuration

Call `dstest.config()` first: it returns a **handle**: then pass that handle
as the first argument to `dstest.setup()`. Full field reference:
[`src/adapters/lua/core/README.md`](src/adapters/lua/core/README.md).

```lua
local docker_config = dstest.config({
    substrate = "docker",      -- Required: must match the engine's compiled substrate
    seed = 42,                 -- Required: random seed for determinism
    weights = {                -- Optional: fault weights (defaults below)
        pause = 0.35,
        kill = 0.25,
        ["deprive:disk"] = 0.10,
        ["deprive:network"] = 0.10,
        ["deprive:memory"] = 0.10,
        ["deprive:cpu"] = 0.10,
    },
    accumulation = "single",   -- "single" (default) or "accumulate"
    steps = 10,                -- Total fault steps in the schedule
    http_timeout = 5,          -- HTTP timeout in seconds
    http_retries = 30,         -- HTTP retry attempts
    http_retry_delay = 500,    -- Delay between retries (ms)
    step_delay = 1000,         -- Delay before fault in single mode (ms)
})
```

## Core API

```lua
local s = dstest.setup(cfg, {
    image = "kennethreitz/httpbin",
    runtime = "dtrun",            -- Required for deterministic process execution & workload timing
    ports = { 80 },
    volumes = { "/absolute/host/path:/container:ro" },
    env = { DEBUG = "true" },
    cmd = { "python", "-m", "httpbin" },
    depends = { other_subject },  -- Wait for another subject to reach running state
})

-- Fault injection (namespaced under dstest.dst)
local result = dstest.dst.step(cfg)        -- Single fault (or step(cfg) with multiple configs)
local results = dstest.dst.run_steps(cfg, 5) -- Multiple faults
dstest.dst.clear(s)                        -- Clear active faults

-- HTTP and TCP (namespaced under dstest.net)
local resp = dstest.net.http(s, "GET", "/get")

-- Virtual clock (per-subject, seeded)
dstest.clock.virtual(s):advance(5000)     -- move subject clock +5s

-- Seeded reproducible randomness for workloads
local port = dstest.random.int(1024, 65536)
dstest.random.shuffle(my_array)

-- Container introspection (flat on dstest)
local info = dstest.inspect(s)
local logs = dstest.logs(s, { tail = "50" })
local exec_result = dstest.exec(s, {"ls", "-la", "/app"})
```

For the full API reference, see the per-module READMEs linked from
[`src/adapters/lua/README.md`](src/adapters/lua/README.md).

## Oracle (Automated Verification)

```lua
dstest.dst.oracle.predicate("health_check", function(subject, fault, round)
    if fault == "pause" or fault == "kill" then return true end
    local ok, resp = pcall(dstest.net.http, subject, "GET", "/health")
    return ok and resp.status == 200
end)

local report = dstest.dst.oracle.run(function()
    dstest.dst.run_steps(cfg, 10)
end)

print(report.passed, report.passed_checks, report.failed_checks)
```

Full oracle reference: [`src/adapters/lua/dst/README.md`](src/adapters/lua/dst/README.md).

## Common Patterns

### Health Check Loop
```lua
while true do
    local result = dstest.dst.step(cfg)
    if not result.more then break end

    if result.fault ~= "pause" and result.fault ~= "kill" then
        local ok, resp = pcall(dstest.net.http, s, "GET", "/get")
        if ok and resp.status == 200 then
            dstest.info("healthy")
        else
            dstest.warn("unhealthy")
        end
    end
end
```

### Multi-Service Testing
```lua
local backend = dstest.setup(cfg, { image = "myapp/backend", ports = { 8080 } })
local cache = dstest.setup(cfg, {
    image = "redis",
    ports = { 6379 },
    depends = { backend },  -- wait for backend to be running
})

dstest.dst.run_steps(cfg, 10)
dstest.dst.clear(backend)
dstest.dst.clear(cache)
```

## Determinism

Same seed = identical fault sequence. Register two configs with the same seed
and both produce the same schedule for their subjects:

```lua
local cfg1 = dstest.config({ substrate = "docker", seed = 42 })
local s1 = dstest.setup(cfg1, { image = "kennethreitz/httpbin", ports = { 80 } })
local r1 = dstest.dst.run_steps(cfg1, 5)
-- re-running the whole script with seed 42 yields the identical schedule
```

Oracle failures make the process exit with code `2` (script errors `1`,
infra errors `3`): CI fails without explicit `error()` calls.

## Logging

```lua
dstest.debug("verbose details")
dstest.info("normal progress")
dstest.warn("something concerning")
dstest.error("failure occurred")
```

## Examples

| File | Demonstrates |
|------|--------------|
| [`examples/oracle.lua`](examples/oracle.lua) | Fault injection with oracle predicates and invariants |
| [`examples/httpbin.lua`](examples/httpbin.lua) | httpbin smoke: check a route, cut the link, verify failure, heal, verify recovery |
| [`examples/link.lua`](examples/link.lua) | Proxied network faults: latency, loss, partitions between subjects |
| [`examples/pg.lua`](examples/pg.lua) | PostgreSQL: connect, create table, insert, query, close |

## Writing Scripts

Scripts are Lua and read from stdin. Use `pcall` for error handling since HTTP may
fail during faults:

```lua
local ok, resp = pcall(dstest.net.http, s, "GET", "/get")
if not ok then
    dstest.warn("request failed: " .. tostring(resp))
end
```

## Requirements

- Docker daemon running
- Rust 1.85+ (uses 2024 edition)
