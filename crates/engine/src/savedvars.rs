//! SavedVariables: read before `ADDON_LOADED`, written on logout and reload.

use std::collections::HashSet;
use std::ffi::c_void;
use std::path::PathBuf;

use mlua::{Lua, Table, Value};
use wowprogramming_lua::mlua;

use crate::natives::run_chunk;
use crate::state::{WtfPaths, state};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Account,
    Character,
}

pub fn file_path(wtf: &WtfPaths, scope: Scope, addon: &str) -> PathBuf {
    let account = wtf.root.join("Account").join(&wtf.account);
    let dir = match scope {
        Scope::Account => account,
        Scope::Character => account.join(&wtf.realm).join(&wtf.character),
    };
    dir.join("SavedVariables").join(format!("{addon}.lua"))
}

fn chunk_name(wtf: &WtfPaths, path: &std::path::Path) -> String {
    let rel = path.strip_prefix(&wtf.root).unwrap_or(path);
    format!("@WTF/{}", rel.to_string_lossy().replace('\\', "/"))
}

/// Run the SavedVariables files of an addon, if they exist.
pub fn load(lua: &Lua, addon: &str) {
    let wtf = state(lua).wtf.clone();
    for scope in [Scope::Account, Scope::Character] {
        let path = file_path(&wtf, scope, addon);
        if let Ok(code) = std::fs::read(&path) {
            run_chunk(lua, &code, &chunk_name(&wtf, &path), ());
        }
    }
}

/// Write the SavedVariables files of an addon.
pub fn save(lua: &Lua, addon: &str, account: &[String], character: &[String]) {
    let wtf = state(lua).wtf.clone();
    for (scope, vars) in [(Scope::Account, account), (Scope::Character, character)] {
        if vars.is_empty() {
            continue;
        }
        let mut out = Vec::new();
        for var in vars {
            let value: Value = lua.globals().get(var.as_str()).unwrap_or(Value::Nil);
            out.extend_from_slice(var.as_bytes());
            out.extend_from_slice(b" = ");
            let mut seen = HashSet::new();
            if !write_value(&mut out, &value, 0, &mut seen) {
                out.extend_from_slice(b"nil");
            }
            out.push(b'\n');
        }
        let path = file_path(&wtf, scope, addon);
        let result = path
            .parent()
            .map(std::fs::create_dir_all)
            .unwrap_or(Ok(()))
            .and_then(|_| std::fs::write(&path, &out));
        if let Err(err) = result {
            state(lua).warn(format!("cannot write {}: {err}", path.display()));
        }
    }
}

