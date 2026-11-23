//! Visual effects of animations that are playing or paused: alpha,
//! translation, scale, rotation and flipbook frames.
//!
//! The animation groups in `widgets.rs` only keep time. [`recompute`] turns
//! the time of each active group into effects on its targets. Layout reads
//! the offsets and scales; the scene reads alpha, rotation and flipbook
//! frames. An animation applies from the moment its order starts, and keeps
//! its end value until the group stops.

use std::collections::{BTreeMap, HashMap};

use mlua::{Table, Value};
use wowprogramming_lua::mlua;

use crate::frames::frame_id;
use crate::state::{FrameId, State};

#[derive(Clone, Debug, PartialEq)]
pub struct AnimFx {
    /// Replaces the target's own alpha.
    pub alpha: Option<f64>,
    /// Added to the target's position, in the target's units.
    pub offset: (f64, f64),
    pub scale: (f64, f64),
    pub origin: Option<(String, f64, f64)>,
    /// Added to a texture's rotation, in radians.
    pub rotation: f64,
    /// Flipbook cell as fractions of the texture coordinates: left, top,
    /// right, bottom.
    pub flipbook: Option<[f32; 4]>,
}

impl Default for AnimFx {
    fn default() -> Self {
        AnimFx {
            alpha: None,
            offset: (0.0, 0.0),
            scale: (1.0, 1.0),
            origin: None,
            rotation: 0.0,
            flipbook: None,
        }
    }
}

impl AnimFx {
    /// Whether the effect moves or resizes the target.
    fn moves(&self) -> bool {
        self.offset != (0.0, 0.0) || self.scale != (1.0, 1.0)
    }
}

/// Apply an easing curve to linear progress.
fn ease(t: f64, smoothing: &str) -> f64 {
    match smoothing {
        "IN" => t * t,
        "OUT" => 1.0 - (1.0 - t) * (1.0 - t),
        "IN_OUT" => {
            if t < 0.5 {
                2.0 * t * t
            } else {
                1.0 - 2.0 * (1.0 - t) * (1.0 - t)
            }
        }
        _ => t,
    }
}

/// The flipbook cell for a frame index.
pub fn flipbook_cell(rows: u32, cols: u32, frame: u32) -> [f32; 4] {
    let (rows, cols) = (rows.max(1), cols.max(1));
    let col = frame % cols;
    let row = (frame / cols).min(rows - 1);
    [
        col as f32 / cols as f32,
        row as f32 / rows as f32,
        (col + 1) as f32 / cols as f32,
        (row + 1) as f32 / rows as f32,
    ]
}

/// Resolve the target of an animation: an explicit target, a child or
/// target key on the group's owner, or the owner itself.
pub fn target(st: &State, anim: FrameId) -> Option<FrameId> {
    let a = st.frames[anim].anim.as_deref()?;
    if let Some(t) = a.target {
        return Some(t);
    }
    let owner = st.frames[anim].parent.and_then(|g| st.frames[g].parent)?;
    let Some(key) = a.child_key.as_ref().or(a.target_key.as_ref()) else {
        return Some(owner);
    };
    let mut current: Table = st.table(owner);
    for part in key.split('.') {
        if part.eq_ignore_ascii_case("$parent") {
            let id = frame_id(&current).ok()?;
            current = st.table(st.frames[id].parent?);
        } else {
            current = match current.get::<Value>(part).ok()? {
                Value::Table(t) => t,
                _ => return None,
            };
        }
    }
    frame_id(&current).ok()
}

/// Start time of each order within a group, and the group's total length.
fn phases(st: &State, group: FrameId) -> (BTreeMap<i64, f64>, f64) {
    let mut lengths: BTreeMap<i64, f64> = BTreeMap::new();
    for &a in &st.frames[group].anims {
        if let Some(an) = st.frames[a].anim.as_deref() {
            let d = an.start_delay + an.duration + an.end_delay;
            let e = lengths.entry(an.order).or_insert(0.0);
            *e = e.max(d);
        }
    }
    let mut starts = BTreeMap::new();
    let mut t = 0.0;
    for (order, len) in lengths {
        starts.insert(order, t);
        t += len;
    }
    (starts, t)
}

