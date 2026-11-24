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

use mlua::{AppDataRefMut, Function, Lua, Table};
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

/// Marker stored at index `[0]` of every widget table.
pub struct FrameRef(pub FrameId);

#[derive(Clone, Debug)]
pub struct Point {
    pub point: &'static str,
    /// The region the point is relative to. `None` means the parent.
    pub relative: Option<FrameId>,
    pub relative_point: &'static str,
    pub x: f64,
    pub y: f64,
}

/// The image shown by a texture.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum TexFile {
    #[default]
    None,
    /// A path such as `Interface\Buttons\WHITE8X8`.
    Path(String),
    /// A FileDataID.
    Id(i64),
    /// A solid colour from `SetColorTexture`.
    Color([f32; 4]),
}

#[derive(Clone, Debug)]
pub struct TexState {
    pub file: TexFile,
    pub atlas: Option<String>,
    /// Texture coordinates of the corners: UL, LL, UR, LR as (x, y) pairs.
    /// With an atlas they are relative to the atlas region, as in the client.
    pub coords: [f32; 8],
    /// The atlas region in the file: left, top, right, bottom.
    pub atlas_rect: Option<[f32; 4]>,
    pub vertex_color: [f32; 4],
    pub desaturation: f32,
    pub blend_mode: String,
    pub horiz_tile: bool,
    pub vert_tile: bool,
    pub rotation: f32,
    pub gradient: Option<([f32; 4], [f32; 4], bool)>,
    /// Flipbook frame left showing when a flipbook animation finished, as
    /// fractions of the texture coordinates: left, top, right, bottom.
    pub flipbook_frame: Option<[f32; 4]>,
}

impl Default for TexState {
    fn default() -> Self {
        Self {
            file: TexFile::None,
            atlas: None,
            coords: [0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0],
            atlas_rect: None,
            vertex_color: [1.0; 4],
            desaturation: 0.0,
            blend_mode: "BLEND".into(),
            horiz_tile: false,
            vert_tile: false,
            rotation: 0.0,
            gradient: None,
            flipbook_frame: None,
        }
    }
}

/// Font attributes. `None` fields come from the font object, if any.
#[derive(Clone, Debug, Default)]
pub struct FontAttrs {
    pub path: Option<String>,
    pub height: Option<f64>,
    pub flags: Option<String>,
    pub color: Option<[f32; 4]>,
    pub shadow_color: Option<[f32; 4]>,
    pub shadow_offset: Option<(f64, f64)>,
    pub justify_h: Option<String>,
    pub justify_v: Option<String>,
    pub spacing: Option<f64>,
    /// Font object (a `Font` widget) that supplies unset attributes.
    pub font_object: Option<FrameId>,
}

#[derive(Clone, Debug)]
pub struct TextState {
    pub text: String,
    pub max_lines: i64,
    pub word_wrap: bool,
    pub non_space_wrap: bool,
    pub text_scale: f64,
    pub alpha_gradient: Option<(i64, i64)>,
    /// Edit box text insets: left, right, top, bottom.
    pub insets: [f64; 4],
}

