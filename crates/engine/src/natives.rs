//! Native globals that the client provides: error and print handlers,
//! secure call helpers, time functions.

use mlua::{Function, IntoLuaMulti, Lua, MultiValue, Result, Table, Value, Variadic};
use wowprogramming_lua::{error_message, mlua, stdlib};

use crate::state::{Output, state};

const DISPATCH_KEY: &str = "wowprogramming.dispatch";
const ERROR_HANDLER_KEY: &str = "wowprogramming.call_error_handler";

/// Lua part of the natives. It runs once at startup with the default
/// handlers as arguments. The chunk is named `[C]` so its frames look like
/// native frames in stack traces.
const PRELUDE: &str = include_str!("lua/prelude.lua");
const CURVE: &str = include_str!("lua/curve.lua");

pub fn install(lua: &Lua) -> Result<()> {
    let default_error_handler = lua.create_function(|lua, msg: Value| {
        let text = lua
            .coerce_string(msg.clone())?
            .map(|s| s.to_string_lossy())
            .unwrap_or_else(|| format!("({})", msg.type_name()));
        let stack = stdlib::debugstack(lua, 2, 12, 10);
        state(lua)
            .output
            .push(Output::Error(format!("{text}\n{}", stack.trim_end())));
        Ok(())
    })?;
    let default_print = lua.create_function(|lua, args: Variadic<Value>| {
        let tostring: Function = lua.globals().get("tostring")?;
        let mut parts = Vec::with_capacity(args.len());
        for arg in args.iter() {
            parts.push(tostring.call::<String>(arg.clone())?);
        }
        state(lua).output.push(Output::Print(parts.join(" ")));
        Ok(())
    })?;

    // Number of stack levels below the caller, counting the caller.
    let get_callstack_height = lua.create_function(|lua, ()| {
        let mut level = 1;
        while lua.inspect_stack(level, |_| ()).is_some() {
            level += 1;
        }
        Ok(level - 1)
    })?;
    lua.globals()
        .set("GetCallstackHeight", get_callstack_height.clone())?;

    let (dispatch, call_error_handler): (Function, Function) =
        lua.load(PRELUDE).set_name("=[C]").call((
            default_error_handler,
            default_print,
            get_callstack_height,
            lua.create_function(normalize_error)?,
        ))?;
    lua.set_named_registry_value(DISPATCH_KEY, dispatch)?;
    lua.set_named_registry_value(ERROR_HANDLER_KEY, call_error_handler)?;

    let g = lua.globals();
    g.set(
        "GetTime",
        lua.create_function(|lua, ()| Ok(state(lua).time))?,
    )?;
    g.set(
        "GetTickTime",
        lua.create_function(|lua, ()| Ok(state(lua).last_elapsed))?,
    )?;
    g.set("GetFramerate", lua.create_function(|_, ()| Ok(60.0))?)?;
    g.set(
        "debugprofilestart",
        lua.create_function(|lua, ()| {
            state(lua).profile_start = std::time::Instant::now();
            Ok(())
        })?,
    )?;
    g.set(
        "debugprofilestop",
        lua.create_function(|lua, ()| {
            Ok(state(lua).profile_start.elapsed().as_secs_f64() * 1000.0)
        })?,
    )?;

    // Text drawn on the game screen for debugging. Not shown here; the same
    // messages also go to C_Log.
    g.set("addframetext", lua.create_function(|_, _: Value| Ok(()))?)?;

    let c_log = lua.create_table()?;
    c_log.set(
        "LogErrorMessage",
        lua.create_function(|lua, msg: String| {
            state(lua)
                .output
                .push(Output::Error(msg.trim_end().to_string()));
            Ok(())
        })?,
    )?;
    c_log.set(
        "LogWarningMessage",
        lua.create_function(|lua, msg: String| {
            state(lua).output.push(Output::Warning(msg));
            Ok(())
        })?,
    )?;
    c_log.set(
        "LogMessage",
        lua.create_function(|lua, msg: String| {
            state(lua).output.push(Output::Log(msg));
            Ok(())
        })?,
    )?;
    c_log.set(
        "LogMessageWithPriority",
        lua.create_function(|lua, (_priority, msg): (Value, String)| {
            state(lua).output.push(Output::Log(msg));
            Ok(())
        })?,
    )?;
    g.set("C_Log", c_log)?;

    // Marks a file to be skipped in error locations. Not needed here.
    g.set(
        "AddSourceLocationExclude",
        lua.create_function(|_, _: Value| Ok(()))?,
    )?;

    // Colors come from the game's GlobalColor table, which needs DB2
    // reading. Until then there are none.
    let c_ui_color = lua.create_table()?;
    c_ui_color.set(
        "GetColors",
        lua.create_function(|lua, ()| lua.create_table())?,
    )?;
    g.set("C_UIColor", c_ui_color)?;

    let c_curve_util: Table = lua.load(CURVE).set_name("=[C]").call(())?;
    g.set("C_CurveUtil", c_curve_util)?;
    Ok(())
}

