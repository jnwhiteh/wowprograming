//! Engine state shared by all native API functions.
//!
//! The state is stored as Lua app data, so Rust callbacks reach it through
//! the `&Lua` they receive. Never call into Lua while holding a borrow of
//! the state: collect what you need, drop the borrow, then call.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use mlua::{AppDataRefMut, Function, Lua, Table, Value};
use wowprogramming_lua::{Watchdog, mlua};

use crate::toc::Toc;

/// One line of output from the sandbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    /// Text from `print` and the default chat output.
    Print(String),
    /// A Lua error, as reported by the default error handler.
    Error(String),
    /// A problem found by the engine, such as a missing addon file.
    Warning(String),
    /// A message sent to the client log with `C_Log`.
    Log(String),
}

impl std::fmt::Display for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Output::Print(s) => write!(f, "{s}"),
            Output::Error(s) => write!(f, "[error] {s}"),
            Output::Warning(s) => write!(f, "[warning] {s}"),
            Output::Log(s) => write!(f, "[log] {s}"),
        }
    }
}

pub type FrameId = usize;

/// Marker stored at index `[0]` of every frame table.
pub struct FrameRef(pub FrameId);

#[derive(Default)]
pub struct Script {
    pub handler: Option<Function>,
    pub hooks: Vec<Function>,
}

#[derive(Clone)]
pub struct Point {
    pub point: String,
    pub relative_to: Value,
    pub relative_point: String,
    pub x: f64,
    pub y: f64,
}

pub struct FrameData {
    pub object_type: &'static str,
    pub name: Option<String>,
    pub table: Table,
    pub parent: Option<FrameId>,
    pub children: Vec<FrameId>,
    pub shown: bool,
    pub scripts: HashMap<String, Script>,
    pub events: Vec<String>,
    pub unit_filters: HashMap<String, Vec<String>>,
    pub all_events: bool,
    pub id: i64,
    pub width: f64,
    pub height: f64,
    pub points: Vec<Point>,
    pub alpha: f64,
    pub scale: f64,
    pub strata: String,
    pub level: i64,
    pub mouse_enabled: bool,
    pub movable: bool,
    pub attributes: Option<Table>,
    pub forbidden: bool,
}

pub struct Timer {
    pub id: u64,
    pub due: f64,
    pub interval: f64,
    /// Remaining calls for a ticker. `None` repeats forever.
    pub remaining: Option<u64>,
    pub repeating: bool,
    pub callback: Function,
    /// The object passed to the callback (`NewTimer` and `NewTicker`).
    pub handle: Option<Table>,
    pub cancelled: Rc<Cell<bool>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddonKind {
    Blizzard,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadState {
    NotLoaded,
    Loading,
    Loaded,
    /// The addon could not be loaded. The reason uses the client's codes,
    /// such as `MISSING` or `DEP_MISSING`.
    Failed(String),
}

pub struct Addon {
    pub name: String,
    pub dir: PathBuf,
    pub toc: Toc,
    pub kind: AddonKind,
    pub load_state: LoadState,
    pub namespace: Option<Table>,
}

/// Where SavedVariables files live.
#[derive(Debug, Clone)]
pub struct WtfPaths {
    pub root: PathBuf,
    pub account: String,
    pub realm: String,
    pub character: String,
}

pub struct State {
    pub output: Vec<Output>,
    pub watchdog: Rc<Watchdog>,
    pub frames: Vec<FrameData>,
    pub event_frames: HashMap<String, Vec<FrameId>>,
    pub all_event_frames: Vec<FrameId>,
    pub frame_metatables: HashMap<&'static str, Table>,
    pub time: f64,
    pub last_elapsed: f64,
    pub started_at: Instant,
    pub profile_start: Instant,
    pub timers: Vec<Timer>,
    pub next_timer_id: u64,
    pub addons: Vec<Addon>,
    pub ui_source_dir: Option<PathBuf>,
    pub wtf: WtfPaths,
    pub warned_templates: Vec<String>,
}

impl State {
    pub fn warn(&mut self, msg: impl Into<String>) {
        self.output.push(Output::Warning(msg.into()));
    }

    pub fn find_addon(&self, name: &str) -> Option<usize> {
        self.addons
            .iter()
            .position(|a| a.name.eq_ignore_ascii_case(name))
    }

    /// User addons in the order the client lists them.
    pub fn user_addons(&self) -> impl Iterator<Item = (usize, &Addon)> {
        self.addons
            .iter()
            .enumerate()
            .filter(|(_, a)| a.kind == AddonKind::User)
    }
}

/// Borrow the engine state. Panics if it is already borrowed, which means
/// a Rust function called into Lua while holding the state.
pub fn state(lua: &Lua) -> AppDataRefMut<'_, State> {
    lua.app_data_mut::<State>()
        .expect("engine state missing or already borrowed")
}
