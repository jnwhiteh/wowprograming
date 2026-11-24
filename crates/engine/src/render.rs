//! Software rendering of a scene to an RGBA image, without a window.
//! Used for screenshots in tests and headless runs. It is simple, not fast:
//! textures are sampled per pixel and text is rasterized with ab_glyph.

use std::collections::HashMap;
use std::io::Write;

use ab_glyph::{Font, FontArc, ScaleFont, point};

use crate::gamedata::GameData;
use crate::gamedata::Image;
use crate::layout::Rect;
use crate::scene::{Item, Quad, Scene, TextItem};

pub struct Canvas {
    pub width: usize,
    pub height: usize,
    /// RGBA, 4 floats per pixel, not premultiplied.
    pixels: Vec<[f32; 4]>,
}

impl Canvas {
    pub fn to_rgba8(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pixels.len() * 4);
        to_rgba8_into(&self.pixels, &mut out);
        out
    }
}

fn to_rgba8_into(pixels: &[[f32; 4]], out: &mut Vec<u8>) {
    out.extend(
        pixels
            .iter()
            .flat_map(|p| p.map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)),
    );
}

/// A horizontal band of the output that one thread draws.
struct Band<'a> {
    width: usize,
    height: usize,
    /// First and last (exclusive) row of the band.
    y0: usize,
    y1: usize,
    pixels: &'a mut [[f32; 4]],
    /// Pixels per screen unit.
    scale: f64,
    screen_height: f64,
}

impl Band<'_> {
    /// Pixel bounds of a rectangle in screen units: (x0, y0, x1, y1).
    fn bounds(&self, r: &Rect) -> (f64, f64, f64, f64) {
        let x0 = r.left * self.scale;
        let x1 = r.right() * self.scale;
        let y0 = (self.screen_height - r.top()) * self.scale;
        let y1 = (self.screen_height - r.bottom) * self.scale;
        (x0, y0, x1, y1)
    }

    fn blend(&mut self, x: usize, y: usize, c: [f32; 4], additive: bool) {
        let p = &mut self.pixels[(y - self.y0) * self.width + x];
        let a = c[3].clamp(0.0, 1.0);
        for i in 0..3 {
            p[i] = if additive {
                (p[i] + c[i] * a).min(1.0)
            } else {
                p[i] * (1.0 - a) + c[i] * a
            };
        }
    }
}

fn sample(img: &Image, u: f32, v: f32, repeat: bool) -> [f32; 4] {
    let (w, h) = (img.width as f32, img.height as f32);
    let (mut u, mut v) = (u, v);
    if repeat {
        u = u.rem_euclid(1.0);
        v = v.rem_euclid(1.0);
    }
    let x = ((u * w).floor() as i64).clamp(0, img.width as i64 - 1) as usize;
    let y = ((v * h).floor() as i64).clamp(0, img.height as i64 - 1) as usize;
    let i = (y * img.width as usize + x) * 4;
    let p = &img.rgba[i..i + 4];
    [
        p[0] as f32 / 255.0,
        p[1] as f32 / 255.0,
        p[2] as f32 / 255.0,
        p[3] as f32 / 255.0,
    ]
}

/// A quad with its images looked up.
struct Prepared<'a> {
    quad: &'a Quad,
    image: Option<&'a Image>,
    masks: Vec<(&'a crate::scene::Mask, Option<&'a Image>)>,
}

