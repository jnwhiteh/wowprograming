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
    /// Run interaction specs: Lua files that drive the UI (Click, Press,
    /// Hold, Wait, Command) and check the result. Takes the same options
    /// as `run`. Exits with failure if a spec fails or a Lua error occurs.
    Test {
        #[command(flatten)]
        run: RunArgs,
        /// Spec files to run, in order, in one session.
        #[arg(required = true)]
        specs: Vec<PathBuf>,
    },
    /// Run a Lua file in the sandbox's Lua environment, without addons.
    Lua { file: PathBuf },
    /// Report which UI.xsd script handlers the engine fires and which
    /// documented widget methods it implements, with their uses in
    /// Blizzard's code.
    Coverage {
        /// Config file. Defaults to config.local.toml when it exists.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Blizzard UI source tree.
        #[arg(long)]
        ui_source: Option<PathBuf>,
        /// List every handler and method, not only the gaps.
        #[arg(long)]
        all: bool,
    },
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
    /// Print rows of a DB2 table from the game data: field values as
    /// numbers, and as strings where they look like strings.
    Db2 {
        /// Game install folder (defaults to game_dir from the config file).
        #[arg(long)]
        game_dir: Option<PathBuf>,
        /// FileDataID of the table, such as 1349477 for Map.db2.
        table: u32,
        /// Row IDs to print. Without any, prints the first 5 rows.
        ids: Vec<u32>,
    },
    /// Save a game texture (BLP, by FileDataID or path) as a PNG file.
    Image {
        #[arg(long)]
        game_dir: Option<PathBuf>,
        file: String,
        out: PathBuf,
    },
    /// Save a file from the game data (by FileDataID) for inspection.
    Extract {
        #[arg(long)]
        game_dir: Option<PathBuf>,
        file: u32,
        out: PathBuf,
    },
    /// Print a model's textures, materials and geosets (M2, by FileDataID).
    Model {
        #[arg(long)]
        game_dir: Option<PathBuf>,
        file: u32,
    },
    /// Render the test world scene (see `world::demo_scene`) to a PNG file.
    World {
        #[arg(long)]
        game_dir: Option<PathBuf>,
        out: PathBuf,
        /// ChrRaces ID of the player model.
        #[arg(long, default_value_t = 3)]
        race: u32,
        #[arg(long, default_value_t = 0)]
        sex: u32,
        #[arg(long, default_value_t = 1720)]
        width: usize,
        #[arg(long, default_value_t = 720)]
        height: usize,
        /// Camera for character select instead of behind the player.
        #[arg(long)]
        charselect: bool,
    },
    /// Render the loading screen of a map (Map.db2 ID) to a PNG file.
    LoadingScreen {
        #[arg(long)]
        game_dir: Option<PathBuf>,
        map: u32,
        out: PathBuf,
        #[arg(long, default_value_t = 3440)]
        width: usize,
        #[arg(long, default_value_t = 1440)]
        height: usize,
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

#[derive(clap::Args, Clone)]
struct RunArgs {
    /// Config file. Defaults to config.local.toml when it exists.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Folder with the third-party addons that `--addon` can name.
    #[arg(long)]
    addons_dir: Option<PathBuf>,
    /// Blizzard UI source tree.
    #[arg(long)]
    ui_source: Option<PathBuf>,
    /// Sandbox WTF folder for SavedVariables.
    #[arg(long)]
    wtf_dir: Option<PathBuf>,
    /// Third-party addon to load (with its dependencies). Can be repeated.
    /// Without this option only the default UI loads.
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
    /// Load the whole default UI (every Blizzard addon the client loads at
    /// startup) instead of the scenario's list.
    #[arg(long)]
    full_ui: bool,
    /// Start at character select (the glue screens) instead of in the world.
    #[arg(long)]
    glue: bool,
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
    /// Reload the UI when a file of a loaded third-party addon changes.
    #[arg(long)]
    watch: bool,
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
        Command::Test { run, specs } => match build_config(&run) {
            Ok((config, game_dir)) => run_specs(config, game_dir, &specs),
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
        Command::Image {
            game_dir,
            file,
            out,
        } => {
            let dir = game_dir.or_else(|| {
                let text = std::fs::read_to_string("config.local.toml").ok()?;
                toml::from_str::<FileConfig>(&text).ok()?.game_dir
            });
            let Some(gd) = open_game_data(dir.as_deref()) else {
                eprintln!("no game data; pass --game-dir");
                return ExitCode::FAILURE;
            };
            let key = match file.parse::<u32>() {
                Ok(id) => wowprogramming_engine::gamedata::ImageKey::Id(id),
                Err(_) => wowprogramming_engine::gamedata::ImageKey::Path(file.clone()),
            };
            let Some(img) = gd.image(&key) else {
                eprintln!("{file}: not an image");
                return ExitCode::FAILURE;
            };
            let opaque = img
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[3] > 200)
                .count();
            println!(
                "{file}: {}x{}, {}% opaque",
                img.width,
                img.height,
                opaque * 100 / (img.width as usize * img.height as usize).max(1)
            );
            match wowprogramming_engine::render::write_png(
                &out,
                img.width as usize,
                img.height as usize,
                &img.rgba,
            ) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("cannot write {}: {e}", out.display());
                    ExitCode::FAILURE
                }
            }
        }
        Command::Extract {
            game_dir,
            file,
            out,
        } => {
            let dir = game_dir.or_else(|| {
                let text = std::fs::read_to_string("config.local.toml").ok()?;
                toml::from_str::<FileConfig>(&text).ok()?.game_dir
            });
            let Some(gd) = open_game_data(dir.as_deref()) else {
                eprintln!("no game data; pass --game-dir");
                return ExitCode::FAILURE;
            };
            match gd.read_file(file).map(|d| std::fs::write(&out, d)) {
                Some(Ok(())) => ExitCode::SUCCESS,
                Some(Err(e)) => {
                    eprintln!("cannot write {}: {e}", out.display());
                    ExitCode::FAILURE
                }
                None => {
                    eprintln!("{file}: not found");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Model { game_dir, file } => {
            let dir = game_dir.or_else(|| {
                let text = std::fs::read_to_string("config.local.toml").ok()?;
                toml::from_str::<FileConfig>(&text).ok()?.game_dir
            });
            let Some(gd) = open_game_data(dir.as_deref()) else {
                eprintln!("no game data; pass --game-dir");
                return ExitCode::FAILURE;
            };
            use wowprogramming_formats::m2;
            let model = match gd.read_file(file).map(|d| m2::parse(&d)) {
                Some(Ok(m)) => m,
                Some(Err(e)) => {
                    eprintln!("{file}: {e}");
                    return ExitCode::FAILURE;
                }
                None => {
                    eprintln!("{file}: not found");
                    return ExitCode::FAILURE;
                }
            };
            println!(
                "{}: {} vertices, bounds {:?}..{:?}, skins {:?}",
                model.name,
                model.vertices.len(),
                model.bounds_min,
                model.bounds_max,
                model.skin_files
            );
            for (i, t) in model.textures.iter().enumerate() {
                println!("  texture {i}: kind {} file {}", t.kind, t.file);
            }
            let mut lo = [f32::MAX; 3];
            let mut hi = [f32::MIN; 3];
            for v in &model.vertices {
                for k in 0..3 {
                    lo[k] = lo[k].min(v.pos[k]);
                    hi[k] = hi[k].max(v.pos[k]);
                }
            }
            println!("  vertex positions {lo:?}..{hi:?}");
            if let Some(skin) = model
                .skin_files
                .first()
                .and_then(|f| gd.read_file(*f))
                .map(|d| m2::parse_skin(&d))
            {
                match skin {
                    Ok(skin) => {
                        let ids: Vec<u16> = skin.sections.iter().map(|s| s.id).collect();
                        println!("  {} batches, geosets {ids:?}", skin.batches.len());
                    }
                    Err(e) => println!("  skin: {e}"),
                }
            }
            ExitCode::SUCCESS
        }
        Command::World {
            game_dir,
            out,
            race,
            sex,
            width,
            height,
            charselect,
        } => {
            let dir = game_dir.or_else(|| {
                let text = std::fs::read_to_string("config.local.toml").ok()?;
                toml::from_str::<FileConfig>(&text).ok()?.game_dir
            });
            let Some(gd) = open_game_data(dir.as_deref()) else {
                eprintln!("no game data; pass --game-dir");
                return ExitCode::FAILURE;
            };
            let started = std::time::Instant::now();
            let view = if charselect {
                wowprogramming_engine::world::View::CharacterSelect
            } else {
                wowprogramming_engine::world::View::Behind
            };
            let world = wowprogramming_engine::world::demo_scene(&gd, race, sex, view);
            println!(
                "{} models in {} ms",
                world.instances.len(),
                started.elapsed().as_millis()
            );
            let started = std::time::Instant::now();
            let img = world.render(&gd, width, height);
            println!("rendered in {} ms", started.elapsed().as_millis());
            match wowprogramming_engine::render::write_png(&out, img.width, img.height, &img.data) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("cannot write {}: {e}", out.display());
                    ExitCode::FAILURE
                }
            }
        }
        Command::LoadingScreen {
            game_dir,
            map,
            out,
            width,
            height,
        } => {
            let dir = game_dir.or_else(|| {
                let text = std::fs::read_to_string("config.local.toml").ok()?;
                toml::from_str::<FileConfig>(&text).ok()?.game_dir
            });
            let Some(gd) = open_game_data(dir.as_deref()) else {
                eprintln!("no game data; pass --game-dir");
                return ExitCode::FAILURE;
            };
            let Some(screen) = wowprogramming_engine::loading::for_map(&gd, map) else {
                eprintln!("no loading screen for map {map}");
                return ExitCode::FAILURE;
            };
            println!("{screen:?}");
            let img = wowprogramming_engine::loading::render(&gd, &screen, width, height, 0.8);
            match wowprogramming_engine::render::write_png(&out, img.width, img.height, &img.data) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("cannot write {}: {e}", out.display());
                    ExitCode::FAILURE
                }
            }
        }
        Command::Db2 {
            game_dir,
            table,
            ids,
        } => db2_rows(game_dir, table, &ids),
        Command::Coverage {
            config,
            ui_source,
            all,
        } => coverage(config, ui_source, all),
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

