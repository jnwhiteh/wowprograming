//! Fonts, text measurement and line breaking for font strings.
//!
//! Fonts are read from the game data when it is available. Without game
//! data, glyph widths are estimated, so layout still works in tests.

use std::collections::HashMap;

use ab_glyph::{Font, FontArc, PxScale};

use crate::gamedata::GameData;
use crate::state::{FrameId, State};

pub const DEFAULT_FONT: &str = "Fonts\\FRIZQT__.TTF";

#[derive(Default)]
pub struct FontCache {
    fonts: HashMap<String, Option<FontArc>>,
}

impl FontCache {
    pub fn get(&mut self, path: &str, gamedata: Option<&GameData>) -> Option<FontArc> {
        let key = path.replace('/', "\\").to_ascii_lowercase();
        self.fonts
            .entry(key)
            .or_insert_with(|| {
                let bytes = gamedata?.read_path(path)?;
                FontArc::try_from_vec(bytes).ok()
            })
            .clone()
    }
}

/// Font attributes after following the chain of font objects.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedFont {
    pub path: String,
    pub height: f64,
    /// Height of a line: the font height rounded down to whole physical
    /// pixels, in the font string's units. The client measures and draws
    /// text at this size.
    pub line_height: f64,
    /// Physical pixels per unit of the font string.
    pub pixels_per_unit: f64,
    pub flags: String,
    pub color: [f32; 4],
    pub shadow_color: [f32; 4],
    pub shadow_offset: (f64, f64),
    pub justify_h: String,
    pub justify_v: String,
    pub spacing: f64,
}

pub fn resolve_font(st: &State, id: FrameId) -> ResolvedFont {
    let mut out = ResolvedFont {
        path: String::new(),
        height: 0.0,
        line_height: 0.0,
        pixels_per_unit: 0.0,
        flags: String::new(),
        color: [1.0; 4],
        shadow_color: [0.0, 0.0, 0.0, 0.0],
        shadow_offset: (0.0, 0.0),
        justify_h: String::new(),
        justify_v: String::new(),
        spacing: -1.0,
    };
    let (mut has_flags, mut has_color, mut has_shadow_c, mut has_shadow_o) =
        (false, false, false, false);
    let mut current = Some(id);
    let mut depth = 0;
    while let Some(i) = current {
        depth += 1;
        if depth > 16 {
            break;
        }
        let Some(f) = st.frames.get(i).and_then(|f| f.font.as_deref()) else {
            break;
        };
        if out.path.is_empty()
            && let Some(p) = &f.path
        {
            out.path = p.clone();
        }
        if out.height <= 0.0
            && let Some(h) = f.height
        {
            out.height = h;
        }
        if !has_flags && let Some(v) = &f.flags {
            out.flags = v.clone();
            has_flags = true;
        }
        if !has_color && let Some(c) = f.color {
            out.color = c;
            has_color = true;
        }
        if !has_shadow_c && let Some(c) = f.shadow_color {
            out.shadow_color = c;
            has_shadow_c = true;
        }
        if !has_shadow_o && let Some(o) = f.shadow_offset {
            out.shadow_offset = o;
            has_shadow_o = true;
        }
        if out.justify_h.is_empty()
            && let Some(j) = &f.justify_h
        {
            out.justify_h = j.clone();
        }
        if out.justify_v.is_empty()
            && let Some(j) = &f.justify_v
        {
            out.justify_v = j.clone();
        }
        if out.spacing < 0.0
            && let Some(s) = f.spacing
        {
            out.spacing = s;
        }
        current = f.font_object;
    }
    if out.path.is_empty() {
        out.path = DEFAULT_FONT.into();
    }
    if out.height <= 0.0 {
        out.height = 12.0;
    }
    if out.justify_h.is_empty() {
        out.justify_h = "CENTER".into();
    }
    if out.justify_v.is_empty() {
        out.justify_v = "MIDDLE".into();
    }
    if out.spacing < 0.0 {
        out.spacing = 0.0;
    }
    out
}

/// A piece of text with one colour. `color` is `None` for the font's colour.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub text: String,
    pub color: Option<[f32; 4]>,
    /// The hyperlink data (`|Hdata|h`) of this run's text, if it is a link.
    pub link: Option<String>,
    /// How far the pen moves after each character (kerning included), in
    /// the font string's units, or in screen units inside a scene.
    pub advances: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub runs: Vec<Run>,
    pub width: f64,
}

