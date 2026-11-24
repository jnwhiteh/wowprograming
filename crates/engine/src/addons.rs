//! Finding, ordering and loading addons, and the `C_AddOns` API.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use mlua::{IntoLuaMulti, Lua, MultiValue, Result, Value};
use wowprogramming_lua::mlua;

use crate::frames::dispatch_event;
use crate::natives::run_chunk;
use crate::savedvars;
use crate::state::{Addon, AddonKind, LoadState, state};
use crate::toc::Toc;

/// Find `name` inside `dir`, ignoring case. Addon files often differ in
/// case from their TOC entries, which matters on case-sensitive systems.
pub fn find_case_insensitive(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.exists() {
        return Some(exact);
    }
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
        .map(|e| e.path())
}

/// Resolve a relative path with `/` separators, ignoring case at each step.
pub fn resolve_path(base: &Path, relative: &str) -> Option<PathBuf> {
    let mut current = base.to_path_buf();
    for part in relative.split('/').filter(|p| !p.is_empty()) {
        current = find_case_insensitive(&current, part)?;
    }
    Some(current)
}

/// Read an addon folder. The folder must contain `<FolderName>_Mainline.toc`
/// or `<FolderName>.toc`; the Retail flavor file wins.
pub fn read_addon(dir: &Path, kind: AddonKind, glue: bool) -> Option<Addon> {
    let name = dir.file_name()?.to_string_lossy().to_string();
    let toc_path = find_case_insensitive(dir, &format!("{name}_Mainline.toc"))
        .or_else(|| find_case_insensitive(dir, &format!("{name}.toc")))?;
    let text = std::fs::read(&toc_path).ok()?;
    Some(Addon {
        name,
        dir: dir.to_path_buf(),
        toc: Toc::parse_for(&String::from_utf8_lossy(&text), glue),
        kind,
        load_state: LoadState::NotLoaded,
        namespace: None,
        bootstrapped: false,
    })
}

/// All addon folders in `dir` that have a matching TOC file, sorted by name
/// the way the client lists them. Folders named `Blizzard_*` are skipped.
pub fn discover_user_addons(dir: &Path, glue: bool) -> Vec<Addon> {
    let mut addons: Vec<Addon> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter(|e| !e.file_name().to_string_lossy().starts_with("Blizzard_"))
        .filter_map(|e| read_addon(&e.path(), AddonKind::User, glue))
        .collect();
    addons.sort_by_key(|a| a.name.to_ascii_lowercase());
    addons
}

/// Find an addon by name, reading Blizzard addons from the UI source on
/// first use. Returns its index in the addon list.
fn lookup(lua: &Lua, name: &str) -> Option<usize> {
    let mut st = state(lua);
    if let Some(i) = st.find_addon(name) {
        return Some(i);
    }
    let ui = st.ui_source_dir.clone()?;
    let dir = resolve_path(&ui, &format!("Interface/AddOns/{name}"))?;
    let addon = read_addon(&dir, AddonKind::Blizzard, st.glue)?;
    st.addons.push(addon);
    Some(st.addons.len() - 1)
}

/// Names of the Blizzard addons the client loads at startup: those in the
/// UI source that may load in game and are not load-on-demand. Addons
/// marked `LoadFirst` come first; each group is sorted by name. Addons
/// that the client runs in its separate secure environment
/// (`UseSecureEnvironment`) come before those: the client sets that
/// environment up first (other addons use its intrinsics, such as
/// `AuraContainer`, without declaring a dependency). The sandbox runs them
/// in the normal environment; see `GetGlobalEnvironment` in
/// `lua/client.lua`.
pub fn startup_blizzard_addons(lua: &Lua) -> Vec<String> {
    let glue = state(lua).glue;
    let mut addons: Vec<(bool, bool, String, String)> = blizzard_addons_in_source(lua)
        .into_iter()
        .filter(|a| !a.toc.load_on_demand() && a.toc.loadable_in(glue))
        .map(|a| {
            let secure = a
                .toc
                .get("UseSecureEnvironment")
                .is_some_and(|v| v.trim() == "1");
            (
                !secure,
                !a.toc.load_first(),
                a.name.to_ascii_lowercase(),
                a.name,
            )
        })
        .collect();
    addons.sort();
    addons.into_iter().map(|(_, _, _, name)| name).collect()
}

