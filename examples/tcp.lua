--- @diagnostic disable:undefined-global
--- Raw TCP against a subject with dstest.net.tcp: open a socket, send an
--- HTTP/1.1 request byte-for-byte, read the status line and headers back,
--- then close.
--- Run: cat examples/tcp.lua | cargo run
---
--- Demonstrates a Lua coroutine driving a compact request/response "protocol"
--- and the seeded `dstest.random` stream to pick each request deterministically.

local cfg = dstest.config({
    substrate = "docker",
    seed = 0x7C97,
    http_timeout = 10,
})

local server = dstest.setup(cfg, {
    image = "kennethreitz/httpbin",
    ports = { 80 },
})

dstest.info("waiting for httpbin to accept connections...")
dstest.exec(server, { "sleep", "5" })

-- A coroutine generator that counts out a fixed number of protocol rounds.
-- The actual request path is chosen inside the loop from the seeded random
-- stream, so the example keeps coroutines (control flow) and randomness
-- (data) as two separable concerns.
local function protocol_rounds(n)
    return coroutine.wrap(function()
        for r = 1, n do
            coroutine.yield(r)
        end
    end)
end

-- A seeded random choice of HTTP GET paths. Deterministic for a given seed.
local function pick_path()
    return dstest.random.choice({ "/get", "/status/200", "/headers" })
end

-- Execute one raw HTTP request/response round trip over a fresh connection:
-- send the request, read the HTTP status line, then read the header block up
-- to the blank line, and close. A fresh connection per request keeps the
-- response framing trivial (nothing to drain across rounds).
local function raw_round_trip(path)
    local conn, err = dstest.net.tcp(server, 80)
    assert(conn, "raw TCP connect failed: " .. tostring(err))
    conn:set_timeout(10)

    conn:send(string.format(
        "GET %s HTTP/1.1\r\nHost: %s\r\nConnection: close\r\n\r\n",
        path, tostring(conn:addr())
    ))

    -- Read the status line, e.g. "HTTP/1.1 200 OK".
    local status_line = conn:recv_line()
    assert(status_line, "connection closed before a status line")

    -- Read the remaining headers up to the blank line terminating the block.
    conn:recv_until("\r\n\r\n")
    conn:close()

    return status_line:gsub("\r\n", "")
end

local count = 0
for _ in protocol_rounds(3) do
    local path = pick_path() -- seeded and reproducible per run
    local status_line = raw_round_trip(path)
    dstest.info(string.format("  %-12s -> %s", path, status_line))
    assert(status_line:match("^HTTP/1%.[01] 200"), path .. " did not return 200")
    count = count + 1
end

dstest.info(string.format("raw tcp exchanged %d requests", count))
assert(count == 3, "expected all three requests to complete")

dstest.dst.clear(server)
dstest.info("tcp example complete")
