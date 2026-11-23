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
    /// Send a command to a running sandbox (see `ctl help`).
    Ctl {
        /// Control port of the sandbox.
        #[arg(long, default_value_t = wowprogramming_engine::control::DEFAULT_PORT)]
        port: u16,
        /// The command and its arguments.
        command: Vec<String>,
    },
    /// Copy a capture from the WowProgrammingCapture addon's SavedVariables
    /// file into a scenario folder as capture.txt.
    ImportCapture {
        /// WTF/Account/<account>/SavedVariables/WowProgrammingCapture.lua
        saved_variables: PathBuf,
        /// Scenario folder to write capture.txt into.
        scenario: PathBuf,
        /// Character as "Name-Realm". Defaults to the newest capture.
        #[arg(long)]
        character: Option<String>,
    },
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
    /// Load the whole default UI (every Blizzard addon the client loads at
    /// startup) instead of the scenario's list.
    #[arg(long)]
    full_ui: bool,
    /// Port for the control bridge (`wowprogramming ctl`). 0 turns it off.
    #[arg(long, default_value_t = wowprogramming_engine::control::DEFAULT_PORT)]
    control_port: u16,
    /// Keep running without a console, until `ctl quit` (for tools).
    #[arg(long)]
    serve: bool,
    /// Headless only: render the UI to this PNG file before exiting,
    /// without opening a window.
    #[arg(long)]
    screenshot: Option<PathBuf>,
    /// Pixels per UI unit for --screenshot.
    #[arg(long, default_value_t = 0.75)]
    screenshot_scale: f64,
    /// gui only: open a normal window instead of a borderless window that
    /// covers the monitor.
    #[arg(long)]
    windowed: bool,
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
        Command::ImportCapture {
            saved_variables,
            scenario,
            character,
        } => import_capture(&saved_variables, &scenario, character.as_deref()),
        Command::Casc { game_dir, files } => casc_info(game_dir, &files),
        Command::Ctl { port, command } => {
            match wowprogramming_engine::control::send(port, &command.join(" ")) {
                Ok(reply) => {
                    print!("{reply}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("cannot reach sandbox on port {port}: {e}");
                    ExitCode::FAILURE
                }
            }
        }
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
    if let Some(dir) = config.scenario.clone() {
        apply_scenario_settings(&mut config, &dir)?;
        if !args.blizzard.is_empty() {
            config.blizzard_addons = args.blizzard.clone();
        }
        if !args.bootstrap.is_empty() {
            config.bootstrap_addons = args.bootstrap.clone();
        }
    }
    if args.full_ui && config.ui_source_dir.is_some() {
        config.blizzard_addons = vec!["*".into()];
        config.blizzard_files.clear();
    }
    let game_dir = if args.no_game_data {
        None
    } else {
        file.game_dir.map(rel)
    };
    Ok((config, game_dir))
}

/// Settings from a scenario's `scenario.toml`: which Blizzard addons and
/// files to load. Command line options still win.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ScenarioConfig {
    blizzard_addons: Option<Vec<String>>,
    bootstrap_addons: Option<Vec<String>>,
    blizzard_files: Option<Vec<String>>,
    screen: Option<(f64, f64)>,
    /// Size of the screen in physical pixels, for pixel-snapped text.
    physical_screen: Option<(f64, f64)>,
}

