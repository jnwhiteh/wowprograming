//! Moving and sizing frames with the mouse, clamping to the screen, and
//! raising frames. The rules and their sources are in
//! `docs/client-runtime.md` ("Moving and sizing", "Clamping", "Raising").

use mlua::{Lua, Result, Table};
use wowprogramming_lua::mlua;

use crate::frames::{frame_id, is_visible};
use crate::layout::{self, Rect, effective_scale};
use crate::state::{FrameId, Moving, Point, State, state};
use crate::widgets::POINTS;

/// UIParent's id, if it exists.
fn ui_parent(lua: &Lua) -> Option<FrameId> {
    let t: Table = lua.globals().get("UIParent").ok()?;
    frame_id(&t).ok()
}

/// `StartMoving` and `StartSizing`. The frame follows the mouse from
/// where the held button was pressed, so it does not lag behind after
/// the drag threshold; with `from_mouse` (`alwaysStartFromMouse`) it
/// follows from the current position (inferred).
pub fn start(lua: &Lua, id: FrameId, sizing: Option<&'static str>, from_mouse: bool) -> Result<()> {
    let mut st = state(lua);
    if sizing.is_none() && !st.frames[id].movable {
        return Err(mlua::Error::runtime(
            "Frame:StartMoving(): Frame is not movable",
        ));
    }
    if sizing.is_some() && !st.frames[id].resizable {
        return Err(mlua::Error::runtime(
            "Frame:StartSizing(): Frame is not resizable",
        ));
    }
    let Some(rect) = layout::rect(&mut st, id) else {
        return Ok(());
    };
    st.frames[id].user_placed = true;
    let mouse = match &st.press {
        Some(p) if !from_mouse => p.at,
        _ => st.mouse,
    };
    st.moving = Some(Moving {
        frame: id,
        sizing,
        mouse,
        rect,
    });
    drop(st);
    follow_mouse(lua);
    Ok(())
}

/// Move or size the frame that follows the mouse.
pub fn follow_mouse(lua: &Lua) {
    let Some(m) = state(lua).moving.clone() else {
        return;
    };
    let parent = ui_parent(lua);
    let mut st = state(lua);
    let (dx, dy) = (st.mouse.0 - m.mouse.0, st.mouse.1 - m.mouse.1);
    let mut r = m.rect;
    match m.sizing {
        None => {
            r.left += dx;
            r.bottom += dy;
        }
        Some(point) => {
            let es = effective_scale(&st, m.frame);
            let [min_w, min_h, max_w, max_h] = st.frames[m.frame].resize_bounds;
            let bound = |v: f64, min: f64, max: f64| {
                let v = v.max(min * es).max(0.0);
                if max > 0.0 { v.min(max * es) } else { v }
            };
            let (right, top) = (m.rect.right(), m.rect.top());
            if point.contains("LEFT") {
                r.width = bound(m.rect.width - dx, min_w, max_w);
                r.left = right - r.width;
            } else if point.contains("RIGHT") {
                r.width = bound(m.rect.width + dx, min_w, max_w);
            }
            if point.contains("BOTTOM") {
                r.height = bound(m.rect.height - dy, min_h, max_h);
                r.bottom = top - r.height;
            } else if point.contains("TOP") {
                r.height = bound(m.rect.height + dy, min_h, max_h);
            }
        }
    }
    let r = clamp_rect(&st, m.frame, r);
    anchor_to(&mut st, m.frame, parent, "TOPLEFT", r);
    if m.sizing.is_some() {
        let es = effective_scale(&st, m.frame);
        st.frames[m.frame].width = r.width / es;
        st.frames[m.frame].height = r.height / es;
    }
    layout::invalidate(&mut st);
}

/// Anchor a frame by one point to the same point of UIParent (or the
/// screen), so that it covers `r`.
fn anchor_to(st: &mut State, id: FrameId, parent: Option<FrameId>, point: &'static str, r: Rect) {
    let base = parent
        .and_then(|p| layout::rect(st, p))
        .unwrap_or_else(|| layout::screen_rect(st));
    let es = effective_scale(st, id);
    let (px, py) = r.point(point);
    let (bx, by) = base.point(point);
    let f = &mut st.frames[id];
    f.points = vec![Point {
        point,
        relative: parent,
        relative_point: point,
        x: (px - bx) / es,
        y: (py - by) / es,
    }];
    f.width = r.width / es;
    f.height = r.height / es;
}

