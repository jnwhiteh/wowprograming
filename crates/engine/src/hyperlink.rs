//! Hyperlinks (`|Hdata|htext|h`) in the text of frames with hyperlinks
//! enabled: OnHyperlinkEnter and OnHyperlinkLeave as the mouse moves over
//! them, OnHyperlinkClick when one is clicked. Rules and sources are in
//! docs/client-runtime.md ("Hyperlinks").

use mlua::{Lua, Value};
use wowprogramming_lua::mlua;

use crate::frames::{fire_script, is_visible};
use crate::layout::{self, Rect};
use crate::state::{FrameId, HoverLink, State, state};

/// The nearest frame at or above `id` that shows hyperlinks.
fn link_owner(st: &State, id: FrameId) -> Option<FrameId> {
    let mut current = Some(id);
    while let Some(i) = current {
        if st.frames[i].hyperlinks_enabled {
            return Some(i);
        }
        current = st.frames[i].parent;
    }
    None
}

/// Visible font strings in a frame and its child frames, and the frame
/// itself if it is a message frame.
fn text_regions(st: &State, owner: FrameId) -> Vec<FrameId> {
    let mut out = Vec::new();
    let mut stack = vec![owner];
    while let Some(f) = stack.pop() {
        if !is_visible(&st.frames, f) {
            continue;
        }
        if st.frames[f].object_type == "MessageFrame" {
            out.push(f);
        }
        out.extend(
            st.frames[f]
                .regions
                .iter()
                .copied()
                .filter(|&r| st.frames[r].object_type == "FontString" && st.frames[r].shown),
        );
        stack.extend(st.frames[f].children.iter().copied());
    }
    out
}

/// The link under a point: owner frame, text region, link data, shown
/// text and its rectangle in screen units.
fn link_at(lua: &Lua, x: f64, y: f64) -> Option<HoverLink> {
    let mut st = state(lua);
    let hit = st.mouse_focus?;
    let owner = link_owner(&st, hit)?;
    for region in text_regions(&st, owner) {
        let Some(item) = crate::scene::text_item(&mut st, region) else {
            continue;
        };
        for line in &item.lines {
            let (top, bottom) = (line.y_top, line.y_top - item.size);
            if y > top || y < bottom {
                continue;
            }
            let mut pen = line.x;
            for run in &line.runs {
                let width: f64 = run.advances.iter().sum();
                if let Some(link) = &run.link
                    && x >= pen
                    && x <= pen + width
                {
                    return Some(HoverLink {
                        owner,
                        region,
                        link: link.clone(),
                        text: run.text.clone(),
                        rect: Rect {
                            left: pen,
                            bottom,
                            width,
                            height: item.size,
                        },
                    });
                }
                pen += width;
            }
        }
    }
    None
}

/// The link's rectangle in the owner's units: left, bottom, width, height.
fn rect_args(st: &State, l: &HoverLink) -> (f64, f64, f64, f64) {
    let es = layout::effective_scale(st, l.owner);
    (
        l.rect.left / es,
        l.rect.bottom / es,
        l.rect.width / es,
        l.rect.height / es,
    )
}

/// After the mouse moved or the frames changed: fire OnHyperlinkLeave and
/// OnHyperlinkEnter if the link under the mouse changed.
pub fn update(lua: &Lua) {
    let (x, y) = state(lua).mouse;
    let now = link_at(lua, x, y);
    let old = state(lua).hover_link.clone();
    let same = match (&old, &now) {
        (Some(a), Some(b)) => a.owner == b.owner && a.link == b.link && a.rect == b.rect,
        (None, None) => true,
        _ => false,
    };
    if same {
        return;
    }
    state(lua).hover_link = now.clone();
    if let Some(o) = old {
        fire_script(lua, o.owner, "OnHyperlinkLeave", (o.link, o.text));
    }
    if let Some(n) = now {
        let (l, b, w, h) = rect_args(&state(lua), &n);
        let region = Value::Table(state(lua).table(n.region));
        fire_script(
            lua,
            n.owner,
            "OnHyperlinkEnter",
            (n.link, n.text, region, l, b, w, h),
        );
    }
}

/// A click released over a link of the pressed frame (or one of its
/// ancestors).
pub fn click(lua: &Lua, pressed: FrameId, button: &str) {
    let Some(l) = state(lua).hover_link.clone() else {
        return;
    };
    let related = {
        let st = state(lua);
        let mut current = Some(pressed);
        let mut found = false;
        while let Some(i) = current {
            if i == l.owner {
                found = true;
                break;
            }
            current = st.frames[i].parent;
        }
        found
    };
    if !related {
        return;
    }
    let (left, b, w, h) = rect_args(&state(lua), &l);
    let region = Value::Table(state(lua).table(l.region));
    fire_script(
        lua,
        l.owner,
        "OnHyperlinkClick",
        (l.link, l.text, button.to_string(), region, left, b, w, h),
    );
}
