//! Loading Blizzard's UI XML: templates, frames, regions, fonts, scripts.
//!
//! Elements are parsed into an owned tree, so templates can be kept and
//! applied later by `CreateFrame` and the `Create*` methods. Building an
//! object follows the client:
//!
//! 1. Create the object, set its global name and its parent key.
//! 2. Apply the intrinsic and `inherits` templates, then the element itself:
//!    mixins, attributes, then child elements (regions, child frames,
//!    scripts, key values ...).
//! 3. When the whole tree is built, apply anchors (so they can refer to
//!    siblings created later), then fire `OnLoad` for each object, children
//!    before parents.
//!
//! Widget properties are set through the native methods of the widget type,
//! not through methods that mixins may override.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use mlua::{Function, IntoLuaMulti, Lua, MultiValue, Result, Table, Value};
use wowprogramming_lua::mlua;

use crate::frames::{
    self, canonical_any_type, canonical_type, create_object, fire_script, frame_id,
};
use crate::natives::run_chunk;
use crate::state::{FrameId, TexFile, state};
use crate::widgets::{layer_index, strata_name};

/// An XML element, owned so templates outlive the parsed document.
#[derive(Debug)]
pub struct Elem {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Rc<Elem>>,
    pub text: String,
    pub file: Rc<str>,
    pub line: u32,
}

impl Elem {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn attr_bool(&self, name: &str) -> Option<bool> {
        self.attr(name)
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
    }

    fn attr_f64(&self, name: &str) -> Option<f64> {
        self.attr(name).and_then(|v| v.trim().parse().ok())
    }

    fn child(&self, tag: &str) -> Option<&Rc<Elem>> {
        self.children
            .iter()
            .find(|c| c.tag.eq_ignore_ascii_case(tag))
    }
}

#[derive(Default)]
pub struct XmlState {
    templates: HashMap<String, Rc<Elem>>,
    /// Intrinsic element names: base widget type and definition.
    intrinsics: HashMap<String, (&'static str, Rc<Elem>)>,
    warned: HashSet<String>,
}

/// State of one build: work done after the whole tree exists.
#[derive(Default)]
struct Build {
    anchors: Vec<(FrameId, Rc<Elem>)>,
    /// `<MaskedTextures>` of mask textures, applied with the anchors.
    masked: Vec<(FrameId, Rc<Elem>)>,
    onloads: Vec<FrameId>,
    /// Script handlers of intrinsics, combined with later handlers.
    intrinsic_scripts: HashMap<(FrameId, String), (Function, bool)>,
}

/// Per-file context for `Script` and `Include` elements.
#[derive(Clone)]
struct FileCtx {
    dir: PathBuf,
    display_dir: String,
    addon: String,
    namespace: Option<Table>,
}

fn warn_once(lua: &Lua, key: &str, msg: impl FnOnce() -> String) {
    let mut st = state(lua);
    if st.xml.warned.insert(key.to_string()) {
        let m = msg();
        st.warn(m);
    }
}

/// Parse an XML file into owned elements.
fn parse(text: &str, file: &str) -> std::result::Result<Rc<Elem>, String> {
    let opts = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let doc = roxmltree::Document::parse_with_options(text, opts).map_err(|e| e.to_string())?;
    let file: Rc<str> = Rc::from(file);
    fn convert(doc: &roxmltree::Document, node: roxmltree::Node, file: &Rc<str>) -> Rc<Elem> {
        let mut text = String::new();
        let mut children = Vec::new();
        for c in node.children() {
            if c.is_element() {
                children.push(convert(doc, c, file));
            } else if c.is_text() {
                text.push_str(c.text().unwrap_or(""));
            }
        }
        Rc::new(Elem {
            tag: node.tag_name().name().to_string(),
            attrs: node
                .attributes()
                .map(|a| (a.name().to_string(), a.value().to_string()))
                .collect(),
            children,
            text,
            file: file.clone(),
            line: doc.text_pos_at(node.range().start).row,
        })
    }
    Ok(convert(&doc, doc.root_element(), &file))
}

/// Load an XML file from an addon. Errors go to the error handler.
pub fn load_file(lua: &Lua, path: &Path, display: &str, addon: &str, namespace: Option<Table>) {
    let text = match std::fs::read(path) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) => {
            state(lua).warn(format!("Couldn't open {display}: {e}"));
            return;
        }
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let root = match parse(text, display) {
        Ok(r) => r,
        Err(e) => {
            crate::natives::report_error(lua, &format!("{display}: {e}"));
            return;
        }
    };
    let ctx = FileCtx {
        dir: path.parent().map(Path::to_path_buf).unwrap_or_default(),
        display_dir: display
            .rsplit_once('/')
            .map(|(d, _)| d.to_string())
            .unwrap_or_default(),
        addon: addon.to_string(),
        namespace,
    };
    process_ui(lua, &root, &ctx);
}

fn process_ui(lua: &Lua, ui: &Rc<Elem>, ctx: &FileCtx) {
    for child in &ui.children {
        let tag = child.tag.as_str();
        match tag {
            "Script" => run_script_element(lua, child, ctx),
            "Include" => {
                if let Some(file) = child.attr("file") {
                    match resolve_file(lua, ctx, file) {
                        Ok((p, display)) => {
                            load_file(lua, &p, &display, &ctx.addon, ctx.namespace.clone())
                        }
                        Err(display) => state(lua).warn(format!("Couldn't open {display}")),
                    }
                }
            }
            "Font" => {
                if let Err(e) = create_font(lua, child) {
                    report(lua, child, &e.to_string());
                }
            }
            "FontFamily" => {
                let member = child
                    .children
                    .iter()
                    .find(|m| {
                        m.attr("alphabet")
                            .is_some_and(|a| a.eq_ignore_ascii_case("roman"))
                    })
                    .or_else(|| child.children.first());
                if let Some(font) = member.and_then(|m| m.child("Font")) {
                    let mut attrs = font.attrs.clone();
                    if let Some(n) = child.attr("name") {
                        attrs.retain(|(k, _)| !k.eq_ignore_ascii_case("name"));
                        attrs.push(("name".into(), n.to_string()));
                    }
                    let merged = Rc::new(Elem {
                        tag: "Font".into(),
                        attrs,
                        children: font.children.clone(),
                        text: String::new(),
                        file: font.file.clone(),
                        line: font.line,
                    });
                    if let Err(e) = create_font(lua, &merged) {
                        report(lua, child, &e.to_string());
                    }
                }
            }
            "ScopedModifier" => process_ui(lua, child, ctx),
            _ => {
                let is_widget = resolve_type(lua, tag).is_some();
                if !is_widget {
                    warn_once(lua, &format!("tag:{tag}"), || {
                        format!(
                            "{}:{}: unsupported XML element <{tag}>",
                            child.file, child.line
                        )
                    });
                    continue;
                }
                let name = child.attr("name");
                if child.attr_bool("intrinsic") == Some(true) {
                    if let (Some(name), Some((base, _))) = (name, resolve_type(lua, tag)) {
                        let mut st = state(lua);
                        st.xml
                            .intrinsics
                            .insert(name.to_ascii_lowercase(), (base, child.clone()));
                    }
                    continue;
                }
                if child.attr_bool("virtual") == Some(true) {
                    if let Some(name) = name {
                        state(lua)
                            .xml
                            .templates
                            .insert(name.to_ascii_lowercase(), child.clone());
                    }
                    continue;
                }
                let parent_name = child
                    .attr("parent")
                    .map(str::to_string)
                    .or_else(|| inherited_attr(lua, child, "parent", 0));
                let parent = match parent_name.as_deref() {
                    Some(p) => match lua.globals().get::<Value>(p) {
                        Ok(Value::Table(t)) => frame_id(&t).ok(),
                        _ => None,
                    },
                    None => None,
                };
                let mut build = Build::default();
                let result = build_object(lua, &mut build, child, parent, None);
                finish(lua, build);
                if let Err(e) = result {
                    report(lua, child, &e.to_string());
                }
            }
        }
    }
}

