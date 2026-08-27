# clock

Real-time and virtual clock access. Exposes `dstest.clock`, `dstest.clock.now`,
and `dstest.clock.virtual` (flat on the `dstest` table).

## `dstest.clock()` / `dstest.clock.now()`

Returns the real wall-clock time of the host (the harness clock).

```lua
local now = dstest.clock()
dstest.info("now: " .. now.secs .. "s")
```

| Field | Type | Description |
|-------|------|-------------|
| `nanos` | number | Nanoseconds since UNIX epoch |
| `micros` | number | Microseconds since UNIX epoch |
| `millis` | number | Milliseconds since UNIX epoch |
| `secs` | number | Seconds since UNIX epoch |

`dstest.clock()` is callable as a function (via `__call` metamethod) — it is
equivalent to `dstest.clock.now()`.

## `dstest.clock.virtual(subject_id)`

Returns a per-subject virtual clock handle. The virtual clock is backed by the
substrate's `ClockControl` implementation; substrates without virtual clock
support return a "not supported" error.

```lua
local vc = dstest.clock.virtual(subject)
```

### Virtual clock methods

| Method | Description |
|--------|-------------|
| `vc:now()` | Returns the current virtual time as `{ millis, secs, nanos, micros }` |
| `vc:advance(delta_ms)` | Advance the virtual clock by `delta_ms` milliseconds |
| `vc:set_offset(offset_ms)` | Set a fixed offset from real time (milliseconds) |
| `vc:set_rate(rate)` | Set the clock rate multiplier (e.g. `2.0` = 2x speed) |
| `vc:freeze()` | Freeze the clock at its current value |
| `vc:release()` | Release the clock — it resumes tracking real time |
| `vc:state()` | Returns `{ virtualised, epoch_millis, offset_millis, rate, frozen }` |

### Virtual clock setup

Subjects must opt in at creation time:

```lua
local s = dstest.setup(cfg, {
    image = "kennethreitz/httpbin",
    ports = { 80 },
    clock = { virtual = true, start_epoch = 1600000000 },
})
```

| Field | Type | Description |
|-------|------|-------------|
| `virtual` | boolean | Enable the virtual clock |
| `start_epoch` | number | Unix epoch seconds to pin the clock at (defaults to real now) |

With a virtual clock, `CLOCK_REALTIME` / `time()` is frozen at the start epoch
and only moves when you call `:advance()` or `:set_offset()`. `CLOCK_MONOTONIC`
is not faked — sleeps and busy-waits use real elapsed time, which is correct
for DST (the virtual clock only moves when dstest says so).

Limitations: only dynamically linked glibc binaries are affected (musl/static
binaries — e.g. Alpine, Go — ignore `LD_PRELOAD`). `set_rate` and `release`
are unsupported on the manual clock.