/// Serialize a value as Lua source. Returns false for values that cannot
/// be saved (functions, userdata, threads, frames, repeated tables).
pub fn write_value(
    out: &mut Vec<u8>,
    value: &Value,
    depth: usize,
    seen: &mut HashSet<*const c_void>,
) -> bool {
    match value {
        Value::Nil => out.extend_from_slice(b"nil"),
        Value::Boolean(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Value::Integer(i) => out.extend_from_slice(i.to_string().as_bytes()),
        Value::Number(n) => out.extend_from_slice(format_number(*n).as_bytes()),
        Value::String(s) => write_string(out, &s.as_bytes()),
        Value::Table(t) => return write_table(out, t, depth, seen),
        _ => return false,
    }
    true
}

fn write_table(
    out: &mut Vec<u8>,
    t: &Table,
    depth: usize,
    seen: &mut HashSet<*const c_void>,
) -> bool {
    if matches!(
        t.raw_get::<Value>(0),
        Ok(Value::UserData(_) | Value::LightUserData(_))
    ) {
        return false; // a frame or other native object
    }
    if !seen.insert(t.to_pointer()) {
        return false;
    }
    let mut array = Vec::new();
    let mut i = 1;
    while let Ok(v) = t.raw_get::<Value>(i) {
        if v.is_nil() {
            break;
        }
        array.push(v);
        i += 1;
    }
    let array_len = array.len() as i64;
    let mut hash: Vec<(Value, Value)> = t
        .pairs::<Value, Value>()
        .filter_map(|p| p.ok())
        .filter(|(k, _)| !is_array_key(k, array_len))
        .collect();
    hash.sort_by_key(|(a, _)| key_order(a));

    let indent = "\t".repeat(depth + 1);
    out.extend_from_slice(b"{\n");
    for (k, v) in hash {
        let mut entry = Vec::new();
        entry.extend_from_slice(indent.as_bytes());
        entry.push(b'[');
        if !write_value(&mut entry, &k, depth + 1, seen) {
            continue;
        }
        entry.extend_from_slice(b"] = ");
        if write_value(&mut entry, &v, depth + 1, seen) {
            out.extend_from_slice(&entry);
            out.extend_from_slice(b",\n");
        }
    }
    for (n, v) in array.iter().enumerate() {
        let mut entry = indent.as_bytes().to_vec();
        if !write_value(&mut entry, v, depth + 1, seen) {
            entry.extend_from_slice(b"nil");
        }
        out.extend_from_slice(&entry);
        out.extend_from_slice(format!(", -- [{}]\n", n + 1).as_bytes());
    }
    out.extend_from_slice("\t".repeat(depth).as_bytes());
    out.push(b'}');
    seen.remove(&t.to_pointer());
    true
}

fn is_array_key(k: &Value, len: i64) -> bool {
    match k {
        Value::Integer(i) => *i >= 1 && *i <= len,
        Value::Number(n) => n.fract() == 0.0 && *n >= 1.0 && *n <= len as f64,
        _ => false,
    }
}

/// Sort order for keys: numbers, then strings, then booleans.
fn key_order(k: &Value) -> (u8, String) {
    match k {
        Value::Integer(i) => (0, format!("{:020}", i + (1 << 62))),
        Value::Number(n) => (0, format!("{:030.10}", n + 1e18)),
        Value::String(s) => (1, s.to_string_lossy()),
        Value::Boolean(b) => (2, b.to_string()),
        _ => (3, String::new()),
    }
}

fn format_number(n: f64) -> String {
    if n.is_nan() {
        "0/0".to_string()
    } else if n.is_infinite() {
        if n > 0.0 { "math.huge" } else { "-math.huge" }.to_string()
    } else if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn write_string(out: &mut Vec<u8>, s: &[u8]) {
    out.push(b'"');
    for (i, &b) in s.iter().enumerate() {
        match b {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            0..=31 | 127 => {
                // Pad to three digits if a digit follows, so it reads back.
                let next_is_digit = s.get(i + 1).is_some_and(u8::is_ascii_digit);
                if next_is_digit {
                    out.extend_from_slice(format!("\\{b:03}").as_bytes());
                } else {
                    out.extend_from_slice(format!("\\{b}").as_bytes());
                }
            }
            _ => out.push(b),
        }
    }
    out.push(b'"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn serialize(lua: &Lua, code: &str) -> String {
        let v: Value = lua.load(code).eval().unwrap();
        let mut out = Vec::new();
        assert!(write_value(&mut out, &v, 0, &mut HashSet::new()));
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn writes_nested_tables_in_stable_order() {
        let lua = Lua::new();
        let text = serialize(
            &lua,
            r#"return { b = true, a = "x\n\"y\"", 10, 20, [5] = 0.5, sub = { 1 }, f = print }"#,
        );
        assert_eq!(
            text,
            "{\n\t[5] = 0.5,\n\t[\"a\"] = \"x\\n\\\"y\\\"\",\n\t[\"b\"] = true,\n\t[\"sub\"] = {\n\t\t1, -- [1]\n\t},\n\t10, -- [1]\n\t20, -- [2]\n}"
        );
    }

    #[test]
    fn round_trips_through_lua() {
        let lua = Lua::new();
        let text = serialize(
            &lua,
            r#"return { n = 1/3, big = 2^40, neg = -7, s = "a\0001\1b", list = { "p", "q" } }"#,
        );
        let back: Table = lua.load(format!("return {text}")).eval().unwrap();
        assert_eq!(back.get::<f64>("n").unwrap(), 1.0 / 3.0);
        assert_eq!(back.get::<f64>("big").unwrap(), 2f64.powi(40));
        assert_eq!(back.get::<f64>("neg").unwrap(), -7.0);
        assert_eq!(
            back.get::<mlua::LuaString>("s")
                .unwrap()
                .as_bytes()
                .as_ref(),
            b"a\x001\x01b"
        );
        let list: Table = back.get("list").unwrap();
        assert_eq!(list.raw_len(), 2);
    }

    #[test]
    fn skips_cycles() {
        let lua = Lua::new();
        let text = serialize(&lua, "local t = { x = 1 } t.self = t return t");
        assert_eq!(text, "{\n\t[\"x\"] = 1,\n}");
    }
}
