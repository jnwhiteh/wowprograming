//! Globals built from Blizzard's API documentation: `Enum`, `Constants`,
//! and stubs for documented functions and widget methods.
//!
//! In the client these come from native code. The same data ships as Lua
//! in `Blizzard_APIDocumentationGenerated`, so we build them from those
//! files. A stub returns default values of the documented return types,
//! and records that it was called (see `called_stubs`).

use std::collections::HashMap;
use std::path::Path;

use mlua::{Function, Lua, Result, Table, Value};
use wowprogramming_lua::mlua;

use crate::state::state;

const BUILDER: &str = include_str!("lua/apidoc.lua");
const DOC_DIR: &str = "Interface/AddOns/Blizzard_APIDocumentationGenerated";
const FUNCTION_STUBS_KEY: &str = "wowprogramming.function_stubs";
const CALLED_STUBS_KEY: &str = "wowprogramming.called_stubs";
/// Registry key of the documented mixins of return values.
pub const RETURN_MIXINS_KEY: &str = "wowprogramming.return_mixins";

/// Widget method groups (see `frames::method_chain`) of each documentation table.
const WIDGET_DOCS: &[(&str, &str)] = &[
    ("SimpleObjectAPI", "Object"),
    ("SimpleFrameScriptObjectAPI", "Object"),
    ("SimpleScriptRegionAPI", "ScriptRegion"),
    ("SimpleScriptRegionResizingAPI", "ScriptRegion"),
    ("SimpleAnimatableObjectAPI", "ScriptRegion"),
    ("SimpleRegionAPI", "Region"),
    ("SimpleTextureBaseAPI", "Texture"),
    ("SimpleTextureAPI", "Texture"),
    ("SimpleMaskTextureAPI", "Texture"),
    ("SimpleLineAPI", "Line"),
    ("SimpleFontAPI", "Font"),
    ("SimpleFontStringAPI", "FontString"),
    ("SimpleFrameAPI", "Frame"),
    ("SimpleButtonAPI", "Button"),
    ("SimpleCheckboxAPI", "CheckButton"),
    ("SimpleEditBoxAPI", "EditBox"),
    ("SimpleSliderAPI", "Slider"),
    ("SimpleStatusBarAPI", "StatusBar"),
    ("SimpleScrollFrameAPI", "ScrollFrame"),
    ("FrameAPICooldown", "Cooldown"),
    ("FrameAPITooltip", "GameTooltip"),
    ("SimpleModelAPI", "Model"),
    ("FrameAPICharacterModelBase", "PlayerModel"),
    ("FrameAPIDressUpModel", "PlayerModel"),
    ("FrameAPICinematicModel", "PlayerModel"),
    ("FrameAPITabardModelBase", "PlayerModel"),
    ("FrameAPITabardModel", "PlayerModel"),
    ("FrameAPIModelSceneFrame", "ModelScene"),
    ("FrameAPIModelSceneFrameShared", "ModelScene"),
    ("SimpleMessageFrameAPI", "MessageFrame"),
    ("SimpleHTMLAPI", "SimpleHTML"),
    ("SimpleColorSelectAPI", "ColorSelect"),
    ("MinimapFrameAPI", "Minimap"),
    ("SimpleAnimGroupAPI", "AnimationGroup"),
];