/// An attribute from the templates an element inherits, searched in
/// `inherits` order and depth first.
fn inherited_attr(lua: &Lua, elem: &Elem, attr: &str, depth: usize) -> Option<String> {
    if depth > 16 {
        return None;
    }
    let names = elem.attr("inherits")?;
    for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        let template = state(lua)
            .xml
            .templates
            .get(&name.to_ascii_lowercase())
            .cloned();
        if let Some(t) = template {
            if let Some(v) = t.attr(attr) {
                return Some(v.to_string());
            }
            if let Some(v) = inherited_attr(lua, &t, attr, depth + 1) {
                return Some(v);
            }
        }
    }
    None
}

fn report(lua: &Lua, elem: &Elem, msg: &str) {
    crate::natives::report_error(lua, &format!("{}:{}: {msg}", elem.file, elem.line));
}

/// Resolve the `file` of a `Script` or `Include` element: relative to the
/// XML file, or else relative to the addon folder, as the client does.
/// Returns the path and the display name, or the display name on failure.
fn resolve_file(
    lua: &Lua,
    ctx: &FileCtx,
    file: &str,
) -> std::result::Result<(PathBuf, String), String> {
    let rel = file.replace('\\', "/");
    let display = format!("{}/{rel}", ctx.display_dir);
    if let Some(p) = crate::addons::resolve_path(&ctx.dir, &rel) {
        return Ok((p, display));
    }
    let root = {
        let st = state(lua);
        match st.find_addon(&ctx.addon) {
            Some(i) => Some(st.addons[i].dir.clone()),
            None => st.ui_source_dir.as_ref().and_then(|ui| {
                crate::addons::resolve_path(ui, &format!("Interface/AddOns/{}", ctx.addon))
            }),
        }
    };
    match root.and_then(|r| crate::addons::resolve_path(&r, &rel)) {
        Some(p) => Ok((p, format!("Interface/AddOns/{}/{rel}", ctx.addon))),
        None => Err(display),
    }
}

fn run_script_element(lua: &Lua, elem: &Rc<Elem>, ctx: &FileCtx) {
    let args = (ctx.addon.as_str(), ctx.namespace.clone());
    if let Some(file) = elem.attr("file") {
        match resolve_file(lua, ctx, file) {
            Ok((p, display)) => match std::fs::read(&p) {
                Ok(code) => {
                    run_chunk(lua, &code, &format!("@{display}"), args);
                }
                Err(e) => state(lua).warn(format!("Couldn't open {display}: {e}")),
            },
            Err(display) => state(lua).warn(format!("Couldn't open {display}")),
        }
    } else if !elem.text.trim().is_empty() {
        let code = pad_lines(elem.line, &elem.text);
        run_chunk(lua, code.as_bytes(), &format!("@{}", elem.file), args);
    }
}

fn pad_lines(line: u32, code: &str) -> String {
    let mut s = "\n".repeat(line.saturating_sub(1) as usize);
    s.push_str(code);
    s
}

/// Map an element name to a widget type, following intrinsics.
fn resolve_type(lua: &Lua, tag: &str) -> Option<(&'static str, Option<Rc<Elem>>)> {
    if let Some((base, def)) = state(lua).xml.intrinsics.get(&tag.to_ascii_lowercase()) {
        return Some((*base, Some(def.clone())));
    }
    let t = match tag {
        "CheckButton" | "Checkbox" => "CheckButton",
        "Texture" | "MaskTexture" | "Line" | "FontString" | "AnimationGroup" => {
            canonical_any_type(tag)?
        }
        "ScrollingMessageFrame" => "MessageFrame",
        t if frames::is_animation_type(t) => canonical_any_type(t)?,
        t => canonical_type(t)?,
    };
    Some((t, None))
}

/// Run anchors and `OnLoad` scripts collected during a build.
fn finish(lua: &Lua, build: Build) {
    for (id, anchors) in &build.anchors {
        apply_anchors(lua, *id, anchors);
    }
    for (id, list) in &build.masked {
        apply_masked_textures(lua, *id, list);
    }
    for id in build.onloads {
        fire_script(lua, id, "OnLoad", ());
    }
}

/// Create an object from an element, with all its children.
fn build_object(
    lua: &Lua,
    build: &mut Build,
    elem: &Rc<Elem>,
    parent: Option<FrameId>,
    layer: Option<(u8, i64)>,
) -> Result<Table> {
    let (object_type, intrinsic) = resolve_type(lua, &elem.tag)
        .ok_or_else(|| mlua::Error::runtime(format!("unknown widget type {}", elem.tag)))?;
    let table = new_object(lua, object_type, elem.attr("name"), parent, elem, layer)?;
    let id = frame_id(&table)?;
    if let Some(def) = intrinsic {
        apply_element(lua, build, &def, &table, id, true)?;
    }
    apply_element(lua, build, elem, &table, id, false)?;
    build.onloads.push(id);
    Ok(table)
}

/// Create the object and set its parent key, before templates apply.
fn new_object(
    lua: &Lua,
    object_type: &'static str,
    name: Option<&str>,
    parent: Option<FrameId>,
    elem: &Elem,
    layer: Option<(u8, i64)>,
) -> Result<Table> {
    let table = create_object(lua, object_type, name.map(str::to_string), parent)?;
    let id = frame_id(&table)?;
    state(lua).frames[id].from_xml = true;
    if let Some((l, sub)) = layer {
        let mut st = state(lua);
        st.frames[id].draw_layer = l;
        st.frames[id].sublevel = sub;
    }
    set_parent_keys(lua, &table, parent, elem)?;
    Ok(table)
}

