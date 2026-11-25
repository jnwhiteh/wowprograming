//! Control bridge: a local TCP server that lets tools drive a running
//! sandbox with short text commands, and get text answers.
//!
//! Each connection sends one command line and receives the reply; then the
//! connection closes. The server only listens on 127.0.0.1. Requests are
//! handled on the main thread in [`ControlServer::poll`], because the engine
//! is not thread safe.
//!
//! Commands:
//!
//! - `exec <line>`: run a console line (`/dump ...`, `/run ...` or Lua).
//! - `eval <expr>`: evaluate a Lua expression and print the results.
//! - `dofile <path>`: run a Lua file from disk.
//! - `key <KEY>`: press and release a key, such as `P`, `SHIFT-P` or
//!   `ESCAPE`.
//! - `type <text>`: type text into the focused edit box.
//! - `click <expr> [button]`: click the frame the Lua expression returns.
//! - `hover <expr>`: move the mouse over that frame.
//! - `drag <expr> <dx> <dy> [button]`: press over the centre of that
//!   frame, move by (dx, dy) in ten steps, release. `drag <x> <y> <dx>
//!   <dy> [button]` starts at a screen position instead.
//! - `down [button]`, `up [button]`: press or release a mouse button.
//! - `mouse <x> <y>`: move the mouse to a screen position (y up).
//! - `under [x y]`: frames under a position (default: the mouse).
//! - `tree <expr> [depth]`: shown widgets below a frame, with rectangles.
//! - `tick <seconds>`: advance time (headless servers only).
//! - `screenshot <path> [scale] [expr]`: render the UI to a PNG file,
//!   cropped to the frame the Lua expression returns, if given.
//! - `stubs`: generated stubs that were called.
//! - `quit`: stop a headless server.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;

use crate::{Engine, Output};

pub const DEFAULT_PORT: u16 = 47811;

pub struct ControlServer {
    requests: mpsc::Receiver<(String, TcpStream)>,
    pub port: u16,
}

/// What a command asks the host loop to do besides replying.
#[derive(Debug, PartialEq)]
pub enum Action {
    None,
    Quit,
    Tick(f64),
}

impl ControlServer {
    pub fn start(port: u16) -> std::io::Result<ControlServer> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let port = listener.local_addr()?.port();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let mut line = String::new();
                let mut reader = BufReader::new(match stream.try_clone() {
                    Ok(s) => s,
                    Err(_) => continue,
                });
                if reader.read_line(&mut line).is_ok() && tx.send((line, stream)).is_err() {
                    break;
                }
            }
        });
        Ok(ControlServer { requests: rx, port })
    }

    /// Handle pending requests. Output produced by a command goes in its
    /// reply and is also returned, so a host can show it too.
    pub fn poll(&mut self, engine: &mut Engine) -> (Vec<Output>, Action) {
        let mut shown = Vec::new();
        let mut action = Action::None;
        while let Ok((line, mut stream)) = self.requests.try_recv() {
            // Keep output that was pending before the command for the host.
            shown.extend(engine.drain_output());
            let (reply, a) = handle(engine, line.trim());
            let output = engine.drain_output();
            let mut text = String::new();
            for o in &output {
                text.push_str(&o.to_string());
                text.push('\n');
            }
            text.push_str(&reply);
            if !text.ends_with('\n') {
                text.push('\n');
            }
            let _ = stream.write_all(text.as_bytes());
            shown.extend(output);
            if a != Action::None {
                action = a;
            }
        }
        (shown, action)
    }
}

/// Send one command to a running sandbox and return its reply.
pub fn send(port: u16, command: &str) -> std::io::Result<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.write_all(command.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.shutdown(std::net::Shutdown::Write)?;
    let mut reply = String::new();
    std::io::Read::read_to_string(&mut stream, &mut reply)?;
    Ok(reply)
}

const INSPECT: &str = include_str!("lua/inspect.lua");

fn lua_helper(engine: &Engine, name: &str, args: &str) -> String {
    let code = format!("{INSPECT}\nreturn {name}({args})");
    match engine.eval::<String>(&code) {
        Ok(s) => s,
        Err(e) => format!("error: {e}"),
    }
}

