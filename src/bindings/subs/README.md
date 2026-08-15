# subs

Subject introspection — container state, logs, and command execution. Exposes
`dstest.inspect`, `dstest.logs`, `dstest.exec` (flat on the `dstest` table).

## `dstest.inspect(subject)`

Returns container metadata and runtime state.

```lua
local info = dstest.inspect(subject)
assert(info.state == "running", "container should be running")

-- Dial the host-mapped address when the bridge IP is not reachable from the
-- orchestrator (e.g. podman/docker machine on macOS):
local addr = info.host or info.ip
```

| Field | Type | Description |
|-------|------|-------------|
| `state` | string | `"running"`, `"paused"`, `"exited"`, or `"dead"` |
| `pid` | number? | Process ID |
| `ip` | string? | Container IP address (bridge network) |
| `host` | string? | Host-mapped `host:port` address (present when ports are published). Dial this from the orchestrator process — the bridge `ip` is not always reachable from the host (e.g. a podman/docker machine VM) |
| `memory_limit` | number? | Memory limit in bytes |
| `cpu_quota` | number? | CPU quota (0.0-1.0) |

## `dstest.logs(subject, options?)`

Fetches container logs. Returns an array of log entries.

```lua
local logs = dstest.logs(subject, { tail = "50", timestamps = true })
for _, entry in ipairs(logs) do
    if entry.stream == "stderr" then
        dstest.warn(entry.message)
    end
end
```

### Options

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `stdout` | boolean | `true` | Include stdout stream |
| `stderr` | boolean | `true` | Include stderr stream |
| `tail` | string | `"all"` | Number of lines (`"50"`, `"all"`) |
| `since` | number | - | UNIX timestamp (seconds) |
| `timestamps` | boolean | `false` | Include timestamps |

### Entry shape

| Field | Type | Description |
|-------|------|-------------|
| `stream` | string | `"stdout"` or `"stderr"` |
| `message` | string | Log line content |

## `dstest.exec(subject, command)`

Runs a command inside the container and returns the result.

```lua
local result = dstest.exec(subject, {"ls", "-la", "/app"})
if result.exit_code == 0 then
    dstest.info("files:\n" .. result.stdout)
else
    dstest.error("exec failed: " .. result.stderr)
end
```

| Field | Type | Description |
|-------|------|-------------|
| `exit_code` | number | Exit code (`-1` if unknown) |
| `stdout` | string | Standard output |
| `stderr` | string | Standard error |
