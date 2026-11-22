//! Rectangle resolution from anchors, sizes and scales.
//!
//! Rectangles are computed on demand in screen units, where the screen is
//! `State::screen` and a scale of 1 maps one unit to one screen unit. The
//! Lua API divides by the region's effective scale, as the client does.
//! Results are cached until `State::layout_generation` changes.

use crate::state::{FrameId, State};
use crate::text;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub left: f64,
    pub bottom: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn right(&self) -> f64 {
        self.left + self.width
    }
    pub fn top(&self) -> f64 {
        self.bottom + self.height
    }

    /// Position of an anchor point.
    pub fn point(&self, point: &str) -> (f64, f64) {
        let (h, v) = point_parts(point);
        let x = match h {
            0 => self.left,
            1 => self.left + self.width / 2.0,
            _ => self.right(),
        };
        let y = match v {
            0 => self.top(),
            1 => self.bottom + self.height / 2.0,
            _ => self.bottom,
        };
        (x, y)
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.left && x <= self.right() && y >= self.bottom && y <= self.top()
    }

    pub fn intersect(&self, other: &Rect) -> Rect {
        let left = self.left.max(other.left);
        let bottom = self.bottom.max(other.bottom);
        let right = self.right().min(other.right());
        let top = self.top().min(other.top());
        Rect {
            left,
            bottom,
            width: (right - left).max(0.0),
            height: (top - bottom).max(0.0),
        }
    }
}

/// Horizontal part (0 left, 1 center, 2 right) and vertical part (0 top,
/// 1 center, 2 bottom) of an anchor point name.
fn point_parts(point: &str) -> (u8, u8) {
    match point {
        "TOPLEFT" => (0, 0),
        "TOP" => (1, 0),
        "TOPRIGHT" => (2, 0),
        "LEFT" => (0, 1),
        "CENTER" => (1, 1),
        "RIGHT" => (2, 1),
        "BOTTOMLEFT" => (0, 2),
        "BOTTOM" => (1, 2),
        _ => (2, 2),
    }
}

/// Product of the scales from the region up to the root.
pub fn effective_scale(st: &State, id: FrameId) -> f64 {
    let mut scale = 1.0;
    let mut current = Some(id);
    while let Some(i) = current {
        let f = &st.frames[i];
        scale *= f.scale;
        if f.ignore_parent_scale {
            break;
        }
        current = f.parent;
    }
    scale
}

/// Effective alpha: own alpha times the parents' alpha.
pub fn effective_alpha(st: &State, id: FrameId) -> f64 {
    let mut alpha = 1.0;
    let mut current = Some(id);
    while let Some(i) = current {
        let f = &st.frames[i];
        alpha *= f.alpha;
        if f.ignore_parent_alpha {
            break;
        }
        current = f.parent;
    }
    alpha
}

pub fn invalidate(st: &mut State) {
    st.layout_generation += 1;
}

/// The rectangle of a region in screen units, or `None` if it has no
/// anchors or depends on a region without a rectangle.
pub fn rect(st: &mut State, id: FrameId) -> Option<Rect> {
    let generation = st.layout_generation;
    if st.rect_cache.len() < st.frames.len() {
        st.rect_cache.resize(st.frames.len(), None);
    }
    if let Some((g, r)) = st.rect_cache[id]
        && g == generation
    {
        return r;
    }
    // Mark as in progress, so an anchor cycle resolves to no rectangle.
    st.rect_cache[id] = Some((generation, None));
    let result = compute(st, id);
    st.rect_cache[id] = Some((generation, result));
    result
}

