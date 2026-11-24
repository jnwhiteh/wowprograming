//! A small 3D view of the world behind the UI: game models (M2) on a
//! textured ground, drawn by a software rasterizer.
//!
//! The client renders the world on the GPU from map files (terrain,
//! buildings, doodads). This module draws a simple stand-in scene with the
//! client's own models and textures: a ground plane tiled with a terrain
//! texture, trees and rocks, creatures, and the player's race model.
//! Models are drawn in their bind pose (no animation yet). Character skin
//! and hair textures are composed from many layers in the client; here
//! they are solid colors.
//!
//! Coordinates follow the client: x north, y west, z up, in yards.

use std::collections::HashMap;
use std::rc::Rc;

use crate::gamedata::{GameData, ImageKey};
use crate::loading::Rgba;
use wowprogramming_formats::blp::Image;
use wowprogramming_formats::{db2, m2};

const CREATURE_DISPLAY_INFO_DB2: u32 = 1108759;
const CREATURE_MODEL_DATA_DB2: u32 = 1365368;
const CHR_MODEL_DB2: u32 = 3384313;
const CHR_RACE_X_CHR_MODEL_DB2: u32 = 3490304;

/// Where a batch gets its color from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Paint {
    Texture(u32),
    Color([u8; 4]),
}

#[derive(Debug, Clone)]
pub struct DrawBatch {
    /// Triangle corners, indices into the mesh's vertices.
    pub indices: Vec<u32>,
    pub paint: Paint,
    /// 0 opaque, 1 alpha key, 2+ blended.
    pub blend: u16,
    pub two_sided: bool,
}

#[derive(Debug, Clone)]
pub struct Mesh {
    pub vertices: Vec<m2::Vertex>,
    pub batches: Vec<DrawBatch>,
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
}

/// Which character geosets to show: group 0, and the first variant of the
/// other groups (the default choice of each customization), except the
/// tabard (12) and cape (15), which come from equipment.
pub fn default_character_geoset(id: u16) -> bool {
    let (group, variant) = (id / 100, id % 100);
    match group {
        0 => true,
        12 | 15 => false,
        // Ears: variant 2 is the plain ear.
        7 => variant == 2,
        _ => variant == 1,
    }
}

fn table(gd: &GameData, id: u32) -> Option<db2::Table> {
    db2::Table::parse(&gd.read_file(id)?).ok()
}

fn value(r: &db2::Record, i: usize) -> u32 {
    r.values
        .get(i)
        .and_then(|v| v.first())
        .copied()
        .unwrap_or(0)
}

/// Load a model with its first skin. `replace` gives the paint of
/// replaceable textures by kind (1 skin, 6 hair, 11..13 creature skins);
/// `geoset` selects the sections to draw (all when `None`).
pub fn load_model(
    gd: &GameData,
    file: u32,
    replace: &HashMap<u32, Paint>,
    geoset: Option<fn(u16) -> bool>,
) -> Option<Mesh> {
    let data = gd.read_file(file)?;
    let mut model = m2::parse(&data).ok()?;
    // Pose the model in the first frame of Stand.
    let skeleton = (model.skeleton_file != 0)
        .then(|| gd.read_file(model.skeleton_file))
        .flatten();
    if let Some(pose) = m2::stand_pose(&data, skeleton.as_deref()) {
        for v in &mut model.vertices {
            let total: u32 = v.bone_weights.iter().map(|&w| w as u32).sum();
            if total == 0 {
                continue;
            }
            let (mut p, mut n) = ([0.0f32; 3], [0.0f32; 3]);
            for k in 0..4 {
                let w = v.bone_weights[k] as f32 / total as f32;
                if w == 0.0 {
                    continue;
                }
                let Some(m) = pose.get(v.bone_indices[k] as usize) else {
                    continue;
                };
                let tp = m2::transform(m, v.pos, 1.0);
                let tn = m2::transform(m, v.normal, 0.0);
                for c in 0..3 {
                    p[c] += tp[c] * w;
                    n[c] += tn[c] * w;
                }
            }
            v.pos = p;
            v.normal = n;
        }
    }
    let skin_file = *model.skin_files.first()?;
    let skin = m2::parse_skin(&gd.read_file(skin_file)?).ok()?;
    let mut batches = Vec::new();
    for b in &skin.batches {
        let Some(section) = skin.sections.get(b.section as usize) else {
            continue;
        };
        if let Some(filter) = geoset
            && !filter(section.id)
        {
            continue;
        }
        let tex_index = model
            .texture_combos
            .get(b.texture_combo as usize)
            .copied()
            .unwrap_or(0) as usize;
        let paint = match model.textures.get(tex_index) {
            Some(t) if t.kind == 0 && t.file != 0 => Paint::Texture(t.file),
            Some(t) => match replace.get(&t.kind) {
                Some(p) => *p,
                None => Paint::Color([150, 150, 150, 255]),
            },
            None => Paint::Color([150, 150, 150, 255]),
        };
        let material = model
            .materials
            .get(b.material as usize)
            .copied()
            .unwrap_or_default();
        let start = section.index_start as usize;
        let end = (start + section.index_count as usize).min(skin.indices.len());
        batches.push(DrawBatch {
            indices: skin.indices[start..end].to_vec(),
            paint,
            blend: material.blend,
            two_sided: material.flags & 0x4 != 0,
        });
    }
    Some(Mesh {
        vertices: model.vertices,
        batches,
        bounds_min: model.bounds_min,
        bounds_max: model.bounds_max,
    })
}

