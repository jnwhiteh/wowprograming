//! Cooldown frames: the swipe that shows how much of a cooldown is left,
//! and OnCooldownDone at the end. Rules and sources are in
//! docs/client-runtime.md ("Cooldown frames").

use mlua::{Lua, Result, Table, Value, Variadic};
use wowprogramming_lua::mlua;

use crate::frames::{change_visibility, fire_script, this};
use crate::state::{CooldownState, FrameId, state};
use crate::widgets::{num, truthy};

fn with_cd<R>(lua: &Lua, id: FrameId, f: impl FnOnce(&mut CooldownState) -> R) -> R {
    let mut st = state(lua);
    let c = st.frames[id].cooldown.get_or_insert_with(Box::default);
    f(c)
}

fn cd(lua: &Lua, id: FrameId) -> CooldownState {
    state(lua).frames[id]
        .cooldown
        .as_deref()
        .cloned()
        .unwrap_or_default()
}

fn set_shown(lua: &Lua, id: FrameId, shown: bool) {
    if state(lua).frames[id].shown != shown {
        change_visibility(lua, id, |frames| frames[id].shown = shown);
    }
}

/// Start a cooldown. A start or duration of 0 or less clears it. The
/// widget shows itself.
pub fn start(lua: &Lua, id: FrameId, start: f64, duration: f64, mod_rate: f64) {
    if start <= 0.0 || duration <= 0.0 {
        clear(lua, id);
        return;
    }
    let now = state(lua).time;
    with_cd(lua, id, |c| {
        c.start = start;
        c.duration = duration;
        c.mod_rate = if mod_rate > 0.0 { mod_rate } else { 1.0 };
        c.active = true;
        c.paused_at = c.paused_at.map(|_| now);
    });
    crate::layout::invalidate(&mut state(lua));
    set_shown(lua, id, true);
}

/// `Clear`: no cooldown, no swipe, no OnCooldownDone; the widget hides
/// (inferred).
pub fn clear(lua: &Lua, id: FrameId) {
    with_cd(lua, id, |c| {
        c.active = false;
        c.start = 0.0;
        c.duration = 0.0;
    });
    set_shown(lua, id, false);
}

/// The fraction of the cooldown that has passed at `now`, or `None` when
/// no cooldown runs.
pub fn elapsed_fraction(c: &CooldownState, now: f64) -> Option<f64> {
    if !c.active || c.duration <= 0.0 {
        return None;
    }
    let now = c.paused_at.unwrap_or(now);
    let length = c.duration / c.mod_rate;
    Some(((now - c.start) / length).clamp(0.0, 1.0))
}

/// Once per frame: end the cooldowns whose time is up. The widget hides
/// and OnCooldownDone fires (order inferred, so a handler can start the
/// next cooldown).
pub fn update(lua: &Lua) {
    let done: Vec<FrameId> = {
        let mut st = state(lua);
        let now = st.time;
        let mut out = Vec::new();
        for (id, f) in st.frames.iter_mut().enumerate() {
            let Some(c) = f.cooldown.as_deref_mut() else {
                continue;
            };
            if c.active && c.paused_at.is_none() && now >= c.start + c.duration / c.mod_rate {
                c.active = false;
                out.push(id);
            }
        }
        out
    };
    for id in done {
        set_shown(lua, id, false);
        fire_script(lua, id, "OnCooldownDone", ());
    }
}

fn id_of(args: &[Value]) -> Result<FrameId> {
    this(args.first().unwrap_or(&Value::Nil))
}

