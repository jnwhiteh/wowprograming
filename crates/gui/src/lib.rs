//! Native window for the sandbox: draws the UI scene and offers a console.
//!
//! The scene is drawn into the central panel, scaled to fit, by the
//! engine's software renderer. The bottom
//! panel shows sandbox output and has an input line for slash commands and
//! Lua. Key presses go to the sandbox's key bindings while the input line
//! does not have focus.

use std::time::Instant;

use eframe::egui::{self, Color32, FontId, Key, Pos2, Rect, TextureHandle, TextureOptions, Vec2};
use wowprogramming_engine::control::{Action, ControlServer};
use wowprogramming_engine::scene::Scene;
use wowprogramming_engine::{Engine, Output, render};

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
}

/// Open the window and run until it closes.
pub fn run(engine: Engine, control: Option<ControlServer>) -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1600.0, 1100.0])
            .with_title("WowProgramming"),
        ..Default::default()
    };
    eframe::run_native(
        "WowProgramming",
        options,
        Box::new(|_cc| {
            let mut app = App::new(engine);
            app.control = control;
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
