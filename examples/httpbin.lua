--- @diagnostic disable:undefined-global
--- httpbin smoke test: exercise a real route, cut the link, verify the failure,
--- then heal the link and verify recovery.
--- Run: cat examples/httpbin.lua | cargo run

local cfg = dstest.config({
	substrate = "docker",
	seed = 0xCAFE,
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

-- Establish the proxied link from the client to the server. The client must
-- dial the link's proxy address, not the server's own bridge IP, for the
-- impairment to affect its traffic.
local link = dstest.net.link(client, server, 80)
local proxy_url = string.format("http://%s/get", tostring(link:addr()))

-- A single probe of httpbin's real /get route from inside the client, through
-- the proxy. Returns (ok, code, stderr).
local function probe_once()
	local r = dstest.exec(client, {
		"curl", "-s", "-o", "/dev/null", "-w", "%{http_code}",
		"--connect-timeout", "2", "--max-time", "5", proxy_url,
	})
	local code = tonumber(r.stdout)
	return r.exit_code == 0 and code == 200, code, r.stderr
end

-- Probe repeatedly until the route responds 200. Used when the link is healthy,
-- where a failed probe may just mean the proxy is still installing socat.
local function probe_healthy()
	for i = 1, 20 do
		local ok, code = probe_once()
		if ok then
			return true, code
		end
		if i < 20 then
			dstest.exec(client, { "sleep", "1" })
		end
	end
	return false, nil
end

-- Phase 1: healthy link, route must respond 200.
local ok, code = probe_healthy()
assert(ok, string.format("phase 1: expected 200, never got a healthy response (last code=%s)", tostring(code)))
dstest.info(string.format("phase 1: /get responded %s over healthy link", tostring(code)))

-- Phase 2: cut off the client->server link with a blackhole partition.
link:partition({ direction = "a->b", mode = "blackhole" })
dstest.info("partitioned link client->server (blackhole)")

-- A blackholed proxy drops the connection, so the probe must fail. Try a few
-- single probes; a 200 at any point means the partition did not hold.
local saw_200 = false
for i = 1, 10 do
	local ok2, code2 = probe_once()
	if ok2 then
		saw_200 = true
		code = code2
		break
	end
	dstest.info(string.format("phase 2 attempt %d: request blocked (no 200)", i))
	dstest.exec(client, { "sleep", "1" })
end
assert(not saw_200, string.format("phase 2: expected failure under partition, but got code=%s", tostring(code)))
dstest.info("phase 2: request blocked as expected under partition")

-- Phase 3: heal the link, route must respond 200 again.
link:heal()
dstest.info("healed link")

local ok3, code3 = probe_healthy()
assert(ok3, string.format("phase 3: expected 200 after heal, got code=%s", tostring(code3)))
dstest.info(string.format("phase 3: /get responded %s after heal", tostring(code3)))

-- Tear both containers down so the example leaves nothing behind.
dstest.dst.clear(client)
dstest.dst.clear(server)
dstest.info("httpbin example complete")