/// Recompute the effects of all active animation groups. Invalidates the
/// layout when an offset or scale changed.
pub fn recompute(st: &mut State) {
    let mut out: HashMap<FrameId, AnimFx> = HashMap::new();
    for g in 0..st.frames.len() {
        if st.frames[g].object_type != "AnimationGroup" {
            continue;
        }
        let Some(ga) = st.frames[g].anim.as_deref() else {
            continue;
        };
        if !ga.playing {
            continue;
        }
        let (elapsed, reverse) = (ga.elapsed, ga.reverse);
        let (starts, total) = phases(st, g);
        let e = if reverse {
            (total - elapsed).max(0.0)
        } else {
            elapsed.min(total)
        };
        let mut anims = st.frames[g].anims.clone();
        anims.sort_by_key(|&a| st.frames[a].anim.as_deref().map(|x| x.order).unwrap_or(0));
        for a in anims {
            let Some(an) = st.frames[a].anim.as_deref() else {
                continue;
            };
            if !an.playing {
                continue;
            }
            let start = starts.get(&an.order).copied().unwrap_or(0.0);
            if e < start {
                continue;
            }
            let local = e - start - an.start_delay;
            let t = if an.duration <= 0.0 {
                if local >= 0.0 { 1.0 } else { 0.0 }
            } else {
                (local / an.duration).clamp(0.0, 1.0)
            };
            let t = ease(t, &an.smoothing);
            let an = an.clone();
            let Some(target) = target(st, a) else {
                continue;
            };
            let fx = out.entry(target).or_default();
            match st.frames[a].object_type {
                "Alpha" => fx.alpha = Some(an.from_alpha + (an.to_alpha - an.from_alpha) * t),
                "Translation" => {
                    fx.offset.0 += an.offset.0 * t;
                    fx.offset.1 += an.offset.1 * t;
                }
                "Scale" => {
                    fx.scale.0 *= an.scale_from.0 + (an.scale_to.0 - an.scale_from.0) * t;
                    fx.scale.1 *= an.scale_from.1 + (an.scale_to.1 - an.scale_from.1) * t;
                    if an.origin.is_some() {
                        fx.origin = an.origin.clone();
                    }
                }
                "Rotation" => fx.rotation += an.radians * t,
                "FlipBook" => {
                    let (rows, cols, frames) = an.flipbook;
                    let frames = frames.max(1);
                    let frame = ((t * frames as f64).floor() as u32).min(frames - 1);
                    fx.flipbook = Some(flipbook_cell(rows, cols, frame));
                }
                _ => {}
            }
        }
    }
    type Moved = Vec<(FrameId, (f64, f64), (f64, f64))>;
    let moved = |m: &HashMap<FrameId, AnimFx>| -> Moved {
        let mut v: Vec<_> = m
            .iter()
            .filter(|(_, f)| f.moves())
            .map(|(&id, f)| (id, f.offset, f.scale))
            .collect();
        v.sort_by_key(|x| x.0);
        v
    };
    if moved(&st.anim_fx) != moved(&out) {
        crate::layout::invalidate(st);
    }
    st.anim_fx = out;
}

/// When a group finishes, flipbooks keep showing their last frame.
pub fn keep_final_flipbook_frames(st: &mut State, group: FrameId) {
    let reverse = st.frames[group].anim.as_deref().is_some_and(|a| a.reverse);
    for a in st.frames[group].anims.clone() {
        if st.frames[a].object_type != "FlipBook" {
            continue;
        }
        let Some((rows, cols, frames)) = st.frames[a].anim.as_deref().map(|x| x.flipbook) else {
            continue;
        };
        let frame = if reverse { 0 } else { frames.max(1) - 1 };
        if let Some(t) = target(st, a)
            && let Some(tex) = st.frames[t].tex.as_deref_mut()
        {
            tex.flipbook_frame = Some(flipbook_cell(rows, cols, frame));
        }
    }
}

/// The alpha of a widget itself, with animation effects.
pub fn own_alpha(st: &State, id: FrameId) -> f64 {
    st.anim_fx
        .get(&id)
        .and_then(|f| f.alpha)
        .unwrap_or(st.frames[id].alpha)
}
