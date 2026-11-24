//! Native widget methods, in groups. `frames::install` combines the groups
//! into one metatable per widget type.

use std::collections::HashMap;

use mlua::{Function, IntoLuaMulti, Lua, MultiValue, Result, Table, Value, Variadic};
use wowprogramming_lua::mlua;

use crate::frames::{
    change_visibility, create_object, fire_script, frame_id, is_visible, optional_frame,
    register_event, this, unregister_event,
};
use crate::layout::{self, effective_scale};
use crate::state::{FrameData, FrameId, Point, State, TexFile, state};
use crate::text;

pub const POINTS: &[&str] = &[
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

pub const STRATA: &[&str] = &[
    "WORLD",
    "BACKGROUND",
    "LOW",
    "MEDIUM",
    "HIGH",
    "DIALOG",
    "FULLSCREEN",
    "FULLSCREEN_DIALOG",
    "TOOLTIP",
    "BLIZZARD",
];

pub const LAYERS: &[&str] = &["BACKGROUND", "BORDER", "ARTWORK", "OVERLAY", "HIGHLIGHT"];

pub fn layer_index(name: &str) -> Option<u8> {
    LAYERS
        .iter()
        .position(|l| l.eq_ignore_ascii_case(name))
        .map(|i| i as u8)
}

pub fn strata_name(name: &str) -> Option<&'static str> {
    let upper = name.to_ascii_uppercase();
    STRATA.iter().find(|s| **s == upper).copied()
}

pub fn point_name(name: &str) -> Option<&'static str> {
    let upper = name.to_ascii_uppercase();
    POINTS.iter().find(|p| **p == upper).copied()
}

pub fn truthy(v: &Value) -> bool {
    !matches!(v, Value::Nil | Value::Boolean(false))
}

pub fn num(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => *n,
        Some(Value::Integer(n)) => *n as f64,
        Some(Value::String(s)) => s.to_string_lossy().trim().parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn is_num(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Number(_) | Value::Integer(_)))
}

/// Run `f` on the widget's data.
fn with<R>(lua: &Lua, this_: &Value, f: impl FnOnce(&mut FrameData) -> R) -> Result<R> {
    let id = this(this_)?;
    Ok(f(&mut state(lua).frames[id]))
}

fn table_of(lua: &Lua, id: Option<FrameId>) -> Value {
    match id {
        Some(i) => Value::Table(state(lua).table(i)),
        None => Value::Nil,
    }
}

/// Read a colour from `(r, g, b [, a])` arguments.
fn color_args(args: &[Value]) -> [f32; 4] {
    let a = if args.len() > 3 && !args[3].is_nil() {
        num(args.get(3))
    } else {
        1.0
    };
    [
        num(args.first()) as f32,
        num(args.get(1)) as f32,
        num(args.get(2)) as f32,
        a as f32,
    ]
}

/// Read a colour from a ColorMixin-style table.
fn color_table(v: &Value) -> [f32; 4] {
    match v {
        Value::Table(t) => {
            let g = |k: &str, d: f64| t.get::<Option<f64>>(k).ok().flatten().unwrap_or(d) as f32;
            [g("r", 1.0), g("g", 1.0), g("b", 1.0), g("a", 1.0)]
        }
        _ => [1.0; 4],
    }
}

/// Resolve a region argument to `SetPoint`: a widget, a global name or nil.
fn relative_region(lua: &Lua, v: Option<&Value>) -> Result<Option<FrameId>> {
    match v {
        None | Some(Value::Nil) => Ok(None),
        Some(Value::String(s)) => match lua.globals().get::<Value>(s.clone())? {
            Value::Table(t) => Ok(frame_id(&t).ok()),
            _ => Ok(None),
        },
        Some(Value::Table(t)) => Ok(frame_id(t).ok()),
        Some(other) => Err(mlua::Error::runtime(format!(
            "SetPoint: bad relative region ({})",
            other.type_name()
        ))),
    }
}

/// Parse the flexible argument forms of `SetPoint`.
fn parse_set_point(lua: &Lua, args: &[Value]) -> Result<Point> {
    let point = match args.first() {
        Some(Value::String(s)) => point_name(&s.to_string_lossy()).ok_or_else(|| {
            mlua::Error::runtime(format!("Invalid point '{}'", s.to_string_lossy()))
        })?,
        _ => {
            return Err(mlua::Error::runtime(
                "Usage: SetPoint(\"point\" [, region or nil] [, \"relativePoint\"] [, offsetX, offsetY])",
            ));
        }
    };
    if is_num(args.get(1)) {
        return Ok(Point {
            point,
            relative: None,
            relative_point: point,
            x: num(args.get(1)),
            y: num(args.get(2)),
        });
    }
    let relative = relative_region(lua, args.get(1))?;
    let (relative_point, rest) = match args.get(2) {
        Some(Value::String(s)) => (
            point_name(&s.to_string_lossy()).ok_or_else(|| {
                mlua::Error::runtime(format!("Invalid relative point '{}'", s.to_string_lossy()))
            })?,
            3,
        ),
        _ => (point, 2),
    };
    Ok(Point {
        point,
        relative,
        relative_point,
        x: num(args.get(rest)),
        y: num(args.get(rest + 1)),
    })
}

/// `GetDebugName`: the global name, or the parent's name plus parent keys.
fn debug_name(lua: &Lua, id: FrameId) -> String {
    let (name, parent, table) = {
        let st = state(lua);
        let f = &st.frames[id];
        (f.name.clone(), f.parent, st.table(id))
    };
    if let Some(n) = name {
        return n;
    }
    let Some(p) = parent else {
        return format!("<unnamed {}>", id);
    };
    let parent_table = state(lua).table(p);
    let mut key = None;
    for (k, v) in parent_table.pairs::<Value, Value>().flatten() {
        if let (Value::String(k), Value::Table(t)) = (&k, &v)
            && *t == table
        {
            key = Some(k.to_string_lossy());
            break;
        }
    }
    let base = debug_name(lua, p);
    match key {
        Some(k) => format!("{base}.{k}"),
        None => format!("{base}.{}", id),
    }
}

/// Scale-relative rectangle of a region in its own coordinate units.
fn own_rect(lua: &Lua, id: FrameId) -> Option<(f64, f64, f64, f64)> {
    let mut st = state(lua);
    let r = layout::rect(&mut st, id)?;
    let es = effective_scale(&st, id);
    Some((r.left / es, r.bottom / es, r.width / es, r.height / es))
}

pub fn method_groups(lua: &Lua) -> Result<HashMap<&'static str, Table>> {
    let mut groups = HashMap::new();
    groups.insert("Object", object_methods(lua)?);
    groups.insert("ScriptRegion", script_region_methods(lua)?);
    groups.insert("Region", region_methods(lua)?);
    groups.insert("Texture", texture_methods(lua)?);
    groups.insert("FontInstance", font_instance_methods(lua)?);
    groups.insert("FontString", font_string_methods(lua)?);
    groups.insert("Frame", frame_methods(lua)?);
    groups.insert("Button", button_methods(lua)?);
    groups.insert("CheckButton", check_button_methods(lua)?);
    groups.insert("EditBox", edit_box_methods(lua)?);
    groups.insert("Slider", value_methods(lua, true)?);
    groups.insert("StatusBar", value_methods(lua, false)?);
    groups.insert("ScrollFrame", scroll_frame_methods(lua)?);
    groups.insert("Cooldown", crate::cooldown::methods(lua)?);
    groups.insert("MessageFrame", crate::messageframe::methods(lua)?);
    groups.insert("AnimCommon", anim_common_methods(lua)?);
    // Tooltip methods override some frame methods (Show) and call them.
    let tooltip: Table = lua
        .load(include_str!("lua/tooltip.lua"))
        .set_name("=[C]")
        .call(groups["ScriptRegion"].clone())?;
    groups.insert("GameTooltip", tooltip);
    groups.insert("AnimationGroup", anim_group_methods(lua)?);
    groups.insert("Animation", animation_methods(lua)?);
    Ok(groups)
}

macro_rules! method {
    ($lua:ident, $t:ident, $name:literal, $f:expr) => {
        $t.raw_set($name, $lua.create_function($f)?)?;
    };
}

/// A method that does nothing yet or returns a fixed value. It is marked
/// so that `wowprogramming coverage` reports it as empty.
macro_rules! empty {
    ($lua:ident, $t:ident, $name:literal, $f:expr) => {
        let f = $lua.create_function($f)?;
        mark_empty($lua, &f)?;
        $t.raw_set($name, f)?;
    };
}

/// Registry key of the set of empty methods (function -> true).
pub const EMPTY_METHODS_KEY: &str = "wowprogramming.empty_methods";

/// Mark a method as empty for the coverage report.
pub fn mark_empty(lua: &Lua, f: &Function) -> Result<()> {
    let set = match lua.named_registry_value::<Value>(EMPTY_METHODS_KEY)? {
        Value::Table(t) => t,
        _ => {
            let t = lua.create_table()?;
            lua.set_named_registry_value(EMPTY_METHODS_KEY, t.clone())?;
            t
        }
    };
    set.raw_set(f.clone(), true)
}

/// A marked method that does nothing.
pub fn empty_fn(lua: &Lua) -> Result<Function> {
    let f = lua.create_function(|_, _: MultiValue| Ok(()))?;
    mark_empty(lua, &f)?;
    Ok(f)
}

fn object_methods(lua: &Lua) -> Result<Table> {
    let m = lua.create_table()?;
    method!(lua, m, "GetName", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.name.clone()
    ));
    method!(lua, m, "GetDebugName", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(debug_name(lua, id))
    });
    method!(lua, m, "GetObjectType", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.object_type
    ));
    method!(
        lua,
        m,
        "IsObjectType",
        |lua, (this_, query): (Value, String)| {
            with(lua, &this_, |f| {
                crate::frames::is_object_type(f.object_type, &query)
            })
        }
    );
    method!(lua, m, "IsForbidden", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.forbidden
    ));
    method!(lua, m, "SetForbidden", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.forbidden = true
    ));
    method!(lua, m, "IsProtected", |_, _this: Value| Ok((false, false)));
    method!(lua, m, "CanChangeProtectedState", |_, _this: Value| Ok(
        true
    ));
    method!(lua, m, "GetParent", |lua, this_: Value| {
        let id = this(&this_)?;
        let p = state(lua).frames[id].parent;
        Ok(table_of(lua, p))
    });
    method!(lua, m, "GetParentKey", |lua, this_: Value| {
        let id = this(&this_)?;
        let name = debug_name(lua, id);
        Ok(name.rsplit_once('.').map(|(_, k)| k.to_string()))
    });
    method!(
        lua,
        m,
        "SetParentKey",
        |lua, (this_, key): (Value, String)| {
            let id = this(&this_)?;
            let (parent, table) = {
                let st = state(lua);
                (st.frames[id].parent, st.table(id))
            };
            if let Value::Table(p) = table_of(lua, parent) {
                p.set(key, table)?;
            }
            Ok(())
        }
    );
    method!(lua, m, "ClearParentKey", |_, _this: Value| Ok(()));

    // Scripts work on frames, regions and animations.
    method!(lua, m, "SetScript", |lua,
                                  (this_, name, handler): (
        Value,
        String,
        Option<Function>
    )| {
        let id = this(&this_)?;
        crate::frames::set_script(lua, id, &name, handler)
    });
    method!(
        lua,
        m,
        "GetScript",
        |lua, (this_, name): (Value, String)| {
            let id = this(&this_)?;
            crate::frames::get_script(lua, id, &name)
        }
    );
    method!(lua, m, "HookScript", |lua,
                                   (this_, name, hook): (
        Value,
        String,
        Function
    )| {
        let id = this(&this_)?;
        crate::frames::hook_script(lua, id, &name, hook)
    });
    method!(lua, m, "HasScript", |_, (this_, name): (Value, String)| {
        this(&this_)?;
        Ok(name.starts_with("On") || name == "PreClick" || name == "PostClick")
    });
    method!(lua, m, "ClearScripts", |lua, this_: Value| {
        let id = this(&this_)?;
        crate::frames::clear_scripts(lua, id)
    });
    Ok(m)
}