/// One character of text after escape sequences are resolved.
#[derive(Clone, Copy)]
struct Glyph {
    c: char,
    color: Option<[f32; 4]>,
    /// 1 + the index of the hyperlink this character belongs to, or 0.
    link: u32,
}

/// Resolve WoW escape sequences: colours, links, textures and newlines.
#[cfg(test)]
fn parse_markup(text: &str) -> Vec<Glyph> {
    parse_markup_links(text).0
}

/// Like [`parse_markup`], with the hyperlink data that the glyphs' `link`
/// fields point to.
fn parse_markup_links(text: &str) -> (Vec<Glyph>, Vec<String>) {
    let mut out = Vec::with_capacity(text.len());
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut links: Vec<String> = Vec::new();
    let mut link: u32 = 0;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\r' {
            // "\r\n" is one line break; a lone "\r" is one too.
            if chars.get(i + 1) != Some(&'\n') {
                out.push(Glyph {
                    c: '\n',
                    color: None,
                    link: 0,
                });
            }
            i += 1;
            continue;
        }
        if c != '|' || i + 1 >= chars.len() {
            out.push(Glyph {
                c,
                color: colors.last().copied(),
                link,
            });
            i += 1;
            continue;
        }
        let code = chars[i + 1];
        match code {
            '|' => {
                out.push(Glyph {
                    c: '|',
                    color: colors.last().copied(),
                    link,
                });
                i += 2;
            }
            'c' => {
                let hex: String = chars.iter().skip(i + 2).take(8).collect();
                if chars.get(i + 2) == Some(&'n') {
                    // Named colour: |cnNAME:text|r. Colour names need the
                    // GlobalColor table; keep the font colour.
                    let end = chars[i..].iter().position(|&c| c == ':').map(|p| i + p + 1);
                    colors.push(colors.last().copied().unwrap_or([1.0; 4]));
                    i = end.unwrap_or(chars.len());
                } else if hex.len() == 8 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    let v = u32::from_str_radix(&hex, 16).unwrap_or(0xffffffff);
                    let ch = |s: u32| ((v >> s) & 0xff) as f32 / 255.0;
                    colors.push([ch(16), ch(8), ch(0), ch(24)]);
                    i += 10;
                } else {
                    i += 2;
                }
            }
            'r' => {
                colors.pop();
                i += 2;
            }
            'n' => {
                out.push(Glyph {
                    c: '\n',
                    color: None,
                    link: 0,
                });
                i += 2;
            }
            'H' => {
                // |Hlinkdata|h shown text |h
                let rest = &chars[i + 2..];
                let end = rest.windows(2).position(|w| w == ['|', 'h']);
                let skip = end.map(|p| p + 4).unwrap_or(rest.len() + 2);
                if let Some(p) = end {
                    links.push(rest[..p].iter().collect());
                    link = links.len() as u32;
                }
                i += skip;
            }
            'h' => {
                link = 0;
                i += 2;
            }
            '4' => {
                // |4singular:plural; picks the form by the number before
                // it ("1 |4minute:minutes;" is "1 minute").
                let rest = &chars[i + 2..];
                let Some(end) = rest.iter().position(|&c| c == ';') else {
                    i += 2;
                    continue;
                };
                let body: String = rest[..end].iter().collect();
                let (one, many) = body.split_once(':').unwrap_or((&body, &body));
                let digits: String = out
                    .iter()
                    .rev()
                    .map(|g| g.c)
                    .skip_while(|c| !c.is_ascii_digit())
                    .take_while(|c| c.is_ascii_digit() || *c == ',' || *c == '.')
                    .collect();
                let form = if digits == "1" { one } else { many };
                for c in form.chars() {
                    out.push(Glyph {
                        c,
                        color: colors.last().copied(),
                        link,
                    });
                }
                i += 2 + end + 1;
            }
            'T' | 'A' | 'K' => {
                // Inline texture, atlas or protected string: shown as a space.
                let end_code = code.to_ascii_lowercase();
                let rest = &chars[i + 2..];
                let skip = rest
                    .windows(2)
                    .position(|w| w == ['|', end_code])
                    .map(|p| p + 4)
                    .unwrap_or(rest.len() + 2);
                if code != 'K' {
                    out.push(Glyph {
                        c: ' ',
                        color: None,
                        link: 0,
                    });
                }
                i += skip;
            }
            _ => {
                out.push(Glyph {
                    c,
                    color: colors.last().copied(),
                    link,
                });
                i += 1;
            }
        }
    }
    (out, links)
}