/// Build `Enum` and `Constants`, and return the widget method stubs by
/// method group. Function stubs are kept for [`install_function_stubs`].
pub fn install(lua: &Lua, ui_source_dir: Option<&Path>) -> Result<HashMap<String, Table>> {
    let files = lua.create_table()?;
    if let Some(dir) = ui_source_dir.map(|d| d.join(DOC_DIR)) {
        let mut paths: Vec<_> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lua")))
            .collect();
        paths.sort();
        for path in paths {
            let Ok(code) = std::fs::read(&path) else {
                continue;
            };
            let name = format!(
                "{DOC_DIR}/{}",
                path.file_name().unwrap_or_default().to_string_lossy()
            );
            files.raw_push(lua.create_sequence_from([
                Value::String(lua.create_string(name)?),
                Value::String(lua.create_string(code)?),
            ])?)?;
        }
        if files.raw_len() == 0 {
            state(lua).warn(format!(
                "no API documentation found in {}; Enum and Constants are empty",
                dir.display()
            ));
        }
    }
    let loadstring: Function = lua.globals().get("loadstring")?;
    let called = lua.create_table()?;
    lua.set_named_registry_value(CALLED_STUBS_KEY, called.clone())?;
    let watchdog = state(lua).watchdog.clone();
    let (enum_table, constants, errors, function_stubs, widget_stubs, return_mixins): (
        Table,
        Table,
        Vec<String>,
        Table,
        Table,
        Table,
    ) = watchdog.guarded(|| {
        lua.load(BUILDER)
            .set_name("=[C]")
            .call((files, loadstring, called))
    })?;
    {
        let mut st = state(lua);
        for err in errors {
            st.warn(format!("API documentation: {err}"));
        }
    }
    lua.globals().set("Enum", enum_table)?;
    lua.globals().set("Constants", constants)?;
    lua.set_named_registry_value(FUNCTION_STUBS_KEY, function_stubs)?;
    lua.set_named_registry_value(RETURN_MIXINS_KEY, return_mixins)?;

    let mut groups: HashMap<String, Table> = HashMap::new();
    for pair in widget_stubs.pairs::<String, Table>() {
        let (doc, methods) = pair?;
        let group = WIDGET_DOCS
            .iter()
            .find(|(d, _)| *d == doc)
            .map(|(_, g)| *g)
            .or_else(|| doc.starts_with("SimpleAnim").then_some("Animation"));
        let Some(group) = group else { continue };
        let target = match groups.get(group) {
            Some(t) => t.clone(),
            None => {
                let t = lua.create_table()?;
                groups.insert(group.to_string(), t.clone());
                t
            }
        };
        for pair in methods.pairs::<Value, Value>() {
            let (k, v) = pair?;
            target.raw_set(k, v)?;
        }
    }
    Ok(groups)
}

/// Add stubs for documented functions that no native or Lua code defined.
/// Call this after all natives are installed.
pub fn install_function_stubs(lua: &Lua) -> Result<()> {
    let Ok(stubs) = lua.named_registry_value::<Table>(FUNCTION_STUBS_KEY) else {
        return Ok(());
    };
    let g = lua.globals();
    for pair in stubs.pairs::<String, Value>() {
        let (name, value) = pair?;
        match value {
            Value::Function(f) => {
                if g.raw_get::<Value>(name.as_str())?.is_nil() {
                    g.raw_set(name, f)?;
                }
            }
            Value::Table(functions) => {
                let ns = match g.raw_get::<Value>(name.as_str())? {
                    Value::Table(t) => t,
                    Value::Nil => {
                        let t = lua.create_table()?;
                        g.raw_set(name.as_str(), t.clone())?;
                        t
                    }
                    _ => continue,
                };
                for pair in functions.pairs::<Value, Value>() {
                    let (k, f) = pair?;
                    if ns.raw_get::<Value>(k.clone())?.is_nil() {
                        ns.raw_set(k, f)?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Add stubs for undocumented client functions (see `clientnames.rs`)
/// that nothing else defined. They return nothing (`GetNum*` returns 0)
/// and are counted like the documented stubs.
pub fn install_named_stubs(lua: &Lua, names: &[String]) -> Result<usize> {
    let called: Table = lua.named_registry_value(CALLED_STUBS_KEY)?;
    let g = lua.globals();
    let mut count = 0;
    for name in names {
        // "C_Namespace.Function" goes into the namespace table.
        let (container, key) = match name.split_once('.') {
            Some((ns, f)) => {
                let t = match g.raw_get::<Value>(ns)? {
                    Value::Table(t) => t,
                    Value::Nil => {
                        let t = lua.create_table()?;
                        g.raw_set(ns, t.clone())?;
                        t
                    }
                    _ => continue,
                };
                (t, f)
            }
            None => (g.clone(), name.as_str()),
        };
        if !container.raw_get::<Value>(key)?.is_nil() {
            continue;
        }
        let called = called.clone();
        let full = name.clone();
        // Counts are used as loop limits: GetNum* returns 0.
        let is_count = key.starts_with("GetNum");
        let f = lua.create_function(move |_, _: mlua::MultiValue| {
            let n: i64 = called.raw_get(full.as_str()).unwrap_or(0);
            called.raw_set(full.as_str(), n + 1)?;
            Ok(if is_count { Some(0) } else { None })
        })?;
        container.raw_set(key, f)?;
        count += 1;
    }
    Ok(count)
}

/// Names of stubs called so far, with call counts, most called first.
pub fn called_stubs(lua: &Lua) -> Vec<(String, i64)> {
    let Ok(t) = lua.named_registry_value::<Table>(CALLED_STUBS_KEY) else {
        return Vec::new();
    };
    let mut out: Vec<(String, i64)> = t.pairs::<String, i64>().flatten().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}
