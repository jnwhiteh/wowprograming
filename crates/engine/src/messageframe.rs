//! Message frames (`UIErrorsFrame`, `RaidWarningFrame`): lines of text
//! that stay for a while and fade out. Rules and sources are in
//! docs/client-runtime.md ("Message frames").

use mlua::{Lua, Result, Table, Value, Variadic};
use wowprogramming_lua::mlua;

use crate::frames::this;
use crate::state::{Message, MessageState, State, state};
use crate::widgets::{num, truthy};

fn with_msg<R>(lua: &Lua, id: usize, f: impl FnOnce(&mut MessageState) -> R) -> R {
    let mut st = state(lua);
    let m = st.frames[id].messages.get_or_insert_with(Box::default);
    f(m)
}

fn msg(lua: &Lua, id: usize) -> MessageState {
    state(lua).frames[id]
        .messages
        .as_deref()
        .cloned()
        .unwrap_or_default()
}

/// The alpha of a message of age `age`: full while it is shown, then a
/// fade over the fade duration (shaped by the fade power).
fn message_alpha(m: &MessageState, age: f64) -> f64 {
    if !m.fading || age < m.time_visible {
        return 1.0;
    }
    if m.fade_duration <= 0.0 {
        return 0.0;
    }
    let t = ((age - m.time_visible) / m.fade_duration).clamp(0.0, 1.0);
    (1.0 - t).powf(m.fade_power.max(0.01))
}

/// The text to draw now: the visible messages, one per line, in their
/// colours and fade, oldest first for BOTTOM, newest first for TOP. The
/// second value is the vertical justification.
pub fn display_text(st: &State, id: usize) -> Option<(String, &'static str)> {
    let m = st.frames[id].messages.as_deref()?;
    let now = st.time;
    let mut lines: Vec<String> = Vec::new();
    for msg in &m.messages {
        let a = message_alpha(m, now - msg.time);
        if a <= 0.0 {
            continue;
        }
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
        let c = msg.color;
        lines.push(format!(
            "|c{:02x}{:02x}{:02x}{:02x}{}|r",
            byte(c[3] * a as f32),
            byte(c[0]),
            byte(c[1]),
            byte(c[2]),
            msg.text
        ));
    }
    if lines.is_empty() {
        return None;
    }
    if m.insert_mode == "TOP" {
        lines.reverse();
        Some((lines.join("\n"), "TOP"))
    } else {
        Some((lines.join("\n"), "BOTTOM"))
    }
}

/// Drop messages that have faded out.
pub fn update(lua: &Lua) {
    let mut st = state(lua);
    let now = st.time;
    for f in st.frames.iter_mut() {
        if let Some(m) = f.messages.as_deref_mut()
            && m.fading
        {
            let limit = m.time_visible + m.fade_duration;
            m.messages.retain(|msg| now - msg.time < limit);
        }
    }
}

pub fn methods(lua: &Lua) -> Result<Table> {
    let m = lua.create_table()?;
    macro_rules! method {
        ($name:literal, $f:expr) => {
            m.raw_set($name, lua.create_function($f)?)?;
        };
    }
    macro_rules! number {
        ($set:literal, $get:literal, $field:ident) => {
            method!($set, |lua, args: Variadic<Value>| {
                let id = this(args.first().unwrap_or(&Value::Nil))?;
                let v = num(args.get(1));
                with_msg(lua, id, |m| m.$field = v);
                Ok(())
            });
            method!($get, |lua, this_: Value| {
                let id = this(&this_)?;
                Ok(msg(lua, id).$field)
            });
        };
    }
    number!("SetTimeVisible", "GetTimeVisible", time_visible);
    number!("SetFadeDuration", "GetFadeDuration", fade_duration);
    number!("SetFadePower", "GetFadePower", fade_power);
    method!("SetFading", |lua, (this_, v): (Value, Value)| {
        let id = this(&this_)?;
        with_msg(lua, id, |m| m.fading = truthy(&v));
        Ok(())
    });
    method!("GetFading", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(msg(lua, id).fading)
    });
    method!("SetInsertMode", |lua, (this_, mode): (Value, Value)| {
        let id = this(&this_)?;
        // Enum.InsertMode: Top = 0, Bottom = 1; strings work too.
        let top = match &mode {
            Value::String(s) => s.to_string_lossy().eq_ignore_ascii_case("TOP"),
            v => num(Some(v)) == 0.0,
        };
        with_msg(lua, id, |m| {
            m.insert_mode = if top { "TOP" } else { "BOTTOM" }.into()
        });
        Ok(())
    });
    method!("GetInsertMode", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(if msg(lua, id).insert_mode == "TOP" {
            0
        } else {
            1
        })
    });
    method!("AddMessage", |lua, args: Variadic<Value>| {
        let id = this(args.first().unwrap_or(&Value::Nil))?;
        let text = crate::widgets::lua_tostring(lua, args.get(1).unwrap_or(&Value::Nil))?;
        // Without a colour the message takes the font's colour (inferred:
        // white).
        let has = |i: usize| args.get(i).is_some_and(|v| !v.is_nil());
        let color = if has(2) {
            [
                num(args.get(2)) as f32,
                num(args.get(3)) as f32,
                num(args.get(4)) as f32,
                if has(5) { num(args.get(5)) as f32 } else { 1.0 },
            ]
        } else {
            [1.0; 4]
        };
        let message_id = has(6).then(|| num(args.get(6)));
        let now = state(lua).time;
        with_msg(lua, id, |m| {
            // A message with the same ID replaces the old one (inferred).
            if let Some(mid) = message_id {
                m.messages.retain(|x| x.id != Some(mid));
            }
            m.messages.push(Message {
                text,
                color,
                id: message_id,
                time: now,
            });
        });
        crate::layout::invalidate(&mut state(lua));
        Ok(())
    });
    method!("Clear", |lua, this_: Value| {
        let id = this(&this_)?;
        with_msg(lua, id, |m| m.messages.clear());
        Ok(())
    });
    method!("HasMessageByID", |lua, (this_, mid): (Value, f64)| {
        let id = this(&this_)?;
        Ok(msg(lua, id).messages.iter().any(|x| x.id == Some(mid)))
    });
    method!("ResetMessageFadeByID", |lua, (this_, mid): (Value, f64)| {
        let id = this(&this_)?;
        let now = state(lua).time;
        with_msg(lua, id, |m| {
            for x in m.messages.iter_mut().filter(|x| x.id == Some(mid)) {
                x.time = now;
            }
        });
        Ok(())
    });
    Ok(m)
}
