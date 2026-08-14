# core

Experiment configuration and subject setup. Exposes `dstest.config` and `dstest.setup`
(flat on the `dstest` table).

## `dstest.config(options) -> handle`

Registers a named experiment configuration and returns its **handle** (a string).
The handle links every `dstest.setup()` to a config — and through it, to a
substrate — so multiple configs can coexist (e.g. different seeds, weights, or
in future, different substrates).

```lua
local docker_config = dstest.config({
    name = "docker_config",          -- optional; auto-generated if omitted
    substrate = "docker",
    seed = 42,
    weights = { pause = 0.5, kill = 0.3, ["deprive:disk"] = 0.2 },
    accumulation = "single",
    steps = 10,
    http_timeout = 10,
})

local s = dstest.setup(docker_config, { image = "kennethreitz/httpbin", ports = { 80 } })
```

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `name` | string | No | `config_N` | Handle name; must be unique |
| `substrate` | string | Yes | - | Substrate name; must match the engine's compiled substrate (`"docker"`) |
| `seed` | number | Yes | - | Random seed for deterministic fault selection (also seeds Lua's `math.random`, network impairments, and storage `corrupt`) |
| `weights` | table | No | [default weights](../../../DOCS.md#default-weights) | Fault-type weights; normalized to sum to 1.0 |
| `accumulation` | string | No | `"single"` | `"single"` (clear before each fault) or `"accumulate"` (stack) |
| `steps` | number | No | `10` | Total fault steps in this config's schedule |
| `http_timeout` | number | No | `5` | HTTP request timeout in seconds |
| `http_retries` | number | No | `30` | HTTP retry attempts |
| `http_retry_delay` | number | No | `500` | Delay between HTTP retries (ms) |
| `step_delay` | number | No | `1000` | Delay before applying fault in single mode (ms) |
| `require_seed` | boolean | No | `true` | Require seed before `step()`/`run_steps()` |

Each call creates a fresh config from defaults — there is no global mutable
config. Handles are deterministic: unnamed configs are `config_1`, `config_2`, …
in registration order.

## `dstest.setup(config_handle, options)`

Creates a test subject (container) under the given config. Returns a subject ID
string `"<substrate>/<id>"` (e.g. `"docker/abc123"`).

```lua
local subject = dstest.setup(docker_config, {
    image = "kennethreitz/httpbin",
    ports = { 80 },
    volumes = { "/host/path:/container/path:ro" },
    env = { DEBUG = "true", LOG_LEVEL = "info" },
    cmd = { "python", "-m", "httpbin" },
})
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `image` | string | Yes | Container image to pull and run (pin by digest for reproducibility) |
| `runtime` | string | No | OCI runtime for container execution (e.g. `"dtrun"`). **Required (`runtime = "dtrun"`) for deterministic process execution and workload timing across runs.** |
| `ports` | table | No | Container ports to expose; host side is **ephemeral** (Docker-assigned), first port's mapping is used for `http`/`tcp` |
| `volumes` | table | No | Array of bind mounts (`host:container[:options]`). Host path must be absolute. |
| `env` | table | No | Key-value table of environment variables |
| `cmd` | table | No | Array of command arguments overriding the entrypoint |
| `depends` | table | No | Array of subject IDs to wait for (substrate-reported running state) before creating this subject |

### `depends`

```lua
local db = dstest.setup(cfg, { image = "postgres:16-alpine", ports = { 5432 } })
local app = dstest.setup(cfg, {
    image = "myapp",
    ports = { 8080 },
    depends = { db },   -- blocks until db's container state is "running"
})
```

Each entry is a subject ID (the string returned by a prior `dstest.setup`).
Before pulling/creating/starting the dependent subject, `setup` polls the
dependency's status via the substrate (e.g. Docker container state) with a
500ms interval until it reports `running` (max 60s). If the dependency
terminates before becoming ready, setup errors immediately rather than
polling to timeout.

### Virtual clock

Subjects can opt into a harness-controlled virtual clock:

```lua
local s = dstest.setup(cfg, {
    image = "kennethreitz/httpbin",
    ports = { 80 },
    clock = { virtual = true, start_epoch = 1600000000 },
})
```

| Field | Type | Description |
|-------|------|-------------|
| `virtual` | boolean | Set `true` to enable the virtual clock |
| `start_epoch` | number | Unix epoch seconds to pin the clock at (defaults to real now) |

With a virtual clock, the subject's `CLOCK_REALTIME` / `time()` is frozen at
the start epoch and only moves when you call `dstest.clock.virtual(subject)`
methods (`:advance(ms)`, `:set_offset(ms)`, `:now()`). `CLOCK_MONOTONIC` is
not faked — sleeps and busy-waits use real elapsed time, which is correct for
DST (the virtual clock only moves when dstest says so).

Limitations: only dynamically linked glibc binaries are affected (musl/static
binaries — e.g. Alpine, Go — ignore `LD_PRELOAD`). `set_rate` and `release`
are unsupported on the manual clock.

Containers are named `dstest-<config>-<n>` and labelled `dstest.managed=true`;
a name collision with a stale dstest container is cleaned up automatically.

> **Note:** subjects created *after* the first `dstest.dst.step()` for a config
> are not part of that config's fault schedule (the schedule snapshots its
> subject set on first step).
