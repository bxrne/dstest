# log

Logging functions that map to `tracing` levels. Exposes
`dstest.debug`, `dstest.info`, `dstest.warn`, and `dstest.error`
(flat on the `dstest` table).

## `dstest.debug(msg)`

Logs a debug-level message.

```lua
dstest.debug("entering fault round " .. round)
```

## `dstest.info(msg)`

Logs an info-level message.

```lua
dstest.info("subject started: " .. subject)
```

## `dstest.warn(msg)`

Logs a warning-level message.

```lua
dstest.warn("health check degraded")
```

## `dstest.error(msg)`

Logs an error-level message.

```lua
dstest.error("connection refused: " .. tostring(err))
```

All messages are forwarded to the `tracing` subscriber configured by
the harness and appear in the experiment log output.