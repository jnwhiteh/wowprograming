//! The renderer-neutral scene: visible regions in draw order, as textured
//! quads and positioned text lines, in screen units with y pointing up.

use crate::frames::is_visible;
use crate::gamedata::ImageKey;
use crate::layout::{self, Rect, effective_alpha, effective_scale};
use crate::state::{FrameId, State, TexFile};
use crate::text::{self, Run};
use crate::widgets::STRATA;

#[derive(Clone, Debug)]
pub struct Quad {
    pub rect: Rect,
    /// Texture coordinates of the corners: UL, LL, UR, LR as (u, v).
    pub uv: [f32; 8],
    pub color: [f32; 4],
    /// `None` draws a solid colour.
    pub image: Option<ImageKey>,
    pub additive: bool,
    pub desaturate: bool,
    pub tile: (bool, bool),
    pub clip: Option<Rect>,
    /// Mask textures: the quad's alpha is multiplied by each mask's alpha.
    pub masks: Vec<Mask>,
    pub id: FrameId,
}

/// A mask texture. Outside its rectangle the mask hides everything.
#[derive(Clone, Debug)]
pub struct Mask {
    pub rect: Rect,
    pub uv: [f32; 8],
    /// `None` is a mask without a file: it only clips to its rectangle.
    pub image: Option<ImageKey>,
}

#[derive(Clone, Debug)]
pub struct TextLine {
    /// Left edge and baseline-independent top of the line box.
    pub x: f64,
    pub y_top: f64,
    pub runs: Vec<Run>,
}

#[derive(Clone, Debug)]
pub struct TextItem {
    pub font_path: String,
    /// Font height in screen units.
    pub size: f64,
    pub color: [f32; 4],
    pub shadow: Option<([f32; 4], (f64, f64))>,
    pub outline: bool,
    pub lines: Vec<TextLine>,
    /// Width of each line in screen units, as measured by the engine.
    pub widths: Vec<f64>,
    pub clip: Option<Rect>,
    pub id: FrameId,
}

#[derive(Clone, Debug)]
pub enum Item {
    Quad(Quad),
    Text(TextItem),
}

#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub width: f64,
    pub height: f64,
    pub items: Vec<Item>,
}

fn strata_index(s: &str) -> usize {
    STRATA.iter().position(|x| *x == s).unwrap_or(3)
}

/// Visible frames in draw order (back to front).
pub fn frame_order(st: &State) -> Vec<FrameId> {
    let mut order = Vec::new();
    let roots: Vec<FrameId> = (0..st.frames.len())
        .filter(|&i| st.frames[i].parent.is_none() && st.frames[i].is_frame())
        .collect();
    let mut stack: Vec<FrameId> = roots.into_iter().rev().collect();
    while let Some(i) = stack.pop() {
        let f = &st.frames[i];
        if !f.shown {
            continue;
        }
        order.push(i);
        for &c in f.children.iter().rev() {
            stack.push(c);
        }
    }
    let position: std::collections::HashMap<FrameId, usize> =
        order.iter().enumerate().map(|(n, &i)| (i, n)).collect();
    order.sort_by_key(|&i| {
        let f = &st.frames[i];
        (strata_index(f.strata), f.level, position[&i])
    });
    order
}

/// Clip rectangle from ancestors that clip their children.
fn clip_rect(st: &mut State, id: FrameId) -> Option<Rect> {
    let mut clip: Option<Rect> = None;
    let mut current = st.frames[id].parent;
    while let Some(p) = current {
        if st.frames[p].clips_children
            && let Some(r) = layout::rect(st, p)
        {
            clip = Some(match clip {
                Some(c) => c.intersect(&r),
                None => r,
            });
        }
        current = st.frames[p].parent;
    }
    clip
}