/// Names of the load-on-demand Blizzard addons with `[Bootstrap]` files,
/// sorted by name. The client runs those files at startup: they define
/// the functions that load the addon, such as `TimeManager_LoadUI`.
pub fn startup_bootstrap_addons(lua: &Lua) -> Vec<String> {
    let glue = state(lua).glue;
    let mut names: Vec<String> = blizzard_addons_in_source(lua)
        .into_iter()
        .filter(|a| a.toc.load_on_demand() && a.toc.loadable_in(glue))
        .filter(|a| a.toc.entries.iter().any(|e| e.bootstrap))
        .map(|a| a.name)
        .collect();
    names.sort_by_key(|n| n.to_ascii_lowercase());
    names
}

/// Every `Blizzard_*` addon in the UI source, read for the current
/// screen (glue or game).
fn blizzard_addons_in_source(lua: &Lua) -> Vec<Addon> {
    let (ui, glue) = {
        let st = state(lua);
        (st.ui_source_dir.clone(), st.glue)
    };
    let Some(dir) = ui.and_then(|ui| resolve_path(&ui, "Interface/AddOns")) else {
        return Vec::new();
    };
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("Blizzard_"))
        .filter_map(|e| read_addon(&e.path(), AddonKind::Blizzard, glue))
        .collect()
}