fn set_parent_keys(lua: &Lua, table: &Table, parent: Option<FrameId>, elem: &Elem) -> Result<()> {
    let Some(p) = parent else { return Ok(()) };
    let parent_table = state(lua).table(p);
    if let Some(key) = elem.attr("parentKey") {
        parent_table.set(key, table.clone())?;
    }
    if let Some(key) = elem.attr("parentArray") {
        let arr = match parent_table.get::<Value>(key)? {
            Value::Table(t) => t,
            _ => {
                let t = lua.create_table()?;
                parent_table.set(key, t.clone())?;
                t
            }
        };
        arr.raw_push(table.clone())?;
    }
    Ok(())
}

/// Call a native method of the object's widget type, ignoring mixins.
fn native(lua: &Lua, table: &Table, method: &str, args: impl IntoLuaMulti) -> Result<MultiValue> {
    let f: Value = match table.metatable() {
        Some(mt) => match mt.raw_get::<Value>("__index")? {
            Value::Table(index) => index.raw_get(method)?,
            _ => Value::Nil,
        },
        None => Value::Nil,
    };
    match f {
        Value::Function(f) => {
            let mut args = args.into_lua_multi(lua)?;
            args.push_front(Value::Table(table.clone()));
            f.call(args)
        }
        _ => Ok(MultiValue::new()),
    }
}

/// Apply templates named in `inherits` (comma separated) to an object.
fn apply_templates(
    lua: &Lua,
    build: &mut Build,
    names: &str,
    table: &Table,
    id: FrameId,
) -> Result<()> {
    for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        let template = state(lua)
            .xml
            .templates
            .get(&name.to_ascii_lowercase())
            .cloned();
        match template {
            Some(t) => {
                apply_element(lua, build, &t, table, id, false)?;
                // A template may carry its own parentKey or parentArray.
                let parent = state(lua).frames[id].parent;
                set_parent_keys(lua, table, parent, &t)?;
            }
            None => {
                // A font string may inherit a font object.
                let is_font_user = {
                    let st = state(lua);
                    st.frames[id].font.is_some()
                };
                let font = match lua.globals().get::<Value>(name)? {
                    Value::Table(t) if frame_id(&t).is_ok() => Some(t),
                    _ => None,
                };
                match font {
                    Some(f) if is_font_user => {
                        native(lua, table, "SetFontObject", f)?;
                    }
                    _ => warn_once(lua, &format!("template:{name}"), || {
                        format!("unknown template '{name}'")
                    }),
                }
            }
        }
    }
    Ok(())
}

fn apply_mixins(lua: &Lua, table: &Table, names: &str) -> Result<()> {
    let mixin: Function = lua.globals().get("Mixin")?;
    for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        match lua.globals().get::<Value>(name)? {
            Value::Table(m) => {
                mixin.call::<()>((table.clone(), m))?;
            }
            _ => warn_once(lua, &format!("mixin:{name}"), || {
                format!("unknown mixin '{name}'")
            }),
        }
    }
    Ok(())
}

/// Apply one element (a template or the object's own element).
fn apply_element(
    lua: &Lua,
    build: &mut Build,
    elem: &Rc<Elem>,
    table: &Table,
    id: FrameId,
    intrinsic: bool,
) -> Result<()> {
    if let Some(inherits) = elem.attr("inherits") {
        apply_templates(lua, build, inherits, table, id)?;
    }
    if let Some(m) = elem.attr("mixin") {
        apply_mixins(lua, table, m)?;
    }
    if let Some(m) = elem.attr("secureMixin") {
        apply_mixins(lua, table, m)?;
    }
    apply_attributes(lua, build, elem, table, id)?;
    for child in &elem.children {
        apply_child(lua, build, elem, child, table, id, intrinsic)?;
    }
    Ok(())
}

fn global_string(lua: &Lua, key: &str) -> String {
    match lua.globals().get::<Value>(key) {
        Ok(Value::String(s)) => s.to_string_lossy(),
        _ => key.to_string(),
    }
}

