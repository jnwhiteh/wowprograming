//! Functions that the WoW client adds to the Lua standard library.
//!
//! Only functions implemented natively by the client belong here. Aliases
//! such as `tinsert` or `strtrim` are defined by Blizzard's own
//! `Compat.lua`, which the sandbox loads from the UI source.

use mlua::{Function, Lua, LuaString, MultiValue, Result, Table, Value, Variadic};

/// Install all native stdlib additions into the globals of `lua`.
pub fn install(lua: &Lua) -> Result<()> {
    install_string(lua)?;
    install_table(lua)?;
    install_bit(lua)?;
    install_mixins(lua)?;
    install_debug(lua)?;
    Ok(())
}

const DEFAULT_TRIM_CHARS: &[u8] = b" \t\r\n";

fn install_string(lua: &Lua) -> Result<()> {
    let globals = lua.globals();
    let string: Table = globals.get("string")?;

    let strsplit = lua.create_function(
        |lua, (delims, subject, pieces): (LuaString, LuaString, Option<i64>)| {
            split(lua, &delims.as_bytes(), &subject.as_bytes(), pieces)
        },
    )?;
    globals.set("strsplit", strsplit.clone())?;
    string.set("split", strsplit)?;

    let strjoin = lua.create_function(|lua, (sep, parts): (LuaString, Variadic<Value>)| {
        let sep = sep.as_bytes().to_vec();
        let mut out = Vec::new();
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                out.extend_from_slice(&sep);
            }
            out.extend_from_slice(&coerce_string(lua, part, "strjoin", i + 2)?);
        }
        lua.create_string(out)
    })?;
    globals.set("strjoin", strjoin.clone())?;
    string.set("join", strjoin)?;

    let strconcat = lua.create_function(|lua, parts: Variadic<Value>| {
        let mut out = Vec::new();
        for (i, part) in parts.iter().enumerate() {
            out.extend_from_slice(&coerce_string(lua, part, "strconcat", i + 1)?);
        }
        lua.create_string(out)
    })?;
    globals.set("strconcat", strconcat.clone())?;
    string.set("concat", strconcat)?;

    let trim = lua.create_function(|lua, (s, chars): (LuaString, Option<LuaString>)| {
        let chars = chars.map(|c| c.as_bytes().to_vec());
        let chars = chars.as_deref().unwrap_or(DEFAULT_TRIM_CHARS);
        let bytes = s.as_bytes();
        let start = bytes.iter().position(|b| !chars.contains(b));
        let out = match start {
            None => &bytes[0..0],
            Some(start) => {
                let end = bytes.iter().rposition(|b| !chars.contains(b)).unwrap();
                &bytes[start..=end]
            }
        };
        lua.create_string(out)
    })?;
    string.set("trim", trim)?;

    let tostringall = lua.create_function(|lua, values: Variadic<Value>| {
        let tostring: Function = lua.globals().get("tostring")?;
        let mut out = Variadic::new();
        for v in values.iter() {
            out.push(tostring.call::<Value>(v.clone())?);
        }
        Ok(out)
    })?;
    globals.set("tostringall", tostringall)?;

    // The client's string.format also takes positional arguments ("%2$s"),
    // which localized strings use to reorder words.
    let original: Function = string.get("format")?;
    let format = lua.create_function(move |lua, (fmt, args): (LuaString, MultiValue)| {
        let Some((rewritten, order)) = rewrite_positional(&fmt.as_bytes()) else {
            return original.call::<MultiValue>((fmt, args));
        };
        let args: Vec<Value> = args.into_iter().collect();
        let mut call = MultiValue::new();
        call.push_back(Value::String(lua.create_string(&rewritten)?));
        for i in order {
            call.push_back(args.get(i).cloned().unwrap_or(Value::Nil));
        }
        original.call::<MultiValue>(call)
    })?;
    string.set("format", format)?;

    Ok(())
}

