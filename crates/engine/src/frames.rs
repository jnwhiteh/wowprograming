//! Frame objects: `CreateFrame`, scripts, events and visibility.
//!
//! As in the client, a frame is a Lua table whose index `[0]` holds a
//! userdata that identifies the native object. Each frame type has its own
//! metatable, so addons can add fields to frames and hook type methods.
//!
//! Layout is not computed yet. Size and anchors are stored and returned
//! as given.

use std::collections::HashMap;

use mlua::{AnyUserData, Function, IntoLuaMulti, Lua, MultiValue, Result, Table, Value, Variadic};
use wowprogramming_lua::mlua;

use crate::natives::safe_call;
use crate::state::{FrameData, FrameId, FrameRef, Point, state};

/// Frame types and their parent type, for `IsObjectType`.
const FRAME_TYPES: &[(&str, &str)] = &[
    ("Frame", "ScriptRegion"),
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
    ("MovieFrame", "Frame"),
    ("Browser", "Frame"),
    ("Minimap", "Frame"),
];

const ABSTRACT_TYPES: &[(&str, &str)] = &[
    ("ScriptRegion", "ScriptObject"),
    ("ScriptObject", "Object"),
    ("Object", ""),
];

const POINTS: &[&str] = &[
    "TOPLEFT",
    "TOP",
    "TOPRIGHT",
    "LEFT",
    "CENTER",
    "RIGHT",
    "BOTTOMLEFT",
    "BOTTOM",
    "BOTTOMRIGHT",
];

const STRATA: &[&str] = &[
    "BACKGROUND",
    "LOW",
    "MEDIUM",
    "HIGH",
    "DIALOG",
    "FULLSCREEN",
    "FULLSCREEN_DIALOG",
    "TOOLTIP",
];

fn canonical_type(name: &str) -> Option<&'static str> {
    FRAME_TYPES
        .iter()
        .find(|(t, _)| t.eq_ignore_ascii_case(name))
        .map(|(t, _)| *t)
}

fn parent_type(name: &str) -> Option<&'static str> {
    FRAME_TYPES
        .iter()
        .chain(ABSTRACT_TYPES)
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

pub fn install(lua: &Lua) -> Result<()> {
    let methods = frame_methods(lua)?;
    let mut metatables = HashMap::new();
    for (type_name, _) in FRAME_TYPES {
        let index = lua.create_table()?;
        for pair in methods.pairs::<Value, Value>() {
            let (k, v) = pair?;
            index.raw_set(k, v)?;
        }
        let mt = lua.create_table()?;
        mt.raw_set("__index", index)?;
        metatables.insert(*type_name, mt);
    }
    state(lua).frame_metatables = metatables;

    lua.globals().set(
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
                let Some(object_type) = canonical_type(&frame_type) else {
                    return Err(mlua::Error::runtime(format!(
                        "CreateFrame: Unknown frame type '{frame_type}'"
                    )));
                };
                let parent = optional_frame(lua, &parent)?;
                if let Some(template) = template {
                    warn_template(lua, &template);
                }
                let table = create_frame(lua, object_type, name, parent)?;
                if let Some(id) = id {
                    let fid = frame_id(&table)?;
                    state(lua).frames[fid].id = id;
                }
                Ok(table)
            },
        )?,
    )?;
    let create_frame_fn: Function = lua.globals().get("CreateFrame")?;
    lua.globals().set("CreateForbiddenFrame", create_frame_fn)?;
    Ok(())
}

fn warn_template(lua: &Lua, template: &str) {
    let mut st = state(lua);
    if !st.warned_templates.iter().any(|t| t == template) {
        st.warned_templates.push(template.to_string());
        st.warn(format!(
            "CreateFrame: templates are not supported yet, ignoring '{template}'"
        ));
    }
}

/// Create a frame and return its table.
pub fn create_frame(
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

    let name = name.map(|n| expand_parent_name(&st.frames, &n, parent));
    let level = parent.map(|p| st.frames[p].level + 1).unwrap_or(0);
    let strata = parent
        .map(|p| st.frames[p].strata.clone())
        .unwrap_or_else(|| "MEDIUM".to_string());
    st.frames.push(FrameData {
        object_type,
        name: name.clone(),
        table: table.clone(),
        parent,
        children: Vec::new(),
        shown: true,
        scripts: HashMap::new(),
        events: Vec::new(),
        unit_filters: HashMap::new(),
        all_events: false,
        id: 0,
        width: 0.0,
        height: 0.0,
        points: Vec::new(),
        alpha: 1.0,
        scale: 1.0,
        strata,
        level,
        mouse_enabled: false,
        movable: false,
        attributes: None,
        forbidden: false,
    });
    if let Some(p) = parent {
        st.frames[p].children.push(id);
    }
    drop(st);
    if let Some(name) = name {
        lua.globals().set(name, table.clone())?;
    }
    Ok(table)
}