/// `StopMovingOrSizing`: anchor the frame by its point nearest to the
/// matching UIParent point.
pub fn stop(lua: &Lua, id: FrameId) {
    let moving = state(lua).moving.as_ref().is_some_and(|m| m.frame == id);
    if !moving {
        return;
    }
    state(lua).moving = None;
    let parent = ui_parent(lua);
    let mut st = state(lua);
    let Some(r) = layout::rect(&mut st, id) else {
        return;
    };
    let base = parent
        .and_then(|p| layout::rect(&mut st, p))
        .unwrap_or_else(|| layout::screen_rect(&st));
    let point = nearest_point(&r, &base);
    anchor_to(&mut st, id, parent, point, r);
    layout::invalidate(&mut st);
}

/// The point of `r` closest to the same point of `base`.
pub fn nearest_point(r: &Rect, base: &Rect) -> &'static str {
    let mut best = ("CENTER", f64::MAX);
    for p in POINTS {
        let (x, y) = r.point(p);
        let (bx, by) = base.point(p);
        let d = (x - bx).powi(2) + (y - by).powi(2);
        if d < best.1 {
            best = (p, d);
        }
    }
    best.0
}

/// Keep a clamped frame's rectangle, grown or shrunk by its clamp insets,
/// on the screen. Insets move the edges in screen directions (positive
/// right and up), as Blizzard code uses them.
pub fn clamp_rect(st: &State, id: FrameId, mut r: Rect) -> Rect {
    let f = &st.frames[id];
    if !f.clamped {
        return r;
    }
    let es = effective_scale(st, id);
    let [il, ir, it, ib] = f.clamp_insets.map(|v| v * es);
    let screen = layout::screen_rect(st);
    let (left, right) = (r.left + il, r.right() + ir);
    let (bottom, top) = (r.bottom + ib, r.top() + it);
    if left < screen.left {
        r.left += screen.left - left;
    } else if right > screen.right() {
        r.left -= (right - screen.right()).min(left - screen.left);
    }
    if top > screen.top() {
        r.bottom -= top - screen.top();
    } else if bottom < screen.bottom {
        r.bottom += (screen.bottom - bottom).min(screen.top() - top);
    }
    r
}

/// The frames in the subtree of `id`.
fn subtree(st: &State, id: FrameId) -> Vec<FrameId> {
    let mut out = Vec::new();
    let mut stack = vec![id];
    while let Some(i) = stack.pop() {
        out.push(i);
        stack.extend(st.frames[i].children.iter().copied());
    }
    out
}

/// `Raise`: put the frame above every other visible frame in its strata.
/// Its children keep their levels relative to it.
pub fn raise(lua: &Lua, id: FrameId) {
    let mut st = state(lua);
    let strata = st.frames[id].strata;
    let tree = subtree(&st, id);
    let top = (0..st.frames.len())
        .filter(|&i| st.frames[i].is_frame() && st.frames[i].strata == strata)
        .filter(|i| !tree.contains(i))
        .filter(|&i| is_visible(&st.frames, i))
        .map(|i| st.frames[i].level)
        .max()
        .unwrap_or(0);
    let level = st.frames[id].level;
    if level > top {
        return;
    }
    let delta = top + 1 - level;
    for i in tree {
        st.frames[i].level += delta;
    }
    layout::invalidate(&mut st);
}

/// `Lower`: put the frame below every other visible frame in its strata.
pub fn lower(lua: &Lua, id: FrameId) {
    let mut st = state(lua);
    let strata = st.frames[id].strata;
    let tree = subtree(&st, id);
    let bottom = (0..st.frames.len())
        .filter(|&i| st.frames[i].is_frame() && st.frames[i].strata == strata)
        .filter(|i| !tree.contains(i))
        .filter(|&i| is_visible(&st.frames, i))
        .map(|i| st.frames[i].level)
        .min()
        .unwrap_or(0);
    let level = st.frames[id].level;
    if level < bottom {
        return;
    }
    let delta = (level - (bottom - 1).max(0)).min(level);
    for i in tree {
        st.frames[i].level = (st.frames[i].level - delta).max(0);
    }
    layout::invalidate(&mut st);
}

/// Raise the nearest toplevel frame at or above `id` (after a click).
pub fn raise_toplevel(lua: &Lua, id: FrameId) {
    let target = {
        let st = state(lua);
        let mut current = Some(id);
        let mut found = None;
        while let Some(i) = current {
            if st.frames[i].toplevel {
                found = Some(i);
                break;
            }
            current = st.frames[i].parent;
        }
        found
    };
    if let Some(t) = target {
        raise(lua, t);
    }
}