fn draw_quad(band: &mut Band, p: &Prepared) {
    let q = p.quad;
    let (x0, y0, x1, y1) = band.bounds(&q.rect);
    let (mut cx0, mut cy0, mut cx1, mut cy1) = (x0, y0, x1, y1);
    if let Some(c) = &q.clip {
        let (a, b, c2, d) = band.bounds(c);
        cx0 = cx0.max(a);
        cy0 = cy0.max(b);
        cx1 = cx1.min(c2);
        cy1 = cy1.min(d);
    }
    let px0 = cx0.floor().max(0.0) as usize;
    let py0 = (cy0.floor().max(0.0) as usize).max(band.y0);
    let px1 = (cx1.ceil().max(0.0) as usize).min(band.width);
    let py1 = (cy1.ceil().max(0.0) as usize).min(band.y1);
    let uv = q.uv;
    let repeat = q.tile.0 || q.tile.1;
    let (mut ux, mut vy) = (1.0f32, 1.0f32);
    if let (Some(img), true) = (p.image, repeat) {
        if q.tile.0 {
            ux = (q.rect.width as f32 / img.width as f32).max(0.01);
        }
        if q.tile.1 {
            vy = (q.rect.height as f32 / img.height as f32).max(0.01);
        }
    }
    for py in py0..py1 {
        let ty = ((py as f64 + 0.5 - y0) / (y1 - y0)) as f32;
        if !(0.0..=1.0).contains(&ty) {
            continue;
        }
        for px in px0..px1 {
            let tx = ((px as f64 + 0.5 - x0) / (x1 - x0)) as f32;
            if !(0.0..=1.0).contains(&tx) {
                continue;
            }
            if let Some((elapsed, reverse)) = q.sweep {
                // Clockwise angle from 12 o'clock, as a fraction of a turn.
                let a = (tx - 0.5).atan2(0.5 - ty);
                let turn = a.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
                // The swipe still covers the part not yet passed.
                let covered = if reverse {
                    turn < elapsed
                } else {
                    turn >= elapsed
                };
                if !covered {
                    continue;
                }
            }
            let mut c = q.color;
            if let Some(img) = p.image {
                // Bilinear interpolation of the corner coordinates.
                let (tx2, ty2) = (tx * ux, ty * vy);
                let top_u = uv[0] + (uv[4] - uv[0]) * tx2;
                let top_v = uv[1] + (uv[5] - uv[1]) * tx2;
                let bot_u = uv[2] + (uv[6] - uv[2]) * tx2;
                let bot_v = uv[3] + (uv[7] - uv[3]) * tx2;
                let u = top_u + (bot_u - top_u) * ty2;
                let v = top_v + (bot_v - top_v) * ty2;
                let mut s = sample(img, u, v, repeat);
                if q.desaturate {
                    let l = s[0] * 0.3 + s[1] * 0.59 + s[2] * 0.11;
                    s = [l, l, l, s[3]];
                }
                c = [c[0] * s[0], c[1] * s[1], c[2] * s[2], c[3] * s[3]];
            }
            if !p.masks.is_empty() {
                let sx = (px as f64 + 0.5) / band.scale;
                let sy = band.screen_height - (py as f64 + 0.5) / band.scale;
                for (m, img) in &p.masks {
                    c[3] *= mask_alpha(m, *img, sx, sy);
                    if c[3] <= 0.0 {
                        break;
                    }
                }
                if c[3] <= 0.0 {
                    continue;
                }
            }
            band.blend(px, py, c, q.additive);
        }
    }
}

/// Alpha of a mask at a point in screen units.
fn mask_alpha(m: &crate::scene::Mask, img: Option<&Image>, sx: f64, sy: f64) -> f32 {
    let r = &m.rect;
    if r.width <= 0.0 || r.height <= 0.0 {
        return 0.0;
    }
    let tx = ((sx - r.left) / r.width) as f32;
    let ty = ((r.top() - sy) / r.height) as f32;
    if !(0.0..=1.0).contains(&tx) || !(0.0..=1.0).contains(&ty) {
        return 0.0;
    }
    let Some(img) = img else { return 1.0 };
    let uv = m.uv;
    let top_u = uv[0] + (uv[4] - uv[0]) * tx;
    let top_v = uv[1] + (uv[5] - uv[1]) * tx;
    let bot_u = uv[2] + (uv[6] - uv[2]) * tx;
    let bot_v = uv[3] + (uv[7] - uv[3]) * tx;
    let u = top_u + (bot_u - top_u) * ty;
    let v = top_v + (bot_v - top_v) * ty;
    if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
        return 0.0;
    }
    sample(img, u, v, false)[3]
}

