--- @diagnostic disable:undefined-global
--- PostgreSQL lifecycle: connect, create table, insert, query, close.
--- Run: cat examples/pg.lua | cargo run

local docker_config = dstest.config({
	substrate = "docker",
	seed = 0xBEEF,
})

local s = dstest.setup(docker_config, {
	image = "postgres:16-alpine",
	ports = { 5432 },
	env = {
		POSTGRES_PASSWORD = "password",
		POSTGRES_DB = "test_db",
	},
})

-- Get the container's address. `host` is the host-mapped address (host:port)
-- that the host process can dial even when the bridge IP is not directly
-- reachable (e.g. a podman/docker machine VM). Fall back to the bridge `ip`
-- when no port mapping exists.
local info = dstest.inspect(s)
local host_addr = info.host or info.ip
dstest.info("connecting via host addr: " .. tostring(host_addr) .. " (ip: " .. tostring(info.ip) .. ")")

-- Give Postgres time to finish its boot cycle
dstest.info("waiting for database boot...")
dstest.exec(s, { "sleep", "3" })

local conn_str = string.format("postgres://postgres:password@%s/test_db", host_addr)
dstest.info("connecting: " .. conn_str)

local pool = dstest.pg.connect(conn_str, 5)
dstest.info("connected")

-- Create a table
dstest.pg.query(pool, "CREATE TABLE users (id SERIAL PRIMARY KEY, name TEXT NOT NULL)")
dstest.info("created table: users")

-- Insert rows
dstest.pg.query(pool, "INSERT INTO users (name) VALUES ('alice'), ('bob'), ('carol')")
dstest.info("inserted 3 rows")

-- Query them back
local rows = dstest.pg.query(pool, "SELECT id, name FROM users ORDER BY id")
dstest.info(string.format("query returned %d rows:", #rows))
for _, row in ipairs(rows) do
	dstest.info(string.format("  id=%d  name=%s", row.id, row.name))
end

-- Verify count
assert(#rows == 3, "expected 3 users, got " .. #rows)

dstest.pg.close(pool)
dstest.info("pg example complete")