/// Measures text for one font and size, as the client does.
///
/// The client's rules, found by comparing captured string widths with the
/// font files (see `docs/status.md`):
///
/// - Glyphs are drawn at the font height in whole physical pixels
///   (rounded to the nearest pixel).
/// - Each glyph advances by its font advance at that size, rounded up to a
///   whole pixel.
/// - Kerning pairs move the next glyph, truncated toward zero to whole
///   pixels.
/// - A string ends at the right edge of the last glyph's outline, rounded
///   up, plus one pixel. A glyph without an outline (a space) ends at its
///   advance.
/// - A shadow to the right widens the string by its offset in whole
///   pixels (1 unit at 1.2 pixels per unit adds 1 pixel).
pub struct Measurer {
    font: Option<FontArc>,
    height: f64,
    /// Extra width from the shadow, in string units.
    shadow: f64,
    /// Physical pixels per unit. Zero measures in units, unrounded.
    pixels_per_unit: f64,
}

/// Glyph scale for an em size, as FreeType sizes fonts. ab_glyph's
/// `PxScale` is the height from descent to ascent instead.
pub fn em_scale(font: &FontArc, em: f32) -> PxScale {
    match font.units_per_em() {
        Some(units) if units > 0.0 => PxScale::from(em * font.height_unscaled() / units),
        _ => PxScale::from(em),
    }
}

/// A font height rounded to whole physical pixels, given the physical
/// pixels per unit. The client rounds to the nearest pixel (size 13 at 1.2
/// pixels per unit is drawn at 16 pixels).
pub fn snap_height(height: f64, pixels_per_unit: f64) -> f64 {
    if pixels_per_unit <= 0.0 || height * pixels_per_unit < 1.0 {
        return height;
    }
    (height * pixels_per_unit + 0.5).floor() / pixels_per_unit
}

impl Measurer {
    pub fn new(st: &mut State, font: &ResolvedFont) -> Measurer {
        let gd = st.gamedata.clone();
        let f = st.fonts.get(&font.path, gd.as_deref());
        let ppu = font.pixels_per_unit;
        let dx = font.shadow_offset.0.max(0.0);
        let shadow = if ppu > 0.0 {
            (dx * ppu + 0.5).floor() / ppu
        } else {
            dx
        };
        Measurer {
            font: f,
            height: font.line_height,
            shadow,
            pixels_per_unit: ppu,
        }
    }

    /// Font units to physical pixels (or to string units, unrounded,
    /// without a pixel scale).
    fn px_per_font_unit(&self, font: &FontArc) -> f64 {
        let upem = font.units_per_em().unwrap_or(1000.0).max(1.0) as f64;
        let ppem = if self.pixels_per_unit > 0.0 {
            (self.height * self.pixels_per_unit).round()
        } else {
            self.height
        };
        ppem / upem
    }

    /// Pixels to string units, rounded with `round` first when there is a
    /// pixel scale.
    fn units(&self, px: f64, round: fn(f64) -> f64) -> f64 {
        if self.pixels_per_unit > 0.0 {
            round(px - 1e-6) / self.pixels_per_unit
        } else {
            px
        }
    }

    /// How far the pen moves after `c`.
    pub fn advance(&self, c: char) -> f64 {
        match &self.font {
            Some(font) => {
                let px =
                    font.h_advance_unscaled(font.glyph_id(c)) as f64 * self.px_per_font_unit(font);
                self.units(px, f64::ceil)
            }
            None => {
                if c == ' ' {
                    self.height * 0.3
                } else {
                    self.height * 0.55
                }
            }
        }
    }

    /// Kerning between `a` and the following `b`.
    pub fn kern(&self, a: char, b: char) -> f64 {
        let Some(font) = &self.font else { return 0.0 };
        let k = font.kern_unscaled(font.glyph_id(a), font.glyph_id(b)) as f64;
        if k == 0.0 {
            return 0.0;
        }
        let px = k * self.px_per_font_unit(font);
        if self.pixels_per_unit > 0.0 {
            px.trunc() / self.pixels_per_unit
        } else {
            px
        }
    }

