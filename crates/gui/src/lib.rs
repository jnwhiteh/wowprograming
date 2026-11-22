//! Native window for the sandbox: draws the UI scene and offers a console.
//!
//! The scene is drawn into the central panel, scaled to fit. The bottom
//! panel shows sandbox output and has an input line for slash commands and
//! Lua. Key presses go to the sandbox's key bindings while the input line
//! does not have focus.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, FontId, Key, Mesh, Pos2, Rect, Shape,
    TextureHandle, TextureOptions, Vec2, text::LayoutJob,
};
use wowprogramming_engine::gamedata::ImageKey;
use wowprogramming_engine::scene::{Item, Quad, Scene, TextItem};
use wowprogramming_engine::{Engine, Output};

const MAX_LOG_LINES: usize = 2000;

struct LogLine {
    text: String,
    color: Color32,
}

pub struct App {
    engine: Engine,
    textures: HashMap<(ImageKey, bool, bool), Option<TextureHandle>>,
    image_sizes: HashMap<ImageKey, (f32, f32)>,
    fonts_loaded: HashSet<String>,
    log: Vec<LogLine>,
    input: String,
    history: Vec<String>,
    last_tick: Instant,
    scroll_to_bottom: bool,
    show_hover: bool,
}

/// Open the window and run until it closes.
pub fn run(engine: Engine) -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1600.0, 1100.0])
            .with_title("WowProgramming"),
        ..Default::default()
    };
    eframe::run_native(
        "WowProgramming",
        options,
        Box::new(|_cc| Ok(Box::new(App::new(engine)))),
    )
    .map_err(|e| e.to_string())
}

impl App {
    pub fn new(engine: Engine) -> App {
        let mut app = App {
            engine,
            textures: HashMap::new(),
            image_sizes: HashMap::new(),
            fonts_loaded: HashSet::new(),
            log: Vec::new(),
            input: String::new(),
            history: Vec::new(),
            last_tick: Instant::now(),
            scroll_to_bottom: true,
            show_hover: false,
        };
        app.push_log(
            "Press P to open the spellbook. Type /dump <expr>, /run <code> or Lua here.".into(),
            Color32::LIGHT_BLUE,
        );
        app.drain_output();
        app
    }

    fn push_log(&mut self, text: String, color: Color32) {
        for line in text.lines() {
            self.log.push(LogLine {
                text: line.to_string(),
                color,
            });
        }
        if self.log.len() > MAX_LOG_LINES {
            let extra = self.log.len() - MAX_LOG_LINES;
            self.log.drain(..extra);
        }
        self.scroll_to_bottom = true;
    }

    fn drain_output(&mut self) {
        for line in self.engine.drain_output() {
            let color = match &line {
                Output::Print(_) => Color32::from_gray(230),
                Output::Error(_) => Color32::from_rgb(255, 110, 110),
                Output::Warning(_) => Color32::from_rgb(240, 200, 90),
                Output::Log(_) => Color32::from_gray(150),
            };
            self.push_log(line.to_string(), color);
        }
    }

    /// Register game fonts used by the scene with egui.
    fn ensure_fonts(&mut self, ctx: &egui::Context, scene: &Scene) {
        let mut missing: Vec<String> = Vec::new();
        for item in &scene.items {
            if let Item::Text(t) = item {
                let key = t.font_path.to_ascii_lowercase();
                if !self.fonts_loaded.contains(&key) && !missing.contains(&key) {
                    missing.push(key);
                }
            }
        }
        if missing.is_empty() {
            return;
        }
        let Some(gd) = self.engine.gamedata().cloned() else {
            self.fonts_loaded.extend(missing);
            return;
        };
        let mut defs = FontDefinitions::default();
        let fallback: Vec<String> = defs
            .families
            .get(&FontFamily::Proportional)
            .cloned()
            .unwrap_or_default();
        self.fonts_loaded.extend(missing);
        for path in &self.fonts_loaded {
            if let Some(bytes) = gd.read_path(path) {
                defs.font_data
                    .insert(path.clone(), Arc::new(FontData::from_owned(bytes)));
                let mut family = vec![path.clone()];
                family.extend(fallback.iter().cloned());
                defs.families
                    .insert(FontFamily::Name(path.as_str().into()), family);
            }
        }
        ctx.set_fonts(defs);
    }

