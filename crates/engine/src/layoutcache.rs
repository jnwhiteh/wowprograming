//! Positions of frames the user moved, kept in the sandbox's own
//! `WTF/Account/<account>/<realm>/<character>/layout-local.txt`, in the
//! client's format. Never written into the game install.
//!
//! Rules (warcraft.wiki.gg, StartMoving and SetUserPlaced): the client
//! restores named, movable, user-placed frames created before
//! PLAYER_LOGIN. The sandbox restores them after the addons load and
//! before the login events, and saves at reload and logout (timing
//! inferred).

use std::path::PathBuf;

use mlua::{Lua, Table, Value};
use wowprogramming_lua::mlua;

use crate::frames::frame_id;
use crate::layout;
use crate::state::{FrameId, Point, WtfPaths, state};

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub frame: String,
    pub anchor: &'static str,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub fn file_path(wtf: &WtfPaths) -> PathBuf {
    wtf.root
        .join("Account")
        .join(&wtf.account)
        .join(&wtf.realm)
        .join(&wtf.character)
        .join("layout-local.txt")
}

pub fn parse(text: &str) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if key == "Frame" {
            out.push(Entry {
                frame: value.to_string(),
                anchor: "CENTER",
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            });
            continue;
        }
        let Some(e) = out.last_mut() else { continue };
        let n = value.parse::<f64>().unwrap_or(0.0);
        match key {
            "Anchor" => e.anchor = crate::widgets::point_name(value).unwrap_or("CENTER"),
            "X" => e.x = n,
            "Y" => e.y = n,
            "W" => e.w = n,
            "H" => e.h = n,
            _ => {}
        }
    }
    out
}

pub fn format(entries: &[Entry]) -> String {
    let mut s = String::from("Version: 1\n");
    for e in entries {
        // FrameLevel is 1 in every file the client wrote that we have
        // seen; its meaning is an open question.
        s.push_str(&format!(
            "Frame: {}\nFrameLevel: 1\nAnchor: {}\nX: {}\nY: {}\nW: {}\nH: {}\n",
            e.frame,
            e.anchor,
            e.x.round() as i64,
            e.y.round() as i64,
            e.w.round() as i64,
            e.h.round() as i64
        ));
    }
    s
}

fn global_frame(lua: &Lua, name: &str) -> Option<FrameId> {
    match lua.globals().raw_get::<Value>(name).ok()? {
        Value::Table(t) => frame_id(&t).ok(),
        _ => None,
    }
}

/// Restore the saved positions of named, movable frames that exist now.
pub fn restore(lua: &Lua) {
    let wtf = state(lua).wtf.clone();
    let Ok(text) = std::fs::read_to_string(file_path(&wtf)) else {
        return;
    };
    let parent = lua
        .globals()
        .get::<Table>("UIParent")
        .ok()
        .and_then(|t| frame_id(&t).ok());
    for e in parse(&text) {
        let Some(id) = global_frame(lua, &e.frame) else {
            continue;
        };
        let mut st = state(lua);
        if !st.frames[id].is_frame() || !st.frames[id].movable {
            continue;
        }
        let f = &mut st.frames[id];
        f.points = vec![Point {
            point: e.anchor,
            relative: parent,
            relative_point: e.anchor,
            x: e.x,
            y: e.y,
        }];
        // The size comes back only for frames the user can size
        // (inferred).
        if f.resizable && e.w > 0.0 && e.h > 0.0 {
            f.width = e.w;
            f.height = e.h;
        }
        f.user_placed = true;
        layout::invalidate(&mut st);
    }
}

/// Save the positions of named, movable, user-placed frames. Entries for
/// frames that do not exist in this session are kept.
pub fn save(lua: &Lua) {
    let (wtf, glue) = {
        let st = state(lua);
        (st.wtf.clone(), st.glue)
    };
    if glue {
        return;
    }
    let path = file_path(&wtf);
    let mut entries = std::fs::read_to_string(&path)
        .map(|t| parse(&t))
        .unwrap_or_default();
    let parent = lua
        .globals()
        .get::<Table>("UIParent")
        .ok()
        .and_then(|t| frame_id(&t).ok());
    let mut st = state(lua);
    let base = parent
        .and_then(|p| layout::rect(&mut st, p))
        .unwrap_or_else(|| layout::screen_rect(&st));
    let mut changed = false;
    for id in 0..st.frames.len() {
        let f = &st.frames[id];
        if !f.is_frame() || !f.movable || !f.user_placed || f.dont_save_position {
            continue;
        }
        let Some(name) = f.name.clone() else { continue };
        let Some(r) = layout::rect(&mut st, id) else {
            continue;
        };
        let es = layout::effective_scale(&st, id);
        let anchor = crate::movement::nearest_point(&r, &base);
        let (px, py) = r.point(anchor);
        let (bx, by) = base.point(anchor);
        let entry = Entry {
            frame: name,
            anchor,
            x: (px - bx) / es,
            y: (py - by) / es,
            w: r.width / es,
            h: r.height / es,
        };
        match entries.iter_mut().find(|e| e.frame == entry.frame) {
            Some(e) => *e = entry,
            None => entries.push(entry),
        }
        changed = true;
    }
    drop(st);
    if !changed {
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&path, format(&entries)) {
        state(lua).warn(format!("cannot write {}: {e}", path.display()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_the_client_format() {
        let text = "Version: 1\nFrame: GroupLootHistoryFrame\nFrameLevel: 1\nAnchor: LEFT\n\
                    X: 127\nY: 78\nW: 261\nH: 590\n";
        let entries = parse(text);
        assert_eq!(
            entries,
            vec![Entry {
                frame: "GroupLootHistoryFrame".into(),
                anchor: "LEFT",
                x: 127.0,
                y: 78.0,
                w: 261.0,
                h: 590.0
            }]
        );
        assert_eq!(format(&entries), text);
    }
}
