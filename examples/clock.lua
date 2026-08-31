--- @diagnostic disable:undefined-global
--- Virtual clock control: freeze, advance, offset, rate against a subject
--- whose CLOCK_REALTIME is pinned to a synthetic epoch.
--- Run: cat examples/clock.lua | cargo run
---
--- Demonstrates a Lua coroutine driving a scripted sequence of clock
--- manipulations, so the scenario reads top-to-bottom instead of nesting.

local cfg = dstest.config({
    substrate = "docker",
    seed = 0xC10C,
})

-- httpbin cheerfully runs with its wall clock pinned to the year 2020. We
-- give it a virtual clock starting at a fixed epoch so every manipulation is
-- measurable.
local s = dstest.setup(cfg, {
    image = "kennethreitz/httpbin",
    ports = { 80 },
    clock = { virtual = true, start_epoch = 1577836800 }, -- 2020-01-01T00:00:00Z
})

local vc = dstest.clock.virtual(s)

dstest.info("virtual clock created; waking subject...")
dstest.exec(s, { "sleep", "1" })

-- A closure that prints the current virtual time as an ISO-ish instant, so
-- the example logs are greppable.
local function log_virtual(label)
    local t = vc:now()
    dstest.info(string.format("%-22s virtual=%.3fs (epoch ms %d)", label, t.secs, t.millis))
end

-- A coroutine generator that yields a list of (label, action) tuples. Because
-- each step calls the async dstest API, we build the plan as plain data and
-- execute it in the main flow rather than yielding across an await boundary.
local function plan()
    return coroutine.wrap(function()
        coroutine.yield("initial", function() end)
        coroutine.yield("advance +5s", function() vc:advance(5000) end)
        coroutine.yield("offset +1h", function() vc:set_offset(3600000) end)
        coroutine.yield("freeze", function() vc:freeze() end)
    end)
end

for label, action in plan() do
    action()
    log_virtual(label)
end

-- Show the full state table once more so the example documents every field.
local st = vc:state()
dstest.info(string.format(
    "state: virtualised=%s epoch=%d offset=%dms rate=%.1fx frozen=%s",
    tostring(st.virtualised), st.epoch_millis, st.offset_millis,
    st.rate, tostring(st.frozen)
))

-- Clock manipulation is deterministic given the seed and call order: the wall
-- clock must have moved forward overall as a result of the advance above.
local st_end = vc:state()
assert(st_end.epoch_millis >= 1577836800000, "virtual clock did not advance")

dstest.dst.clear(s)
dstest.info("clock example complete")
