//! The addon sandbox engine: loads addons and runs them against an
//! emulation of the client's native API.

pub mod addons;
pub mod animfx;
pub mod api;
pub mod apidoc;
pub mod bindings;
pub mod clientnames;
pub mod control;
pub mod cooldown;
pub mod coverage;
pub mod editbox;
pub mod frames;
pub mod gamedata;
pub mod hyperlink;
pub mod input;
pub mod layout;
pub mod layoutcache;
pub mod loading;
pub mod maps;
pub mod messageframe;
pub mod movement;
pub mod natives;
pub mod render;
pub mod savedvars;
pub mod scene;
pub mod slider;
pub mod state;
pub mod text;
pub mod timers;
pub mod toc;
pub mod widgets;
pub mod world;
pub mod xml;

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use mlua::{FromLuaMulti, Function, IntoLuaMulti, Lua, Table, Value};
use wowprogramming_lua::{DEFAULT_SCRIPT_TIMEOUT, Runtime, mlua};

use state::{AddonKind, State, WtfPaths, state};
pub use state::{LoadState, Output};

/// The character chosen on the character select screen, for the game
/// session that EnterWorld() starts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorldEntry {
    pub guid: String,
    pub name: String,
    pub realm: String,
    /// Class file name, such as "MONK".
    pub class: String,
    /// Race file name, such as "Dwarf".
    pub race: String,
    /// ChrRaces ID.
    pub race_id: u32,
    /// 0 male, 1 female (genderEnum).
    pub sex: i64,
    pub level: i64,
    pub faction: String,
    pub zone: String,
    /// Map.db2 ID of the character's location, when known.
    pub map_id: Option<u32>,
}

impl WorldEntry {
    fn from_table(t: &Table) -> WorldEntry {
        let s = |k: &str| {
            t.get::<Option<String>>(k)
                .ok()
                .flatten()
                .unwrap_or_default()
        };
        let n = |k: &str| t.get::<Option<i64>>(k).ok().flatten();
        WorldEntry {
            guid: s("guid"),
            name: s("name"),
            realm: s("realmName"),
            class: s("classFilename"),
            race: s("raceFilename"),
            race_id: n("raceID").unwrap_or(0) as u32,
            sex: n("genderEnum").unwrap_or(0),
            level: n("experienceLevel").unwrap_or(1),
            faction: s("faction"),
            zone: s("areaName"),
            map_id: n("mapID").map(|v| v as u32),
        }
    }

    fn to_table(&self, lua: &Lua) -> mlua::Result<Table> {
        let t = lua.create_table()?;
        t.set("guid", self.guid.as_str())?;
        t.set("name", self.name.as_str())?;
        t.set("realm", self.realm.as_str())?;
        t.set("class", self.class.as_str())?;
        t.set("race", self.race.as_str())?;
        t.set("raceID", self.race_id)?;
        t.set("sex", self.sex)?;
        t.set("level", self.level)?;
        t.set("faction", self.faction.as_str())?;
        t.set("zone", self.zone.as_str())?;
        t.set("mapID", self.map_id)?;
        Ok(t)
    }
}

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
    /// The character to play, when the session starts from character
    /// select. Scenarios see it as `S.character`.
    pub world_entry: Option<WorldEntry>,
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
            world_entry: None,
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
    /// Startup work not done yet (see [`Engine::start_step`]).
    pending: std::collections::VecDeque<StartStep>,
    /// Steps done and planned in the current startup, for progress.
    start_done: usize,
    start_total: usize,
}

/// One unit of startup work. The window runs one step per frame so the
/// loading screen can show progress.
#[derive(Clone, Debug)]
enum StartStep {
    /// Saved data, the scenario and key bindings.
    Prepare {
        reloading: bool,
    },
    /// Reads the addon tables of contents and plans the addon steps.
    Plan {
        reloading: bool,
    },
    Blizzard(String),
    /// Bootstrap files, third-party addons and saved frame positions.
    AddOns,
    /// The login events, then the UI becomes visible.
    Login {
        reloading: bool,
    },
    Glue,
}