/// Turn an error raised by a Rust function into a string with the location
/// of the Lua code that called it, as `luaL_error` does for C functions.
/// Other error values are returned unchanged.
fn normalize_error(lua: &Lua, msg: Value) -> Result<Value> {
    let Value::Error(err) = msg else {
        return Ok(msg);
    };
    let text = error_message(&err);
    // Level 0 is this function and level 1 is the prelude's error handler.
    let mut level = 2;
    let mut location = None;
    while let Some(frame) = lua.inspect_stack(level, |d| {
        let source = d.source();
        let src = source.source.as_deref().unwrap_or("").to_string();
        (source.what, src, d.current_line())
    }) {
        let (what, src, line) = frame;
        if what != "C" && src != "=[C]" {
            location = line.map(|l| format!("{}:{l}: ", stdlib::chunk_display(&src)));
            break;
        }
        level += 1;
    }
    let full = format!("{}{text}", location.unwrap_or_default());
    Ok(Value::String(lua.create_string(full)?))
}

/// Call `f` so that errors go to the Lua error handler, like the client
/// does for scripts and callbacks. Returns the results, or `None` if the
/// call failed. Each top-level call gets its own watchdog budget.
pub fn safe_call(lua: &Lua, f: &Function, args: impl IntoLuaMulti) -> Option<MultiValue> {
    let dispatch: Function = lua.named_registry_value(DISPATCH_KEY).ok()?;
    let watchdog = state(lua).watchdog.clone();
    let result = args.into_lua_multi(lua).and_then(|mut args| {
        args.push_front(Value::Function(f.clone()));
        watchdog.guarded(|| dispatch.call::<MultiValue>(args))
    });
    match result {
        Ok(mut values) => match values.pop_front() {
            Some(Value::Boolean(true)) => Some(values),
            _ => None,
        },
        Err(err) => {
            // Raised past the error handler: a timeout, or an error inside
            // the error handler itself.
            state(lua).output.push(Output::Error(err.to_string()));
            None
        }
    }
}

/// Send an error message to the Lua error handler.
pub fn report_error(lua: &Lua, message: &str) {
    match lua.named_registry_value::<Function>(ERROR_HANDLER_KEY) {
        Ok(handler) => {
            safe_call(lua, &handler, message);
        }
        Err(_) => state(lua).output.push(Output::Error(message.to_string())),
    }
}

/// Compile and run a chunk of Lua code with arguments. Syntax and runtime
/// errors go to the error handler. Returns false on error.
pub fn run_chunk(lua: &Lua, code: &[u8], chunk_name: &str, args: impl IntoLuaMulti) -> bool {
    let code = code.strip_prefix(b"\xef\xbb\xbf").unwrap_or(code);
    match lua.load(code).set_name(chunk_name).into_function() {
        Ok(f) => safe_call(lua, &f, args).is_some(),
        Err(err) => {
            report_error(lua, &syntax_error_message(&err));
            false
        }
    }
}

fn syntax_error_message(err: &mlua::Error) -> String {
    match err {
        mlua::Error::SyntaxError { message, .. } => message.clone(),
        other => other.to_string(),
    }
}
