# dstest Documentation

## Usage

dstest reads a Lua script from stdin. Pipe in a script file or type interactively.

```bash
# Run a script file
dstest < examples/oracle.lua
cat examples/oracle.lua | dstest

# REPL mode: run dstest with no redirect, type your script, press Ctrl+D to execute
dstest
```

In REPL mode, dstest reads until EOF (Ctrl+D), then executes the entire script.
Logging output is printed to stderr as the script runs.

## Lua API

The global `dstest` table is the entry point. Some functions are flat on `dstest`,
others are namespaced under sub-tables.

| Namespace | Functions | Reference |
|-----------|-----------|-----------|
| `dstest.config`, `dstest.setup` | experiment config, create subjects | [`src/adapters/lua/core/README.md`](src/adapters/lua/core/README.md) |
| `dstest.dst.step`, `dstest.dst.run_steps`, `dstest.dst.clear` | fault injection | [`src/adapters/lua/dst/README.md`](src/adapters/lua/dst/README.md) |
| `dstest.dst.oracle.*` | predicates, invariants, reports | [`src/adapters/lua/dst/README.md`](src/adapters/lua/dst/README.md) |
| `dstest.net.http`, `dstest.net.tcp`, `dstest.net.link` | HTTP, TCP, subject links | [`src/adapters/lua/net/README.md`](src/adapters/lua/net/README.md) |
| `dstest.inspect`, `dstest.logs`, `dstest.exec` | container introspection | [`src/adapters/lua/subs/README.md`](src/adapters/lua/subs/README.md) |
| `dstest.pg.connect`, `dstest.pg.query`, `dstest.pg.close` | PostgreSQL | [`src/adapters/lua/pg/README.md`](src/adapters/lua/pg/README.md) |
| `dstest.clock`, `dstest.clock.now`, `dstest.clock.virtual` | timestamps, virtual clocks | [`src/adapters/lua/clock/README.md`](src/adapters/lua/clock/README.md) |
| `dstest.storage.*` | virtual disk faults | [`src/adapters/lua/storage/README.md`](src/adapters/lua/storage/README.md) |
| `dstest.random.*` | seeded reproducible randomness | [`src/adapters/lua/random/README.md`](src/adapters/lua/random/README.md) |
| `dstest.workload.http`, `dstest.workload.pg` | sustained HTTP/PG traffic with stats | [`src/adapters/lua/workload/README.md`](src/adapters/lua/workload/README.md) |
| `dstest.debug`, `dstest.info`, `dstest.warn`, `dstest.error` | logging | [`src/adapters/lua/log/README.md`](src/adapters/lua/log/README.md) |

## Configs and subjects

`dstest.config({...})` registers a named configuration and returns a **handle**.
`dstest.setup(handle, {...})` creates a subject linked to that config (and its
substrate). The setup options table accepts `image`, optional OCI `runtime`
(e.g. `"dtrun"`, `"runc"`, `"crun"`, `"gvisor"`), `ports`, `volumes`, `env`,
`cmd`, `clock`, `network`, `storage`, and `depends`. Multiple configs can
coexist; `dstest.dst.step()` / `dstest.dst.run_steps()` accept an optional
config handle when more than one is registered. `dstest.setup`'s `depends`
field waits for upstream subjects to reach a running state (reported by the
substrate) before creating the dependent subject.

## Podman / Docker-machine hosts

dstest talks to the Docker API over `DOCKER_HOST` (via [bollard](https://crates.io/crates/bollard)),
so it works unchanged against Podman when `DOCKER_HOST` points at the Podman
socket (and `docker` is aliased to `podman`).

One gotcha: on a podman/docker *machine* VM (e.g. Podman Machine on macOS) the
container's bridge `ip` is **not** reachable from the macOS host process. Use
the host-mapped `host` address returned by `dstest.inspect` (it is
`host:port`, derived from the published port) to dial services from the
orchestrator — `examples/pg.lua` does exactly this. Likewise, the proxied
network link's readiness probe is bounded so it cannot hang when the host
cannot reach the proxy's bridge IP.

All faults are container/OCI-level (pause, kill, network disconnect, memory,
CPU, and an OCI `blkio` storage fault for disk), so none of them depend on
resolving a host block device — they run under Podman Machine too.

## Default Weights

| Fault | Weight |
|-------|--------|
| `pause` | 0.35 |
| `kill` | 0.25 |
| `deprive:disk` | 0.10 |
| `deprive:network` | 0.10 |
| `deprive:memory` | 0.10 |
| `deprive:cpu` | 0.10 |

## Accumulation Modes

- `single`: Each subject can have only one active fault. Previous faults are cleared before applying new ones.
- `accumulate`: Multiple faults can stack on the same subject.

## Fault Types

| Fault | Effect |
|-------|--------|
| `pause` | Freezes the container (cgroups freeze) |
| `kill` | Kills the container (SIGKILL) |
| `deprive:disk` | OCI block-IO fault: drops the container's blkio weight to 50 (plus a best-effort 1MB/s per-device read/write cap when a throttleable host device resolves) |
| `deprive:network` | Disconnects from bridge network (no internet) |
| `deprive:memory` | Reduces memory limit to 50% of current (min 64MB) |
| `deprive:cpu` | Limits CPU to 20% quota |

## Determinism

Same seed produces identical fault schedules. The fault schedule is a pure
function of the config's `seed`, `weights`, `steps`, and subject set:

```lua
local cfg = dstest.config({ substrate = "docker", seed = 42, steps = 10 })
local s = dstest.setup(cfg, { image = "kennethreitz/httpbin", runtime = "dtrun", ports = { 80 } })
local results = dstest.dst.run_steps(10)
-- re-running this script with seed 42 yields the identical fault sequence
```

### Determinism contract

Guaranteed reproducible for a given seed:

- the fault schedule (types, targets, order): weight iteration order is
  sorted, so schedules do not depend on map ordering;
- Lua's `math.random` (seeded from `seed` via `math.randomseed`).

> [!IMPORTANT]
> **Deterministic Execution & Workloads (`dtrun`)**
> Standard process execution on Docker (`runc`) is non-deterministic due to OS thread scheduling variance, wall-clock timing, and I/O timing. To ensure that service execution and workload timing are completely deterministic across runs, subjects must be created using the [`dtrun`](https://github.com/bxrne/dtrun) OCI runtime (`runtime = "dtrun"`). Pin container images by digest to prevent image drift.

## Exit codes

| Code | Meaning |
|------|---------|
| `0` | success |
| `1` | script error |
| `2` | oracle check failures |
| `3` | infrastructure error (e.g. Docker daemon unreachable) |

## Examples

| File | Demonstrates |
|------|--------------|
| [`examples/oracle.lua`](examples/oracle.lua) | Fault injection with oracle predicates and invariants |
| [`examples/httpbin.lua`](examples/httpbin.lua) | httpbin smoke: check a route, cut the link, verify failure, heal, verify recovery |
| [`examples/link.lua`](examples/link.lua) | Proxied network faults: latency, loss, partitions between subjects |
| [`examples/pg.lua`](examples/pg.lua) | PostgreSQL: connect, create table, insert, query, close |