    /// Where a string ends after its last glyph `c`, from the pen position.
    pub fn ink_end(&self, c: char) -> f64 {
        let Some(font) = &self.font else {
            return self.advance(c);
        };
        match font.outline(font.glyph_id(c)) {
            Some(o) => {
                let px = o.bounds.max.x as f64 * self.px_per_font_unit(font);
                if self.pixels_per_unit > 0.0 {
                    // A small tolerance: outlines that end on a pixel edge
                    // do not take another pixel.
                    ((px - 0.02).ceil() + 1.0) / self.pixels_per_unit
                } else {
                    px
                }
            }
            None => self.advance(c),
        }
    }

    pub fn width(&self, s: &str) -> f64 {
        let chars: Vec<char> = s.chars().collect();
        chars_width(self, &chars)
    }
}

/// Pen advance after each character, including kerning with the next one.
fn pen_advances(m: &Measurer, chars: &[char]) -> Vec<f64> {
    (0..chars.len())
        .map(|i| {
            let k = chars.get(i + 1).map_or(0.0, |&n| m.kern(chars[i], n));
            m.advance(chars[i]) + k
        })
        .collect()
}

/// Width of a sequence of characters, as the client measures a string.
fn chars_width(m: &Measurer, chars: &[char]) -> f64 {
    let Some((&last, rest)) = chars.split_last() else {
        return 0.0;
    };
    let pen: f64 = pen_advances(m, chars)[..rest.len()].iter().sum();
    pen + m.ink_end(last) + m.shadow
}

fn glyphs_width(m: &Measurer, glyphs: &[Glyph]) -> f64 {
    let chars: Vec<char> = glyphs.iter().map(|g| g.c).collect();
    chars_width(m, &chars)
}

/// Break text into lines. `max_width` of `None` means no wrapping.
/// `max_lines` of 0 means no limit.
pub fn layout_lines(
    m: &Measurer,
    text: &str,
    max_width: Option<f64>,
    word_wrap: bool,
    non_space_wrap: bool,
    max_lines: usize,
) -> Vec<Line> {
    let (glyphs, links) = parse_markup_links(text);
    let mut lines: Vec<Vec<Glyph>> = Vec::new();
    let mut current: Vec<Glyph> = Vec::new();
    // Pen position after the last glyph of `current`.
    let mut pen = 0.0;
    let mut last_space: Option<usize> = None;
    let wrap = max_width.filter(|w| *w > 0.0 && word_wrap);
    let pen_of = |glyphs: &[Glyph]| -> f64 {
        let chars: Vec<char> = glyphs.iter().map(|g| g.c).collect();
        pen_advances(m, &chars).iter().sum()
    };

    for g in glyphs {
        if g.c == '\n' {
            lines.push(std::mem::take(&mut current));
            pen = 0.0;
            last_space = None;
            continue;
        }
        let kern = current.last().map_or(0.0, |p| m.kern(p.c, g.c));
        if let Some(maxw) = wrap
            && pen + kern + m.ink_end(g.c) + m.shadow > maxw + 0.01
            && !current.is_empty()
            && g.c != ' '
        {
            // Break at the last space. Without one, break inside the word
            // only with non-space wrap; otherwise the word overflows and is
            // cut with an ellipsis below, as in the client.
            let broke = match last_space {
                Some(sp) => {
                    let rest = current.split_off(sp + 1);
                    current.pop();
                    while current.last().is_some_and(|g| g.c == ' ') {
                        current.pop();
                    }
                    lines.push(std::mem::take(&mut current));
                    current = rest;
                    true
                }
                None if non_space_wrap => {
                    lines.push(std::mem::take(&mut current));
                    true
                }
                None => false,
            };
            if broke {
                pen = pen_of(&current);
                last_space = None;
            }
        }
        if g.c == ' ' {
            last_space = Some(current.len());
        }
        pen += current.last().map_or(0.0, |p| m.kern(p.c, g.c)) + m.advance(g.c);
        current.push(g);
    }
    lines.push(current);

    let mut truncated = false;
    if max_lines > 0 && lines.len() > max_lines {
        lines.truncate(max_lines);
        truncated = true;
    }
    let mut out: Vec<Line> = lines.iter().map(|l| to_line(m, l, &links)).collect();
    // Without wrapping, a line wider than the box ends in an ellipsis.
    let limit = max_width.filter(|w| *w > 0.0);
    if let Some(maxw) = limit {
        for (line, glyphs) in out.iter_mut().zip(&lines) {
            if line.width > maxw + 0.5 {
                *line = ellipsize(m, glyphs, maxw, &links);
            }
        }
    }
    if truncated && let Some(last) = lines.last() {
        let maxw = limit.unwrap_or(f64::MAX);
        let mut glyphs = last.clone();
        while glyphs.last().is_some_and(|g| g.c == ' ') {
            glyphs.pop();
        }
        glyphs.extend(dots());
        let l = to_line(m, &glyphs, &links);
        let n = out.len();
        out[n - 1] = if l.width > maxw {
            ellipsize(m, last, maxw, &links)
        } else {
            l
        };
    }
    out
}