fn script_region_methods(lua: &Lua) -> Result<Table> {
    let m = lua.create_table()?;
    // SetToDefaults: back to a freshly created state (pools call it when they
    // reuse a region): no anchors, no size, full alpha, scale 1, and no
    // texture or text.
    method!(lua, m, "SetToDefaults", |lua, this_: Value| {
        let id = this(&this_)?;
        let mut st = state(lua);
        let f = &mut st.frames[id];
        f.points.clear();
        f.width = 0.0;
        f.height = 0.0;
        f.alpha = 1.0;
        f.scale = 1.0;
        f.masks.clear();
        if f.tex.is_some() {
            f.tex = Some(Box::default());
        }
        if let Some(t) = f.text.as_deref_mut() {
            t.text.clear();
        }
        layout::invalidate(&mut st);
        Ok(())
    });

    // Visibility
    method!(lua, m, "Show", |lua, this_: Value| {
        let id = this(&this_)?;
        change_visibility(lua, id, |frames| frames[id].shown = true);
        Ok(())
    });
    method!(lua, m, "Hide", |lua, this_: Value| {
        let id = this(&this_)?;
        change_visibility(lua, id, |frames| frames[id].shown = false);
        Ok(())
    });
    method!(lua, m, "SetShown", |lua, (this_, shown): (Value, Value)| {
        let id = this(&this_)?;
        let shown = truthy(&shown);
        change_visibility(lua, id, |frames| frames[id].shown = shown);
        Ok(())
    });
    method!(lua, m, "IsShown", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.shown
    ));
    method!(lua, m, "IsVisible", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(is_visible(&state(lua).frames, id))
    });
    method!(
        lua,
        m,
        "SetParent",
        |lua, (this_, parent): (Value, Value)| {
            let id = this(&this_)?;
            let parent = optional_frame(lua, &parent)?;
            change_visibility(lua, id, |frames| {
                let is_region = frames[id].is_region();
                if let Some(old) = frames[id].parent {
                    frames[old].children.retain(|&c| c != id);
                    frames[old].regions.retain(|&c| c != id);
                }
                frames[id].parent = parent;
                if let Some(p) = parent {
                    if is_region {
                        frames[p].regions.push(id);
                    } else {
                        frames[p].children.push(id);
                        if !frames[id].strata_explicit {
                            frames[id].strata = frames[p].strata;
                        }
                        let min_level = frames[p].level + 1;
                        if frames[id].level < min_level {
                            raise_levels(frames, id, min_level);
                        }
                    }
                }
            });
            Ok(())
        }
    );

    // Size and anchors
    method!(
        lua,
        m,
        "SetSize",
        |lua, (this_, w, h): (Value, Value, Value)| {
            let id = this(&this_)?;
            let mut st = state(lua);
            st.frames[id].width = num(Some(&w)).max(0.0);
            st.frames[id].height = num(Some(&h)).max(0.0);
            layout::invalidate(&mut st);
            Ok(())
        }
    );
    method!(lua, m, "SetWidth", |lua, (this_, w): (Value, Value)| {
        let id = this(&this_)?;
        let mut st = state(lua);
        st.frames[id].width = num(Some(&w)).max(0.0);
        layout::invalidate(&mut st);
        Ok(())
    });
    method!(lua, m, "SetHeight", |lua, (this_, h): (Value, Value)| {
        let id = this(&this_)?;
        let mut st = state(lua);
        st.frames[id].height = num(Some(&h)).max(0.0);
        layout::invalidate(&mut st);
        Ok(())
    });
    method!(lua, m, "GetWidth", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(layout::size(&mut state(lua), id).0)
    });
    method!(lua, m, "GetHeight", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(layout::size(&mut state(lua), id).1)
    });
    method!(lua, m, "GetSize", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(layout::size(&mut state(lua), id))
    });
    method!(lua, m, "SetPoint", |lua, args: Variadic<Value>| {
        let id = this(args.first().unwrap_or(&Value::Nil))?;
        let point = parse_set_point(lua, &args[1..])?;
        if point.relative == Some(id) {
            return Err(mlua::Error::runtime("Cannot anchor to itself"));
        }
        let mut st = state(lua);
        let points = &mut st.frames[id].points;
        points.retain(|p| p.point != point.point);
        points.push(point);
        layout::invalidate(&mut st);
        Ok(())
    });
    method!(lua, m, "SetAllPoints", |lua,
                                     (this_, relative): (
        Value,
        Value
    )| {
        let id = this(&this_)?;
        let relative = match relative {
            Value::Boolean(_) => None,
            v => relative_region(lua, Some(&v))?,
        };
        let mut st = state(lua);
        st.frames[id].points = ["TOPLEFT", "BOTTOMRIGHT"]
            .iter()
            .map(|p| Point {
                point: p,
                relative,
                relative_point: p,
                x: 0.0,
                y: 0.0,
            })
            .collect();
        layout::invalidate(&mut st);
        Ok(())
    });
    method!(lua, m, "ClearAllPoints", |lua, this_: Value| {
        let id = this(&this_)?;
        let mut st = state(lua);
        st.frames[id].points.clear();
        layout::invalidate(&mut st);
        Ok(())
    });
    method!(
        lua,
        m,
        "ClearPoint",
        |lua, (this_, point): (Value, String)| {
            let id = this(&this_)?;
            let mut st = state(lua);
            let upper = point.to_ascii_uppercase();
            st.frames[id].points.retain(|p| p.point != upper);
            layout::invalidate(&mut st);
            Ok(())
        }
    );
    method!(lua, m, "GetNumPoints", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.points.len()
    ));
    method!(lua, m, "GetPoint", |lua,
                                 (this_, index): (
        Value,
        Option<usize>
    )| {
        let id = this(&this_)?;
        let point = {
            let st = state(lua);
            index
                .unwrap_or(1)
                .checked_sub(1)
                .and_then(|i| st.frames[id].points.get(i))
                .cloned()
        };
        match point {
            Some(p) => {
                let parent = state(lua).frames[id].parent;
                let rel = table_of(lua, p.relative.or(parent));
                (p.point, rel, p.relative_point, p.x, p.y).into_lua_multi(lua)
            }
            None => Ok(MultiValue::new()),
        }
    });
    method!(lua, m, "GetPointByName", |lua,
                                       (this_, name): (
        Value,
        String
    )| {
        let id = this(&this_)?;
        let upper = name.to_ascii_uppercase();
        let point = state(lua).frames[id]
            .points
            .iter()
            .find(|p| p.point == upper)
            .cloned();
        match point {
            Some(p) => {
                let parent = state(lua).frames[id].parent;
                let rel = table_of(lua, p.relative.or(parent));
                (p.point, rel, p.relative_point, p.x, p.y).into_lua_multi(lua)
            }
            None => Ok(MultiValue::new()),
        }
    });
    method!(lua, m, "AdjustPointsOffset", |lua,
                                           (this_, x, y): (
        Value,
        f64,
        f64
    )| {
        let id = this(&this_)?;
        let mut st = state(lua);
        for p in st.frames[id].points.iter_mut() {
            p.x += x;
            p.y += y;
        }
        layout::invalidate(&mut st);
        Ok(())
    });
    method!(lua, m, "ClearPointsOffset", |lua, this_: Value| {
        let id = this(&this_)?;
        let mut st = state(lua);
        for p in st.frames[id].points.iter_mut() {
            p.x = 0.0;
            p.y = 0.0;
        }
        layout::invalidate(&mut st);
        Ok(())
    });
    method!(lua, m, "GetLeft", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(own_rect(lua, id).map(|r| r.0))
    });
    method!(lua, m, "GetRight", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(own_rect(lua, id).map(|r| r.0 + r.2))
    });
    method!(lua, m, "GetTop", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(own_rect(lua, id).map(|r| r.1 + r.3))
    });
    method!(lua, m, "GetBottom", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(own_rect(lua, id).map(|r| r.1))
    });
    method!(lua, m, "GetCenter", |lua, this_: Value| {
        let id = this(&this_)?;
        match own_rect(lua, id) {
            Some(r) => (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0).into_lua_multi(lua),
            None => Ok(MultiValue::new()),
        }
    });
    method!(lua, m, "GetRect", |lua, this_: Value| {
        let id = this(&this_)?;
        match own_rect(lua, id) {
            Some(r) => r.into_lua_multi(lua),
            None => Ok(MultiValue::new()),
        }
    });
    method!(lua, m, "GetScaledRect", |lua, this_: Value| {
        let id = this(&this_)?;
        let r = layout::rect(&mut state(lua), id);
        match r {
            Some(r) => (r.left, r.bottom, r.width, r.height).into_lua_multi(lua),
            None => Ok(MultiValue::new()),
        }
    });
    method!(lua, m, "IsRectValid", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(layout::rect(&mut state(lua), id).is_some())
    });
    method!(lua, m, "IsAnchoringRestricted", |_, _this: Value| Ok(false));
    method!(lua, m, "IsAnchoringSecret", |_, _this: Value| Ok(false));
    empty!(lua, m, "IsCollapsed", |_, _this: Value| Ok(false));
    method!(lua, m, "GetSourceLocation", |_, _this: Value| Ok(
        Value::Nil
    ));

    // Alpha and scale
    method!(lua, m, "SetAlpha", |lua, (this_, a): (Value, Value)| {
        with(lua, &this_, |f| f.alpha = num(Some(&a)).clamp(0.0, 1.0))
    });
    method!(lua, m, "GetAlpha", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.alpha
    ));
    method!(lua, m, "GetEffectiveAlpha", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(layout::effective_alpha(&state(lua), id))
    });
    method!(lua, m, "SetScale", |lua, (this_, s): (Value, f64)| {
        let id = this(&this_)?;
        if s <= 0.0 {
            return Err(mlua::Error::runtime("Scale must be > 0"));
        }
        let mut st = state(lua);
        st.frames[id].scale = s;
        layout::invalidate(&mut st);
        Ok(())
    });
    method!(lua, m, "GetScale", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.scale
    ));
    method!(lua, m, "GetEffectiveScale", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(effective_scale(&state(lua), id))
    });
    method!(lua, m, "SetIgnoreParentAlpha", |lua,
                                             (this_, v): (
        Value,
        Value
    )| {
        with(lua, &this_, |f| f.ignore_parent_alpha = truthy(&v))
    });
    method!(lua, m, "IsIgnoringParentAlpha", |lua, this_: Value| {
        with(lua, &this_, |f| f.ignore_parent_alpha)
    });
    method!(lua, m, "SetIgnoreParentScale", |lua,
                                             (this_, v): (
        Value,
        Value
    )| {
        let id = this(&this_)?;
        let mut st = state(lua);
        st.frames[id].ignore_parent_scale = truthy(&v);
        layout::invalidate(&mut st);
        Ok(())
    });
    method!(lua, m, "IsIgnoringParentScale", |lua, this_: Value| {
        with(lua, &this_, |f| f.ignore_parent_scale)
    });

    // Mouse
    method!(
        lua,
        m,
        "EnableMouse",
        |lua, (this_, enable): (Value, Value)| {
            with(lua, &this_, |f| {
                f.mouse_enabled = truthy(&enable);
                f.mouse_motion = f.mouse_enabled;
            })
        }
    );
    method!(lua, m, "IsMouseEnabled", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.mouse_enabled
    ));
    method!(lua, m, "SetMouseClickEnabled", |lua,
                                             (this_, enable): (
        Value,
        Value
    )| {
        with(lua, &this_, |f| f.mouse_enabled = truthy(&enable))
    });
    method!(lua, m, "IsMouseClickEnabled", |lua, this_: Value| {
        with(lua, &this_, |f| f.mouse_enabled)
    });
    method!(lua, m, "SetMouseMotionEnabled", |lua,
                                              (this_, enable): (
        Value,
        Value
    )| {
        with(lua, &this_, |f| f.mouse_motion = truthy(&enable))
    });
    method!(lua, m, "EnableMouseMotion", |lua,
                                          (this_, enable): (
        Value,
        Value
    )| {
        with(lua, &this_, |f| f.mouse_motion = truthy(&enable))
    });
    method!(lua, m, "IsMouseMotionEnabled", |lua, this_: Value| {
        with(lua, &this_, |f| f.mouse_motion)
    });
    method!(lua, m, "IsMouseOver", |lua, args: Variadic<Value>| {
        let id = this(args.first().unwrap_or(&Value::Nil))?;
        let mut st = state(lua);
        let (mx, my) = st.mouse;
        let Some(r) = layout::rect(&mut st, id) else {
            return Ok(false);
        };
        let es = effective_scale(&st, id);
        let (top, bottom, left, right) = (
            num(args.get(1)) * es,
            num(args.get(2)) * es,
            num(args.get(3)) * es,
            num(args.get(4)) * es,
        );
        Ok(mx >= r.left + left
            && mx <= r.right() + right
            && my >= r.bottom + bottom
            && my <= r.top() + top)
    });
    method!(lua, m, "IsMouseMotionFocus", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(state(lua).mouse_focus == Some(id))
    });

    // Animations
    method!(
        lua,
        m,
        "CreateAnimationGroup",
        |lua, (this_, name, template): (Value, Option<String>, Option<String>)| {
            let id = this(&this_)?;
            crate::xml::create_child(lua, "AnimationGroup", name, id, template.as_deref(), None)
        }
    );
    method!(lua, m, "GetAnimationGroups", |lua, this_: Value| {
        let id = this(&this_)?;
        let st = state(lua);
        let groups: Variadic<Table> = st.frames[id].anims.iter().map(|&a| st.table(a)).collect();
        Ok(groups)
    });
    method!(lua, m, "StopAnimating", |lua, this_: Value| {
        let id = this(&this_)?;
        let groups = state(lua).frames[id].anims.clone();
        for g in groups {
            stop_group(lua, g);
        }
        Ok(())
    });
    Ok(m)
}

/// Fire `OnSizeChanged` for frames whose size changed since the last call,
/// as the client does after layout, not inside `SetSize`. Handlers may
/// change more sizes; a few passes settle that.
/// Horizontal and vertical scroll range of a scroll frame.
fn scroll_ranges(st: &mut State, id: FrameId) -> Option<(f64, f64)> {
    let child = st.frames[id].scroll_child?;
    layout::rect(st, child)?;
    let (cw, ch) = layout::size(st, child);
    let (w, h) = layout::size(st, id);
    Some(((cw - w).max(0.0), (ch - h).max(0.0)))
}

/// Fire OnScrollRangeChanged for scroll frames whose range changed since
/// the last report (all of them, or only `only`).
pub fn fire_scroll_range_changes(lua: &Lua, only: Option<FrameId>) {
    let changed: Vec<(FrameId, (f64, f64))> = {
        let mut st = state(lua);
        let ids: Vec<FrameId> = match only {
            Some(id) => vec![id],
            None => (0..st.frames.len())
                .filter(|&i| {
                    st.frames[i].scroll_child.is_some()
                        && st.frames[i].has_script("OnScrollRangeChanged")
                })
                .collect(),
        };
        let mut out = Vec::new();
        for id in ids {
            let Some(r) = scroll_ranges(&mut st, id) else {
                continue;
            };
            let last = st.last_scroll_ranges.insert(id, r);
            let differs = match last {
                Some((x, y)) => (x - r.0).abs() > 1e-3 || (y - r.1).abs() > 1e-3,
                None => true,
            };
            if differs {
                out.push((id, r));
            }
        }
        out
    };
    for (id, (x, y)) in changed {
        fire_script(lua, id, "OnScrollRangeChanged", (x, y));
    }
}

pub fn fire_size_changes(lua: &Lua) {
    for _ in 0..8 {
        let candidates: Vec<FrameId> = {
            let st = state(lua);
            (0..st.frames.len())
                .filter(|&i| st.frames[i].has_script("OnSizeChanged"))
                .collect()
        };
        let mut changed = Vec::new();
        {
            let mut st = state(lua);
            for id in candidates {
                // Only frames with a resolved rectangle have a size to report.
                if layout::rect(&mut st, id).is_none() {
                    continue;
                }
                let size = layout::size(&mut st, id);
                let last = st.last_sizes.insert(id, size);
                let differs = match last {
                    Some((w, h)) => (w - size.0).abs() > 1e-3 || (h - size.1).abs() > 1e-3,
                    None => size.0 > 0.0 || size.1 > 0.0,
                };
                if differs {
                    changed.push((id, size));
                }
            }
        }
        if changed.is_empty() {
            return;
        }
        for (id, (w, h)) in changed {
            // An earlier handler may have cleared this frame's anchors.
            let has_rect = layout::rect(&mut state(lua), id).is_some();
            if !has_rect {
                state(lua).last_sizes.remove(&id);
                continue;
            }
            fire_script(lua, id, "OnSizeChanged", (w, h));
        }
    }
}

