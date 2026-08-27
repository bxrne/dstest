# bindings

Lua-facing functions exposed to user scripts via the global `dstest` table.

## Architecture

Every namespace is a module implementing the `LuaModule<S>` trait:

```rust
pub trait LuaModule<S: Substrate> {
    fn register(lua: &Lua, dstest: &Table, ctx: &BindingContext<S>) -> mlua::Result<()>;
}
```

`register_all` (in `mod.rs`) wires every module into the `dstest` table at engine
startup. Each module either registers functions directly on `dstest` (flat) or
creates a sub-table (namespaced).

`BindingContext<S>` (defined in [`src/application/context.rs`](../application/context.rs)) is the
shared handle every binding clones `Arc`s from: application state (the event log plus the
subject/config/fault-tree registries), the check runner, the substrate, and the workload RNG.

## Namespaces

| Namespace | Folder | Lua functions | Style |
|-----------|--------|---------------|-------|
| `dstest.config`, `dstest.setup` | [`core/`](core/README.md) | `config`, `setup` | flat |
| `dstest.dst.step`, `dstest.dst.run_steps`, `dstest.dst.clear`, `dstest.dst.oracle.*` | [`dst/`](dst/README.md) | `step`, `run_steps`, `clear`, `oracle` | namespaced |
| `dstest.net.http`, `dstest.net.tcp` | [`net/`](net/README.md) | `http`, `tcp` | namespaced |
| `dstest.inspect`, `dstest.logs`, `dstest.exec` | [`subs/`](subs/README.md) | `inspect`, `logs`, `exec` | flat |
| `dstest.pg.connect`, `dstest.pg.query`, `dstest.pg.close` | [`pg/`](pg/README.md) | `pg.connect`, `pg.query`, `pg.close` | namespaced |
| `dstest.clock` | [`clock/`](clock/README.md) | `clock` | flat |
| `dstest.storage.*` | [`storage/`](storage/README.md) | `error`, `drop_writes`, `corrupt`, `snapshot`, `restore` | namespaced |
| `dstest.random.*` | [`random/`](random/README.md) | `int`, `float`, `bool`, `choice`, `shuffle` | namespaced |
| `dstest.debug/info/warn/error` | [`log/`](log/README.md) | `debug`, `info`, `warn`, `error` | flat |

## Adding a new binding module

1. Create `src/adapters/lua/<name>/` with a `mod.rs` defining a unit struct (e.g. `pub struct Foo;`).
2. `impl<S: Substrate> LuaModule<S> for Foo` — create a sub-table, register leaf functions, set it on `dstest`.
3. Add `mod <name>;` to [`mod.rs`](mod.rs) and call `foo::Foo::register(lua, dstest, ctx)?;` in `register_all`.
4. If the module has leaf functions, put each in its own file under the folder with a `pub fn register<S: Substrate>(...)`.
5. Update the table above and the relevant per-module README.

## Conventions

- Leaf files each expose a `pub fn register<S: Substrate>(lua, table, ctx)` — the module `mod.rs` calls them and sets the sub-table on `dstest`.
- Bindings clone `Arc` handles out of `ctx` (e.g. `Arc::clone(&ctx.substrate)`) so closures are `'static + Send`.
- Errors return `mlua::Error::RuntimeError(String)`. Prefer `String`/`anyhow` in substrate code and convert at the binding boundary.
- Update `DOCS.md` and the per-module README when changing the Lua API surface.