/// Model file and creature skin textures of a creature display
/// (CreatureDisplayInfo ID).
pub fn creature_display(gd: &GameData, display_id: u32) -> Option<(u32, [u32; 3])> {
    let displays = table(gd, CREATURE_DISPLAY_INFO_DB2)?;
    let d = displays.records.iter().find(|r| r.id == display_id)?;
    let model_id = value(d, 1);
    let textures = d.values.get(27).cloned().unwrap_or_default();
    let models = table(gd, CREATURE_MODEL_DATA_DB2)?;
    let m = models.records.iter().find(|r| r.id == model_id)?;
    // Fields (the ID is not inline): GeoBox, Flags, FileDataID, ...
    let file = value(m, 2);
    let t = |i: usize| textures.get(i).copied().unwrap_or(0);
    Some((file, [t(0), t(1), t(2)]))
}

/// The first creature display that uses a model file, with its textures.
pub fn display_for_model(gd: &GameData, model_file: u32) -> Option<(u32, [u32; 3])> {
    let models = table(gd, CREATURE_MODEL_DATA_DB2)?;
    let model_ids: Vec<u32> = models
        .records
        .iter()
        .filter(|r| value(r, 2) == model_file)
        .map(|r| r.id)
        .collect();
    let displays = table(gd, CREATURE_DISPLAY_INFO_DB2)?;
    let d = displays.records.iter().find(|r| {
        model_ids.contains(&value(r, 1)) && r.values.get(27).is_some_and(|t| t[0] != 0)
    })?;
    creature_display(gd, d.id)
}

/// The model file of a playable race and sex (ChrRaces ID; sex 0 male, 1
/// female), through ChrRaceXChrModel, ChrModel and the creature display.
pub fn race_model(gd: &GameData, race_id: u32, sex: u32) -> Option<u32> {
    let links = table(gd, CHR_RACE_X_CHR_MODEL_DB2)?;
    let link = links
        .records
        .iter()
        .find(|r| value(r, 0) == race_id && value(r, 2) == sex)?;
    let chr_model_id = value(link, 1);
    let models = table(gd, CHR_MODEL_DB2)?;
    let m = models.records.iter().find(|r| r.id == chr_model_id)?;
    // Fields: face offset, customize offset, ID, sex, DisplayID, ...
    let display = Some(value(m, 4)).filter(|&v| v != 0)?;
    creature_display(gd, display).map(|(file, _)| file)
}

#[derive(Debug, Clone)]
pub struct Instance {
    pub mesh: Rc<Mesh>,
    pub pos: [f32; 3],
    /// Rotation around z, in radians.
    pub yaw: f32,
    pub scale: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub pos: [f32; 3],
    pub target: [f32; 3],
    /// Vertical field of view, in radians.
    pub fov: f32,
}