/// Progress of a startup: what the next step does, and how far it is.
#[derive(Clone, Debug)]
pub struct StartProgress {
    pub done: usize,
    pub total: usize,
    /// What the next step does, for the loading screen.
    pub next: String,
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
            pending: Default::default(),
            start_done: 0,
            start_total: 0,
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
        self.begin_start();
        while self.start_step().is_some() {}
    }

    /// Plan a startup without doing any work; [`Engine::start_step`] then
    /// does it one step at a time.
    pub fn begin_start(&mut self) {
        self.plan(false);
    }

    fn plan(&mut self, reloading: bool) {
        self.pending.clear();
        self.pending.push_back(if self.config.glue {
            StartStep::Glue
        } else {
            StartStep::Prepare { reloading }
        });
        self.start_done = 0;
        self.start_total = 1;
    }

    /// Progress of the startup, or `None` when nothing is left to do.
    pub fn start_progress(&self) -> Option<StartProgress> {
        let next = self.pending.front()?;
        let next = match next {
            StartStep::Prepare { .. } => "Setting up the character".to_string(),
            StartStep::Plan { .. } => "Reading the addon list".to_string(),
            StartStep::Blizzard(name) => format!("Loading {name}"),
            StartStep::AddOns => "Loading addons".to_string(),
            StartStep::Login { .. } => "Logging in".to_string(),
            StartStep::Glue => "Loading the login screens".to_string(),
        };
        Some(StartProgress {
            done: self.start_done,
            total: self.start_total.max(1),
            next,
        })
    }

    /// Do the next startup step. Returns the progress after it, or `None`
    /// when the startup is complete.
    pub fn start_step(&mut self) -> Option<StartProgress> {
        let step = self.pending.pop_front()?;
        match step {
            StartStep::Prepare { reloading } => {
                self.prepare();
                self.start_total = 2;
                self.pending.push_back(StartStep::Plan { reloading });
            }
            StartStep::Plan { reloading } => {
                let names = self.plan_addons();
                self.start_total = 2 + names.len() + 2;
                self.pending
                    .extend(names.into_iter().map(StartStep::Blizzard));
                self.pending.push_back(StartStep::AddOns);
                self.pending.push_back(StartStep::Login { reloading });
            }
            StartStep::Blizzard(name) => {
                let lua = self.runtime.lua();
                let result = addons::load_addon(lua, &name);
                if result != LoadState::Loaded {
                    state(lua).warn(format!("Blizzard addon {name} not loaded: {result:?}"));
                }
            }
            StartStep::AddOns => self.load_user_addons(),
            StartStep::Login { reloading } => self.login(reloading),
            StartStep::Glue => self.load_glue(),
        }
        self.start_done += 1;
        self.start_progress()
    }

    fn load_all(&mut self, reloading: bool) {
        self.plan(reloading);
        while self.start_step().is_some() {}
    }

    /// Saved data, the scenario and key bindings.
    fn prepare(&mut self) {
        let lua = self.runtime.lua();
        // Data the client keeps for the UI (Edit Mode layouts), from the
        // sandbox's own files.
        savedvars::load(lua, EDIT_MODE_FILE);
        savedvars::load(lua, CLIENT_STATE_FILE);
        if let Some(dir) = &self.config.scenario {
            let full_ui = self.config.blizzard_addons.iter().any(|n| n == "*");
            load_scenario(lua, dir, full_ui);
        }
        // Key bindings exist before the UI loads; UPDATE_BINDINGS follows
        // at login.
        if let Err(err) = bindings::load(lua) {
            state(lua).warn(format!("cannot load key bindings: {err}"));
        }
        self.apply_closed_tutorials();
    }

    /// Tutorials and help tips the player closed in the game, from the
    /// config-cache.wtf files the client writes (read only): the account's
    /// and the character's.
    fn apply_closed_tutorials(&self) {
        let Some(account) = self.config.game_account_dir.as_ref() else {
            return;
        };
        let files = [
            account.join("config-cache.wtf"),
            account
                .join(&self.config.realm)
                .join(&self.config.character)
                .join("config-cache.wtf"),
        ];
        let mut st = state(self.runtime.lua());
        for file in files {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            for (name, value) in closed_tutorial_cvars(&text) {
                st.cvars.insert(name.to_ascii_lowercase(), value);
            }
        }
    }

    /// The Blizzard addons to load, dependencies first.
    fn plan_addons(&mut self) -> Vec<String> {
        let lua = self.runtime.lua();
        let mut blizzard = Vec::new();
        for name in &self.config.blizzard_addons {
            if name == "*" {
                blizzard.extend(addons::startup_blizzard_addons(lua));
            } else {
                blizzard.push(name.clone());
            }
        }
        addons::load_order(lua, &blizzard)
    }

    fn load_user_addons(&mut self) {
        let lua = self.runtime.lua();
        run_bootstraps(lua, &self.config);
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

        // Frames the user moved come back from layout-local.txt.
        layoutcache::restore(self.runtime.lua());
    }

    fn login(&mut self, reloading: bool) {
        // The client's order, from traces of logins and reloads (see
        // docs/status.md): bindings and display first, then saved
        // variables, edit mode layouts, PLAYER_LOGIN and
        // PLAYER_ENTERING_WORLD; the loading screen ends, then the spell
        // list arrives.
        self.fire_event("UPDATE_BINDINGS", ());
        self.fire_event("DISPLAY_SIZE_CHANGED", ());
        self.fire_event("UI_SCALE_CHANGED", ());
        self.fire_event("VARIABLES_LOADED", ());
        // The edit mode layouts place the unit frames, action bars and
        // other HUD frames.
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
        self.fire_event("PLAYER_LOGIN", ());
        self.fire_event("PLAYER_ENTERING_WORLD", (!reloading, reloading));
        // The loading screen ends.
        self.fire_event("LOADING_SCREEN_DISABLED", ());
        // AssistedCombatManager and the spellbook read the spell list on
        // this event.
        self.fire_event("SPELLS_CHANGED", ());
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
        // ... and one that asks to enter the world as a character.
        let enter = lua.create_function(|lua, t: Table| {
            state(lua).enter_world = Some(WorldEntry::from_table(&t));
            Ok(())
        });
        let result = fire.and_then(|fire| {
            lua.load(include_str!("lua/glue.lua"))
                .set_name("=[C]")
                .call::<()>((fire, enter?))
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
        run_bootstraps(lua, &self.config);
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
        cooldown::update(lua);
        messageframe::update(lua);
        editbox::fire_pending_text_changes(lua);
        frames::run_on_update(lua, elapsed);
        widgets::fire_size_changes(lua);
        widgets::fire_scroll_range_changes(lua, None);
        input::refresh_focus(lua);
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

    /// Whose model the 3D view shows, as (ChrRaces ID, sex 0/1), and
    /// whether it is the character select view. On character select it is
    /// the selected character; in the game, the player.
    pub fn world_subject(&self) -> Option<(u32, u32, bool)> {
        let lua = self.runtime.lua();
        if self.config.glue {
            let (race, sex): (Option<u32>, Option<u32>) = lua
                .load(
                    "local c = GlueCharacters and GetCharacterSelection \
                     and GlueCharacters[GetCharacterSelection()] \
                     return c and c.raceID, c and c.genderEnum",
                )
                .eval()
                .unwrap_or((None, None));
            return race.map(|r| (r, sex.unwrap_or(0), true));
        }
        if let Some(e) = &state(lua).world_entry
            && e.race_id != 0
        {
            return Some((e.race_id, e.sex.max(0) as u32, false));
        }
        let (race, sex): (Option<u32>, Option<u32>) = lua
            .load("return select(3, UnitRace(\"player\")), UnitSex(\"player\")")
            .eval()
            .unwrap_or((None, None));
        race.filter(|&r| r != 0).map(|r| {
            (
                r,
                sex.map(|s| if s == 3 { 1 } else { 0 }).unwrap_or(0),
                false,
            )
        })
    }

    /// Whether Logout() asked to return to character select, once.
    pub fn take_logout(&mut self) -> bool {
        std::mem::take(&mut state(self.runtime.lua()).logout)
    }

    /// The character that EnterWorld() asked to play, once.
    pub fn take_enter_world(&mut self) -> Option<WorldEntry> {
        state(self.runtime.lua()).enter_world.take()
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
            // The glue screens are at most twice as wide as high (2880x1440
            // on a 3440x1440 monitor, with black bars at the sides).
            let aspect = if st.glue {
                (width / height).min(2.0)
            } else {
                width / height
            };
            let ui_width = ui_height * aspect;
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

    /// Press a mouse button at `(x, y)`, move by `(dx, dy)` in `steps`
    /// equal moves, and release. Positions are in screen units, y up. A
    /// frame (1/60 s) passes after the press and after each move, as in a
    /// real drag: Blizzard's scroll bars follow the cursor in OnUpdate.
    pub fn drag(&mut self, from: (f64, f64), by: (f64, f64), button: &str, steps: u32) {
        const FRAME: f64 = 1.0 / 60.0;
        let steps = steps.max(1);
        self.mouse_move(from.0, from.1);
        self.mouse_down(button);
        self.tick(FRAME);
        for i in 1..=steps {
            let t = i as f64 / steps as f64;
            self.mouse_move(from.0 + by.0 * t, from.1 + by.1 * t);
            self.tick(FRAME);
        }
        self.mouse_up(button);
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

    /// A key was pressed, such as `P`, `SHIFT-P` or `ESCAPE`. It goes to
    /// the focused edit box, then to keyboard-enabled frames, then to the
    /// key bindings. Returns false if nothing took it.
    pub fn key_down(&mut self, key: &str) -> bool {
        input::key_down(self.runtime.lua(), key)
    }

    /// A key was released. The frame that took the press gets OnKeyUp.
    pub fn key_up(&mut self, key: &str) {
        input::key_up(self.runtime.lua(), key);
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
                // Blizzard's chat parser first (secure commands such as
                // /cast live only there), then SlashCmdList for UIs
                // without the chat frame.
                let parsed = self
                    .runtime
                    .lua()
                    .globals()
                    .get::<Function>("WowProgramming_ExecuteChatLine")
                    .and_then(|f| f.call::<bool>(line))
                    .unwrap_or(false);
                if !parsed && !run_slash_command(self.runtime.lua(), &command, &rest) {
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
        save_client_files(self.runtime.lua());
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
        save_client_files(self.runtime.lua());
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

/// SavedVariables file name of the Edit Mode layouts the client keeps
/// (`C_EditMode` in `lua/client.lua`).
const EDIT_MODE_FILE: &str = "WowProgramming_EditMode";

/// SavedVariables file name of the client state the server would keep:
/// the action slots the player arranged (`lua/clientstate.lua`).
const CLIENT_STATE_FILE: &str = "WowProgramming_ClientState";
const ACTION_SLOTS_VAR: &str = "WowProgramming_ActionSlots";

/// Write the data the client keeps for the UI: moved frames and Edit
/// Mode layouts.
fn save_client_files(lua: &Lua) {
    layoutcache::save(lua);
    if state(lua).glue {
        return;
    }
    let has = |name: &str| {
        lua.globals()
            .raw_get::<Value>(name)
            .is_ok_and(|v| !v.is_nil())
    };
    if has(ACTION_SLOTS_VAR) {
        savedvars::save(lua, CLIENT_STATE_FILE, &[], &[ACTION_SLOTS_VAR.to_string()]);
    }
    if has("WowProgramming_EditModeAccount") || has("WowProgramming_EditModeCharacter") {
        savedvars::save(
            lua,
            EDIT_MODE_FILE,
            &["WowProgramming_EditModeAccount".to_string()],
            &["WowProgramming_EditModeCharacter".to_string()],
        );
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
        enter_world: None,
        logout: false,
        world_entry: config.world_entry.clone(),
        pixel_scale: config.pixel_scale,
        xml: Default::default(),
        cvars: HashMap::new(),
        cvar_defaults: api::builtin_cvar_defaults(),
        unknown_cvars: Default::default(),
        gamedata,
        fonts: Default::default(),
        focus: None,
        mouse_focus: None,
        mouse: (-1.0, -1.0),
        raise_counter: 0,
        press: None,
        last_click: None,
        moving: None,
        slider_drag: None,
        hover_link: None,
        hover_generation: 0,
        key_targets: HashMap::new(),
        anim_fx: HashMap::new(),
        last_sizes: HashMap::new(),
        last_scroll_ranges: HashMap::new(),
    });
    let lua = runtime.lua();
    natives::install(lua)?;
    // Logout() returns to character select. The client waits 20 seconds
    // unless the player is resting; the sandbox logs out at once.
    for name in ["Logout", "ForceLogout"] {
        lua.globals().set(
            name,
            lua.create_function(|lua, _: mlua::MultiValue| {
                state(lua).logout = true;
                Ok(())
            })?,
        )?;
    }
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
        // UIParent stays shown while the UI loads: Blizzard code relies on
        // it (a cast bar fades itself out at login only when IsVisible).
    }
    debug_assert!(state(lua).addons.iter().all(|a| a.kind == AddonKind::User));
    Ok(runtime)
}

/// The bitfield CVars of closed tutorials and help tips in a
/// config-cache.wtf file: `SET closedInfoFrames "..."` lines.
fn closed_tutorial_cvars(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("SET ")?;
            let (name, value) = rest.split_once(' ')?;
            let wanted = ["closedInfoFrames", "closedInfoFramesAccountWide"];
            if !wanted.iter().any(|w| w.eq_ignore_ascii_case(name)) {
                return None;
            }
            Some((name.to_string(), value.trim().trim_matches('"').to_string()))
        })
        .collect()
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
            g.raw_set(key, lua.create_string(unescape_global_string(&value))?)?;
        }
    }
    Ok(())
}

/// GlobalStrings.db2 keeps the Lua escapes of the strings' source: `\n`,
/// `\"`, `\\` and decimal codes such as `\32`. Other backslashes are
/// part of texture paths (`Interface\Icons\...`) and stay.
fn unescape_global_string(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' || i + 1 >= b.len() {
            out.push(b[i]);
            i += 1;
            continue;
        }
        match b[i + 1] {
            b'n' => {
                out.push(b'\n');
                i += 2;
            }
            b'"' | b'\\' => {
                out.push(b[i + 1]);
                i += 2;
            }
            b'0'..=b'9' => {
                let mut j = i + 1;
                let mut code: u32 = 0;
                while j < b.len() && j < i + 4 && b[j].is_ascii_digit() {
                    code = code * 10 + (b[j] - b'0') as u32;
                    j += 1;
                }
                if code <= 255 {
                    out.push(code as u8);
                    i = j;
                } else {
                    out.push(b'\\');
                    i += 1;
                }
            }
            _ => {
                out.push(b'\\');
                i += 1;
            }
        }
    }
    out
}

