//! Rectangle resolution from anchors, sizes and scales.
//!
//! Rectangles are computed on demand in screen units, where the screen is
//! `State::screen` and a scale of 1 maps one unit to one screen unit. The
//! Lua API divides by the region's effective scale, as the client does.
//! Results are cached until `State::layout_generation` changes.

use crate::state::{FrameId, State};
use crate::text;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
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
        alpha *= crate::animfx::own_alpha(st, i);
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
    let mut points = st.frames[id].points.clone();
    if points.is_empty() {
        // A button's text without anchors is centred on the button; a
        // status bar's texture without anchors fills the bar, and so does
        // an XML texture with neither anchors nor size.
        let parent = st.frames[id].parent?;
        let is_button_text = st.frames[parent]
            .button
            .as_deref()
            .is_some_and(|b| b.font_string == Some(id));
        let is_bar_texture = st.frames[parent].status_texture == Some(id);
        let f = &st.frames[id];
        let fills_parent = f.from_xml
            && matches!(f.object_type, "Texture" | "MaskTexture")
            && f.width <= 0.0
            && f.height <= 0.0;
        let point = |p: &'static str| crate::state::Point {
            point: p,
            relative: None,
            relative_point: p,
            x: 0.0,
            y: 0.0,
        };
        if is_button_text {
            points.push(point("CENTER"));
        } else if is_bar_texture || fills_parent {
            points.push(point("TOPLEFT"));
            points.push(point("BOTTOMRIGHT"));
        } else {
            return None;
        }
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
    let (mut own_w, mut own_h) = (f.width, f.height);
    // A texture without a size takes the size of its atlas or image.
    if matches!(f.object_type, "Texture" | "MaskTexture")
        && own_w <= 0.0
        && own_h <= 0.0
        && let Some((w, h)) = natural_size(st, id)
    {
        own_w = w;
        own_h = h;
    }
    let f = &st.frames[id];
    let mut width = own_w * es;
    let mut height = own_h * es;
    let is_text = f.object_type == "FontString";

    // Like the client, only two opposite edges set a size; an edge and a
    // centre (TOP with LEFT, for example) leave the size alone.
    let derived_width = match (left, right) {
        (Some(l), Some(r)) => Some(r - l),
        _ => None,
    };
    if let Some(w) = derived_width {
        width = w;
    }
    let derived_height = match (top, bottom) {
        (Some(t), Some(b)) => Some(t - b),
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
    let mut r = Rect {
        left: l,
        bottom: b,
        width,
        height,
    };
    if let Some(fx) = st.anim_fx.get(&id) {
        r.left += fx.offset.0 * es;
        r.bottom += fx.offset.1 * es;
        if fx.scale != (1.0, 1.0) {
            let (point, ox, oy) = fx.origin.clone().unwrap_or(("CENTER".into(), 0.0, 0.0));
            let (px, py) = r.point(&point);
            let (px, py) = (px + ox * es, py + oy * es);
            r.left = px + (r.left - px) * fx.scale.0;
            r.bottom = py + (r.bottom - py) * fx.scale.1;
            r.width *= fx.scale.0;
            r.height *= fx.scale.1;
        }
    }
    Some(r)
}

/// The size of a texture's atlas or image, in UI units.
fn natural_size(st: &State, id: FrameId) -> Option<(f64, f64)> {
    let tex = st.frames[id].tex.as_deref()?;
    let gd = st.gamedata.as_ref()?;
    if let Some(name) = &tex.atlas {
        let a = gd.atlas(name)?;
        return Some((a.width, a.height));
    }
    let key = match &tex.file {
        crate::state::TexFile::Id(i) => crate::gamedata::ImageKey::Id(*i as u32),
        crate::state::TexFile::Path(p) => crate::gamedata::ImageKey::Path(p.clone()),
        _ => return None,
    };
    let img = gd.image(&key)?;
    Some((img.width as f64, img.height as f64))
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
    // A hidden font string reports its text size, not the size its anchors
    // would give. Inferred from one capture: the spellbook's assistant
    // label is measured this way before the spellbook is shown, and its
    // frame takes the text width. Hidden frames still use their anchors
    // (UIParent is hidden while loading and must report the screen size).
    let laid_out = !is_text || crate::frames::is_visible(&st.frames, id);
    match laid_out.then(|| rect(st, id)).flatten() {
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

/// Whether the points include both opposite edges on an axis, which
/// sets the size on that axis.
pub fn has_two(points: &[crate::state::Point], horizontal: bool) -> bool {
    let parts: Vec<u8> = points
        .iter()
        .map(|p| {
            let (h, v) = point_parts(p.point);
            if horizontal { h } else { v }
        })
        .collect();
    parts.contains(&0) && parts.contains(&2)
}