fn compute(st: &mut State, id: FrameId) -> Option<Rect> {
    let points = st.frames[id].points.clone();
    if points.is_empty() {
        return None;
    }
    let es = effective_scale(st, id);
    let parent = st.frames[id].parent;

    let (mut left, mut right, mut cx) = (None, None, None);
    let (mut top, mut bottom, mut cy) = (None, None, None);
    for p in &points {
        let target = match p.relative.or(parent) {
            Some(t) => rect(st, t)?,
            None => screen_rect(st),
        };
        let (ax, ay) = target.point(p.relative_point);
        let (x, y) = (ax + p.x * es, ay + p.y * es);
        let (h, v) = point_parts(p.point);
        match h {
            0 => left = Some(x),
            1 => cx = Some(x),
            _ => right = Some(x),
        }
        match v {
            0 => top = Some(y),
            1 => cy = Some(y),
            _ => bottom = Some(y),
        }
    }

    let f = &st.frames[id];
    let mut width = f.width * es;
    let mut height = f.height * es;
    let is_text = f.object_type == "FontString";

    let derived_width = match (left, right, cx) {
        (Some(l), Some(r), _) => Some(r - l),
        (Some(l), None, Some(c)) => Some(2.0 * (c - l)),
        (None, Some(r), Some(c)) => Some(2.0 * (r - c)),
        _ => None,
    };
    if let Some(w) = derived_width {
        width = w;
    }
    let derived_height = match (top, bottom, cy) {
        (Some(t), Some(b), _) => Some(t - b),
        (Some(t), None, Some(c)) => Some(2.0 * (t - c)),
        (None, Some(b), Some(c)) => Some(2.0 * (c - b)),
        _ => None,
    };
    if let Some(h) = derived_height {
        height = h;
    }

    if is_text
        && (derived_width.is_none() && f.width <= 0.0
            || derived_height.is_none() && f.height <= 0.0)
    {
        let wrap_width = if derived_width.is_some() || f.width > 0.0 {
            Some(width / es)
        } else {
            None
        };
        let (tw, th) = text::string_size(st, id, wrap_width);
        if derived_width.is_none() && st.frames[id].width <= 0.0 {
            width = tw * es;
        }
        if derived_height.is_none() && st.frames[id].height <= 0.0 {
            height = th * es;
        }
    }

    let l = match (left, right, cx) {
        (Some(l), _, _) => l,
        (None, Some(r), _) => r - width,
        (None, None, Some(c)) => c - width / 2.0,
        _ => return None,
    };
    let b = match (bottom, top, cy) {
        (Some(b), _, _) => b,
        (None, Some(t), _) => t - height,
        (None, None, Some(c)) => c - height / 2.0,
        _ => return None,
    };
    Some(Rect {
        left: l,
        bottom: b,
        width,
        height,
    })
}

pub fn screen_rect(st: &State) -> Rect {
    Rect {
        left: 0.0,
        bottom: 0.0,
        width: st.screen.0,
        height: st.screen.1,
    }
}

/// Width and height as the Lua API reports them, in the region's own units.
pub fn size(st: &mut State, id: FrameId) -> (f64, f64) {
    let f = &st.frames[id];
    let has_two_h = has_two(&f.points, true);
    let has_two_v = has_two(&f.points, false);
    let is_text = f.object_type == "FontString";
    let (w, h) = (f.width, f.height);
    if !has_two_h && !has_two_v && !(is_text && (w <= 0.0 || h <= 0.0)) {
        return (w, h);
    }
    match rect(st, id) {
        Some(r) => {
            let es = effective_scale(st, id);
            (r.width / es, r.height / es)
        }
        None if is_text => {
            let wrap = if w > 0.0 { Some(w) } else { None };
            let (tw, th) = text::string_size(st, id, wrap);
            (if w > 0.0 { w } else { tw }, if h > 0.0 { h } else { th })
        }
        None => (w, h),
    }
}

fn has_two(points: &[crate::state::Point], horizontal: bool) -> bool {
    let mut parts = points.iter().map(|p| {
        let (h, v) = point_parts(p.point);
        if horizontal { h } else { v }
    });
    let Some(first) = parts.next() else {
        return false;
    };
    parts.any(|p| p != first)
}
