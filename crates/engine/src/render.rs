//! Software rendering of a scene to an RGBA image, without a window.
//! Used for screenshots in tests and headless runs. It is simple, not fast:
//! textures are sampled per pixel and text is rasterized with ab_glyph.

use std::collections::HashMap;
use std::io::Write;

use ab_glyph::{Font, FontArc, PxScale, ScaleFont, point};

use crate::gamedata::GameData;
use crate::layout::Rect;
use crate::scene::{Item, Quad, Scene, TextItem};

pub struct Canvas {
    pub width: usize,
    pub height: usize,
    /// Premultiplied-free RGBA, 4 floats per pixel.
    pixels: Vec<[f32; 4]>,
    /// Pixels per screen unit.
    scale: f64,
    screen_height: f64,
}

impl Canvas {
    pub fn new(scene: &Scene, scale: f64) -> Canvas {
        let width = (scene.width * scale).round() as usize;
        let height = (scene.height * scale).round() as usize;
        Canvas {
            width,
            height,
            pixels: vec![[0.11, 0.12, 0.14, 1.0]; width * height],
            scale,
            screen_height: scene.height,
        }
    }

    /// Pixel bounds of a rectangle in screen units: (x0, y0, x1, y1).
    fn bounds(&self, r: &Rect) -> (f64, f64, f64, f64) {
        let x0 = r.left * self.scale;
        let x1 = r.right() * self.scale;
        let y0 = (self.screen_height - r.top()) * self.scale;
        let y1 = (self.screen_height - r.bottom) * self.scale;
        (x0, y0, x1, y1)
    }

    fn blend(&mut self, x: usize, y: usize, c: [f32; 4], additive: bool) {
        let p = &mut self.pixels[y * self.width + x];
        let a = c[3].clamp(0.0, 1.0);
        for i in 0..3 {
            p[i] = if additive {
                (p[i] + c[i] * a).min(1.0)
            } else {
                p[i] * (1.0 - a) + c[i] * a
            };
        }
    }

    pub fn to_rgba8(&self) -> Vec<u8> {
        self.pixels
            .iter()
            .flat_map(|p| p.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8))
            .collect()
    }
}

fn sample(img: &crate::gamedata::Image, u: f32, v: f32, repeat: bool) -> [f32; 4] {
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

fn draw_quad(canvas: &mut Canvas, gd: Option<&GameData>, q: &Quad) {
    let image = match (&q.image, gd) {
        (Some(key), Some(gd)) => match gd.image(key) {
            Some(img) => Some(img),
            None => return,
        },
        (Some(_), None) => return,
        (None, _) => None,
    };
    let (x0, y0, x1, y1) = canvas.bounds(&q.rect);
    let (mut cx0, mut cy0, mut cx1, mut cy1) = (x0, y0, x1, y1);
    if let Some(c) = &q.clip {
        let (a, b, c2, d) = canvas.bounds(c);
        cx0 = cx0.max(a);
        cy0 = cy0.max(b);
        cx1 = cx1.min(c2);
        cy1 = cy1.min(d);
    }
    let px0 = cx0.floor().max(0.0) as usize;
    let py0 = cy0.floor().max(0.0) as usize;
    let px1 = (cx1.ceil().max(0.0) as usize).min(canvas.width);
    let py1 = (cy1.ceil().max(0.0) as usize).min(canvas.height);
    let uv = q.uv;
    let repeat = q.tile.0 || q.tile.1;
    let (mut ux, mut vy) = (1.0f32, 1.0f32);
    if let (Some(img), true) = (&image, repeat) {
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
            let mut c = q.color;
            if let Some(img) = &image {
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
            canvas.blend(px, py, c, q.additive);
        }
    }
}

fn draw_text(
    canvas: &mut Canvas,
    fonts: &mut HashMap<String, Option<FontArc>>,
    gd: Option<&GameData>,
    t: &TextItem,
) {
    let key = t.font_path.to_ascii_lowercase();
    let font = fonts
        .entry(key)
        .or_insert_with(|| {
            gd.and_then(|g| g.read_path(&t.font_path))
                .and_then(|b| FontArc::try_from_vec(b).ok())
        })
        .clone();
    let Some(font) = font else { return };
    let size = (t.size * canvas.scale) as f32;
    let scaled = font.as_scaled(PxScale::from(size));
    let clip = t.clip.map(|c| canvas.bounds(&c));
    let mut passes: Vec<([f32; 4], f32, f32, bool)> = Vec::new();
    if let Some((c, (dx, dy))) = t.shadow {
        passes.push((
            c,
            (dx * canvas.scale) as f32,
            (-dy * canvas.scale) as f32,
            true,
        ));
    }
    passes.push((t.color, 0.0, 0.0, false));
    for (color, dx, dy, is_shadow) in passes {
        for line in &t.lines {
            let mut x = (line.x * canvas.scale) as f32 + dx;
            let baseline =
                ((canvas.screen_height - line.y_top) * canvas.scale) as f32 + scaled.ascent() + dy;
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
                for ch in run.text.chars() {
                    let id = font.glyph_id(ch);
                    let glyph = id.with_scale_and_position(size, point(x, baseline));
                    x += scaled.h_advance(id);
                    let Some(outline) = font.outline_glyph(glyph) else {
                        continue;
                    };
                    let b = outline.px_bounds();
                    outline.draw(|gx, gy, cov| {
                        let px = b.min.x as i64 + gx as i64;
                        let py = b.min.y as i64 + gy as i64;
                        if px < 0
                            || py < 0
                            || px as usize >= canvas.width
                            || py as usize >= canvas.height
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
                        canvas.blend(px as usize, py as usize, cc, false);
                    });
                }
            }
        }
    }
}

/// Render a scene at `scale` pixels per screen unit.
pub fn render(scene: &Scene, gd: Option<&GameData>, scale: f64) -> Canvas {
    let mut canvas = Canvas::new(scene, scale);
    let mut fonts = HashMap::new();
    for item in &scene.items {
        match item {
            Item::Quad(q) => draw_quad(&mut canvas, gd, q),
            Item::Text(t) => draw_text(&mut canvas, &mut fonts, gd, t),
        }
    }
    canvas
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