/// The ellipsis that ends cut text, in the font's colour.
fn dots() -> [Glyph; 3] {
    [Glyph {
        c: '.',
        color: None,
        link: 0,
    }; 3]
}

fn to_line(m: &Measurer, glyphs: &[Glyph], links: &[String]) -> Line {
    let chars: Vec<char> = glyphs.iter().map(|g| g.c).collect();
    let advances = pen_advances(m, &chars);
    let mut runs: Vec<Run> = Vec::new();
    let mut run_links: Vec<u32> = Vec::new();
    for (g, adv) in glyphs.iter().zip(advances) {
        match runs.last_mut() {
            Some(r) if r.color == g.color && run_links.last() == Some(&g.link) => {
                r.text.push(g.c);
                r.advances.push(adv);
            }
            _ => {
                runs.push(Run {
                    text: g.c.to_string(),
                    color: g.color,
                    link: (g.link > 0)
                        .then(|| links.get(g.link as usize - 1).cloned())
                        .flatten(),
                    advances: vec![adv],
                });
                run_links.push(g.link);
            }
        }
    }
    Line {
        runs,
        width: chars_width(m, &chars),
    }
}

/// The longest start of `glyphs` that fits in `max_width` with an ellipsis
/// after it.
fn ellipsize(m: &Measurer, glyphs: &[Glyph], max_width: f64, links: &[String]) -> Line {
    let mut keep = glyphs.len();
    loop {
        let mut g: Vec<Glyph> = glyphs[..keep].to_vec();
        g.extend(dots());
        if keep == 0 || glyphs_width(m, &g) <= max_width + 0.01 {
            let mut line = to_line(m, &g, links);
            // The ellipsis is its own run, so callers can find it.
            if let Some(last) = line.runs.last_mut()
                && last.text.len() > 3
                && last.text.ends_with("...")
            {
                let n = last.text.len() - 3;
                let text = last.text.split_off(n);
                let advances = last.advances.split_off(last.advances.len() - 3);
                line.runs.push(Run {
                    text,
                    color: None,
                    link: None,
                    advances,
                });
            }
            return line;
        }
        keep -= 1;
    }
}

/// Lines of a font string, wrapped at `wrap_width` (own units) if given.
pub fn font_string_lines(
    st: &mut State,
    id: FrameId,
    wrap_width: Option<f64>,
) -> (ResolvedFont, Vec<Line>) {
    font_string_lines_in(st, id, wrap_width, None)
}

/// Like [`font_string_lines`], keeping only the lines that fit in
/// `max_height` (in the font string's units); the last kept line ends in an
/// ellipsis when text was cut.
pub fn font_string_lines_in(
    st: &mut State,
    id: FrameId,
    wrap_width: Option<f64>,
    max_height: Option<f64>,
) -> (ResolvedFont, Vec<Line>) {
    let mut font = resolve_font(st, id);
    let ppu = crate::layout::effective_scale(st, id) * st.pixel_scale;
    font.line_height = snap_height(font.height, ppu);
    font.pixels_per_unit = ppu;
    let (text, max_lines, word_wrap, non_space) = match st.frames[id].text.as_deref() {
        Some(t) => (
            t.text.clone(),
            t.max_lines.max(0) as usize,
            t.word_wrap,
            t.non_space_wrap,
        ),
        None => (String::new(), 0, true, false),
    };
    if text.is_empty() {
        return (font, Vec::new());
    }
    let m = Measurer::new(st, &font);
    let mut max_lines = max_lines;
    if let Some(h) = max_height.filter(|h| *h > 0.0)
        && font.line_height > 0.0
    {
        let fit = (((h + font.spacing) / (font.line_height + font.spacing)) + 0.01).floor();
        let fit = (fit as usize).max(1);
        max_lines = if max_lines == 0 {
            fit
        } else {
            max_lines.min(fit)
        };
    }
    let lines = layout_lines(&m, &text, wrap_width, word_wrap, non_space, max_lines);
    (font, lines)
}