/// Rewrite a format string with positional specifiers (`%2$s`) into plain
/// ones, with the argument index (from 0) that each specifier takes.
/// Returns `None` when the string has no positional specifier.
fn rewrite_positional(fmt: &[u8]) -> Option<(Vec<u8>, Vec<usize>)> {
    let mut out = Vec::with_capacity(fmt.len());
    let mut order = Vec::new();
    let mut next = 0usize;
    let mut positional = false;
    let mut i = 0;
    while i < fmt.len() {
        let b = fmt[i];
        out.push(b);
        i += 1;
        if b != b'%' {
            continue;
        }
        if fmt.get(i) == Some(&b'%') {
            out.push(b'%');
            i += 1;
            continue;
        }
        // An optional "<digits>$" position.
        let digits_end = fmt[i..]
            .iter()
            .position(|c| !c.is_ascii_digit())
            .map_or(fmt.len(), |p| i + p);
        if digits_end > i && fmt.get(digits_end) == Some(&b'$') {
            let n: usize = std::str::from_utf8(&fmt[i..digits_end])
                .ok()?
                .parse()
                .ok()?;
            order.push(n.checked_sub(1)?);
            positional = true;
            i = digits_end + 1;
        } else {
            order.push(next);
            next += 1;
        }
        // Copy flags, width, precision and the conversion letter.
        while i < fmt.len() {
            let c = fmt[i];
            out.push(c);
            i += 1;
            if c.is_ascii_alphabetic() {
                break;
            }
        }
    }
    positional.then_some((out, order))
}

/// Split `subject` on any byte in `delims`, like the client's `strsplit`.
fn split(lua: &Lua, delims: &[u8], subject: &[u8], pieces: Option<i64>) -> Result<MultiValue> {
    let limit = pieces.filter(|&p| p > 0).map(|p| p as usize);
    let mut out = MultiValue::new();
    let mut start = 0;
    for (i, b) in subject.iter().enumerate() {
        if limit.is_some_and(|l| out.len() + 1 >= l) {
            break;
        }
        if delims.contains(b) {
            out.push_back(Value::String(lua.create_string(&subject[start..i])?));
            start = i + 1;
        }
    }
    out.push_back(Value::String(lua.create_string(&subject[start..])?));
    Ok(out)
}

fn coerce_string(lua: &Lua, v: &Value, func: &str, arg: usize) -> Result<Vec<u8>> {
    match v {
        Value::String(s) => Ok(s.as_bytes().to_vec()),
        Value::Integer(_) | Value::Number(_) => {
            let s: LuaString = lua.coerce_string(v.clone())?.unwrap();
            Ok(s.as_bytes().to_vec())
        }
        other => Err(mlua::Error::runtime(format!(
            "bad argument #{arg} to '{func}' (string expected, got {})",
            other.type_name()
        ))),
    }
}

fn install_table(lua: &Lua) -> Result<()> {
    let table: Table = lua.globals().get("table")?;
    table.set(
        "wipe",
        lua.create_function(|_, t: Table| {
            t.clear()?;
            Ok(t)
        })?,
    )?;
    Ok(())
}

/// The client's `bit` library works on unsigned 32-bit integers.
fn install_bit(lua: &Lua) -> Result<()> {
    fn to_u32(n: f64) -> u32 {
        (n as i64) as u32
    }
    let bit = lua.create_table()?;
    bit.set(
        "band",
        lua.create_function(|_, args: Variadic<f64>| {
            Ok(args.iter().fold(u32::MAX, |acc, &n| acc & to_u32(n)) as f64)
        })?,
    )?;
    bit.set(
        "bor",
        lua.create_function(|_, args: Variadic<f64>| {
            Ok(args.iter().fold(0u32, |acc, &n| acc | to_u32(n)) as f64)
        })?,
    )?;
    bit.set(
        "bxor",
        lua.create_function(|_, args: Variadic<f64>| {
            Ok(args.iter().fold(0u32, |acc, &n| acc ^ to_u32(n)) as f64)
        })?,
    )?;
    bit.set(
        "bnot",
        lua.create_function(|_, n: f64| Ok(!to_u32(n) as f64))?,
    )?;
    bit.set(
        "lshift",
        lua.create_function(|_, (n, s): (f64, u32)| Ok(to_u32(n).wrapping_shl(s) as f64))?,
    )?;
    bit.set(
        "rshift",
        lua.create_function(|_, (n, s): (f64, u32)| Ok(to_u32(n).wrapping_shr(s) as f64))?,
    )?;
    bit.set(
        "arshift",
        lua.create_function(|_, (n, s): (f64, u32)| {
            Ok(((to_u32(n) as i32).wrapping_shr(s)) as u32 as f64)
        })?,
    )?;
    bit.set(
        "mod",
        lua.create_function(|_, (a, b): (f64, f64)| {
            let b = to_u32(b);
            if b == 0 {
                return Err(mlua::Error::runtime("bit.mod: division by zero"));
            }
            Ok((to_u32(a) % b) as f64)
        })?,
    )?;
    lua.globals().set("bit", bit)?;
    Ok(())
}

