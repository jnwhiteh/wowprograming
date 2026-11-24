//! Sliders: the thumb's place along the track, and dragging it with the
//! mouse. Rules and sources are in docs/client-runtime.md ("Sliders").

use mlua::Lua;
use wowprogramming_lua::mlua;

use crate::frames::fire_script;
use crate::layout::{self, Rect};
use crate::state::{FrameId, State, state};

/// The slider a thumb texture belongs to.
pub fn thumb_owner(st: &State, id: FrameId) -> Option<FrameId> {
    let p = st.frames[id].parent?;
    (st.frames[p].object_type == "Slider" && st.frames[p].status_texture == Some(id)).then_some(p)
}

fn fraction(st: &State, slider: FrameId) -> f64 {
    let (min, max, value) = st.frames[slider].value;
    if max > min {
        ((value - min) / (max - min)).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The thumb keeps its own size and moves along the slider: left to right
/// for horizontal sliders, top to bottom for vertical ones, staying inside
/// the slider (inferred).
pub fn thumb_rect(st: &State, slider: FrameId, track: Rect, size: (f64, f64)) -> Rect {
    let (w, h) = size;
    let frac = fraction(st, slider);
    if st.frames[slider].orientation == "VERTICAL" {
        let top = track.top() - frac * (track.height - h);
        Rect {
            left: track.left + (track.width - w) / 2.0,
            bottom: top - h,
            width: w,
            height: h,
        }
    } else {
        Rect {
            left: track.left + frac * (track.width - w),
            bottom: track.bottom + (track.height - h) / 2.0,
            width: w,
            height: h,
        }
    }
}

/// Set a slider's value, rounded to its step and clamped. Fires
/// OnValueChanged on a change.
pub fn set_value(lua: &Lua, id: FrameId, v: f64, user_input: bool, round: bool) {
    let changed = {
        let mut st = state(lua);
        let f = &mut st.frames[id];
        let (min, max) = (f.value.0, f.value.1.max(f.value.0));
        let mut v = v.clamp(min, max);
        if round && f.value_step > 0.0 {
            v = (min + ((v - min) / f.value_step).round() * f.value_step).clamp(min, max);
        }
        let changed = (f.value.2 - v).abs() > f64::EPSILON;
        f.value.2 = v;
        if changed {
            layout::invalidate(&mut st);
        }
        changed.then_some(v)
    };
    if let Some(v) = changed {
        fire_script(lua, id, "OnValueChanged", (v, user_input));
    }
}

/// A press on an enabled slider: a press on the thumb grabs it where it
/// was pressed; a press elsewhere moves the thumb's centre to the mouse.
pub fn press(lua: &Lua, id: FrameId) {
    let grab = {
        let mut st = state(lua);
        let f = &st.frames[id];
        if f.object_type != "Slider" || f.disabled {
            return;
        }
        let Some(thumb) = f.status_texture else {
            return;
        };
        let mouse = st.mouse;
        match layout::rect(&mut st, thumb) {
            Some(r) if r.contains(mouse.0, mouse.1) => {
                let (cx, cy) = r.point("CENTER");
                (mouse.0 - cx, mouse.1 - cy)
            }
            _ => (0.0, 0.0),
        }
    };
    state(lua).slider_drag = Some((id, grab));
    follow_mouse(lua);
}

/// While the button is held, the value follows the mouse.
pub fn follow_mouse(lua: &Lua) {
    let Some((id, grab)) = state(lua).slider_drag else {
        return;
    };
    let value = {
        let mut st = state(lua);
        let Some(thumb) = st.frames[id].status_texture else {
            return;
        };
        let (Some(track), Some(t)) = (layout::rect(&mut st, id), layout::rect(&mut st, thumb))
        else {
            return;
        };
        let (min, max, _) = st.frames[id].value;
        let mouse = st.mouse;
        let frac = if st.frames[id].orientation == "VERTICAL" {
            let travel = track.height - t.height;
            let y = mouse.1 - grab.1;
            if travel > 0.0 {
                (track.top() - t.height / 2.0 - y) / travel
            } else {
                0.0
            }
        } else {
            let travel = track.width - t.width;
            let x = mouse.0 - grab.0;
            if travel > 0.0 {
                (x - track.left - t.width / 2.0) / travel
            } else {
                0.0
            }
        };
        min + frac.clamp(0.0, 1.0) * (max - min)
    };
    let round = state(lua).frames[id].obey_step_on_drag;
    set_value(lua, id, value, true, round);
}

pub fn release(lua: &Lua) {
    state(lua).slider_drag = None;
}
