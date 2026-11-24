//! Widget objects: creation, hierarchy, scripts, events and visibility.
//!
//! As in the client, a widget is a Lua table whose index `[0]` holds a
//! userdata that identifies the native object. Each widget type has its
//! own metatable, so addons can add fields to widgets and hook type methods.
//! The native methods live in `widgets.rs`.

use std::collections::HashMap;

use mlua::{AnyUserData, Function, IntoLuaMulti, Lua, MultiValue, Result, Table, Value};
use wowprogramming_lua::mlua;

use crate::natives::safe_call;
use crate::state::{
    AnimState, ButtonState, FontAttrs, FrameData, FrameId, FrameRef, TexState, TextState, state,
};

/// Frame types and their parent type, for `IsObjectType`.
pub const FRAME_TYPES: &[(&str, &str)] = &[
    ("Frame", "Region"),
    ("Button", "Frame"),
    ("CheckButton", "Button"),
    ("ItemButton", "Button"),
    ("EditBox", "Frame"),
    ("ScrollFrame", "Frame"),
    ("Slider", "Frame"),
    ("StatusBar", "Frame"),
    ("Cooldown", "Frame"),
    ("GameTooltip", "Frame"),
    ("MessageFrame", "Frame"),
    ("SimpleHTML", "Frame"),
    ("ColorSelect", "Frame"),
    ("Model", "Frame"),
    ("PlayerModel", "Model"),
    ("DressUpModel", "PlayerModel"),
    ("CinematicModel", "PlayerModel"),
    ("TabardModel", "PlayerModel"),
    ("ModelScene", "Frame"),
    // Glue only: the character select 3D scene, and a model for effects.
    ("MapScene", "Frame"),
    ("ModelFFX", "Model"),
    ("MovieFrame", "Frame"),
    ("Browser", "Frame"),
    ("Minimap", "Frame"),
    ("OffScreenFrame", "Frame"),
    ("ArchaeologyDigSiteFrame", "Frame"),
    ("QuestPOIFrame", "Frame"),
    ("ScenarioPOIFrame", "Frame"),
    ("FogOfWarFrame", "Frame"),
    ("UnitPositionFrame", "Frame"),
];

/// Non-frame widget types.
pub const OTHER_TYPES: &[(&str, &str)] = &[
    ("Texture", "TextureBase"),
    ("MaskTexture", "TextureBase"),
    ("Line", "TextureBase"),
    ("TextureBase", "Region"),
    ("FontString", "Region"),
    ("Region", "ScriptRegion"),
    ("ScriptRegion", "ScriptObject"),
    ("ScriptObject", "Object"),
    ("Font", "FontInstance"),
    ("FontInstance", "Object"),
    ("AnimationGroup", "ScriptObject"),
    ("Animation", "ScriptObject"),
    ("Alpha", "Animation"),
    ("Scale", "Animation"),
    ("LineScale", "Animation"),
    ("Translation", "Animation"),
    ("LineTranslation", "Animation"),
    ("Rotation", "Animation"),
    ("Path", "Animation"),
    ("FlipBook", "Animation"),
    ("TextureCoordTranslation", "Animation"),
    ("VertexColor", "Animation"),
    ("Object", ""),
];

/// Method groups of each widget type, in lookup order (later groups do
/// not override earlier ones).
fn method_chain(object_type: &str) -> Vec<&'static str> {
    let mut chain = vec!["Object"];
    match object_type {
        "Texture" | "MaskTexture" => chain.extend(["ScriptRegion", "Region", "Texture"]),
        "Line" => chain.extend(["ScriptRegion", "Region", "Texture", "Line"]),
        "FontString" => chain.extend(["ScriptRegion", "Region", "FontInstance", "FontString"]),
        "Font" => chain.extend(["FontInstance", "Font"]),
        "AnimationGroup" => chain.extend(["AnimCommon", "AnimationGroup"]),
        t if is_animation_type(t) => chain.extend(["AnimCommon", "Animation"]),
        t => {
            chain.extend(["ScriptRegion", "Frame"]);
            match t {
                "Button" | "ItemButton" => chain.push("Button"),
                "CheckButton" => chain.extend(["Button", "CheckButton"]),
                "EditBox" => chain.extend(["FontInstance", "EditBox"]),
                "Slider" => chain.push("Slider"),
                "StatusBar" => chain.push("StatusBar"),
                "ScrollFrame" => chain.push("ScrollFrame"),
                "Cooldown" => chain.push("Cooldown"),
                "GameTooltip" => chain.push("GameTooltip"),
                "MessageFrame" => chain.extend(["FontInstance", "MessageFrame"]),
                "SimpleHTML" => chain.extend(["FontInstance", "SimpleHTML"]),
                "ColorSelect" => chain.push("ColorSelect"),
                "Model" => chain.push("Model"),
                "PlayerModel" | "DressUpModel" | "CinematicModel" | "TabardModel" => {
                    chain.extend(["Model", "PlayerModel"])
                }
                "ModelScene" => chain.push("ModelScene"),
                "MapScene" => chain.push("MapScene"),
                "ModelFFX" => chain.extend(["Model", "ModelFFX"]),
                "Minimap" => chain.push("Minimap"),
                _ => {}
            }
        }
    }
    chain
}

