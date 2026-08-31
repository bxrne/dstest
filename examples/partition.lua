--- @diagnostic disable:undefined-global
--- Proxied network link impairments: latency, loss, and directional
--- partitions (blackhole vs reset), measured from inside a client subject.
--- Run: cat examples/partition.lua | cargo run
---
--- Uses a Lua coroutine to script a list of impairment scenarios, each of
--- which is applied and then measured through the proxy.

local cfg = dstest.config({
    substrate = "docker",
    seed = 0xD157,
    accumulation = "single",
})

local server = dstest.setup(cfg, {
    image = "kennethreitz/httpbin",
    ports = { 80 },
})

local client = dstest.setup(cfg, {
    image = "curlimages/curl:latest",
    cmd = { "sleep", "300" },
    depends = { server },
})

local link = dstest.net.link(client, server, 80)
local proxy_url = string.format("http://%s/get", tostring(link:addr()))

-- Wait for the proxy's socat to be listening.
for _ = 1, 20 do
    local r = dstest.exec(client, {
        "curl", "-s", "-o", "/dev/null", "-w", "%{http_code}",
        "--connect-timeout", "2", "--max-time", "5", proxy_url,
    })
    if tonumber(r.stdout) == 200 then break end
    dstest.exec(client, { "sleep", "1" })
end

-- Measure one HTTP round trip through the proxy from inside the client.
-- Returns ok (true iff the request completed with 200).
local function probe(label)
    local start = dstest.clock()
    local r = dstest.exec(client, {
        "curl", "-s", "-o", "/dev/null", "-w", "%{http_code}",
        "--connect-timeout", "2", "--max-time", "5", proxy_url,
    })
    local ms = (dstest.clock().nanos - start.nanos) / 1e6
    local code = tonumber(r.stdout)
    local ok = r.exit_code == 0 and code == 200
    dstest.info(string.format("  %-28s ok=%-5s code=%-4s %.0fms", label, tostring(ok), tostring(code), ms))
    return ok
end

-- A coroutine generator of (label, apply) scenario steps. The impairment
-- functions are async, so we build the plan as closures and run it in the
-- main flow, resuming between measurements.
local function scenarios()
    return coroutine.wrap(function()
        coroutine.yield("baseline", function() link:heal() end)
        coroutine.yield("latency 150+jitter 50", function() link:latency(150, 50) end)
        coroutine.yield("loss 25%", function() link:loss(0.25) end)
        coroutine.yield("partition a->b blackhole", function()
            link:partition({ direction = "a->b", mode = "blackhole" })
        end)
        coroutine.yield("partition both reset", function()
            link:partition({ direction = "both", mode = "reset" })
        end)
        coroutine.yield("heal", function() link:heal() end)
    end)
end

local results = {}
for label, apply in scenarios() do
    dstest.info("scenario: " .. label)
    apply()
    dstest.exec(client, { "sleep", "1" }) -- let tc reshape settle
    results[#results + 1] = { label = label, ok = probe(label) }
end

-- Baseline and final heal must succeed; impairments are expected to degrade
-- or drop traffic, so we only assert the two healthy windows.
assert(results[1].ok, "baseline must be reachable")
assert(results[#results].ok, "post-heal must be reachable")

dstest.dst.clear(client)
dstest.dst.clear(server)
dstest.info("partition example complete")