fn draw_text(band: &mut Band, font: &FontArc, t: &TextItem) {
    let size = (t.size * band.scale) as f32;
    if size <= 0.0 {
        return;
    }
    let scale = crate::text::em_scale(font, size);
    let scaled = font.as_scaled(scale);
    // The line box is one em high; split the glyphs' extent above and below
    // the baseline in proportion.
    let ascent = size * scaled.ascent() / (scaled.ascent() - scaled.descent()).max(1e-3);
    // Skip text that cannot touch this band.
    let first_top = t.lines.first().map(|l| l.y_top).unwrap_or(0.0);
    let last_top = t.lines.last().map(|l| l.y_top).unwrap_or(0.0);
    let top_px = (band.screen_height - first_top) * band.scale - size as f64;
    let bottom_px = (band.screen_height - last_top) * band.scale + 2.0 * size as f64;
    if bottom_px < band.y0 as f64 || top_px > band.y1 as f64 {
        return;
    }
    let clip = t.clip.map(|c| band.bounds(&c));
    let mut passes: Vec<([f32; 4], f32, f32, bool)> = Vec::new();
    if let Some((c, (dx, dy))) = t.shadow {
        passes.push((c, (dx * band.scale) as f32, (-dy * band.scale) as f32, true));
    }
    passes.push((t.color, 0.0, 0.0, false));
    for (color, dx, dy, is_shadow) in passes {
        for line in &t.lines {
            let mut x = (line.x * band.scale) as f32 + dx;
            let baseline = ((band.screen_height - line.y_top) * band.scale) as f32 + ascent + dy;
            for run in &line.runs {
                let mut c = if is_shadow {
                    color
                } else {
                    run.color.unwrap_or(color)
                };
                c[3] = color[3]
                    * if is_shadow {
                        1.0
                    } else {
                        run.color.map(|r| r[3]).unwrap_or(1.0)
                    };
                for (i, ch) in run.text.chars().enumerate() {
                    let id = font.glyph_id(ch);
                    let glyph = id.with_scale_and_position(scale, point(x, baseline));
                    // Place glyphs where the engine measured them.
                    x += match run.advances.get(i) {
                        Some(a) => (a * band.scale) as f32,
                        None => scaled.h_advance(id),
                    };
                    let Some(outline) = font.outline_glyph(glyph) else {
                        continue;
                    };
                    let b = outline.px_bounds();
                    if b.max.y < band.y0 as f32 || b.min.y > band.y1 as f32 {
                        continue;
                    }
                    outline.draw(|gx, gy, cov| {
                        let px = b.min.x as i64 + gx as i64;
                        let py = b.min.y as i64 + gy as i64;
                        if px < 0
                            || py < band.y0 as i64
                            || px as usize >= band.width
                            || py as usize >= band.y1.min(band.height)
                        {
                            return;
                        }
                        if let Some((cx0, cy0, cx1, cy1)) = clip
                            && ((px as f64) < cx0
                                || (px as f64) >= cx1
                                || (py as f64) < cy0
                                || (py as f64) >= cy1)
                        {
                            return;
                        }
                        let mut cc = c;
                        cc[3] *= cov;
                        band.blend(px as usize, py as usize, cc, false);
                    });
                }
            }
        }
    }
}

/// Fonts by path, kept between frames.
#[derive(Default)]
pub struct FontStore {
    fonts: HashMap<String, Option<FontArc>>,
}

impl FontStore {
    fn get(&mut self, gd: Option<&GameData>, path: &str) -> Option<FontArc> {
        self.fonts
            .entry(path.to_ascii_lowercase())
            .or_insert_with(|| {
                gd.and_then(|g| g.read_path(path))
                    .and_then(|b| FontArc::try_from_vec(b).ok())
            })
            .clone()
    }
}

/// Background colour of the canvas.
const BACKGROUND: [f32; 4] = [0.11, 0.12, 0.14, 1.0];

/// Render a scene at `scale` pixels per screen unit.
pub fn render(scene: &Scene, gd: Option<&GameData>, scale: f64) -> Canvas {
    let mut fonts = FontStore::default();
    render_with(scene, gd, scale, &mut fonts)
}

/// Render a scene, reusing loaded fonts. Bands of rows are drawn on
/// several threads.
pub fn render_with(
    scene: &Scene,
    gd: Option<&GameData>,
    scale: f64,
    fonts: &mut FontStore,
) -> Canvas {
    render_over(scene, gd, scale, fonts, None)
}