/// Run the `[Bootstrap]` files of load-on-demand addons: all Blizzard ones
/// with the full default UI, and those the config names.
fn run_bootstraps(lua: &Lua, config: &EngineConfig) {
    let mut names = Vec::new();
    if config.blizzard_addons.iter().any(|n| n == "*") {
        names = addons::startup_bootstrap_addons(lua);
    }
    names.extend(config.bootstrap_addons.iter().cloned());
    for name in &names {
        addons::run_bootstrap(lua, name);
    }
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
            let entry = state(lua).world_entry.clone();
            if let Some(entry) = entry
                && let Ok(t) = entry.to_table(lua)
            {
                let _ = h.set("character", t);
            }
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
            slice: None,
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
    let helpers: Table = lua
        .load(include_str!("lua/scenario.lua"))
        .set_name("=[scenario]")
        .call((
            read_file,
            decode_base64,
            decompress,
            decode_json,
            add_atlas,
            lua.named_registry_value::<Value>(apidoc::RETURN_MIXINS_KEY)?,
            fire_event.clone(),
            read_game_file,
        ))?;
    // The client state model (S.State); a scenario turns it on.
    lua.load(include_str!("lua/clientstate.lua"))
        .set_name("=[C]")
        .call::<()>((helpers.clone(), fire_event))?;
    Ok(helpers)
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

#[cfg(test)]
mod tests {
    use super::{closed_tutorial_cvars, unescape_global_string};

    #[test]
    fn unescapes_global_strings() {
        assert_eq!(
            unescape_global_string(r"Latency:\n%.0f ms"),
            b"Latency:\n%.0f ms".to_vec()
        );
        assert_eq!(unescape_global_string(r"yells:\32"), b"yells: ".to_vec());
        assert_eq!(
            unescape_global_string(r#"|TInterface\Icons\X|t \\A \"q\""#),
            br#"|TInterface\Icons\X|t \A "q""#.to_vec()
        );
    }

    #[test]
    fn reads_closed_tutorials_from_config_cache() {
        let text = "SET uiScale \"1\"\nSET closedInfoFrames \"@@rDAB\"\nSET closedInfoFramesAccountWide \"}_ow_A\"\n";
        assert_eq!(
            closed_tutorial_cvars(text),
            vec![
                ("closedInfoFrames".to_string(), "@@rDAB".to_string()),
                (
                    "closedInfoFramesAccountWide".to_string(),
                    "}_ow_A".to_string()
                ),
            ]
        );
    }
}
