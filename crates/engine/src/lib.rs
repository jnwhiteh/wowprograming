//! The addon sandbox engine: loads addons and runs them against an
//! emulation of the client's native API.

pub mod addons;
pub mod api;
pub mod apidoc;
pub mod bindings;
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
    /// dependencies are loaded too.
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
        let lua = self.runtime.lua();
        if let Some(dir) = &self.config.scenario {
            load_scenario(lua, dir);
        }
        for name in &self.config.blizzard_addons {
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
        if let Err(err) = ensure_framexml_globals(lua) {
            state(lua).warn(format!("cannot create FrameXML globals: {err}"));
        }
        if let Err(err) = bindings::load(lua) {
            state(lua).warn(format!("cannot load key bindings: {err}"));
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
        self.fire_event("VARIABLES_LOADED", ());
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
    }

    /// The current scene: visible regions in draw order.
    pub fn scene(&self) -> scene::Scene {
        scene::build(&mut state(self.runtime.lua()))
    }

    /// Render the current scene to a PNG file without a window, at
    /// `scale` pixels per UI unit.
    pub fn save_screenshot(&self, path: &std::path::Path, scale: f64) -> std::io::Result<()> {
        let scene = self.scene();
        let canvas = render::render(&scene, self.gamedata.as_deref(), scale);
        render::write_png(path, canvas.width, canvas.height, &canvas.to_rgba8())
    }

    /// Screen size in UI units.
    pub fn screen_size(&self) -> (f64, f64) {
        state(self.runtime.lua()).screen
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

    /// A key was pressed, such as `P`, `SHIFT-P` or `ESCAPE`. Runs the
    /// bound action. Returns false if the key has no binding.
    pub fn key_down(&mut self, key: &str) -> bool {
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
        .map(addons::discover_user_addons)
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
        xml: Default::default(),
        cvars: HashMap::new(),
        gamedata,
        fonts: Default::default(),
        focus: None,
        mouse_focus: None,
        bindings: HashMap::new(),
        mouse: (-1.0, -1.0),
        raise_counter: 0,
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
    }
    debug_assert!(state(lua).addons.iter().all(|a| a.kind == AddonKind::User));
    Ok(runtime)
}

/// Define the localized global strings from the game data, as the client
/// does before any UI code runs.
fn install_global_strings(lua: &Lua) -> mlua::Result<()> {
    let gd = state(lua).gamedata.clone();
    let Some(gd) = gd else { return Ok(()) };
    let g = lua.globals();
    for (key, value) in gd.global_strings() {
        if g.raw_get::<Value>(key.as_str())?.is_nil() {
            g.raw_set(key, value)?;
        }
    }
    Ok(())
}

/// Run a scenario's `scenario.lua`, which sets up game state before any
/// addon loads. The scenario folder is passed as the first argument.
fn load_scenario(lua: &Lua, dir: &std::path::Path) {
    let path = dir.join("scenario.lua");
    match std::fs::read(&path) {
        Ok(code) => {
            let name = format!("@{}", path.display());
            natives::run_chunk(lua, &code, &name, dir.to_string_lossy().to_string());
        }
        Err(err) => state(lua).warn(format!("cannot read {}: {err}", path.display())),
    }
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
