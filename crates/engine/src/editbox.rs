//! Edit box editing: the cursor, the highlighted (selected) text, letter
//! limits, numeric input, focus and the keys an edit box handles itself.
//! Positions are byte offsets into the UTF-8 text, as `GetCursorPosition`
//! reports them (`GetUTF8CursorPosition` counts characters). Rules and
//! sources are in docs/client-runtime.md ("Edit boxes").

use mlua::{Lua, Result, Table, Value, Variadic};
use wowprogramming_lua::mlua;

use crate::frames::{fire_script, this};
use crate::state::{EditState, FrameId, state};
use crate::widgets::{num, set_text, truthy};

fn text(lua: &Lua, id: FrameId) -> String {
    state(lua).frames[id]
        .text
        .as_deref()
        .map(|t| t.text.clone())
        .unwrap_or_default()
}

fn edit(lua: &Lua, id: FrameId) -> EditState {
    state(lua).frames[id]
        .edit
        .as_deref()
        .cloned()
        .unwrap_or_default()
}

fn with_edit<R>(lua: &Lua, id: FrameId, f: impl FnOnce(&mut EditState) -> R) -> R {
    let mut st = state(lua);
    let e = st.frames[id].edit.get_or_insert_with(Box::default);
    f(e)
}

/// The nearest character boundary at or before `pos`.
fn floor_boundary(s: &str, pos: usize) -> usize {
    let mut p = pos.min(s.len());
    while !s.is_char_boundary(p) {
        p -= 1;
    }
    p
}

/// Cut text to the edit box's letter and byte limits.
fn limit(e: &EditState, s: &str) -> String {
    let mut out = String::new();
    for (n, c) in s.chars().enumerate() {
        if e.max_letters > 0 && n >= e.max_letters {
            break;
        }
        if e.max_bytes > 0 && out.len() + c.len_utf8() > e.max_bytes {
            break;
        }
        out.push(c);
    }
    out
}

/// Fire OnCursorChanged with the cursor's position and size in the edit
/// box: `(x, y, width, height)`, y negative downwards from the top.
fn cursor_changed(lua: &Lua, id: FrameId) {
    if !state(lua).frames[id].has_script("OnCursorChanged") {
        return;
    }
    let t = text(lua, id);
    let cursor = floor_boundary(&t, edit(lua, id).cursor);
    let (x, h, lines) = {
        let mut st = state(lua);
        let (w, h) = crate::text::prefix_width(&mut st, id, &t[..cursor]);
        (w, h, t[..cursor].matches('\n').count() as f64)
    };
    fire_script(lua, id, "OnCursorChanged", (x, -lines * h, 1.0, h));
}

fn set_cursor(lua: &Lua, id: FrameId, pos: usize) {
    let t = text(lua, id);
    let pos = floor_boundary(&t, pos);
    let moved = with_edit(lua, id, |e| {
        let moved = e.cursor != pos;
        e.cursor = pos;
        moved
    });
    if moved {
        cursor_changed(lua, id);
    }
}

/// `SetText`: replaces the text, puts the cursor at the end and clears
/// the highlight. OnTextSet fires at once; OnTextChanged(false) follows on
/// the next frame if the text then differs from the last reported text
/// (inferred: Blizzard's currency transfer box calls SetNumber in OnLoad,
/// and its handler would fail if OnTextChanged fired inside the call).
pub fn set_text_api(lua: &Lua, id: FrameId, s: String) {
    let s = limit(&edit(lua, id), &s);
    let len = s.len();
    set_text(lua, id, s);
    with_edit(lua, id, |e| {
        e.cursor = len;
        e.highlight = None;
        e.text_change_pending = true;
    });
    fire_script(lua, id, "OnTextSet", ());
}

/// Fire the OnTextChanged(false) calls due from `SetText`. Setting the
/// same text again does not count as a change (handlers that set the text
/// from OnTextChanged would loop otherwise).
pub fn fire_pending_text_changes(lua: &Lua) {
    let due: Vec<FrameId> = {
        let mut st = state(lua);
        let mut out = Vec::new();
        for id in 0..st.frames.len() {
            let f = &mut st.frames[id];
            let Some(e) = f.edit.as_deref_mut() else {
                continue;
            };
            if !e.text_change_pending {
                continue;
            }
            e.text_change_pending = false;
            let current = f.text.as_deref().map(|t| t.text.as_str()).unwrap_or("");
            if e.reported_text != current {
                e.reported_text = current.to_string();
                out.push(id);
            }
        }
        out
    };
    for id in due {
        fire_script(lua, id, "OnTextChanged", false);
    }
}