/// Make sure a frame and its descendants are at least at `level`.
fn raise_levels(frames: &mut [FrameData], id: FrameId, level: i64) {
    frames[id].level = level;
    let children = frames[id].children.clone();
    for c in children {
        if frames[c].level < level + 1 {
            raise_levels(frames, c, level + 1);
        }
    }
}

fn set_strata_tree(frames: &mut [FrameData], id: FrameId, strata: &'static str) {
    frames[id].strata = strata;
    let children = frames[id].children.clone();
    for c in children {
        if !frames[c].strata_explicit {
            set_strata_tree(frames, c, strata);
        }
    }
}

fn region_methods(lua: &Lua) -> Result<Table> {
    let m = lua.create_table()?;
    method!(lua, m, "SetDrawLayer", |lua,
                                     (this_, layer, sub): (
        Value,
        String,
        Option<i64>
    )| {
        with(lua, &this_, |f| {
            if let Some(l) = layer_index(&layer) {
                f.draw_layer = l;
            }
            if let Some(s) = sub {
                f.sublevel = s.clamp(-8, 7);
            }
        })
    });
    method!(lua, m, "GetDrawLayer", |lua, this_: Value| {
        with(lua, &this_, |f| (LAYERS[f.draw_layer as usize], f.sublevel))
    });
    method!(lua, m, "SetVertexColor", |lua, args: Variadic<Value>| {
        let id = this(args.first().unwrap_or(&Value::Nil))?;
        let c = color_args(&args[1..]);
        let mut st = state(lua);
        let f = &mut st.frames[id];
        if let Some(t) = f.tex.as_deref_mut() {
            t.vertex_color = c;
        } else if let Some(font) = f.font.as_deref_mut() {
            font.color = Some(c);
        }
        Ok(())
    });
    method!(lua, m, "GetVertexColor", |lua, this_: Value| {
        with(lua, &this_, |f| {
            let c = f.tex.as_deref().map(|t| t.vertex_color).unwrap_or([1.0; 4]);
            (c[0], c[1], c[2], c[3])
        })
    });
    method!(lua, m, "IsObjectLoaded", |_, _this: Value| Ok(true));
    Ok(m)
}

fn set_texture_file(lua: &Lua, id: FrameId, file: TexFile) {
    let mut st = state(lua);
    let tex = crate::frames::ensure_tex(&mut st.frames[id]);
    tex.atlas = None;
    tex.atlas_rect = None;
    tex.file = file;
}

pub fn tex_file_arg(v: &Value) -> TexFile {
    match v {
        Value::Integer(i) => TexFile::Id(*i),
        Value::Number(n) => TexFile::Id(*n as i64),
        Value::String(s) => {
            let s = s.to_string_lossy();
            if s.is_empty() {
                TexFile::None
            } else if let Ok(id) = s.trim().parse::<i64>() {
                TexFile::Id(id)
            } else {
                TexFile::Path(s)
            }
        }
        _ => TexFile::None,
    }
}

/// Apply an atlas to a texture. Returns false if the atlas is unknown.
pub fn set_atlas(lua: &Lua, id: FrameId, atlas: &str, use_size: bool) -> bool {
    let gd = state(lua).gamedata.clone();
    let info = gd.as_ref().and_then(|g| g.atlas(atlas));
    let mut st = state(lua);
    let f = &mut st.frames[id];
    let tex = crate::frames::ensure_tex(f);
    match info {
        Some(a) => {
            tex.file = TexFile::Id(a.file as i64);
            tex.atlas = Some(a.name.clone());
            // Texture coordinates stay as they are, relative to the atlas.
            tex.atlas_rect = Some([a.left, a.top, a.right, a.bottom]);
            tex.horiz_tile = a.tiles_horizontally;
            tex.vert_tile = a.tiles_vertically;
            if use_size {
                f.width = a.width;
                f.height = a.height;
            }
            layout::invalidate(&mut st);
            true
        }
        None => {
            // Keep the name so GetAtlas works without game data.
            tex.atlas = Some(atlas.to_string());
            tex.atlas_rect = None;
            tex.file = TexFile::None;
            false
        }
    }
}

fn texture_methods(lua: &Lua) -> Result<Table> {
    let m = lua.create_table()?;
    method!(
        lua,
        m,
        "SetTexture",
        |lua, (this_, file): (Value, Value)| {
            let id = this(&this_)?;
            set_texture_file(lua, id, tex_file_arg(&file));
            Ok(true)
        }
    );
    method!(
        lua,
        m,
        "SetMask",
        |_, (_this, _file): (Value, Value)| Ok(())
    );
    method!(lua, m, "GetTexture", |lua, this_: Value| {
        let id = this(&this_)?;
        let file = state(lua).frames[id].tex.as_deref().map(|t| t.file.clone());
        let gd = state(lua).gamedata.clone();
        Ok(match file {
            Some(TexFile::Id(i)) => Value::Integer(i),
            Some(TexFile::Path(p)) => match gd.and_then(|g| g.file_id(&p)) {
                Some(i) => Value::Integer(i as i64),
                None => Value::String(lua.create_string(&p)?),
            },
            Some(TexFile::Color(_)) => Value::String(lua.create_string("FileData ID 0")?),
            _ => Value::Nil,
        })
    });
    method!(lua, m, "GetTextureFileID", |lua, this_: Value| {
        let id = this(&this_)?;
        let file = state(lua).frames[id].tex.as_deref().map(|t| t.file.clone());
        let gd = state(lua).gamedata.clone();
        Ok(match file {
            Some(TexFile::Id(i)) => Some(i),
            Some(TexFile::Path(p)) => gd.and_then(|g| g.file_id(&p)).map(|i| i as i64),
            _ => None,
        })
    });
    method!(lua, m, "GetTextureFilePath", |lua, this_: Value| {
        with(lua, &this_, |f| match f.tex.as_deref().map(|t| &t.file) {
            Some(TexFile::Path(p)) => Some(p.clone()),
            Some(TexFile::Id(i)) => Some(i.to_string()),
            _ => None,
        })
    });
    method!(lua, m, "SetAtlas", |lua, args: Variadic<Value>| {
        let id = this(args.first().unwrap_or(&Value::Nil))?;
        let atlas = match args.get(1) {
            Some(Value::String(s)) => s.to_string_lossy(),
            _ => {
                set_texture_file(lua, id, TexFile::None);
                return Ok(false);
            }
        };
        let use_size = args.get(2).is_some_and(truthy);
        Ok(set_atlas(lua, id, &atlas, use_size))
    });
    method!(lua, m, "GetAtlas", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.tex.as_deref().and_then(|t| t.atlas.clone())
        })
    });
    method!(lua, m, "SetColorTexture", |lua, args: Variadic<Value>| {
        let id = this(args.first().unwrap_or(&Value::Nil))?;
        set_texture_file(lua, id, TexFile::Color(color_args(&args[1..])));
        Ok(())
    });
    method!(lua, m, "SetTexCoord", |lua, args: Variadic<Value>| {
        let id = this(args.first().unwrap_or(&Value::Nil))?;
        let v: Vec<f32> = args[1..].iter().map(|a| num(Some(a)) as f32).collect();
        let coords = if v.len() >= 8 {
            [v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7]]
        } else if v.len() >= 4 {
            let (l, r, t, b) = (v[0], v[1], v[2], v[3]);
            [l, t, l, b, r, t, r, b]
        } else {
            return Err(mlua::Error::runtime(
                "Usage: SetTexCoord(left, right, top, bottom)",
            ));
        };
        let mut st = state(lua);
        crate::frames::ensure_tex(&mut st.frames[id]).coords = coords;
        Ok(())
    });
    method!(lua, m, "ResetTexCoord", |lua, this_: Value| {
        with(lua, &this_, |f| {
            crate::frames::ensure_tex(f).coords = crate::state::TexState::default().coords
        })
    });
    method!(lua, m, "GetTexCoord", |lua, this_: Value| {
        let c = with(lua, &this_, |f| {
            f.tex.as_deref().map(|t| t.coords).unwrap_or_default()
        })?;
        c.into_iter().collect::<Variadic<f32>>().into_lua_multi(lua)
    });
    method!(
        lua,
        m,
        "SetDesaturated",
        |lua, (this_, v): (Value, Value)| {
            with(lua, &this_, |f| {
                crate::frames::ensure_tex(f).desaturation = if truthy(&v) { 1.0 } else { 0.0 }
            })
        }
    );
    method!(lua, m, "IsDesaturated", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.tex.as_deref().is_some_and(|t| t.desaturation > 0.0)
        })
    });
    method!(
        lua,
        m,
        "SetDesaturation",
        |lua, (this_, v): (Value, f64)| {
            with(lua, &this_, |f| {
                crate::frames::ensure_tex(f).desaturation = v as f32
            })
        }
    );
    method!(lua, m, "GetDesaturation", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.tex.as_deref().map(|t| t.desaturation).unwrap_or(0.0)
        })
    });
    method!(
        lua,
        m,
        "SetBlendMode",
        |lua, (this_, mode): (Value, String)| {
            with(lua, &this_, |f| {
                crate::frames::ensure_tex(f).blend_mode = mode.to_ascii_uppercase()
            })
        }
    );
    method!(lua, m, "GetBlendMode", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.tex
                .as_deref()
                .map(|t| t.blend_mode.clone())
                .unwrap_or_default()
        })
    });
    method!(lua, m, "SetHorizTile", |lua, (this_, v): (Value, Value)| {
        with(lua, &this_, |f| {
            crate::frames::ensure_tex(f).horiz_tile = truthy(&v)
        })
    });
    method!(lua, m, "GetHorizTile", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.tex.as_deref().is_some_and(|t| t.horiz_tile)
        })
    });
    method!(lua, m, "SetVertTile", |lua, (this_, v): (Value, Value)| {
        with(lua, &this_, |f| {
            crate::frames::ensure_tex(f).vert_tile = truthy(&v)
        })
    });
    method!(lua, m, "GetVertTile", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.tex.as_deref().is_some_and(|t| t.vert_tile)
        })
    });
    method!(lua, m, "SetRotation", |lua, (this_, r): (Value, f64)| {
        with(lua, &this_, |f| {
            crate::frames::ensure_tex(f).rotation = r as f32
        })
    });
    method!(lua, m, "GetRotation", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.tex.as_deref().map(|t| t.rotation).unwrap_or(0.0)
        })
    });
    method!(lua, m, "SetGradient", |lua,
                                    (this_, orientation, a, b): (
        Value,
        String,
        Value,
        Value
    )| {
        let (ca, cb) = (color_table(&a), color_table(&b));
        with(lua, &this_, |f| {
            crate::frames::ensure_tex(f).gradient =
                Some((ca, cb, orientation.eq_ignore_ascii_case("HORIZONTAL")))
        })
    });
    method!(
        lua,
        m,
        "AddMaskTexture",
        |lua, (this_, mask): (Value, Value)| {
            let id = this(&this_)?;
            let mask = this(&mask)?;
            let mut st = state(lua);
            if !st.frames[id].masks.contains(&mask) {
                st.frames[id].masks.push(mask);
            }
            Ok(())
        }
    );
    method!(lua, m, "RemoveMaskTexture", |lua,
                                          (this_, mask): (
        Value,
        Value
    )| {
        let id = this(&this_)?;
        let mask = this(&mask)?;
        state(lua).frames[id].masks.retain(|&m| m != mask);
        Ok(())
    });
    method!(lua, m, "GetNumMaskTextures", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.masks.len()
    ));
    method!(
        lua,
        m,
        "GetMaskTexture",
        |lua, (this_, i): (Value, usize)| {
            let id = this(&this_)?;
            let mask = state(lua).frames[id].masks.get(i.wrapping_sub(1)).copied();
            Ok(table_of(lua, mask))
        }
    );
    Ok(m)
}

/// Font attributes of a widget, created if missing.
fn font_attrs(f: &mut FrameData) -> &mut crate::state::FontAttrs {
    f.font.get_or_insert_with(Box::default)
}

