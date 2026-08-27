use crate::adapters::lua::pg::pool::LuaPgPool;
use crate::application::context::BindingContext;
use crate::ports::Substrate;
use mlua::{Lua, Result, Table, Value};
use sqlx::postgres::{PgPoolOptions, PgRow};
use sqlx::{Column, Row, TypeInfo};
use tracing::info;

fn pg_cell_to_lua_value(lua: &Lua, row: &PgRow, index: usize) -> Result<Value> {
    let column = row.column(index);
    let type_name = column.type_info().name();

    if row
        .try_get_raw(index)
        .map_or(true, |v| sqlx::ValueRef::is_null(&v))
    {
        return Ok(Value::Nil);
    }

    match type_name {
        "INT2" | "SMALLINT" | "SMALLSERIAL" => {
            let val: i16 = row.get(index);
            Ok(Value::Integer(val as i64))
        }
        "INT4" | "INT" | "SERIAL" => {
            let val: i32 = row.get(index);
            Ok(Value::Integer(val as i64))
        }
        "INT8" | "BIGINT" | "BIGSERIAL" => {
            let val: i64 = row.get(index);
            Ok(Value::Integer(val))
        }
        "FLOAT4" | "REAL" => {
            let val: f32 = row.get(index);
            Ok(Value::Number(val as f64))
        }
        "FLOAT8" | "DOUBLE PRECISION" => {
            let val: f64 = row.get(index);
            Ok(Value::Number(val))
        }
        "BOOL" | "BOOLEAN" => {
            let val: bool = row.get(index);
            Ok(Value::Boolean(val))
        }
        _ => {
            let val: String = row.get(index);
            Ok(Value::String(lua.create_string(&val)?))
        }
    }
}

pub fn register<S: Substrate>(lua: &Lua, dstest: &Table, _ctx: &BindingContext<S>) -> Result<()> {
    let connect_fn = lua.create_async_function(
        |_, (conn_str, max_conns): (String, Option<u32>)| async move {
            info!("Connecting to PostgreSQL database: {}", conn_str);

            let pool = PgPoolOptions::new()
                .max_connections(max_conns.unwrap_or(5))
                .connect(&conn_str)
                .await
                .map_err(|e| mlua::Error::external(format!("Database connection failed: {}", e)))?;

            Ok(LuaPgPool(pool))
        },
    )?;

    let query_fn = lua.create_async_function(
        |lua, (wrapped_pool, query): (LuaPgPool, String)| async move {
            info!("Executing SQL query: {}", query);

            let rows = sqlx::query(&query)
                .fetch_all(&wrapped_pool.0)
                .await
                .map_err(|e| mlua::Error::external(format!("Query execution failed: {}", e)))?;

            let lua_rows = lua.create_table()?;

            for (row_idx, row) in rows.iter().enumerate() {
                let lua_row = lua.create_table()?;

                for column in row.columns() {
                    let col_name = column.name();
                    let col_idx = column.ordinal();
                    let col_value = pg_cell_to_lua_value(&lua, row, col_idx)?;

                    lua_row.set(col_name, col_value)?;
                }

                lua_rows.raw_insert((row_idx + 1) as i64, lua_row)?;
            }

            Ok(lua_rows)
        },
    )?;

    let close_fn = lua.create_async_function(|_, (wrapped_pool,): (LuaPgPool,)| async move {
        info!("Closing SQL connection pool");
        wrapped_pool.0.close().await;
        Ok(())
    })?;

    dstest.set("connect", connect_fn)?;
    dstest.set("query", query_fn)?;
    dstest.set("close", close_fn)?;

    Ok(())
}