    fn texture(
        &mut self,
        ctx: &egui::Context,
        key: &ImageKey,
        desaturate: bool,
        repeat: bool,
    ) -> Option<TextureHandle> {
        let cache_key = (key.clone(), desaturate, repeat);
        if let Some(t) = self.textures.get(&cache_key) {
            return t.clone();
        }
        let image = self.engine.gamedata().and_then(|gd| gd.image(key));
        let handle = image.map(|img| {
            let mut rgba = img.rgba.clone();
            if desaturate {
                for px in rgba.as_chunks_mut::<4>().0 {
                    let l = (px[0] as f32 * 0.3 + px[1] as f32 * 0.59 + px[2] as f32 * 0.11) as u8;
                    px[0] = l;
                    px[1] = l;
                    px[2] = l;
                }
            }
            self.image_sizes
                .insert(key.clone(), (img.width as f32, img.height as f32));
            let color = egui::ColorImage::from_rgba_unmultiplied(
                [img.width as usize, img.height as usize],
                &rgba,
            );
            let mut options = TextureOptions::LINEAR;
            if repeat {
                options.wrap_mode = egui::TextureWrapMode::Repeat;
            }
            ctx.load_texture(format!("{key:?}{desaturate}{repeat}"), color, options)
        });
        self.textures.insert(cache_key, handle.clone());
        handle
    }

    fn draw_scene(&mut self, ui: &mut egui::Ui, scene: &Scene) -> (Rect, f32) {
        let avail = ui.available_rect_before_wrap();
        let k = (avail.width() / scene.width as f32).min(avail.height() / scene.height as f32);
        let size = Vec2::new(scene.width as f32 * k, scene.height as f32 * k);
        let origin = avail.min + (avail.size() - size) / 2.0;
        let area = Rect::from_min_size(origin, size);
        let painter = ui.painter_at(area);
        painter.rect_filled(area, 0.0, Color32::from_rgb(28, 30, 36));
        let ctx = ui.ctx().clone();
        let h = scene.height as f32;
        let to_screen =
            |x: f64, y: f64| Pos2::new(origin.x + x as f32 * k, origin.y + (h - y as f32) * k);

        for item in &scene.items {
            match item {
                Item::Quad(q) => self.draw_quad(&ctx, &painter, q, &to_screen, area),
                Item::Text(t) => draw_text(&painter, t, &to_screen, k, area),
            }
        }
        (area, k)
    }

    fn draw_quad(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        q: &Quad,
        to_screen: &dyn Fn(f64, f64) -> Pos2,
        area: Rect,
    ) {
        let min = to_screen(q.rect.left, q.rect.top());
        let max = to_screen(q.rect.right(), q.rect.bottom);
        let rect = Rect::from_min_max(min, max);
        let clip = q
            .clip
            .map(|c| Rect::from_min_max(to_screen(c.left, c.top()), to_screen(c.right(), c.bottom)))
            .unwrap_or(area)
            .intersect(area);
        let mut color = q.color;
        if q.additive {
            // egui has no additive blending; a lighter alpha is close enough.
            color[3] *= 0.6;
        }
        let tint = Color32::from_rgba_unmultiplied(
            (color[0].clamp(0.0, 1.0) * 255.0) as u8,
            (color[1].clamp(0.0, 1.0) * 255.0) as u8,
            (color[2].clamp(0.0, 1.0) * 255.0) as u8,
            (color[3].clamp(0.0, 1.0) * 255.0) as u8,
        );
        let mut uv = q.uv;
        let repeat = q.tile.0 || q.tile.1;
        let texture_id = match &q.image {
            None => egui::TextureId::default(),
            Some(key) => match self.texture(ctx, key, q.desaturate, repeat) {
                Some(t) => {
                    if repeat && let Some(&(iw, ih)) = self.image_sizes.get(key) {
                        let (w, h) = (q.rect.width as f32, q.rect.height as f32);
                        if q.tile.0 && iw > 0.0 {
                            let u = w / iw;
                            uv[4] = uv[0] + u;
                            uv[6] = uv[2] + u;
                        }
                        if q.tile.1 && ih > 0.0 {
                            let v = h / ih;
                            uv[3] = uv[1] + v;
                            uv[7] = uv[5] + v;
                        }
                    }
                    t.id()
                }
                None => return,
            },
        };
        if q.image.is_none() {
            // Solid colour: the white pixel of the font texture.
            uv = [0.0; 8];
        }
        if q.rotation != 0.0 {
            let (s, c) = (q.rotation.sin(), q.rotation.cos());
            let cx = (uv[0] + uv[6]) / 2.0;
            let cy = (uv[1] + uv[7]) / 2.0;
            for i in 0..4 {
                let (x, y) = (uv[i * 2] - cx, uv[i * 2 + 1] - cy);
                uv[i * 2] = cx + x * c + y * s;
                uv[i * 2 + 1] = cy - x * s + y * c;
            }
        }
        let mut mesh = Mesh::with_texture(texture_id);
        let corners = [
            rect.left_top(),
            rect.left_bottom(),
            rect.right_top(),
            rect.right_bottom(),
        ];
        for (i, p) in corners.iter().enumerate() {
            let tex = if q.image.is_none() {
                egui::epaint::WHITE_UV
            } else {
                Pos2::new(uv[i * 2], uv[i * 2 + 1])
            };
            mesh.vertices.push(egui::epaint::Vertex {
                pos: *p,
                uv: tex,
                color: tint,
            });
        }
        mesh.indices.extend_from_slice(&[0, 1, 2, 2, 1, 3]);
        painter.with_clip_rect(clip).add(Shape::mesh(mesh));
    }

