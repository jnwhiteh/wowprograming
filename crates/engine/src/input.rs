//! Mouse input: hover (`OnEnter`/`OnLeave`), presses and clicks.
//! Positions are in screen units with y pointing up.

use mlua::Lua;
use wowprogramming_lua::mlua;

use crate::frames::{fire_script, is_visible};
use crate::layout;
use crate::scene::frame_order;
use crate::state::{FrameId, state};
use crate::widgets::{click, update_button_font};

/// The topmost visible frame under the point that takes mouse input.
pub fn hit_test(lua: &Lua, x: f64, y: f64) -> Option<FrameId> {
    let mut st = state(lua);
    let order = frame_order(&st);
    for &id in order.iter().rev() {
        let f = &st.frames[id];
        if !(f.mouse_enabled || f.mouse_motion) || !is_visible(&st.frames, id) {
            continue;
        }
        let insets = f.hit_insets;
        let Some(r) = layout::rect(&mut st, id) else {
            continue;
        };
        let es = layout::effective_scale(&st, id);
        let inside = x >= r.left + insets[0] * es
            && x <= r.right() - insets[1] * es
            && y <= r.top() - insets[2] * es
            && y >= r.bottom + insets[3] * es;
        if !inside {
            continue;
        }
        // Clipping ancestors hide the parts of children outside them.
        let mut clipped = false;
        let mut p = st.frames[id].parent;
        while let Some(pid) = p {
            if st.frames[pid].clips_children
                && let Some(pr) = layout::rect(&mut st, pid)
                && !pr.contains(x, y)
            {
                clipped = true;
                break;
            }
            p = st.frames[pid].parent;
        }
        if !clipped {
            return Some(id);
        }
    }
    None
}

pub fn mouse_move(lua: &Lua, x: f64, y: f64) {
    state(lua).mouse = (x, y);
    let hit = hit_test(lua, x, y);
    let old = state(lua).mouse_focus;
    if old == hit {
        return;
    }
    {
        let mut st = state(lua);
        st.mouse_focus = hit;
        if let Some(o) = old {
            st.frames[o].mouse_over = false;
            if let Some(b) = st.frames[o].button.as_deref_mut() {
                b.pushed = false;
            }
        }
        if let Some(h) = hit {
            st.frames[h].mouse_over = true;
        }
    }
    if let Some(o) = old {
        update_button_font(lua, o);
        if motion_allowed(lua, o) {
            fire_script(lua, o, "OnLeave", true);
        }
    }
    if let Some(h) = hit {
        update_button_font(lua, h);
        if motion_allowed(lua, h) {
            fire_script(lua, h, "OnEnter", true);
        }
    }
}

fn motion_allowed(lua: &Lua, id: FrameId) -> bool {
    let st = state(lua);
    match st.frames[id].button.as_deref() {
        Some(b) => b.enabled || b.motion_scripts_while_disabled,
        None => true,
    }
}

pub fn mouse_down(lua: &Lua, button: &str) {
    let Some(id) = state(lua).mouse_focus else {
        // Clicking empty space clears keyboard focus.
        let focus = state(lua).focus.take();
        if let Some(f) = focus {
            fire_script(lua, f, "OnEditFocusLost", ());
        }
        return;
    };
    let is_edit = state(lua).frames[id].object_type == "EditBox";
    if is_edit {
        let old = state(lua).focus.replace(id);
        if let Some(o) = old.filter(|&o| o != id) {
            fire_script(lua, o, "OnEditFocusLost", ());
        }
        if old != Some(id) {
            fire_script(lua, id, "OnEditFocusGained", ());
        }
    }
    if let Some(b) = state(lua).frames[id].button.as_deref_mut()
        && b.enabled
    {
        b.pushed = true;
    }
    fire_script(lua, id, "OnMouseDown", button);
}

pub fn mouse_up(lua: &Lua, button: &str) {
    let Some(id) = state(lua).mouse_focus else {
        return;
    };
    let was_pushed = {
        let mut st = state(lua);
        match st.frames[id].button.as_deref_mut() {
            Some(b) => {
                let p = b.pushed;
                b.pushed = false;
                Some(p)
            }
            None => None,
        }
    };
    fire_script(lua, id, "OnMouseUp", button);
    if was_pushed == Some(true) {
        let registered = {
            let st = state(lua);
            st.frames[id].button.as_deref().is_some_and(|b| {
                b.clicks.iter().any(|c| {
                    let c = c.to_ascii_lowercase();
                    c == "anyup"
                        || c == format!("{}up", button.to_ascii_lowercase())
                        || c == "anydown"
                        || c == format!("{}down", button.to_ascii_lowercase())
                })
            })
        };
        if registered {
            click(lua, id, button, false);
        }
    }
}

pub fn mouse_wheel(lua: &Lua, delta: f64) {
    let mut target = state(lua).mouse_focus;
    while let Some(id) = target {
        if state(lua).frames[id].has_script("OnMouseWheel") {
            fire_script(lua, id, "OnMouseWheel", delta);
            return;
        }
        target = state(lua).frames[id].parent;
    }
}

/// Typed text: goes to the focused edit box, one character at a time, as
/// OnChar and OnTextChanged(userInput = true). Returns false without a
/// focused edit box.
pub fn text_input(lua: &Lua, text: &str) -> bool {
    let Some(id) = state(lua).focus else {
        return false;
    };
    for c in text.chars() {
        let current = state(lua).frames[id]
            .text
            .as_deref()
            .map(|t| t.text.clone())
            .unwrap_or_default();
        crate::widgets::set_text(lua, id, format!("{current}{c}"));
        fire_script(lua, id, "OnChar", c.to_string());
        fire_script(lua, id, "OnTextChanged", true);
    }
    true
}

/// Keys that a focused edit box handles itself. Returns true if the key
/// went to the edit box.
pub fn edit_box_key(lua: &Lua, key: &str) -> bool {
    let Some(id) = state(lua).focus else {
        return false;
    };
    match key.to_ascii_uppercase().as_str() {
        "BACKSPACE" => {
            let mut text = state(lua).frames[id]
                .text
                .as_deref()
                .map(|t| t.text.clone())
                .unwrap_or_default();
            text.pop();
            crate::widgets::set_text(lua, id, text);
            fire_script(lua, id, "OnTextChanged", true);
        }
        "ENTER" => fire_script(lua, id, "OnEnterPressed", ()),
        "ESCAPE" => fire_script(lua, id, "OnEscapePressed", ()),
        "TAB" => fire_script(lua, id, "OnTabPressed", ()),
        _ => return false,
    }
    true
}