/// `Mixin` and `CreateFromMixins` are native in current Retail clients.
fn install_mixins(lua: &Lua) -> Result<()> {
    fn mix(object: &Table, mixins: &Variadic<Table>) -> Result<()> {
        for mixin in mixins.iter() {
            for pair in mixin.pairs::<Value, Value>() {
                let (k, v) = pair?;
                object.set(k, v)?;
            }
        }
        Ok(())
    }
    let globals = lua.globals();
    globals.set(
        "Mixin",
        lua.create_function(|_, (object, mixins): (Table, Variadic<Table>)| {
            mix(&object, &mixins)?;
            Ok(object)
        })?,
    )?;
    globals.set(
        "CreateFromMixins",
        lua.create_function(|lua, mixins: Variadic<Table>| {
            let object = lua.create_table()?;
            mix(&object, &mixins)?;
            Ok(object)
        })?,
    )?;
    Ok(())
}

fn install_debug(lua: &Lua) -> Result<()> {
    let globals = lua.globals();
    globals.set(
        "debugstack",
        lua.create_function(
            |lua, (start, top, bottom): (Option<i64>, Option<i64>, Option<i64>)| {
                // Levels below 0 count as 0 (BugGrabber can compute -1).
                let n = |v: Option<i64>, d: i64| v.unwrap_or(d).max(0) as usize;
                Ok(debugstack(lua, n(start, 1), n(top, 12), n(bottom, 10)))
            },
        )?,
    )?;
    Ok(())
}

/// Chunk name of the engine's dispatcher, which calls Lua code on behalf
/// of the engine. Stack traces stop at this boundary.
pub const DISPATCH_CHUNK: &str = "=[dispatch]";

/// How a chunk name appears in messages: `@file` as `file`, `=name` as
/// `name`, and source code as `[string "first line"]`.
pub fn chunk_display(source: &str) -> String {
    if let Some(rest) = source
        .strip_prefix('@')
        .or_else(|| source.strip_prefix('='))
    {
        return rest.to_string();
    }
    let first = source.lines().next().unwrap_or("");
    if first.len() < source.len() {
        format!("[string \"{first}...\"]")
    } else {
        format!("[string \"{first}\"]")
    }
}