/// Whether a region should be drawn, given its owner's button state.
fn region_active(st: &State, frame: FrameId, region: FrameId) -> bool {
    let f = &st.frames[frame];
    let r = &st.frames[region];
    let highlighted = f.mouse_over || f.button.as_deref().is_some_and(|b| b.locked_highlight);
    if r.draw_layer == 4 && !highlighted {
        return false;
    }
    let Some(b) = f.button.as_deref() else {
        return true;
    };
    let is = |slot: Option<FrameId>| slot == Some(region);
    if is(b.highlight) {
        return highlighted && b.enabled;
    }
    if is(b.checked_tex) {
        return b.checked && (b.enabled || b.disabled_checked.is_none());
    }
    if is(b.disabled_checked) {
        return b.checked && !b.enabled;
    }
    if is(b.disabled) {
        return !b.enabled;
    }
    if is(b.pushed_tex) {
        return b.enabled && b.pushed;
    }
    if is(b.normal) {
        let replaced = (!b.enabled && b.disabled.is_some())
            || (b.enabled && b.pushed && b.pushed_tex.is_some());
        return !replaced;
    }
    true
}

pub fn build(st: &mut State) -> Scene {
    let mut scene = Scene {
        width: st.screen.0,
        height: st.screen.1,
        items: Vec::new(),
    };
    crate::animfx::recompute(st);
    let order = frame_order(st);
    // Like the client, regions of frames at the same strata and level are
    // drawn layer by layer across those frames (all BACKGROUND regions,
    // then BORDER, ...). A child frame with useParentLevel, such as a
    // tooltip's NineSlice border, therefore stays behind the parent's text.
    // Within a frame, layer and sublevel, regions created later draw first:
    // the action button template relies on its icon (created first) showing
    // above the slot art.
    struct Entry {
        key: (usize, i64, u8, i64, usize, std::cmp::Reverse<FrameId>),
        frame: FrameId,
        region: Option<FrameId>,
    }
    let mut entries: Vec<Entry> = Vec::new();
    let mut frame_info: std::collections::HashMap<FrameId, (f64, Option<Rect>)> =
        std::collections::HashMap::new();
    for (pos, fid) in order.into_iter().enumerate() {
        if !is_visible(&st.frames, fid) {
            continue;
        }
        let alpha = effective_alpha(st, fid);
        if alpha <= 0.001 {
            continue;
        }
        let clip = clip_rect(st, fid);
        frame_info.insert(fid, (alpha, clip));
        let strata = strata_index(st.frames[fid].strata);
        let level = st.frames[fid].level;
        if st.frames[fid].object_type == "EditBox" {
            entries.push(Entry {
                key: (strata, level, 3, i64::MAX, pos, std::cmp::Reverse(0)),
                frame: fid,
                region: None,
            });
        }
        let regions: Vec<FrameId> = st.frames[fid]
            .regions
            .iter()
            .copied()
            .filter(|&r| st.frames[r].shown && st.frames[r].object_type != "MaskTexture")
            .filter(|&r| region_active(st, fid, r))
            .collect();
        for r in regions {
            entries.push(Entry {
                key: (
                    strata,
                    level,
                    st.frames[r].draw_layer,
                    st.frames[r].sublevel,
                    pos,
                    std::cmp::Reverse(r),
                ),
                frame: fid,
                region: Some(r),
            });
        }
    }
    entries.sort_by_key(|e| e.key);
    for e in entries {
        let (alpha, clip) = frame_info[&e.frame];
        let Some(r) = e.region else {
            edit_box_text(st, e.frame, clip, alpha, &mut scene);
            continue;
        };
        let ralpha = alpha * crate::animfx::own_alpha(st, r);
        if ralpha <= 0.001 {
            continue;
        }
        match st.frames[r].object_type {
            "Texture" => texture_quad(st, e.frame, r, clip, ralpha, &mut scene),
            "FontString" => font_string_text(st, r, clip, ralpha, &mut scene),
            _ => {}
        }
    }
    scene
}

