//! Globals built from Blizzard's API documentation: `Enum` and `Constants`.
//!
//! In the client these tables come from native code. The same data ships
//! as Lua in `Blizzard_APIDocumentationGenerated`, so we build the tables
//! from those files.

use std::path::Path;

use mlua::{Function, Lua, Result, Table};
use wowprogramming_lua::mlua;

use crate::state::state;

const BUILDER: &str = include_str!("lua/apidoc.lua");
const DOC_DIR: &str = "Interface/AddOns/Blizzard_APIDocumentationGenerated";

pub fn install(lua: &Lua, ui_source_dir: Option<&Path>) -> Result<()> {
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
                mlua::Value::String(lua.create_string(name)?),
                mlua::Value::String(lua.create_string(code)?),
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
    let watchdog = state(lua).watchdog.clone();
    let (enum_table, constants, errors): (Table, Table, Vec<String>) =
        watchdog.guarded(|| lua.load(BUILDER).set_name("=[C]").call((files, loadstring)))?;
    {
        let mut st = state(lua);
        for err in errors {
            st.warn(format!("API documentation: {err}"));
        }
    }
    lua.globals().set("Enum", enum_table)?;
    lua.globals().set("Constants", constants)?;
    Ok(())
}