impl Default for TextState {
    fn default() -> Self {
        Self {
            insets: [0.0; 4],
            text: String::new(),
            max_lines: 0,
            word_wrap: true,
            non_space_wrap: false,
            text_scale: 1.0,
            alpha_gradient: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ButtonState {
    pub enabled: bool,
    pub pushed: bool,
    pub locked_highlight: bool,
    pub checked: bool,
    pub normal: Option<FrameId>,
    pub pushed_tex: Option<FrameId>,
    pub disabled: Option<FrameId>,
    pub highlight: Option<FrameId>,
    pub checked_tex: Option<FrameId>,
    pub disabled_checked: Option<FrameId>,
    pub font_string: Option<FrameId>,
    pub normal_font: Option<FrameId>,
    pub highlight_font: Option<FrameId>,
    pub disabled_font: Option<FrameId>,
    pub pushed_offset: (f64, f64),
    pub clicks: Vec<String>,
    pub motion_scripts_while_disabled: bool,
}

#[derive(Clone, Debug, Default)]
pub struct AnimState {
    pub playing: bool,
    pub paused: bool,
    pub elapsed: f64,
    pub duration: f64,
    pub start_delay: f64,
    pub end_delay: f64,
    pub order: i64,
    pub looping: String,
    pub reverse: bool,
    pub target: Option<FrameId>,
    pub child_key: Option<String>,
    pub target_key: Option<String>,
    pub from_alpha: f64,
    pub to_alpha: f64,
    pub set_to_final_alpha: bool,
    /// Translation: offset in the target's units.
    pub offset: (f64, f64),
    /// Scale: start and end scale factors.
    pub scale_from: (f64, f64),
    pub scale_to: (f64, f64),
    /// Scale and rotation origin: point and offset.
    pub origin: Option<(String, f64, f64)>,
    /// Rotation in radians.
    pub radians: f64,
    /// FlipBook: rows, columns and frames.
    pub flipbook: (u32, u32, u32),
    /// Easing: NONE, IN, OUT or IN_OUT.
    pub smoothing: String,
}

/// One widget: a frame, region, font object or animation. Its Lua table
/// and script handlers live in Lua tables owned by [`State`], because mlua
/// limits how many Lua values Rust may hold at once.
pub struct FrameData {
    pub object_type: &'static str,
    pub name: Option<String>,
    pub parent: Option<FrameId>,
    /// Child frames (not regions).
    pub children: Vec<FrameId>,
    /// Textures, font strings and other regions owned by this frame.
    pub regions: Vec<FrameId>,
    /// Animation groups (for frames and regions) or animations (for groups).
    pub anims: Vec<FrameId>,
    pub shown: bool,
    /// Names of scripts that have a handler or hooks.
    pub scripts: Vec<String>,
    pub events: Vec<String>,
    pub unit_filters: HashMap<String, Vec<String>>,
    pub all_events: bool,
    pub id: i64,
    pub width: f64,
    pub height: f64,
    pub points: Vec<Point>,
    pub alpha: f64,
    pub scale: f64,
    pub strata: &'static str,
    pub strata_explicit: bool,
    pub level: i64,
    pub toplevel: bool,
    pub mouse_enabled: bool,
    pub mouse_motion: bool,
    pub mouse_over: bool,
    pub movable: bool,
    /// Mouse buttons that start a drag (`RegisterForDrag`).
    pub drag_buttons: Vec<String>,
    /// Moved by the user (`StartMoving`, `SetUserPlaced`).
    pub user_placed: bool,
    pub dont_save_position: bool,
    pub clamped: bool,
    /// `SetClampRectInsets`: left, right, top, bottom.
    pub clamp_insets: [f64; 4],
    pub resizable: bool,
    /// `SetResizeBounds`: min width, min height, max width, max height
    /// (0 for no maximum).
    pub resize_bounds: [f64; 4],
    pub keyboard_enabled: bool,
    pub propagate_keyboard: bool,
    /// `LockHighlight` on a frame: the HIGHLIGHT layer stays shown.
    pub highlight_locked: bool,
    /// Disabled edit box or slider (buttons keep this in `button`).
    pub disabled: bool,
    pub clips_children: bool,
    pub ignore_parent_alpha: bool,
    pub ignore_parent_scale: bool,
    pub hit_insets: [f64; 4],
    pub attributes: Option<Table>,
    pub forbidden: bool,
    pub draw_layer: u8,
    pub sublevel: i64,
    pub tex: Option<Box<TexState>>,
    pub font: Option<Box<FontAttrs>>,
    pub text: Option<Box<TextState>>,
    pub button: Option<Box<ButtonState>>,
    pub anim: Option<Box<AnimState>>,
    /// Slider and status bar values: min, max, value.
    pub value: (f64, f64, f64),
    pub status_texture: Option<FrameId>,
    pub orientation: String,
    pub scroll_child: Option<FrameId>,
    pub scroll: (f64, f64),
    pub focus: bool,
    pub masks: Vec<FrameId>,
    /// Created from XML: a texture without anchors or size then fills its
    /// parent, as in the client.
    pub from_xml: bool,
}

impl FrameData {
    pub fn new(object_type: &'static str) -> FrameData {
        FrameData {
            object_type,
            name: None,
            parent: None,
            children: Vec::new(),
            regions: Vec::new(),
            anims: Vec::new(),
            shown: true,
            scripts: Vec::new(),
            events: Vec::new(),
            unit_filters: HashMap::new(),
            all_events: false,
            id: 0,
            width: 0.0,
            height: 0.0,
            points: Vec::new(),
            alpha: 1.0,
            scale: 1.0,
            strata: "MEDIUM",
            strata_explicit: false,
            level: 0,
            toplevel: false,
            mouse_enabled: false,
            mouse_motion: false,
            mouse_over: false,
            movable: false,
            drag_buttons: Vec::new(),
            user_placed: false,
            dont_save_position: false,
            clamped: false,
            clamp_insets: [0.0; 4],
            resizable: false,
            resize_bounds: [0.0; 4],
            keyboard_enabled: false,
            propagate_keyboard: false,
            highlight_locked: false,
            disabled: false,
            clips_children: false,
            ignore_parent_alpha: false,
            ignore_parent_scale: false,
            hit_insets: [0.0; 4],
            attributes: None,
            forbidden: false,
            draw_layer: 2,
            sublevel: 0,
            tex: None,
            font: None,
            text: None,
            button: None,
            anim: None,
            value: (0.0, 0.0, 0.0),
            status_texture: None,
            orientation: "HORIZONTAL".into(),
            scroll_child: None,
            scroll: (0.0, 0.0),
            focus: false,
            masks: Vec::new(),
            from_xml: false,
        }
    }

    pub fn is_enabled(&self) -> bool {
        match self.button.as_deref() {
            Some(b) => b.enabled,
            None => !self.disabled,
        }
    }

    pub fn has_script(&self, name: &str) -> bool {
        self.scripts.iter().any(|s| s == name)
    }

    pub fn is_region(&self) -> bool {
        matches!(
            self.object_type,
            "Texture" | "FontString" | "MaskTexture" | "Line"
        )
    }

    pub fn is_frame(&self) -> bool {
        !self.is_region() && self.anim.is_none() && self.object_type != "Font"
    }
}

/// A mouse button held down over a frame. The frame gets the release
/// (mouse capture).
#[derive(Clone, Debug)]
pub struct Press {
    pub frame: FrameId,
    pub button: String,
    /// Mouse position at the press, in screen units.
    pub at: (f64, f64),
    /// OnDragStart fired for this press.
    pub dragging: bool,
}

#[derive(Clone, Debug)]
pub struct LastClick {
    pub frame: FrameId,
    pub button: String,
    pub time: f64,
    /// The click was the second of a double click.
    pub double: bool,
}

/// A frame following the mouse after `StartMoving` or `StartSizing`.
#[derive(Clone, Debug)]
pub struct Moving {
    pub frame: FrameId,
    /// The point being dragged when sizing; `None` when moving.
    pub sizing: Option<&'static str>,
    pub mouse: (f64, f64),
    pub rect: crate::layout::Rect,
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
    /// The `[Bootstrap]` files have run.
    pub bootstrapped: bool,
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
    /// Widget tables, indexed by widget id + 1.
    pub objects: Table,
    /// Script handlers: widget id + 1 -> { [name] = { handler = f, hooks = {...} } }.
    pub script_table: Table,
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
    /// Bumped whenever something changes that can move a region, so cached
    /// rectangles are recomputed.
    pub layout_generation: u64,
    pub rect_cache: Vec<Option<(u64, Option<crate::layout::Rect>)>>,
    /// The glue environment (login and character select) instead of the
    /// game. Decides which addons may load.
    pub glue: bool,
    /// The game's account folder, for scenarios (read only).
    pub game_account_dir: Option<PathBuf>,
    /// Set by Logout() in the game: return to character select.
    pub logout: bool,
    /// Set by EnterWorld() on the character select screen.
    pub enter_world: Option<crate::WorldEntry>,
    /// The character this game session plays (from character select).
    pub world_entry: Option<crate::WorldEntry>,
    /// Size of the screen in UI units (the size of UIParent at scale 1).
    pub screen: (f64, f64),
    /// Physical pixels per UI unit at scale 1.
    pub pixel_scale: f64,
    pub xml: crate::xml::XmlState,
    pub cvars: HashMap<String, String>,
    /// Default values of CVars, by lower-case name.
    pub cvar_defaults: HashMap<String, String>,
    /// CVars that were read but have neither a value nor a default.
    pub unknown_cvars: std::collections::BTreeSet<String>,
    pub gamedata: Option<Rc<crate::gamedata::GameData>>,
    pub fonts: crate::text::FontCache,
    pub focus: Option<FrameId>,
    pub mouse_focus: Option<FrameId>,
    pub bindings: HashMap<String, Function>,
    /// Mouse position in screen units.
    pub mouse: (f64, f64),
    /// The mouse button held down, and the frame that captured it.
    pub press: Option<Press>,
    /// The last click, for double clicks.
    pub last_click: Option<LastClick>,
    /// The frame being moved or sized with the mouse.
    pub moving: Option<Moving>,
    /// Layout generation when the mouse focus was last checked.
    pub hover_generation: u64,
    /// Order in which frames were last raised, for frames at the same level.
    pub raise_counter: u64,
    /// Visual effects of the animations that are playing, by target.
    pub anim_fx: HashMap<FrameId, crate::animfx::AnimFx>,
    /// Sizes last reported to `OnSizeChanged` handlers.
    pub last_sizes: HashMap<FrameId, (f64, f64)>,
}

impl State {
    /// The Lua table of a widget.
    pub fn table(&self, id: FrameId) -> Table {
        self.objects
            .raw_get(id as i64 + 1)
            .expect("widget table missing")
    }

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