/// Width of `prefix` in the font of a widget, and its line height, in the
/// widget's units. Edit boxes use it to place the cursor.
pub fn prefix_width(st: &mut State, id: FrameId, prefix: &str) -> (f64, f64) {
    let mut font = resolve_font(st, id);
    let ppu = crate::layout::effective_scale(st, id) * st.pixel_scale;
    font.line_height = snap_height(font.height, ppu);
    font.pixels_per_unit = ppu;
    let m = Measurer::new(st, &font);
    let line = prefix.rsplit('\n').next().unwrap_or("");
    let chars: Vec<char> = line.chars().collect();
    // The cursor stands at the pen position after the prefix.
    let width: f64 = pen_advances(&m, &chars).iter().sum();
    (width, font.line_height)
}

/// Size of a font string's text: widest line and total height.
pub fn string_size(st: &mut State, id: FrameId, wrap_width: Option<f64>) -> (f64, f64) {
    let (font, lines) = font_string_lines(st, id, wrap_width);
    if lines.is_empty() {
        return (0.0, 0.0);
    }
    let width = lines.iter().map(|l| l.width).fold(0.0, f64::max);
    let n = lines.len() as f64;
    let height = n * font.line_height + (n - 1.0) * font.spacing;
    (width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.runs.iter().map(|r| r.text.as_str()).collect())
            .collect()
    }

    fn measurer() -> Measurer {
        // Without a font, letters are 5.5 wide and spaces 3 at height 10.
        Measurer {
            font: None,
            height: 10.0,
            shadow: 0.0,
            pixels_per_unit: 0.0,
        }
    }

    #[test]
    fn parses_color_codes_and_links() {
        let g = parse_markup("a|cffff0000b|rc|Hspell:1|h[X]|h||");
        let s: String = g.iter().map(|g| g.c).collect();
        assert_eq!(s, "abc[X]|");
        assert_eq!(g[1].color, Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(g[2].color, None);
    }

    #[test]
    fn picks_singular_or_plural() {
        let text = |s: &str| parse_markup(s).iter().map(|g| g.c).collect::<String>();
        assert_eq!(text("1 |4minute:minutes; left"), "1 minute left");
        assert_eq!(text("21 |4minute:minutes; left"), "21 minutes left");
        assert_eq!(text("|4Min:Min;"), "Min");
    }

    #[test]
    fn wraps_at_spaces() {
        let m = measurer();
        let lines = layout_lines(&m, "aaa bbb ccc", Some(40.0), true, false, 0);
        assert_eq!(plain(&lines), vec!["aaa bbb", "ccc"]);
        let lines = layout_lines(&m, "aaa bbb ccc", None, true, false, 0);
        assert_eq!(plain(&lines), vec!["aaa bbb ccc"]);
    }

    #[test]
    fn snaps_height_to_nearest_pixel() {
        // At 1.2 pixels per unit: 12 -> 14 px, 13 -> 16 px, 8 -> 10 px.
        assert!((snap_height(12.0, 1.2) - 14.0 / 1.2).abs() < 1e-9);
        assert!((snap_height(13.0, 1.2) - 16.0 / 1.2).abs() < 1e-9);
        assert!((snap_height(8.0, 1.2) - 10.0 / 1.2).abs() < 1e-9);
    }

    #[test]
    fn shadow_widens_strings() {
        let mut m = measurer();
        let plain = m.width("ab");
        m.shadow = 1.0;
        assert_eq!(m.width("ab"), plain + 1.0);
        assert_eq!(m.width(""), 0.0);
    }

    #[test]
    fn limits_lines_with_ellipsis() {
        let m = measurer();
        let lines = layout_lines(&m, "aaa bbb ccc", Some(40.0), true, false, 1);
        assert_eq!(lines.len(), 1);
        assert!(plain(&lines)[0].ends_with("..."));
    }
}