#[derive(Debug, Clone)]
pub struct World {
    pub instances: Vec<Instance>,
    pub ground: Option<u32>,
    /// Yards per repeat of the ground texture.
    pub ground_tile: f32,
    pub ground_size: f32,
    pub sky_top: [f32; 3],
    pub sky_horizon: [f32; 3],
    pub fog: [f32; 3],
    pub fog_start: f32,
    pub fog_end: f32,
    /// Direction towards the sun.
    pub sun: [f32; 3],
    pub ambient: f32,
    pub camera: Camera,
}

impl Default for World {
    fn default() -> World {
        World {
            instances: Vec::new(),
            ground: None,
            ground_tile: 8.0,
            ground_size: 400.0,
            sky_top: [0.33, 0.52, 0.80],
            sky_horizon: [0.72, 0.80, 0.88],
            fog: [0.70, 0.78, 0.86],
            fog_start: 40.0,
            fog_end: 220.0,
            sun: normalize([0.4, 0.3, 0.85]),
            ambient: 0.45,
            camera: Camera {
                pos: [-12.0, 0.0, 5.0],
                target: [0.0, 0.0, 1.5],
                fov: 0.9,
            },
        }
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn normalize(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt().max(1e-6);
    [a[0] / l, a[1] / l, a[2] / l]
}

/// A vertex in view space with its attributes.
#[derive(Clone, Copy)]
struct ViewVertex {
    /// View space: x right, y up, z forward (distance).
    p: [f32; 3],
    uv: [f32; 2],
    light: f32,
}

/// A triangle ready to rasterize.
struct ScreenTri {
    /// Screen x, y, and 1/z for each corner.
    s: [[f32; 3]; 3],
    /// uv/z and light/z, z for fog.
    uv: [[f32; 2]; 3],
    light: [f32; 3],
    depth: [f32; 3],
    paint: usize,
    blend: u16,
}

struct Projector {
    right: [f32; 3],
    up: [f32; 3],
    forward: [f32; 3],
    eye: [f32; 3],
    focal: f32,
    cx: f32,
    cy: f32,
}

const NEAR: f32 = 0.1;

impl Projector {
    fn view(&self, p: [f32; 3]) -> [f32; 3] {
        let d = sub(p, self.eye);
        [dot(d, self.right), dot(d, self.up), dot(d, self.forward)]
    }
    fn screen(&self, v: [f32; 3]) -> [f32; 3] {
        let iz = 1.0 / v[2];
        [
            self.cx + v[0] * self.focal * iz,
            self.cy - v[1] * self.focal * iz,
            iz,
        ]
    }
}

/// Clip a triangle against the near plane; returns 0, 3 or 4 corners.
fn clip_near(tri: [ViewVertex; 3]) -> Vec<ViewVertex> {
    let mut out = Vec::with_capacity(4);
    for i in 0..3 {
        let a = tri[i];
        let b = tri[(i + 1) % 3];
        let a_in = a.p[2] >= NEAR;
        let b_in = b.p[2] >= NEAR;
        if a_in {
            out.push(a);
        }
        if a_in != b_in {
            let t = (NEAR - a.p[2]) / (b.p[2] - a.p[2]);
            let lerp = |x: f32, y: f32| x + (y - x) * t;
            out.push(ViewVertex {
                p: [lerp(a.p[0], b.p[0]), lerp(a.p[1], b.p[1]), NEAR],
                uv: [lerp(a.uv[0], b.uv[0]), lerp(a.uv[1], b.uv[1])],
                light: lerp(a.light, b.light),
            });
        }
    }
    out
}

fn sample_wrap(img: &Image, u: f32, v: f32) -> [f32; 4] {
    let w = img.width as i64;
    let h = img.height as i64;
    let x = u * w as f32 - 0.5;
    let y = v * h as f32 - 0.5;
    let (fx, fy) = (x.floor(), y.floor());
    let (tx, ty) = (x - fx, y - fy);
    let texel = |ix: i64, iy: i64| -> [f32; 4] {
        let ix = ix.rem_euclid(w) as usize;
        let iy = iy.rem_euclid(h) as usize;
        let i = (iy * w as usize + ix) * 4;
        let p = &img.rgba[i..i + 4];
        [p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32]
    };
    let (ix, iy) = (fx as i64, fy as i64);
    let a = texel(ix, iy);
    let b = texel(ix + 1, iy);
    let c = texel(ix, iy + 1);
    let d = texel(ix + 1, iy + 1);
    let mut out = [0.0; 4];
    for k in 0..4 {
        let top = a[k] + (b[k] - a[k]) * tx;
        let bottom = c[k] + (d[k] - c[k]) * tx;
        out[k] = (top + (bottom - top) * ty) / 255.0;
    }
    out
}

enum PaintRef<'a> {
    Image(&'a Image),
    Color([f32; 4]),
}

impl World {
    /// Draw the world at `width` x `height` pixels.
    pub fn render(&self, gd: &GameData, width: usize, height: usize) -> Rgba {
        let (w, h) = (width as f32, height as f32);
        let cam = self.camera;
        let forward = normalize(sub(cam.target, cam.pos));
        let right = normalize(cross(forward, [0.0, 0.0, 1.0]));
        let up = cross(right, forward);
        let proj = Projector {
            right,
            up,
            forward,
            eye: cam.pos,
            focal: (h / 2.0) / (cam.fov / 2.0).tan(),
            cx: w / 2.0,
            cy: h / 2.0,
        };

        // Paints: index 0 is the ground.
        let mut paint_keys: Vec<Paint> = Vec::new();
        let paint_index = |p: Paint, keys: &mut Vec<Paint>| -> usize {
            match keys.iter().position(|k| *k == p) {
                Some(i) => i,
                None => {
                    keys.push(p);
                    keys.len() - 1
                }
            }
        };
        let ground_paint = paint_index(
            match self.ground {
                Some(t) => Paint::Texture(t),
                None => Paint::Color([90, 120, 60, 255]),
            },
            &mut paint_keys,
        );

        let mut tris: Vec<ScreenTri> = Vec::new();
        let mut push = |corners: [ViewVertex; 3], paint: usize, blend: u16, two_sided: bool| {
            let poly = clip_near(corners);
            if poly.len() < 3 {
                return;
            }
            for k in 1..poly.len() - 1 {
                let c = [poly[0], poly[k], poly[k + 1]];
                let s = [
                    proj.screen(c[0].p),
                    proj.screen(c[1].p),
                    proj.screen(c[2].p),
                ];
                let area = (s[1][0] - s[0][0]) * (s[2][1] - s[0][1])
                    - (s[2][0] - s[0][0]) * (s[1][1] - s[0][1]);
                if area.abs() < 1e-6 || (!two_sided && area > 0.0) {
                    continue;
                }
                tris.push(ScreenTri {
                    s,
                    uv: [
                        [c[0].uv[0] * s[0][2], c[0].uv[1] * s[0][2]],
                        [c[1].uv[0] * s[1][2], c[1].uv[1] * s[1][2]],
                        [c[2].uv[0] * s[2][2], c[2].uv[1] * s[2][2]],
                    ],
                    light: [
                        c[0].light * s[0][2],
                        c[1].light * s[1][2],
                        c[2].light * s[2][2],
                    ],
                    depth: [c[0].p[2], c[1].p[2], c[2].p[2]],
                    paint,
                    blend,
                });
            }
        };

        // Ground: a grid of quads, so fog and clipping stay smooth.
        let n = 16;
        let size = self.ground_size;
        let step = size / n as f32;
        let ground_light = self.ambient + (1.0 - self.ambient) * self.sun[2].max(0.0);
        for i in 0..n {
            for j in 0..n {
                let x0 = -size / 2.0 + i as f32 * step;
                let y0 = -size / 2.0 + j as f32 * step;
                let corner = |x: f32, y: f32| ViewVertex {
                    p: proj.view([x, y, 0.0]),
                    uv: [x / self.ground_tile, y / self.ground_tile],
                    light: ground_light,
                };
                let a = corner(x0, y0);
                let b = corner(x0 + step, y0);
                let c = corner(x0 + step, y0 + step);
                let d = corner(x0, y0 + step);
                push([a, b, c], ground_paint, 0, true);
                push([a, c, d], ground_paint, 0, true);
            }
        }

        for inst in &self.instances {
            let (sy, cy) = inst.yaw.sin_cos();
            let world = |v: [f32; 3]| -> [f32; 3] {
                let x = v[0] * cy - v[1] * sy;
                let y = v[0] * sy + v[1] * cy;
                [
                    inst.pos[0] + x * inst.scale,
                    inst.pos[1] + y * inst.scale,
                    inst.pos[2] + v[2] * inst.scale,
                ]
            };
            let views: Vec<ViewVertex> = inst
                .mesh
                .vertices
                .iter()
                .map(|v| {
                    let nx = v.normal[0] * cy - v.normal[1] * sy;
                    let ny = v.normal[0] * sy + v.normal[1] * cy;
                    let nd = dot(normalize([nx, ny, v.normal[2]]), self.sun);
                    ViewVertex {
                        p: proj.view(world(v.pos)),
                        uv: v.uv,
                        light: self.ambient + (1.0 - self.ambient) * nd.max(0.0),
                    }
                })
                .collect();
            for b in &inst.mesh.batches {
                let paint = paint_index(b.paint, &mut paint_keys);
                for t in b.indices.as_chunks::<3>().0 {
                    let get = |i: u32| views.get(i as usize).copied();
                    if let (Some(a), Some(bb), Some(c)) = (get(t[0]), get(t[1]), get(t[2])) {
                        push([a, bb, c], paint, b.blend, b.two_sided || b.blend == 1);
                    }
                }
            }
        }

        // Resolve paints to images (looked up on this thread).
        let images: Vec<Option<Rc<Image>>> = paint_keys
            .iter()
            .map(|p| match p {
                Paint::Texture(id) => gd.image(&ImageKey::Id(*id)),
                Paint::Color(_) => None,
            })
            .collect();
        let paints: Vec<PaintRef> = paint_keys
            .iter()
            .zip(&images)
            .map(|(p, img)| match (p, img) {
                (_, Some(img)) => PaintRef::Image(img.as_ref()),
                (Paint::Color(c), None) => PaintRef::Color([
                    c[0] as f32 / 255.0,
                    c[1] as f32 / 255.0,
                    c[2] as f32 / 255.0,
                    c[3] as f32 / 255.0,
                ]),
                (Paint::Texture(_), None) => PaintRef::Color([0.6, 0.6, 0.6, 1.0]),
            })
            .collect();

        // Opaque first, then blended back to front.
        let (opaque, mut blended): (Vec<&ScreenTri>, Vec<&ScreenTri>) =
            tris.iter().partition(|t| t.blend <= 1);
        blended.sort_by(|a, b| {
            let da = a.depth[0] + a.depth[1] + a.depth[2];
            let db = b.depth[0] + b.depth[1] + b.depth[2];
            db.total_cmp(&da)
        });

        let mut color = vec![[0f32; 3]; width * height];
        // Sky gradient.
        for y in 0..height {
            let t = (y as f32 / h).min(0.5) * 2.0;
            let c = [
                self.sky_top[0] + (self.sky_horizon[0] - self.sky_top[0]) * t,
                self.sky_top[1] + (self.sky_horizon[1] - self.sky_top[1]) * t,
                self.sky_top[2] + (self.sky_horizon[2] - self.sky_top[2]) * t,
            ];
            for x in 0..width {
                color[y * width + x] = c;
            }
        }
        let mut zbuf = vec![0f32; width * height];
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(16);
        let rows_per = height.div_ceil(threads);
        let fog = (self.fog, self.fog_start, self.fog_end);
        std::thread::scope(|scope| {
            let mut color_rest: &mut [[f32; 3]] = &mut color;
            let mut z_rest: &mut [f32] = &mut zbuf;
            let mut y0 = 0;
            while y0 < height {
                let rows = rows_per.min(height - y0);
                let (c_band, c_next) = color_rest.split_at_mut(rows * width);
                let (z_band, z_next) = z_rest.split_at_mut(rows * width);
                color_rest = c_next;
                z_rest = z_next;
                let (opaque, blended, paints) = (&opaque, &blended, &paints);
                scope.spawn(move || {
                    for t in opaque.iter().chain(blended.iter()) {
                        raster(t, c_band, z_band, width, y0, rows, paints, fog);
                    }
                });
                y0 += rows;
            }
        });

        let mut out = Rgba::new(width, height, [0, 0, 0]);
        for (i, c) in color.iter().enumerate() {
            out.data[i * 4] = (c[0].clamp(0.0, 1.0) * 255.0).round() as u8;
            out.data[i * 4 + 1] = (c[1].clamp(0.0, 1.0) * 255.0).round() as u8;
            out.data[i * 4 + 2] = (c[2].clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        out
    }
}

#[allow(clippy::too_many_arguments)]
fn raster(
    t: &ScreenTri,
    color: &mut [[f32; 3]],
    zbuf: &mut [f32],
    width: usize,
    y_start: usize,
    rows: usize,
    paints: &[PaintRef],
    fog: ([f32; 3], f32, f32),
) {
    let s = &t.s;
    let min_x = s
        .iter()
        .map(|p| p[0])
        .fold(f32::MAX, f32::min)
        .floor()
        .max(0.0) as usize;
    let max_x =
        (s.iter().map(|p| p[0]).fold(f32::MIN, f32::max).ceil() as isize).min(width as isize - 1);
    let min_y =
        (s.iter().map(|p| p[1]).fold(f32::MAX, f32::min).floor() as isize).max(y_start as isize);
    let max_y = (s.iter().map(|p| p[1]).fold(f32::MIN, f32::max).ceil() as isize)
        .min((y_start + rows) as isize - 1);
    if max_x < min_x as isize || max_y < min_y {
        return;
    }
    let edge = |a: [f32; 3], b: [f32; 3], x: f32, y: f32| {
        (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0])
    };
    let area = edge(s[0], s[1], s[2][0], s[2][1]);
    if area == 0.0 {
        return;
    }
    let inv_area = 1.0 / area;
    let paint = &paints[t.paint];
    let (fog_color, fog_start, fog_end) = fog;
    for y in min_y..=max_y {
        let py = y as f32 + 0.5;
        let row = (y as usize - y_start) * width;
        for x in min_x..=max_x as usize {
            let px = x as f32 + 0.5;
            let w0 = edge(s[1], s[2], px, py) * inv_area;
            let w1 = edge(s[2], s[0], px, py) * inv_area;
            let w2 = 1.0 - w0 - w1;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let iz = w0 * s[0][2] + w1 * s[1][2] + w2 * s[2][2];
            let i = row + x;
            if iz <= zbuf[i] {
                continue;
            }
            let z = 1.0 / iz;
            let u = (w0 * t.uv[0][0] + w1 * t.uv[1][0] + w2 * t.uv[2][0]) * z;
            let v = (w0 * t.uv[0][1] + w1 * t.uv[1][1] + w2 * t.uv[2][1]) * z;
            let light = (w0 * t.light[0] + w1 * t.light[1] + w2 * t.light[2]) * z;
            let texel = match paint {
                PaintRef::Image(img) => sample_wrap(img, u, v),
                PaintRef::Color(c) => *c,
            };
            let alpha = match t.blend {
                0 => 1.0,
                1 => {
                    if texel[3] < 0.5 {
                        continue;
                    }
                    1.0
                }
                _ => texel[3],
            };
            if alpha <= 0.01 {
                continue;
            }
            let f = ((z - fog_start) / (fog_end - fog_start)).clamp(0.0, 1.0);
            let dst = color[i];
            let mut out = [0.0; 3];
            for k in 0..3 {
                let lit = texel[k] * light;
                let fogged = lit + (fog_color[k] - lit) * f;
                out[k] = dst[k] + (fogged - dst[k]) * alpha;
            }
            color[i] = out;
            if t.blend <= 1 {
                zbuf[i] = iz;
            }
        }
    }
}

/// Where the camera looks from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Behind and above the player, as in the game.
    Behind,
    /// In front of the player, as on the character select screen.
    CharacterSelect,
}

/// FileDataIDs of the demo scene's art.
mod art {
    /// tileset/elwynn/elwynngrassbase.blp
    pub const GROUND: u32 = 187126;
    /// world/azeroth/elwynn/passivedoodads/trees/elwynntreecanopy01..04.m2
    /// and elwynntreemid01.m2
    pub const TREES: [u32; 5] = [189927, 189928, 189929, 189930, 189932];
    /// world/azeroth/elwynn/passivedoodads/trees/elwynnfirtree01.m2,
    /// elwynntree01/elwynnpine01..02.m2
    pub const FIRS: [u32; 3] = [189923, 189924, 189925];
    /// world/azeroth/elwynn/passivedoodads/bush/elwynnbush01..09.m2
    pub const BUSHES: [u32; 5] = [189692, 189693, 189694, 189696, 189699];
    /// world/azeroth/elwynn/passivedoodads/cliffrocks/elwynncliffrock01..03.m2
    pub const ROCKS: [u32; 3] = [189715, 189716, 189717];
    /// creature/deer/deer.m2, creature/rabbit/rabbit.m2
    pub const CREATURES: [u32; 2] = [123362, 125512];
}

/// The default skin texture of a race and sex (ChrRaces ID; 0 male, 1
/// female): `character/<race>/<sex>/<race><sex>skin00_00_hd.blp`, a whole
/// body and face texture in the layout the model expects. The client
/// composes skins from customization choices; this is the first choice.
fn default_skin(race: u32, sex: u32) -> Option<u32> {
    let (male, female) = match race {
        1 => (1027767, 1002483),
        2 => (916966, 949450),
        3 => (878515, 949155),
        4 => (1030035, 0),
        5 => (959282, 1019816),
        6 => (974717, 996031),
        7 => (916486, 948734),
        8 => (1034514, 1037096),
        9 => (3058800, 3062624),
        10 => (1098758, 1098217),
        11 => (1003984, 1022933),
        22 => (3056241, 3057321),
        // character/dracthyr/dracthyrdragon_skin_color_4207726.blp
        52 => (4207726, 4207726),
        _ => (0, 0),
    };
    let id = if sex == 1 { female } else { male };
    (id != 0).then_some(id)
}

/// Skin and hair paints for character models: the race's default skin
/// texture where known, else a color by race (ChrRaces ID).
fn character_paints(race: u32, sex: u32) -> HashMap<u32, Paint> {
    let skin = match race {
        // Orc, Mag'har: green; Troll, Zandalari: blue-green.
        2 | 36 => [96, 130, 70],
        8 | 31 => [80, 120, 130],
        // Undead: grey; Tauren, Highmountain: brown fur.
        5 => [150, 155, 150],
        6 | 28 => [110, 80, 60],
        // Draenei: blue; Lightforged: lavender; Void elf: purple.
        11 => [110, 120, 180],
        30 => [170, 160, 200],
        29 => [120, 100, 170],
        // Nightborne, Night elf: violet; Dracthyr: bronze scales.
        27 | 4 => [140, 110, 170],
        52 => [150, 120, 80],
        // Goblin: green; Vulpera: red fur; Earthen: stone.
        9 => [120, 160, 80],
        35 => [170, 90, 50],
        84 => [140, 140, 150],
        _ => [196, 150, 120],
    };
    let mut m = HashMap::new();
    let skin = match default_skin(race, sex) {
        Some(id) => Paint::Texture(id),
        None => Paint::Color([skin[0], skin[1], skin[2], 255]),
    };
    m.insert(1, skin);
    m.insert(6, Paint::Color([90, 60, 40, 255]));
    m.insert(2, Paint::Color([120, 100, 80, 255]));
    m
}

/// A clearing with trees, rocks, a deer and a rabbit, and the player's race
/// model in the middle, at the origin, facing north (+x).
pub fn demo_scene(gd: &GameData, race: u32, sex: u32, view: View) -> World {
    let mut world = World {
        ground: Some(art::GROUND),
        ground_tile: 6.0,
        ..World::default()
    };
    let none = HashMap::new();
    let mut cache: HashMap<u32, Option<Rc<Mesh>>> = HashMap::new();
    let mut mesh = |file: u32, replace: &HashMap<u32, Paint>| -> Option<Rc<Mesh>> {
        cache
            .entry(file)
            .or_insert_with(|| load_model(gd, file, replace, None).map(Rc::new))
            .clone()
    };
    // A ring of trees around the clearing, deterministic placement.
    let mut seed = 12345u32;
    let mut rand = move || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / (1u32 << 24) as f32
    };
    for i in 0..36 {
        let angle = i as f32 / 36.0 * std::f32::consts::TAU + rand() * 0.15;
        let dist = 24.0 + rand() * 40.0;
        let file = if i % 3 == 0 {
            art::FIRS[i % art::FIRS.len()]
        } else {
            art::TREES[i % art::TREES.len()]
        };
        if let Some(m) = mesh(file, &none) {
            world.instances.push(Instance {
                mesh: m,
                pos: [dist * angle.cos(), dist * angle.sin(), 0.0],
                yaw: rand() * std::f32::consts::TAU,
                scale: 0.45 + rand() * 0.3,
            });
        }
    }
    for i in 0..14 {
        let angle = rand() * std::f32::consts::TAU;
        let dist = 9.0 + rand() * 18.0;
        if let Some(m) = mesh(art::BUSHES[i % art::BUSHES.len()], &none) {
            world.instances.push(Instance {
                mesh: m,
                pos: [dist * angle.cos(), dist * angle.sin(), 0.0],
                yaw: rand() * std::f32::consts::TAU,
                scale: 0.6 + rand() * 0.4,
            });
        }
    }
    for (i, file) in art::ROCKS.iter().enumerate() {
        if let Some(m) = mesh(*file, &none) {
            let angle = 0.6 + i as f32 * 1.9;
            world.instances.push(Instance {
                mesh: m,
                pos: [18.0 * angle.cos(), 18.0 * angle.sin(), -0.3],
                yaw: angle,
                scale: 0.3,
            });
        }
    }
    for (i, file) in art::CREATURES.iter().enumerate() {
        let Some((_, textures)) = display_for_model(gd, *file) else {
            continue;
        };
        let mut replace = HashMap::new();
        for (k, t) in textures.iter().enumerate() {
            if *t != 0 {
                replace.insert(11 + k as u32, Paint::Texture(*t));
            }
        }
        if let Some(m) = load_model(gd, *file, &replace, None) {
            world.instances.push(Instance {
                mesh: Rc::new(m),
                pos: [7.0 + i as f32 * 2.0, -5.0 + i as f32 * 9.0, 0.0],
                yaw: 2.2 + i as f32,
                scale: 1.0,
            });
        }
    }
    let loaded = race_model(gd, race, sex).and_then(|f| {
        load_model(
            gd,
            f,
            &character_paints(race, sex),
            Some(default_character_geoset),
        )
    });
    if let Some(m) = loaded {
        // Taller models (the Dracthyr dragon form) need the camera
        // further away.
        let top = m.vertices.iter().map(|v| v.pos[2]).fold(0.0f32, f32::max);
        let extra = (top - 2.0).max(0.0);
        world.instances.push(Instance {
            mesh: Rc::new(m),
            pos: [0.0, 0.0, 0.0],
            yaw: 0.0,
            scale: 1.0,
        });
        world.camera = match view {
            // About the game's default camera distance, in yards.
            View::Behind => Camera {
                pos: [-6.5 - extra, 0.0, 3.4 + extra * 0.3],
                target: [3.0, 0.0, 1.2 + extra * 0.2],
                fov: 0.95,
            },
            View::CharacterSelect => Camera {
                pos: [4.2 + extra * 1.3, -0.8, 1.5 + extra * 0.3],
                target: [0.0, 0.0, 1.0 + extra * 0.3],
                fov: 0.75,
            },
        };
    }
    world
}
