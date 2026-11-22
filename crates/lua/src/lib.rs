//! Lua 5.1 environment that matches what the WoW client gives to addon code.

use mlua::{Lua, Result, Table};

/// A Lua state prepared like the game client's addon environment.
pub struct Runtime {
    lua: Lua,
}

impl Runtime {
    /// Create a new runtime with the WoW subset of the Lua standard library.
    pub fn new() -> Result<Self> {
        let lua = Lua::new();
        restrict_stdlib(&lua)?;
        Ok(Self { lua })
    }

    /// Run a chunk of Lua code. `name` is used in error messages.
    pub fn exec(&self, code: &str, name: &str) -> Result<()> {
        self.lua.load(code).set_name(name).exec()
    }

    /// Access the underlying Lua state.
    pub fn lua(&self) -> &Lua {
        &self.lua
    }
}

/// Remove libraries that the game does not expose, and move the few `os`
/// functions the game keeps into globals.
fn restrict_stdlib(lua: &Lua) -> Result<()> {
    let globals = lua.globals();

    let os: Table = globals.get("os")?;
    for name in ["date", "time", "difftime"] {
        globals.set(name, os.get::<mlua::Function>(name)?)?;
    }

    for name in [
        "io", "os", "package", "require", "dofile", "loadfile", "module",
    ] {
        globals.set(name, mlua::Value::Nil)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_lua_51() {
        let rt = Runtime::new().unwrap();
        let version: String = rt.lua().globals().get("_VERSION").unwrap();
        assert_eq!(version, "Lua 5.1");
    }

    #[test]
    fn removes_unavailable_libraries() {
        let rt = Runtime::new().unwrap();
        rt.exec(
            "assert(io == nil and os == nil and require == nil and loadfile == nil)",
            "test",
        )
        .unwrap();
    }

    #[test]
    fn keeps_date_and_time_globals() {
        let rt = Runtime::new().unwrap();
        rt.exec(
            "assert(type(date) == 'function' and type(time) == 'function')",
            "test",
        )
        .unwrap();
    }
}
