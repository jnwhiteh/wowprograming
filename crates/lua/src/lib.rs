//! Lua 5.1 environment that matches what the WoW client gives to addon code.

pub mod stdlib;

pub use mlua;

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use mlua::{Function, HookTriggers, Lua, MultiValue, Result, Table, Value, VmState};

/// Error message raised when a script exceeds its time budget. Matches the
/// message the game client uses.
pub const SCRIPT_TIMEOUT_MESSAGE: &str = "script ran too long";

/// Default time budget for one entry into Lua (loading a file, one event
/// handler, one `OnUpdate`).
pub const DEFAULT_SCRIPT_TIMEOUT: Duration = Duration::from_secs(5);

/// Default limit on memory used by the Lua state.
pub const DEFAULT_MEMORY_LIMIT: usize = 1024 * 1024 * 1024;

/// How often, in VM instructions, the watchdog checks the deadline.
const WATCHDOG_INTERVAL: u32 = 1000;

/// A Lua state prepared like the game client's addon environment.
pub struct Runtime {
    lua: Lua,
    watchdog: Rc<Watchdog>,
}

/// Time budget for calls into Lua. Shared between the runtime and code that
/// calls Lua from inside Rust callbacks.
pub struct Watchdog {
    timeout: Cell<Duration>,
    deadline: Cell<Option<Instant>>,
}

impl Watchdog {
    /// True when the current call has run past its deadline.
    pub fn tripped(&self) -> bool {
        self.deadline.get().is_some_and(|d| Instant::now() >= d)
    }

    /// Run `f` with a deadline. Nested calls share the outer deadline.
    pub fn guarded<R>(&self, f: impl FnOnce() -> R) -> R {
        let outer = self.deadline.get();
        if outer.is_none() {
            self.deadline.set(Some(Instant::now() + self.timeout.get()));
        }
        let result = f();
        self.deadline.set(outer);
        result
    }
}

/// Wrap `pcall`, `xpcall` and `coroutine.resume` so that a timeout error is
/// raised again instead of being caught. Without this, a loop around
/// `pcall` could run forever.
fn make_timeout_uncatchable(lua: &Lua, watchdog: &Rc<Watchdog>) -> Result<()> {
    let globals = lua.globals();
    let coroutine: Table = globals.get("coroutine")?;
    let targets = [
        (&globals, "pcall"),
        (&globals, "xpcall"),
        (&coroutine, "resume"),
    ];
    for (table, name) in targets {
        let original: Function = table.get(name)?;
        let watchdog = Rc::clone(watchdog);
        let wrapper = lua.create_function(move |lua, args: MultiValue| {
            let mut results: MultiValue = original.call(args)?;
            let failed = matches!(results.front(), Some(Value::Boolean(false)));
            if failed && watchdog.tripped() {
                return Err(mlua::Error::runtime(SCRIPT_TIMEOUT_MESSAGE));
            }
            if failed && let Some(Value::Error(err)) = results.get(1) {
                let message = Value::String(lua.create_string(error_message(err))?);
                results[1] = message;
            }
            Ok(results)
        })?;
        table.set(name, wrapper)?;
    }
    Ok(())
}

/// The message for an error raised by a Rust function, in the form a C Lua
/// function would give it (for example `bad argument #2 to '?' (number
/// expected, got nil)`). Inside Lua such errors are `Value::Error` objects,
/// which Lua code cannot use as strings.
pub fn error_message(err: &mlua::Error) -> String {
    use mlua::Error;
    match err {
        Error::RuntimeError(msg) => msg.clone(),
        Error::CallbackError { cause, .. } => error_message(cause),
        Error::WithContext { cause, .. } => error_message(cause),
        Error::BadArgument { to, pos, cause, .. } => format!(
            "bad argument #{pos} to '{}' ({})",
            to.as_deref().unwrap_or("?"),
            error_message(cause)
        ),
        Error::FromLuaConversionError { from, to, .. } => {
            format!("{} expected, got {from}", lua_type_name(to))
        }
        other => other.to_string(),
    }
}

/// Map a Rust type name from a conversion error to the Lua type name.
fn lua_type_name(rust: &str) -> &str {
    match rust {
        "f32" | "f64" | "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32" | "u64"
        | "usize" | "Integer" | "Number" => "number",
        "String" | "LuaString" | "&str" | "BString" => "string",
        "Table" => "table",
        "Function" => "function",
        "bool" => "boolean",
        other => other,
    }
}

