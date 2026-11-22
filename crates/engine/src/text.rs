//! Fonts, text measurement and line breaking for font strings.
//!
//! Fonts are read from the game data when it is available. Without game
//! data, glyph widths are estimated, so layout still works in tests.

use std::collections::HashMap;

use ab_glyph::{Font, FontArc, PxScale, ScaleFont};

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
}

/// Resolve WoW escape sequences: colours, links, textures and newlines.
fn parse_markup(text: &str) -> Vec<Glyph> {
    let mut out = Vec::with_capacity(text.len());
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c != '|' || i + 1 >= chars.len() {
            out.push(Glyph {
                c,
                color: colors.last().copied(),
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
                });
                i += 2;
            }
            'H' => {
                // |Hlinkdata|h shown text |h
                let rest = &chars[i + 2..];
                let skip = rest
                    .windows(2)
                    .position(|w| w == ['|', 'h'])
                    .map(|p| p + 4)
                    .unwrap_or(rest.len() + 2);
                i += skip;
            }
            'h' => i += 2,
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
                    });
                }
                i += skip;
            }
            _ => {
                out.push(Glyph {
                    c,
                    color: colors.last().copied(),
                });
                i += 1;
            }
        }
    }
    out
}

/// Measures glyph advances for one font and size.
pub struct Measurer {
    font: Option<FontArc>,
    height: f64,
}

impl Measurer {
    pub fn new(st: &mut State, font: &ResolvedFont) -> Measurer {
        let gd = st.gamedata.clone();
        let f = st.fonts.get(&font.path, gd.as_deref());
        Measurer {
            font: f,
            height: font.height,
        }
    }

    pub fn char_width(&self, c: char) -> f64 {
        match &self.font {
            Some(font) => {
                let scaled = font.as_scaled(PxScale::from(self.height as f32));
                scaled.h_advance(font.glyph_id(c)) as f64
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

    pub fn width(&self, s: &str) -> f64 {
        s.chars().map(|c| self.char_width(c)).sum()
    }
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
    let glyphs = parse_markup(text);
    let mut lines: Vec<Vec<Glyph>> = Vec::new();
    let mut current: Vec<Glyph> = Vec::new();
    let mut width = 0.0;
    let mut last_space: Option<usize> = None;
    let wrap = max_width.filter(|w| *w > 0.0 && word_wrap);

    for g in glyphs {
        if g.c == '\n' {
            lines.push(std::mem::take(&mut current));
            width = 0.0;
            last_space = None;
            continue;
        }
        let w = m.char_width(g.c);
        if let Some(maxw) = wrap
            && width + w > maxw + 0.01
            && !current.is_empty()
            && g.c != ' '
        {
            match last_space {
                Some(sp) if !non_space_wrap || sp > 0 => {
                    let rest = current.split_off(sp + 1);
                    current.pop();
                    lines.push(std::mem::take(&mut current));
                    current = rest;
                }
                _ => lines.push(std::mem::take(&mut current)),
            }
            width = current.iter().map(|g| m.char_width(g.c)).sum();
            last_space = None;
        }
        if g.c == ' ' {
            last_space = Some(current.len());
        }
        current.push(g);
        width += w;
    }
    lines.push(current);

    let mut truncated = false;
    if max_lines > 0 && lines.len() > max_lines {
        lines.truncate(max_lines);
        truncated = true;
    }
    let mut out: Vec<Line> = lines
        .into_iter()
        .map(|mut l| {
            while l.last().is_some_and(|g| g.c == ' ') {
                l.pop();
            }
            to_line(m, &l)
        })
        .collect();
    // Without wrapping, a line wider than the box ends in an ellipsis.
    let limit = max_width.filter(|w| *w > 0.0);
    if let Some(maxw) = limit {
        for line in out.iter_mut() {
            if line.width > maxw + 0.5 {
                *line = ellipsize(m, line, maxw);
            }
        }
    }
    if truncated && let Some(last) = out.last_mut() {
        let maxw = limit.unwrap_or(f64::MAX);
        let mut l = last.clone();
        l.runs.push(Run {
            text: "...".into(),
            color: None,
        });
        l.width += m.width("...");
        *last = if l.width > maxw {
            ellipsize(m, &l, maxw)
        } else {
            l
        };
    }
    out
}

fn to_line(m: &Measurer, glyphs: &[Glyph]) -> Line {
    let mut runs: Vec<Run> = Vec::new();
    let mut width = 0.0;
    for g in glyphs {
        width += m.char_width(g.c);
        match runs.last_mut() {
            Some(r) if r.color == g.color => r.text.push(g.c),
            _ => runs.push(Run {
                text: g.c.to_string(),
                color: g.color,
            }),
        }
    }
    Line { runs, width }
}

fn ellipsize(m: &Measurer, line: &Line, max_width: f64) -> Line {
    let budget = max_width - m.width("...");
    let mut out = Line {
        runs: Vec::new(),
        width: 0.0,
    };
    'outer: for run in &line.runs {
        let mut text = String::new();
        for c in run.text.chars() {
            let w = m.char_width(c);
            if out.width + w > budget {
                if !text.is_empty() {
                    out.runs.push(Run {
                        text,
                        color: run.color,
                    });
                }
                break 'outer;
            }
            out.width += w;
            text.push(c);
        }
        if !text.is_empty() {
            out.runs.push(Run {
                text,
                color: run.color,
            });
        }
    }
    out.runs.push(Run {
        text: "...".into(),
        color: None,
    });
    out.width += m.width("...");
    out
}

/// Lines of a font string, wrapped at `wrap_width` (own units) if given.
pub fn font_string_lines(
    st: &mut State,
    id: FrameId,
    wrap_width: Option<f64>,
) -> (ResolvedFont, Vec<Line>) {
    let font = resolve_font(st, id);
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
    let lines = layout_lines(&m, &text, wrap_width, word_wrap, non_space, max_lines);
    (font, lines)
}

/// Size of a font string's text: widest line and total height.
pub fn string_size(st: &mut State, id: FrameId, wrap_width: Option<f64>) -> (f64, f64) {
    let (font, lines) = font_string_lines(st, id, wrap_width);
    if lines.is_empty() {
        return (0.0, 0.0);
    }
    let width = lines.iter().map(|l| l.width).fold(0.0, f64::max);
    let n = lines.len() as f64;
    let height = n * font.height + (n - 1.0) * font.spacing;
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
    fn wraps_at_spaces() {
        let m = measurer();
        let lines = layout_lines(&m, "aaa bbb ccc", Some(40.0), true, false, 0);
        assert_eq!(plain(&lines), vec!["aaa bbb", "ccc"]);
        let lines = layout_lines(&m, "aaa bbb ccc", None, true, false, 0);
        assert_eq!(plain(&lines), vec!["aaa bbb ccc"]);
    }

    #[test]
    fn limits_lines_with_ellipsis() {
        let m = measurer();
        let lines = layout_lines(&m, "aaa bbb ccc", Some(40.0), true, false, 1);
        assert_eq!(lines.len(), 1);
        assert!(plain(&lines)[0].ends_with("..."));
    }
}
