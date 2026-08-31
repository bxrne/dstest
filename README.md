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
- `clock.lua` - Virtual clock control: freeze, advance, offset (manual clock)
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

## Environment-limited examples

Four examples need a real Linux Docker environment that a macOS + podman-machine
host cannot provide:

| Example      | Why it fails on podman-machine (macOS)                                    |
|--------------|-----------------------------------------------------------------------------|
| `httpbin.lua`   | The link proxy's bridge IP (`10.88.x.x`) is unreachable from the host process |
| `link.lua`      | Same bridge-reachability requirement                                       |
| `partition.lua` | Same bridge-reachability requirement                                       |
| `storage.lua`   | Needs root + device-mapper (`losetup`, `dmsetup`, `mkfs.ext4`, `mount`) and the `dm-flakey` kernel module |

The harness host process must be able to dial the Docker bridge IP that the link
proxy binds; from inside the macOS podman-machine VM the sandbox bridge is not
routable, and the VM kernel does not expose `dm-flakey`. These are environment
limitations, not example bugs.

These four examples are not exercised by the automated checks on this host and no
in-repo workaround exists: running them requires a real Linux Docker host where
the bridge IP is reachable from the `dstest` process and root/device-mapper with
`dm-flakey` is available. They are documented for completeness and are expected
to fail here.

## Requirements

- A Docker-compatible daemon reachable over `DOCKER_HOST` (Docker or Podman
  with `DOCKER_HOST` pointed at the Podman socket; `docker` may be aliased to
  `podman`)
- Rust 1.85+ (uses 2024 edition)
- [Zig](https://ziglang.org/): `build.rs` cross-compiles the virtual-clock
  shim (`shim/clock.c`) to a Linux x86-64 ELF pinned to the glibc 2.17
  baseline with `zig cc`, so it loads into any glibc-based subject. Zig is
  required on every build platform, including Linux and CI. Install via
  `nix profile install nixpkgs#zig` or your package manager.

## License

MIT