impl Runtime {
    /// Create a new runtime with the WoW subset of the Lua standard library,
    /// a script watchdog and a memory limit.
    pub fn new() -> Result<Self> {
        let lua = Lua::new();
        restrict_stdlib(&lua)?;
        stdlib::install(&lua)?;
        // WoW's xpcall passes extra arguments to the function, as Lua 5.2
        // does (`xpcall(f, handler, ...)`); Lua 5.1 drops them.
        let xpcall: Function = lua
            .load(
                r##"
                local xpcall, select, unpack = xpcall, select, unpack
                return function(f, handler, ...)
                    local n = select("#", ...)
                    if n == 0 then return xpcall(f, handler) end
                    local args = { ... }
                    return xpcall(function() return f(unpack(args, 1, n)) end, handler)
                end"##,
            )
            .set_name("=[C]")
            .call(())?;
        lua.globals().set("xpcall", xpcall)?;
        lua.set_memory_limit(DEFAULT_MEMORY_LIMIT)?;

        let watchdog = Rc::new(Watchdog {
            timeout: Cell::new(DEFAULT_SCRIPT_TIMEOUT),
            deadline: Cell::new(None),
        });
        // A global hook is inherited by coroutines; a thread hook is not.
        let hook_watchdog = Rc::clone(&watchdog);
        lua.set_global_hook(
            HookTriggers::new().every_nth_instruction(WATCHDOG_INTERVAL),
            move |_, _| {
                if hook_watchdog.tripped() {
                    Err(mlua::Error::runtime(SCRIPT_TIMEOUT_MESSAGE))
                } else {
                    Ok(VmState::Continue)
                }
            },
        )?;
        make_timeout_uncatchable(&lua, &watchdog)?;
        Ok(Self { lua, watchdog })
    }

    /// Set the time budget for one entry into Lua.
    pub fn set_script_timeout(&self, timeout: Duration) {
        self.watchdog.timeout.set(timeout);
    }

    /// Set the memory limit in bytes. Returns the previous limit.
    pub fn set_memory_limit(&self, bytes: usize) -> Result<usize> {
        self.lua.set_memory_limit(bytes)
    }

    /// Run `f` under the watchdog. Every call from the engine into Lua must
    /// go through this. Nested calls share the outer deadline.
    pub fn guarded<R>(&self, f: impl FnOnce(&Lua) -> Result<R>) -> Result<R> {
        self.watchdog.guarded(|| f(&self.lua))
    }

    /// A shared handle to the watchdog, for calls into Lua made from inside
    /// Rust callbacks.
    pub fn watchdog(&self) -> Rc<Watchdog> {
        Rc::clone(&self.watchdog)
    }

    /// Run a chunk of Lua code under the watchdog. `name` is used in error
    /// messages.
    pub fn exec(&self, code: &str, name: &str) -> Result<()> {
        self.guarded(|lua| lua.load(code).set_name(name).exec())
    }

    /// Access the underlying Lua state. Calls into Lua made through this
    /// reference are not guarded unless wrapped in [`Runtime::guarded`].
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
    fn xpcall_passes_extra_arguments() {
        let rt = Runtime::new().unwrap();
        rt.exec(
            "local ok, a, b = xpcall(function(x, y) return y, x end, print, 1, nil)
             assert(ok and a == nil and b == 1)
             local ok2, err = xpcall(function(t) error(t.msg, 0) end, function(e) return 'h:' .. e end, { msg = 'm' })
             assert(not ok2 and err == 'h:m')",
            "test",
        )
        .unwrap();
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

    fn assert_times_out(code: &str) {
        let rt = Runtime::new().unwrap();
        rt.set_script_timeout(Duration::from_millis(100));
        let start = Instant::now();
        let err = rt.exec(code, "=test").unwrap_err();
        assert!(err.to_string().contains(SCRIPT_TIMEOUT_MESSAGE), "{err}");
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn watchdog_stops_infinite_loop() {
        assert_times_out("while true do end");
    }

    #[test]
    fn watchdog_stops_loop_that_catches_errors() {
        assert_times_out("while true do pcall(function() for i = 1, 100 do end end) end");
    }

    #[test]
    fn watchdog_stops_loop_inside_coroutine() {
        assert_times_out("coroutine.wrap(function() while true do end end)()");
    }

    #[test]
    fn watchdog_resets_between_entries() {
        let rt = Runtime::new().unwrap();
        rt.set_script_timeout(Duration::from_millis(100));
        assert!(rt.exec("while true do end", "=test").is_err());
        rt.exec("local x = 0 for i = 1, 1000 do x = x + i end", "=test")
            .unwrap();
    }

    #[test]
    fn memory_limit_stops_runaway_allocation() {
        let rt = Runtime::new().unwrap();
        rt.set_memory_limit(64 * 1024 * 1024).unwrap();
        let err = rt
            .exec(
                "local t = {} while true do t[#t + 1] = string.rep('x', 1e6) .. #t end",
                "=test",
            )
            .unwrap_err();
        assert!(matches!(err, mlua::Error::MemoryError(_)), "{err}");
    }
}