fn font_instance_methods(lua: &Lua) -> Result<Table> {
    let m = lua.create_table()?;
    method!(lua, m, "SetFont", |lua,
                                (this_, path, height, flags): (
        Value,
        String,
        f64,
        Option<String>
    )| {
        let id = this(&this_)?;
        let mut st = state(lua);
        let f = font_attrs(&mut st.frames[id]);
        f.path = Some(path);
        f.height = Some(height);
        f.flags = Some(flags.unwrap_or_default());
        layout::invalidate(&mut st);
        Ok(true)
    });
    method!(lua, m, "GetFont", |lua, this_: Value| {
        let id = this(&this_)?;
        let st = state(lua);
        let f = text::resolve_font(&st, id);
        Ok((f.path, f.height, f.flags))
    });
    method!(lua, m, "SetFontHeight", |lua, (this_, h): (Value, f64)| {
        let id = this(&this_)?;
        let mut st = state(lua);
        font_attrs(&mut st.frames[id]).height = Some(h);
        layout::invalidate(&mut st);
        Ok(())
    });
    method!(lua, m, "GetFontHeight", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(text::resolve_font(&state(lua), id).height)
    });
    method!(
        lua,
        m,
        "SetFontObject",
        |lua, (this_, obj): (Value, Value)| {
            let id = this(&this_)?;
            let obj = match &obj {
                Value::String(s) => match lua.globals().get::<Value>(s.clone())? {
                    Value::Table(t) => frame_id(&t).ok(),
                    _ => {
                        return Err(mlua::Error::runtime(format!(
                            "SetFontObject: font '{}' does not exist",
                            s.to_string_lossy()
                        )));
                    }
                },
                Value::Table(t) => frame_id(t).ok(),
                _ => None,
            };
            let mut st = state(lua);
            if obj == Some(id) {
                return Ok(());
            }
            let f = font_attrs(&mut st.frames[id]);
            *f = crate::state::FontAttrs {
                font_object: obj,
                ..Default::default()
            };
            layout::invalidate(&mut st);
            Ok(())
        }
    );
    method!(lua, m, "GetFontObject", |lua, this_: Value| {
        let id = this(&this_)?;
        let obj = state(lua).frames[id]
            .font
            .as_deref()
            .and_then(|f| f.font_object);
        Ok(table_of(lua, obj))
    });
    method!(
        lua,
        m,
        "CopyFontObject",
        |lua, (this_, src): (Value, Value)| {
            let id = this(&this_)?;
            let src = match &src {
                Value::String(s) => match lua.globals().get::<Value>(s.clone())? {
                    Value::Table(t) => frame_id(&t).ok(),
                    _ => None,
                },
                Value::Table(t) => frame_id(t).ok(),
                _ => None,
            };
            let Some(src) = src else { return Ok(()) };
            let mut st = state(lua);
            let r = text::resolve_font(&st, src);
            let f = font_attrs(&mut st.frames[id]);
            f.path = Some(r.path);
            f.height = Some(r.height);
            f.flags = Some(r.flags);
            f.color = Some(r.color);
            f.shadow_color = Some(r.shadow_color);
            f.shadow_offset = Some(r.shadow_offset);
            f.justify_h = Some(r.justify_h);
            f.justify_v = Some(r.justify_v);
            f.spacing = Some(r.spacing);
            layout::invalidate(&mut st);
            Ok(())
        }
    );
    method!(lua, m, "SetTextColor", |lua, args: Variadic<Value>| {
        let id = this(args.first().unwrap_or(&Value::Nil))?;
        let c = color_args(&args[1..]);
        with(lua, &args[0], |f| font_attrs(f).color = Some(c))?;
        let _ = id;
        Ok(())
    });
    method!(lua, m, "GetTextColor", |lua, this_: Value| {
        let id = this(&this_)?;
        let c = text::resolve_font(&state(lua), id).color;
        Ok((c[0], c[1], c[2], c[3]))
    });
    method!(lua, m, "SetShadowColor", |lua, args: Variadic<Value>| {
        let c = color_args(&args[1..]);
        with(lua, args.first().unwrap_or(&Value::Nil), |f| {
            font_attrs(f).shadow_color = Some(c)
        })
    });
    method!(lua, m, "GetShadowColor", |lua, this_: Value| {
        let id = this(&this_)?;
        let c = text::resolve_font(&state(lua), id).shadow_color;
        Ok((c[0], c[1], c[2], c[3]))
    });
    method!(lua, m, "SetShadowOffset", |lua,
                                        (this_, x, y): (
        Value,
        f64,
        f64
    )| {
        with(lua, &this_, |f| font_attrs(f).shadow_offset = Some((x, y)))
    });
    method!(lua, m, "GetShadowOffset", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(text::resolve_font(&state(lua), id).shadow_offset)
    });
    method!(lua, m, "SetJustifyH", |lua, (this_, j): (Value, String)| {
        with(lua, &this_, |f| {
            font_attrs(f).justify_h = Some(j.to_ascii_uppercase())
        })
    });
    method!(lua, m, "GetJustifyH", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(text::resolve_font(&state(lua), id).justify_h)
    });
    method!(lua, m, "SetJustifyV", |lua, (this_, j): (Value, String)| {
        with(lua, &this_, |f| {
            font_attrs(f).justify_v = Some(j.to_ascii_uppercase())
        })
    });
    method!(lua, m, "GetJustifyV", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(text::resolve_font(&state(lua), id).justify_v)
    });
    method!(lua, m, "SetSpacing", |lua, (this_, s): (Value, f64)| {
        let id = this(&this_)?;
        let mut st = state(lua);
        font_attrs(&mut st.frames[id]).spacing = Some(s);
        layout::invalidate(&mut st);
        Ok(())
    });
    method!(lua, m, "GetSpacing", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(text::resolve_font(&state(lua), id).spacing)
    });
    method!(lua, m, "SetAlpha", |lua, (this_, a): (Value, f64)| {
        with(lua, &this_, |f| f.alpha = a.clamp(0.0, 1.0))
    });
    method!(lua, m, "GetAlpha", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.alpha
    ));
    empty!(
        lua,
        m,
        "SetIndentedWordWrap",
        |_, _args: Variadic<Value>| Ok(())
    );
    empty!(lua, m, "GetIndentedWordWrap", |_, _this: Value| Ok(false));
    method!(lua, m, "GetFontObjectForAlphabet", |_, this_: Value| Ok(
        this_
    ));
    Ok(m)
}

pub fn lua_tostring(lua: &Lua, v: &Value) -> Result<String> {
    match v {
        Value::Nil => Ok(String::new()),
        Value::String(s) => Ok(s.to_string_lossy()),
        other => {
            let ts: Function = lua.globals().get("tostring")?;
            ts.call::<String>(other.clone())
        }
    }
}

pub fn set_text(lua: &Lua, id: FrameId, text: String) {
    let mut st = state(lua);
    let f = &mut st.frames[id];
    f.text.get_or_insert_with(Box::default).text = text;
    layout::invalidate(&mut st);
}

fn format_args(lua: &Lua, args: &[Value]) -> Result<String> {
    let format: Function = lua.globals().get::<Table>("string")?.get("format")?;
    format.call::<String>(args.iter().cloned().collect::<MultiValue>())
}

/// Wrap width of a font string in its own units, if it has one.
fn wrap_width(st: &mut State, id: FrameId) -> Option<f64> {
    let f = &st.frames[id];
    let two = {
        let mut hs = f.points.iter().map(|p| match p.point {
            "TOPLEFT" | "LEFT" | "BOTTOMLEFT" => 0,
            "TOP" | "CENTER" | "BOTTOM" => 1,
            _ => 2,
        });
        match hs.next() {
            Some(first) => hs.any(|h| h != first),
            None => false,
        }
    };
    if f.width > 0.0 && !two {
        return Some(f.width);
    }
    if two {
        let r = layout::rect(st, id)?;
        return Some(r.width / effective_scale(st, id));
    }
    None
}

fn font_string_methods(lua: &Lua) -> Result<Table> {
    let m = lua.create_table()?;
    method!(lua, m, "SetText", |lua, (this_, t): (Value, Value)| {
        let id = this(&this_)?;
        let s = lua_tostring(lua, &t)?;
        set_text(lua, id, s);
        Ok(())
    });
    method!(lua, m, "ClearText", |lua, this_: Value| {
        let id = this(&this_)?;
        set_text(lua, id, String::new());
        Ok(())
    });
    method!(lua, m, "SetFormattedText", |lua, args: Variadic<Value>| {
        let id = this(args.first().unwrap_or(&Value::Nil))?;
        let s = format_args(lua, &args[1..])?;
        set_text(lua, id, s);
        Ok(())
    });
    method!(lua, m, "GetText", |lua, this_: Value| {
        with(lua, &this_, |f| f.text.as_deref().map(|t| t.text.clone()))
    });
    method!(lua, m, "SetTextToFit", |lua, (this_, t): (Value, Value)| {
        let id = this(&this_)?;
        let s = lua_tostring(lua, &t)?;
        set_text(lua, id, s);
        let mut st = state(lua);
        let (w, _) = text::string_size(&mut st, id, None);
        st.frames[id].width = w;
        layout::invalidate(&mut st);
        Ok(())
    });
    method!(lua, m, "GetStringWidth", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(text::string_size(&mut state(lua), id, None).0)
    });
    method!(lua, m, "GetUnboundedStringWidth", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(text::string_size(&mut state(lua), id, None).0)
    });
    method!(lua, m, "GetWrappedWidth", |lua, this_: Value| {
        let id = this(&this_)?;
        let mut st = state(lua);
        let w = wrap_width(&mut st, id);
        Ok(text::string_size(&mut st, id, w).0)
    });
    method!(lua, m, "GetStringHeight", |lua, this_: Value| {
        let id = this(&this_)?;
        let mut st = state(lua);
        let w = wrap_width(&mut st, id);
        Ok(text::string_size(&mut st, id, w).1)
    });
    method!(lua, m, "GetNumLines", |lua, this_: Value| {
        let id = this(&this_)?;
        let mut st = state(lua);
        let w = wrap_width(&mut st, id);
        Ok(text::font_string_lines(&mut st, id, w).1.len())
    });
    method!(lua, m, "IsTruncated", |lua, this_: Value| {
        let id = this(&this_)?;
        let mut st = state(lua);
        let w = wrap_width(&mut st, id);
        let (_, lines) = text::font_string_lines(&mut st, id, w);
        Ok(lines
            .last()
            .and_then(|l| l.runs.last())
            .is_some_and(|r| r.text == "..."))
    });
    method!(lua, m, "GetLineHeight", |lua, this_: Value| {
        let id = this(&this_)?;
        // The client reports the height in whole pixels (12 at 1.2 pixels
        // per unit gives 11.67).
        let st = state(lua);
        let ppu = layout::effective_scale(&st, id) * st.pixel_scale;
        Ok(text::snap_height(text::resolve_font(&st, id).height, ppu))
    });
    method!(lua, m, "SetMaxLines", |lua, (this_, n): (Value, i64)| {
        let id = this(&this_)?;
        let mut st = state(lua);
        st.frames[id]
            .text
            .get_or_insert_with(Box::default)
            .max_lines = n;
        layout::invalidate(&mut st);
        Ok(())
    });
    method!(lua, m, "GetMaxLines", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.text.as_deref().map(|t| t.max_lines).unwrap_or(0)
        })
    });
    method!(lua, m, "SetWordWrap", |lua, (this_, v): (Value, Value)| {
        let id = this(&this_)?;
        let mut st = state(lua);
        st.frames[id]
            .text
            .get_or_insert_with(Box::default)
            .word_wrap = truthy(&v);
        layout::invalidate(&mut st);
        Ok(())
    });
    method!(lua, m, "CanWordWrap", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.text.as_deref().is_none_or(|t| t.word_wrap)
        })
    });
    method!(
        lua,
        m,
        "SetNonSpaceWrap",
        |lua, (this_, v): (Value, Value)| {
            let id = this(&this_)?;
            let mut st = state(lua);
            st.frames[id]
                .text
                .get_or_insert_with(Box::default)
                .non_space_wrap = truthy(&v);
            layout::invalidate(&mut st);
            Ok(())
        }
    );
    method!(lua, m, "CanNonSpaceWrap", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.text.as_deref().is_some_and(|t| t.non_space_wrap)
        })
    });
    method!(lua, m, "SetTextScale", |lua, (this_, s): (Value, f64)| {
        with(lua, &this_, |f| {
            f.text.get_or_insert_with(Box::default).text_scale = s
        })
    });
    method!(lua, m, "GetTextScale", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.text.as_deref().map(|t| t.text_scale).unwrap_or(1.0)
        })
    });
    method!(lua, m, "SetTextHeight", |lua, (this_, h): (Value, f64)| {
        let id = this(&this_)?;
        let mut st = state(lua);
        font_attrs(&mut st.frames[id]).height = Some(h);
        layout::invalidate(&mut st);
        Ok(())
    });
    method!(lua, m, "SetAlphaGradient", |lua,
                                         (this_, start, len): (
        Value,
        i64,
        i64
    )| {
        with(lua, &this_, |f| {
            f.text.get_or_insert_with(Box::default).alpha_gradient = Some((start, len))
        })
    });
    method!(lua, m, "ClearAlphaGradient", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.text.get_or_insert_with(Box::default).alpha_gradient = None
        })
    });
    Ok(m)
}