    fn handle_input(&mut self, ctx: &egui::Context, area: Rect, k: f32, input_has_focus: bool) {
        let screen_h = self.engine.screen_size().1;
        let (pointer, events) = ctx.input(|i| (i.pointer.hover_pos(), i.events.clone()));
        if let Some(p) = pointer
            && area.contains(p)
        {
            let x = ((p.x - area.min.x) / k) as f64;
            let y = screen_h - ((p.y - area.min.y) / k) as f64;
            self.engine.mouse_move(x, y);
        }
        for event in events {
            match event {
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    ..
                } if area.contains(pos) => {
                    let name = match button {
                        egui::PointerButton::Primary => "LeftButton",
                        egui::PointerButton::Secondary => "RightButton",
                        egui::PointerButton::Middle => "MiddleButton",
                        _ => "Button4",
                    };
                    if pressed {
                        self.engine.mouse_down(name);
                    } else {
                        self.engine.mouse_up(name);
                    }
                }
                egui::Event::MouseWheel { delta, .. }
                    if pointer.is_some_and(|p| area.contains(p)) =>
                {
                    if delta.y != 0.0 {
                        self.engine.mouse_wheel(delta.y.signum() as f64);
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } if !input_has_focus => {
                    if let Some(name) = key_name(key) {
                        let full = if modifiers.shift {
                            format!("SHIFT-{name}")
                        } else {
                            name.to_string()
                        };
                        if !self.engine.key_down(&full) && full != name {
                            self.engine.key_down(name);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn key_name(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::Escape => "ESCAPE",
        Key::Enter => "ENTER",
        Key::Tab => "TAB",
        Key::Space => "SPACE",
        Key::A => "A",
        Key::B => "B",
        Key::C => "C",
        Key::D => "D",
        Key::E => "E",
        Key::F => "F",
        Key::G => "G",
        Key::H => "H",
        Key::I => "I",
        Key::J => "J",
        Key::K => "K",
        Key::L => "L",
        Key::M => "M",
        Key::N => "N",
        Key::O => "O",
        Key::P => "P",
        Key::Q => "Q",
        Key::R => "R",
        Key::S => "S",
        Key::T => "T",
        Key::U => "U",
        Key::V => "V",
        Key::W => "W",
        Key::X => "X",
        Key::Y => "Y",
        Key::Z => "Z",
        _ => return None,
    })
}

fn to_color(c: [f32; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (c[0].clamp(0.0, 1.0) * 255.0) as u8,
        (c[1].clamp(0.0, 1.0) * 255.0) as u8,
        (c[2].clamp(0.0, 1.0) * 255.0) as u8,
        (c[3].clamp(0.0, 1.0) * 255.0) as u8,
    )
}

fn draw_text(
    painter: &egui::Painter,
    t: &TextItem,
    to_screen: &dyn Fn(f64, f64) -> Pos2,
    k: f32,
    area: Rect,
) {
    let family = FontFamily::Name(t.font_path.to_ascii_lowercase().as_str().into());
    let has_family = painter.ctx().fonts(|f| f.families().contains(&family));
    let family = if has_family {
        family
    } else {
        FontFamily::Proportional
    };
    let clip = t
        .clip
        .map(|c| Rect::from_min_max(to_screen(c.left, c.top()), to_screen(c.right(), c.bottom)))
        .unwrap_or(area)
        .intersect(area);
    let painter = painter.with_clip_rect(clip);
    let size = (t.size as f32 * k).max(1.0);
    for (line, width) in t.lines.iter().zip(&t.widths) {
        let build = |font_size: f32, shadow: Option<Color32>| {
            let mut job = LayoutJob::default();
            for run in &line.runs {
                let color = shadow.unwrap_or_else(|| {
                    let mut c = run.color.unwrap_or(t.color);
                    c[3] = t.color[3] * run.color.map(|c| c[3]).unwrap_or(1.0);
                    to_color(c)
                });
                job.append(
                    &run.text,
                    0.0,
                    egui::TextFormat::simple(FontId::new(font_size, family.clone()), color),
                );
            }
            job
        };
        // Match the engine's measured width, so alignment matches layout.
        let probe = painter.layout_job(build(size, None));
        let target = *width as f32 * k;
        let mut font_size = size;
        if probe.size().x > 1.0 && target > 1.0 {
            font_size = (size * target / probe.size().x).clamp(size * 0.75, size * 1.3);
        }
        let pos = to_screen(line.x, line.y_top);
        if let Some((c, (dx, dy))) = t.shadow {
            let galley = painter.layout_job(build(font_size, Some(to_color(c))));
            painter.galley(
                pos + Vec2::new(dx as f32 * k, -dy as f32 * k),
                galley,
                Color32::BLACK,
            );
        }
        if t.outline {
            let black = Color32::from_rgba_unmultiplied(0, 0, 0, (t.color[3] * 255.0) as u8);
            for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
                let galley = painter.layout_job(build(font_size, Some(black)));
                painter.galley(pos + Vec2::new(dx, dy), galley, Color32::BLACK);
            }
        }
        let galley = painter.layout_job(build(font_size, None));
        painter.galley(pos, galley, Color32::WHITE);
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f64().min(0.25);
        self.last_tick = now;
        self.engine.tick(dt);
        self.drain_output();

        let mut input_has_focus = false;
        egui::Panel::bottom("console")
            .resizable(true)
            .default_size(240.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(">");
                    let edit = egui::TextEdit::singleline(&mut self.input)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(ui.available_width() - 160.0);
                    let response = ui.add(edit);
                    input_has_focus = response.has_focus();
                    if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        let line = std::mem::take(&mut self.input);
                        if !line.trim().is_empty() {
                            self.push_log(format!("> {line}"), Color32::LIGHT_GREEN);
                            self.history.push(line.clone());
                            self.engine.run_command(&line);
                            self.drain_output();
                        }
                        response.request_focus();
                    }
                    if input_has_focus
                        && ui.input(|i| i.key_pressed(Key::ArrowUp))
                        && let Some(last) = self.history.last()
                    {
                        self.input = last.clone();
                    }
                    ui.checkbox(&mut self.show_hover, "Show hover");
                });
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in &self.log {
                            ui.label(
                                egui::RichText::new(&line.text)
                                    .monospace()
                                    .color(line.color),
                            );
                        }
                    });
            });

        egui::CentralPanel::default().show(ui, |ui| {
            let scene = self.engine.scene();
            self.ensure_fonts(&ctx, &scene);
            let (area, k) = self.draw_scene(ui, &scene);
            self.handle_input(&ctx, area, k, input_has_focus);
            if self.show_hover
                && let Some(name) = self.engine.mouse_focus_name()
            {
                ui.painter().text(
                    area.left_top() + Vec2::new(8.0, 8.0),
                    egui::Align2::LEFT_TOP,
                    name,
                    FontId::monospace(14.0),
                    Color32::YELLOW,
                );
            }
        });
        self.drain_output();
        ctx.request_repaint();
    }
}