pub fn is_animation_type(t: &str) -> bool {
    OTHER_TYPES
        .iter()
        .any(|(name, parent)| *name == t && (*parent == "Animation" || t == "Animation"))
}

/// The canonical spelling of a creatable frame type.
pub fn canonical_type(name: &str) -> Option<&'static str> {
    FRAME_TYPES
        .iter()
        .find(|(t, _)| t.eq_ignore_ascii_case(name))
        .map(|(t, _)| *t)
}

/// The canonical spelling of any widget type.
pub fn canonical_any_type(name: &str) -> Option<&'static str> {
    FRAME_TYPES
        .iter()
        .chain(OTHER_TYPES)
        .find(|(t, _)| t.eq_ignore_ascii_case(name))
        .map(|(t, _)| *t)
}

fn parent_type(name: &str) -> Option<&'static str> {
    FRAME_TYPES
        .iter()
        .chain(OTHER_TYPES)
        .find(|(t, _)| *t == name)
        .map(|(_, p)| *p)
        .filter(|p| !p.is_empty())
}

pub fn is_object_type(object_type: &str, query: &str) -> bool {
    let mut current = Some(object_type);
    while let Some(t) = current {
        if t.eq_ignore_ascii_case(query) {
            return true;
        }
        current = parent_type(t);
    }
    false
}

/// Build the metatables of all widget types from the native method groups
/// and the generated stubs (methods documented but not implemented).
pub fn install(lua: &Lua, stubs: &HashMap<String, Table>) -> Result<()> {
    let groups = crate::widgets::method_groups(lua)?;
    let mut metatables = HashMap::new();
    for (type_name, _) in FRAME_TYPES.iter().chain(OTHER_TYPES) {
        let index = lua.create_table()?;
        let chain = method_chain(type_name);
        // Base groups first, so a derived type's methods replace them.
        for group in chain.iter() {
            if let Some(methods) = groups.get(group) {
                for pair in methods.pairs::<Value, Value>() {
                    let (k, v) = pair?;
                    index.raw_set(k, v)?;
                }
            }
        }
        for group in &chain {
            if let Some(methods) = stubs.get(*group) {
                for pair in methods.pairs::<Value, Value>() {
                    let (k, v) = pair?;
                    if index.raw_get::<Value>(k.clone())?.is_nil() {
                        index.raw_set(k, v)?;
                    }
                }
            }
        }
        let mt = lua.create_table()?;
        mt.raw_set("__index", index)?;
        metatables.insert(*type_name, mt);
    }
    state(lua).frame_metatables = metatables;

    let g = lua.globals();
    g.set(
        "CreateFrame",
        lua.create_function(
            |lua,
             (frame_type, name, parent, template, id): (
                String,
                Option<String>,
                Value,
                Option<String>,
                Option<i64>,
            )| {
                let parent = optional_frame(lua, &parent)?;
                let table =
                    crate::xml::create_frame(lua, &frame_type, name, parent, template.as_deref())?;
                if let Some(id) = id {
                    let fid = frame_id(&table)?;
                    state(lua).frames[fid].id = id;
                }
                Ok(table)
            },
        )?,
    )?;
    let create_frame_fn: Function = g.get("CreateFrame")?;
    g.set("CreateForbiddenFrame", create_frame_fn)?;
    g.set(
        "CreateFont",
        lua.create_function(|lua, name: String| {
            if let Value::Table(t) = lua.globals().get::<Value>(name.as_str())?
                && frame_id(&t).is_ok()
            {
                return Ok(t);
            }
            create_object(lua, "Font", Some(name), None)
        })?,
    )?;
    g.set(
        "GetFonts",
        lua.create_function(|lua, ()| {
            let st = state(lua);
            let names: Vec<String> = st
                .frames
                .iter()
                .filter(|f| f.object_type == "Font")
                .filter_map(|f| f.name.clone())
                .collect();
            Ok(names)
        })?,
    )?;
    Ok(())
}

