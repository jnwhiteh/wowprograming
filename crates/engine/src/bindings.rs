//! Key bindings: actions from `Bindings_Standard.xml` and a default key map.

use mlua::{Function, Lua, Result, Value};
use wowprogramming_lua::mlua;

use crate::natives::safe_call;
use crate::state::state;

/// Default keys of the client, for the actions the sandbox supports.
pub const DEFAULT_KEYS: &[(&str, &str)] = &[
    ("P", "TOGGLESPELLBOOK"),
    ("SHIFT-P", "TOGGLEPETBOOK"),
    ("N", "TOGGLETALENTS"),
    ("C", "TOGGLECHARACTER0"),
    ("B", "OPENALLBAGS"),
    ("ESCAPE", "TOGGLEGAMEMENU"),
    ("TAB", "TARGETNEARESTENEMY"),
    ("1", "ACTIONBUTTON1"),
    ("2", "ACTIONBUTTON2"),
    ("3", "ACTIONBUTTON3"),
    ("4", "ACTIONBUTTON4"),
    ("5", "ACTIONBUTTON5"),
    ("6", "ACTIONBUTTON6"),
    ("7", "ACTIONBUTTON7"),
    ("8", "ACTIONBUTTON8"),
    ("9", "ACTIONBUTTON9"),
    ("0", "ACTIONBUTTON10"),
    ("-", "ACTIONBUTTON11"),
    ("=", "ACTIONBUTTON12"),
];

const BINDINGS_FILE: &str = "Interface/AddOns/Blizzard_FrameXML/Bindings_Standard.xml";

/// Read binding actions and install the binding API functions.
pub fn load(lua: &Lua) -> Result<()> {
    let ui = state(lua).ui_source_dir.clone();
    if let Some(path) = ui.map(|d| d.join(BINDINGS_FILE))
        && let Ok(text) = std::fs::read_to_string(&path)
    {
        let doc = roxmltree::Document::parse(text.trim_start_matches('\u{feff}'))
            .map_err(mlua::Error::external)?;
        for node in doc.descendants().filter(|n| n.has_tag_name("Binding")) {
            let Some(name) = node.attribute("name") else {
                continue;
            };
            let body: String = node.children().filter_map(|c| c.text()).collect();
            if body.trim().is_empty() {
                continue;
            }
            let code = format!("return function(keystate) {body} end");
            match lua
                .load(code)
                .set_name(format!("={name}"))
                .call::<Function>(())
            {
                Ok(f) => {
                    state(lua).bindings.insert(name.to_string(), f);
                }
                Err(e) => state(lua).warn(format!("binding {name}: {e}")),
            }
        }
    }

    let g = lua.globals();
    g.set(
        "GetBindingKey",
        lua.create_function(|_, action: String| {
            // One return value per key, not a table.
            let keys: mlua::Variadic<&str> = DEFAULT_KEYS
                .iter()
                .filter(|(_, a)| a.eq_ignore_ascii_case(&action))
                .map(|(k, _)| *k)
                .collect();
            Ok(keys)
        })?,
    )?;
    g.set(
        "GetBindingAction",
        lua.create_function(|_, key: String| {
            Ok(DEFAULT_KEYS
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(&key))
                .map(|(_, a)| *a)
                .unwrap_or(""))
        })?,
    )?;
    g.set(
        "GetBindingText",
        lua.create_function(|_, key: Value| {
            Ok(match key {
                Value::String(s) => s.to_string_lossy(),
                _ => String::new(),
            })
        })?,
    )?;
    Ok(())
}

/// Run the action bound to a key, as the client does on key press.
/// Returns false if the key has no binding.
pub fn key_down(lua: &Lua, key: &str) -> bool {
    let Some((_, action)) = DEFAULT_KEYS
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
    else {
        return false;
    };
    if *action == "TOGGLEGAMEMENU" {
        // Blizzard's handler stops a cast, closes windows, clears the
        // target, then shows the game menu. Without the game menu addon,
        // Escape closes open panels.
        let close = lua.load(
            "if ToggleGameMenu then return ToggleGameMenu() end \
             if CloseAllWindows then return CloseAllWindows() end",
        );
        if let Ok(f) = close.set_name("=ESCAPE").into_function() {
            safe_call(lua, &f, ());
        }
        return true;
    }
    let f = state(lua).bindings.get(*action).cloned();
    match f {
        Some(f) => {
            safe_call(lua, &f, "down");
            true
        }
        None => {
            state(lua).warn(format!("no binding action {action}"));
            false
        }
    }
}
