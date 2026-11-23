//! Native window for the sandbox: draws the UI scene and offers a console.
//!
//! By default the window is borderless and covers the whole monitor at its
//! native resolution, like the game's "windowed (fullscreen)" mode, so the
//! sandbox can be compared with game screenshots pixel for pixel. The scene
//! is drawn by the engine's software renderer. F12 shows or hides the
//! console: sandbox output and an input line for slash commands and Lua.
//! Key presses go to the sandbox's key bindings while the input line does
//! not have focus.

use std::time::Instant;

use eframe::egui::{self, Color32, FontId, Key, Pos2, Rect, TextureHandle, TextureOptions, Vec2};
use wowprogramming_engine::control::{Action, ControlServer};
use wowprogramming_engine::scene::Scene;
use wowprogramming_engine::{Engine, Output, WorldEntry, loading, render};

/// Starts a game session for a character chosen on the character select
/// screen (builds and starts a new engine).
pub type WorldFactory = Box<dyn FnMut(&WorldEntry) -> Result<Engine, String>>;

/// The loading screen between character select and the world.
struct Loading {
    entry: WorldEntry,
    /// Frames drawn so far; the new session is built after the loading
    /// screen has been shown.
    frames: u32,
    texture: Option<TextureHandle>,
}

const MAX_LOG_LINES: usize = 2000;

struct LogLine {
    text: String,
    color: Color32,
}

pub struct App {
    engine: Engine,
    fonts: render::FontStore,
    frame_texture: Option<TextureHandle>,
    log: Vec<LogLine>,
    input: String,
    history: Vec<String>,
    last_tick: Instant,
    scroll_to_bottom: bool,
    show_hover: bool,
    control: Option<ControlServer>,
    show_console: bool,
    /// Last display size given to the engine, in physical pixels.
    display: Option<(u32, u32)>,
    fullscreen: bool,
    resized_to_monitor: bool,
    factory: Option<WorldFactory>,
    loading: Option<Loading>,
}

/// How to open the window.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowOptions {
    /// A normal window with a title bar instead of a borderless window
    /// covering the monitor.
    pub windowed: bool,
}

