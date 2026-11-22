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
    /// Load addons and open a window that shows the UI, with a console.
    Gui(RunArgs),
    /// Run a Lua file in the sandbox's Lua environment, without addons.
    Lua { file: PathBuf },
    /// Show whether files exist in the game data, by path or FileDataID.
    Casc {
        /// Game install folder (defaults to game_dir from the config file).
        #[arg(long)]
        game_dir: Option<PathBuf>,
        /// Paths or FileDataIDs.
        files: Vec<String>,
    },
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
    /// Scenario folder with a scenario.lua that sets up game state.
    #[arg(long)]
    scenario: Option<PathBuf>,
    /// Load-on-demand Blizzard addon whose [Bootstrap] files run at startup.
    /// Can be repeated.
    #[arg(long = "bootstrap", value_name = "NAME")]
    bootstrap: Vec<String>,
    /// Do not read textures, atlases and fonts from the game install.
    #[arg(long)]
    no_game_data: bool,
    /// Do not load third-party addons.
    #[arg(long)]
    no_addons: bool,
    /// Headless only: render the UI to this PNG file before exiting,
    /// without opening a window.
    #[arg(long)]
    screenshot: Option<PathBuf>,
    /// Pixels per UI unit for --screenshot.
    #[arg(long, default_value_t = 0.75)]
    screenshot_scale: f64,
}

/// Settings read from the config file. Relative paths are relative to the
/// config file.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    game_dir: Option<PathBuf>,
    ui_source_dir: Option<PathBuf>,
    addons_dir: Option<PathBuf>,
    wtf_dir: Option<PathBuf>,
    blizzard_addons: Option<Vec<String>>,
    bootstrap_addons: Option<Vec<String>>,
    blizzard_files: Option<Vec<String>>,
    scenario: Option<PathBuf>,
    account: Option<String>,
    realm: Option<String>,
    character: Option<String>,
}

const FRAME_TIME: f64 = 1.0 / 60.0;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Run(args) => match build_config(&args) {
            Ok((config, game_dir)) => run(config, game_dir, &args),
            Err(err) => {
                eprintln!("{err}");
                ExitCode::from(2)
            }
        },
        Command::Gui(args) => match build_config(&args) {
            Ok((config, game_dir)) => run_gui(config, game_dir, &args),
            Err(err) => {
                eprintln!("{err}");
                ExitCode::from(2)
            }
        },
        Command::Lua { file } => run_lua_file(&file),
        Command::Casc { game_dir, files } => casc_info(game_dir, &files),
    }
}

fn build_config(args: &RunArgs) -> Result<(EngineConfig, Option<PathBuf>), String> {
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

    let addons_dir = if args.no_addons {
        None
    } else {
        args.addons_dir.clone().or(file.addons_dir.map(rel))
    };
    let mut config = EngineConfig {
        ui_source_dir: args.ui_source.clone().or(file.ui_source_dir.map(rel)),
        addons_dir,
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
    if let Some(list) = file.bootstrap_addons {
        config.bootstrap_addons = list;
    }
    if let Some(list) = file.blizzard_files {
        config.blizzard_files = list;
    }
    if !args.bootstrap.is_empty() {
        config.bootstrap_addons = args.bootstrap.clone();
    }
    config.scenario = args.scenario.clone().or(file.scenario.map(rel));
    let game_dir = if args.no_game_data {
        None
    } else {
        file.game_dir.map(rel)
    };
    Ok((config, game_dir))
}

/// Open the game data, or explain why it is not available.
fn open_game_data(
    dir: Option<&Path>,
) -> Option<std::rc::Rc<wowprogramming_engine::gamedata::GameData>> {
    let dir = dir?;
    let started = Instant::now();
    match wowprogramming_engine::gamedata::GameData::open(dir) {
        Ok(gd) => {
            eprintln!(
                "Game data: {} ({} atlases) in {:.1}s",
                gd.build_name(),
                gd.atlas_count(),
                started.elapsed().as_secs_f64()
            );
            Some(std::rc::Rc::new(gd))
        }
        Err(e) => {
            eprintln!("warning: no game data from {}: {e}", dir.display());
            None
        }
    }
}

fn print_output(engine: &Engine) {
    for line in engine.drain_output() {
        match line {
            Output::Print(_) => println!("{line}"),
            _ => eprintln!("{line}"),
        }
    }
}

fn run(config: EngineConfig, game_dir: Option<PathBuf>, args: &RunArgs) -> ExitCode {
    let gamedata = open_game_data(game_dir.as_deref());
    let mut engine = match Engine::with_gamedata(config, gamedata) {
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
    if let Some(path) = &args.screenshot {
        match engine.save_screenshot(path, args.screenshot_scale) {
            Ok(()) => eprintln!("Saved {}", path.display()),
            Err(e) => eprintln!("cannot write {}: {e}", path.display()),
        }
    }
    engine.shutdown();
    print_output(&engine);
    ExitCode::SUCCESS
}

fn run_gui(config: EngineConfig, game_dir: Option<PathBuf>, args: &RunArgs) -> ExitCode {
    let gamedata = open_game_data(game_dir.as_deref());
    let mut engine = match Engine::with_gamedata(config, gamedata) {
        Ok(engine) => engine,
        Err(err) => {
            eprintln!("cannot start sandbox: {err}");
            return ExitCode::FAILURE;
        }
    };
    engine.start();
    for line in &args.exec {
        engine.run_command(line);
    }
    match wowprogramming_gui::run(engine) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("window error: {err}");
            ExitCode::FAILURE
        }
    }
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

fn casc_info(game_dir: Option<PathBuf>, files: &[String]) -> ExitCode {
    let dir = game_dir.or_else(|| {
        let text = std::fs::read_to_string("config.local.toml").ok()?;
        toml::from_str::<FileConfig>(&text).ok()?.game_dir
    });
    let Some(gd) = open_game_data(dir.as_deref()) else {
        eprintln!("no game data; pass --game-dir");
        return ExitCode::FAILURE;
    };
    for f in files {
        let id = f.parse::<u32>().ok().or_else(|| gd.file_id(f));
        match id.and_then(|id| gd.read_file(id).map(|d| (id, d))) {
            Some((id, data)) => {
                let head: String = data
                    .iter()
                    .take(16)
                    .map(|b| {
                        if b.is_ascii_graphic() {
                            *b as char
                        } else {
                            '.'
                        }
                    })
                    .collect();
                println!(
                    "{f}: FileDataID {id}, {} bytes, starts {head:?}",
                    data.len()
                );
            }
            None => println!("{f}: not found"),
        }
    }
    ExitCode::SUCCESS
}
