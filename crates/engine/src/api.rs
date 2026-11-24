//! Hand-written game API functions that the client provides natively and
//! that behave beyond what a generated stub can do: CVars, screen size,
//! atlases, encoding, templates, input state and sounds.
//!
//! Scenario files can replace any of these from Lua.

use std::collections::HashMap;
use std::io::{Read, Write};

use mlua::{IntoLuaMulti, Lua, MultiValue, Result, Table, Value};
use wowprogramming_lua::mlua;

use crate::frames::frame_id;
use crate::state::state;

pub fn install(lua: &Lua) -> Result<()> {
    let g = lua.globals();

    // CVars
    let c_cvar = namespace(lua, "C_CVar")?;
    let get_cvar = lua.create_function(|lua, name: String| Ok(cvar_value(lua, &name)))?;
    let get_cvar_default = lua.create_function(|lua, name: String| {
        let key = name.to_ascii_lowercase();
        let mut st = state(lua);
        let d = st.cvar_defaults.get(&key).cloned();
        if d.is_none() && !st.cvars.contains_key(&key) {
            st.unknown_cvars.insert(name);
        }
        Ok(d)
    })?;
    let set_cvar = lua.create_function(|lua, (name, value): (String, Value)| {
        let v = cvar_string(lua, value)?;
        state(lua).cvars.insert(name.to_ascii_lowercase(), v);
        Ok(true)
    })?;
    let get_cvar_bool = lua.create_function(|lua, name: String| {
        Ok(cvar_value(lua, &name).is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true")))
    })?;
    // RegisterCVar(name, default): the default also becomes the value if
    // the CVar has none yet.
    let register_cvar = lua.create_function(|lua, (name, value): (String, Value)| {
        let v = cvar_string(lua, value)?;
        let key = name.to_ascii_lowercase();
        let mut st = state(lua);
        st.cvar_defaults.insert(key.clone(), v.clone());
        st.cvars.entry(key).or_insert(v);
        st.unknown_cvars.remove(&name);
        Ok(())
    })?;
    // Value, default, stored on account, stored on character, locked,
    // secure, read only.
    let get_cvar_info = lua.create_function(|lua, name: String| {
        let value = cvar_value(lua, &name);
        let Some(value) = value else {
            return Ok(MultiValue::new());
        };
        let default = state(lua)
            .cvar_defaults
            .get(&name.to_ascii_lowercase())
            .cloned()
            .unwrap_or_else(|| value.clone());
        (value, default, false, false, false, false, false).into_lua_multi(lua)
    })?;
    c_cvar.set("GetCVarInfo", get_cvar_info)?;
    for (name, f) in [
        ("GetCVar", &get_cvar),
        ("SetCVar", &set_cvar),
        ("GetCVarBool", &get_cvar_bool),
        ("RegisterCVar", &register_cvar),
        ("GetCVarDefault", &get_cvar_default),
    ] {
        c_cvar.set(name, f.clone())?;
        if g.raw_get::<Value>(name)?.is_nil() {
            g.set(name, f.clone())?;
        }
    }
    let bitfield = lua.create_function(|_, _: MultiValue| Ok(false))?;
    c_cvar.set("GetCVarBitfield", bitfield.clone())?;
    g.set("GetCVarBitfield", bitfield)?;
    let set_bitfield = lua.create_function(|_, _: MultiValue| Ok(true))?;
    c_cvar.set("SetCVarBitfield", set_bitfield.clone())?;
    g.set("SetCVarBitfield", set_bitfield)?;

    // Client and screen
    g.set("GetLocale", lua.create_function(|_, ()| Ok("enUS"))?)?;
    g.set(
        "GetScreenWidth",
        lua.create_function(|lua, ()| Ok(screen_size(lua).0))?,
    )?;
    g.set(
        "GetScreenHeight",
        lua.create_function(|lua, ()| Ok(screen_size(lua).1))?,
    )?;
    g.set(
        "GetPhysicalScreenSize",
        lua.create_function(|lua, ()| Ok(state(lua).screen))?,
    )?;
    g.set("GetScreenDPIScale", lua.create_function(|_, ()| Ok(1.0))?)?;
    g.set("IsMacClient", lua.create_function(|_, ()| Ok(false))?)?;
    g.set("IsWindowsClient", lua.create_function(|_, ()| Ok(true))?)?;
    g.set("IsLinuxClient", lua.create_function(|_, ()| Ok(false))?)?;
    g.set("IsOnGlueScreen", lua.create_function(|_, ()| Ok(false))?)?;
    g.set("IsLoggedIn", lua.create_function(|_, ()| Ok(true))?)?;
    g.set("InCombatLockdown", lua.create_function(|_, ()| Ok(false))?)?;
    g.set(
        "GetBuildInfo",
        lua.create_function(|lua, ()| {
            ("12.1.5", "70961", "Nov 1 2026", 120105, "", "", 120105).into_lua_multi(lua)
        })?,
    )?;

    // Input state
    for name in [
        "IsShiftKeyDown",
        "IsControlKeyDown",
        "IsAltKeyDown",
        "IsModifierKeyDown",
        "IsLeftShiftKeyDown",
        "IsRightShiftKeyDown",
        "IsLeftControlKeyDown",
        "IsRightControlKeyDown",
        "IsLeftAltKeyDown",
        "IsRightAltKeyDown",
        "IsMetaKeyDown",
        "IsModifiedClick",
        "IsMouseButtonDown",
        "CanAutoSetGamePadCursorControl",
        "IsGamePadFreelookEnabled",
        "IsKioskModeEnabled",
        "IsTrialAccount",
        "IsVeteranTrialAccount",
        "IsRestrictedAccount",
        "IsCommunitiesUIDisabledByTrialAccount",
        "HasCompletedAnyAchievement",
        "HasAttachedGlyph",
        "IsSpellValidForPendingGlyph",
        "HasPendingGlyphCast",
        "IsPendingGlyphRemoval",
        "IsCastingGlyph",
        "IsCharacterNewlyBoosted",
        "PetHasSpellbook",
        "InClickBindingMode",
        "IsSpecializationActivateSpell",
        "IsInGroup",
        "IsInRaid",
        "IsPartyWorldPVP",
    ] {
        g.set(name, lua.create_function(|_, _: MultiValue| Ok(false))?)?;
    }
    g.set(
        "GetCursorPosition",
        lua.create_function(|lua, ()| Ok(state(lua).mouse))?,
    )?;
    g.set(
        "GetMouseFoci",
        lua.create_function(|lua, ()| {
            let st = state(lua);
            let t = lua.create_table()?;
            if let Some(f) = st.mouse_focus {
                t.raw_push(st.table(f))?;
            }
            Ok(t)
        })?,
    )?;
    g.set(
        "GetCurrentKeyBoardFocus",
        lua.create_function(|lua, ()| {
            let st = state(lua);
            Ok(st.focus.map(|f| st.table(f)))
        })?,
    )?;

    // Sounds do nothing.
    g.set(
        "PlaySound",
        lua.create_function(|lua, _: MultiValue| (true, 1).into_lua_multi(lua))?,
    )?;
    g.set(
        "PlaySoundFile",
        lua.create_function(|lua, _: MultiValue| (true, 1).into_lua_multi(lua))?,
    )?;
    g.set("StopSound", lua.create_function(|_, _: MultiValue| Ok(()))?)?;

    // Localized text helper from the client.
    g.set(
        "GetText",
        lua.create_function(|lua, (token, gender): (String, Option<i64>)| {
            let g = lua.globals();
            if let Some(gender) = gender {
                let v: Value = g.get(format!("{token}_{gender}"))?;
                if !v.is_nil() {
                    return Ok(v);
                }
            }
            g.get::<Value>(token)
        })?,
    )?;

    g.set(
        "EnumerateFrames",
        lua.create_function(|lua, current: Value| {
            let start = match &current {
                Value::Table(t) => frame_id(t).map(|i| i + 1).unwrap_or(0),
                _ => 0,
            };
            let st = state(lua);
            let next = (start..st.frames.len()).find(|&i| st.frames[i].is_frame());
            Ok(next.map(|i| st.table(i)))
        })?,
    )?;

    lua.load(include_str!("lua/extensions.lua"))
        .set_name("=[C]")
        .exec()?;
    // GetFrameMetatable, GetTextureMetatable and so on.
    let metatables: Vec<(&'static str, Table)> = state(lua)
        .frame_metatables
        .iter()
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    for (kind, mt) in metatables {
        g.set(
            format!("Get{kind}Metatable"),
            lua.create_function(move |_, ()| Ok(mt.clone()))?,
        )?;
    }
    // client.lua receives a function that fires client events.
    let fire = lua.create_function(|lua, args: MultiValue| {
        let mut args = args.into_iter();
        let Some(Value::String(event)) = args.next() else {
            return Ok(());
        };
        let event = event.to_string_lossy();
        crate::frames::dispatch_event(lua, &event, args.collect());
        Ok(())
    })?;
    lua.load(include_str!("lua/client.lua"))
        .set_name("=[C]")
        .call::<()>(fire)?;
    // Undocumented client functions the UI calls at load time.
    g.set(
        "RegisterStaticConstants",
        lua.create_function(|_, _: MultiValue| Ok(()))?,
    )?;
    g.set("GetMaxBattlefieldID", lua.create_function(|_, ()| Ok(0))?)?;
    // Bandwidth in, bandwidth out, home latency, world latency.
    g.set(
        "GetNetStats",
        lua.create_function(|_, ()| Ok((0.0, 0.0, 0, 0)))?,
    )?;
    for name in [
        "ClearOnBarHighlightMarks",
        "UpdateOnBarHighlightMarksBySpell",
        "UpdateOnBarHighlightMarksByFlyout",
        "UpdateOnBarHighlightMarksByPetAction",
        "SetGamePadCursorControl",
    ] {
        g.set(name, lua.create_function(|_, _: MultiValue| Ok(()))?)?;
    }
    for name in ["GetNumSubgroupMembers", "GetNumGroupMembers"] {
        g.set(name, lua.create_function(|_, _: MultiValue| Ok(0))?)?;
    }
    namespace(lua, "C_AdventureJournal")?
        .set("CanBeShown", lua.create_function(|_, ()| Ok(false))?)?;
    for name in [
        "GetGuildLogoInfo",
        "GetPendingGlyphName",
        "GetCurrentGlyphNameForSpell",
    ] {
        g.set(name, lua.create_function(|_, _: MultiValue| Ok(()))?)?;
    }
    // The client's Kiosk namespace (demo kiosks); the sandbox is never one.
    let kiosk = namespace(lua, "Kiosk")?;
    kiosk.set("IsEnabled", lua.create_function(|_, ()| Ok(false))?)?;
    kiosk.set(
        "IsCompetitiveModeEnabled",
        lua.create_function(|_, ()| Ok(false))?,
    )?;
    install_texture_api(lua)?;
    install_encoding_api(lua)?;
    install_xml_util(lua)?;
    Ok(())
}

/// A CVar's value, or its default. Records CVars that have neither.
fn cvar_value(lua: &Lua, name: &str) -> Option<String> {
    let key = name.to_ascii_lowercase();
    let mut st = state(lua);
    if let Some(v) = st.cvars.get(&key).or_else(|| st.cvar_defaults.get(&key)) {
        return Some(v.clone());
    }
    st.unknown_cvars.insert(name.to_string());
    None
}

fn cvar_string(lua: &Lua, value: Value) -> Result<String> {
    Ok(match value {
        Value::Boolean(b) => if b { "1" } else { "0" }.to_string(),
        Value::Nil => String::new(),
        other => lua
            .coerce_string(other)?
            .map(|s| s.to_string_lossy())
            .unwrap_or_default(),
    })
}

/// CVar defaults known without a capture, from `cvar_defaults.txt`
/// (`name value` per line). A capture replaces them.
pub fn builtin_cvar_defaults() -> HashMap<String, String> {
    include_str!("data/cvar_defaults.txt")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (k, v) = l.split_once(char::is_whitespace).unwrap_or((l, ""));
            (k.to_ascii_lowercase(), v.trim().to_string())
        })
        .collect()
}