/// Open the window and run until it closes.
pub fn run(
    engine: Engine,
    control: Option<ControlServer>,
    window: WindowOptions,
    factory: Option<WorldFactory>,
) -> Result<(), String> {
    let mut viewport = egui::ViewportBuilder::default().with_title("WowProgramming");
    viewport = if window.windowed {
        viewport.with_inner_size([1600.0, 1100.0])
    } else {
        // Borderless fullscreen on the current monitor, at its native mode.
        viewport.with_fullscreen(true).with_decorations(false)
    };
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "WowProgramming",
        options,
        Box::new(|_cc| {
            let mut app = App::new(engine);
            app.control = control;
            app.factory = factory;
            app.fullscreen = !window.windowed;
            app.show_console = window.windowed;
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| e.to_string())
}

impl App {
    pub fn new(engine: Engine) -> App {
        let mut app = App {
            engine,
            fonts: render::FontStore::default(),
            frame_texture: None,
            log: Vec::new(),
            input: String::new(),
            history: Vec::new(),
            last_tick: Instant::now(),
            scroll_to_bottom: true,
            show_hover: false,
            control: None,
            show_console: true,
            display: None,
            fullscreen: false,
            resized_to_monitor: false,
            factory: None,
            loading: None,
        };
        app.push_log(
            "F12 shows or hides this console. Type /dump <expr>, /run <code> or Lua here.".into(),
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

    /// Show the loading screen; once it is on screen, build the game
    /// session (this blocks while the UI loads, as the client's loading
    /// screen does).
    fn show_loading(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let rect = ctx.content_rect();
        let ppp = ctx.pixels_per_point();
        let Some(state) = self.loading.as_mut() else {
            return;
        };
        if state.texture.is_none() {
            let (w, h) = (
                (rect.width() * ppp).round().max(1.0) as usize,
                (rect.height() * ppp).round().max(1.0) as usize,
            );
            let gd = self.engine.gamedata().cloned();
            let image = gd.and_then(|gd| {
                let map = state.entry.map_id.unwrap_or(0);
                let screen = loading::for_map(&gd, map).or_else(|| loading::for_map(&gd, 0))?;
                Some(loading::render(&gd, &screen, w, h, 0.15))
            });
            let image = match image {
                Some(img) => {
                    egui::ColorImage::from_rgba_unmultiplied([img.width, img.height], &img.data)
                }
                None => egui::ColorImage::new([w, h], vec![Color32::BLACK; w * h]),
            };
            state.texture = Some(ctx.load_texture("loading", image, TextureOptions::LINEAR));
        }
        if let Some(t) = &state.texture {
            ui.painter().image(
                t.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        state.frames += 1;
        if state.frames < 3 {
            return;
        }
        let entry = state.entry.clone();
        let result = match self.factory.as_mut() {
            Some(f) => f(&entry),
            None => Err("no game session factory".into()),
        };
        self.loading = None;
        match result {
            Ok(engine) => {
                self.engine = engine;
                self.display = None;
                self.frame_texture = None;
                self.last_tick = Instant::now();
                self.drain_output();
            }
            Err(err) => self.push_log(format!("cannot enter the world: {err}"), Color32::RED),
        }
    }

    /// Give the engine the window's size in physical pixels, so the UI
    /// takes the monitor's aspect ratio and text its pixel size. Only a
    /// full-screen window does this; a normal window keeps the configured
    /// screen and scales it to fit.
    fn sync_display_size(&mut self, ctx: &egui::Context) {
        if !self.fullscreen {
            return;
        }
        let (size, ppp) = ctx.input(|i| {
            (
                i.viewport().inner_rect.map(|r| r.size()),
                i.pixels_per_point(),
            )
        });
        let Some(size) = size else { return };
        // Without a window manager (Xvfb) the fullscreen request does
        // nothing, so also cover the monitor by hand.
        let monitor = ctx.input(|i| i.viewport().monitor_size);
        if let Some(m) = monitor
            && (m - size).length() > 1.0
            && !self.resized_to_monitor
        {
            self.resized_to_monitor = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(Pos2::ZERO));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(m));
        }
        let w = (size.x * ppp).round() as u32;
        let h = (size.y * ppp).round() as u32;
        if w == 0 || h == 0 || self.display == Some((w, h)) {
            return;
        }
        self.display = Some((w, h));
        self.engine.set_display_size(w as f64, h as f64);
    }

    /// Draw the scene with the engine's software renderer, so the window
    /// shows exactly what screenshots show (masks, blending, rotation).
    /// Returns the area and the scale from screen units to points.
    fn draw_scene(&mut self, ui: &mut egui::Ui, scene: &Scene) -> (Rect, f32) {
        let avail = ui.available_rect_before_wrap();
        let k = (avail.width() / scene.width as f32).min(avail.height() / scene.height as f32);
        let size = Vec2::new(scene.width as f32 * k, scene.height as f32 * k);
        let origin = avail.min + (avail.size() - size) / 2.0;
        let area = Rect::from_min_size(origin, size);
        let pixels_per_point = ui.ctx().pixels_per_point();
        let scale = (k * pixels_per_point) as f64;
        let canvas = render::render_with(
            scene,
            self.engine.gamedata().map(|g| g.as_ref()),
            scale,
            &mut self.fonts,
        );
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [canvas.width, canvas.height],
            &canvas.to_rgba8(),
        );
        match &mut self.frame_texture {
            Some(t) => t.set(image, TextureOptions::LINEAR),
            None => {
                self.frame_texture = Some(ui.ctx().load_texture(
                    "scene",
                    image,
                    TextureOptions::LINEAR,
                ));
            }
        }
        if let Some(t) = &self.frame_texture {
            ui.painter_at(area).image(
                t.id(),
                area,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        (area, k)
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
                egui::Event::Text(text) if !input_has_focus => {
                    self.engine.text_input(&text);
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } if !input_has_focus => {
                    if let Some(name) = key_name(key) {
                        // Letters typed into an edit box are text, not bindings.
                        if self.engine.has_text_focus() && name.len() == 1 {
                            continue;
                        }
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
        Key::Backspace => "BACKSPACE",
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

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.loading.is_some() {
            self.show_loading(ui);
            ctx.request_repaint();
            return;
        }
        if let Some(entry) = self.engine.take_enter_world()
            && self.factory.is_some()
        {
            self.push_log(
                format!("Entering the world as {}", entry.name),
                Color32::LIGHT_BLUE,
            );
            self.loading = Some(Loading {
                entry,
                frames: 0,
                texture: None,
            });
            ctx.request_repaint();
            return;
        }
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f64().min(0.25);
        self.last_tick = now;
        self.engine.tick(dt);
        self.drain_output();
        if let Some(server) = &mut self.control {
            let (shown, action) = server.poll(&mut self.engine);
            for line in shown {
                let color = match &line {
                    Output::Error(_) => Color32::from_rgb(255, 110, 110),
                    _ => Color32::from_gray(200),
                };
                self.push_log(line.to_string(), color);
            }
            match action {
                Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Action::Tick(s) => self.engine.tick(s),
                Action::None => {}
            }
        }

        if ctx.input(|i| i.key_pressed(Key::F12)) {
            self.show_console = !self.show_console;
        }
        self.sync_display_size(&ctx);

        let mut input_has_focus = false;
        let show_console = self.show_console;
        if show_console {
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
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::BLACK))
            .show(ui, |ui| {
                let scene = self.engine.scene();
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
