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
/// Registry key of the Lua function that applies a bindings file.
const APPLY_KEY: &str = "wowprogramming.bindings.apply";
/// The client's file of changed key bindings.
pub const FILE_NAME: &str = "bindings-cache.wtf";

/// The sandbox's own bindings file (account-wide).
pub fn sandbox_file(lua: &Lua) -> std::path::PathBuf {
    let st = state(lua);
    st.wtf
        .root
        .join("Account")
        .join(&st.wtf.account)
        .join(FILE_NAME)
}

/// Apply a bindings file's text over the current bindings.
pub fn apply_file(lua: &Lua, text: &str) {
    if let Ok(f) = lua.named_registry_value::<Function>(APPLY_KEY) {
        safe_call(lua, &f, text);
    }
}

/// Read binding actions and install the binding API functions.
pub fn load(lua: &Lua) -> Result<()> {
    let actions = lua.create_table()?;
    let run_on_up = lua.create_table()?;
    let modified_clicks = lua.create_table()?;
    // Every binding in file order: { name, category } (GetBinding).
    let list = lua.create_table()?;
    let ui = state(lua).ui_source_dir.clone();
    if let Some(path) = ui.map(|d| d.join(BINDINGS_FILE))
        && let Ok(text) = std::fs::read_to_string(&path)
    {
        let doc = roxmltree::Document::parse(text.trim_start_matches('\u{feff}'))
            .map_err(mlua::Error::external)?;
        for node in doc
            .descendants()
            .filter(|n| n.has_tag_name("ModifiedClick"))
        {
            if let (Some(action), Some(default)) =
                (node.attribute("action"), node.attribute("default"))
            {
                modified_clicks.set(action, default)?;
            }
        }
        for node in doc.descendants().filter(|n| n.has_tag_name("Binding")) {
            let Some(name) = node.attribute("name") else {
                continue;
            };
            let entry = lua.create_table()?;
            entry.set("name", name)?;
            entry.set("category", node.attribute("category"))?;
            list.push(entry)?;
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
    // SaveBindings writes the sandbox's own file, never the game's.
    let save = lua.create_function(|lua, text: String| {
        let path = sandbox_file(lua);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&path, text) {
            state(lua).warn(format!("cannot write {}: {e}", path.display()));
        }
        Ok(())
    })?;
    let (run, apply): (Function, Function) = lua
        .load(include_str!("lua/bindings.lua"))
        .set_name("=[C]")
        .call((
            actions,
            run_on_up,
            registered,
            fire,
            save,
            modified_clicks,
            list,
        ))?;
    lua.set_named_registry_value(RUN_KEY, run)?;
    lua.set_named_registry_value(APPLY_KEY, apply)?;
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