fn frame_methods(lua: &Lua) -> Result<Table> {
    let m = lua.create_table()?;
    method!(lua, m, "GetID", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.id
    ));
    method!(lua, m, "SetID", |lua, (this_, value): (Value, i64)| {
        with(lua, &this_, |f| f.id = value)
    });
    method!(lua, m, "GetChildren", |lua, this_: Value| {
        let id = this(&this_)?;
        let st = state(lua);
        let children: Variadic<Table> = st.frames[id]
            .children
            .iter()
            .map(|&c| st.table(c))
            .collect();
        Ok(children)
    });
    method!(lua, m, "GetNumChildren", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.children.len()
    ));
    method!(lua, m, "GetRegions", |lua, this_: Value| {
        let id = this(&this_)?;
        let st = state(lua);
        let regions: Variadic<Table> = st.frames[id].regions.iter().map(|&c| st.table(c)).collect();
        Ok(regions)
    });
    method!(lua, m, "GetNumRegions", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.regions.len()
    ));
    method!(lua, m, "CreateTexture", |lua, args: Variadic<Value>| {
        let id = this(args.first().unwrap_or(&Value::Nil))?;
        let name = opt_string(args.get(1));
        let layer = opt_string(args.get(2));
        let template = opt_string(args.get(3));
        let sub = args
            .get(4)
            .filter(|v| is_num(Some(v)))
            .map(|v| num(Some(v)) as i64);
        let layer = layer
            .as_deref()
            .and_then(layer_index)
            .map(|l| (l, sub.unwrap_or(0)));
        crate::xml::create_child(lua, "Texture", name, id, template.as_deref(), layer)
    });
    method!(lua, m, "CreateMaskTexture", |lua, args: Variadic<Value>| {
        let id = this(args.first().unwrap_or(&Value::Nil))?;
        let name = opt_string(args.get(1));
        let layer = opt_string(args.get(2))
            .as_deref()
            .and_then(layer_index)
            .map(|l| (l, 0));
        let template = opt_string(args.get(3));
        crate::xml::create_child(lua, "MaskTexture", name, id, template.as_deref(), layer)
    });
    method!(lua, m, "CreateLine", |lua, args: Variadic<Value>| {
        let id = this(args.first().unwrap_or(&Value::Nil))?;
        let name = opt_string(args.get(1));
        let layer = opt_string(args.get(2))
            .as_deref()
            .and_then(layer_index)
            .map(|l| (l, 0));
        let template = opt_string(args.get(3));
        crate::xml::create_child(lua, "Line", name, id, template.as_deref(), layer)
    });
    method!(lua, m, "CreateFontString", |lua, args: Variadic<Value>| {
        let id = this(args.first().unwrap_or(&Value::Nil))?;
        let name = opt_string(args.get(1));
        let layer = opt_string(args.get(2))
            .as_deref()
            .and_then(layer_index)
            .map(|l| (l, 0));
        let template = opt_string(args.get(3));
        crate::xml::create_child(lua, "FontString", name, id, template.as_deref(), layer)
    });

    // Events
    method!(lua, m, "RegisterEvent", |lua,
                                      (this_, event): (
        Value,
        String
    )| {
        let id = this(&this_)?;
        register_event(lua, id, &event);
        Ok(true)
    });
    method!(lua, m, "RegisterUnitEvent", |lua,
                                          (this_, event, units): (
        Value,
        String,
        Variadic<Value>
    )| {
        let id = this(&this_)?;
        register_event(lua, id, &event);
        let units: Vec<String> = units
            .iter()
            .filter_map(|u| match u {
                Value::String(s) => Some(s.to_string_lossy()),
                _ => None,
            })
            .collect();
        if !units.is_empty() {
            state(lua).frames[id].unit_filters.insert(event, units);
        }
        Ok(true)
    });
    method!(lua, m, "UnregisterEvent", |lua,
                                        (this_, event): (
        Value,
        String
    )| {
        let id = this(&this_)?;
        unregister_event(lua, id, &event);
        Ok(())
    });
    method!(lua, m, "UnregisterAllEvents", |lua, this_: Value| {
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
    method!(lua, m, "RegisterAllEvents", |lua, this_: Value| {
        let id = this(&this_)?;
        let mut st = state(lua);
        st.frames[id].all_events = true;
        if !st.all_event_frames.contains(&id) {
            st.all_event_frames.push(id);
        }
        Ok(())
    });
    method!(lua, m, "IsEventRegistered", |lua,
                                          (this_, event): (
        Value,
        String
    )| {
        with(lua, &this_, |f| f.all_events || f.events.contains(&event))
    });

    // Attributes
    method!(lua, m, "SetAttribute", |lua,
                                     (this_, name, value): (
        Value,
        String,
        Value
    )| {
        let id = this(&this_)?;
        let attrs = attributes(lua, id)?;
        attrs.raw_set(name.clone(), value.clone())?;
        fire_script(lua, id, "OnAttributeChanged", (name, value));
        Ok(())
    });
    method!(lua, m, "SetAttributeNoHandler", |lua,
                                              (this_, name, value): (
        Value,
        String,
        Value
    )| {
        let id = this(&this_)?;
        attributes(lua, id)?.raw_set(name, value)?;
        Ok(())
    });
    method!(lua, m, "GetAttribute", |lua,
                                     (this_, a, b, c): (
        Value,
        String,
        Option<String>,
        Option<String>
    )| {
        let id = this(&this_)?;
        let attrs = state(lua).frames[id].attributes.clone();
        let Some(t) = attrs else {
            return Ok(Value::Nil);
        };
        // GetAttribute(prefix, name, suffix) tries the exact key, then
        // wildcards for the prefix and the suffix, then the bare name.
        let (Some(name), Some(suffix)) = (b, c) else {
            return t.raw_get::<Value>(a);
        };
        let prefix = a;
        for key in [
            format!("{prefix}{name}{suffix}"),
            format!("*{name}{suffix}"),
            format!("{prefix}{name}*"),
            format!("*{name}*"),
            name.clone(),
        ] {
            let v = t.raw_get::<Value>(key)?;
            if !v.is_nil() {
                return Ok(v);
            }
        }
        Ok(Value::Nil)
    });

    // Stacking
    method!(lua, m, "SetFrameStrata", |lua,
                                       (this_, strata): (
        Value,
        String
    )| {
        let id = this(&this_)?;
        if let Some(s) = strata_name(&strata) {
            let mut st = state(lua);
            st.frames[id].strata_explicit = true;
            set_strata_tree(&mut st.frames, id, s);
        }
        Ok(())
    });
    method!(lua, m, "GetFrameStrata", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.strata
    ));
    method!(
        lua,
        m,
        "SetFrameLevel",
        |lua, (this_, level): (Value, i64)| {
            let id = this(&this_)?;
            let mut st = state(lua);
            raise_levels(&mut st.frames, id, level.max(0));
            Ok(())
        }
    );
    method!(lua, m, "GetFrameLevel", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.level
    ));
    method!(lua, m, "SetToplevel", |lua, (this_, v): (Value, Value)| {
        with(lua, &this_, |f| f.toplevel = truthy(&v))
    });
    method!(lua, m, "IsToplevel", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.toplevel
    ));
    method!(
        lua,
        m,
        "SetClipsChildren",
        |lua, (this_, v): (Value, Value)| { with(lua, &this_, |f| f.clips_children = truthy(&v)) }
    );
    method!(lua, m, "DoesClipChildren", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.clips_children
    ));
    method!(lua, m, "SetHitRectInsets", |lua,
                                         (this_, l, r, t, b): (
        Value,
        f64,
        f64,
        f64,
        f64
    )| {
        with(lua, &this_, |f| f.hit_insets = [l, r, t, b])
    });
    method!(lua, m, "GetHitRectInsets", |lua, this_: Value| {
        with(lua, &this_, |f| {
            (
                f.hit_insets[0],
                f.hit_insets[1],
                f.hit_insets[2],
                f.hit_insets[3],
            )
        })
    });
    method!(lua, m, "SetMovable", |lua, (this_, v): (Value, Value)| {
        with(lua, &this_, |f| f.movable = truthy(&v))
    });
    method!(lua, m, "IsMovable", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.movable
    ));
    method!(lua, m, "StartMoving", |lua,
                                    (this_, from_mouse): (
        Value,
        Value
    )| {
        let id = this(&this_)?;
        crate::movement::start(lua, id, None, truthy(&from_mouse))
    });
    method!(lua, m, "StartSizing", |lua,
                                    (this_, point, from_mouse): (
        Value,
        Value,
        Value
    )| {
        let id = this(&this_)?;
        let point = match &point {
            Value::String(s) => point_name(&s.to_string_lossy()).unwrap_or("BOTTOMRIGHT"),
            _ => "BOTTOMRIGHT",
        };
        crate::movement::start(lua, id, Some(point), truthy(&from_mouse))
    });
    method!(lua, m, "StopMovingOrSizing", |lua, this_: Value| {
        let id = this(&this_)?;
        crate::movement::stop(lua, id);
        Ok(())
    });
    method!(lua, m, "RegisterForDrag", |lua, args: Variadic<Value>| {
        let buttons: Vec<String> = args[1..]
            .iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s.to_string_lossy()),
                _ => None,
            })
            .collect();
        with(lua, args.first().unwrap_or(&Value::Nil), |f| {
            f.drag_buttons = buttons
        })
    });
    method!(lua, m, "IsDragging", |lua, this_: Value| {
        let id = this(&this_)?;
        let st = state(lua);
        Ok(st
            .press
            .as_ref()
            .is_some_and(|p| p.dragging && p.frame == id))
    });
    method!(lua, m, "SetClampedToScreen", |lua,
                                           (this_, v): (
        Value,
        Value
    )| {
        let id = this(&this_)?;
        let mut st = state(lua);
        st.frames[id].clamped = truthy(&v);
        layout::invalidate(&mut st);
        Ok(())
    });
    method!(lua, m, "IsClampedToScreen", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.clamped
    ));
    method!(
        lua,
        m,
        "SetClampRectInsets",
        |lua, args: Variadic<Value>| {
            let id = this(args.first().unwrap_or(&Value::Nil))?;
            let mut st = state(lua);
            st.frames[id].clamp_insets = [
                num(args.get(1)),
                num(args.get(2)),
                num(args.get(3)),
                num(args.get(4)),
            ];
            layout::invalidate(&mut st);
            Ok(())
        }
    );
    method!(lua, m, "GetClampRectInsets", |lua, this_: Value| with(
        lua,
        &this_,
        |f| {
            let i = f.clamp_insets;
            (i[0], i[1], i[2], i[3])
        }
    ));
    method!(
        lua,
        m,
        "SetUserPlaced",
        |lua, (this_, v): (Value, Value)| { with(lua, &this_, |f| f.user_placed = truthy(&v)) }
    );
    method!(lua, m, "IsUserPlaced", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.user_placed
    ));
    method!(lua, m, "SetDontSavePosition", |lua,
                                            (this_, v): (
        Value,
        Value
    )| {
        with(lua, &this_, |f| f.dont_save_position = truthy(&v))
    });
    method!(lua, m, "GetDontSavePosition", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.dont_save_position
    ));
    method!(lua, m, "SetResizable", |lua, (this_, v): (Value, Value)| {
        with(lua, &this_, |f| f.resizable = truthy(&v))
    });
    method!(lua, m, "IsResizable", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.resizable
    ));
    method!(lua, m, "SetResizeBounds", |lua, args: Variadic<Value>| {
        with(lua, args.first().unwrap_or(&Value::Nil), |f| {
            f.resize_bounds = [
                num(args.get(1)),
                num(args.get(2)),
                num(args.get(3)),
                num(args.get(4)),
            ]
        })
    });
    method!(lua, m, "GetResizeBounds", |lua, this_: Value| with(
        lua,
        &this_,
        |f| {
            let b = f.resize_bounds;
            (b[0], b[1], b[2], b[3])
        }
    ));
    method!(lua, m, "Raise", |lua, this_: Value| {
        let id = this(&this_)?;
        crate::movement::raise(lua, id);
        Ok(())
    });
    method!(lua, m, "Lower", |lua, this_: Value| {
        let id = this(&this_)?;
        crate::movement::lower(lua, id);
        Ok(())
    });
    method!(
        lua,
        m,
        "EnableKeyboard",
        |lua, (this_, v): (Value, Value)| {
            with(lua, &this_, |f| f.keyboard_enabled = truthy(&v))
        }
    );
    method!(lua, m, "IsKeyboardEnabled", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.keyboard_enabled
    ));
    method!(lua, m, "SetPropagateKeyboardInput", |lua,
                                                  (this_, v): (
        Value,
        Value
    )| {
        with(lua, &this_, |f| f.propagate_keyboard = truthy(&v))
    });
    method!(lua, m, "GetPropagateKeyboardInput", |lua, this_: Value| {
        with(lua, &this_, |f| f.propagate_keyboard)
    });
    method!(lua, m, "SetHyperlinksEnabled", |lua,
                                             (this_, v): (
        Value,
        Value
    )| {
        with(lua, &this_, |f| f.hyperlinks_enabled = truthy(&v))
    });
    method!(lua, m, "GetHyperlinksEnabled", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.hyperlinks_enabled
    ));
    method!(lua, m, "LockHighlight", |lua, this_: Value| {
        with(lua, &this_, |f| f.highlight_locked = true)
    });
    method!(lua, m, "UnlockHighlight", |lua, this_: Value| {
        with(lua, &this_, |f| f.highlight_locked = false)
    });
    method!(lua, m, "SetHighlightLocked", |lua,
                                           (this_, v): (
        Value,
        Value
    )| {
        with(lua, &this_, |f| f.highlight_locked = truthy(&v))
    });
    method!(lua, m, "IsHighlightLocked", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.highlight_locked
    ));
    for name in [
        "SetFixedFrameStrata",
        "SetFixedFrameLevel",
        "SetFlattensRenderLayers",
        "EnableMouseWheel",
        "EnableGamePadButton",
        "EnableGamePadStick",
        "SetDrawLayerEnabled",
        "SetIsFrameBuffer",
        "SetUsingParentLevel",
        "SetWindow",
    ] {
        m.raw_set(name, empty_fn(lua)?)?;
    }
    method!(lua, m, "GetBoundsRect", |lua, this_: Value| {
        let id = this(&this_)?;
        match own_rect(lua, id) {
            Some(r) => r.into_lua_multi(lua),
            None => Ok(MultiValue::new()),
        }
    });
    Ok(m)
}

fn opt_string(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::String(s)) => Some(s.to_string_lossy()).filter(|s| !s.is_empty()),
        _ => None,
    }
}

fn attributes(lua: &Lua, id: FrameId) -> Result<Table> {
    let mut st = state(lua);
    match &st.frames[id].attributes {
        Some(t) => Ok(t.clone()),
        None => {
            let t = lua.create_table()?;
            st.frames[id].attributes = Some(t.clone());
            Ok(t)
        }
    }
}

/// Which texture slot of a button a setter targets.
#[derive(Clone, Copy)]
enum Slot {
    Normal,
    Pushed,
    Disabled,
    Highlight,
    Checked,
    DisabledChecked,
}

fn slot_get(b: &crate::state::ButtonState, s: Slot) -> Option<FrameId> {
    match s {
        Slot::Normal => b.normal,
        Slot::Pushed => b.pushed_tex,
        Slot::Disabled => b.disabled,
        Slot::Highlight => b.highlight,
        Slot::Checked => b.checked_tex,
        Slot::DisabledChecked => b.disabled_checked,
    }
}

fn slot_set(b: &mut crate::state::ButtonState, s: Slot, v: Option<FrameId>) {
    match s {
        Slot::Normal => b.normal = v,
        Slot::Pushed => b.pushed_tex = v,
        Slot::Disabled => b.disabled = v,
        Slot::Highlight => b.highlight = v,
        Slot::Checked => b.checked_tex = v,
        Slot::DisabledChecked => b.disabled_checked = v,
    }
}

/// The texture in a button slot, created on first use like the client does.
pub fn button_slot_texture(lua: &Lua, id: FrameId, slot_name: &str) -> Result<FrameId> {
    let slot = parse_slot(slot_name);
    if let Some(t) = state(lua).frames[id]
        .button
        .as_deref()
        .and_then(|b| slot_get(b, slot))
    {
        return Ok(t);
    }
    let layer = if matches!(slot, Slot::Highlight) {
        4
    } else {
        2
    };
    let table = create_object(lua, "Texture", None, Some(id))?;
    let tex = frame_id(&table)?;
    let mut st = state(lua);
    st.frames[tex].draw_layer = layer;
    if matches!(slot, Slot::Highlight) {
        crate::frames::ensure_tex(&mut st.frames[tex]).blend_mode = "ADD".into();
    }
    st.frames[tex].points = ["TOPLEFT", "BOTTOMRIGHT"]
        .iter()
        .map(|p| Point {
            point: p,
            relative: None,
            relative_point: p,
            x: 0.0,
            y: 0.0,
        })
        .collect();
    if let Some(b) = st.frames[id].button.as_deref_mut() {
        slot_set(b, slot, Some(tex));
    }
    layout::invalidate(&mut st);
    Ok(tex)
}