/// Like [`render_with`], drawing the UI over `background` (such as the 3D
/// world), stretched to the canvas size.
pub fn render_over(
    scene: &Scene,
    gd: Option<&GameData>,
    scale: f64,
    fonts: &mut FontStore,
    background: Option<&crate::loading::Rgba>,
) -> Canvas {
    let width = (scene.width * scale).round() as usize;
    let height = (scene.height * scale).round() as usize;
    let mut pixels = vec![BACKGROUND; width * height];
    if let Some(bg) = background
        && bg.width > 0
        && bg.height > 0
    {
        for y in 0..height {
            let sy = (y * bg.height / height.max(1)).min(bg.height - 1);
            for x in 0..width {
                let sx = (x * bg.width / width.max(1)).min(bg.width - 1);
                let i = (sy * bg.width + sx) * 4;
                let p = &bg.data[i..i + 4];
                pixels[y * width + x] = [
                    p[0] as f32 / 255.0,
                    p[1] as f32 / 255.0,
                    p[2] as f32 / 255.0,
                    1.0,
                ];
            }
        }
    }

    // Look up images and fonts on this thread; the bands only read them.
    let mut images: HashMap<crate::gamedata::ImageKey, Option<std::rc::Rc<Image>>> = HashMap::new();
    let mut image = |key: &Option<crate::gamedata::ImageKey>| -> Option<std::rc::Rc<Image>> {
        let key = key.as_ref()?;
        images
            .entry(key.clone())
            .or_insert_with(|| gd.and_then(|g| g.image(key)))
            .clone()
    };
    enum Owned {
        Quad(
            usize,
            Option<std::rc::Rc<Image>>,
            Vec<Option<std::rc::Rc<Image>>>,
        ),
        Text(usize, FontArc),
    }
    let mut owned = Vec::with_capacity(scene.items.len());
    for (i, item) in scene.items.iter().enumerate() {
        match item {
            Item::Quad(q) => {
                let img = image(&q.image);
                if q.image.is_some() && img.is_none() {
                    continue;
                }
                let masks = q.masks.iter().map(|m| image(&m.image)).collect();
                owned.push(Owned::Quad(i, img, masks));
            }
            Item::Text(t) => {
                if let Some(f) = fonts.get(gd, &t.font_path) {
                    owned.push(Owned::Text(i, f));
                }
            }
        }
    }
    enum Draw<'a> {
        Quad(Prepared<'a>),
        Text(&'a TextItem, &'a FontArc),
    }
    let draws: Vec<Draw> = owned
        .iter()
        .map(|o| match o {
            Owned::Quad(i, img, masks) => {
                let Item::Quad(q) = &scene.items[*i] else {
                    unreachable!()
                };
                Draw::Quad(Prepared {
                    quad: q,
                    image: img.as_deref(),
                    masks: q
                        .masks
                        .iter()
                        .zip(masks)
                        .map(|(m, img)| (m, img.as_deref()))
                        .collect(),
                })
            }
            Owned::Text(i, f) => {
                let Item::Text(t) = &scene.items[*i] else {
                    unreachable!()
                };
                Draw::Text(t, f)
            }
        })
        .collect();

    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 16);
    let rows_per_band = height.div_ceil(threads).max(1);
    let draws = &draws;
    std::thread::scope(|s| {
        for (n, chunk) in pixels.chunks_mut(rows_per_band * width.max(1)).enumerate() {
            let y0 = n * rows_per_band;
            let y1 = (y0 + rows_per_band).min(height);
            s.spawn(move || {
                let mut band = Band {
                    width,
                    height,
                    y0,
                    y1,
                    pixels: chunk,
                    scale,
                    screen_height: scene.height,
                };
                for d in draws {
                    match d {
                        Draw::Quad(p) => draw_quad(&mut band, p),
                        Draw::Text(t, f) => draw_text(&mut band, f, t),
                    }
                }
            });
        }
    });
    Canvas {
        width,
        height,
        pixels,
    }
}

/// Write an RGBA image as PNG.
pub fn write_png(
    path: &std::path::Path,
    width: usize,
    height: usize,
    rgba: &[u8],
) -> std::io::Result<()> {
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut crc_data = kind.to_vec();
        crc_data.extend_from_slice(data);
        out.extend_from_slice(&crc_data);
        out.extend_from_slice(&crc32(&crc_data).to_be_bytes());
    }
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xffff_ffffu32;
        for &b in data {
            crc ^= b as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }
    let mut raw = Vec::with_capacity((width * 4 + 1) * height);
    for row in rgba.chunks_exact(width * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    z.write_all(&raw)?;
    let compressed = z.finish()?;
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend_from_slice(&(width as u32).to_be_bytes());
    header.extend_from_slice(&(height as u32).to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &compressed);
    chunk(&mut out, b"IEND", &[]);
    std::fs::write(path, out)
}