/// Quote a string as a Lua string literal.
fn lua_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn handle(engine: &mut Engine, line: &str) -> (String, Action) {
    let (cmd, rest) = match line.split_once(char::is_whitespace) {
        Some((c, r)) => (c, r.trim()),
        None => (line, ""),
    };
    match cmd {
        "exec" => {
            engine.run_command(rest);
            (String::new(), Action::None)
        }
        "type" => {
            if engine.text_input(rest) {
                (String::new(), Action::None)
            } else {
                ("no edit box has the keyboard focus".into(), Action::None)
            }
        }
        "dofile" => match std::fs::read_to_string(rest) {
            Ok(code) => {
                engine.exec(&code);
                (String::new(), Action::None)
            }
            Err(e) => (format!("error: {rest}: {e}"), Action::None),
        },
        "eval" => (
            lua_helper(engine, "Inspect_Eval", &lua_quote(rest)),
            Action::None,
        ),
        "key" => {
            let ok = engine.key_down(rest);
            engine.key_up(rest);
            (
                if ok {
                    String::new()
                } else {
                    format!("no binding for {rest}")
                },
                Action::None,
            )
        }
        // Hold a key across ticks (movement keys), then release it.
        "keydown" => {
            let ok = engine.key_down(rest);
            (
                if ok {
                    String::new()
                } else {
                    format!("no binding for {rest}")
                },
                Action::None,
            )
        }
        "keyup" => {
            engine.key_up(rest);
            (String::new(), Action::None)
        }
        "click" | "hover" => {
            let (expr, button) = match rest.rsplit_once(' ') {
                Some((e, b)) if b.ends_with("Button") => (e, b),
                _ => (rest, "LeftButton"),
            };
            let center = lua_helper(engine, "Inspect_Center", &lua_quote(expr));
            let mut parts = center.split_whitespace().map(|p| p.parse::<f64>());
            match (parts.next(), parts.next()) {
                (Some(Ok(x)), Some(Ok(y))) => {
                    engine.mouse_move(x, y);
                    if cmd == "click" {
                        engine.mouse_down(button);
                        engine.mouse_up(button);
                    }
                    let focus = engine
                        .mouse_focus_name()
                        .unwrap_or_else(|| "nothing".into());
                    (format!("mouse at {x:.1} {y:.1} over {focus}"), Action::None)
                }
                _ => (center, Action::None),
            }
        }
        "drag" => {
            let mut words: Vec<&str> = rest.split_whitespace().collect();
            let button = match words.last() {
                Some(b) if b.ends_with("Button") => {
                    let b = b.to_string();
                    words.pop();
                    b
                }
                _ => "LeftButton".to_string(),
            };
            let nums: Vec<f64> = words.iter().filter_map(|w| w.parse().ok()).collect();
            let (from, by) = if nums.len() == 4 && words.len() == 4 {
                ((nums[0], nums[1]), (nums[2], nums[3]))
            } else if words.len() >= 3 {
                let n = words.len();
                let (Ok(dx), Ok(dy)) = (words[n - 2].parse(), words[n - 1].parse()) else {
                    return ("usage: drag <expr> <dx> <dy> [button]".into(), Action::None);
                };
                let expr = words[..n - 2].join(" ");
                let center = lua_helper(engine, "Inspect_Center", &lua_quote(&expr));
                let mut parts = center.split_whitespace().map(|p| p.parse::<f64>());
                match (parts.next(), parts.next()) {
                    (Some(Ok(x)), Some(Ok(y))) => ((x, y), (dx, dy)),
                    _ => return (center, Action::None),
                }
            } else {
                return ("usage: drag <expr> <dx> <dy> [button]".into(), Action::None);
            };
            engine.drag(from, by, &button, 10);
            let focus = engine
                .mouse_focus_name()
                .unwrap_or_else(|| "nothing".into());
            (
                format!(
                    "dragged from {:.1} {:.1} to {:.1} {:.1}, over {focus}",
                    from.0,
                    from.1,
                    from.0 + by.0,
                    from.1 + by.1
                ),
                Action::None,
            )
        }
        "down" | "up" => {
            let button = if rest.is_empty() { "LeftButton" } else { rest };
            if cmd == "down" {
                engine.mouse_down(button);
            } else {
                engine.mouse_up(button);
            }
            (String::new(), Action::None)
        }
        "mouse" => {
            let v: Vec<f64> = rest
                .split_whitespace()
                .filter_map(|p| p.parse().ok())
                .collect();
            if v.len() < 2 {
                return ("usage: mouse <x> <y>".into(), Action::None);
            }
            engine.mouse_move(v[0], v[1]);
            let focus = engine
                .mouse_focus_name()
                .unwrap_or_else(|| "nothing".into());
            (format!("over {focus}"), Action::None)
        }
        "under" => {
            let v: Vec<f64> = rest
                .split_whitespace()
                .filter_map(|p| p.parse().ok())
                .collect();
            let args = if v.len() >= 2 {
                format!("{}, {}", v[0], v[1])
            } else {
                String::new()
            };
            (lua_helper(engine, "Inspect_Under", &args), Action::None)
        }
        "tree" => {
            let (expr, depth) = match rest.rsplit_once(' ') {
                Some((e, d)) if d.parse::<u32>().is_ok() => (e, d.to_string()),
                _ => (rest, "3".to_string()),
            };
            (
                lua_helper(
                    engine,
                    "Inspect_Tree",
                    &format!("{}, {depth}", lua_quote(expr)),
                ),
                Action::None,
            )
        }
        "tick" => (String::new(), Action::Tick(rest.parse().unwrap_or(0.1))),
        "screenshot" => {
            let mut parts = rest.split_whitespace();
            let Some(path) = parts.next() else {
                return ("usage: screenshot <path> [scale]".into(), Action::None);
            };
            let scale = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.75);
            let expr: Vec<&str> = parts.collect();
            let crop = if expr.is_empty() {
                None
            } else {
                let code = format!("return ({}):GetRect()", expr.join(" "));
                match engine.eval::<(f64, f64, f64, f64)>(&code) {
                    Ok((l, b, w, h)) => Some(crate::layout::Rect {
                        left: l,
                        bottom: b,
                        width: w,
                        height: h,
                    }),
                    Err(e) => return (format!("error: {e}"), Action::None),
                }
            };
            match engine.save_screenshot_crop(std::path::Path::new(path), scale, crop) {
                Ok(()) => (format!("saved {path}"), Action::None),
                Err(e) => (format!("error: {e}"), Action::None),
            }
        }
        "stubs" => {
            engine.run_command("/stubs");
            (String::new(), Action::None)
        }
        "quit" => ("bye".into(), Action::Quit),
        "" => (
            "commands: exec eval dofile key type click hover drag down up mouse under tree tick screenshot stubs quit"
                .into(),
            Action::None,
        ),
        other => (format!("unknown command {other}"), Action::None),
    }
}