pub fn methods(lua: &Lua) -> Result<Table> {
    let m = lua.create_table()?;
    macro_rules! method {
        ($name:literal, $f:expr) => {
            m.raw_set($name, lua.create_function($f)?)?;
        };
    }
    macro_rules! flag {
        ($set:literal, $get:literal, $field:ident) => {
            method!($set, |lua, args: Variadic<Value>| {
                let id = id_of(&args)?;
                let v = truthy(args.get(1).unwrap_or(&Value::Nil));
                with_cd(lua, id, |c| c.$field = v);
                Ok(())
            });
            method!($get, |lua, this_: Value| {
                let id = this(&this_)?;
                Ok(cd(lua, id).$field)
            });
        };
    }
    macro_rules! number {
        ($set:literal, $get:literal, $field:ident) => {
            method!($set, |lua, args: Variadic<Value>| {
                let id = id_of(&args)?;
                let v = num(args.get(1));
                with_cd(lua, id, |c| c.$field = v);
                Ok(())
            });
            method!($get, |lua, this_: Value| {
                let id = this(&this_)?;
                Ok(cd(lua, id).$field)
            });
        };
    }
    flag!("SetDrawSwipe", "GetDrawSwipe", draw_swipe);
    flag!("SetDrawEdge", "GetDrawEdge", draw_edge);
    flag!("SetDrawBling", "GetDrawBling", draw_bling);
    flag!("SetReverse", "GetReverse", reverse);
    flag!(
        "SetHideCountdownNumbers",
        "GetHideCountdownNumbers",
        hide_numbers
    );
    flag!(
        "SetUseAuraDisplayTime",
        "GetUseAuraDisplayTime",
        use_aura_display_time
    );
    number!("SetRotation", "GetRotation", rotation);
    number!("SetEdgeScale", "GetEdgeScale", edge_scale);
    number!(
        "SetCountdownAbbrevThreshold",
        "GetCountdownAbbrevThreshold",
        abbrev_threshold
    );
    number!(
        "SetCountdownMillisecondsThreshold",
        "GetCountdownMillisecondsThreshold",
        milliseconds_threshold
    );
    number!(
        "SetMinimumCountdownDuration",
        "GetMinimumCountdownDuration",
        minimum_countdown_duration
    );
    method!("SetCooldown", |lua, args: Variadic<Value>| {
        let id = id_of(&args)?;
        let mod_rate = match args.get(3) {
            Some(v) if !v.is_nil() => num(Some(v)),
            _ => 1.0,
        };
        start(lua, id, num(args.get(1)), num(args.get(2)), mod_rate);
        Ok(())
    });
    method!("SetCooldownDuration", |lua, args: Variadic<Value>| {
        let id = id_of(&args)?;
        let now = state(lua).time;
        let mod_rate = match args.get(2) {
            Some(v) if !v.is_nil() => num(Some(v)),
            _ => 1.0,
        };
        start(lua, id, now, num(args.get(1)), mod_rate);
        Ok(())
    });
    method!(
        "SetCooldownFromExpirationTime",
        |lua, args: Variadic<Value>| {
            let id = id_of(&args)?;
            let duration = num(args.get(2));
            let mod_rate = match args.get(3) {
                Some(v) if !v.is_nil() => num(Some(v)),
                _ => 1.0,
            };
            let expiration = num(args.get(1));
            start(
                lua,
                id,
                expiration - duration / mod_rate.max(1e-6),
                duration,
                mod_rate,
            );
            Ok(())
        }
    );
    method!("SetCooldownUNIX", |lua, args: Variadic<Value>| {
        let id = id_of(&args)?;
        // A start in seconds since 1970: convert it to GetTime() seconds.
        let unix_now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let now = state(lua).time;
        let mod_rate = match args.get(3) {
            Some(v) if !v.is_nil() => num(Some(v)),
            _ => 1.0,
        };
        let s = num(args.get(1));
        let begin = if s > 0.0 { now - (unix_now - s) } else { 0.0 };
        start(lua, id, begin, num(args.get(2)), mod_rate);
        Ok(())
    });
    method!("Clear", |lua, this_: Value| {
        let id = this(&this_)?;
        clear(lua, id);
        Ok(())
    });
    method!("GetCooldownTimes", |lua, this_: Value| {
        let id = this(&this_)?;
        let c = cd(lua, id);
        Ok((c.start * 1000.0, c.duration * 1000.0))
    });
    method!("GetCooldownDuration", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(cd(lua, id).duration * 1000.0)
    });
    method!("GetCooldownDisplayDuration", |lua, this_: Value| {
        let id = this(&this_)?;
        let c = cd(lua, id);
        Ok(c.duration / c.mod_rate)
    });
    method!("Pause", |lua, this_: Value| {
        let id = this(&this_)?;
        let now = state(lua).time;
        with_cd(lua, id, |c| {
            c.paused_at.get_or_insert(now);
        });
        Ok(())
    });
    method!("Resume", |lua, this_: Value| {
        let id = this(&this_)?;
        let now = state(lua).time;
        with_cd(lua, id, |c| {
            if let Some(p) = c.paused_at.take() {
                c.start += now - p;
            }
        });
        Ok(())
    });
    method!("SetPaused", |lua, (this_, v): (Value, Value)| {
        let id = this(&this_)?;
        let now = state(lua).time;
        let pause = truthy(&v);
        with_cd(lua, id, |c| match (pause, c.paused_at) {
            (true, None) => c.paused_at = Some(now),
            (false, Some(p)) => {
                c.start += now - p;
                c.paused_at = None;
            }
            _ => {}
        });
        Ok(())
    });
    method!("IsPaused", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(cd(lua, id).paused_at.is_some())
    });
    method!("SetCountdownFont", |lua, (this_, font): (Value, Value)| {
        let id = this(&this_)?;
        let name = match font {
            Value::String(s) => Some(s.to_string_lossy()),
            _ => None,
        };
        with_cd(lua, id, |c| c.countdown_font = name);
        Ok(())
    });
    // The part of the swipe texture used: low (left, top), high (right,
    // bottom) as two vectors (stored; the swipe uses the whole texture).
    method!("SetTexCoordRange", |lua, args: Variadic<Value>| {
        let id = id_of(&args)?;
        let v = |i: usize| -> (f64, f64) {
            match args.get(i) {
                Some(Value::Table(t)) => (
                    t.get::<Option<f64>>("x").ok().flatten().unwrap_or(0.0),
                    t.get::<Option<f64>>("y").ok().flatten().unwrap_or(0.0),
                ),
                _ => (0.0, 0.0),
            }
        };
        let range = (v(1), v(2));
        with_cd(lua, id, |c| c.tex_coord_range = Some(range));
        Ok(())
    });
    method!("SetSwipeColor", |lua, args: Variadic<Value>| {
        let id = id_of(&args)?;
        let a = match args.get(4) {
            Some(v) if !v.is_nil() => num(Some(v)),
            _ => 1.0,
        };
        let color = [
            num(args.get(1)) as f32,
            num(args.get(2)) as f32,
            num(args.get(3)) as f32,
            a as f32,
        ];
        with_cd(lua, id, |c| c.swipe_color = color);
        Ok(())
    });
    method!("SetSwipeTexture", |lua, args: Variadic<Value>| {
        let id = id_of(&args)?;
        let file = crate::widgets::tex_file_arg(args.get(1).unwrap_or(&Value::Nil));
        let color = match args.get(2) {
            Some(v) if !v.is_nil() => Some([
                num(Some(v)) as f32,
                num(args.get(3)) as f32,
                num(args.get(4)) as f32,
                match args.get(5) {
                    Some(a) if !a.is_nil() => num(Some(a)) as f32,
                    _ => 1.0,
                },
            ]),
            _ => None,
        };
        with_cd(lua, id, |c| {
            c.swipe_texture = file;
            if let Some(col) = color {
                c.swipe_color = col;
            }
        });
        Ok(())
    });
    for name in [
        "SetEdgeTexture",
        "SetBlingTexture",
        "SetEdgeColor",
        "SetUseCircularEdge",
        "SetCountdownFormatter",
    ] {
        m.raw_set(name, crate::widgets::empty_fn(lua)?)?;
    }
    Ok(m)
}