fn coverage(config: Option<PathBuf>, ui_source: Option<PathBuf>, all: bool) -> ExitCode {
    #[derive(Parser)]
    struct Defaults {
        #[command(flatten)]
        args: RunArgs,
    }
    let args = Defaults::parse_from(["run"]).args;
    let args = RunArgs {
        config,
        ui_source,
        no_game_data: true,
        ..args
    };
    let (mut config, _) = match build_config(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let Some(ui) = config.ui_source_dir.clone() else {
        eprintln!("coverage needs the UI source (--ui-source or ui_source_dir in the config)");
        return ExitCode::FAILURE;
    };
    let xsd_path = ui.join("Interface/AddOns/Blizzard_SharedXML/UI.xsd");
    let xsd = match std::fs::read_to_string(&xsd_path) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("cannot read {}: {e}", xsd_path.display());
            return ExitCode::FAILURE;
        }
    };
    config.blizzard_addons.clear();
    config.scenario = None;
    let engine = match Engine::new(config) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("cannot start sandbox: {e}");
            return ExitCode::FAILURE;
        }
    };
    let uses = wowprogramming_engine::coverage::scan_uses(&ui);
    match wowprogramming_engine::coverage::collect(engine.lua(), &xsd, &uses) {
        Ok(report) => {
            print!("{}", report.to_text(all));
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
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

    // Only the default UI loads unless third-party addons are named; the
    // character's enabled addons in the game do not matter.
    let addons_dir = if args.addons.is_empty() {
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
    config.watch = args.watch;
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
    if args.glue {
        config.glue = true;
        config.blizzard_addons = vec!["*".into()];
        config.blizzard_files.clear();
        config.bootstrap_addons.clear();
    }
    let game_dir = if args.no_game_data {
        None
    } else {
        file.game_dir.map(rel)
    };
    config.game_account_dir = game_dir.as_deref().and_then(game_account_dir);
    Ok((config, game_dir))
}

/// The game's account folder (`WTF/Account/<ACCOUNT>`). With several
/// accounts, the one used last (newest `config-cache.wtf`).
fn game_account_dir(game_dir: &Path) -> Option<PathBuf> {
    let root = game_dir.join("WTF").join("Account");
    std::fs::read_dir(root)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n != "SavedVariables"))
        .filter(|p| p.join("SavedVariables").is_dir())
        .max_by_key(|p| {
            std::fs::metadata(p.join("config-cache.wtf"))
                .and_then(|m| m.modified())
                .ok()
        })
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
    /// The character this scenario plays, as "Name-Realm". Entering the
    /// world as that character on the character select screen uses it.
    character: Option<String>,
}

