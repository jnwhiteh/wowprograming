//! Mouse input: hover (`OnEnter`/`OnLeave`), presses and clicks.
//! Positions are in screen units with y pointing up.

use mlua::Lua;
use wowprogramming_lua::mlua;

use crate::frames::{fire_script, is_visible};
use crate::layout;
use crate::scene::frame_order;
use crate::state::{FrameId, LastClick, Press, state};
use crate::widgets::{click, update_button_font};

/// The topmost visible frame under the point that takes mouse input.
pub fn hit_test(lua: &Lua, x: f64, y: f64) -> Option<FrameId> {
    hit_test_where(lua, x, y, false)
}

/// The topmost visible frame under the point that takes mouse clicks.
/// Frames with only motion enabled (`SetMouseClickEnabled(false)`, as on
/// the player frame's health bar) let presses through to the frame below.
pub fn click_test(lua: &Lua, x: f64, y: f64) -> Option<FrameId> {
    hit_test_where(lua, x, y, true)
}

fn hit_test_where(lua: &Lua, x: f64, y: f64, clicks_only: bool) -> Option<FrameId> {
    let mut st = state(lua);
    let order = frame_order(&st);
    for &id in order.iter().rev() {
        let f = &st.frames[id];
        let takes = if clicks_only {
            f.mouse_enabled
        } else {
            f.mouse_enabled || f.mouse_motion
        };
        if !takes || !is_visible(&st.frames, id) {
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

/// How far the mouse must move after a press before OnDragStart fires,
/// in screen pixels. Inferred (the Windows default); see client-runtime.md.
pub const DRAG_THRESHOLD_PIXELS: f64 = 4.0;

/// Longest time between two clicks of a double click, in seconds.
/// Inferred (the Windows default); see client-runtime.md.
pub const DOUBLE_CLICK_TIME: f64 = 0.5;

pub fn mouse_move(lua: &Lua, x: f64, y: f64) {
    state(lua).mouse = (x, y);
    crate::movement::follow_mouse(lua);
    crate::slider::follow_mouse(lua);
    maybe_start_drag(lua);
    update_focus(lua, true);
    crate::hyperlink::update(lua);
}

/// Fire OnDragStart when the held button has moved far enough over a
/// frame registered for dragging with it.
fn maybe_start_drag(lua: &Lua) {
    let (frame, button) = {
        let mut st = state(lua);
        let threshold = DRAG_THRESHOLD_PIXELS / st.pixel_scale.max(0.01);
        let mouse = st.mouse;
        let Some(p) = st.press.as_mut() else { return };
        if p.dragging {
            return;
        }
        let (dx, dy) = (mouse.0 - p.at.0, mouse.1 - p.at.1);
        if (dx * dx + dy * dy).sqrt() < threshold {
            return;
        }
        let (frame, button) = (p.frame, p.button.clone());
        let registered = st.frames[frame].drag_buttons.iter().any(|b| {
            let b = b.to_ascii_lowercase();
            b == button.to_ascii_lowercase() || b == "anybutton"
        });
        if !registered {
            return;
        }
        let p = st.press.as_mut().expect("press");
        p.dragging = true;
        if let Some(b) = st.frames[frame].button.as_deref_mut() {
            b.pushed = false;
        }
        (frame, button)
    };
    fire_script(lua, frame, "OnDragStart", button);
}

/// Recompute the frame under the mouse and fire OnLeave and OnEnter.
/// `motion` is false when frames moved, showed or hid under a still mouse.
pub fn update_focus(lua: &Lua, motion: bool) {
    let (x, y) = state(lua).mouse;
    let hit = hit_test(lua, x, y);
    let old = {
        let mut st = state(lua);
        st.hover_generation = st.layout_generation;
        st.mouse_focus
    };
    if old == hit {
        return;
    }
    {
        let mut st = state(lua);
        st.mouse_focus = hit;
        let captured = st.press.as_ref().filter(|p| !p.dragging).map(|p| p.frame);
        if let Some(o) = old {
            st.frames[o].mouse_over = false;
            if let Some(b) = st.frames[o].button.as_deref_mut() {
                b.pushed = false;
            }
        }
        if let Some(h) = hit {
            st.frames[h].mouse_over = true;
            // Coming back over the pressed button pushes it again.
            if captured == Some(h)
                && let Some(b) = st.frames[h].button.as_deref_mut()
                && b.enabled
            {
                b.pushed = true;
            }
        }
    }
    if let Some(o) = old {
        update_button_font(lua, o);
        if motion_allowed(lua, o) {
            fire_script(lua, o, "OnLeave", motion);
        }
    }
    if let Some(h) = hit {
        update_button_font(lua, h);
        if motion_allowed(lua, h) {
            fire_script(lua, h, "OnEnter", motion);
        }
    }
}

/// Once per frame: if the layout or visibility changed, frames may have
/// moved under the still mouse.
pub fn refresh_focus(lua: &Lua) {
    let changed = {
        let st = state(lua);
        st.hover_generation != st.layout_generation
    };
    if changed {
        update_focus(lua, false);
        crate::hyperlink::update(lua);
    }
}

fn motion_allowed(lua: &Lua, id: FrameId) -> bool {
    let st = state(lua);
    match st.frames[id].button.as_deref() {
        Some(b) => b.enabled || b.motion_scripts_while_disabled,
        None => true,
    }
}

/// Whether a button is registered for clicks of `button` going up or down.
fn registered_for(lua: &Lua, id: FrameId, button: &str, down: bool) -> bool {
    let st = state(lua);
    let dir = if down { "down" } else { "up" };
    let wanted = format!("{}{dir}", button.to_ascii_lowercase());
    let any = format!("any{dir}");
    st.frames[id].button.as_deref().is_some_and(|b| {
        b.clicks.iter().any(|c| {
            let c = c.to_ascii_lowercase();
            c == wanted || c == any
        })
    })
}

pub fn mouse_down(lua: &Lua, button: &str) {
    if state(lua).press.is_some() {
        // A second button while one is held: the client tracks one
        // capture at a time here.
        return;
    }
    let (x, y) = state(lua).mouse;
    let Some(id) = click_test(lua, x, y) else {
        // Clicking empty space clears keyboard focus.
        let focus = state(lua).focus.take();
        if let Some(f) = focus {
            fire_script(lua, f, "OnEditFocusLost", ());
        }
        return;
    };
    let is_edit = {
        let st = state(lua);
        st.frames[id].object_type == "EditBox" && !st.frames[id].disabled
    };
    if is_edit {
        crate::editbox::set_focus(lua, id);
    }
    crate::movement::raise_toplevel(lua, id);
    {
        let mut st = state(lua);
        let at = st.mouse;
        st.press = Some(Press {
            frame: id,
            button: button.to_string(),
            at,
            dragging: false,
        });
        if let Some(b) = st.frames[id].button.as_deref_mut()
            && b.enabled
        {
            b.pushed = true;
        }
    }
    fire_script(lua, id, "OnMouseDown", button);
    if button == "LeftButton" {
        crate::slider::press(lua, id);
    }
    if registered_for(lua, id, button, true) {
        click_or_double(lua, id, button, true);
    }
}

pub fn mouse_up(lua: &Lua, button: &str) {
    let press = {
        let mut st = state(lua);
        match &st.press {
            Some(p) if p.button.eq_ignore_ascii_case(button) => st.press.take(),
            _ => None,
        }
    };
    let Some(press) = press else { return };
    crate::slider::release(lua);
    let id = press.frame;
    if let Some(b) = state(lua).frames[id].button.as_deref_mut() {
        b.pushed = false;
    }
    if press.dragging {
        // After a drag, neither OnMouseUp nor OnClick fires. The drag
        // ends on the frame that started it, then the frame under the
        // mouse receives it (order inferred).
        fire_script(lua, id, "OnDragStop", ());
        update_focus(lua, true);
        let target = state(lua).mouse_focus;
        if let Some(target) = target {
            fire_script(lua, target, "OnReceiveDrag", ());
        }
        return;
    }
    let (x, y) = state(lua).mouse;
    let inside = click_test(lua, x, y) == Some(id);
    fire_script(lua, id, "OnMouseUp", (button, inside));
    if inside {
        crate::hyperlink::click(lua, id, button);
    }
    if inside && registered_for(lua, id, button, false) {
        click_or_double(lua, id, button, false);
    }
}

/// A click, or a double click if the same button clicked this frame a
/// moment ago and the frame has an OnDoubleClick handler.
fn click_or_double(lua: &Lua, id: FrameId, button: &str, down: bool) {
    let enabled = state(lua).frames[id]
        .button
        .as_deref()
        .is_none_or(|b| b.enabled);
    if !enabled {
        return;
    }
    let double = {
        let st = state(lua);
        st.frames[id].has_script("OnDoubleClick")
            && st.last_click.as_ref().is_some_and(|c| {
                c.frame == id
                    && !c.double
                    && c.button.eq_ignore_ascii_case(button)
                    && st.time - c.time <= DOUBLE_CLICK_TIME
            })
    };
    {
        let mut st = state(lua);
        let time = st.time;
        st.last_click = Some(LastClick {
            frame: id,
            button: button.to_string(),
            time,
            double,
        });
    }
    if double {
        fire_script(lua, id, "OnDoubleClick", (button, down));
    } else {
        click(lua, id, button, down);
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

/// The key without modifiers: `SHIFT-P` is `P`, `CTRL--` is `-`.
pub fn base_key(key: &str) -> &str {
    let mut rest = key;
    for m in ["SHIFT-", "CTRL-", "ALT-", "META-"] {
        loop {
            match rest.get(..m.len()) {
                Some(p) if p.eq_ignore_ascii_case(m) && rest.len() > m.len() => {
                    rest = &rest[m.len()..]
                }
                _ => break,
            }
        }
    }
    rest
}

/// A key press. The focused edit box takes every key; otherwise
/// keyboard-enabled frames get OnKeyDown from the top down until one
/// does not propagate it; otherwise the key binding runs.
pub fn key_down(lua: &Lua, key: &str) -> bool {
    let base = base_key(key).to_string();
    let focus = state(lua).focus;
    if let Some(id) = focus {
        state(lua).key_targets.insert(base.clone(), id);
        fire_script(lua, id, "OnKeyDown", base);
        edit_box_key(lua, key);
        return true;
    }
    let frames: Vec<FrameId> = {
        let st = state(lua);
        let order = frame_order(&st);
        order
            .into_iter()
            .rev()
            .filter(|&i| st.frames[i].keyboard_enabled)
            .collect()
    };
    for id in frames {
        if !is_visible(&state(lua).frames, id) {
            continue;
        }
        fire_script(lua, id, "OnKeyDown", base.clone());
        // The handler may call SetPropagateKeyboardInput for this key.
        if !state(lua).frames[id].propagate_keyboard {
            state(lua).key_targets.insert(base, id);
            return true;
        }
    }
    crate::bindings::key_down(lua, key)
}

/// A key release: OnKeyUp for the frame that took the press.
pub fn key_up(lua: &Lua, key: &str) {
    let target = state(lua).key_targets.remove(base_key(key));
    if let Some(id) = target {
        fire_script(lua, id, "OnKeyUp", base_key(key).to_string());
    }
}

/// Typed text: goes to the focused edit box, one character at a time, as
/// OnChar and OnTextChanged(userInput = true). Returns false without a
/// focused edit box.
pub fn text_input(lua: &Lua, text: &str) -> bool {
    let Some(id) = state(lua).focus else {
        return false;
    };
    crate::editbox::type_text(lua, id, text);
    true
}

/// Keys that a focused edit box handles itself. Returns true if the key
/// went to the edit box.
pub fn edit_box_key(lua: &Lua, key: &str) -> bool {
    let Some(id) = state(lua).focus else {
        return false;
    };
    crate::editbox::key(lua, id, key)
}
