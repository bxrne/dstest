# dstest.workload

Sustained workload generation for HTTP and PostgreSQL subjects.

## `dstest.workload.http(subject_id, opts)`

Generates sustained HTTP traffic against a subject. Supports manual request
lists or loading endpoints from an OpenAPI spec.

```lua
-- Manual request list
local stats = dstest.workload.http(s, {
    duration_secs = 10,
    rate = 20,
    requests = {
        { method = "GET",  path = "/get" },
        { method = "POST", path = "/post", body = '{"key":"val"}', content_type = "application/json" },
        { method = "GET",  path = "/status/200" },
    },
})

-- From OpenAPI spec (JSON or YAML)
local stats = dstest.workload.http(s, {
    duration_secs = 10,
    rate = 20,
    openapi = "/path/to/openapi.yaml",
})
```

### Options

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `duration_secs` | number | `10` | How long to run the workload |
| `rate` | number | `10` | Requests per second |
| `requests` | table | `[{ method = "GET", path = "/get" }]` | Array of request specs |
| `openapi` | string | — | Path to an OpenAPI 3.x JSON or YAML spec |

### Request spec

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `method` | string | No | HTTP method (default: `GET`) |
| `path` | string | Yes | Request path |
| `body` | string | No | Request body |
| `content_type` | string | No | Content-Type header |

### Returns

| Field | Type | Description |
|-------|------|-------------|
| `total_requests` | number | Total requests sent |
| `ok` | number | Successful (2xx) responses |
| `failed` | number | Failed responses |
| `avg_latency_ms` | number | Average response latency |
| `max_latency_ms` | number | Maximum response latency |
| `breakdown` | table | Per-endpoint `{ok, failed}` counts |

## `dstest.workload.pg(pool, opts)`

Generates sustained PostgreSQL traffic against a connection pool.

```lua
local pool = dstest.pg.connect("postgres://user:pass@host:5432/db", 5)
local stats = dstest.workload.pg(pool, {
    duration_secs = 10,
    rate = 20,
    queries = {
        "SELECT 1",
        "SELECT id, name FROM users ORDER BY id",
    },
})
```

### Options

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `duration_secs` | number | `10` | How long to run the workload |
| `rate` | number | `10` | Queries per second |
| `queries` | table | `["SELECT 1"]` | Array of SQL queries to cycle through |

### Returns

| Field | Type | Description |
|-------|------|-------------|
| `total_queries` | number | Total queries executed |
| `ok` | number | Successful queries |
| `failed` | number | Failed queries |
| `avg_latency_ms` | number | Average query latency |
| `max_latency_ms` | number | Maximum query latency |
