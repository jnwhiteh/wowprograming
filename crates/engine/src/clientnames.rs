//! Undocumented native functions of the game client.
//!
//! The API documentation does not list many older global functions, such
//! as `GetChatWindowInfo` or `GetNumClasses`. They are found at startup:
//! names that the UI source calls as global functions, that no UI file
//! defines, and that appear as strings in the client executable. The list
//! is computed from the local install and cached in the user's cache
//! folder, never in the repository. Namespace functions ("C_X.Func") are
//! found the same way, for namespaces the documentation leaves out (the
//! secure environment's, for example).

use std::collections::HashSet;
use std::path::Path;

const KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local",
    "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Identifier-like strings of at least four characters in a binary file.
pub fn exe_identifiers(path: &Path) -> HashSet<String> {
    let Ok(bytes) = std::fs::read(path) else {
        return HashSet::new();
    };
    let mut out = HashSet::new();
    let mut start = None;
    for (i, &b) in bytes.iter().enumerate() {
        match (start, is_ident(b)) {
            (None, true) if is_ident_start(b) => start = Some(i),
            (Some(s), false) => {
                // Only whole strings: the run must end at a NUL byte.
                if b == 0 && i - s >= 4 {
                    out.insert(String::from_utf8_lossy(&bytes[s..i]).into_owned());
                }
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// Global names the UI calls as functions, and global names it defines.
#[derive(Default)]
pub struct UiGlobals {
    pub called: HashSet<String>,
    pub defined: HashSet<String>,
    /// Calls of namespace functions, as "C_Namespace.Function".
    pub called_in_namespace: HashSet<String>,
}

fn scan_lua(text: &[u8], g: &mut UiGlobals) {
    // Definitions: "function Name(" and "Name =" at the start of a line.
    for line in text.split(|&b| b == b'\n') {
        let mut l = line.trim_ascii_start();
        if let Some(rest) = l.strip_prefix(b"local ") {
            l = rest.trim_ascii_start();
        }
        let is_function = l.starts_with(b"function ");
        if is_function {
            l = l[9..].trim_ascii_start();
        }
        let n = l.iter().take_while(|&&b| is_ident(b)).count();
        if n == 0 || !is_ident_start(l[0]) {
            continue;
        }
        let rest = l[n..].trim_ascii_start();
        let defines = if is_function {
            rest.starts_with(b"(")
        } else {
            rest.starts_with(b"=") && !rest.starts_with(b"==")
        };
        if defines {
            g.defined
                .insert(String::from_utf8_lossy(&l[..n]).into_owned());
        }
    }
    // Calls: an identifier not after '.', ':' or another identifier
    // character, followed by '('.
    let mut i = 0;
    while i < text.len() {
        let b = text[i];
        if is_ident_start(b)
            && (i == 0 || !(is_ident(text[i - 1]) || matches!(text[i - 1], b'.' | b':')))
        {
            let s = i;
            while i < text.len() && is_ident(text[i]) {
                i += 1;
            }
            let mut j = i;
            while j < text.len() && (text[j] == b' ' || text[j] == b'\t') {
                j += 1;
            }
            if j < text.len() && text[j] == b'(' {
                g.called
                    .insert(String::from_utf8_lossy(&text[s..i]).into_owned());
            } else if text[s..i].starts_with(b"C_") && i < text.len() && text[i] == b'.' {
                // C_Namespace.Function(
                let f0 = i + 1;
                let mut f1 = f0;
                while f1 < text.len() && is_ident(text[f1]) {
                    f1 += 1;
                }
                let mut k = f1;
                while k < text.len() && (text[k] == b' ' || text[k] == b'\t') {
                    k += 1;
                }
                if f1 > f0 && k < text.len() && text[k] == b'(' {
                    g.called_in_namespace
                        .insert(String::from_utf8_lossy(&text[s..f1]).into_owned());
                }
            }
        } else {
            i += 1;
        }
    }
}

fn scan_xml(text: &[u8], g: &mut UiGlobals) {
    let text = String::from_utf8_lossy(text);
    for (attr, set) in [("function=\"", true), ("name=\"", false)] {
        for part in text.split(attr).skip(1) {
            let name: String = part.chars().take_while(|c| *c != '"').collect();
            if !name.is_empty() && name.bytes().all(is_ident) {
                if set {
                    g.called.insert(name);
                } else {
                    g.defined.insert(name);
                }
            }
        }
    }
}

/// Scan all Lua and XML files of the UI source, except the API
/// documentation.
pub fn ui_globals(ui_source: &Path) -> UiGlobals {
    let mut g = UiGlobals::default();
    let mut stack = vec![ui_source.join("Interface").join("AddOns")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            if path.is_dir() {
                if name != "blizzard_apidocumentationgenerated" {
                    stack.push(path);
                }
            } else if name.ends_with(".lua") {
                if let Ok(t) = std::fs::read(&path) {
                    scan_lua(&t, &mut g);
                }
            } else if name.ends_with(".xml")
                && let Ok(t) = std::fs::read(&path)
            {
                scan_xml(&t, &mut g);
            }
        }
    }
    g
}

/// A key that changes when the executable or the UI source changes: the
/// executable's size and modification time, and the UI source's commit.
fn cache_key(exe: &Path, ui_source: &Path) -> Option<String> {
    let meta = std::fs::metadata(exe).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let git = ui_source.join(".git");
    let head = std::fs::read_to_string(git.join("HEAD")).unwrap_or_default();
    let commit = match head.trim().strip_prefix("ref: ") {
        Some(r) => std::fs::read_to_string(git.join(r)).unwrap_or_else(|_| {
            // Packed refs.
            std::fs::read_to_string(git.join("packed-refs"))
                .unwrap_or_default()
                .lines()
                .find(|l| l.ends_with(r))
                .unwrap_or("")
                .to_string()
        }),
        None => head,
    };
    Some(format!(
        "{} {} {} {}",
        meta.len(),
        mtime,
        ui_source.display(),
        commit.trim()
    ))
}

/// The user's cache folder for the sandbox.
fn cache_dir() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from))
        .or_else(|| {
            let home = std::path::PathBuf::from(std::env::var_os("HOME")?);
            Some(if cfg!(target_os = "macos") {
                home.join("Library").join("Caches")
            } else {
                home.join(".cache")
            })
        })?;
    Some(base.join("wowprogramming"))
}