/// Screen size in UIParent units.
fn screen_size(lua: &Lua) -> (f64, f64) {
    let st = state(lua);
    let scale = st
        .frames
        .iter()
        .find(|f| f.name.as_deref() == Some("UIParent"))
        .map(|f| f.scale)
        .unwrap_or(1.0);
    (st.screen.0 / scale, st.screen.1 / scale)
}

fn namespace(lua: &Lua, name: &str) -> Result<Table> {
    let g = lua.globals();
    match g.get::<Value>(name)? {
        Value::Table(t) => Ok(t),
        _ => {
            let t = lua.create_table()?;
            g.set(name, t.clone())?;
            Ok(t)
        }
    }
}

fn install_texture_api(lua: &Lua) -> Result<()> {
    let c = namespace(lua, "C_Texture")?;
    c.set(
        "GetAtlasInfo",
        lua.create_function(|lua, name: String| {
            let gd = state(lua).gamedata.clone();
            let Some(a) = gd.as_ref().and_then(|g| g.atlas(&name)) else {
                return Ok(Value::Nil);
            };
            let t = lua.create_table()?;
            t.set("elementName", a.name.as_str())?;
            t.set("width", a.width)?;
            t.set("height", a.height)?;
            t.set("leftTexCoord", a.left)?;
            t.set("rightTexCoord", a.right)?;
            t.set("topTexCoord", a.top)?;
            t.set("bottomTexCoord", a.bottom)?;
            t.set("tilesHorizontally", a.tiles_horizontally)?;
            t.set("tilesVertically", a.tiles_vertically)?;
            t.set("file", a.file)?;
            let raw = lua.create_table()?;
            raw.set("x", a.width)?;
            raw.set("y", a.height)?;
            t.set("rawSize", raw)?;
            Ok(Value::Table(t))
        })?,
    )?;
    c.set(
        "GetFilenameFromFileDataID",
        lua.create_function(|_, id: i64| Ok(id.to_string()))?,
    )?;
    lua.globals().set(
        "GetFileIDFromPath",
        lua.create_function(|lua, path: String| {
            let gd = state(lua).gamedata.clone();
            Ok(gd.and_then(|g| g.file_id(&path)))
        })?,
    )?;
    Ok(())
}

