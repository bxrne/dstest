# net

HTTP and TCP networking against subjects. Exposes `dstest.net.http` and
`dstest.net.tcp`.

## `dstest.net.http(subject, method, path)`

Makes an HTTP request to a subject's mapped host port. Returns a table with
`status` (number) and `body` (string). Retries on connection failure using
`http_retries` / `http_retry_delay` from config.

```lua
local resp = dstest.net.http(subject, "GET", "/get")
if resp.status == 200 then
    dstest.info("request successful")
end
```

The host is resolved from the first port in `dstest.setup({ ports = { ... } })`.
Timeout is `http_timeout` seconds per attempt.

| Return field | Type | Description |
|--------------|------|-------------|
| `status` | number | HTTP status code |
| `body` | string | Response body |

Wrap in `pcall` — requests can fail during faults (e.g. network deprivation):

```lua
local ok, resp = pcall(dstest.net.http, subject, "GET", "/get")
if not ok then dstest.warn("request failed: " .. tostring(resp)) end
```

## `dstest.net.tcp(subject, port)`

Opens a TCP connection to a port on the subject. Returns `(conn, err)` — a
connection userdata on success, or `nil` and an error string on failure.

```lua
local conn, err = dstest.net.tcp(subject, 6379)
if not conn then
    dstest.warn("connect failed: " .. tostring(err))
    return
end
conn:send("PING\r\n")
local line = conn:recv_line()
conn:close()
```

The host IP is extracted from the subject's mapped host (first port in `setup`).
Timeout is `http_timeout` seconds.

### Connection methods

| Method | Description |
|--------|-------------|
| `conn:send(data)` | Send a string |
| `conn:recv(n)` | Read up to `n` bytes (returns `nil` on EOF) |
| `conn:recv_line()` | Read until `\n` (returns `nil` on EOF) |
| `conn:recv_until(delim)` | Read until a delimiter string (returns `nil` on EOF) |
| `conn:close()` | Close both directions |
| `conn:addr()` | Return the remote address string |
| `conn:set_timeout(secs)` | Set read/write timeout in seconds |
| `conn:set_nodelay(bool)` | Enable/disable TCP_NODELAY |

## `dstest.net.link(a, b, port)`

Establishes a controllable, proxied link between two subjects and returns a link
handle for impairing it. The Docker substrate implements this via `tc`/`iptables`
inside an intermediary proxy container — requires root or `CAP_NET_ADMIN` on the
host. Substrates without network virtualization return a "not supported" error.

```lua
local link = dstest.net.link(subject_a, subject_b, 8080)
link:latency(50, 10)                              -- 50ms delay + 10ms jitter
link:loss(0.05)                                   -- 5% loss
link:partition({ direction = "a->b", mode = "blackhole" })
link:heal()
```

| Method | Description |
|--------|-------------|
| `link:latency(delay_ms, jitter_ms)` | Impose constant delay plus uniform jitter |
| `link:loss(pct)` | Impose probabilistic loss (0.0–1.0) |
| `link:partition({ direction, mode })` | Partition the link; `direction` is `"a->b"`, `"b->a"`, or `"both"` (default), `mode` is `"blackhole"` (default) or `"reset"` |
| `link:heal()` | Remove all impairments |

All impairment randomness derives from the experiment seed.