fn texture_quad(
    st: &mut State,
    owner: FrameId,
    id: FrameId,
    clip: Option<Rect>,
    alpha: f64,
    scene: &mut Scene,
) {
    let Some(mut rect) = layout::rect(st, id) else {
        return;
    };
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let Some(tex) = st.frames[id].tex.as_deref().cloned() else {
        return;
    };
    let mut uv = tex.coords;
    // A status bar shows part of its texture.
    if st.frames[owner].status_texture == Some(id) && st.frames[owner].object_type == "StatusBar" {
        let (min, max, value) = st.frames[owner].value;
        let frac = if max > min {
            ((value - min) / (max - min)).clamp(0.0, 1.0)
        } else {
            0.0
        } as f32;
        if st.frames[owner].orientation == "VERTICAL" {
            rect.height *= frac as f64;
            uv[1] = uv[3] - (uv[3] - uv[1]) * frac;
            uv[5] = uv[7] - (uv[7] - uv[5]) * frac;
        } else {
            rect.width *= frac as f64;
            uv[4] = uv[0] + (uv[4] - uv[0]) * frac;
            uv[6] = uv[2] + (uv[6] - uv[2]) * frac;
        }
        if frac <= 0.0 {
            return;
        }
    }
    // A flipbook shows one cell of the texture.
    let fx = st.anim_fx.get(&id);
    if let Some(cell) = fx.and_then(|f| f.flipbook).or(tex.flipbook_frame) {
        uv = sub_uv(&uv, cell);
    }
    if let Some(r) = tex.atlas_rect {
        uv = atlas_map(&uv, r);
    }
    // SetRotation turns the texture coordinates around their centre.
    let rotation = tex.rotation + fx.map(|f| f.rotation as f32).unwrap_or(0.0);
    if rotation != 0.0 {
        let (s, c) = (rotation.sin(), rotation.cos());
        let cx = (uv[0] + uv[6]) / 2.0;
        let cy = (uv[1] + uv[7]) / 2.0;
        for i in 0..4 {
            let (x, y) = (uv[i * 2] - cx, uv[i * 2 + 1] - cy);
            uv[i * 2] = cx + x * c + y * s;
            uv[i * 2 + 1] = cy - x * s + y * c;
        }
    }
    let vc = tex.vertex_color;
    let (image, base) = match &tex.file {
        TexFile::None => return,
        TexFile::Color(c) => (None, *c),
        TexFile::Id(i) => (Some(ImageKey::Id(*i as u32)), [1.0; 4]),
        TexFile::Path(p) => (Some(ImageKey::Path(p.clone())), [1.0; 4]),
    };
    let color = [
        base[0] * vc[0],
        base[1] * vc[1],
        base[2] * vc[2],
        base[3] * vc[3] * alpha as f32,
    ];
    let masks = masks_of(st, id);
    scene.items.push(Item::Quad(Quad {
        rect,
        uv,
        color,
        image,
        additive: tex.blend_mode == "ADD",
        desaturate: tex.desaturation > 0.5,
        tile: (tex.horiz_tile, tex.vert_tile),
        clip,
        masks,
        id,
    }));
}

/// Map coordinates relative to an atlas region (left, top, right,
/// bottom) into the file.
fn atlas_map(uv: &[f32; 8], r: [f32; 4]) -> [f32; 8] {
    let mut out = *uv;
    for i in 0..4 {
        out[i * 2] = r[0] + uv[i * 2] * (r[2] - r[0]);
        out[i * 2 + 1] = r[1] + uv[i * 2 + 1] * (r[3] - r[1]);
    }
    out
}

/// Part of a texture coordinate quad: `cell` is left, top, right, bottom
/// as fractions.
fn sub_uv(uv: &[f32; 8], cell: [f32; 4]) -> [f32; 8] {
    let at = |fx: f32, fy: f32| {
        let top = (uv[0] + (uv[4] - uv[0]) * fx, uv[1] + (uv[5] - uv[1]) * fx);
        let bot = (uv[2] + (uv[6] - uv[2]) * fx, uv[3] + (uv[7] - uv[3]) * fx);
        (top.0 + (bot.0 - top.0) * fy, top.1 + (bot.1 - top.1) * fy)
    };
    let [l, t, r, b] = cell;
    let (a, c, e, g) = (at(l, t), at(l, b), at(r, t), at(r, b));
    [a.0, a.1, c.0, c.1, e.0, e.1, g.0, g.1]
}

fn masks_of(st: &mut State, id: FrameId) -> Vec<Mask> {
    let ids = st.frames[id].masks.clone();
    let mut out = Vec::new();
    for m in ids {
        if !st.frames[m].shown {
            continue;
        }
        let rect = layout::rect(st, m).unwrap_or_default();
        let (image, uv) = match st.frames[m].tex.as_deref() {
            Some(t) => (
                match &t.file {
                    TexFile::Id(i) => Some(ImageKey::Id(*i as u32)),
                    TexFile::Path(p) => Some(ImageKey::Path(p.clone())),
                    _ => None,
                },
                match t.atlas_rect {
                    Some(r) => atlas_map(&t.coords, r),
                    None => t.coords,
                },
            ),
            None => (None, [0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0]),
        };
        out.push(Mask { rect, uv, image });
    }
    out
}