fn apply_attributes(
    lua: &Lua,
    build: &mut Build,
    e: &Rc<Elem>,
    table: &Table,
    id: FrameId,
) -> Result<()> {
    let object_type = state(lua).frames[id].object_type;
    let is_texture = matches!(object_type, "Texture" | "MaskTexture" | "Line");
    for (key, value) in &e.attrs {
        let b = value.eq_ignore_ascii_case("true");
        let f = value.trim().parse::<f64>().unwrap_or(0.0);
        match key.as_str() {
            "hidden" => state(lua).frames[id].shown = !b,
            "alpha" => state(lua).frames[id].alpha = f.clamp(0.0, 1.0),
            "scale" if f > 0.0 => state(lua).frames[id].scale = f,
            "id" => state(lua).frames[id].id = f as i64,
            "setAllPoints" if b => build.anchors.push((id, e.clone())),
            "frameStrata" => {
                if let Some(s) = strata_name(value) {
                    native(lua, table, "SetFrameStrata", s)?;
                }
            }
            "frameLevel" => {
                native(lua, table, "SetFrameLevel", f as i64)?;
            }
            "toplevel" => state(lua).frames[id].toplevel = b,
            "enableMouse" => {
                let mut st = state(lua);
                st.frames[id].mouse_enabled = b;
                st.frames[id].mouse_motion = b;
            }
            "enableMouseClicks" => state(lua).frames[id].mouse_enabled = b,
            "enableMouseMotion" => state(lua).frames[id].mouse_motion = b,
            "movable" => state(lua).frames[id].movable = b,
            "resizable" => state(lua).frames[id].resizable = b,
            "hyperlinksEnabled" => state(lua).frames[id].hyperlinks_enabled = b,
            "clampedToScreen" => state(lua).frames[id].clamped = b,
            "dontSavePosition" => state(lua).frames[id].dont_save_position = b,
            "enableKeyboard" => state(lua).frames[id].keyboard_enabled = b,
            "propagateKeyboardInput" => state(lua).frames[id].propagate_keyboard = b,
            "autoFocus" | "multiLine" | "numeric" | "letters" | "bytes" | "historyLines"
                if state(lua).frames[id].edit.is_some() =>
            {
                let mut st = state(lua);
                let e = st.frames[id].edit.as_deref_mut().expect("edit state");
                match key.as_str() {
                    "autoFocus" => e.auto_focus = b,
                    "multiLine" => e.multi_line = b,
                    "numeric" => e.numeric = b,
                    "letters" => e.max_letters = f.max(0.0) as usize,
                    "bytes" => e.max_bytes = f.max(0.0) as usize,
                    _ => e.history_lines = f.max(0.0) as usize,
                }
            }
            "reverse"
            | "drawSwipe"
            | "drawEdge"
            | "drawBling"
            | "hideCountdownNumbers"
            | "rotation"
                if state(lua).frames[id].cooldown.is_some() =>
            {
                let mut st = state(lua);
                let cd = st.frames[id].cooldown.as_deref_mut().expect("cooldown");
                match key.as_str() {
                    "reverse" => cd.reverse = b,
                    "drawSwipe" => cd.draw_swipe = b,
                    "drawEdge" => cd.draw_edge = b,
                    "drawBling" => cd.draw_bling = b,
                    "hideCountdownNumbers" => cd.hide_numbers = b,
                    _ => cd.rotation = f,
                }
            }
            "fade" | "fadeDuration" | "fadePower" | "displayDuration" | "insertMode"
                if state(lua).frames[id].messages.is_some() =>
            {
                let mut st = state(lua);
                let m = st.frames[id].messages.as_deref_mut().expect("messages");
                match key.as_str() {
                    "fade" => m.fading = b,
                    "fadeDuration" => m.fade_duration = f,
                    "fadePower" => m.fade_power = f,
                    "displayDuration" => m.time_visible = f,
                    _ => m.insert_mode = value.to_ascii_uppercase(),
                }
            }
            "registerForDrag" => {
                state(lua).frames[id].drag_buttons =
                    value.split(',').map(|s| s.trim().to_string()).collect();
            }
            "clipChildren" => state(lua).frames[id].clips_children = b,
            "ignoreParentAlpha" => state(lua).frames[id].ignore_parent_alpha = b,
            "ignoreParentScale" => state(lua).frames[id].ignore_parent_scale = b,
            "useParentLevel" if b => {
                let mut st = state(lua);
                if let Some(p) = st.frames[id].parent {
                    st.frames[id].level = st.frames[p].level;
                }
            }
            "text" => {
                let s = global_string(lua, value);
                native(lua, table, "SetText", s)?;
            }
            "file" if is_texture => {
                native(lua, table, "SetTexture", value.as_str())?;
            }
            "atlas" if is_texture => {
                let use_size = e.attr_bool("useAtlasSize").unwrap_or(false);
                crate::widgets::set_atlas(lua, id, value, use_size);
            }
            "alphaMode" => {
                native(lua, table, "SetBlendMode", value.as_str())?;
            }
            "desaturated" => {
                native(lua, table, "SetDesaturated", b)?;
            }
            "horizTile" => {
                native(lua, table, "SetHorizTile", b)?;
            }
            "vertTile" => {
                native(lua, table, "SetVertTile", b)?;
            }
            "rotation" => {
                native(lua, table, "SetRotation", f.to_radians())?;
            }
            "font" => {
                let height = e.attr_f64("height").unwrap_or(0.0);
                let mut st = state(lua);
                let font = st.frames[id].font.get_or_insert_with(Box::default);
                font.path = Some(value.clone());
                if height > 0.0 {
                    font.height = Some(height);
                }
            }
            "outline" => {
                let flags = match value.to_ascii_uppercase().as_str() {
                    "NORMAL" => "OUTLINE",
                    "THICK" => "THICKOUTLINE",
                    _ => "",
                };
                let mut st = state(lua);
                st.frames[id].font.get_or_insert_with(Box::default).flags = Some(flags.to_string());
            }
            "justifyH" => {
                native(lua, table, "SetJustifyH", value.as_str())?;
            }
            "justifyV" => {
                native(lua, table, "SetJustifyV", value.as_str())?;
            }
            "spacing" => {
                native(lua, table, "SetSpacing", f)?;
            }
            "maxLines" => {
                native(lua, table, "SetMaxLines", f as i64)?;
            }
            "wordwrap" => {
                native(lua, table, "SetWordWrap", b)?;
            }
            "nonspacewrap" => {
                native(lua, table, "SetNonSpaceWrap", b)?;
            }
            "registerForClicks" => {
                let clicks: Vec<String> = value.split(',').map(|s| s.trim().to_string()).collect();
                native(
                    lua,
                    table,
                    "RegisterForClicks",
                    MultiValue::from_iter(
                        clicks
                            .into_iter()
                            .map(|c| Value::String(lua.create_string(c).unwrap())),
                    ),
                )?;
            }
            "motionScriptsWhileDisabled" => {
                native(lua, table, "SetMotionScriptsWhileDisabled", b)?;
            }
            "minValue" | "maxValue" => {
                let mut st = state(lua);
                if key == "minValue" {
                    st.frames[id].value.0 = f;
                } else {
                    st.frames[id].value.1 = f;
                }
            }
            "defaultValue" => state(lua).frames[id].value.2 = f,
            "valueStep" => state(lua).frames[id].value_step = f.max(0.0),
            "obeyStepOnDrag" => state(lua).frames[id].obey_step_on_drag = b,
            "stepsPerPage" => state(lua).frames[id].steps_per_page = f,
            "orientation" => state(lua).frames[id].orientation = value.to_ascii_uppercase(),
            "looping" => {
                native(lua, table, "SetLooping", value.as_str())?;
            }
            "setToFinalAlpha" => {
                native(lua, table, "SetToFinalAlpha", b)?;
            }
            "duration" => {
                native(lua, table, "SetDuration", f)?;
            }
            "startDelay" => {
                native(lua, table, "SetStartDelay", f)?;
            }
            "endDelay" => {
                native(lua, table, "SetEndDelay", f)?;
            }
            "order" => {
                native(lua, table, "SetOrder", f as i64)?;
            }
            "fromAlpha" => {
                native(lua, table, "SetFromAlpha", f)?;
            }
            "toAlpha" => {
                native(lua, table, "SetToAlpha", f)?;
            }
            "offsetX" | "offsetY" | "scaleX" | "scaleY" | "fromScaleX" | "fromScaleY"
            | "toScaleX" | "toScaleY"
                if state(lua).frames[id].anim.is_some() =>
            {
                let mut st = state(lua);
                if let Some(a) = st.frames[id].anim.as_deref_mut() {
                    match key.as_str() {
                        "offsetX" => a.offset.0 = f,
                        "offsetY" => a.offset.1 = f,
                        "scaleX" | "toScaleX" => a.scale_to.0 = f,
                        "scaleY" | "toScaleY" => a.scale_to.1 = f,
                        "fromScaleX" => a.scale_from.0 = f,
                        _ => a.scale_from.1 = f,
                    }
                }
            }
            "degrees" | "radians" if state(lua).frames[id].anim.is_some() => {
                let r = if key == "degrees" { f.to_radians() } else { f };
                native(lua, table, "SetRadians", r)?;
            }
            "flipBookRows" => {
                native(lua, table, "SetFlipBookRows", f.max(1.0) as u32)?;
            }
            "flipBookColumns" => {
                native(lua, table, "SetFlipBookColumns", f.max(1.0) as u32)?;
            }
            "flipBookFrames" => {
                native(lua, table, "SetFlipBookFrames", f.max(1.0) as u32)?;
            }
            "smoothing" if state(lua).frames[id].anim.is_some() => {
                native(lua, table, "SetSmoothing", value.as_str())?;
            }
            "childKey" => {
                native(lua, table, "SetChildKey", value.as_str())?;
            }
            "targetKey" => {
                native(lua, table, "SetTargetKey", value.as_str())?;
            }
            "target" => {
                if let Ok(Value::Table(t)) = lua.globals().get::<Value>(value.as_str()) {
                    native(lua, table, "SetTarget", t)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Read a size from `x`/`y` attributes or an `AbsDimension` child.
fn dimension(e: &Elem) -> (Option<f64>, Option<f64>) {
    let src = e.child("AbsDimension").map(|r| r.as_ref()).unwrap_or(e);
    (src.attr_f64("x"), src.attr_f64("y"))
}

fn color(e: &Elem) -> [f32; 4] {
    if let Some(c) = e.attr("color") {
        // A named colour; colour tables need DB2 data, use white.
        let _ = c;
        return [1.0; 4];
    }
    let g = |k: &str, d: f64| e.attr_f64(k).unwrap_or(d) as f32;
    [g("r", 0.0), g("g", 0.0), g("b", 0.0), g("a", 1.0)]
}

const TEXTURE_SLOTS: &[(&str, &str)] = &[
    ("NormalTexture", "Normal"),
    ("PushedTexture", "Pushed"),
    ("DisabledTexture", "Disabled"),
    ("HighlightTexture", "Highlight"),
    ("CheckedTexture", "Checked"),
    ("DisabledCheckedTexture", "DisabledChecked"),
];

fn apply_child(
    lua: &Lua,
    build: &mut Build,
    owner: &Rc<Elem>,
    child: &Rc<Elem>,
    table: &Table,
    id: FrameId,
    intrinsic: bool,
) -> Result<()> {
    let tag = child.tag.as_str();
    match tag {
        "Size" => {
            let (x, y) = dimension(child);
            let mut st = state(lua);
            if let Some(x) = x {
                st.frames[id].width = x;
            }
            if let Some(y) = y {
                st.frames[id].height = y;
            }
            crate::layout::invalidate(&mut st);
        }
        "Anchors" => build.anchors.push((id, child.clone())),
        "MaskedTextures" => build.masked.push((id, child.clone())),
        // <Mixins><Mixin key="Name" source="secure"/></Mixins>: like the
        // mixin attribute. The secure/public partitions do not exist here.
        "Mixins" => {
            for m in &child.children {
                if let Some(key) = m.attr("key") {
                    apply_mixins(lua, table, key)?;
                }
            }
        }
        "KeyValues" => {
            for kv in &child.children {
                if let Err(e) = apply_key_value(lua, table, kv) {
                    report(lua, kv, &e.to_string());
                }
            }
        }
        "Layers" => {
            for layer in &child.children {
                let level = layer.attr("level").and_then(layer_index).unwrap_or(2);
                let sub = layer.attr_f64("textureSubLevel").unwrap_or(0.0) as i64;
                for region in &layer.children {
                    if let Err(e) = build_object(lua, build, region, Some(id), Some((level, sub))) {
                        report(lua, region, &e.to_string());
                    }
                }
            }
        }
        "Frames" => {
            for f in &child.children {
                if let Err(e) = build_object(lua, build, f, Some(id), None) {
                    report(lua, f, &e.to_string());
                }
            }
        }
        "Animations" => {
            for g in &child.children {
                if let Err(e) = build_object(lua, build, g, Some(id), None) {
                    report(lua, g, &e.to_string());
                }
            }
        }
        "Scripts" => {
            for s in &child.children {
                if let Err(e) = apply_script(lua, build, s, table, id, intrinsic) {
                    report(lua, s, &e.to_string());
                }
            }
        }
        "Attributes" => {
            for a in &child.children {
                let Some(name) = a.attr("name") else { continue };
                let v = typed_value(lua, a.attr("type"), a.attr("value").unwrap_or(""))?;
                // During construction attributes do not fire OnAttributeChanged
                // (the frame's children may not exist yet).
                native(lua, table, "SetAttributeNoHandler", (name, v))?;
            }
        }
        "HitRectInsets" => {
            let src = child.child("AbsInset").map(|r| r.as_ref()).unwrap_or(child);
            let g = |k: &str| src.attr_f64(k).unwrap_or(0.0);
            state(lua).frames[id].hit_insets = [g("left"), g("right"), g("top"), g("bottom")];
        }
        "TexCoords" => {
            if let Some(r) = child.child("Rect") {
                let g = |k: &str| r.attr_f64(k).unwrap_or(0.0);
                native(
                    lua,
                    table,
                    "SetTexCoord",
                    (
                        g("ULx"),
                        g("ULy"),
                        g("LLx"),
                        g("LLy"),
                        g("URx"),
                        g("URy"),
                        g("LRx"),
                        g("LRy"),
                    ),
                )?;
            } else {
                let g = |k: &str, d: f64| child.attr_f64(k).unwrap_or(d);
                native(
                    lua,
                    table,
                    "SetTexCoord",
                    (
                        g("left", 0.0),
                        g("right", 1.0),
                        g("top", 0.0),
                        g("bottom", 1.0),
                    ),
                )?;
            }
        }
        "Color" => {
            let c = color(child);
            let kind = state(lua).frames[id].object_type;
            match kind {
                "Texture" | "MaskTexture" | "Line" => {
                    native(lua, table, "SetColorTexture", (c[0], c[1], c[2], c[3]))?;
                }
                _ => {
                    let mut st = state(lua);
                    st.frames[id].font.get_or_insert_with(Box::default).color = Some(c);
                }
            }
        }
        "Gradient" => {
            let orientation = child.attr("orientation").unwrap_or("HORIZONTAL");
            let min = child
                .child("MinColor")
                .map(|c| color(c))
                .unwrap_or([1.0; 4]);
            let max = child
                .child("MaxColor")
                .map(|c| color(c))
                .unwrap_or([1.0; 4]);
            let mut st = state(lua);
            crate::frames::ensure_tex(&mut st.frames[id]).gradient =
                Some((min, max, orientation.eq_ignore_ascii_case("HORIZONTAL")));
        }
        "Shadow" => {
            let (x, y) = child
                .child("Offset")
                .map(|o| dimension(o))
                .unwrap_or((child.attr_f64("x"), child.attr_f64("y")));
            let c = child
                .child("Color")
                .map(|c| color(c))
                .unwrap_or([0.0, 0.0, 0.0, 1.0]);
            let mut st = state(lua);
            let font = st.frames[id].font.get_or_insert_with(Box::default);
            font.shadow_offset = Some((x.unwrap_or(0.0), y.unwrap_or(0.0)));
            font.shadow_color = Some(c);
        }
        "FontHeight" => {
            let v = child
                .child("AbsValue")
                .and_then(|a| a.attr_f64("val"))
                .or_else(|| child.attr_f64("val"));
            if let Some(h) = v {
                let mut st = state(lua);
                st.frames[id].font.get_or_insert_with(Box::default).height = Some(h);
            }
        }
        "ButtonText" => {
            let existing = state(lua).frames[id]
                .button
                .as_deref()
                .and_then(|b| b.font_string);
            let fs = match existing {
                Some(fs) => {
                    let t = state(lua).table(fs);
                    apply_element(lua, build, child, &t, fs, false)?;
                    set_parent_keys(lua, &t, Some(id), child)?;
                    t
                }
                None => {
                    let t = new_object(
                        lua,
                        "FontString",
                        child.attr("name"),
                        Some(id),
                        child,
                        Some((3, 0)),
                    )?;
                    let fs = frame_id(&t)?;
                    apply_element(lua, build, child, &t, fs, false)?;
                    t
                }
            };
            native(lua, table, "SetFontString", fs)?;
        }
        "NormalFont" | "HighlightFont" | "DisabledFont" => {
            if let Some(style) = child.attr("style") {
                let method = format!("Set{tag}Object");
                if let Ok(Value::Table(f)) = lua.globals().get::<Value>(style) {
                    native(lua, table, &method, f)?;
                }
            }
        }
        "PushedTextOffset" => {
            let (x, y) = dimension(child);
            native(
                lua,
                table,
                "SetPushedTextOffset",
                (x.unwrap_or(0.0), y.unwrap_or(0.0)),
            )?;
        }
        "ThumbTexture" | "BarTexture" => {
            // UI.xsd: the thumb is drawn in OVERLAY unless the slider's
            // drawLayer says otherwise.
            let layer = if tag == "ThumbTexture" { 3 } else { 2 };
            let tex = build_region_for(lua, build, child, id, (layer, 0))?;
            let method = if tag == "ThumbTexture" {
                "SetThumbTexture"
            } else {
                "SetStatusBarTexture"
            };
            native(lua, table, method, tex)?;
        }
        "BarColor" => {
            let c = color(child);
            native(lua, table, "SetStatusBarColor", (c[0], c[1], c[2], c[3]))?;
        }
        "ScrollChild" => {
            for f in &child.children {
                let c = build_object(lua, build, f, Some(id), None)?;
                native(lua, table, "SetScrollChild", c)?;
            }
        }
        "FontString" if state(lua).frames[id].object_type == "EditBox" => {
            if let Some(inh) = child.attr("inherits") {
                apply_templates(lua, build, inh, table, id)?;
            }
            apply_attributes(lua, build, child, table, id)?;
        }
        t if TEXTURE_SLOTS.iter().any(|(n, _)| *n == t) => {
            let slot = TEXTURE_SLOTS
                .iter()
                .find(|(n, _)| *n == t)
                .map(|(_, s)| *s)
                .unwrap();
            let existing = crate::widgets::button_slot_texture(lua, id, slot)?;
            let tex_table = state(lua).table(existing);
            if child.children.iter().any(|c| c.tag == "Anchors") {
                // Explicit anchors replace the default full-size anchors.
                state(lua).frames[existing].points.clear();
            }
            apply_element(lua, build, child, &tex_table, existing, false)?;
            set_parent_keys(lua, &tex_table, Some(id), child)?;
        }
        t if frames::is_animation_type(t)
            && state(lua).frames[id].object_type == "AnimationGroup" =>
        {
            build_object(lua, build, child, Some(id), None)?;
        }
        "SwipeTexture" if state(lua).frames[id].cooldown.is_some() => {
            let file = child.attr("file").map(|f| TexFile::Path(f.to_string()));
            let c = child.child("Color").map(|c| color(c.as_ref()));
            let mut st = state(lua);
            let cd = st.frames[id].cooldown.as_deref_mut().expect("cooldown");
            if let Some(f) = file {
                cd.swipe_texture = f;
            }
            if let Some(c) = c {
                cd.swipe_color = c;
            }
        }
        "Offset" | "Rect" | "AbsDimension" | "ResizeBounds" | "TitleRegion" | "Backdrop"
        | "SwipeTexture" | "EdgeTexture" | "BlingTexture" | "FlipBook" | "KeyValue" => {}
        "TextInsets" => {
            let src = child.child("AbsInset").map(|r| r.as_ref()).unwrap_or(child);
            let g = |k: &str| src.attr_f64(k).unwrap_or(0.0);
            native(
                lua,
                table,
                "SetTextInsets",
                (g("left"), g("right"), g("top"), g("bottom")),
            )?;
        }
        "ViewInsets" => {
            let src = child.child("AbsInset").map(|r| r.as_ref()).unwrap_or(child);
            let g = |k: &str| src.attr_f64(k).unwrap_or(0.0);
            native(
                lua,
                table,
                "SetViewInsets",
                (g("left"), g("right"), g("top"), g("bottom")),
            )?;
        }
        "Origin" => {
            let point = child.attr("point").unwrap_or("CENTER").to_ascii_uppercase();
            let (x, y) = child
                .child("Offset")
                .map(|o| dimension(o))
                .unwrap_or((None, None));
            let mut st = state(lua);
            if let Some(a) = st.frames[id].anim.as_deref_mut() {
                a.origin = Some((point, x.unwrap_or(0.0), y.unwrap_or(0.0)));
            }
        }
        _ => warn_once(lua, &format!("child:{}:{tag}", owner.tag), || {
            format!(
                "{}:{}: unsupported element <{tag}> in <{}>",
                child.file, child.line, owner.tag
            )
        }),
    }
    Ok(())
}

/// Build a texture for a slot element such as `<BarTexture>`.
fn build_region_for(
    lua: &Lua,
    build: &mut Build,
    elem: &Rc<Elem>,
    parent: FrameId,
    layer: (u8, i64),
) -> Result<Table> {
    let t = new_object(
        lua,
        "Texture",
        elem.attr("name"),
        Some(parent),
        elem,
        Some(layer),
    )?;
    let id = frame_id(&t)?;
    apply_element(lua, build, elem, &t, id, false)?;
    Ok(t)
}

fn typed_value(lua: &Lua, kind: Option<&str>, value: &str) -> Result<Value> {
    Ok(match kind.map(|k| k.to_ascii_lowercase()).as_deref() {
        Some("number") => Value::Number(value.trim().parse().unwrap_or(0.0)),
        Some("boolean") => Value::Boolean(value.eq_ignore_ascii_case("true") || value == "1"),
        Some("nil") => Value::Nil,
        Some("global") => {
            // Dotted paths such as SOUNDKIT.UI_CLASS_TALENT_TAB.
            let mut v = Value::Table(lua.globals());
            for part in value.split('.') {
                v = match v {
                    Value::Table(t) => t.get(part)?,
                    _ => Value::Nil,
                };
            }
            v
        }
        _ => Value::String(lua.create_string(value)?),
    })
}

fn apply_key_value(lua: &Lua, table: &Table, kv: &Elem) -> Result<()> {
    let Some(key) = kv.attr("key") else {
        return Ok(());
    };
    let value = typed_value(lua, kv.attr("type"), kv.attr("value").unwrap_or(""))?;
    let key = match kv.attr("keyType") {
        Some(k) if k.eq_ignore_ascii_case("number") => {
            Value::Number(key.trim().parse().unwrap_or(0.0))
        }
        Some(k) if k.eq_ignore_ascii_case("global") => typed_value(lua, Some("global"), key)?,
        _ => Value::String(lua.create_string(key)?),
    };
    table.set(key, value)
}

/// Parameter names of script handlers, for inline script bodies.
fn script_params(name: &str) -> &'static str {
    match name {
        "OnEvent" => "self, event, ...",
        "OnUpdate" => "self, elapsed",
        "OnClick" | "PreClick" | "PostClick" => "self, button, down",
        "OnDoubleClick" => "self, button",
        "OnMouseDown" | "OnMouseUp" | "OnDragStart" | "OnDragStop" | "OnReceiveDrag" => {
            "self, button"
        }
        "OnMouseWheel" => "self, delta",
        "OnEnter" | "OnLeave" => "self, motion",
        "OnValueChanged" => "self, value, userInput",
        "OnTextChanged" => "self, userInput",
        "OnSizeChanged" => "self, width, height",
        "OnAttributeChanged" => "self, name, value",
        "OnChar" => "self, text",
        "OnKeyDown" | "OnKeyUp" => "self, key",
        "OnHyperlinkClick" | "OnHyperlinkEnter" | "OnHyperlinkLeave" => "self, link, text, button",
        "OnVerticalScroll" | "OnHorizontalScroll" => "self, offset",
        "OnScrollRangeChanged" => "self, xrange, yrange",
        "OnFinished" | "OnStop" => "self, requested",
        "OnLoop" => "self, loopState",
        _ => "self, ...",
    }
}

const HELPERS: &str = r#"
local error, format = error, string.format
local function method(name)
    return function(self, ...)
        local f = self[name]
        if f == nil then
            local who = self.GetDebugName and self:GetDebugName() or "?"
            error(format("%s: attempt to call method '%s' (a nil value)", who, name), 2)
        end
        return f(self, ...)
    end
end
local function combine(a, b)
    return function(...) a(...); return b(...) end
end
return method, combine
"#;

fn helpers(lua: &Lua) -> Result<(Function, Function)> {
    const KEY: &str = "wowprogramming.xml_helpers";
    if let Ok(t) = lua.named_registry_value::<Table>(KEY) {
        return Ok((t.get(1)?, t.get(2)?));
    }
    let (m, c): (Function, Function) = lua.load(HELPERS).set_name("=[C]").call(())?;
    let t = lua.create_sequence_from([m.clone(), c.clone()])?;
    lua.set_named_registry_value(KEY, t)?;
    Ok((m, c))
}

fn apply_script(
    lua: &Lua,
    build: &mut Build,
    s: &Elem,
    table: &Table,
    id: FrameId,
    intrinsic: bool,
) -> Result<()> {
    let name = s.tag.clone();
    let (method_helper, combine) = helpers(lua)?;
    let handler: Option<Function> = if let Some(f) = s.attr("function") {
        match lua.globals().get::<Value>(f)? {
            Value::Function(f) => Some(f),
            _ => {
                warn_once(lua, &format!("function:{f}"), || {
                    format!(
                        "{}:{}: script function '{f}' does not exist",
                        s.file, s.line
                    )
                });
                None
            }
        }
    } else if let Some(m) = s.attr("method") {
        Some(method_helper.call(m)?)
    } else if !s.text.trim().is_empty() {
        let code = pad_lines(
            s.line,
            &format!("return function({}) {} end", script_params(&name), s.text),
        );
        let f: Function = lua.load(code).set_name(format!("@{}", s.file)).call(())?;
        Some(f)
    } else {
        None
    };
    let Some(handler) = handler else {
        return Ok(());
    };

    let existing: Option<Function> = native(lua, table, "GetScript", name.as_str())?
        .into_iter()
        .next()
        .and_then(|v| match v {
            Value::Function(f) => Some(f),
            _ => None,
        });
    let inherit = s.attr("inherit").map(|v| v.to_ascii_lowercase());
    let combined = match (inherit.as_deref(), existing) {
        // inherit="prepend" puts the inherited handler first; "append" puts
        // it after this one. (StanceBar's OnLoad prepends EditModeActionBar's,
        // which creates the buttons it then uses.)
        (Some("prepend"), Some(old)) => combine.call((old, handler.clone()))?,
        (Some("append"), Some(old)) => combine.call((handler.clone(), old))?,
        _ => {
            let key = (id, name.clone());
            match build.intrinsic_scripts.get(&key).cloned() {
                Some((f, postcall)) if !intrinsic => {
                    if postcall {
                        combine.call((handler.clone(), f))?
                    } else {
                        combine.call((f, handler.clone()))?
                    }
                }
                _ => handler.clone(),
            }
        }
    };
    if intrinsic {
        let postcall = s
            .attr("intrinsicOrder")
            .is_some_and(|o| o.eq_ignore_ascii_case("postcall"));
        build
            .intrinsic_scripts
            .insert((id, name.clone()), (handler, postcall));
    }
    native(lua, table, "SetScript", (name, combined))?;
    Ok(())
}

/// Resolve `relativeKey` paths such as `$parent.$parent.Foo`.
fn relative_key(lua: &Lua, id: FrameId, path: &str) -> Option<Table> {
    let mut current: Table = state(lua).table(id);
    for part in path.split('.') {
        if part.eq_ignore_ascii_case("$parent") {
            let cid = frame_id(&current).ok()?;
            let p = state(lua).frames[cid].parent?;
            current = state(lua).table(p);
        } else {
            current = match current.get::<Value>(part).ok()? {
                Value::Table(t) => t,
                _ => return None,
            };
        }
    }
    Some(current)
}

/// Add a mask texture to the textures listed by `childKey`, which are
/// keys on the mask's parent.
fn apply_masked_textures(lua: &Lua, mask: FrameId, list: &Rc<Elem>) {
    let mask_table = state(lua).table(mask);
    for m in &list.children {
        let Some(key) = m.attr("childKey") else {
            continue;
        };
        match relative_key(lua, mask, &format!("$parent.{key}")) {
            Some(t) => {
                if let Err(e) = native(lua, &t, "AddMaskTexture", mask_table.clone()) {
                    report(lua, m, &e.to_string());
                }
            }
            None => report(lua, m, &format!("masked texture '{key}' not found")),
        }
    }
}

fn apply_anchors(lua: &Lua, id: FrameId, elem: &Rc<Elem>) {
    let table = state(lua).table(id);
    if elem.tag != "Anchors" {
        // setAllPoints="true"
        let _ = native(lua, &table, "SetAllPoints", ());
        return;
    }
    for a in &elem.children {
        if a.tag != "Anchor" {
            continue;
        }
        let Some(point) = a.attr("point") else {
            continue;
        };
        let parent = state(lua).frames[id].parent;
        let relative: Value = if let Some(key) = a.attr("relativeKey") {
            match relative_key(lua, id, key) {
                Some(t) => Value::Table(t),
                None => {
                    report(lua, a, &format!("relativeKey '{key}' not found"));
                    continue;
                }
            }
        } else if let Some(rel) = a.attr("relativeTo") {
            let name = frames::expand_parent_name(&state(lua).frames, rel, parent);
            if rel.eq_ignore_ascii_case("$parent") {
                match parent {
                    Some(p) => Value::Table(state(lua).table(p)),
                    None => Value::Nil,
                }
            } else {
                match lua.globals().get::<Value>(name.as_str()) {
                    Ok(Value::Table(t)) => Value::Table(t),
                    _ => {
                        report(lua, a, &format!("relativeTo '{name}' not found"));
                        continue;
                    }
                }
            }
        } else {
            Value::Nil
        };
        let (mut x, mut y) = (
            a.attr_f64("x").unwrap_or(0.0),
            a.attr_f64("y").unwrap_or(0.0),
        );
        if let Some(o) = a.child("Offset") {
            let (ox, oy) = dimension(o);
            x = ox.unwrap_or(x);
            y = oy.unwrap_or(y);
        }
        let rel_point = a.attr("relativePoint").unwrap_or(point);
        if let Err(e) = native(lua, &table, "SetPoint", (point, relative, rel_point, x, y)) {
            report(lua, a, &e.to_string());
        }
    }
}

fn create_font(lua: &Lua, e: &Rc<Elem>) -> Result<()> {
    let name = e.attr("name").map(str::to_string);
    let table = match name.as_deref().map(|n| lua.globals().get::<Value>(n)) {
        Some(Ok(Value::Table(t))) if frame_id(&t).is_ok() => t,
        _ => create_object(lua, "Font", name, None)?,
    };
    let id = frame_id(&table)?;
    if let Some(inh) = e.attr("inherits")
        && let Ok(Value::Table(src)) = lua.globals().get::<Value>(inh)
        && let Ok(src_id) = frame_id(&src)
    {
        state(lua).frames[id]
            .font
            .get_or_insert_with(Box::default)
            .font_object = Some(src_id);
    }
    let mut build = Build::default();
    apply_attributes(lua, &mut build, e, &table, id)?;
    if let Some(h) = e.attr_f64("height") {
        state(lua).frames[id]
            .font
            .get_or_insert_with(Box::default)
            .height = Some(h);
    }
    for c in &e.children {
        apply_child(lua, &mut build, e, c, &table, id, false)?;
    }
    Ok(())
}

/// `CreateFrame` with templates (and intrinsic types).
pub fn create_frame(
    lua: &Lua,
    frame_type: &str,
    name: Option<String>,
    parent: Option<FrameId>,
    templates: Option<&str>,
) -> Result<Table> {
    let (object_type, intrinsic) = match resolve_type(lua, frame_type) {
        Some((t, i)) if canonical_type(t).is_some() => (t, i),
        _ => {
            return Err(mlua::Error::runtime(format!(
                "CreateFrame: Unknown frame type '{frame_type}'"
            )));
        }
    };
    let table = create_object(lua, object_type, name, parent)?;
    let id = frame_id(&table)?;
    let mut build = Build::default();
    let result = (|| {
        if let Some(def) = intrinsic {
            apply_element(lua, &mut build, &def, &table, id, true)?;
        }
        if let Some(t) = templates {
            apply_templates(lua, &mut build, t, &table, id)?;
        }
        Ok(())
    })();
    build.onloads.push(id);
    finish(lua, build);
    result.map(|_| table)
}

/// Create a region, animation group or animation owned by `parent`, with
/// an optional template, as the `Create*` methods do.
pub fn create_child(
    lua: &Lua,
    kind: &str,
    name: Option<String>,
    parent: FrameId,
    template: Option<&str>,
    layer: Option<(u8, i64)>,
) -> Result<Table> {
    let object_type = canonical_any_type(kind)
        .ok_or_else(|| mlua::Error::runtime(format!("Unknown object type '{kind}'")))?;
    let table = create_object(lua, object_type, name, Some(parent))?;
    let id = frame_id(&table)?;
    if let Some((l, sub)) = layer {
        let mut st = state(lua);
        st.frames[id].draw_layer = l;
        st.frames[id].sublevel = sub;
    }
    let mut build = Build::default();
    let result = match template {
        Some(t) => apply_templates(lua, &mut build, t, &table, id),
        None => Ok(()),
    };
    // Layer from the call wins over the template's.
    if let Some((l, sub)) = layer {
        let mut st = state(lua);
        st.frames[id].draw_layer = l;
        st.frames[id].sublevel = sub;
    }
    build.onloads.push(id);
    finish(lua, build);
    result.map(|_| table)
}

/// Template details for `C_XMLUtil.GetTemplateInfo`.
pub struct TemplateDetails {
    pub kind: String,
    pub width: f64,
    pub height: f64,
    pub inherits: Option<String>,
    pub source: String,
    /// Key, key type, value type and raw value.
    pub key_values: Vec<(String, String, String, String)>,
}

pub fn template_details(lua: &Lua, name: &str) -> Option<TemplateDetails> {
    let t = state(lua)
        .xml
        .templates
        .get(&name.to_ascii_lowercase())
        .cloned()?;
    let kind = resolve_type(lua, &t.tag)
        .map(|(k, _)| k.to_string())
        .unwrap_or_else(|| t.tag.clone());
    let mut size = (None, None);
    let mut key_values: Vec<(String, String, String, String)> = Vec::new();
    collect_template(lua, &t, &mut size, &mut key_values, 0);
    Some(TemplateDetails {
        kind,
        width: size.0.unwrap_or(0.0),
        height: size.1.unwrap_or(0.0),
        inherits: t.attr("inherits").map(str::to_string),
        source: format!("{}:{}", t.file, t.line),
        key_values,
    })
}

/// Gather size and key values; the template's own values win over the
/// ones it inherits.
fn collect_template(
    lua: &Lua,
    t: &Rc<Elem>,
    size: &mut (Option<f64>, Option<f64>),
    kvs: &mut Vec<(String, String, String, String)>,
    depth: usize,
) {
    if depth > 16 {
        return;
    }
    if let Some(inherits) = t.attr("inherits") {
        for name in inherits.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            let parent = state(lua)
                .xml
                .templates
                .get(&name.to_ascii_lowercase())
                .cloned();
            if let Some(p) = parent {
                collect_template(lua, &p, size, kvs, depth + 1);
            }
        }
    }
    if let Some(s) = t.child("Size") {
        let (w, h) = dimension(s);
        if w.is_some() {
            size.0 = w;
        }
        if h.is_some() {
            size.1 = h;
        }
    }
    if let Some(list) = t.child("KeyValues") {
        for kv in &list.children {
            let Some(key) = kv.attr("key") else { continue };
            kvs.retain(|(k, _, _, _)| k != key);
            kvs.push((
                key.to_string(),
                kv.attr("keyType").unwrap_or("string").to_string(),
                kv.attr("type").unwrap_or("string").to_string(),
                kv.attr("value").unwrap_or("").to_string(),
            ));
        }
    }
}