/// `names` with their dependencies placed before them, each once, in the
/// order [`load_addon`] would load them. Loading the result one name at a
/// time does the same work as loading `names`, in smaller steps.
pub fn load_order(lua: &Lua, names: &[String]) -> Vec<String> {
    fn visit(lua: &Lua, name: &str, seen: &mut HashSet<String>, out: &mut Vec<String>) {
        if !seen.insert(name.to_ascii_lowercase()) {
            return;
        }
        let Some(i) = lookup(lua, name) else {
            out.push(name.to_string());
            return;
        };
        let (deps, optional) = {
            let st = state(lua);
            let a = &st.addons[i];
            (a.toc.dependencies(), a.toc.optional_dependencies())
        };
        for dep in &deps {
            visit(lua, dep, seen, out);
        }
        for dep in &optional {
            let available =
                lookup(lua, dep).is_some_and(|i| !state(lua).addons[i].toc.load_on_demand());
            if available {
                visit(lua, dep, seen, out);
            }
        }
        out.push(name.to_string());
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for n in names {
        visit(lua, n, &mut seen, &mut out);
    }
    out
}

/// Load an addon and its dependencies. Returns the final load state.
pub fn load_addon(lua: &Lua, name: &str) -> LoadState {
    let Some(index) = lookup(lua, name) else {
        return LoadState::Failed("MISSING".into());
    };
    let (addon_name, deps, optional) = {
        let mut st = state(lua);
        let addon = &st.addons[index];
        if addon.load_state != LoadState::NotLoaded {
            return addon.load_state.clone();
        }
        if !addon.toc.loadable_in(st.glue) {
            let failed = LoadState::Failed("INCOMPATIBLE".into());
            st.addons[index].load_state = failed.clone();
            return failed;
        }
        let addon = &st.addons[index];
        (
            addon.name.clone(),
            addon.toc.dependencies(),
            addon.toc.optional_dependencies(),
        )
    };
    state(lua).addons[index].load_state = LoadState::Loading;

    for dep in &deps {
        let result = load_addon(lua, dep);
        if result != LoadState::Loaded && result != LoadState::Loading {
            let reason = if result == LoadState::Failed("MISSING".into()) {
                "DEP_MISSING"
            } else {
                "DEP_FAILED"
            };
            let mut st = state(lua);
            st.warn(format!(
                "{addon_name}: not loaded, dependency {dep} failed ({result:?})"
            ));
            st.addons[index].load_state = LoadState::Failed(reason.into());
            return LoadState::Failed(reason.into());
        }
    }
    for dep in &optional {
        let available =
            lookup(lua, dep).is_some_and(|i| !state(lua).addons[i].toc.load_on_demand());
        if available {
            load_addon(lua, dep);
        }
    }

    let started = std::time::Instant::now();
    run_addon_files(lua, index, false);
    if std::env::var_os("WOWPROGRAMMING_PROFILE").is_some() {
        let ms = started.elapsed().as_millis();
        if ms >= 200 {
            eprintln!("profile: {addon_name} took {ms} ms (with dependencies loaded before)");
        }
    }
    savedvars::load(lua, &addon_name);
    state(lua).addons[index].load_state = LoadState::Loaded;
    if let Ok(args) = (addon_name.as_str(), false).into_lua_multi(lua) {
        dispatch_event(lua, "ADDON_LOADED", args);
    }
    LoadState::Loaded
}

/// Run one file of a Blizzard addon without loading the addon, such as
/// `Blizzard_ActionBar/Shared/ActionButtonUtil.lua`.
pub fn run_blizzard_file(lua: &Lua, file: &str) {
    let Some(ui) = state(lua).ui_source_dir.clone() else {
        return;
    };
    let file = file.replace('\\', "/");
    let display = format!("Interface/AddOns/{file}");
    let addon = file.split('/').next().unwrap_or("").to_string();
    let Some(path) = resolve_path(&ui, &display) else {
        state(lua).warn(format!("Couldn't open {display}"));
        return;
    };
    if file.to_ascii_lowercase().ends_with(".xml") {
        crate::xml::load_file(lua, &path, &display, &addon, None);
        return;
    }
    match std::fs::read(&path) {
        Ok(code) => {
            let ns = lua.create_table().ok();
            run_chunk(lua, &code, &format!("@{display}"), (addon.as_str(), ns));
        }
        Err(err) => state(lua).warn(format!("Couldn't open {display}: {err}")),
    }
}

/// Run the `[Bootstrap]` files of a load-on-demand addon, as the client
/// does at startup.
pub fn run_bootstrap(lua: &Lua, name: &str) {
    let Some(index) = lookup(lua, name) else {
        state(lua).warn(format!("bootstrap: addon {name} not found"));
        return;
    };
    if state(lua).addons[index].bootstrapped {
        return;
    }
    run_addon_files(lua, index, true);
    state(lua).addons[index].bootstrapped = true;
}

fn run_addon_files(lua: &Lua, index: usize, bootstrap_only: bool) {
    let (name, dir, entries, bootstrapped) = {
        let st = state(lua);
        let a = &st.addons[index];
        (
            a.name.clone(),
            a.dir.clone(),
            a.toc.entries.clone(),
            a.bootstrapped,
        )
    };
    let namespace = match state(lua).addons[index].namespace.clone() {
        Some(t) => t,
        None => match lua.create_table() {
            Ok(t) => t,
            Err(_) => return,
        },
    };
    state(lua).addons[index].namespace = Some(namespace.clone());

    for entry in entries {
        if bootstrap_only && !entry.bootstrap || !bootstrap_only && entry.bootstrap && bootstrapped
        {
            continue;
        }
        let file = entry.path;
        let display = format!("Interface/AddOns/{name}/{file}");
        let lower = file.to_ascii_lowercase();
        let Some(path) = resolve_path(&dir, &file) else {
            state(lua).warn(format!("Couldn't open {display}"));
            continue;
        };
        if lower.ends_with(".xml") {
            crate::xml::load_file(lua, &path, &display, &name, Some(namespace.clone()));
            continue;
        }
        if !lower.ends_with(".lua") {
            state(lua).warn(format!("{display}: unknown file type, skipped"));
            continue;
        }
        match std::fs::read(&path) {
            Ok(code) => {
                run_chunk(
                    lua,
                    &code,
                    &format!("@{display}"),
                    (name.as_str(), namespace.clone()),
                );
            }
            Err(err) => state(lua).warn(format!("Couldn't open {display}: {err}")),
        }
    }
}

/// Write the SavedVariables of every loaded addon.
pub fn save_all(lua: &Lua) {
    let to_save: Vec<(String, Vec<String>, Vec<String>)> = {
        let st = state(lua);
        st.addons
            .iter()
            .filter(|a| a.load_state == LoadState::Loaded)
            .map(|a| {
                (
                    a.name.clone(),
                    a.toc.saved_variables(),
                    a.toc.saved_variables_per_character(),
                )
            })
            .collect()
    };
    for (name, account, character) in to_save {
        savedvars::save(lua, &name, &account, &character);
    }
}

/// An addon argument to the `C_AddOns` functions: a name or a 1-based index
/// into the list of user addons.
fn resolve_arg(lua: &Lua, arg: &Value) -> Option<usize> {
    let st = state(lua);
    match arg {
        Value::String(s) => st.find_addon(&s.to_string_lossy()),
        Value::Integer(_) | Value::Number(_) => {
            let n = match arg {
                Value::Integer(i) => *i,
                Value::Number(n) => *n as i64,
                _ => unreachable!(),
            };
            st.user_addons()
                .nth((n - 1).max(0) as usize)
                .filter(|_| n >= 1)
                .map(|(i, _)| i)
        }
        _ => None,
    }
}

pub fn install(lua: &Lua) -> Result<()> {
    let c = lua.create_table()?;
    c.set(
        "GetNumAddOns",
        lua.create_function(|lua, ()| Ok(state(lua).user_addons().count()))?,
    )?;
    c.set(
        "GetAddOnInfo",
        lua.create_function(|lua, arg: Value| {
            let Some(i) = resolve_arg(lua, &arg) else {
                return Ok(MultiValue::new());
            };
            let (name, title, notes, reason) = {
                let st = state(lua);
                let a = &st.addons[i];
                let reason = match &a.load_state {
                    LoadState::Failed(r) => Some(r.clone()),
                    _ if a.toc.load_on_demand() => Some("DEMAND_LOADED".to_string()),
                    _ => None,
                };
                (
                    a.name.clone(),
                    a.toc.title().unwrap_or(&a.name).to_string(),
                    a.toc.get("Notes").map(str::to_string),
                    reason,
                )
            };
            let loadable = reason.as_deref().is_none_or(|r| r == "DEMAND_LOADED");
            (name, title, notes, loadable, reason, "INSECURE", false).into_lua_multi(lua)
        })?,
    )?;
    c.set(
        "GetAddOnMetadata",
        lua.create_function(|lua, (arg, field): (Value, String)| {
            let Some(i) = resolve_arg(lua, &arg) else {
                return Err(mlua::Error::runtime("Invalid AddOn name or index"));
            };
            Ok(state(lua).addons[i].toc.get(&field).map(str::to_string))
        })?,
    )?;
    c.set(
        "IsAddOnLoaded",
        lua.create_function(|lua, arg: Value| {
            let loaded = resolve_arg(lua, &arg)
                .map(|i| state(lua).addons[i].load_state.clone())
                .unwrap_or(LoadState::NotLoaded);
            let finished = loaded == LoadState::Loaded;
            Ok((finished || loaded == LoadState::Loading, finished))
        })?,
    )?;
    c.set(
        "IsAddOnLoadOnDemand",
        lua.create_function(|lua, arg: Value| {
            Ok(resolve_arg(lua, &arg).is_some_and(|i| state(lua).addons[i].toc.load_on_demand()))
        })?,
    )?;
    c.set(
        "DoesAddOnExist",
        lua.create_function(|lua, arg: Value| Ok(resolve_arg(lua, &arg).is_some()))?,
    )?;
    c.set(
        "LoadAddOn",
        lua.create_function(|lua, arg: Value| {
            let name = match resolve_arg(lua, &arg) {
                Some(i) => state(lua).addons[i].name.clone(),
                None => match &arg {
                    Value::String(s) => s.to_string_lossy(),
                    _ => return (false, "MISSING").into_lua_multi(lua),
                },
            };
            match load_addon(lua, &name) {
                LoadState::Loaded | LoadState::Loading => true.into_lua_multi(lua),
                LoadState::Failed(reason) => (false, reason).into_lua_multi(lua),
                LoadState::NotLoaded => (false, "MISSING").into_lua_multi(lua),
            }
        })?,
    )?;
    c.set(
        "GetAddOnEnableState",
        lua.create_function(|lua, (arg, _character): (Value, Value)| {
            Ok(if resolve_arg(lua, &arg).is_some() {
                2
            } else {
                0
            })
        })?,
    )?;
    for name in [
        "EnableAddOn",
        "DisableAddOn",
        "EnableAllAddOns",
        "DisableAllAddOns",
        "SaveAddOns",
        "ResetAddOns",
    ] {
        c.set(name, lua.create_function(|_, _: MultiValue| Ok(()))?)?;
    }
    lua.globals().set("C_AddOns", c)?;
    Ok(())
}
