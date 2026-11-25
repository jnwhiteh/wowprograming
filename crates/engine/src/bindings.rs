//! Key bindings. Rust reads the binding actions from
//! `Bindings_Standard.xml`; `lua/bindings.lua` holds the key map, the
//! override bindings and the binding API, and runs a key's command.

use mlua::{Function, Lua, Result, Value};
use wowprogramming_lua::mlua;

use crate::natives::safe_call;
use crate::state::state;

const BINDINGS_FILE: &str = "Interface/AddOns/Blizzard_FrameXML/Bindings_Standard.xml";

/// Registry key of the Lua function that runs a key: `(key, down) -> bool`.
const RUN_KEY: &str = "wowprogramming.bindings.run";

/// Read binding actions and install the binding API functions.
pub fn load(lua: &Lua) -> Result<()> {
    let actions = lua.create_table()?;
    let run_on_up = lua.create_table()?;
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
                    actions.set(name, f)?;
                    if node.attribute("runOnUp") == Some("true") {
                        run_on_up.set(name, true)?;
                    }
                }
                Err(e) => state(lua).warn(format!("binding {name}: {e}")),
            }
        }
    }
    // Whether a button takes a click of `mouseButton` going down or up.
    let registered = lua.create_function(|lua, (frame, button, down): (Value, String, bool)| {
        let Value::Table(t) = frame else {
            return Ok(false);
        };
        let Ok(id) = crate::frames::frame_id(&t) else {
            return Ok(false);
        };
        Ok(crate::input::registered_for(lua, id, &button, down))
    })?;
    let fire = lua.create_function(|lua, event: String| {
        crate::frames::dispatch_event(lua, &event, mlua::MultiValue::new());
        Ok(())
    })?;
    let run: Function = lua
        .load(include_str!("lua/bindings.lua"))
        .set_name("=[C]")
        .call((actions, run_on_up, registered, fire))?;
    lua.set_named_registry_value(RUN_KEY, run)?;
    Ok(())
}

fn run(lua: &Lua, key: &str, down: bool) -> bool {
    let Ok(f) = lua.named_registry_value::<Function>(RUN_KEY) else {
        return false;
    };
    // Errors in binding handlers go to the error handler; the key counts
    // as bound.
    match safe_call(lua, &f, (key, down)) {
        Some(values) => !matches!(values.front(), Some(Value::Boolean(false)) | None),
        None => true,
    }
}

/// Run the command bound to a key, as the client does on key press.
/// Returns false if the key has no binding.
pub fn key_down(lua: &Lua, key: &str) -> bool {
    run(lua, key, true)
}

/// A key release: commands that run on release (movement) stop.
pub fn key_up(lua: &Lua, key: &str) {
    run(lua, key, false);
}