fn apply_scenario_settings(config: &mut EngineConfig, dir: &Path) -> Result<(), String> {
    let path = dir.join("scenario.toml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    let sc: ScenarioConfig =
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if let Some(v) = sc.blizzard_addons {
        config.blizzard_addons = v;
    }
    if let Some(v) = sc.bootstrap_addons {
        config.bootstrap_addons = v;
    }
    if let Some(v) = sc.blizzard_files {
        config.blizzard_files = v;
    }
    if let Some(v) = sc.screen {
        config.screen = v;
    }
    if let Some((_, h)) = sc.physical_screen
        && config.screen.1 > 0.0
    {
        config.pixel_scale = h / config.screen.1;
    }
    Ok(())
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

    let mut control = start_control(args.control_port, args.seconds.is_none());
    match args.seconds {
        Some(seconds) => run_headless(&mut engine, seconds),
        None => run_console(&mut engine, control.as_mut(), args.serve),
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
    let control = start_control(args.control_port, true);
    let window = wowprogramming_gui::WindowOptions {
        windowed: args.windowed,
    };
    match wowprogramming_gui::run(engine, control, window) {
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

fn start_control(port: u16, wanted: bool) -> Option<wowprogramming_engine::control::ControlServer> {
    if port == 0 || !wanted {
        return None;
    }
    match wowprogramming_engine::control::ControlServer::start(port) {
        Ok(server) => {
            eprintln!(
                "Control bridge on 127.0.0.1:{} (wowprogramming ctl)",
                server.port
            );
            Some(server)
        }
        Err(e) => {
            eprintln!("warning: no control bridge on port {port}: {e}");
            None
        }
    }
}

/// Real-time loop with a console on stdin (unless serving) and the
/// control bridge.
fn run_console(
    engine: &mut Engine,
    mut control: Option<&mut wowprogramming_engine::control::ControlServer>,
    serve: bool,
) {
    let (tx, rx) = mpsc::channel();
    if !serve {
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        eprintln!("Console ready. Type /quit to exit, /reload to reload, or any Lua code.");
    } else {
        eprintln!("Serving. Stop with: wowprogramming ctl quit");
    }
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
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if !serve {
                    break;
                }
            }
        }
        if let Some(server) = control.as_deref_mut() {
            use wowprogramming_engine::control::Action;
            let (shown, action) = server.poll(engine);
            for line in shown {
                eprintln!("{line}");
            }
            match action {
                Action::Quit => break,
                Action::Tick(s) => {
                    let frames = (s / FRAME_TIME).round().max(1.0) as u64;
                    for _ in 0..frames {
                        engine.tick(FRAME_TIME);
                    }
                }
                Action::None => {}
            }
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

/// Read WowProgrammingCaptureDB from a SavedVariables file and write one
/// character's capture to `<scenario>/capture.txt`.
fn import_capture(sv: &Path, scenario: &Path, character: Option<&str>) -> ExitCode {
    use wowprogramming_lua::mlua::{Table, Value};
    let result = (|| -> Result<String, String> {
        let code = std::fs::read_to_string(sv).map_err(|e| format!("{}: {e}", sv.display()))?;
        let rt = Runtime::new().map_err(|e| e.to_string())?;
        rt.exec(&code, &format!("@{}", sv.display()))
            .map_err(|e| e.to_string())?;
        let db: Table = rt
            .lua()
            .globals()
            .get("WowProgrammingCaptureDB")
            .map_err(|_| "no WowProgrammingCaptureDB in the file".to_string())?;
        let mut best: Option<(String, i64, String)> = None;
        for pair in db.pairs::<String, Table>() {
            let (name, entry) = pair.map_err(|e| e.to_string())?;
            let time: i64 = entry.get("time").unwrap_or(0);
            let data: Value = entry.get("data").unwrap_or(Value::Nil);
            let Value::String(data) = data else { continue };
            let wanted = character.is_none_or(|c| c.eq_ignore_ascii_case(&name));
            if wanted && best.as_ref().is_none_or(|b| time > b.1) {
                best = Some((name, time, data.to_string_lossy()));
            }
        }
        let (name, _, data) = best.ok_or_else(|| match character {
            Some(c) => format!("no capture for {c}"),
            None => "no captures in the file".to_string(),
        })?;
        std::fs::create_dir_all(scenario).map_err(|e| e.to_string())?;
        let out = scenario.join("capture.txt");
        std::fs::write(&out, format!("{data}\n")).map_err(|e| format!("{}: {e}", out.display()))?;
        Ok(format!(
            "wrote {} for {name} ({} KB)",
            out.display(),
            data.len() / 1024
        ))
    })();
    match result {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
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