fn parse_slot(name: &str) -> Slot {
    match name {
        "Pushed" => Slot::Pushed,
        "Disabled" => Slot::Disabled,
        "Highlight" => Slot::Highlight,
        "Checked" => Slot::Checked,
        "DisabledChecked" => Slot::DisabledChecked,
        _ => Slot::Normal,
    }
}

/// Assign a texture (or a file, or an atlas) to a button slot.
fn set_slot(lua: &Lua, id: FrameId, slot_name: &str, value: &Value, atlas: bool) -> Result<()> {
    let slot = parse_slot(slot_name);
    match value {
        Value::Table(t) => {
            let tex = frame_id(t)?;
            let mut st = state(lua);
            if let Some(b) = st.frames[id].button.as_deref_mut() {
                slot_set(b, slot, Some(tex));
            }
            if st.frames[tex].points.is_empty() {
                st.frames[tex].points = ["TOPLEFT", "BOTTOMRIGHT"]
                    .iter()
                    .map(|p| Point {
                        point: p,
                        relative: None,
                        relative_point: p,
                        x: 0.0,
                        y: 0.0,
                    })
                    .collect();
            }
            if matches!(slot, Slot::Highlight) {
                st.frames[tex].draw_layer = 4;
            }
            layout::invalidate(&mut st);
        }
        Value::Nil => {
            let mut st = state(lua);
            if let Some(t) = st.frames[id]
                .button
                .as_deref()
                .and_then(|b| slot_get(b, slot))
            {
                crate::frames::ensure_tex(&mut st.frames[t]).file = TexFile::None;
            }
        }
        v => {
            let tex = button_slot_texture(lua, id, slot_name)?;
            if atlas {
                if let Value::String(s) = v {
                    set_atlas(lua, tex, &s.to_string_lossy(), false);
                }
            } else {
                set_texture_file(lua, tex, tex_file_arg(v));
            }
        }
    }
    Ok(())
}

/// The button's font string, created on first use.
pub fn button_font_string(lua: &Lua, id: FrameId) -> Result<FrameId> {
    if let Some(fs) = state(lua).frames[id]
        .button
        .as_deref()
        .and_then(|b| b.font_string)
    {
        return Ok(fs);
    }
    let table = create_object(lua, "FontString", None, Some(id))?;
    let fs = frame_id(&table)?;
    let mut st = state(lua);
    st.frames[fs].draw_layer = 3;
    st.frames[fs].points = vec![Point {
        point: "CENTER",
        relative: None,
        relative_point: "CENTER",
        x: 0.0,
        y: 0.0,
    }];
    let font = st.frames[id].button.as_deref().and_then(|b| b.normal_font);
    if let Some(f) = st.frames[fs].font.as_deref_mut() {
        f.font_object = font;
    }
    if let Some(b) = st.frames[id].button.as_deref_mut() {
        b.font_string = Some(fs);
    }
    layout::invalidate(&mut st);
    Ok(fs)
}

/// Point the button's font string at the font object for its state.
pub fn update_button_font(lua: &Lua, id: FrameId) {
    let mut st = state(lua);
    let Some(b) = st.frames[id].button.as_deref() else {
        return;
    };
    let Some(fs) = b.font_string else { return };
    let font = if !b.enabled {
        b.disabled_font.or(b.normal_font)
    } else if b.locked_highlight || st.frames[id].mouse_over {
        b.highlight_font.or(b.normal_font)
    } else {
        b.normal_font
    };
    if let Some(font) = font
        && let Some(f) = st.frames[fs].font.as_deref_mut()
        && f.font_object != Some(font)
    {
        f.font_object = Some(font);
        layout::invalidate(&mut st);
    }
}

fn font_object_arg(lua: &Lua, v: &Value) -> Result<Option<FrameId>> {
    Ok(match v {
        Value::String(s) => match lua.globals().get::<Value>(s.clone())? {
            Value::Table(t) => frame_id(&t).ok(),
            _ => None,
        },
        Value::Table(t) => frame_id(t).ok(),
        _ => None,
    })
}

pub fn click(lua: &Lua, id: FrameId, button: &str, down: bool) {
    let enabled = state(lua).frames[id]
        .button
        .as_deref()
        .is_none_or(|b| b.enabled);
    if !enabled {
        return;
    }
    if state(lua).frames[id].object_type == "CheckButton"
        && let Some(b) = state(lua).frames[id].button.as_deref_mut()
    {
        b.checked = !b.checked;
    }
    fire_script(lua, id, "PreClick", (button, down));
    fire_script(lua, id, "OnClick", (button, down));
    fire_script(lua, id, "PostClick", (button, down));
}

fn button_methods(lua: &Lua) -> Result<Table> {
    let m = lua.create_table()?;
    for slot in ["Normal", "Pushed", "Disabled", "Highlight"] {
        let set_name = format!("Set{slot}Texture");
        let set_atlas_name = format!("Set{slot}Atlas");
        let get_name = format!("Get{slot}Texture");
        let clear_name = format!("Clear{slot}Texture");
        m.raw_set(
            set_name,
            lua.create_function(move |lua, (this_, v): (Value, Value)| {
                let id = this(&this_)?;
                set_slot(lua, id, slot, &v, false)
            })?,
        )?;
        m.raw_set(
            set_atlas_name,
            lua.create_function(move |lua, (this_, v): (Value, Value)| {
                let id = this(&this_)?;
                set_slot(lua, id, slot, &v, true)
            })?,
        )?;
        m.raw_set(
            get_name,
            lua.create_function(move |lua, this_: Value| {
                let id = this(&this_)?;
                let t = state(lua).frames[id]
                    .button
                    .as_deref()
                    .and_then(|b| slot_get(b, parse_slot(slot)));
                Ok(table_of(lua, t))
            })?,
        )?;
        m.raw_set(
            clear_name,
            lua.create_function(move |lua, this_: Value| {
                let id = this(&this_)?;
                set_slot(lua, id, slot, &Value::Nil, false)
            })?,
        )?;
    }
    method!(lua, m, "SetText", |lua, (this_, t): (Value, Value)| {
        let id = this(&this_)?;
        let s = lua_tostring(lua, &t)?;
        let fs = button_font_string(lua, id)?;
        set_text(lua, fs, s);
        Ok(())
    });
    method!(lua, m, "SetFormattedText", |lua, args: Variadic<Value>| {
        let id = this(args.first().unwrap_or(&Value::Nil))?;
        let s = format_args(lua, &args[1..])?;
        let fs = button_font_string(lua, id)?;
        set_text(lua, fs, s);
        Ok(())
    });
    method!(lua, m, "GetText", |lua, this_: Value| {
        let id = this(&this_)?;
        let st = state(lua);
        let fs = st.frames[id].button.as_deref().and_then(|b| b.font_string);
        Ok(fs.and_then(|fs| st.frames[fs].text.as_deref().map(|t| t.text.clone())))
    });
    method!(lua, m, "GetFontString", |lua, this_: Value| {
        let id = this(&this_)?;
        let fs = state(lua).frames[id]
            .button
            .as_deref()
            .and_then(|b| b.font_string);
        Ok(table_of(lua, fs))
    });
    method!(
        lua,
        m,
        "SetFontString",
        |lua, (this_, fs): (Value, Value)| {
            let id = this(&this_)?;
            let fs = this(&fs)?;
            if let Some(b) = state(lua).frames[id].button.as_deref_mut() {
                b.font_string = Some(fs);
            }
            update_button_font(lua, id);
            Ok(())
        }
    );
    method!(lua, m, "GetTextWidth", |lua, this_: Value| {
        let id = this(&this_)?;
        let fs = state(lua).frames[id]
            .button
            .as_deref()
            .and_then(|b| b.font_string);
        Ok(match fs {
            Some(fs) => text::string_size(&mut state(lua), fs, None).0,
            None => 0.0,
        })
    });
    method!(lua, m, "GetTextHeight", |lua, this_: Value| {
        let id = this(&this_)?;
        let fs = state(lua).frames[id]
            .button
            .as_deref()
            .and_then(|b| b.font_string);
        Ok(match fs {
            Some(fs) => text::string_size(&mut state(lua), fs, None).1,
            None => 0.0,
        })
    });
    for (name, which) in [
        ("SetNormalFontObject", 0),
        ("SetHighlightFontObject", 1),
        ("SetDisabledFontObject", 2),
    ] {
        m.raw_set(
            name,
            lua.create_function(move |lua, (this_, v): (Value, Value)| {
                let id = this(&this_)?;
                let font = font_object_arg(lua, &v)?;
                if let Some(b) = state(lua).frames[id].button.as_deref_mut() {
                    match which {
                        0 => b.normal_font = font,
                        1 => b.highlight_font = font,
                        _ => b.disabled_font = font,
                    }
                }
                if which == 0 {
                    // The client creates the font string when a font is set.
                    let fs = button_font_string(lua, id)?;
                    if let Some(f) = state(lua).frames[fs].font.as_deref_mut() {
                        f.font_object = font;
                    }
                }
                update_button_font(lua, id);
                Ok(())
            })?,
        )?;
    }
    for (name, which) in [
        ("GetNormalFontObject", 0),
        ("GetHighlightFontObject", 1),
        ("GetDisabledFontObject", 2),
    ] {
        m.raw_set(
            name,
            lua.create_function(move |lua, this_: Value| {
                let id = this(&this_)?;
                let font = state(lua).frames[id]
                    .button
                    .as_deref()
                    .and_then(|b| match which {
                        0 => b.normal_font,
                        1 => b.highlight_font,
                        _ => b.disabled_font,
                    });
                Ok(table_of(lua, font))
            })?,
        )?;
    }
    enable_methods(lua, &m)?;
    method!(lua, m, "Click", |lua,
                              (this_, button, down): (
        Value,
        Option<String>,
        Option<bool>
    )| {
        let id = this(&this_)?;
        click(
            lua,
            id,
            button.as_deref().unwrap_or("LeftButton"),
            down.unwrap_or(false),
        );
        Ok(())
    });
    method!(lua, m, "RegisterForClicks", |lua, args: Variadic<Value>| {
        let clicks: Vec<String> = args[1..]
            .iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s.to_string_lossy()),
                _ => None,
            })
            .collect();
        with(lua, args.first().unwrap_or(&Value::Nil), |f| {
            if let Some(b) = f.button.as_deref_mut() {
                b.clicks = clicks;
            }
        })
    });
    method!(lua, m, "LockHighlight", |lua, this_: Value| {
        let id = this(&this_)?;
        if let Some(b) = state(lua).frames[id].button.as_deref_mut() {
            b.locked_highlight = true;
        }
        update_button_font(lua, id);
        Ok(())
    });
    method!(lua, m, "UnlockHighlight", |lua, this_: Value| {
        let id = this(&this_)?;
        if let Some(b) = state(lua).frames[id].button.as_deref_mut() {
            b.locked_highlight = false;
        }
        update_button_font(lua, id);
        Ok(())
    });
    method!(lua, m, "IsHighlightLocked", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.button.as_deref().is_some_and(|b| b.locked_highlight)
        })
    });
    method!(lua, m, "SetButtonState", |lua,
                                       (this_, s, _lock): (
        Value,
        String,
        Value
    )| {
        with(lua, &this_, |f| {
            if let Some(b) = f.button.as_deref_mut() {
                b.pushed = s.eq_ignore_ascii_case("PUSHED");
            }
        })
    });
    method!(lua, m, "GetButtonState", |lua, this_: Value| {
        with(lua, &this_, |f| {
            if f.button.as_deref().is_some_and(|b| b.pushed) {
                "PUSHED"
            } else if f.button.as_deref().is_some_and(|b| !b.enabled) {
                "DISABLED"
            } else {
                "NORMAL"
            }
        })
    });
    method!(lua, m, "SetPushedTextOffset", |lua,
                                            (this_, x, y): (
        Value,
        f64,
        f64
    )| {
        with(lua, &this_, |f| {
            if let Some(b) = f.button.as_deref_mut() {
                b.pushed_offset = (x, y);
            }
        })
    });
    method!(lua, m, "GetPushedTextOffset", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.button
                .as_deref()
                .map(|b| b.pushed_offset)
                .unwrap_or_default()
        })
    });
    method!(lua, m, "SetMotionScriptsWhileDisabled", |lua,
                                                      (this_, v): (
        Value,
        Value
    )| {
        with(lua, &this_, |f| {
            if let Some(b) = f.button.as_deref_mut() {
                b.motion_scripts_while_disabled = truthy(&v);
            }
        })
    });
    method!(
        lua,
        m,
        "GetMotionScriptsWhileDisabled",
        |lua, this_: Value| {
            with(lua, &this_, |f| {
                f.button
                    .as_deref()
                    .is_some_and(|b| b.motion_scripts_while_disabled)
            })
        }
    );
    Ok(m)
}

/// `Enable`, `Disable`, `SetEnabled` and `IsEnabled` for buttons, edit
/// boxes and sliders.
fn enable_methods(lua: &Lua, m: &Table) -> Result<()> {
    method!(lua, m, "Enable", |lua, this_: Value| {
        let id = this(&this_)?;
        set_enabled(lua, id, true);
        Ok(())
    });
    method!(lua, m, "Disable", |lua, this_: Value| {
        let id = this(&this_)?;
        set_enabled(lua, id, false);
        Ok(())
    });
    method!(lua, m, "SetEnabled", |lua, (this_, v): (Value, Value)| {
        let id = this(&this_)?;
        set_enabled(lua, id, truthy(&v));
        Ok(())
    });
    method!(lua, m, "IsEnabled", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.is_enabled()
    ));
    Ok(())
}

