--- @diagnostic disable:undefined-global
--- Proxied network faults between two subjects: latency, loss, partitions.
--- The client dials the link's proxy address so the impairment actually
--- affects the measured traffic.
--- Run: cat examples/link.lua | cargo run

local cfg = dstest.config({
    substrate = "docker",
    seed = 0xBEEF,
    weights = { pause = 0.5, ["deprive:memory"] = 0.3, kill = 0.2 },
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

-- Impair the link from client to server. The client must dial the proxy's
-- address (link:addr()) rather than the server's own bridge IP for the
-- impairments to be applied to its traffic.
local link = dstest.net.link(client, server, 80)
link:latency(100, 30)   -- 100ms base + 30ms jitter
link:loss(0.1)          -- 10% packet loss
local proxy_url = string.format("http://%s/get", tostring(link:addr()))
dstest.info("client dialing proxy at " .. proxy_url)

-- Wait for the proxy's socat to be listening before the first measurement.
for i = 1, 20 do
    local r = dstest.exec(client, {
        "curl", "-s", "-o", "/dev/null", "-w", "%{http_code}",
        "--connect-timeout", "2", "--max-time", "5", proxy_url,
    })
    if tonumber(r.stdout) == 200 then
        break
    end
    dstest.exec(client, { "sleep", "1" })
end

-- Measure latency under impairment (from inside the client container)
local start = dstest.clock()
local r1 = dstest.exec(client, { "curl", "-s", "-o", "/dev/null", "-w", "%{http_code} %{time_total}", "--max-time", "10", proxy_url })
local elapsed = (dstest.clock().nanos - start.nanos) / 1e6

if r1.exit_code == 0 then
    dstest.info(string.format("under impairment: %s (%.0fms)", r1.stdout, elapsed))
else
    dstest.warn("request failed: " .. r1.stderr)
end

-- Heal the link and re-measure
link:heal()
local start2 = dstest.clock()
local r2 = dstest.exec(client, { "curl", "-s", "-o", "/dev/null", "-w", "%{http_code} %{time_total}", "--max-time", "10", proxy_url })
local elapsed2 = (dstest.clock().nanos - start2.nanos) / 1e6

if r2.exit_code == 0 then
    dstest.info(string.format("after heal: %s (%.0fms)", r2.stdout, elapsed2))
end

-- Tear both containers down so the example leaves nothing behind.
dstest.dst.clear(client)
dstest.dst.clear(server)
dstest.info("link example complete")