fn expand_parent_name(frames: &[FrameData], name: &str, parent: Option<FrameId>) -> String {
    let lower = name.to_ascii_lowercase();
    match lower.find("$parent") {
        Some(pos) => {
            let parent_name = parent
                .and_then(|p| frames[p].name.clone())
                .unwrap_or_default();
            format!("{}{}{}", &name[..pos], parent_name, &name[pos + 7..])
        }
        None => name.to_string(),
    }
}

/// Get the frame id from a frame table.
pub fn frame_id(table: &Table) -> Result<FrameId> {
    let ud: AnyUserData = table.raw_get(0)?;
    Ok(ud.borrow::<FrameRef>()?.0)
}

/// Resolve a method's `self` argument.
fn this(value: &Value) -> Result<FrameId> {
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

/// Resolve an optional frame argument: a frame table, a global name or nil.
fn optional_frame(lua: &Lua, value: &Value) -> Result<Option<FrameId>> {
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
fn change_visibility(lua: &Lua, id: FrameId, change: impl FnOnce(&mut Vec<FrameData>)) {
    let (shown, hidden) = {
        let mut st = state(lua);
        let before = visible_subtree(&st.frames, id);
        change(&mut st.frames);
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
        fire_script(lua, f, "OnHide", ());
    }
    for f in shown {
        if is_visible(&state(lua).frames, f) {
            fire_script(lua, f, "OnShow", ());
        }
    }
}

/// Run a frame's script handler and its hooks with `(self, args...)`.
pub fn fire_script(lua: &Lua, id: FrameId, script: &str, args: impl IntoLuaMulti) {
    let (table, funcs) = {
        let st = state(lua);
        let frame = &st.frames[id];
        let Some(s) = frame.scripts.get(script) else {
            return;
        };
        let funcs: Vec<Function> = s.handler.iter().chain(s.hooks.iter()).cloned().collect();
        (frame.table.clone(), funcs)
    };
    if funcs.is_empty() {
        return;
    }
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

/// Run `OnUpdate` for every visible frame that has one.
pub fn run_on_update(lua: &Lua, elapsed: f64) {
    let targets: Vec<FrameId> = {
        let st = state(lua);
        (0..st.frames.len())
            .filter(|&i| st.frames[i].scripts.contains_key("OnUpdate"))
            .filter(|&i| is_visible(&st.frames, i))
            .collect()
    };
    for id in targets {
        if is_visible(&state(lua).frames, id) {
            fire_script(lua, id, "OnUpdate", elapsed);
        }
    }
}

fn register_event(lua: &Lua, id: FrameId, event: &str) {
    let mut st = state(lua);
    if !st.frames[id].events.iter().any(|e| e == event) {
        st.frames[id].events.push(event.to_string());
        st.event_frames
            .entry(event.to_string())
            .or_default()
            .push(id);
    }
}

fn unregister_event(lua: &Lua, id: FrameId, event: &str) {
    let mut st = state(lua);
    st.frames[id].events.retain(|e| e != event);
    st.frames[id].unit_filters.remove(event);
    if let Some(list) = st.event_frames.get_mut(event) {
        list.retain(|&f| f != id);
    }
}

fn normalize_point(lua_value: &str) -> Result<String> {
    let upper = lua_value.to_ascii_uppercase();
    if POINTS.contains(&upper.as_str()) {
        Ok(upper)
    } else {
        Err(mlua::Error::runtime(format!("Invalid point '{lua_value}'")))
    }
}

/// Parse the flexible argument forms of `SetPoint`.
fn parse_set_point(lua: &Lua, args: &[Value]) -> Result<Point> {
    let point = match args.first() {
        Some(Value::String(s)) => normalize_point(&s.to_string_lossy())?,
        _ => {
            return Err(mlua::Error::runtime(
                "Usage: SetPoint(\"point\" [, region or nil] [, \"relativePoint\"] [, offsetX, offsetY])",
            ));
        }
    };
    let num = |v: Option<&Value>| match v {
        Some(Value::Number(n)) => *n,
        Some(Value::Integer(n)) => *n as f64,
        _ => 0.0,
    };
    let is_num = |v: Option<&Value>| matches!(v, Some(Value::Number(_) | Value::Integer(_)));

    // SetPoint(point, x, y)
    if is_num(args.get(1)) {
        return Ok(Point {
            relative_point: point.clone(),
            point,
            relative_to: Value::Nil,
            x: num(args.get(1)),
            y: num(args.get(2)),
        });
    }
    let relative_to = match args.get(1) {
        Some(Value::String(s)) => lua.globals().get::<Value>(s.clone())?,
        Some(v) => v.clone(),
        None => Value::Nil,
    };
    let (relative_point, rest) = match args.get(2) {
        Some(Value::String(s)) => (normalize_point(&s.to_string_lossy())?, 3),
        _ => (point.clone(), 2),
    };
    Ok(Point {
        point,
        relative_to,
        relative_point,
        x: num(args.get(rest)),
        y: num(args.get(rest + 1)),
    })
}

/// Build the method table shared by all frame types.
fn frame_methods(lua: &Lua) -> Result<Table> {
    let m = lua.create_table()?;

    macro_rules! method {
        ($name:literal, $f:expr) => {
            m.raw_set($name, lua.create_function($f)?)?;
        };
    }

    method!("GetName", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(state(lua).frames[id].name.clone())
    });
    method!("GetDebugName", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(state(lua).frames[id]
            .name
            .clone()
            .unwrap_or_else(|| format!("<unnamed {}>", id)))
    });
    method!("GetObjectType", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(state(lua).frames[id].object_type)
    });
    method!("IsObjectType", |lua, (this_, query): (Value, String)| {
        let id = this(&this_)?;
        Ok(is_object_type(state(lua).frames[id].object_type, &query))
    });
    method!("IsForbidden", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(state(lua).frames[id].forbidden)
    });
    method!("SetForbidden", |lua, this_: Value| {
        let id = this(&this_)?;
        state(lua).frames[id].forbidden = true;
        Ok(())
    });
    method!("IsProtected", |_, _this: Value| Ok((false, false)));
    method!("CanChangeProtectedState", |_, _this: Value| Ok(true));
    method!("GetID", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(state(lua).frames[id].id)
    });
    method!("SetID", |lua, (this_, value): (Value, i64)| {
        let id = this(&this_)?;
        state(lua).frames[id].id = value;
        Ok(())
    });

    // Hierarchy
    method!("GetParent", |lua, this_: Value| {
        let id = this(&this_)?;
        let st = state(lua);
        Ok(st.frames[id].parent.map(|p| st.frames[p].table.clone()))
    });
    method!("SetParent", |lua, (this_, parent): (Value, Value)| {
        let id = this(&this_)?;
        let parent = optional_frame(lua, &parent)?;
        change_visibility(lua, id, |frames| {
            if let Some(old) = frames[id].parent {
                frames[old].children.retain(|&c| c != id);
            }
            frames[id].parent = parent;
            if let Some(p) = parent {
                frames[p].children.push(id);
            }
        });
        Ok(())
    });
    method!("GetChildren", |lua, this_: Value| {
        let id = this(&this_)?;
        let st = state(lua);
        let children: Variadic<Table> = st.frames[id]
            .children
            .iter()
            .map(|&c| st.frames[c].table.clone())
            .collect();
        Ok(children)
    });
    method!("GetNumChildren", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(state(lua).frames[id].children.len())
    });

    // Visibility
    method!("Show", |lua, this_: Value| {
        let id = this(&this_)?;
        change_visibility(lua, id, |frames| frames[id].shown = true);
        Ok(())
    });
    method!("Hide", |lua, this_: Value| {
        let id = this(&this_)?;
        change_visibility(lua, id, |frames| frames[id].shown = false);
        Ok(())
    });
    method!("SetShown", |lua, (this_, shown): (Value, Value)| {
        let id = this(&this_)?;
        let shown = !matches!(shown, Value::Nil | Value::Boolean(false));
        change_visibility(lua, id, |frames| frames[id].shown = shown);
        Ok(())
    });
    method!("IsShown", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(state(lua).frames[id].shown)
    });
    method!("IsVisible", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(is_visible(&state(lua).frames, id))
    });

    // Scripts
    method!("SetScript", |lua,
                          (this_, name, handler): (
        Value,
        String,
        Option<Function>
    )| {
        let id = this(&this_)?;
        let mut st = state(lua);
        let script = st.frames[id].scripts.entry(name.clone()).or_default();
        script.handler = handler;
        script.hooks.clear();
        if script.handler.is_none() {
            st.frames[id].scripts.remove(&name);
        }
        Ok(())
    });
    method!("GetScript", |lua, (this_, name): (Value, String)| {
        let id = this(&this_)?;
        let st = state(lua);
        Ok(st.frames[id]
            .scripts
            .get(&name)
            .and_then(|s| s.handler.clone()))
    });
    method!("HookScript", |lua,
                           (this_, name, hook): (
        Value,
        String,
        Function
    )| {
        let id = this(&this_)?;
        state(lua).frames[id]
            .scripts
            .entry(name)
            .or_default()
            .hooks
            .push(hook);
        Ok(())
    });
    method!("HasScript", |_, (this_, name): (Value, String)| {
        this(&this_)?;
        Ok(name.starts_with("On") || name == "PreClick" || name == "PostClick")
    });

    // Events
    method!("RegisterEvent", |lua, (this_, event): (Value, String)| {
        let id = this(&this_)?;
        register_event(lua, id, &event);
        Ok(true)
    });
    method!("RegisterUnitEvent", |lua,
                                  (this_, event, units): (
        Value,
        String,
        Variadic<String>
    )| {
        let id = this(&this_)?;
        register_event(lua, id, &event);
        state(lua).frames[id]
            .unit_filters
            .insert(event, units.into_iter().collect());
        Ok(true)
    });
    method!("UnregisterEvent", |lua, (this_, event): (Value, String)| {
        let id = this(&this_)?;
        unregister_event(lua, id, &event);
        Ok(())
    });
    method!("UnregisterAllEvents", |lua, this_: Value| {
        let id = this(&this_)?;
        let events = state(lua).frames[id].events.clone();
        for e in events {
            unregister_event(lua, id, &e);
        }
        let mut st = state(lua);
        st.frames[id].all_events = false;
        st.all_event_frames.retain(|&f| f != id);
        Ok(())
    });
    method!("RegisterAllEvents", |lua, this_: Value| {
        let id = this(&this_)?;
        let mut st = state(lua);
        st.frames[id].all_events = true;
        if !st.all_event_frames.contains(&id) {
            st.all_event_frames.push(id);
        }
        Ok(())
    });
    method!(
        "IsEventRegistered",
        |lua, (this_, event): (Value, String)| {
            let id = this(&this_)?;
            let st = state(lua);
            let frame = &st.frames[id];
            Ok(frame.all_events || frame.events.contains(&event))
        }
    );

    // Attributes
    method!("SetAttribute", |lua,
                             (this_, name, value): (
        Value,
        String,
        Value
    )| {
        let id = this(&this_)?;
        let attrs = {
            let mut st = state(lua);
            match &st.frames[id].attributes {
                Some(t) => t.clone(),
                None => {
                    let t = lua.create_table()?;
                    st.frames[id].attributes = Some(t.clone());
                    t
                }
            }
        };
        attrs.raw_set(name.clone(), value.clone())?;
        fire_script(lua, id, "OnAttributeChanged", (name, value));
        Ok(())
    });
    method!("GetAttribute", |lua, (this_, name): (Value, String)| {
        let id = this(&this_)?;
        let attrs = state(lua).frames[id].attributes.clone();
        match attrs {
            Some(t) => t.raw_get::<Value>(name),
            None => Ok(Value::Nil),
        }
    });

    // Size and anchors (stored only; layout comes later)
    method!("SetSize", |lua, (this_, w, h): (Value, f64, f64)| {
        let id = this(&this_)?;
        let mut st = state(lua);
        st.frames[id].width = w;
        st.frames[id].height = h;
        Ok(())
    });
    method!("SetWidth", |lua, (this_, w): (Value, f64)| {
        let id = this(&this_)?;
        state(lua).frames[id].width = w;
        Ok(())
    });
    method!("SetHeight", |lua, (this_, h): (Value, f64)| {
        let id = this(&this_)?;
        state(lua).frames[id].height = h;
        Ok(())
    });
    method!("GetWidth", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(state(lua).frames[id].width)
    });
    method!("GetHeight", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(state(lua).frames[id].height)
    });
    method!("GetSize", |lua, this_: Value| {
        let id = this(&this_)?;
        let st = state(lua);
        Ok((st.frames[id].width, st.frames[id].height))
    });
    method!("SetPoint", |lua, args: Variadic<Value>| {
        let id = this(args.first().unwrap_or(&Value::Nil))?;
        let point = parse_set_point(lua, &args[1..])?;
        let mut st = state(lua);
        let points = &mut st.frames[id].points;
        points.retain(|p| p.point != point.point);
        points.push(point);
        Ok(())
    });
    method!("SetAllPoints", |lua, (this_, relative): (Value, Value)| {
        let id = this(&this_)?;
        let relative = match relative {
            Value::String(s) => lua.globals().get::<Value>(s)?,
            Value::Boolean(_) => Value::Nil,
            v => v,
        };
        let mut st = state(lua);
        st.frames[id].points = ["TOPLEFT", "BOTTOMRIGHT"]
            .iter()
            .map(|p| Point {
                point: p.to_string(),
                relative_to: relative.clone(),
                relative_point: p.to_string(),
                x: 0.0,
                y: 0.0,
            })
            .collect();
        Ok(())
    });
    method!("ClearAllPoints", |lua, this_: Value| {
        let id = this(&this_)?;
        state(lua).frames[id].points.clear();
        Ok(())
    });
    method!("GetNumPoints", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(state(lua).frames[id].points.len())
    });
    method!("GetPoint", |lua, (this_, index): (Value, Option<usize>)| {
        let id = this(&this_)?;
        let st = state(lua);
        let index = index.unwrap_or(1);
        match index
            .checked_sub(1)
            .and_then(|i| st.frames[id].points.get(i))
        {
            Some(p) => (
                p.point.clone(),
                p.relative_to.clone(),
                p.relative_point.clone(),
                p.x,
                p.y,
            )
                .into_lua_multi(lua),
            None => Ok(MultiValue::new()),
        }
    });

    // Appearance and stacking (stored only)
    method!("SetAlpha", |lua, (this_, a): (Value, f64)| {
        let id = this(&this_)?;
        state(lua).frames[id].alpha = a.clamp(0.0, 1.0);
        Ok(())
    });
    method!("GetAlpha", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(state(lua).frames[id].alpha)
    });
    method!("SetScale", |lua, (this_, s): (Value, f64)| {
        let id = this(&this_)?;
        if s <= 0.0 {
            return Err(mlua::Error::runtime("Scale must be > 0"));
        }
        state(lua).frames[id].scale = s;
        Ok(())
    });
    method!("GetScale", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(state(lua).frames[id].scale)
    });
    method!("GetEffectiveScale", |lua, this_: Value| {
        let id = this(&this_)?;
        let st = state(lua);
        let mut scale = 1.0;
        let mut current = Some(id);
        while let Some(i) = current {
            scale *= st.frames[i].scale;
            current = st.frames[i].parent;
        }
        Ok(scale)
    });
    method!("SetFrameStrata", |lua, (this_, strata): (Value, String)| {
        let id = this(&this_)?;
        let upper = strata.to_ascii_uppercase();
        if STRATA.contains(&upper.as_str()) {
            state(lua).frames[id].strata = upper;
        }
        Ok(())
    });
    method!("GetFrameStrata", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(state(lua).frames[id].strata.clone())
    });
    method!("SetFrameLevel", |lua, (this_, level): (Value, i64)| {
        let id = this(&this_)?;
        state(lua).frames[id].level = level.max(0);
        Ok(())
    });
    method!("GetFrameLevel", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(state(lua).frames[id].level)
    });
    method!("EnableMouse", |lua, (this_, enable): (Value, Value)| {
        let id = this(&this_)?;
        state(lua).frames[id].mouse_enabled = !matches!(enable, Value::Nil | Value::Boolean(false));
        Ok(())
    });
    method!("IsMouseEnabled", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(state(lua).frames[id].mouse_enabled)
    });
    method!("SetMovable", |lua, (this_, movable): (Value, Value)| {
        let id = this(&this_)?;
        state(lua).frames[id].movable = !matches!(movable, Value::Nil | Value::Boolean(false));
        Ok(())
    });
    method!("IsMovable", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(state(lua).frames[id].movable)
    });

    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_type_hierarchy() {
        assert!(is_object_type("CheckButton", "Button"));
        assert!(is_object_type("CheckButton", "frame"));
        assert!(is_object_type("Frame", "Object"));
        assert!(!is_object_type("Frame", "Button"));
    }
}