/// Create a widget of any type and return its table. Regions and
/// animations need a parent; frames and fonts may have none.
pub fn create_object(
    lua: &Lua,
    object_type: &'static str,
    name: Option<String>,
    parent: Option<FrameId>,
) -> Result<Table> {
    let table = lua.create_table()?;
    let mut st = state(lua);
    let id = st.frames.len();
    table.raw_set(0, lua.create_any_userdata(FrameRef(id))?)?;
    table.set_metatable(st.frame_metatables.get(object_type).cloned())?;

    let name = name
        .filter(|n| !n.is_empty())
        .map(|n| expand_parent_name(&st.frames, &n, parent));
    st.objects.raw_set(id as i64 + 1, table.clone())?;
    let mut data = FrameData::new(object_type);
    data.name = name.clone();
    data.parent = parent;
    if let Some(p) = parent {
        data.level = st.frames[p].level + 1;
        data.strata = st.frames[p].strata;
    }
    match object_type {
        "Texture" | "MaskTexture" | "Line" => data.tex = Some(Box::default()),
        "FontString" => {
            data.font = Some(Box::default());
            data.text = Some(Box::default());
        }
        "Font" => data.font = Some(Box::default()),
        "EditBox" | "MessageFrame" | "SimpleHTML" => {
            data.font = Some(Box::<FontAttrs>::default());
            data.text = Some(Box::<TextState>::default());
        }
        "AnimationGroup" => {
            data.anim = Some(Box::new(AnimState {
                looping: "NONE".into(),
                ..Default::default()
            }))
        }
        t if is_animation_type(t) => {
            data.anim = Some(Box::new(AnimState {
                from_alpha: 1.0,
                to_alpha: 1.0,
                scale_from: (1.0, 1.0),
                scale_to: (1.0, 1.0),
                flipbook: (1, 1, 1),
                ..Default::default()
            }))
        }
        _ => {}
    }
    if is_object_type(object_type, "Button") {
        // Buttons take mouse input unless told otherwise.
        data.mouse_enabled = true;
        data.button = Some(Box::new(ButtonState {
            enabled: true,
            clicks: vec!["LeftButtonUp".into()],
            ..Default::default()
        }));
    }
    if object_type == "EditBox" {
        // Edit boxes take clicks without enableMouse: almost no Blizzard
        // edit box sets it (inferred).
        data.mouse_enabled = true;
    }
    if matches!(object_type, "Slider" | "StatusBar") {
        data.value = (0.0, 0.0, 0.0);
    }
    let is_region = data.is_region();
    let is_anim = data.anim.is_some();
    let is_font = object_type == "Font";
    st.frames.push(data);
    if let Some(p) = parent {
        if is_region {
            st.frames[p].regions.push(id);
        } else if is_anim {
            st.frames[p].anims.push(id);
        } else if !is_font {
            st.frames[p].children.push(id);
        }
    }
    crate::layout::invalidate(&mut st);
    drop(st);
    if let Some(name) = name {
        lua.globals().set(name, table.clone())?;
    }
    Ok(table)
}

/// Create a frame without templates.
pub fn create_frame(
    lua: &Lua,
    object_type: &'static str,
    name: Option<String>,
    parent: Option<FrameId>,
) -> Result<Table> {
    create_object(lua, object_type, name, parent)
}

pub fn expand_parent_name(frames: &[FrameData], name: &str, parent: Option<FrameId>) -> String {
    let lower = name.to_ascii_lowercase();
    match lower.find("$parent") {
        Some(pos) => {
            // The name comes from the nearest named ancestor.
            let mut p = parent;
            let mut parent_name = String::new();
            while let Some(i) = p {
                if let Some(n) = &frames[i].name {
                    parent_name = n.clone();
                    break;
                }
                p = frames[i].parent;
            }
            format!("{}{}{}", &name[..pos], parent_name, &name[pos + 7..])
        }
        None => name.to_string(),
    }
}