/// Build a stack trace in the client's `debugstack` format. Level 1 is the
/// function that called `debugstack`. The trace stops where the engine
/// called into Lua.
pub fn debugstack(lua: &Lua, start: usize, top: usize, bottom: usize) -> String {
    let mut lines = Vec::new();
    let mut level = start;
    while let Some((line, boundary)) = lua.inspect_stack(level, |d| {
        let boundary = d.source().source.as_deref() == Some(DISPATCH_CHUNK);
        (describe_frame(d), boundary)
    }) {
        if boundary {
            // Drop the native and tail-call frames between the dispatcher
            // and the Lua code it called.
            while lines
                .last()
                .is_some_and(|l: &String| l.starts_with("[C]") || l.starts_with("(tail call)"))
            {
                lines.pop();
            }
            break;
        }
        lines.push(line);
        level += 1;
    }
    if lines.len() > top + bottom {
        let tail = lines.split_off(lines.len() - bottom);
        lines.truncate(top);
        lines.push("...".to_string());
        lines.extend(tail);
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn describe_frame(debug: &mlua::debug::Debug) -> String {
    let source = debug.source();
    let names = debug.names();
    let src = match source.source.as_deref() {
        Some("=(tail call)") => return "(tail call): ?".to_string(),
        // Engine helpers written in Lua show as native code.
        Some("=[C]") if source.what != "C" => return "[C]: ?".to_string(),
        Some(s) => chunk_display(s),
        None => "?".to_string(),
    };
    let location = match debug.current_line() {
        Some(line) => format!("{src}:{line}:"),
        None => format!("{src}:"),
    };
    match names.name.as_deref() {
        Some(name) => format!("{location} in function `{name}'"),
        None if source.what == "main" => format!("{location} in main chunk"),
        None => match source.line_defined {
            Some(line) if line > 0 => format!("{location} in function <{src}:{line}>"),
            _ => format!("{location} in function <?>"),
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::Runtime;

    fn check(code: &str) {
        let rt = Runtime::new().unwrap();
        if let Err(err) = rt.exec(code, "=test") {
            panic!("{err}");
        }
    }

    #[test]
    fn strsplit_splits_on_any_delimiter() {
        check(
            r##"
            local a, b, c, d = strsplit(",;", "x,y;z")
            assert(a == "x" and b == "y" and c == "z" and d == nil)
            assert(select("#", strsplit(",", "a,,b")) == 3)
            assert(select("#", strsplit(",", "")) == 1)
            local p, q = strsplit(" ", "a b c", 2)
            assert(p == "a" and q == "b c", q)
            assert(string.split("=", "k=v") == "k")
            "##,
        );
    }

    #[test]
    fn format_takes_positional_arguments() {
        check(
            r#"
            assert(string.format("%2$s %1$s", "a", "b") == "b a")
            assert(string.format("%1$d%%-%1$d", 7) == "7%-7")
            assert(("%s=%d"):format("x", 3) == "x=3")
            assert(string.format("%2$5.1f|%1$s", "n", 2.5) == "  2.5|n")
            "#,
        );
    }

    #[test]
    fn strjoin_and_strconcat() {
        check(
            r##"
            assert(strjoin(", ", "a", "b", 3) == "a, b, 3")
            assert(strjoin("-") == "")
            assert(strconcat("a", 1, "b") == "a1b")
            assert(not pcall(strjoin, ",", {}))
            "##,
        );
    }

    #[test]
    fn trim_uses_whitespace_or_given_chars() {
        check(
            r##"
            assert(string.trim("  hi \n") == "hi")
            assert(string.trim("xxhixx", "x") == "hi")
            assert(string.trim("   ") == "")
            "##,
        );
    }

    #[test]
    fn wipe_clears_and_returns_table() {
        check(
            r##"
            local t = { 1, 2, a = 3 }
            assert(table.wipe(t) == t)
            assert(next(t) == nil)
            "##,
        );
    }

    #[test]
    fn bit_is_unsigned_32_bit() {
        check(
            r##"
            assert(bit.band(0xff, 0x0f) == 0x0f)
            assert(bit.bor(1, 2, 4) == 7)
            assert(bit.bxor(3, 1) == 2)
            assert(bit.bnot(0) == 4294967295)
            assert(bit.lshift(1, 4) == 16)
            assert(bit.rshift(256, 4) == 16)
            assert(bit.arshift(0x80000000, 4) == 0xf8000000)
            assert(bit.mod(10, 3) == 1)
            "##,
        );
    }

    #[test]
    fn mixins_copy_fields() {
        check(
            r##"
            local A, B = { a = 1 }, { b = 2 }
            local o = CreateFromMixins(A, B)
            assert(o.a == 1 and o.b == 2)
            local t = {}
            assert(Mixin(t, A) == t and t.a == 1)
            "##,
        );
    }

    #[test]
    fn debugstack_names_the_caller() {
        let rt = Runtime::new().unwrap();
        rt.exec(
            "function Outer() local s = debugstack() return s end",
            "@Interface/AddOns/Test/Test.lua",
        )
        .unwrap();
        let stack: String = rt.lua().load("local s = Outer() return s").eval().unwrap();
        assert!(
            stack.starts_with("Interface/AddOns/Test/Test.lua:1: in function `Outer'"),
            "{stack}"
        );
    }
}