fn set_enabled(lua: &Lua, id: FrameId, enabled: bool) {
    let changed = {
        let mut st = state(lua);
        let f = &mut st.frames[id];
        if f.is_enabled() == enabled {
            false
        } else {
            match f.button.as_deref_mut() {
                Some(b) => b.enabled = enabled,
                None => f.disabled = !enabled,
            }
            true
        }
    };
    // A disabled edit box loses the keyboard focus (inferred).
    if !enabled && state(lua).focus == Some(id) {
        state(lua).focus = None;
        fire_script(lua, id, "OnEditFocusLost", ());
    }
    update_button_font(lua, id);
    if changed {
        fire_script(lua, id, if enabled { "OnEnable" } else { "OnDisable" }, ());
    }
}

fn check_button_methods(lua: &Lua) -> Result<Table> {
    let m = lua.create_table()?;
    method!(lua, m, "SetChecked", |lua, (this_, v): (Value, Value)| {
        with(lua, &this_, |f| {
            if let Some(b) = f.button.as_deref_mut() {
                b.checked = truthy(&v);
            }
        })
    });
    method!(lua, m, "GetChecked", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.button.as_deref().is_some_and(|b| b.checked)
        })
    });
    for slot in ["Checked", "DisabledChecked"] {
        m.raw_set(
            format!("Set{slot}Texture"),
            lua.create_function(move |lua, (this_, v): (Value, Value)| {
                let id = this(&this_)?;
                set_slot(lua, id, slot, &v, false)
            })?,
        )?;
        m.raw_set(
            format!("Get{slot}Texture"),
            lua.create_function(move |lua, this_: Value| {
                let id = this(&this_)?;
                let t = state(lua).frames[id]
                    .button
                    .as_deref()
                    .and_then(|b| slot_get(b, parse_slot(slot)));
                Ok(table_of(lua, t))
            })?,
        )?;
    }
    Ok(m)
}

fn edit_box_methods(lua: &Lua) -> Result<Table> {
    let m = lua.create_table()?;
    method!(lua, m, "GetText", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.text
                .as_deref()
                .map(|t| t.text.clone())
                .unwrap_or_default()
        })
    });
    method!(lua, m, "GetDisplayText", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.text
                .as_deref()
                .map(|t| t.text.clone())
                .unwrap_or_default()
        })
    });
    method!(lua, m, "GetNumLetters", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.text
                .as_deref()
                .map(|t| t.text.chars().count())
                .unwrap_or(0)
        })
    });
    method!(lua, m, "ClearFocus", |lua, this_: Value| {
        let id = this(&this_)?;
        let had = {
            let mut st = state(lua);
            let had = st.focus == Some(id);
            if had {
                st.focus = None;
            }
            had
        };
        if had {
            fire_script(lua, id, "OnEditFocusLost", ());
        }
        Ok(())
    });
    method!(lua, m, "HasFocus", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(state(lua).focus == Some(id))
    });
    enable_methods(lua, &m)?;
    for name in [
        "SetBlinkSpeed",
        "SetVisibleTextByteLimit",
        "SetSecureText",
        "SetSecurityDisablePaste",
        "SetSecurityDisableSetText",
    ] {
        m.raw_set(name, empty_fn(lua)?)?;
    }
    method!(lua, m, "SetTextInsets", |lua, args: Variadic<Value>| {
        let id = this(args.first().unwrap_or(&Value::Nil))?;
        let v: Vec<f64> = (1..5).map(|i| num(args.get(i))).collect();
        let mut st = state(lua);
        if let Some(t) = st.frames[id].text.as_deref_mut() {
            t.insets = [v[0], v[1], v[2], v[3]];
        }
        Ok(())
    });
    method!(lua, m, "GetTextInsets", |lua, this_: Value| {
        with(lua, &this_, |f| {
            let i = f.text.as_deref().map(|t| t.insets).unwrap_or([0.0; 4]);
            (i[0], i[1], i[2], i[3])
        })
    });
    method!(lua, m, "GetNumber", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.text
                .as_deref()
                .and_then(|t| t.text.trim().parse::<f64>().ok())
                .unwrap_or(0.0)
        })
    });
    crate::editbox::methods(lua, &m)?;
    Ok(m)
}

fn value_methods(lua: &Lua, slider: bool) -> Result<Table> {
    let m = lua.create_table()?;
    if slider {
        enable_methods(lua, &m)?;
    }
    method!(lua, m, "SetMinMaxValues", |lua,
                                        (this_, min, max): (
        Value,
        f64,
        f64
    )| {
        with(lua, &this_, |f| {
            f.value.0 = min;
            f.value.1 = max;
            f.value.2 = f.value.2.clamp(min, max.max(min));
        })
    });
    method!(lua, m, "GetMinMaxValues", |lua, this_: Value| with(
        lua,
        &this_,
        |f| (f.value.0, f.value.1)
    ));
    method!(lua, m, "SetValue", move |lua, (this_, v): (Value, f64)| {
        let id = this(&this_)?;
        if slider {
            // SetValue does not round to the step (inferred).
            crate::slider::set_value(lua, id, v, false, false);
            return Ok(());
        }
        let changed = {
            let mut st = state(lua);
            let f = &mut st.frames[id];
            let v = v.clamp(f.value.0, f.value.1.max(f.value.0));
            let changed = (f.value.2 - v).abs() > f64::EPSILON;
            f.value.2 = v;
            changed.then_some(v)
        };
        if let Some(v) = changed {
            fire_script(lua, id, "OnValueChanged", (v, false));
        }
        Ok(())
    });
    method!(lua, m, "GetValue", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.value.2
    ));
    method!(
        lua,
        m,
        "SetOrientation",
        |lua, (this_, o): (Value, String)| {
            with(lua, &this_, |f| f.orientation = o.to_ascii_uppercase())
        }
    );
    method!(lua, m, "GetOrientation", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.orientation.clone()
    ));
    let tex_name = if slider {
        "ThumbTexture"
    } else {
        "StatusBarTexture"
    };
    m.raw_set(
        format!("Set{tex_name}"),
        lua.create_function(move |lua, (this_, v): (Value, Value)| {
            let id = this(&this_)?;
            let tex = match &v {
                Value::Table(t) => frame_id(t)?,
                other => {
                    let existing = state(lua).frames[id].status_texture;
                    let tex = match existing {
                        Some(t) => t,
                        None => {
                            let t = frame_id(&create_object(lua, "Texture", None, Some(id))?)?;
                            // UI.xsd: a slider's thumb is drawn in OVERLAY.
                            state(lua).frames[t].draw_layer = if slider { 3 } else { 2 };
                            t
                        }
                    };
                    // The asset may be an atlas name as well as a file.
                    let atlas = match other {
                        Value::String(s) => {
                            let name = s.to_string_lossy();
                            let is_path = name.contains('\\') || name.contains('/');
                            let known = state(lua)
                                .gamedata
                                .as_ref()
                                .is_some_and(|g| g.atlas(&name).is_some());
                            (!is_path && known).then_some(name)
                        }
                        _ => None,
                    };
                    match atlas {
                        Some(name) => {
                            set_atlas(lua, tex, &name, false);
                        }
                        None => set_texture_file(lua, tex, tex_file_arg(other)),
                    }
                    tex
                }
            };
            state(lua).frames[id].status_texture = Some(tex);
            Ok(())
        })?,
    )?;
    m.raw_set(
        format!("Get{tex_name}"),
        lua.create_function(|lua, this_: Value| {
            let id = this(&this_)?;
            let t = state(lua).frames[id].status_texture;
            Ok(table_of(lua, t))
        })?,
    )?;
    method!(lua, m, "SetStatusBarColor", |lua, args: Variadic<Value>| {
        let id = this(args.first().unwrap_or(&Value::Nil))?;
        let c = color_args(&args[1..]);
        let tex = state(lua).frames[id].status_texture;
        if let Some(t) = tex {
            crate::frames::ensure_tex(&mut state(lua).frames[t]).vertex_color = c;
        }
        Ok(())
    });
    method!(lua, m, "GetStatusBarColor", |lua, this_: Value| {
        let id = this(&this_)?;
        let tex = state(lua).frames[id].status_texture;
        let c = tex
            .and_then(|t| state(lua).frames[t].tex.as_deref().map(|t| t.vertex_color))
            .unwrap_or([1.0; 4]);
        Ok((c[0], c[1], c[2], c[3]))
    });
    method!(lua, m, "SetValueStep", |lua, (this_, v): (Value, f64)| {
        with(lua, &this_, |f| f.value_step = v.max(0.0))
    });
    method!(lua, m, "GetValueStep", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.value_step
    ));
    method!(
        lua,
        m,
        "SetObeyStepOnDrag",
        |lua, (this_, v): (Value, Value)| {
            with(lua, &this_, |f| f.obey_step_on_drag = truthy(&v))
        }
    );
    method!(lua, m, "GetObeyStepOnDrag", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.obey_step_on_drag
    ));
    method!(
        lua,
        m,
        "SetStepsPerPage",
        |lua, (this_, v): (Value, f64)| { with(lua, &this_, |f| f.steps_per_page = v) }
    );
    method!(lua, m, "GetStepsPerPage", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.steps_per_page
    ));
    empty!(lua, m, "SetFillStyle", |_, _args: Variadic<Value>| Ok(()));
    empty!(lua, m, "SetReverseFill", |_, _args: Variadic<Value>| Ok(()));
    empty!(lua, m, "SetRotatesTexture", |_, _args: Variadic<Value>| Ok(
        ()
    ));
    Ok(m)
}

fn place_scroll_child(lua: &Lua, id: FrameId) {
    let mut st = state(lua);
    let Some(child) = st.frames[id].scroll_child else {
        return;
    };
    let (h, v) = st.frames[id].scroll;
    st.frames[child].points = vec![Point {
        point: "TOPLEFT",
        relative: Some(id),
        relative_point: "TOPLEFT",
        x: -h,
        y: v,
    }];
    layout::invalidate(&mut st);
}

fn scroll_frame_methods(lua: &Lua) -> Result<Table> {
    let m = lua.create_table()?;
    method!(lua, m, "SetScrollChild", |lua,
                                       (this_, child): (
        Value,
        Value
    )| {
        let id = this(&this_)?;
        let child = this(&child)?;
        change_visibility(lua, child, |frames| {
            if let Some(old) = frames[child].parent {
                frames[old].children.retain(|&c| c != child);
            }
            frames[child].parent = Some(id);
            frames[id].children.push(child);
            frames[id].scroll_child = Some(child);
            frames[id].clips_children = true;
        });
        place_scroll_child(lua, id);
        Ok(())
    });
    method!(lua, m, "GetScrollChild", |lua, this_: Value| {
        let id = this(&this_)?;
        let c = state(lua).frames[id].scroll_child;
        Ok(table_of(lua, c))
    });
    method!(
        lua,
        m,
        "SetVerticalScroll",
        |lua, (this_, v): (Value, f64)| {
            let id = this(&this_)?;
            // Like the client, the script runs only when the value changes;
            // scroll bars and scroll frames update each other through it.
            let old = std::mem::replace(&mut state(lua).frames[id].scroll.1, v);
            place_scroll_child(lua, id);
            if (old - v).abs() > 1e-6 {
                fire_script(lua, id, "OnVerticalScroll", v);
            }
            Ok(())
        }
    );
    method!(
        lua,
        m,
        "SetHorizontalScroll",
        |lua, (this_, v): (Value, f64)| {
            let id = this(&this_)?;
            // Like the client, the script runs only when the value changes;
            // scroll bars and scroll frames update each other through it.
            let old = std::mem::replace(&mut state(lua).frames[id].scroll.0, v);
            place_scroll_child(lua, id);
            if (old - v).abs() > 1e-6 {
                fire_script(lua, id, "OnHorizontalScroll", v);
            }
            Ok(())
        }
    );
    method!(lua, m, "GetVerticalScroll", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.scroll.1
    ));
    method!(lua, m, "GetHorizontalScroll", |lua, this_: Value| with(
        lua,
        &this_,
        |f| f.scroll.0
    ));
    method!(lua, m, "GetVerticalScrollRange", |lua, this_: Value| {
        let id = this(&this_)?;
        let mut st = state(lua);
        let Some(child) = st.frames[id].scroll_child else {
            return Ok(0.0);
        };
        let (_, ch) = layout::size(&mut st, child);
        let (_, h) = layout::size(&mut st, id);
        Ok((ch - h).max(0.0))
    });
    method!(lua, m, "GetHorizontalScrollRange", |lua, this_: Value| {
        let id = this(&this_)?;
        let mut st = state(lua);
        let Some(child) = st.frames[id].scroll_child else {
            return Ok(0.0);
        };
        let (cw, _) = layout::size(&mut st, child);
        let (w, _) = layout::size(&mut st, id);
        Ok((cw - w).max(0.0))
    });
    method!(lua, m, "UpdateScrollChildRect", |lua, this_: Value| {
        let id = this(&this_)?;
        fire_scroll_range_changes(lua, Some(id));
        Ok(())
    });
    Ok(m)
}

fn anim_common_methods(lua: &Lua) -> Result<Table> {
    let m = lua.create_table()?;
    method!(lua, m, "IsPlaying", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.anim.as_deref().is_some_and(|a| a.playing)
        })
    });
    method!(lua, m, "IsPaused", |lua, this_: Value| {
        with(lua, &this_, |f| f.anim.as_deref().is_some_and(|a| a.paused))
    });
    method!(lua, m, "IsDone", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.anim.as_deref().is_some_and(|a| !a.playing)
        })
    });
    method!(lua, m, "IsStopped", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.anim.as_deref().is_some_and(|a| !a.playing)
        })
    });
    method!(lua, m, "Pause", |lua, this_: Value| {
        with(lua, &this_, |f| {
            if let Some(a) = f.anim.as_deref_mut() {
                a.paused = true;
            }
        })
    });
    method!(lua, m, "GetProgress", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.anim
                .as_deref()
                .map(|a| {
                    if a.duration > 0.0 {
                        (a.elapsed / a.duration).min(1.0)
                    } else {
                        0.0
                    }
                })
                .unwrap_or(0.0)
        })
    });
    method!(lua, m, "GetElapsed", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.anim.as_deref().map(|a| a.elapsed).unwrap_or(0.0)
        })
    });
    Ok(m)
}

