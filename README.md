# dstest

[![CI](https://github.com/bxrne/dstest/actions/workflows/ci.yml/badge.svg)](https://github.com/bxrne/dstest/actions/workflows/ci.yml)
[![Release](https://github.com/bxrne/dstest/actions/workflows/release.yml/badge.svg)](https://github.com/bxrne/dstest/actions/workflows/release.yml)
![Tag](https://img.shields.io/github/v/tag/bxrne/dstest?include_prereleases&sort=semver&style=flat)

Deterministic Simulation Testing for containerised services.

Write Lua scripts to define, control, and verify chaos experiments on Docker containers with reproducible fault injection.

Workload generation, walkable fault trees and benefit of the LuaJIT std lib.

## Installation

From crates.io:

```bash
cargo install dstest

# Run a script file
dstest < examples/oracle.lua

# Or use REPL mode: type your script interactively, press Ctrl+D to execute
dstest
```

Or build from source:

```bash
git clone https://github.com/bxrne/dstest
cd dstest
cargo build --release
```

## Quick Start

```bash
# Run a script file
cat examples/oracle.lua | cargo run

# Or use REPL mode: type your script and press Ctrl+D to run it
cargo run
```

## Overview

dstest lets you write Lua scripts that define test subjects (Docker containers), inject faults (pause, kill, resource deprivation, proxied network impairments), and verify service resilience: including virtual clocks for time-dependent logic, seeded workload randomness, `depends` for multi-service startup ordering, and sustained workload generation from OpenAPI specs.

> [!IMPORTANT]
> **Deterministic Execution & Workloads**
> While fault selection and schedule generation are seed-deterministic, standard container execution under default runtimes (`runc`) is subject to OS process/thread scheduling variance and wall-clock timing jitter. To make container execution and workload timing fully deterministic across runs, subjects must be configured with the [`dtrun`](https://github.com/bxrne/dtrun) OCI runtime:
>
> ```lua
> local s = dstest.setup(cfg, {
>     image = "my-service:latest",
>     runtime = "dtrun", -- Enables deterministic execution via dtrun
>     ports = { 8080 },
> })
> ```

## Examples

- `openapi.lua` - Drive sustained HTTP workload from an OpenAPI spec file
- `oracle.lua` - Fault injection with oracle predicates and invariants
- `link.lua` - Proxied network faults: latency, loss, partitions between subjects
- `partition.lua` - Directional link partitions and latency/loss measured through a proxy
- `clock.lua` - Virtual clock control: freeze, advance, offset, rate
- `tcp.lua` - Raw TCP protocol exchange over `dstest.net.tcp`
- `storage.lua` - Virtual disk faults: corrupt, snapshot, restore, I/O errors
- `orchestrate.lua` - Full fault-schedule orchestration with `run_steps` and oracles
- `pg.lua` - PostgreSQL: connect, create table, insert, query, close

## Documentation

See [DOCS.md](DOCS.md) for the full Lua API reference.

## AI Assistant Support

This repo includes an AI skill (`SKILL.md`) that teaches assistants how to work with dstest.

**To use with your agent:**

```bash
# Claude Code / Opencode
cp SKILL.md ~/.config/opencode/skills/dstest/SKILL.md

# Other agents (e.g., ~/.agents/skills/)
mkdir -p ~/.agents/skills/dstest
cp SKILL.md ~/.agents/skills/dstest/SKILL.md
```

Then instruct your assistant to "use the dstest skill" when writing or debugging chaos experiments.

## Requirements

- A Docker-compatible daemon reachable over `DOCKER_HOST` (Docker or Podman
  with `DOCKER_HOST` pointed at the Podman socket; `docker` may be aliased to
  `podman`)
- Rust 1.85+ (uses 2024 edition)
- [Zig](https://ziglang.org/) when building from source: `build.rs`
  cross-compiles the virtual-clock shim (`shim/clock.c`) to a Linux x86-64
  ELF with `zig cc`. Install via `nix profile install nixpkgs#zig` or your
  package manager. Not needed for `cargo install` from a prebuilt crate.

## License

MIT