fn json_to_lua(lua: &Lua, v: &serde_json::Value) -> Result<Value> {
    Ok(match v {
        serde_json::Value::Null => Value::Nil,
        serde_json::Value::Bool(b) => Value::Boolean(*b),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => Value::Integer(i),
            None => Value::Number(n.as_f64().unwrap_or(0.0)),
        },
        serde_json::Value::String(s) => Value::String(lua.create_string(s)?),
        serde_json::Value::Array(items) => {
            let t = lua.create_table()?;
            for (i, item) in items.iter().enumerate() {
                t.raw_set(i + 1, json_to_lua(lua, item)?)?;
            }
            Value::Table(t)
        }
        serde_json::Value::Object(map) => {
            let t = lua.create_table()?;
            for (k, item) in map {
                t.raw_set(k.as_str(), json_to_lua(lua, item)?)?;
            }
            Value::Table(t)
        }
    })
}

fn lua_to_json(v: &Value, depth: usize) -> Result<serde_json::Value> {
    if depth > 64 {
        return Err(mlua::Error::runtime(
            "SerializeJSON: table nested too deeply",
        ));
    }
    Ok(match v {
        Value::Nil => serde_json::Value::Null,
        Value::Boolean(b) => serde_json::Value::Bool(*b),
        Value::Integer(i) => serde_json::Value::from(*i),
        Value::Number(n) => {
            if n.fract() == 0.0 && n.abs() < 9.0e15 {
                serde_json::Value::from(*n as i64)
            } else {
                serde_json::Value::from(*n)
            }
        }
        Value::String(s) => serde_json::Value::String(s.to_string_lossy()),
        Value::Table(t) => {
            let len = t.raw_len();
            let count = t.pairs::<Value, Value>().count();
            if len > 0 && count == len {
                let mut out = Vec::with_capacity(len);
                for i in 1..=len {
                    out.push(lua_to_json(&t.raw_get::<Value>(i)?, depth + 1)?);
                }
                serde_json::Value::Array(out)
            } else {
                let mut map = serde_json::Map::new();
                for pair in t.pairs::<Value, Value>() {
                    let (k, v) = pair?;
                    let key = match k {
                        Value::String(s) => s.to_string_lossy(),
                        Value::Integer(i) => i.to_string(),
                        Value::Number(n) => n.to_string(),
                        _ => continue,
                    };
                    map.insert(key, lua_to_json(&v, depth + 1)?);
                }
                serde_json::Value::Object(map)
            }
        }
        _ => serde_json::Value::Null,
    })
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' | b'\t' => continue,
            _ => return None,
        };
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

