//! The loading screen the client draws while a map loads.
//!
//! The client draws it in C++ (no UI code is involved), from the
//! LoadingScreens table: the map's row in Map.db2 names a loading screen.
//! Older screens are one wide image. Newer ones have a skin (a parchment
//! frame, from LoadingScreenSkin), a main image and a logo. The layout was
//! measured from a capture of the game at 3440x1440.

use crate::gamedata::{GameData, ImageKey};
use wowprogramming_formats::blp::Image;
use wowprogramming_formats::db2;

/// FileDataIDs of the tables used here.
const MAP_DB2: u32 = 1349477;
const LOADING_SCREENS_DB2: u32 = 1266541;
const LOADING_SCREEN_SKIN_DB2: u32 = 2478235;
/// Map.db2 field of the loading screen ID.
const MAP_LOADING_SCREEN: usize = 11;
/// Loading bar textures.
const BAR_BACKGROUND: u32 = 2241407;
const BAR_FILL: u32 = 131812;
const BAR_BORDER: u32 = 1620539;

/// An RGBA image with 8 bits per channel, row-major.
pub struct Rgba {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

impl Rgba {
    pub fn new(width: usize, height: usize, rgb: [u8; 3]) -> Rgba {
        let mut data = vec![255; width * height * 4];
        for px in data.as_chunks_mut::<4>().0 {
            px[..3].copy_from_slice(&rgb);
        }
        Rgba {
            width,
            height,
            data,
        }
    }

    /// Draw `img` (or the part `uv` = left, top, right, bottom in 0..1)
    /// into the rectangle x, y, w, h with bilinear filtering and alpha
    /// blending.
    pub fn draw(&mut self, img: &Image, x: f64, y: f64, w: f64, h: f64, uv: [f64; 4]) {
        if w <= 0.0 || h <= 0.0 || img.width == 0 || img.height == 0 {
            return;
        }
        let x0 = x.floor().max(0.0) as usize;
        let y0 = y.floor().max(0.0) as usize;
        let x1 = ((x + w).ceil() as usize).min(self.width);
        let y1 = ((y + h).ceil() as usize).min(self.height);
        let (iw, ih) = (img.width as f64, img.height as f64);
        for py in y0..y1 {
            let v = uv[1] + ((py as f64 + 0.5 - y) / h) * (uv[3] - uv[1]);
            for px in x0..x1 {
                let u = uv[0] + ((px as f64 + 0.5 - x) / w) * (uv[2] - uv[0]);
                let s = sample(img, u * iw - 0.5, v * ih - 0.5);
                let a = s[3] / 255.0;
                if a <= 0.0 {
                    continue;
                }
                let i = (py * self.width + px) * 4;
                for c in 0..3 {
                    let d = self.data[i + c] as f64;
                    self.data[i + c] = (d + (s[c] - d) * a).round() as u8;
                }
            }
        }
    }

    /// Fill a rectangle with a solid color and alpha.
    pub fn fill(&mut self, x: f64, y: f64, w: f64, h: f64, rgba: [f64; 4]) {
        let x0 = x.max(0.0) as usize;
        let y0 = y.max(0.0) as usize;
        let x1 = ((x + w) as usize).min(self.width);
        let y1 = ((y + h) as usize).min(self.height);
        for py in y0..y1 {
            for px in x0..x1 {
                let i = (py * self.width + px) * 4;
                for c in 0..3 {
                    let d = self.data[i + c] as f64;
                    self.data[i + c] = (d + (rgba[c] * 255.0 - d) * rgba[3]).round() as u8;
                }
            }
        }
    }
}

fn texel(img: &Image, x: i64, y: i64) -> [f64; 4] {
    let x = x.clamp(0, img.width as i64 - 1) as usize;
    let y = y.clamp(0, img.height as i64 - 1) as usize;
    let i = (y * img.width as usize + x) * 4;
    let p = &img.rgba[i..i + 4];
    [p[0] as f64, p[1] as f64, p[2] as f64, p[3] as f64]
}

fn sample(img: &Image, x: f64, y: f64) -> [f64; 4] {
    let (fx, fy) = (x.floor(), y.floor());
    let (tx, ty) = (x - fx, y - fy);
    let (ix, iy) = (fx as i64, fy as i64);
    let a = texel(img, ix, iy);
    let b = texel(img, ix + 1, iy);
    let c = texel(img, ix, iy + 1);
    let d = texel(img, ix + 1, iy + 1);
    let mut out = [0.0; 4];
    for k in 0..4 {
        let top = a[k] + (b[k] - a[k]) * tx;
        let bottom = c[k] + (d[k] - c[k]) * tx;
        out[k] = top + (bottom - top) * ty;
    }
    out
}

const FULL: [f64; 4] = [0.0, 0.0, 1.0, 1.0];

/// What a loading screen shows, from the game tables.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LoadingScreen {
    /// One image for the whole screen (older loading screens).
    pub wide: Option<u32>,
    /// Skin textures (LoadingScreenSkin.FileDataID[10]).
    pub skin: Vec<u32>,
    pub main: Option<u32>,
    pub logo: Option<u32>,
}