/// Total duration of a group: animations with the same order run in
/// parallel; orders run one after another.
fn group_duration(st: &State, group: FrameId) -> f64 {
    let mut by_order: std::collections::BTreeMap<i64, f64> = Default::default();
    for &a in &st.frames[group].anims {
        if let Some(an) = st.frames[a].anim.as_deref() {
            let d = an.start_delay + an.duration + an.end_delay;
            let e = by_order.entry(an.order).or_insert(0.0);
            *e = e.max(d);
        }
    }
    by_order.values().sum()
}

fn play_group(lua: &Lua, id: FrameId, reverse: bool) {
    {
        let mut st = state(lua);
        let duration = group_duration(&st, id);
        let anims = st.frames[id].anims.clone();
        if let Some(a) = st.frames[id].anim.as_deref_mut() {
            a.playing = true;
            a.paused = false;
            a.elapsed = 0.0;
            a.duration = duration;
            a.reverse = reverse;
        }
        for c in anims {
            if let Some(a) = st.frames[c].anim.as_deref_mut() {
                a.playing = true;
                a.elapsed = 0.0;
            }
        }
    }
    fire_script(lua, id, "OnPlay", ());
}

fn stop_group(lua: &Lua, id: FrameId) {
    let was_playing = {
        let mut st = state(lua);
        let anims = st.frames[id].anims.clone();
        for c in anims {
            if let Some(a) = st.frames[c].anim.as_deref_mut() {
                a.playing = false;
            }
        }
        match st.frames[id].anim.as_deref_mut() {
            Some(a) if a.playing => {
                a.playing = false;
                a.paused = false;
                true
            }
            _ => false,
        }
    };
    if was_playing {
        fire_script(lua, id, "OnStop", true);
    }
}

/// End a group: apply final alphas if requested and fire `OnFinished`.
fn finish_group(lua: &Lua, id: FrameId, requested: bool) {
    {
        let mut st = state(lua);
        crate::animfx::keep_final_flipbook_frames(&mut st, id);
        let parent = st.frames[id].parent;
        let final_alpha = st.frames[id]
            .anim
            .as_deref()
            .is_some_and(|a| a.set_to_final_alpha);
        let anims = st.frames[id].anims.clone();
        for c in &anims {
            let Some(a) = st.frames[*c].anim.as_deref_mut() else {
                continue;
            };
            a.playing = false;
            let is_alpha = st.frames[*c].object_type == "Alpha";
            if final_alpha && is_alpha {
                let a = st.frames[*c].anim.as_deref().cloned().unwrap_or_default();
                let target = a.target.or_else(|| {
                    let key = a.child_key.as_ref().or(a.target_key.as_ref())?;
                    let p = parent?;
                    let t: Table = st.table(p).get(key.as_str()).ok()?;
                    frame_id(&t).ok()
                });
                if let Some(t) = target.or(parent) {
                    st.frames[t].alpha = a.to_alpha.clamp(0.0, 1.0);
                }
            }
        }
        if let Some(a) = st.frames[id].anim.as_deref_mut() {
            a.playing = false;
            a.paused = false;
        }
    }
    fire_script(lua, id, "OnFinished", requested);
}

/// Advance playing animation groups by `elapsed` seconds.
pub fn advance_animations(lua: &Lua, elapsed: f64) {
    let groups: Vec<FrameId> = {
        let st = state(lua);
        (0..st.frames.len())
            .filter(|&i| st.frames[i].object_type == "AnimationGroup")
            .filter(|&i| {
                st.frames[i]
                    .anim
                    .as_deref()
                    .is_some_and(|a| a.playing && !a.paused)
            })
            .collect()
    };
    for g in groups {
        let (done, looping) = {
            let mut st = state(lua);
            let Some(a) = st.frames[g].anim.as_deref_mut() else {
                continue;
            };
            a.elapsed += elapsed;
            (a.elapsed >= a.duration, a.looping.clone())
        };
        if done {
            if looping.eq_ignore_ascii_case("REPEAT") || looping.eq_ignore_ascii_case("BOUNCE") {
                if let Some(a) = state(lua).frames[g].anim.as_deref_mut() {
                    a.elapsed = 0.0;
                }
                fire_script(lua, g, "OnLoop", "FORWARD");
            } else {
                finish_group(lua, g, false);
            }
        }
    }
    crate::animfx::recompute(&mut state(lua));
}

fn anim_group_methods(lua: &Lua) -> Result<Table> {
    let m = lua.create_table()?;
    method!(lua, m, "Play", |lua, (this_, reverse): (Value, Value)| {
        let id = this(&this_)?;
        play_group(lua, id, truthy(&reverse));
        Ok(())
    });
    method!(
        lua,
        m,
        "Restart",
        |lua, (this_, reverse): (Value, Value)| {
            let id = this(&this_)?;
            play_group(lua, id, truthy(&reverse));
            Ok(())
        }
    );
    method!(lua, m, "PlaySynced", |lua, this_: Value| {
        let id = this(&this_)?;
        play_group(lua, id, false);
        Ok(())
    });
    method!(lua, m, "SetPlaying", |lua, (this_, v): (Value, Value)| {
        let id = this(&this_)?;
        if truthy(&v) {
            play_group(lua, id, false);
        } else {
            stop_group(lua, id);
        }
        Ok(())
    });
    method!(lua, m, "Stop", |lua, this_: Value| {
        let id = this(&this_)?;
        stop_group(lua, id);
        Ok(())
    });
    method!(lua, m, "Finish", |lua, this_: Value| {
        let id = this(&this_)?;
        let playing = state(lua).frames[id]
            .anim
            .as_deref()
            .is_some_and(|a| a.playing);
        if playing {
            finish_group(lua, id, true);
        }
        Ok(())
    });
    method!(lua, m, "IsReverse", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.anim.as_deref().is_some_and(|a| a.reverse)
        })
    });
    method!(lua, m, "SetLooping", |lua, (this_, l): (Value, String)| {
        with(lua, &this_, |f| {
            if let Some(a) = f.anim.as_deref_mut() {
                a.looping = l.to_ascii_uppercase();
            }
        })
    });
    method!(lua, m, "GetLooping", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.anim
                .as_deref()
                .map(|a| a.looping.clone())
                .unwrap_or_default()
        })
    });
    empty!(lua, m, "GetLoopState", |_, _this: Value| Ok("NONE"));
    method!(
        lua,
        m,
        "SetToFinalAlpha",
        |lua, (this_, v): (Value, Value)| {
            with(lua, &this_, |f| {
                if let Some(a) = f.anim.as_deref_mut() {
                    a.set_to_final_alpha = truthy(&v);
                }
            })
        }
    );
    method!(lua, m, "IsSetToFinalAlpha", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.anim.as_deref().is_some_and(|a| a.set_to_final_alpha)
        })
    });
    method!(lua, m, "GetDuration", |lua, this_: Value| {
        let id = this(&this_)?;
        Ok(group_duration(&state(lua), id))
    });
    method!(lua, m, "CreateAnimation", |lua,
                                        (
        this_,
        kind,
        name,
        template,
    ): (
        Value,
        Option<String>,
        Option<String>,
        Option<String>
    )| {
        let id = this(&this_)?;
        let kind = kind.unwrap_or_else(|| "Animation".into());
        crate::xml::create_child(lua, &kind, name, id, template.as_deref(), None)
    });
    method!(lua, m, "GetAnimations", |lua, this_: Value| {
        let id = this(&this_)?;
        let st = state(lua);
        let anims: Variadic<Table> = st.frames[id].anims.iter().map(|&a| st.table(a)).collect();
        Ok(anims)
    });
    method!(lua, m, "SetAnimationSpeedMultiplier", |_,
                                                    _args: Variadic<
        Value,
    >| Ok(()));
    empty!(lua, m, "GetAnimationSpeedMultiplier", |_, _this: Value| Ok(
        1.0
    ));
    Ok(m)
}

fn animation_methods(lua: &Lua) -> Result<Table> {
    let m = lua.create_table()?;
    macro_rules! anim_field {
        ($set:literal, $get:literal, $field:ident) => {
            method!(lua, m, $set, |lua, (this_, v): (Value, f64)| {
                with(lua, &this_, |f| {
                    if let Some(a) = f.anim.as_deref_mut() {
                        a.$field = v;
                    }
                })
            });
            method!(lua, m, $get, |lua, this_: Value| {
                with(lua, &this_, |f| {
                    f.anim.as_deref().map(|a| a.$field).unwrap_or(0.0)
                })
            });
        };
    }
    anim_field!("SetDuration", "GetDuration", duration);
    anim_field!("SetStartDelay", "GetStartDelay", start_delay);
    anim_field!("SetEndDelay", "GetEndDelay", end_delay);
    anim_field!("SetFromAlpha", "GetFromAlpha", from_alpha);
    anim_field!("SetToAlpha", "GetToAlpha", to_alpha);
    macro_rules! anim_pair {
        ($set:literal, $get:literal, $field:ident) => {
            method!(lua, m, $set, |lua, (this_, x, y): (Value, f64, f64)| {
                with(lua, &this_, |f| {
                    if let Some(a) = f.anim.as_deref_mut() {
                        a.$field = (x, y);
                    }
                })
            });
            method!(lua, m, $get, |lua, this_: Value| {
                with(lua, &this_, |f| {
                    f.anim.as_deref().map(|a| a.$field).unwrap_or_default()
                })
            });
        };
    }
    anim_pair!("SetOffset", "GetOffset", offset);
    anim_pair!("SetScaleFrom", "GetScaleFrom", scale_from);
    anim_pair!("SetScaleTo", "GetScaleTo", scale_to);
    anim_pair!("SetScale", "GetScale", scale_to);
    anim_field!("SetRadians", "GetRadians", radians);
    method!(lua, m, "SetDegrees", |lua, (this_, v): (Value, f64)| {
        with(lua, &this_, |f| {
            if let Some(a) = f.anim.as_deref_mut() {
                a.radians = v.to_radians();
            }
        })
    });
    method!(lua, m, "GetDegrees", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.anim
                .as_deref()
                .map(|a| a.radians.to_degrees())
                .unwrap_or(0.0)
        })
    });
    method!(lua, m, "SetOrigin", |lua,
                                  (this_, p, x, y): (
        Value,
        String,
        Option<f64>,
        Option<f64>
    )| {
        with(lua, &this_, |f| {
            if let Some(a) = f.anim.as_deref_mut() {
                a.origin = Some((p.to_ascii_uppercase(), x.unwrap_or(0.0), y.unwrap_or(0.0)));
            }
        })
    });
    method!(lua, m, "GetOrigin", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.anim
                .as_deref()
                .and_then(|a| a.origin.clone())
                .unwrap_or(("CENTER".into(), 0.0, 0.0))
        })
    });
    macro_rules! flipbook_field {
        ($set:literal, $get:literal, $idx:tt) => {
            method!(lua, m, $set, |lua, (this_, v): (Value, u32)| {
                with(lua, &this_, |f| {
                    if let Some(a) = f.anim.as_deref_mut() {
                        a.flipbook.$idx = v.max(1);
                    }
                })
            });
            method!(lua, m, $get, |lua, this_: Value| {
                with(lua, &this_, |f| {
                    f.anim.as_deref().map(|a| a.flipbook.$idx).unwrap_or(1)
                })
            });
        };
    }
    flipbook_field!("SetFlipBookRows", "GetFlipBookRows", 0);
    flipbook_field!("SetFlipBookColumns", "GetFlipBookColumns", 1);
    flipbook_field!("SetFlipBookFrames", "GetFlipBookFrames", 2);
    method!(
        lua,
        m,
        "SetSmoothing",
        |lua, (this_, v): (Value, String)| {
            with(lua, &this_, |f| {
                if let Some(a) = f.anim.as_deref_mut() {
                    a.smoothing = v.to_ascii_uppercase();
                }
            })
        }
    );
    method!(lua, m, "GetSmoothing", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.anim
                .as_deref()
                .map(|a| {
                    if a.smoothing.is_empty() {
                        "NONE".to_string()
                    } else {
                        a.smoothing.clone()
                    }
                })
                .unwrap_or_default()
        })
    });
    method!(lua, m, "SetOrder", |lua, (this_, v): (Value, i64)| {
        with(lua, &this_, |f| {
            if let Some(a) = f.anim.as_deref_mut() {
                a.order = v;
            }
        })
    });
    method!(lua, m, "GetOrder", |lua, this_: Value| {
        with(lua, &this_, |f| {
            f.anim.as_deref().map(|a| a.order).unwrap_or(0)
        })
    });
    method!(lua, m, "SetChildKey", |lua, (this_, v): (Value, String)| {
        with(lua, &this_, |f| {
            if let Some(a) = f.anim.as_deref_mut() {
                a.child_key = Some(v);
            }
        })
    });
    method!(
        lua,
        m,
        "SetTargetKey",
        |lua, (this_, v): (Value, String)| {
            with(lua, &this_, |f| {
                if let Some(a) = f.anim.as_deref_mut() {
                    a.target_key = Some(v);
                }
            })
        }
    );
    method!(lua, m, "SetTarget", |lua, (this_, v): (Value, Value)| {
        let target = optional_frame(lua, &v).ok().flatten();
        with(lua, &this_, |f| {
            if let Some(a) = f.anim.as_deref_mut() {
                a.target = target;
            }
        })
    });
    method!(lua, m, "GetTarget", |lua, this_: Value| {
        let id = this(&this_)?;
        let (target, group) = {
            let st = state(lua);
            (
                st.frames[id].anim.as_deref().and_then(|a| a.target),
                st.frames[id].parent,
            )
        };
        let owner = group.and_then(|g| state(lua).frames[g].parent);
        Ok(table_of(lua, target.or(owner)))
    });
    method!(lua, m, "GetRegionParent", |lua, this_: Value| {
        let id = this(&this_)?;
        let group = state(lua).frames[id].parent;
        let owner = group.and_then(|g| state(lua).frames[g].parent);
        Ok(table_of(lua, owner))
    });
    method!(lua, m, "Play", |lua, this_: Value| {
        with(lua, &this_, |f| {
            if let Some(a) = f.anim.as_deref_mut() {
                a.playing = true;
            }
        })
    });
    method!(lua, m, "Stop", |lua, this_: Value| {
        with(lua, &this_, |f| {
            if let Some(a) = f.anim.as_deref_mut() {
                a.playing = false;
            }
        })
    });
    Ok(m)
}