/// Get the widget id from a widget table.
pub fn frame_id(table: &Table) -> Result<FrameId> {
    let ud: AnyUserData = table.raw_get(0)?;
    Ok(ud.borrow::<FrameRef>()?.0)
}

/// Resolve a method's `self` argument.
pub fn this(value: &Value) -> Result<FrameId> {
    let id = match value {
        Value::Table(t) => frame_id(t).ok(),
        _ => None,
    };
    id.ok_or_else(|| {
        mlua::Error::runtime(
            "Attempt to find 'this' in non-table object (used '.' instead of ':' ?)",
        )
    })
}

/// Resolve an optional widget argument: a widget table, a global name or nil.
pub fn optional_frame(lua: &Lua, value: &Value) -> Result<Option<FrameId>> {
    match value {
        Value::Nil => Ok(None),
        Value::Table(t) => Ok(Some(frame_id(t)?)),
        Value::String(s) => {
            let v: Value = lua.globals().get(s.clone())?;
            match v {
                Value::Table(t) => Ok(Some(frame_id(&t)?)),
                _ => Err(mlua::Error::runtime(format!(
                    "Couldn't find frame named '{}'",
                    s.to_string_lossy()
                ))),
            }
        }
        other => Err(mlua::Error::runtime(format!(
            "bad argument (frame expected, got {})",
            other.type_name()
        ))),
    }
}

pub fn is_visible(frames: &[FrameData], id: FrameId) -> bool {
    let mut current = Some(id);
    while let Some(i) = current {
        if !frames[i].shown {
            return false;
        }
        current = frames[i].parent;
    }
    true
}

/// Frames in the subtree of `id` that are visible, in pre-order.
fn visible_subtree(frames: &[FrameData], id: FrameId) -> Vec<FrameId> {
    let mut out = Vec::new();
    if is_visible(frames, id) {
        let mut stack = vec![id];
        while let Some(i) = stack.pop() {
            out.push(i);
            for &c in frames[i].children.iter().rev() {
                if frames[c].shown {
                    stack.push(c);
                }
            }
        }
    }
    out
}

/// Apply a change that may alter visibility, then fire `OnShow` for frames
/// that became visible and `OnHide` for frames that became hidden.
pub fn change_visibility(lua: &Lua, id: FrameId, change: impl FnOnce(&mut Vec<FrameData>)) {
    let (shown, hidden) = {
        let mut st = state(lua);
        let before = visible_subtree(&st.frames, id);
        change(&mut st.frames);
        crate::layout::invalidate(&mut st);
        let after = visible_subtree(&st.frames, id);
        let shown: Vec<_> = after
            .iter()
            .copied()
            .filter(|f| !before.contains(f))
            .collect();
        let hidden: Vec<_> = before
            .iter()
            .copied()
            .filter(|f| !after.contains(f))
            .collect();
        (shown, hidden)
    };
    for f in hidden {
        {
            let mut st = state(lua);
            if st.focus == Some(f) {
                st.focus = None;
            }
        }
        // Hiding a frame that follows the mouse stops the move.
        crate::movement::stop(lua, f);
        fire_script(lua, f, "OnHide", ());
    }
    for f in shown {
        if is_visible(&state(lua).frames, f) {
            if state(lua).frames[f].toplevel {
                crate::movement::raise(lua, f);
            }
            fire_script(lua, f, "OnShow", ());
        }
    }
}

/// The script entry table `{ handler = f, hooks = {...} }`, created if
/// `create` is set.
fn script_entry(lua: &Lua, id: FrameId, name: &str, create: bool) -> Result<Option<Table>> {
    let scripts = state(lua).script_table.clone();
    let per_object = match scripts.raw_get::<Value>(id as i64 + 1)? {
        Value::Table(t) => t,
        _ if create => {
            let t = lua.create_table()?;
            scripts.raw_set(id as i64 + 1, t.clone())?;
            t
        }
        _ => return Ok(None),
    };
    match per_object.raw_get::<Value>(name)? {
        Value::Table(t) => Ok(Some(t)),
        _ if create => {
            let t = lua.create_table()?;
            per_object.raw_set(name, t.clone())?;
            Ok(Some(t))
        }
        _ => Ok(None),
    }
}