fn table(gd: &GameData, id: u32) -> Option<db2::Table> {
    db2::Table::parse(&gd.read_file(id)?).ok()
}

fn field(r: &db2::Record, i: usize) -> u32 {
    r.values
        .get(i)
        .and_then(|v| v.first())
        .copied()
        .unwrap_or(0)
}

/// The loading screen of a map (Map.db2 ID).
pub fn for_map(gd: &GameData, map_id: u32) -> Option<LoadingScreen> {
    let maps = table(gd, MAP_DB2)?;
    let map = maps.records.iter().find(|r| r.id == map_id)?;
    let screen_id = field(map, MAP_LOADING_SCREEN);
    let screens = table(gd, LOADING_SCREENS_DB2)?;
    let row = screens.records.iter().find(|r| r.id == screen_id)?;
    let nonzero = |v: u32| (v != 0).then_some(v);
    let mut out = LoadingScreen {
        wide: nonzero(field(row, 2)).or(nonzero(field(row, 1))),
        main: nonzero(field(row, 4)),
        logo: nonzero(field(row, 5)),
        skin: Vec::new(),
    };
    let skin_id = field(row, 3);
    if skin_id != 0
        && let Some(skins) = table(gd, LOADING_SCREEN_SKIN_DB2)
        && let Some(skin) = skins.records.iter().find(|r| r.id == skin_id)
    {
        out.skin = skin.values.first().cloned().unwrap_or_default();
    }
    Some(out)
}