fn push_text(
    st: &mut State,
    id: FrameId,
    rect: Rect,
    wrap: Option<f64>,
    clip: Option<Rect>,
    alpha: f64,
    scene: &mut Scene,
) {
    let es = effective_scale(st, id);
    // A font string with a fixed height shows only the lines that fit.
    let f = &st.frames[id];
    let fixed_height =
        f.object_type == "FontString" && (f.height > 0.0 || layout::has_two(&f.points, false));
    let max_height = fixed_height.then_some(rect.height / es);
    let (font, lines) = text::font_string_lines_in(st, id, wrap, max_height);
    if lines.is_empty() {
        return;
    }
    let size = font.line_height * es;
    let spacing = font.spacing * es;
    let total = lines.len() as f64 * size + (lines.len() as f64 - 1.0) * spacing;
    let top = match font.justify_v.as_str() {
        "TOP" => rect.top(),
        "BOTTOM" => rect.bottom + total,
        _ => rect.bottom + rect.height / 2.0 + total / 2.0,
    };
    let mut out = Vec::new();
    let mut widths = Vec::new();
    for (n, line) in lines.iter().enumerate() {
        let w = line.width * es;
        let x = match font.justify_h.as_str() {
            "LEFT" => rect.left,
            "RIGHT" => rect.right() - w,
            _ => rect.left + (rect.width - w) / 2.0,
        };
        out.push(TextLine {
            x,
            y_top: top - n as f64 * (size + spacing),
            runs: line.runs.clone(),
        });
        widths.push(w);
    }
    let mut color = font.color;
    color[3] *= alpha as f32;
    let shadow = (font.shadow_color[3] > 0.0
        && (font.shadow_offset.0 != 0.0 || font.shadow_offset.1 != 0.0))
        .then(|| {
            let mut c = font.shadow_color;
            c[3] *= alpha as f32;
            (c, (font.shadow_offset.0 * es, font.shadow_offset.1 * es))
        });
    scene.items.push(Item::Text(TextItem {
        font_path: font.path.clone(),
        size,
        color,
        shadow,
        outline: font.flags.contains("OUTLINE"),
        lines: out,
        widths,
        clip,
        id,
    }));
}

fn font_string_text(
    st: &mut State,
    id: FrameId,
    clip: Option<Rect>,
    alpha: f64,
    scene: &mut Scene,
) {
    let Some(rect) = layout::rect(st, id) else {
        return;
    };
    let es = effective_scale(st, id);
    let f = &st.frames[id];
    let has_width = f.width > 0.0 || layout::has_two(&f.points, true);
    let wrap = has_width.then_some(rect.width / es);
    push_text(st, id, rect, wrap, clip, alpha, scene);
}

fn edit_box_text(st: &mut State, id: FrameId, clip: Option<Rect>, alpha: f64, scene: &mut Scene) {
    let Some(mut rect) = layout::rect(st, id) else {
        return;
    };
    // Text insets shrink the text area (left, right, top, bottom).
    let es = effective_scale(st, id);
    if let Some(i) = st.frames[id].text.as_deref().map(|t| t.insets) {
        rect.left += i[0] * es;
        rect.width = (rect.width - (i[0] + i[1]) * es).max(0.0);
        rect.bottom += i[3] * es;
        rect.height = (rect.height - (i[2] + i[3]) * es).max(0.0);
    }
    let has_text = st.frames[id]
        .text
        .as_deref()
        .is_some_and(|t| !t.text.is_empty());
    if !has_text {
        return;
    }
    if let Some(f) = st.frames[id].font.as_deref_mut()
        && f.justify_h.is_none()
    {
        f.justify_h = Some("LEFT".into());
    }
    let es = effective_scale(st, id);
    push_text(
        st,
        id,
        rect,
        Some(rect.width / es),
        clip.or(Some(rect)),
        alpha,
        scene,
    );
}
