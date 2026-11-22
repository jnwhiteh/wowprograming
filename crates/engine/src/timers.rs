//! `C_Timer`: callbacks that run on the engine clock.

use std::cell::Cell;
use std::rc::Rc;

use mlua::{AnyUserData, Function, Lua, Result, Table, Value};
use wowprogramming_lua::mlua;

use crate::natives::safe_call;
use crate::state::{Timer, state};

/// Marker stored at `[0]` of timer handle tables.
struct TimerRef(Rc<Cell<bool>>);

pub fn install(lua: &Lua) -> Result<()> {
    let handle_methods = lua.create_table()?;
    handle_methods.set(
        "Cancel",
        lua.create_function(|_, this: Table| {
            if let Ok(ud) = this.raw_get::<AnyUserData>(0) {
                ud.borrow::<TimerRef>()?.0.set(true);
            }
            Ok(())
        })?,
    )?;
    handle_methods.set(
        "IsCancelled",
        lua.create_function(|_, this: Table| {
            let ud: AnyUserData = this.raw_get(0)?;
            Ok(ud.borrow::<TimerRef>()?.0.get())
        })?,
    )?;
    let handle_mt = lua.create_table()?;
    handle_mt.set("__index", handle_methods)?;

    let c_timer = lua.create_table()?;
    c_timer.set(
        "After",
        lua.create_function(|lua, (seconds, callback): (f64, Value)| {
            let Value::Function(callback) = callback else {
                return Err(mlua::Error::runtime(
                    "Usage: C_Timer.After(seconds, callback)",
                ));
            };
            add_timer(lua, seconds, callback, None, false, None)?;
            Ok(())
        })?,
    )?;
    let mt = handle_mt.clone();
    c_timer.set(
        "NewTimer",
        lua.create_function(move |lua, (seconds, callback): (f64, Value)| {
            let Value::Function(callback) = callback else {
                return Err(mlua::Error::runtime(
                    "Usage: C_Timer.NewTimer(seconds, callback)",
                ));
            };
            add_timer(lua, seconds, callback, Some(&mt), false, None)
        })?,
    )?;
    let mt = handle_mt;
    c_timer.set(
        "NewTicker",
        lua.create_function(
            move |lua, (seconds, callback, iterations): (f64, Value, Option<u64>)| {
                let Value::Function(callback) = callback else {
                    return Err(mlua::Error::runtime(
                        "Usage: C_Timer.NewTicker(seconds, callback [, iterations])",
                    ));
                };
                add_timer(lua, seconds, callback, Some(&mt), true, iterations)
            },
        )?,
    )?;
    lua.globals().set("C_Timer", c_timer)?;
    Ok(())
}

fn add_timer(
    lua: &Lua,
    seconds: f64,
    callback: Function,
    handle_mt: Option<&Table>,
    repeating: bool,
    iterations: Option<u64>,
) -> Result<Option<Table>> {
    let cancelled = Rc::new(Cell::new(false));
    let handle = match handle_mt {
        Some(mt) => {
            let t = lua.create_table()?;
            t.raw_set(0, lua.create_any_userdata(TimerRef(Rc::clone(&cancelled)))?)?;
            t.set_metatable(Some(mt.clone()))?;
            Some(t)
        }
        None => None,
    };
    let mut st = state(lua);
    let id = st.next_timer_id;
    st.next_timer_id += 1;
    let interval = seconds.max(0.0);
    let due = st.time + interval;
    st.timers.push(Timer {
        id,
        due,
        interval,
        remaining: iterations.filter(|&n| n > 0),
        repeating,
        callback,
        handle: handle.clone(),
        cancelled,
    });
    Ok(handle)
}

/// Run every timer that is due at the current engine time. Timers created
/// while this runs wait for the next call.
pub fn run_due(lua: &Lua) {
    let due: Vec<u64> = {
        let mut st = state(lua);
        st.timers.retain(|t| !t.cancelled.get());
        let now = st.time;
        let mut due: Vec<(f64, u64)> = st
            .timers
            .iter()
            .filter(|t| t.due <= now)
            .map(|t| (t.due, t.id))
            .collect();
        due.sort_by(|a, b| a.partial_cmp(b).unwrap());
        due.into_iter().map(|(_, id)| id).collect()
    };
    for id in due {
        let call = {
            let mut st = state(lua);
            let now = st.time;
            let Some(pos) = st.timers.iter().position(|t| t.id == id) else {
                continue;
            };
            if st.timers[pos].cancelled.get() {
                continue;
            }
            let timer = &mut st.timers[pos];
            let call = (timer.callback.clone(), timer.handle.clone());
            let finished = if timer.repeating {
                timer.due = now + timer.interval;
                match &mut timer.remaining {
                    Some(n) => {
                        *n -= 1;
                        *n == 0
                    }
                    None => false,
                }
            } else {
                true
            };
            if finished {
                st.timers[pos].cancelled.set(true);
                st.timers.remove(pos);
            }
            call
        };
        match call {
            (f, Some(handle)) => safe_call(lua, &f, handle),
            (f, None) => safe_call(lua, &f, ()),
        };
    }
}
