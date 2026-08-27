# storage

Virtual disk fault injection. Exposes `dstest.storage.*` (namespaced on
the `dstest` table).

## Setup

Opt in at subject creation — the disk is prepared before the container
starts and bind-mounted at `mount`:

```lua
local s = dstest.setup(cfg, {
    image = "alpine:3.20",
    cmd = { "sleep", "300" },
    storage = { flaky = true, mount = "/data", size_mb = 64 },
})
```

Requires root on the host (loop devices + device-mapper `dm-flakey`).

## `dstest.storage.error(id, on)`

Toggle EIO on all I/O for the subject's virtual disk.

```lua
dstest.storage.error(subject, true)   -- inject I/O errors
dstest.storage.error(subject, false)  -- clear I/O errors
```

| Argument | Type | Description |
|----------|------|-------------|
| `id` | string | Subject ID (`"<substrate>/<id>"`) |
| `on` | boolean | `true` to inject errors, `false` to clear |

## `dstest.storage.drop_writes(id, on)`

ACK writes but discard them (write-drop / null-device mode).

```lua
dstest.storage.drop_writes(subject, true)
```

## `dstest.storage.corrupt(id, n)`

Flip `n` bytes on the virtual disk. Offsets are deterministic given
the experiment `seed`.

```lua
dstest.storage.corrupt(subject, 10)
```

| Argument | Type | Description |
|----------|------|-------------|
| `id` | string | Subject ID |
| `n` | number | Number of bytes to corrupt |

## `dstest.storage.snapshot(id)`

Snapshot the backing store and return a snapshot ID string.

```lua
local snap = dstest.storage.snapshot(subject)
```

## `dstest.storage.restore(id, snap)`

Restore a previously taken snapshot.

```lua
dstest.storage.restore(subject, snap)
```

| Argument | Type | Description |
|----------|------|-------------|
| `id` | string | Subject ID |
| `snap` | string | Snapshot ID returned by `snapshot` |

## `dstest.storage.slow(id, ms)`

Not supported on dm-flakey — returns a "not supported" error.

## `dstest.storage.attach(id, opts)`

Rejected — storage must be configured at `dstest.setup` time via the
`storage` field.