/// Undocumented native function names, sorted. The result is cached in
/// the user's cache folder, because scanning the UI source is slow on some
/// file systems.
pub fn undocumented_natives(exe: &Path, ui_source: &Path) -> Vec<String> {
    let key = cache_key(exe, ui_source);
    let file = cache_dir().map(|d| d.join("client_natives.txt"));
    if let (Some(key), Some(file)) = (&key, &file)
        && let Ok(text) = std::fs::read_to_string(file)
        && let Some((first, rest)) = text.split_once('\n')
        && first == key
    {
        return rest.lines().map(str::to_string).collect();
    }
    let names = compute_natives(exe, ui_source);
    if let (Some(key), Some(file)) = (key, file)
        && !names.is_empty()
        && let Some(dir) = file.parent()
        && std::fs::create_dir_all(dir).is_ok()
    {
        let _ = std::fs::write(&file, format!("{key}\n{}\n", names.join("\n")));
    }
    names
}

fn compute_natives(exe: &Path, ui_source: &Path) -> Vec<String> {
    let names = exe_identifiers(exe);
    if names.is_empty() {
        return Vec::new();
    }
    let g = ui_globals(ui_source);
    let mut out: Vec<String> = g
        .called
        .into_iter()
        .filter(|n| !g.defined.contains(n))
        .filter(|n| !KEYWORDS.contains(&n.as_str()))
        .filter(|n| names.contains(n))
        .collect();
    // Namespace functions: both names must be in the executable.
    out.extend(g.called_in_namespace.into_iter().filter(|full| {
        full.split_once('.')
            .is_some_and(|(ns, f)| names.contains(ns) && names.contains(f))
    }));
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_calls_and_definitions() {
        let mut g = UiGlobals::default();
        scan_lua(
            b"local x = GetFoo(1)\nfunction Bar(a) return a:Baz() end\nQux = 1\nif Qux == 2 then t.Quux() end\n",
            &mut g,
        );
        assert!(g.called.contains("GetFoo"));
        assert!(!g.called.contains("Baz"));
        assert!(!g.called.contains("Quux"));
        assert!(g.defined.contains("Bar"));
        assert!(g.defined.contains("Qux"));
        assert!(g.defined.contains("x"));
    }
}
