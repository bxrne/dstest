--- @diagnostic disable:undefined-global
--- Full orchestration: a pre-configured fault schedule driven by
--- dstest.dst.run_steps, guarded by an oracle predicate, and enriched with
--- seeded-random workload hints.
--- Run: cat examples/orchestrate.lua | cargo run
---
--- Demonstrates combining a weights table, single accumulation, a predicate,
--- and a coroutine generator that frames each fault step for the log.

local cfg = dstest.config({
    substrate = "docker",
    seed = 0x0A11,
    weights = {
        pause = 0.4,
        kill = 0.2,
        ["deprive:memory"] = 0.2,
        ["deprive:cpu"] = 0.2,
    },
    accumulation = "single",
    steps = 6,
    step_delay = 200,
    http_retries = 12,
    http_retry_delay = 250,
})

local s = dstest.setup(cfg, {
    image = "kennethreitz/httpbin",
    ports = { 80 },
})

-- A predicate reports whether the subject stayed healthy during a fault that
-- should be non-fatal. Hard kills are excluded because taking the process down
-- is the fault's intended effect, not an unexpected failure.
dstest.dst.oracle.predicate("http_healthy", function(subject, fault, round)
    if fault == "kill" then
        return true
    end
    local ok, resp = pcall(dstest.net.http, subject, "GET", "/get")
    if not ok then
        return { false, "request failed: " .. tostring(resp) }
    end
    if resp.status ~= 200 then
        return { false, "expected 200, got " .. resp.status }
    end
    return true
end)

-- Seeded randomness selects an adjective for each fault round, purely as
-- diagnostic garnish, to show the reproducible workload RNG.
local function adjectives()
    return coroutine.wrap(function()
        local pool = { "steady", "bursty", "quiet", "noisy", "spiky" }
        while true do
            coroutine.yield(pool[dstest.random.int(1, #pool + 1)])
        end
    end)
end

dstest.info(string.format("orchestrating config '%s' (%d-step schedule begins)", cfg, 6))

local gen = adjectives()
local steps = dstest.dst.run_steps(cfg, 6)
for i, step in ipairs(steps) do
    local mood = gen() -- advances the seeded RNG deterministically
    dstest.info(string.format(
        "step %d: fault=%-16s subject=%s round=%d/%d remaining=%d [%s]",
        i, step.fault, step.subject, step.round, step.total_rounds,
        step.remaining, mood
    ))
end

-- Every fault is cleared; the service must be healthy again at the end.
local ok, resp = pcall(dstest.net.http, s, "GET", "/get")
dstest.info(string.format(
    "post-run health: ok=%s status=%s", tostring(ok), tostring(resp and resp.status)
))

-- The engine derives the oracle report from its event log.
local report = dstest.dst.oracle.report()
dstest.info(string.format(
    "oracle: %d/%d checks passed (%d failed)",
    report.passed_checks, report.total_checks, report.failed_checks
))

dstest.dst.clear(s)
dstest.info("orchestrate example complete")