/// `SetScript`: replace the handler and remove hooks.
pub fn set_script(lua: &Lua, id: FrameId, name: &str, handler: Option<Function>) -> Result<()> {
    let entry = script_entry(lua, id, name, true)?.expect("script entry");
    entry.raw_set("hooks", Value::Nil)?;
    let has = handler.is_some();
    entry.raw_set("handler", handler)?;
    let mut st = state(lua);
    // Like the client, a mouse script handler enables mouse input.
    if has && st.frames[id].is_frame() {
        match name {
            "OnEnter" | "OnLeave" => st.frames[id].mouse_motion = true,
            "OnMouseDown" | "OnMouseUp" | "OnClick" | "OnDoubleClick" => {
                st.frames[id].mouse_enabled = true
            }
            _ => {}
        }
    }
    let names = &mut st.frames[id].scripts;
    names.retain(|n| n != name);
    if has {
        names.push(name.to_string());
    }
    Ok(())
}

pub fn get_script(lua: &Lua, id: FrameId, name: &str) -> Result<Option<Function>> {
    match script_entry(lua, id, name, false)? {
        Some(e) => e.raw_get("handler"),
        None => Ok(None),
    }
}

pub fn hook_script(lua: &Lua, id: FrameId, name: &str, hook: Function) -> Result<()> {
    let entry = script_entry(lua, id, name, true)?.expect("script entry");
    let hooks = match entry.raw_get::<Value>("hooks")? {
        Value::Table(t) => t,
        _ => {
            let t = lua.create_table()?;
            entry.raw_set("hooks", t.clone())?;
            t
        }
    };
    hooks.raw_push(hook)?;
    let mut st = state(lua);
    if !st.frames[id].has_script(name) {
        st.frames[id].scripts.push(name.to_string());
    }
    Ok(())
}

pub fn clear_scripts(lua: &Lua, id: FrameId) -> Result<()> {
    let scripts = state(lua).script_table.clone();
    scripts.raw_set(id as i64 + 1, Value::Nil)?;
    state(lua).frames[id].scripts.clear();
    Ok(())
}

/// Every script handler the engine fires. `wowprogramming coverage`
/// compares it with the handlers in `UI.xsd`; a test checks it against the
/// calls to [`fire_script`] in the source.
pub const FIRED_SCRIPTS: &[&str] = &[
    "OnAttributeChanged",
    "OnChar",
    "OnClick",
    "OnDisable",
    "OnDoubleClick",
    "OnDragStart",
    "OnDragStop",
    "OnEditFocusGained",
    "OnEditFocusLost",
    "OnEnable",
    "OnEnter",
    "OnEnterPressed",
    "OnEscapePressed",
    "OnEvent",
    "OnHide",
    "OnHorizontalScroll",
    "OnLeave",
    "OnLoad",
    "OnMouseDown",
    "OnMouseUp",
    "OnMouseWheel",
    "OnShow",
    "OnSizeChanged",
    "OnTabPressed",
    "OnTextChanged",
    "OnUpdate",
    "OnValueChanged",
    "OnVerticalScroll",
    "PostClick",
    "OnReceiveDrag",
    "PreClick",
    // Animation scripts (not in ScriptsType).
    "OnFinished",
    "OnLoop",
    "OnPlay",
    "OnStop",
];

/// Run a widget's script handler and its hooks with `(self, args...)`.
pub fn fire_script(lua: &Lua, id: FrameId, script: &str, args: impl IntoLuaMulti) {
    debug_assert!(
        FIRED_SCRIPTS.contains(&script),
        "{script} is missing from FIRED_SCRIPTS"
    );
    let has = state(lua)
        .frames
        .get(id)
        .is_some_and(|f| f.has_script(script));
    if !has {
        return;
    }
    let Ok(Some(entry)) = script_entry(lua, id, script, false) else {
        return;
    };
    let mut funcs: Vec<Function> = Vec::new();
    if let Ok(Some(h)) = entry.raw_get::<Option<Function>>("handler") {
        funcs.push(h);
    }
    if let Ok(Value::Table(hooks)) = entry.raw_get::<Value>("hooks") {
        funcs.extend(hooks.sequence_values::<Function>().flatten());
    }
    if funcs.is_empty() {
        return;
    }
    let table = state(lua).table(id);
    let Ok(mut args) = args.into_lua_multi(lua) else {
        return;
    };
    args.push_front(Value::Table(table));
    for f in funcs {
        safe_call(lua, &f, args.clone());
    }
}

