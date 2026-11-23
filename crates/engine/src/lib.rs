//! The addon sandbox engine: loads addons and runs them against an
//! emulation of the client's native API.

pub mod addons;
pub mod animfx;
pub mod api;
pub mod apidoc;
pub mod bindings;
pub mod clientnames;
pub mod control;
pub mod frames;
pub mod gamedata;
pub mod input;
pub mod layout;
pub mod natives;
pub mod render;
pub mod savedvars;
pub mod scene;
pub mod state;
pub mod text;
pub mod timers;
pub mod toc;
pub mod widgets;
pub mod xml;

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use mlua::{FromLuaMulti, Function, IntoLuaMulti, Lua, Table, Value};
use wowprogramming_lua::{DEFAULT_SCRIPT_TIMEOUT, Runtime, mlua};

use state::{AddonKind, State, WtfPaths, state};
pub use state::{LoadState, Output};

/// Settings for one sandbox session.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Blizzard UI source tree for the emulated flavor. Blizzard addons are
    /// loaded from `<ui_source_dir>/Interface/AddOns`.
    pub ui_source_dir: Option<PathBuf>,
    /// Folder with the third-party addons to load.
    pub addons_dir: Option<PathBuf>,
    /// Root of the sandbox's `WTF` folder, where SavedVariables are written.
    pub wtf_dir: PathBuf,
    pub account: String,
    pub realm: String,
    pub character: String,
    /// Blizzard addons to load before third-party addons. Their
    /// dependencies are loaded too. `"*"` stands for every addon the
    /// client loads at startup.
    pub blizzard_addons: Vec<String>,
    /// Load-on-demand Blizzard addons whose `[Bootstrap]` files run at
    /// startup.
    pub bootstrap_addons: Vec<String>,
    /// Single files from Blizzard addons that are not loaded as a whole,
    /// relative to `Interface/AddOns`, run after the Blizzard addons.
    pub blizzard_files: Vec<String>,
    /// When set, only these third-party addons (and their dependencies)
    /// are loaded at startup.
    pub only_addons: Option<Vec<String>>,
    /// Scenario folder with a `scenario.lua` that sets up game state.
    pub scenario: Option<PathBuf>,
    /// Screen size in UI units.
    pub screen: (f64, f64),
    /// Run the glue environment (character select) instead of the game.
    pub glue: bool,
    /// The game's own account folder (`WTF/Account/<ACCOUNT>`), read only:
    /// scenarios read the character list from addon data there.
    pub game_account_dir: Option<PathBuf>,
    /// Physical pixels per UI unit at scale 1. Text is drawn at whole
    /// pixel sizes, as in the client.
    pub pixel_scale: f64,
    /// Value of `GetTime()` at startup.
    pub start_time: f64,
    /// Time budget for one call into Lua.
    pub script_timeout: Duration,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            ui_source_dir: None,
            addons_dir: None,
            wtf_dir: PathBuf::from(".sandbox/WTF"),
            account: "SANDBOX".into(),
            realm: "Sandbox".into(),
            character: "Player".into(),
            blizzard_addons: vec!["Blizzard_SharedXMLBase".into()],
            bootstrap_addons: Vec::new(),
            blizzard_files: Vec::new(),
            only_addons: None,
            scenario: None,
            screen: (1920.0, 1080.0),
            glue: false,
            game_account_dir: None,
            pixel_scale: 1.0,
            start_time: 1000.0,
            script_timeout: DEFAULT_SCRIPT_TIMEOUT,
        }
    }
}

/// A running sandbox session.
pub struct Engine {
    config: EngineConfig,
    runtime: Runtime,
    gamedata: Option<Rc<gamedata::GameData>>,
}

impl Engine {
    /// Create the Lua state and native API, and find the addons. Nothing is
    /// loaded until [`Engine::start`].
    pub fn new(config: EngineConfig) -> mlua::Result<Engine> {
        Self::with_gamedata(config, None)
    }