/// OnTextChanged at once, for changes the user made.
fn text_changed_now(lua: &Lua, id: FrameId, user_input: bool) {
    let t = text(lua, id);
    with_edit(lua, id, |e| {
        e.reported_text = t;
        e.text_change_pending = false;
    });
    fire_script(lua, id, "OnTextChanged", user_input);
}

/// Insert text at the cursor, replacing the highlighted text. Returns
/// false if nothing could be inserted.
pub fn insert(lua: &Lua, id: FrameId, s: &str, user_input: bool) -> bool {
    let e = edit(lua, id);
    let s: String = if e.numeric {
        s.chars().filter(|c| c.is_ascii_digit()).collect()
    } else {
        s.to_string()
    };
    let t = text(lua, id);
    let (start, end) = match e.highlight {
        Some((a, b)) => (floor_boundary(&t, a.min(b)), floor_boundary(&t, a.max(b))),
        None => {
            let c = floor_boundary(&t, e.cursor);
            (c, c)
        }
    };
    let before = &t[..start];
    let after = &t[end..];
    // Room left under the limits, without the replaced part.
    let room_letters = if e.max_letters > 0 {
        e.max_letters
            .saturating_sub(before.chars().count() + after.chars().count())
    } else {
        usize::MAX
    };
    let room_bytes = if e.max_bytes > 0 {
        e.max_bytes.saturating_sub(before.len() + after.len())
    } else {
        usize::MAX
    };
    let mut add = String::new();
    for c in s.chars() {
        if add.chars().count() >= room_letters || add.len() + c.len_utf8() > room_bytes {
            break;
        }
        add.push(c);
    }
    if add.is_empty() && start == end {
        return false;
    }
    let new = format!("{before}{add}{after}");
    let cursor = start + add.len();
    set_text(lua, id, new);
    with_edit(lua, id, |e| {
        e.cursor = cursor;
        e.highlight = None;
    });
    if user_input {
        text_changed_now(lua, id, true);
    } else {
        with_edit(lua, id, |e| e.text_change_pending = true);
    }
    cursor_changed(lua, id);
    !add.is_empty()
}

/// Typed text: each character goes in at the cursor, then OnChar and
/// OnTextChanged fire.
pub fn type_text(lua: &Lua, id: FrameId, s: &str) {
    for c in s.chars() {
        let mut buf = [0u8; 4];
        let cs = c.encode_utf8(&mut buf);
        if insert(lua, id, cs, true) {
            fire_script(lua, id, "OnChar", cs.to_string());
        }
    }
}

/// Delete the highlighted text, or one character before (`back`) or
/// after the cursor.
fn delete(lua: &Lua, id: FrameId, back: bool) {
    let e = edit(lua, id);
    let t = text(lua, id);
    let (start, end) = match e.highlight {
        Some((a, b)) if a != b => (floor_boundary(&t, a.min(b)), floor_boundary(&t, a.max(b))),
        _ => {
            let c = floor_boundary(&t, e.cursor);
            if back {
                let prev = t[..c].chars().next_back().map_or(c, |ch| c - ch.len_utf8());
                (prev, c)
            } else {
                let next = t[c..].chars().next().map_or(c, |ch| c + ch.len_utf8());
                (c, next)
            }
        }
    };
    with_edit(lua, id, |e| e.highlight = None);
    if start == end {
        return;
    }
    let new = format!("{}{}", &t[..start], &t[end..]);
    set_text(lua, id, new);
    with_edit(lua, id, |e| e.cursor = start);
    text_changed_now(lua, id, true);
    cursor_changed(lua, id);
}