/// Draw a loading screen at `width` x `height` pixels, with the loading
/// bar at `progress` (0..1).
pub fn render(
    gd: &GameData,
    screen: &LoadingScreen,
    width: usize,
    height: usize,
    progress: f64,
) -> Rgba {
    let mut out = Rgba::new(width, height, [0, 0, 0]);
    let image = |id: u32| gd.image(&ImageKey::Id(id));
    let (w, h) = (width as f64, height as f64);
    let bar_y;
    if let (Some(main), false) = (screen.main, screen.skin.is_empty()) {
        // Skinned screen: parchment, side backgrounds, main image, logo.
        let skin = |i: usize| screen.skin.get(i).copied().filter(|&v| v != 0);
        // Skin textures: 0 outer border strip, 1 inner border strip, 2
        // parchment fill, 3 bookend, 4 left background, 5 right background
        // (0 means mirror the left one), 6 side fill, 7 gradient.
        let tile_x = |out: &mut Rgba, img: &Image, y: f64, th: f64, uv: [f64; 4]| {
            let tw = th * img.width as f64 / img.height as f64;
            let mut tx = 0.0;
            while tx < w {
                out.draw(img, tx, y, tw, th, uv);
                tx += tw;
            }
        };
        if let Some(fill) = skin(2).and_then(image) {
            tile_x(&mut out, &fill, 0.0, h / 2.0, FULL);
            tile_x(&mut out, &fill, h / 2.0, h / 2.0, FULL);
        }
        // The main image is 16:9 with transparent bands; it covers the
        // full height. The art band is 15.7% to 84.2% of the height.
        let main_img = image(main);
        let aspect = main_img
            .as_ref()
            .map(|i| i.width as f64 / i.height as f64)
            .unwrap_or(16.0 / 9.0);
        let mw = h * aspect;
        let (mx0, mx1) = ((w - mw) / 2.0, (w + mw) / 2.0);
        let (my0, my1) = (0.157 * h, 0.842 * h);
        // Side backgrounds (maps on parchment) end at the main image.
        if let Some(left) = skin(4).and_then(image) {
            let lw = h * left.width as f64 / left.height as f64;
            out.draw(&left, mx0 - lw + 0.02 * h, 0.0, lw, h, FULL);
            let right = skin(5).and_then(image);
            match right {
                Some(r) => {
                    let rw = h * r.width as f64 / r.height as f64;
                    out.draw(&r, mx1 - 0.02 * h, 0.0, rw, h, FULL);
                }
                None => out.draw(&left, mx1 - 0.02 * h, 0.0, lw, h, [1.0, 0.0, 0.0, 1.0]),
            }
        }
        // Parchment bands above and below the art, with border strips.
        if let Some(fill) = skin(2).and_then(image) {
            tile_x(&mut out, &fill, 0.0, my0, FULL);
            tile_x(&mut out, &fill, my1, h - my1, FULL);
        }
        if let Some(outer) = skin(0).and_then(image) {
            let th = 0.035 * h;
            tile_x(&mut out, &outer, 0.0, th, [0.0, 1.0, 1.0, 0.0]);
            tile_x(&mut out, &outer, h - th, th, FULL);
        }
        if let Some(img) = &main_img {
            out.draw(img, mx0, 0.0, mw, h, FULL);
        }
        if let Some(inner) = skin(1).and_then(image) {
            let th = 0.026 * h;
            tile_x(&mut out, &inner, my0 - th * 0.6, th, FULL);
            tile_x(&mut out, &inner, my1 - th * 0.4, th, [0.0, 1.0, 1.0, 0.0]);
        }
        // Bookends frame the art band.
        if let Some(book) = skin(3).and_then(image) {
            let bh = my1 - my0;
            let bw = bh * book.width as f64 / book.height as f64;
            out.draw(&book, mx0 - bw * 0.95, my0, bw, bh, FULL);
            out.draw(&book, mx1 - bw * 0.05, my0, bw, bh, [1.0, 0.0, 0.0, 1.0]);
        }
        if let Some(logo) = screen.logo.and_then(image) {
            // Centred at 29% of the art's width, 13.5% of the height.
            let lh = 0.27 * h;
            let lw = lh * logo.width as f64 / logo.height as f64;
            out.draw(
                &logo,
                mx0 + 0.29 * mw - lw / 2.0,
                0.135 * h - lh / 2.0,
                lw,
                lh,
                FULL,
            );
        }
        bar_y = 0.91 * h;
    } else {
        // One wide image, fit to the height.
        if let Some(img) = screen.wide.or(screen.main).and_then(image) {
            let iw = h * img.width as f64 / img.height as f64;
            out.draw(&img, (w - iw) / 2.0, 0.0, iw, h, FULL);
        }
        bar_y = 0.92 * h;
    }
    // Loading bar.
    // Centred, about 0.79 of the height wide.
    let (bw, bh) = (0.79 * h, 0.028 * h);
    let bx = (w - bw) / 2.0;
    if let Some(bg) = image(BAR_BACKGROUND) {
        out.draw(&bg, bx, bar_y, bw, bh, FULL);
    } else {
        out.fill(bx, bar_y, bw, bh, [0.0, 0.0, 0.0, 0.8]);
    }
    if let Some(fill) = image(BAR_FILL) {
        let p = progress.clamp(0.0, 1.0);
        out.draw(&fill, bx, bar_y, bw * p, bh, [0.0, 0.0, p, 1.0]);
    }
    if let Some(border) = image(BAR_BORDER) {
        out.draw(
            &border,
            bx - 0.01 * w,
            bar_y - 0.3 * bh,
            bw + 0.02 * w,
            bh * 1.6,
            FULL,
        );
    }
    out
}