    /// Like [`Engine::new`], with game data for textures, atlases and fonts.
    pub fn with_gamedata(
        config: EngineConfig,
        gamedata: Option<Rc<gamedata::GameData>>,
    ) -> mlua::Result<Engine> {
        let runtime = build_runtime(&config, gamedata.clone())?;
        Ok(Engine {
            config,
            runtime,
            gamedata,
        })
    }

    pub fn gamedata(&self) -> Option<&Rc<gamedata::GameData>> {
        self.gamedata.as_ref()
    }

    pub fn lua(&self) -> &Lua {
        self.runtime.lua()
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Load Blizzard and third-party addons, then fire the login events.
    pub fn start(&mut self) {
        self.load_all(false);
    }

    fn load_all(&mut self, reloading: bool) {
        if self.config.glue {
            self.load_glue();
            return;
        }
        let lua = self.runtime.lua();
        if let Some(dir) = &self.config.scenario {
            let full_ui = self.config.blizzard_addons.iter().any(|n| n == "*");
            load_scenario(lua, dir, full_ui);
        }
        // Key bindings exist before the UI loads; UPDATE_BINDINGS follows
        // at login.
        if let Err(err) = bindings::load(lua) {
            state(lua).warn(format!("cannot load key bindings: {err}"));
        }
        let mut blizzard = Vec::new();
        for name in &self.config.blizzard_addons {
            if name == "*" {
                blizzard.extend(addons::startup_blizzard_addons(lua));
            } else {
                blizzard.push(name.clone());
            }
        }
        for name in &blizzard {
            let result = addons::load_addon(lua, name);
            if result != LoadState::Loaded {
                state(lua).warn(format!("Blizzard addon {name} not loaded: {result:?}"));
            }
        }
        for name in &self.config.bootstrap_addons {
            addons::run_bootstrap(lua, name);
        }
        for file in &self.config.blizzard_files {
            addons::run_blizzard_file(lua, file);
        }
        if let Err(err) = echo_prints(lua) {
            state(lua).warn(format!("cannot hook print: {err}"));
        }
        if let Err(err) = ensure_framexml_globals(lua) {
            state(lua).warn(format!("cannot create FrameXML globals: {err}"));
        }

        let names: Vec<String> = {
            let st = state(lua);
            st.user_addons()
                .filter(|(_, a)| !a.toc.load_on_demand())
                .filter(|(_, a)| match &self.config.only_addons {
                    Some(only) => only.iter().any(|n| n.eq_ignore_ascii_case(&a.name)),
                    None => true,
                })
                .map(|(_, a)| a.name.clone())
                .collect()
        };
        for name in names {
            addons::load_addon(lua, &name);
        }

        self.fire_event("PLAYER_LOGIN", ());
        self.fire_event("PLAYER_ENTERING_WORLD", (!reloading, reloading));
        // The client sends the edit mode layouts at login; they place the
        // unit frames, action bars and other HUD frames.
        let layouts: Option<Table> = self
            .runtime
            .lua()
            .load("return C_EditMode and C_EditMode.GetLayouts and C_EditMode.GetLayouts()")
            .eval()
            .ok()
            .flatten();
        if let Some(layouts) = layouts {
            self.fire_event("EDIT_MODE_LAYOUTS_UPDATED", (layouts, false));
        }
        self.fire_event("VARIABLES_LOADED", ());
        self.fire_event("UPDATE_BINDINGS", ());
        // The client sends the spell list after login; AssistedCombatManager
        // and the spellbook read it on this event.
        self.fire_event("SPELLS_CHANGED", ());
        // The loading screen ends: the UI becomes visible.
        let lua = self.runtime.lua();
        let ui_parent = lua
            .globals()
            .get::<Table>("UIParent")
            .ok()
            .and_then(|t| frames::frame_id(&t).ok());
        if let Some(id) = ui_parent {
            frames::change_visibility(lua, id, |f| f[id].shown = true);
        }
        self.fire_event("LOADING_SCREEN_DISABLED", ());
        widgets::fire_size_changes(self.runtime.lua());
    }

    /// Start the glue environment: Blizzard's glue addons (login, realm
    /// list, character select), no third-party addons. The client sends
    /// FRAMES_LOADED when they are loaded; GlueParent then picks the screen
    /// from `C_Login.GetState()` (see `lua/glue.lua`).
    fn load_glue(&mut self) {
        let lua = self.runtime.lua();
        // glue.lua receives a function that fires client events.
        let fire = lua.create_function(|lua, args: mlua::MultiValue| {
            let mut args = args.into_iter();
            let Some(Value::String(event)) = args.next() else {
                return Ok(());
            };
            let event = event.to_string_lossy();
            frames::dispatch_event(lua, &event, args.collect());
            Ok(())
        });
        let result = fire.and_then(|fire| {
            lua.load(include_str!("lua/glue.lua"))
                .set_name("=[C]")
                .call::<()>(fire)
        });
        if let Err(err) = result {
            state(lua).warn(format!("cannot set up the glue environment: {err}"));
        }
        if let Some(dir) = &self.config.scenario {
            load_scenario(lua, dir, true);
        }
        let mut blizzard = Vec::new();
        for name in &self.config.blizzard_addons {
            if name == "*" {
                blizzard.extend(addons::startup_blizzard_addons(lua));
            } else {
                blizzard.push(name.clone());
            }
        }
        for name in &blizzard {
            let result = addons::load_addon(lua, name);
            if result != LoadState::Loaded {
                state(lua).warn(format!("Blizzard addon {name} not loaded: {result:?}"));
            }
        }
        if let Err(err) = echo_prints(lua) {
            state(lua).warn(format!("cannot hook print: {err}"));
        }
        self.fire_event("FRAMES_LOADED", ());
        self.fire_event("LOGIN_STATE_CHANGED", ());
        // The payload is the size of the character list.
        let count: i64 = self
            .runtime
            .lua()
            .load("return GetNumCharacters and GetNumCharacters() or 0")
            .eval()
            .unwrap_or(0);
        self.fire_event("CHARACTER_LIST_UPDATE", count);
        widgets::fire_size_changes(self.runtime.lua());
    }

    /// Advance the clock by `elapsed` seconds: run due timers, then
    /// `OnUpdate` scripts.
    pub fn tick(&mut self, elapsed: f64) {
        let lua = self.runtime.lua();
        {
            let mut st = state(lua);
            st.time += elapsed;
            st.last_elapsed = elapsed;
        }
        timers::run_due(lua);
        frames::run_on_update(lua, elapsed);
        widgets::fire_size_changes(lua);
    }

    /// The current scene: visible regions in draw order.
    pub fn scene(&self) -> scene::Scene {
        scene::build(&mut state(self.runtime.lua()))
    }

    /// Render the current scene to a PNG file without a window, at
    /// `scale` pixels per UI unit.
    pub fn save_screenshot(&self, path: &std::path::Path, scale: f64) -> std::io::Result<()> {
        self.save_screenshot_crop(path, scale, None)
    }

    /// Like [`Engine::save_screenshot`], cropped to a rectangle in screen
    /// units.
    pub fn save_screenshot_crop(
        &self,
        path: &std::path::Path,
        scale: f64,
        crop: Option<layout::Rect>,
    ) -> std::io::Result<()> {
        let scene = self.scene();
        let canvas = render::render(&scene, self.gamedata.as_deref(), scale);
        let rgba = canvas.to_rgba8();
        let Some(c) = crop else {
            return render::write_png(path, canvas.width, canvas.height, &rgba);
        };
        let x0 = ((c.left * scale).floor().max(0.0) as usize).min(canvas.width);
        let x1 = ((c.right() * scale).ceil().max(0.0) as usize).min(canvas.width);
        let y0 = (((scene.height - c.top()) * scale).floor().max(0.0) as usize).min(canvas.height);
        let y1 = (((scene.height - c.bottom) * scale).ceil().max(0.0) as usize).min(canvas.height);
        let (w, h) = (x1.saturating_sub(x0).max(1), y1.saturating_sub(y0).max(1));
        let mut out = vec![0u8; w * h * 4];
        for y in 0..h.min(y1 - y0) {
            let src = ((y0 + y) * canvas.width + x0) * 4;
            out[y * w * 4..y * w * 4 + (x1 - x0) * 4]
                .copy_from_slice(&rgba[src..src + (x1 - x0) * 4]);
        }
        render::write_png(path, w, h, &out)
    }

    /// Screen size in UI units.
    pub fn screen_size(&self) -> (f64, f64) {
        state(self.runtime.lua()).screen
    }

    /// The window's size in physical pixels changed (for example when a
    /// full-screen window learns the monitor size). The UI keeps its height
    /// in UI units; its width follows the window's aspect ratio, and text
    /// is drawn at the window's pixel size. Fires DISPLAY_SIZE_CHANGED, as
    /// the client does when the resolution changes.
    pub fn set_display_size(&mut self, width: f64, height: f64) {
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        {
            let mut st = state(self.runtime.lua());
            let ui_height = st.screen.1;
            let ui_width = ui_height * width / height;
            let pixel_scale = height / ui_height;
            if (st.screen.0 - ui_width).abs() < 0.01 && (st.pixel_scale - pixel_scale).abs() < 1e-6
            {
                return;
            }
            st.screen.0 = ui_width;
            st.pixel_scale = pixel_scale;
            layout::invalidate(&mut st);
        }
        self.fire_event("DISPLAY_SIZE_CHANGED", ());
    }

    /// Mouse moved to a position in screen units (y up).
    pub fn mouse_move(&mut self, x: f64, y: f64) {
        input::mouse_move(self.runtime.lua(), x, y);
    }

    pub fn mouse_down(&mut self, button: &str) {
        input::mouse_down(self.runtime.lua(), button);
    }

    pub fn mouse_up(&mut self, button: &str) {
        input::mouse_up(self.runtime.lua(), button);
    }

    pub fn mouse_wheel(&mut self, delta: f64) {
        input::mouse_wheel(self.runtime.lua(), delta);
    }

    /// Typed text for the focused edit box. Returns false if no edit box
    /// has the keyboard focus.
    pub fn text_input(&mut self, text: &str) -> bool {
        input::text_input(self.runtime.lua(), text)
    }

    /// Whether an edit box has the keyboard focus.
    pub fn has_text_focus(&self) -> bool {
        state(self.runtime.lua()).focus.is_some()
    }

    /// A key was pressed, such as `P`, `SHIFT-P` or `ESCAPE`. Runs the
    /// bound action. Returns false if the key has no binding.
    pub fn key_down(&mut self, key: &str) -> bool {
        // A focused edit box takes Backspace, Enter, Escape and Tab.
        if input::edit_box_key(self.runtime.lua(), key) {
            return true;
        }
        bindings::key_down(self.runtime.lua(), key)
    }

    /// Debug name of the frame under the mouse, if any.
    pub fn mouse_focus_name(&self) -> Option<String> {
        let lua = self.runtime.lua();
        let id = state(lua).mouse_focus?;
        let table = state(lua).table(id);
        let f: mlua::Function = table.get("GetDebugName").ok()?;
        f.call::<String>(table).ok()
    }

    /// Fire an event to all frames registered for it.
    pub fn fire_event(&self, event: &str, args: impl IntoLuaMulti) {
        let lua = self.runtime.lua();
        match args.into_lua_multi(lua) {
            Ok(args) => frames::dispatch_event(lua, event, args),
            Err(err) => state(lua).warn(format!("bad arguments for {event}: {err}")),
        }
    }

    /// Run a console line. Lines starting with `/` are slash commands;
    /// other lines run as Lua code.
    pub fn run_command(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        let Some(command_line) = line.strip_prefix('/') else {
            self.exec(line);
            return;
        };
        let (command, rest) = match command_line.split_once(char::is_whitespace) {
            Some((c, r)) => (c.to_ascii_lowercase(), r.trim().to_string()),
            None => (command_line.to_ascii_lowercase(), String::new()),
        };
        match command.as_str() {
            "run" | "script" => {
                self.exec(&rest);
            }
            "dump" => self.dump(&rest),
            "reload" => self.reload(),
            "stubs" => {
                let called = apidoc::called_stubs(self.runtime.lua());
                let mut st = state(self.runtime.lua());
                st.output
                    .push(Output::Print(format!("{} stubs called:", called.len())));
                for (name, count) in called {
                    st.output.push(Output::Print(format!("  {name} x{count}")));
                }
                let unknown: Vec<String> = st.unknown_cvars.iter().cloned().collect();
                st.output.push(Output::Print(format!(
                    "{} unknown CVars read: {}",
                    unknown.len(),
                    unknown.join(" ")
                )));
            }
            _ => {
                if !run_slash_command(self.runtime.lua(), &command, &rest) {
                    state(self.runtime.lua())
                        .output
                        .push(Output::Print(format!("Unknown command: /{command}")));
                }
            }
        }
    }

    /// Run Lua code. Errors go to the Lua error handler. Returns false on
    /// error.
    pub fn exec(&self, code: &str) -> bool {
        natives::run_chunk(self.runtime.lua(), code.as_bytes(), code, ())
    }

    /// Evaluate Lua code and return its results to Rust. Errors are
    /// returned, not sent to the error handler.
    pub fn eval<R: FromLuaMulti>(&self, code: &str) -> mlua::Result<R> {
        self.runtime
            .guarded(|lua| lua.load(code).set_name("=eval").eval())
    }

    fn dump(&self, expr: &str) {
        let lua = self.runtime.lua();
        let values: mlua::Result<mlua::MultiValue> = self
            .runtime
            .guarded(|lua| lua.load(format!("return {expr}")).set_name("=dump").eval());
        let mut st = state(lua);
        match values {
            Ok(values) => {
                st.output.push(Output::Print(format!("Dump: {expr}")));
                if values.is_empty() {
                    st.output.push(Output::Print("empty result".into()));
                }
                for (i, v) in values.iter().enumerate() {
                    let mut out = Vec::new();
                    let text = if savedvars::write_value(&mut out, v, 0, &mut Default::default()) {
                        String::from_utf8_lossy(&out).to_string()
                    } else {
                        format!("{v:?}")
                    };
                    st.output
                        .push(Output::Print(format!("[{}] = {text}", i + 1)));
                }
            }
            Err(err) => st.output.push(Output::Error(err.to_string())),
        }
    }

    /// Save SavedVariables, rebuild the Lua state and load everything again,
    /// like `/reload` in the client. Output not yet drained is kept.
    pub fn reload(&mut self) {
        self.fire_event("PLAYER_LEAVING_WORLD", ());
        addons::save_all(self.runtime.lua());
        let pending = self.drain_output();
        match build_runtime(&self.config, self.gamedata.clone()) {
            Ok(runtime) => {
                self.runtime = runtime;
                state(self.runtime.lua()).output = pending;
                self.load_all(true);
            }
            Err(err) => {
                let mut st = state(self.runtime.lua());
                st.output = pending;
                st.warn(format!("reload failed: {err}"));
            }
        }
    }

    /// Fire the logout events and write SavedVariables.
    pub fn shutdown(&mut self) {
        self.fire_event("PLAYER_LOGOUT", ());
        self.fire_event("PLAYER_LEAVING_WORLD", ());
        addons::save_all(self.runtime.lua());
    }

    /// Take all output produced since the last call.
    pub fn drain_output(&self) -> Vec<Output> {
        std::mem::take(&mut state(self.runtime.lua()).output)
    }

    /// Load state of an addon, by name.
    pub fn addon_state(&self, name: &str) -> Option<LoadState> {
        let st = state(self.runtime.lua());
        st.find_addon(name).map(|i| st.addons[i].load_state.clone())
    }

    /// Names of loaded addons in load order is not tracked; this returns
    /// loaded addons in list order.
    pub fn loaded_addons(&self) -> Vec<String> {
        state(self.runtime.lua())
            .addons
            .iter()
            .filter(|a| a.load_state == LoadState::Loaded)
            .map(|a| a.name.clone())
            .collect()
    }
}

fn build_runtime(
    config: &EngineConfig,
    gamedata: Option<Rc<gamedata::GameData>>,
) -> mlua::Result<Runtime> {
    let runtime = Runtime::new()?;
    runtime.set_script_timeout(config.script_timeout);
    let user_addons = config
        .addons_dir
        .as_deref()
        .map(|d| addons::discover_user_addons(d, config.glue))
        .unwrap_or_default();
    let now = Instant::now();
    runtime.lua().set_app_data(State {
        objects: runtime.lua().create_table()?,
        script_table: runtime.lua().create_table()?,
        output: Vec::new(),
        watchdog: runtime.watchdog(),
        frames: Vec::new(),
        event_frames: HashMap::new(),
        all_event_frames: Vec::new(),
        frame_metatables: HashMap::new(),
        time: config.start_time,
        last_elapsed: 0.0,
        started_at: now,
        profile_start: now,
        timers: Vec::new(),
        next_timer_id: 1,
        addons: user_addons,
        ui_source_dir: config.ui_source_dir.clone(),
        wtf: WtfPaths {
            root: config.wtf_dir.clone(),
            account: config.account.clone(),
            realm: config.realm.clone(),
            character: config.character.clone(),
        },
        warned_templates: Vec::new(),
        layout_generation: 0,
        rect_cache: Vec::new(),
        screen: config.screen,
        glue: config.glue,
        game_account_dir: config.game_account_dir.clone(),
        pixel_scale: config.pixel_scale,
        xml: Default::default(),
        cvars: HashMap::new(),
        cvar_defaults: api::builtin_cvar_defaults(),
        unknown_cvars: Default::default(),
        gamedata,
        fonts: Default::default(),
        focus: None,
        mouse_focus: None,
        bindings: HashMap::new(),
        mouse: (-1.0, -1.0),
        raise_counter: 0,
        anim_fx: HashMap::new(),
        last_sizes: HashMap::new(),
    });
    let lua = runtime.lua();
    natives::install(lua)?;
    let widget_stubs = apidoc::install(lua, config.ui_source_dir.as_deref())?;
    frames::install(lua, &widget_stubs)?;
    timers::install(lua)?;
    addons::install(lua)?;
    api::install(lua)?;
    apidoc::install_function_stubs(lua)?;
    install_global_strings(lua)?;
    if let (Some(exe), Some(ui)) = (
        state(lua)
            .gamedata
            .as_ref()
            .and_then(|g| g.client_exe.clone()),
        config.ui_source_dir.as_deref(),
    ) {
        let names = clientnames::undocumented_natives(&exe, ui);
        apidoc::install_named_stubs(lua, &names)?;
    }
    // WorldFrame and UIParent are created by the client itself, before any
    // UI code runs. Both cover the screen.
    for name in ["WorldFrame", "UIParent"] {
        let t = frames::create_frame(lua, "Frame", Some(name.into()), None)?;
        let id = frames::frame_id(&t)?;
        let mut st = state(lua);
        st.frames[id].points = ["TOPLEFT", "BOTTOMRIGHT"]
            .iter()
            .map(|p| state::Point {
                point: p,
                relative: None,
                relative_point: p,
                x: 0.0,
                y: 0.0,
            })
            .collect();
        // The client hides the UI while the loading screen is up and shows
        // it after login, so visible frames get OnShow then (see load_all).
        if name == "UIParent" {
            st.frames[id].shown = false;
        }
    }
    debug_assert!(state(lua).addons.iter().all(|a| a.kind == AddonKind::User));
    Ok(runtime)
}

/// Define the localized global strings from the game data, as the client
/// does before any UI code runs.
fn install_global_strings(lua: &Lua) -> mlua::Result<()> {
    let (gd, glue) = {
        let st = state(lua);
        (st.gamedata.clone(), st.glue)
    };
    let Some(gd) = gd else { return Ok(()) };
    let g = lua.globals();
    for (key, value) in gd.global_strings(glue) {
        if g.raw_get::<Value>(key.as_str())?.is_nil() {
            g.raw_set(key, value)?;
        }
    }
    Ok(())
}

/// Run a scenario's `scenario.lua`, which sets up game state before any
/// addon loads. It receives the scenario folder and a helper table (see
/// `lua/scenario.lua`).
/// Blizzard_PrintHandler sends `print` to the chat frame only. Echo
/// printed text to the sandbox console too.
fn echo_prints(lua: &Lua) -> mlua::Result<()> {
    let loaded = {
        let st = state(lua);
        st.find_addon("Blizzard_PrintHandler")
            .is_some_and(|i| st.addons[i].load_state == LoadState::Loaded)
    };
    if !loaded {
        return Ok(());
    }
    let echo = lua.create_function(|lua, args: mlua::Variadic<Value>| {
        let tostring: mlua::Function = lua.globals().get("tostring")?;
        let mut parts = Vec::with_capacity(args.len());
        for arg in args.iter() {
            parts.push(tostring.call::<String>(arg.clone())?);
        }
        state(lua).output.push(Output::Print(parts.join(" ")));
        Ok(())
    })?;
    let hook: mlua::Function = lua.globals().get("hooksecurefunc")?;
    hook.call::<()>(("print", echo))
}

fn load_scenario(lua: &Lua, dir: &std::path::Path, full_ui: bool) {
    let path = dir.join("scenario.lua");
    let code = match std::fs::read(&path) {
        Ok(code) => code,
        Err(err) => {
            state(lua).warn(format!("cannot read {}: {err}", path.display()));
            return;
        }
    };
    let helpers = match scenario_helpers(lua, dir) {
        Ok(h) => {
            let _ = h.set("fullUI", full_ui);
            h
        }
        Err(err) => {
            state(lua).warn(format!("scenario helpers: {err}"));
            return;
        }
    };
    let name = format!("@{}", path.display());
    natives::run_chunk(
        lua,
        &code,
        &name,
        (dir.to_string_lossy().to_string(), helpers),
    );
}

fn scenario_helpers(lua: &Lua, dir: &std::path::Path) -> mlua::Result<Table> {
    let account_dir = state(lua).game_account_dir.clone();
    // readGameFile(name): a file in the game's account folder, read only.
    let read_game_file = lua.create_function(move |lua, name: String| {
        let Some(root) = &account_dir else {
            return Ok(Value::Nil);
        };
        if name.contains("..") || name.starts_with('/') || name.contains('\\') {
            return Err(mlua::Error::runtime(
                "game files must be inside the account folder",
            ));
        }
        match std::fs::read(root.join(&name)) {
            Ok(bytes) => Ok(Value::String(lua.create_string(bytes)?)),
            Err(_) => Ok(Value::Nil),
        }
    })?;
    let dir = dir.to_path_buf();
    let read_file = lua.create_function(move |lua, name: String| {
        // Inside the scenario folder, or "../<scenario>/<file>" for a
        // file of another scenario (such as its capture).
        let inside = |p: &str| !p.contains("..") && !p.starts_with('/') && !p.contains('\\');
        let ok = match name.strip_prefix("../") {
            Some(rest) => inside(rest) && rest.contains('/'),
            None => inside(&name),
        };
        if !ok {
            return Err(mlua::Error::runtime(
                "scenario files must be inside the scenarios folder",
            ));
        }
        match std::fs::read(dir.join(&name)) {
            Ok(bytes) => Ok(Value::String(lua.create_string(bytes)?)),
            Err(_) => Ok(Value::Nil),
        }
    })?;
    let decode_base64 =
        lua.create_function(|lua, s: mlua::LuaString| {
            match api::base64_decode(&s.to_string_lossy()) {
                Some(b) => Ok(Value::String(lua.create_string(b)?)),
                None => Ok(Value::Nil),
            }
        })?;
    let decompress =
        lua.create_function(
            |lua, s: mlua::LuaString| match api::decompress_any(&s.as_bytes()) {
                Some(b) => Ok(Value::String(lua.create_string(b)?)),
                None => Ok(Value::Nil),
            },
        )?;
    let decode_json: mlua::Function = lua
        .globals()
        .get::<Table>("C_EncodingUtil")?
        .get("DeserializeJSON")?;
    // addAtlas(name, info): info is a C_Texture.GetAtlasInfo table.
    let add_atlas = lua.create_function(|lua, (name, info): (String, Table)| {
        let Some(gd) = state(lua).gamedata.clone() else {
            return Ok(false);
        };
        if gd.atlas(&name).is_some() {
            return Ok(false);
        }
        let n = |k: &str| info.get::<Option<f64>>(k).ok().flatten().unwrap_or(0.0);
        let b = |k: &str| info.get::<Option<bool>>(k).ok().flatten().unwrap_or(false);
        gd.add_atlas(gamedata::AtlasInfo {
            name,
            file: n("file") as u32,
            width: n("width"),
            height: n("height"),
            left: n("leftTexCoord") as f32,
            right: n("rightTexCoord") as f32,
            top: n("topTexCoord") as f32,
            bottom: n("bottomTexCoord") as f32,
            tiles_horizontally: b("tilesHorizontally"),
            tiles_vertically: b("tilesVertically"),
        });
        Ok(true)
    })?;
    // fireEvent(event, ...): deliver an event to registered frames.
    let fire_event = lua.create_function(|lua, args: mlua::MultiValue| {
        let mut args = args.into_iter();
        let Some(Value::String(event)) = args.next() else {
            return Err(mlua::Error::runtime("Usage: FireEvent(event, ...)"));
        };
        let event = event.to_string_lossy();
        frames::dispatch_event(lua, &event, args.collect());
        Ok(())
    })?;
    lua.load(include_str!("lua/scenario.lua"))
        .set_name("=[scenario]")
        .call((
            read_file,
            decode_base64,
            decompress,
            decode_json,
            add_atlas,
            lua.named_registry_value::<Value>(apidoc::RETURN_MIXINS_KEY)?,
            fire_event,
            read_game_file,
        ))
}

/// Create globals that FrameXML normally defines, when the Blizzard addons
/// that define them are not loaded.
fn ensure_framexml_globals(lua: &Lua) -> mlua::Result<()> {
    let g = lua.globals();
    if g.get::<Value>("SlashCmdList")?.is_nil() {
        g.set("SlashCmdList", lua.create_table()?)?;
    }
    Ok(())
}

/// Find and run a slash command registered through `SlashCmdList` and
/// `SLASH_<KEY><n>` globals. Returns false if no command matched.
fn run_slash_command(lua: &Lua, command: &str, rest: &str) -> bool {
    let g = lua.globals();
    let Ok(list) = g.get::<Table>("SlashCmdList") else {
        return false;
    };
    let wanted = format!("/{command}");
    let mut found = None;
    for pair in list.pairs::<String, Function>() {
        let Ok((key, func)) = pair else { continue };
        for i in 1.. {
            match g.get::<Value>(format!("SLASH_{key}{i}")) {
                Ok(Value::String(s)) => {
                    if s.to_string_lossy().eq_ignore_ascii_case(&wanted) {
                        found = Some(func.clone());
                    }
                }
                _ => break,
            }
        }
        if found.is_some() {
            break;
        }
    }
    match found {
        Some(func) => {
            natives::safe_call(lua, &func, (rest, Value::Nil));
            true
        }
        None => false,
    }
}
