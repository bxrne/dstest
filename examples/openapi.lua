--- @diagnostic disable:undefined-global
--- OpenAPI-driven workload: load a spec file and drive sustained HTTP traffic
--- against a subject from the endpoints it declares.
--- Run: cat examples/openapi.lua | cargo run
---
--- The spec lives at examples/openapi.json. We also walk it manually with a
--- Lua iterator to show how a script can query an OpenAPI document for its own
--- verification, and we ship the same file into a worker container via
--- `volumes` + `depends`.

local cfg = dstest.config({
    substrate = "docker",
    seed = 0x0A0A,
    http_retries = 6,
    http_retry_delay = 300,
})

local server = dstest.setup(cfg, {
    image = "kennethreitz/httpbin",
    ports = { 80 },
})

-- A worker that carries the spec file via a bind mount and waits on the
-- server using `depends` — a realistic deployment where the driver and the
-- worker are separate containers.
local client = dstest.setup(cfg, {
    image = "curlimages/curl:latest",
    cmd = { "sleep", "300" },
    volumes = { "examples/openapi.json:/specs/openapi.json:ro" },
    depends = { server },
})

-- Iterate every (method, path) pair declared by an OpenAPI spec file on the
-- host (the Lua harness runs in the same process as the workload generator).
-- Returns a function for use in a generic `for` loop, the classic Lua
-- iterator idiom.
local function openapi_pairs(path)
    local fh = assert(io.open(path, "r"))
    local json = assert(fh:read("*a"), "cannot read " .. path)
    fh:close()

    -- Lua 5.3+ has a built-in JSON? No. Parse with a tiny recursive descent
    -- parser so this example depends on no external pure-Lua modules.
    local pos = 1
    local function skip()
        while json:sub(pos, pos):match("%s") do pos = pos + 1 end
    end
    local function parse_value()
        skip()
        local c = json:sub(pos, pos)
        if c == "{" then
            pos = pos + 1
            local t = {}
            skip()
            if json:sub(pos, pos) == "}" then pos = pos + 1 return t end
            while true do
                skip(); assert(json:sub(pos, pos) == '"', "expected key")
                local k = assert(parse_value(), "expected string key")
                skip(); assert(json:sub(pos, pos) == ":", "expected colon"); pos = pos + 1
                t[k] = parse_value()
                skip()
                if json:sub(pos, pos) == "," then pos = pos + 1
                else assert(json:sub(pos, pos) == "}", "expected }"); pos = pos + 1; break end
            end
            return t
        elseif c == "\"" then
            pos = pos + 1
            local out = {}
            while true do
                local ch = json:sub(pos, pos)
                if ch == "\"" then pos = pos + 1 break end
                if ch == "\\" then
                    local esc = json:sub(pos + 1, pos + 1)
                    if esc == "n" then out[#out + 1] = "\n"
                    else out[#out + 1] = esc end
                    pos = pos + 2
                else
                    out[#out + 1] = ch
                    pos = pos + 1
                end
            end
            return table.concat(out)
        else
            -- numbers / booleans / null: read a bare token
            local tok = json:match("^[%w%.%-]+", pos)
            assert(tok, "bad value at " .. pos)
            pos = pos + #tok
            if tok == "true" then return true
            elseif tok == "false" then return false
            elseif tok == "null" then return nil
            else return tonumber(tok) end
        end
    end

    local root = assert(parse_value(), "empty spec")
    local paths = root.paths or {}
    local keys = {}
    for p in pairs(paths) do keys[#keys + 1] = p end
    table.sort(keys)

    local i = 0
    return function()
        while i < #keys do
            i = i + 1
            local p = keys[i]
            local methods = paths[p]
            for m in pairs(methods) do
                local up = string.upper(m)
                if up == "GET" or up == "POST" or up == "PUT"
                   or up == "DELETE" or up == "PATCH" then
                    return up, p
                end
            end
        end
        return nil
    end
end

-- Read the spec from the host, exactly as dstest.workload.http will, and log
-- the discovered surface.
local listed = {}
for method, path in openapi_pairs("examples/openapi.json") do
    listed[#listed + 1] = method .. " " .. path
end
dstest.info("spec endpoints: " .. table.concat(listed, ", "))

-- Drive sustained traffic from the endpoints declared by the spec. The
-- generator reads examples/openapi.json from the host filesystem.
local report = dstest.workload.http(server, {
    duration_secs = 8,
    rate = 12,
    openapi = "examples/openapi.json",
})

dstest.info(string.format(
    "workload: %d requests, %d ok, %d failed (avg %dms, max %dms)",
    report.total_requests, report.ok, report.failed,
    report.avg_latency_ms, report.max_latency_ms
))
assert(report.failed == 0, "every spec-declared endpoint must be reachable")

-- Show the per-method / per-path breakdown the generator reports.
for key, stats in pairs(report.breakdown) do
    dstest.info(string.format("  %-14s ok=%d failed=%d", key, stats.ok, stats.failed))
end

dstest.dst.clear(client)
dstest.dst.clear(server)
dstest.info("openapi example complete")