/// Deliver an event to every frame registered for it.
pub fn dispatch_event(lua: &Lua, event: &str, args: MultiValue) {
    let targets: Vec<FrameId> = {
        let st = state(lua);
        let mut ids = st.event_frames.get(event).cloned().unwrap_or_default();
        for &f in &st.all_event_frames {
            if !ids.contains(&f) {
                ids.push(f);
            }
        }
        ids
    };
    let unit = match args.front() {
        Some(Value::String(s)) => Some(s.to_string_lossy()),
        _ => None,
    };
    for id in targets {
        let deliver = {
            let st = state(lua);
            let frame = &st.frames[id];
            let registered = frame.all_events || frame.events.iter().any(|e| e == event);
            let unit_ok = match (frame.unit_filters.get(event), &unit) {
                (Some(units), Some(u)) => units.iter().any(|x| x.eq_ignore_ascii_case(u)),
                (Some(_), None) => false,
                (None, _) => true,
            };
            registered && unit_ok
        };
        if deliver {
            let mut call_args = args.clone();
            call_args.push_front(Value::String(lua.create_string(event).unwrap()));
            fire_script(lua, id, "OnEvent", call_args);
        }
    }
}

/// Run `OnUpdate` for every visible frame that has one, and advance
/// playing animations.
pub fn run_on_update(lua: &Lua, elapsed: f64) {
    let targets: Vec<FrameId> = {
        let st = state(lua);
        (0..st.frames.len())
            .filter(|&i| st.frames[i].has_script("OnUpdate"))
            .filter(|&i| st.frames[i].anim.is_none())
            .filter(|&i| is_visible(&st.frames, i))
            .collect()
    };
    for id in targets {
        if is_visible(&state(lua).frames, id) {
            fire_script(lua, id, "OnUpdate", elapsed);
        }
    }
    crate::widgets::advance_animations(lua, elapsed);
}

pub fn register_event(lua: &Lua, id: FrameId, event: &str) {
    let mut st = state(lua);
    if !st.frames[id].events.iter().any(|e| e == event) {
        st.frames[id].events.push(event.to_string());
        st.event_frames
            .entry(event.to_string())
            .or_default()
            .push(id);
    }
}

pub fn unregister_event(lua: &Lua, id: FrameId, event: &str) {
    let mut st = state(lua);
    st.frames[id].events.retain(|e| e != event);
    st.frames[id].unit_filters.remove(event);
    if let Some(list) = st.event_frames.get_mut(event) {
        list.retain(|&f| f != id);
    }
}

/// Default texture state, for widgets that gain one later.
pub fn ensure_tex(data: &mut FrameData) -> &mut TexState {
    data.tex.get_or_insert_with(Box::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FIRED_SCRIPTS lists exactly the handler names passed to fire_script.
    #[test]
    fn fired_scripts_match_the_source() {
        let sources = [
            include_str!("frames.rs"),
            include_str!("input.rs"),
            include_str!("widgets.rs"),
            include_str!("xml.rs"),
        ];
        let mut found = std::collections::BTreeSet::new();
        for src in sources {
            for (i, _) in src.match_indices("fire_script(") {
                let call = &src[i..src.len().min(i + 200)];
                let call = &call[..call.find(';').unwrap_or(call.len())];
                for part in call.split('"').skip(1).step_by(2) {
                    let handler = part.starts_with("On") || part.ends_with("Click");
                    if handler && part.chars().all(|c| c.is_ascii_alphabetic()) {
                        found.insert(part.to_string());
                    }
                }
            }
        }
        let listed: std::collections::BTreeSet<String> =
            FIRED_SCRIPTS.iter().map(|s| s.to_string()).collect();
        assert_eq!(found, listed);
    }

    #[test]
    fn object_type_hierarchy() {
        assert!(is_object_type("CheckButton", "Button"));
        assert!(is_object_type("CheckButton", "frame"));
        assert!(is_object_type("Frame", "Object"));
        assert!(is_object_type("Texture", "Region"));
        assert!(is_object_type("Alpha", "Animation"));
        assert!(!is_object_type("Frame", "Button"));
    }
}