/// A key pressed while the edit box has the focus. Returns true if the
/// edit box handled it.
pub fn key(lua: &Lua, id: FrameId, key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    let shift = upper.starts_with("SHIFT-");
    let base = crate::input::base_key(&upper).to_string();
    let t = text(lua, id);
    let e = edit(lua, id);
    let cursor = floor_boundary(&t, e.cursor);
    // With Shift, arrow keys grow the highlight from its anchor.
    let move_to = |pos: usize| {
        let pos = floor_boundary(&t, pos);
        if shift {
            let anchor = match e.highlight {
                Some((a, b)) if b == cursor => a,
                Some((a, b)) if a == cursor => b,
                _ => cursor,
            };
            with_edit(lua, id, |e| e.highlight = Some((anchor, pos)));
        } else {
            with_edit(lua, id, |e| e.highlight = None);
        }
        set_cursor(lua, id, pos);
    };
    match base.as_str() {
        "BACKSPACE" => delete(lua, id, true),
        "DELETE" => delete(lua, id, false),
        "LEFT" => {
            let p = t[..cursor]
                .chars()
                .next_back()
                .map_or(cursor, |c| cursor - c.len_utf8());
            move_to(p);
        }
        "RIGHT" => {
            let p = t[cursor..]
                .chars()
                .next()
                .map_or(cursor, |c| cursor + c.len_utf8());
            move_to(p);
        }
        "HOME" => {
            let p = if e.multi_line {
                t[..cursor].rfind('\n').map_or(0, |i| i + 1)
            } else {
                0
            };
            move_to(p);
        }
        "END" => {
            let p = if e.multi_line {
                t[cursor..].find('\n').map_or(t.len(), |i| cursor + i)
            } else {
                t.len()
            };
            move_to(p);
        }
        "ENTER" => {
            // A multi-line edit box without an OnEnterPressed handler
            // takes the new line (inferred).
            let has = state(lua).frames[id].has_script("OnEnterPressed");
            if e.multi_line && !has {
                insert(lua, id, "\n", true);
            } else {
                fire_script(lua, id, "OnEnterPressed", ());
            }
        }
        "ESCAPE" => fire_script(lua, id, "OnEscapePressed", ()),
        "TAB" => fire_script(lua, id, "OnTabPressed", ()),
        "SPACE" => fire_script(lua, id, "OnSpacePressed", ()),
        _ => return false,
    }
    true
}

/// Give an edit box the keyboard focus (`SetFocus`, a click, autoFocus).
pub fn set_focus(lua: &Lua, id: FrameId) {
    let old = state(lua).focus.replace(id);
    if let Some(old) = old.filter(|&o| o != id) {
        fire_script(lua, old, "OnEditFocusLost", ());
    }
    if old != Some(id) {
        fire_script(lua, id, "OnEditFocusGained", ());
    }
}

/// An edit box became visible: with autoFocus, it takes the focus.
pub fn shown(lua: &Lua, id: FrameId) {
    let auto = {
        let st = state(lua);
        let f = &st.frames[id];
        f.object_type == "EditBox"
            && f.is_enabled()
            && f.edit.as_deref().is_none_or(|e| e.auto_focus)
    };
    if auto {
        set_focus(lua, id);
    }
}

fn id_of(args: &[Value]) -> Result<FrameId> {
    this(args.first().unwrap_or(&Value::Nil))
}

