use mlua::{FromLua, Lua, Result, UserData, Value};
use sqlx::PgPool;

#[derive(Clone)]
pub struct LuaPgPool(pub PgPool);

impl UserData for LuaPgPool {
    fn add_methods<M: mlua::UserDataMethods<Self>>(_methods: &mut M) {}
}

impl FromLua for LuaPgPool {
    fn from_lua(value: Value, _lua: &Lua) -> Result<Self> {
        let userdata = match value {
            Value::UserData(ud) => ud,
            _ => {
                return Err(mlua::Error::FromLuaConversionError {
                    from: value.type_name(),
                    to: "LuaPgPool".to_string(),
                    message: Some("Expected UserData".to_string()),
                });
            }
        };
        let pool = userdata.borrow::<Self>()?.clone();
        Ok(pool)
    }
}
