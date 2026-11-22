use std::process::ExitCode;

use wowprogramming_lua::Runtime;

const USAGE: &str = "usage: wowprogramming lua <file.lua>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [cmd, path] if cmd == "lua" => run_lua_file(path),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run_lua_file(path: &str) -> ExitCode {
    let code = match std::fs::read_to_string(path) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("cannot read {path}: {err}");
            return ExitCode::FAILURE;
        }
    };
    let result = Runtime::new().and_then(|rt| rt.exec(&code, &format!("@{path}")));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}