/// The edit box methods for editing, added to the EditBox method group.
pub fn methods(lua: &Lua, m: &Table) -> Result<()> {
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
                with_edit(lua, id, |e| e.$field = v);
                Ok(())
            });
            method!($get, |lua, this_: Value| {
                let id = this(&this_)?;
                Ok(edit(lua, id).$field)
            });
        };
    }
    flag!("SetAutoFocus", "IsAutoFocus", auto_focus);
    flag!("SetMultiLine", "IsMultiLine", multi_line);
    flag!("SetNumeric", "IsNumeric", numeric);
    flag!(
        "SetAltArrowKeyMode",
        "GetAltArrowKeyMode",
        alt_arrow_key_mode
    );
    flag!(
        "SetCountInvisibleLetters",
        "IsCountInvisibleLetters",
        count_invisible_letters
    );
    method!("SetText", |lua, (this_, t): (Value, Value)| {
        let id = this(&this_)?;
        let s = crate::widgets::lua_tostring(lua, &t)?;
        set_text_api(lua, id, s);
        Ok(())
    });
    method!("SetNumber", |lua, (this_, n): (Value, Value)| {
        let id = this(&this_)?;
        let s = crate::widgets::lua_tostring(lua, &n)?;
        set_text_api(lua, id, s);
        Ok(())
    });
    method!("Insert", |lua, (this_, t): (Value, Value)| {
        let id = this(&this_)?;
        let s = crate::widgets::lua_tostring(lua, &t)?;
        insert(lua, id, &s, false);
        Ok(())
    });
    method!("HasText", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(!text(lua, id).is_empty())
    });
    method!("SetMaxLetters", |lua, args: Variadic<Value>| {
        let id = id_of(&args)?;
        let n = num(args.get(1)).max(0.0) as usize;
        with_edit(lua, id, |e| e.max_letters = n);
        // Text over the new limit is cut (inferred).
        let t = text(lua, id);
        let cut = limit(&edit(lua, id), &t);
        if cut != t {
            set_text_api(lua, id, cut);
        }
        Ok(())
    });
    method!("GetMaxLetters", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(edit(lua, id).max_letters)
    });
    method!("SetMaxBytes", |lua, args: Variadic<Value>| {
        let id = id_of(&args)?;
        let n = num(args.get(1)).max(0.0) as usize;
        with_edit(lua, id, |e| e.max_bytes = n);
        Ok(())
    });
    method!("GetMaxBytes", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(edit(lua, id).max_bytes)
    });
    method!("GetCursorPosition", |lua, this_: Value| {
        let id = this(&this_)?;
        let t = text(lua, id);
        Ok(floor_boundary(&t, edit(lua, id).cursor))
    });
    method!("GetUTF8CursorPosition", |lua, this_: Value| {
        let id = this(&this_)?;
        let t = text(lua, id);
        let c = floor_boundary(&t, edit(lua, id).cursor);
        Ok(t[..c].chars().count())
    });
    method!("SetCursorPosition", |lua, args: Variadic<Value>| {
        let id = id_of(&args)?;
        let pos = num(args.get(1)).max(0.0) as usize;
        // Moving the cursor ends the highlight (inferred).
        with_edit(lua, id, |e| e.highlight = None);
        set_cursor(lua, id, pos);
        Ok(())
    });
    method!("HighlightText", |lua, args: Variadic<Value>| {
        let id = id_of(&args)?;
        let len = text(lua, id).len();
        // No arguments: everything. A negative end means the end.
        let start = match args.get(1) {
            Some(v) if !v.is_nil() => num(Some(v)).max(0.0) as usize,
            _ => 0,
        };
        let end = match args.get(2) {
            Some(v) if !v.is_nil() && num(Some(v)) >= 0.0 => num(Some(v)) as usize,
            _ => len,
        };
        let (start, end) = (start.min(len), end.min(len));
        with_edit(lua, id, |e| {
            e.highlight = (start != end).then_some((start, end));
        });
        Ok(())
    });
    method!("ClearHighlightText", |lua, this_: Value| {
        let id = this(&this_)?;
        with_edit(lua, id, |e| e.highlight = None);
        Ok(())
    });
    method!("GetHighlightText", |lua, this_: Value| {
        let id = this(&this_)?;
        let t = text(lua, id);
        Ok(match edit(lua, id).highlight {
            Some((a, b)) => {
                let (a, b) = (floor_boundary(&t, a.min(b)), floor_boundary(&t, a.max(b)));
                t[a..b].to_string()
            }
            None => String::new(),
        })
    });
    method!("SetHistoryLines", |lua, args: Variadic<Value>| {
        let id = id_of(&args)?;
        let n = num(args.get(1)).max(0.0) as usize;
        with_edit(lua, id, |e| e.history_lines = n);
        Ok(())
    });
    method!("GetHistoryLines", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(edit(lua, id).history_lines)
    });
    method!("AddHistoryLine", |lua, (this_, line): (Value, String)| {
        let id = this(&this_)?;
        with_edit(lua, id, |e| {
            e.history.push(line);
            if e.history_lines > 0 && e.history.len() > e.history_lines {
                e.history.remove(0);
            }
        });
        Ok(())
    });
    method!("ClearHistory", |lua, this_: Value| {
        let id = this(&this_)?;
        with_edit(lua, id, |e| e.history.clear());
        Ok(())
    });
    method!("GetNumLines", |lua, this_: Value| {
        let id = this(&this_)?;
        let t = text(lua, id);
        Ok(t.matches('\n').count() + 1)
    });
    method!("SetFocus", |lua, this_: Value| {
        let id = this(&this_)?;
        set_focus(lua, id);
        Ok(())
    });
    Ok(())
}
