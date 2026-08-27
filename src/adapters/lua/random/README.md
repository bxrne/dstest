# random

Seeded reproducible randomness for workload generation. Exposes
`dstest.random.*` (namespaced on the `dstest` table).

All draws come from a single `StdRng` seeded by the first config's seed,
independent of the fault tree's RNG stream (so workload draws don't
perturb the fault schedule). Lua is single-threaded, so the stream is
deterministic for a given seed + call order.

## `dstest.random.int(min, max)`

Returns a random integer in the range `[min, max)`.

```lua
local port = dstest.random.int(1024, 65536)
```

## `dstest.random.float()`

Returns a random float in `[0.0, 1.0)`.

```lua
local jitter = dstest.random.float()
```

## `dstest.random.bool(p?)`

Returns a random boolean. Optional probability `p` (default `0.5`).

```lua
if dstest.random.bool(0.3) then
    dstest.info("30% chance event fired")
end
```

| Argument | Type | Default | Description |
|----------|------|---------|-------------|
| `p` | number | `0.5` | Probability of `true` (0.0–1.0) |

## `dstest.random.choice(tbl)`

Returns a random element from a 1-indexed table. Returns `nil` for an
empty table.

```lua
local method = dstest.random.choice({ "GET", "POST", "PUT", "DELETE" })
```

## `dstest.random.shuffle(tbl)`

Shuffles a 1-indexed table in place using the Fisher–Yates algorithm
with the seeded RNG.

```lua
local items = { 1, 2, 3, 4, 5 }
dstest.random.shuffle(items)
-- items is now in a deterministic random order
```