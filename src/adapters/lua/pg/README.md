# pg

PostgreSQL access via `sqlx`. Exposes `dstest.pg.connect`, `dstest.pg.query`,
and `dstest.pg.close`. All functions are async (run on the tokio reactor).

## `dstest.pg.connect(conn_str, max_conns?)`

Connects to a PostgreSQL database and returns a `LuaPgPool` userdata.

```lua
local pool = dstest.pg.connect(
    "postgres://postgres:password@192.168.1.10:5432/test_db",
    5  -- max connections (default 5)
)
```

Use `dstest.inspect(subject)` to get the container's bridge IP, then build the
connection string. The pool is a userdata — pass it to `query` and `close`.

## `dstest.pg.query(pool, sql)`

Executes a SQL query and returns rows as a 1-indexed array of tables.

```lua
local rows = dstest.pg.query(pool, "SELECT 1 as active")
dstest.info("row count: " .. #rows)
dstest.info("value: " .. rows[1].active)
```

### SQL type → Lua value mapping

| PostgreSQL type | Lua type |
|-----------------|----------|
| `INT2`, `SMALLINT`, `SMALLSERIAL` | integer |
| `INT4`, `INT`, `SERIAL` | integer |
| `INT8`, `BIGINT`, `BIGSERIAL` | integer |
| `FLOAT4`, `REAL` | number |
| `FLOAT8`, `DOUBLE PRECISION` | number |
| `BOOL`, `BOOLEAN` | boolean |
| `NULL` (any type) | `nil` |
| everything else (`TEXT`, `VARCHAR`, etc.) | string |

## `dstest.pg.close(pool)`

Closes the connection pool.

```lua
dstest.pg.close(pool)
```

## `LuaPgPool`

A userdata wrapping `sqlx::PgPool`. Has no callable methods — it's an opaque
handle passed between `connect`, `query`, and `close`. Cloning is cheap
(reference-counted).