/// Decompress data that may be raw deflate, zlib or gzip.
pub fn decompress_any(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    if flate2::read::ZlibDecoder::new(data)
        .read_to_end(&mut out)
        .is_ok()
    {
        return Some(out);
    }
    out.clear();
    if flate2::read::DeflateDecoder::new(data)
        .read_to_end(&mut out)
        .is_ok()
    {
        return Some(out);
    }
    out.clear();
    if flate2::read::GzDecoder::new(data)
        .read_to_end(&mut out)
        .is_ok()
    {
        return Some(out);
    }
    None
}

fn install_encoding_api(lua: &Lua) -> Result<()> {
    let c = namespace(lua, "C_EncodingUtil")?;
    c.set(
        "SerializeJSON",
        lua.create_function(|_, v: Value| {
            serde_json::to_string(&lua_to_json(&v, 0)?).map_err(mlua::Error::external)
        })?,
    )?;
    c.set(
        "DeserializeJSON",
        lua.create_function(|lua, s: mlua::LuaString| {
            let v: serde_json::Value =
                serde_json::from_slice(&s.as_bytes()).map_err(mlua::Error::external)?;
            json_to_lua(lua, &v)
        })?,
    )?;
    c.set(
        "EncodeBase64",
        lua.create_function(|_, s: mlua::LuaString| Ok(base64_encode(&s.as_bytes())))?,
    )?;
    c.set(
        "DecodeBase64",
        lua.create_function(|lua, s: String| match base64_decode(&s) {
            Some(b) => Ok(Value::String(lua.create_string(b)?)),
            None => Ok(Value::Nil),
        })?,
    )?;
    c.set(
        "CompressString",
        lua.create_function(|lua, (s, method): (mlua::LuaString, Option<i64>)| {
            let data = s.as_bytes();
            let out = match method.unwrap_or(0) {
                1 => {
                    let mut z =
                        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                    z.write_all(&data).map_err(mlua::Error::external)?;
                    z.finish().map_err(mlua::Error::external)?
                }
                2 => {
                    let mut z =
                        flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                    z.write_all(&data).map_err(mlua::Error::external)?;
                    z.finish().map_err(mlua::Error::external)?
                }
                _ => {
                    let mut z = flate2::write::DeflateEncoder::new(
                        Vec::new(),
                        flate2::Compression::default(),
                    );
                    z.write_all(&data).map_err(mlua::Error::external)?;
                    z.finish().map_err(mlua::Error::external)?
                }
            };
            lua.create_string(out)
        })?,
    )?;
    c.set(
        "DecompressString",
        lua.create_function(
            |lua, s: mlua::LuaString| match decompress_any(&s.as_bytes()) {
                Some(b) => Ok(Value::String(lua.create_string(b)?)),
                None => Ok(Value::Nil),
            },
        )?,
    )?;
    Ok(())
}

fn install_xml_util(lua: &Lua) -> Result<()> {
    let c = namespace(lua, "C_XMLUtil")?;
    c.set(
        "GetTemplateInfo",
        lua.create_function(|lua, name: String| {
            let Some(info) = crate::xml::template_details(lua, &name) else {
                return Ok(Value::Nil);
            };
            let t = lua.create_table()?;
            t.set("type", info.kind)?;
            t.set("width", info.width)?;
            t.set("height", info.height)?;
            t.set("inherits", info.inherits)?;
            t.set("sourceLocation", info.source)?;
            let kvs = lua.create_table()?;
            for (key, key_type, value_type, value) in info.key_values {
                let kv = lua.create_table()?;
                kv.set("key", key)?;
                kv.set("keyType", key_type)?;
                kv.set("type", value_type)?;
                kv.set("value", value)?;
                kvs.raw_push(kv)?;
            }
            t.set("keyValues", kvs)?;
            Ok(Value::Table(t))
        })?,
    )?;
    c.set(
        "GetTemplates",
        lua.create_function(|lua, ()| lua.create_table())?,
    )?;
    Ok(())
}