/// The scenario for a character chosen on the character select screen:
/// the one whose `scenario.toml` names it, or `world` for any other.
fn scenario_for(root: &Path, entry: &wowprogramming_engine::WorldEntry) -> PathBuf {
    let wanted = format!("{}-{}", entry.name, entry.realm);
    let found = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|dir| {
            std::fs::read_to_string(dir.join("scenario.toml"))
                .ok()
                .and_then(|t| toml::from_str::<ScenarioConfig>(&t).ok())
                .and_then(|c| c.character)
                .is_some_and(|c| c.eq_ignore_ascii_case(&wanted))
        });
    found.unwrap_or_else(|| root.join("world"))
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
        Some(seconds) => run_headless(&mut engine, seconds, args),
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

fn run_specs(config: EngineConfig, game_dir: Option<PathBuf>, specs: &[PathBuf]) -> ExitCode {
    let gamedata = open_game_data(game_dir.as_deref());
    let mut engine = match Engine::with_gamedata(config, gamedata) {
        Ok(engine) => engine,
        Err(err) => {
            eprintln!("cannot start sandbox: {err}");
            return ExitCode::FAILURE;
        }
    };
    engine.start();
    // Let the UI settle; errors from loading count against the first spec.
    for _ in 0..30 {
        engine.tick(FRAME_TIME);
    }
    let mut failed = 0;
    for spec in specs {
        match engine.run_spec(spec) {
            Ok(()) => println!("PASS {}", spec.display()),
            Err(err) => {
                failed += 1;
                println!("FAIL {}\n  {}", spec.display(), err.replace('\n', "\n  "));
            }
        }
    }
    engine.shutdown();
    println!("{} passed, {} failed", specs.len() - failed, failed);
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
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
    // From character select, Enter World starts a game session with the
    // chosen character's scenario.
    let factory: Option<wowprogramming_gui::WorldFactory> = if args.glue {
        let base = args.clone();
        let gamedata = engine.gamedata().cloned();
        Some(Box::new(move |entry| {
            session_engine(&base, gamedata.clone(), entry)
        }))
    } else {
        None
    };
    match wowprogramming_gui::run(engine, control, window, factory) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("window error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// The engine for the session that Enter World starts from character
/// select (`--glue`): the chosen character's scenario, not yet started.
/// Without an entry, the engine for character select again (log out).
fn session_engine(
    base: &RunArgs,
    gamedata: Option<std::rc::Rc<wowprogramming_engine::gamedata::GameData>>,
    entry: Option<&wowprogramming_engine::WorldEntry>,
) -> Result<Engine, String> {
    let mut a = base.clone();
    let Some(entry) = entry else {
        let (config, _) = build_config(&a)?;
        return Engine::with_gamedata(config, gamedata).map_err(|e| e.to_string());
    };
    let root = base
        .scenario
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("scenarios"));
    a.glue = false;
    a.full_ui = true;
    a.exec.clear();
    a.scenario = Some(scenario_for(&root, entry));
    eprintln!(
        "Entering the world as {}-{} with {}",
        entry.name,
        entry.realm,
        a.scenario.as_deref().unwrap_or(Path::new("")).display()
    );
    let (mut config, _) = build_config(&a)?;
    config.world_entry = Some(entry.clone());
    config.character = entry.name.clone();
    config.realm = entry.realm.clone();
    Engine::with_gamedata(config, gamedata).map_err(|e| e.to_string())
}

/// Advance game time as fast as possible, in fixed frames. With `--glue`,
/// Enter World switches to the character's session, as in the window.
fn run_headless(engine: &mut Engine, seconds: f64, args: &RunArgs) {
    let frames = (seconds / FRAME_TIME).round().max(0.0) as u64;
    for _ in 0..frames {
        engine.tick(FRAME_TIME);
        print_output(engine);
        if args.glue
            && let Some(entry) = engine.take_enter_world()
        {
            match session_engine(args, engine.gamedata().cloned(), Some(&entry)) {
                Ok(mut world) => {
                    world.start();
                    print_output(&world);
                    *engine = world;
                }
                Err(err) => eprintln!("cannot enter the world: {err}"),
            }
        }
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

fn db2_rows(game_dir: Option<PathBuf>, table: u32, ids: &[u32]) -> ExitCode {
    let dir = game_dir.or_else(|| {
        let text = std::fs::read_to_string("config.local.toml").ok()?;
        toml::from_str::<FileConfig>(&text).ok()?.game_dir
    });
    let Some(gd) = open_game_data(dir.as_deref()) else {
        eprintln!("no game data; pass --game-dir");
        return ExitCode::FAILURE;
    };
    let Some(data) = gd.read_file(table) else {
        eprintln!("table {table} not found");
        return ExitCode::FAILURE;
    };
    let t = match wowprogramming_formats::db2::Table::parse(&data) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("cannot parse table {table}: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("{} rows, {} fields", t.records.len(), t.field_count());
    let rows: Vec<_> = if ids.is_empty() {
        t.records.iter().take(5).collect()
    } else {
        t.records.iter().filter(|r| ids.contains(&r.id)).collect()
    };
    for r in rows {
        println!("ID {}", r.id);
        for (i, v) in r.values.iter().enumerate() {
            let s = t.string(r, i);
            if s.is_empty() || s.chars().any(|c| c.is_control()) {
                println!("  {i}: {v:?}");
            } else {
                println!("  {i}: {v:?} {s:?}");
            }
        }
    }
    ExitCode::SUCCESS
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
