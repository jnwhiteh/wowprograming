use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use serde::Deserialize;
use wowprogramming_engine::{Engine, EngineConfig, Output};
use wowprogramming_lua::Runtime;

/// Run World of Warcraft addons outside the game.
#[derive(Parser)]
#[command(name = "wowprogramming", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Load addons and open a console. Type /quit to exit.
    Run(RunArgs),
    /// Run a Lua file in the sandbox's Lua environment, without addons.
    Lua { file: PathBuf },
}

#[derive(clap::Args)]
struct RunArgs {
    /// Config file. Defaults to config.local.toml when it exists.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Folder with the addons to load.
    #[arg(long)]
    addons_dir: Option<PathBuf>,
    /// Blizzard UI source tree.
    #[arg(long)]
    ui_source: Option<PathBuf>,
    /// Sandbox WTF folder for SavedVariables.
    #[arg(long)]
    wtf_dir: Option<PathBuf>,
    /// Load only this addon (and its dependencies). Can be repeated.
    #[arg(long = "addon", value_name = "NAME")]
    addons: Vec<String>,
    /// Blizzard addon to load. Replaces the default list. Can be repeated.
    #[arg(long = "blizzard", value_name = "NAME")]
    blizzard: Vec<String>,
    /// Do not load any Blizzard addons.
    #[arg(long, conflicts_with = "blizzard")]
    no_blizzard: bool,
    /// Console line to run after login. Can be repeated.
    #[arg(long = "exec", value_name = "LINE")]
    exec: Vec<String>,
    /// Run this many seconds of game time without a console, then exit.
    #[arg(long)]
    seconds: Option<f64>,
}

/// Settings read from the config file. Relative paths are relative to the
/// config file.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    #[allow(dead_code)]
    game_dir: Option<PathBuf>,
    ui_source_dir: Option<PathBuf>,
    addons_dir: Option<PathBuf>,
    wtf_dir: Option<PathBuf>,
    blizzard_addons: Option<Vec<String>>,
    account: Option<String>,
    realm: Option<String>,
    character: Option<String>,
}

const FRAME_TIME: f64 = 1.0 / 60.0;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Run(args) => match build_config(&args) {
            Ok(config) => run(config, &args),
            Err(err) => {
                eprintln!("{err}");
                ExitCode::from(2)
            }
        },
        Command::Lua { file } => run_lua_file(&file),
    }
}

fn build_config(args: &RunArgs) -> Result<EngineConfig, String> {
    let config_path = args.config.clone().or_else(|| {
        let default = PathBuf::from("config.local.toml");
        default.exists().then_some(default)
    });
    let (file, base) = match &config_path {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            let file: FileConfig =
                toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            let base = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            (file, base)
        }
        None => (FileConfig::default(), PathBuf::from(".")),
    };
    let rel = |p: PathBuf| if p.is_absolute() { p } else { base.join(p) };

    let mut config = EngineConfig {
        ui_source_dir: args.ui_source.clone().or(file.ui_source_dir.map(rel)),
        addons_dir: args.addons_dir.clone().or(file.addons_dir.map(rel)),
        ..EngineConfig::default()
    };
    if let Some(wtf) = args.wtf_dir.clone().or(file.wtf_dir.map(rel)) {
        config.wtf_dir = wtf;
    }
    if let Some(list) = file.blizzard_addons {
        config.blizzard_addons = list;
    }
    if !args.blizzard.is_empty() {
        config.blizzard_addons = args.blizzard.clone();
    }
    if args.no_blizzard || config.ui_source_dir.is_none() {
        config.blizzard_addons.clear();
    }
    if !args.addons.is_empty() {
        config.only_addons = Some(args.addons.clone());
    }
    if let Some(v) = file.account {
        config.account = v;
    }
    if let Some(v) = file.realm {
        config.realm = v;
    }
    if let Some(v) = file.character {
        config.character = v;
    }
    Ok(config)
}

fn print_output(engine: &Engine) {
    for line in engine.drain_output() {
        match line {
            Output::Print(_) => println!("{line}"),
            _ => eprintln!("{line}"),
        }
    }
}

fn run(config: EngineConfig, args: &RunArgs) -> ExitCode {
    let mut engine = match Engine::new(config) {
        Ok(engine) => engine,
        Err(err) => {
            eprintln!("cannot start sandbox: {err}");
            return ExitCode::FAILURE;
        }
    };
    engine.start();
    print_output(&engine);
    for line in &args.exec {
        engine.run_command(line);
        print_output(&engine);
    }

    match args.seconds {
        Some(seconds) => run_headless(&mut engine, seconds),
        None => run_console(&mut engine),
    }
    engine.shutdown();
    print_output(&engine);
    ExitCode::SUCCESS
}

/// Advance game time as fast as possible, in fixed frames.
fn run_headless(engine: &mut Engine, seconds: f64) {
    let frames = (seconds / FRAME_TIME).round().max(0.0) as u64;
    for _ in 0..frames {
        engine.tick(FRAME_TIME);
        print_output(engine);
    }
}

/// Real-time loop with a console on stdin.
fn run_console(engine: &mut Engine) {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    eprintln!("Console ready. Type /quit to exit, /reload to reload, or any Lua code.");
    let mut last = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_secs_f64(FRAME_TIME)) {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.eq_ignore_ascii_case("/quit") || trimmed.eq_ignore_ascii_case("/exit") {
                    break;
                }
                engine.run_command(trimmed);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        let now = Instant::now();
        engine.tick(now.duration_since(last).as_secs_f64());
        last = now;
        print_output(engine);
    }
}

fn run_lua_file(path: &Path) -> ExitCode {
    let code = match std::fs::read_to_string(path) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("cannot read {}: {err}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let name = format!("@{}", path.display());
    match Runtime::new().and_then(|rt| rt.exec(&code, &name)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}
