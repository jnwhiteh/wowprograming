//! Real third-party addons in the full default UI: TomTom, Clique and
//! WowLua (the "Real addons" goal in docs/scope.md). They come from the
//! addons folder in `config.local.toml` (or `WOWPROGRAMMING_ADDONS_DIR`);
//! the test is skipped when the UI source, the game data or an addon is
//! missing, as on CI.

use std::path::{Path, PathBuf};

use wowprogramming_engine::gamedata::GameData;
use wowprogramming_engine::{Engine, EngineConfig, Output};

const ADDONS: [&str; 3] = ["TomTom", "Clique", "WowLua"];

fn config_value(key: &str) -> Option<String> {
    let config = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config.local.toml");
    let text = std::fs::read_to_string(config).ok()?;
    let line = text.lines().find(|l| l.trim_start().starts_with(key))?;
    Some(line.split_once('=')?.1.trim().trim_matches('"').to_string())
}

fn path_from(env: &str, key: &str) -> Option<PathBuf> {
    std::env::var_os(env)
        .map(PathBuf::from)
        .or_else(|| config_value(key).map(PathBuf::from))
}

fn ui_source() -> Option<PathBuf> {
    let ui = std::env::var_os("WOWPROGRAMMING_UI_SOURCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../wow-ui-source-retail")
        });
    ui.join("Interface/AddOns/Blizzard_SharedXMLBase")
        .is_dir()
        .then_some(ui)
}

fn engine(wtf: &Path) -> Option<Engine> {
    let Some(ui) = ui_source() else {
        eprintln!("skipping: no UI source");
        return None;
    };
    let Some(addons) = path_from("WOWPROGRAMMING_ADDONS_DIR", "addons_dir") else {
        eprintln!("skipping: no addons_dir");
        return None;
    };
    if let Some(missing) = ADDONS.iter().find(|a| !addons.join(a).is_dir()) {
        eprintln!("skipping: {missing} is not in {}", addons.display());
        return None;
    }
    let Some(gamedata) =
        path_from("WOWPROGRAMMING_GAME_DIR", "game_dir").and_then(|d| GameData::open(&d).ok())
    else {
        eprintln!("skipping: no game data");
        return None;
    };
    let mut engine = Engine::with_gamedata(
        EngineConfig {
            ui_source_dir: Some(ui),
            wtf_dir: wtf.to_path_buf(),
            blizzard_addons: vec!["*".into()],
            addons_dir: Some(addons),
            only_addons: Some(ADDONS.iter().map(|s| s.to_string()).collect()),
            scenario: Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios/dummy")),
            ..EngineConfig::default()
        },
        Some(std::rc::Rc::new(gamedata)),
    )
    .unwrap();
    engine.start();
    Some(engine)
}

fn errors(e: &mut Engine) -> Vec<String> {
    e.drain_output()
        .into_iter()
        .filter_map(|o| match o {
            Output::Error(e) => Some(e),
            _ => None,
        })
        .collect()
}

fn run(e: &mut Engine, seconds: f64) {
    let steps = (seconds / 0.1).round() as usize;
    for _ in 0..steps {
        e.tick(0.1);
    }
}

fn hold(e: &mut Engine, key: &str, seconds: f64) {
    assert!(e.key_down(key), "no binding for {key}");
    run(e, seconds);
    e.key_up(key);
    e.tick(0.05);
}

fn click(e: &mut Engine, expr: &str, button: &str) {
    let (x, y): (f64, f64) = e.eval(&format!("return ({expr}):GetCenter()")).unwrap();
    e.mouse_move(x, y);
    e.mouse_down(button);
    e.mouse_up(button);
}

#[test]
fn tomtom_clique_and_wowlua_work() {
    let wtf = tempfile::tempdir().unwrap();
    let Some(mut e) = engine(wtf.path()) else {
        return;
    };

    // 1. The three addons load and run 10 seconds without a Lua error.
    run(&mut e, 10.0);
    let errs = errors(&mut e);
    assert!(errs.is_empty(), "Lua errors:\n{}", errs.join("\n"));

    // 2. Maps from the game data; the player stands in Goldshire.
    let (map, zone, width): (i64, String, f64) = e
        .eval("return C_Map.GetBestMapForUnit('player'), GetZoneText(), C_Map.GetMapWorldSize(37)")
        .unwrap();
    assert_eq!((map, zone.as_str()), (37, "Elwynn Forest"));
    assert!(
        (width - 3470.84).abs() < 0.01,
        "Elwynn is {width} yards wide"
    );

    // 4. TomTom: a waypoint 0.05 of the map north of the player
    // (115.7 yards, 105.8 m); the crazy arrow shows its distance.
    e.run_command("/way 42 60 Testpoint");
    run(&mut e, 0.5);
    let (shown, title, status): (bool, String, String) = e
        .eval("return TomTomCrazyArrow:IsShown(), TomTomCrazyArrow.title:GetText(), TomTomCrazyArrow.status:GetText()")
        .unwrap();
    assert_eq!((shown, title.as_str()), (true, "Testpoint"));
    assert_eq!(status, "105m away");

    // 3. Walking north for 5 seconds covers 35 yards; the distance shrinks.
    let x0: f64 = e.eval("return (UnitPosition('player'))").unwrap();
    hold(&mut e, "W", 5.0);
    let (x1, moving): (f64, bool) = e
        .eval("return (UnitPosition('player')), IsPlayerMoving()")
        .unwrap();
    assert!((x1 - x0 - 35.0).abs() < 1.0, "walked {} yards", x1 - x0);
    assert!(!moving);
    let status: String = e.eval("return TomTomCrazyArrow.status:GetText()").unwrap();
    assert_eq!(status, "73m away");

    // Turning left for half a second faces about 90 degrees west.
    hold(&mut e, "A", 0.5);
    let facing: f64 = e.eval("return GetPlayerFacing()").unwrap();
    assert!(
        (facing - std::f64::consts::FRAC_PI_2).abs() < 0.2,
        "facing {facing}"
    );
    hold(&mut e, "D", 0.5);

    // On arrival TomTom clears the waypoint and hides the arrow.
    hold(&mut e, "W", 12.0);
    let shown: bool = e.eval("return TomTomCrazyArrow:IsShown()").unwrap();
    assert!(!shown, "the arrow is still shown after arriving");

    // 5. Clique: a middle click on the player frame casts the bound spell
    // on the player, through Clique's secure snippets.
    e.exec(
        "Clique:AddBinding({ key = 'BUTTON3', type = 'spell', spell = 'Aegis of Dawn', sets = { default = true } })",
    );
    run(&mut e, 0.2);
    click(&mut e, "PlayerFrame", "MiddleButton");
    run(&mut e, 0.2);
    let buff: Option<String> = e
        .eval(
            "local a = C_UnitAuras.GetAuraDataByIndex('player', 1, 'HELPFUL') return a and a.name",
        )
        .unwrap();
    assert_eq!(buff.as_deref(), Some("Aegis of Dawn"));

    // 6. WowLua: open the window, type code, run it, see the output.
    e.run_command("/wowlua");
    run(&mut e, 0.2);
    assert!(e.eval::<bool>("return WowLuaFrame:IsShown()").unwrap());
    click(&mut e, "WowLuaFrameEditBox", "LeftButton");
    e.text_input("print('hello', 6 * 7)");
    click(&mut e, "WowLuaButton_Run", "LeftButton");
    run(&mut e, 0.2);
    let out: String = e
        .eval("return (WowLuaFrameOutput:GetMessageInfo(WowLuaFrameOutput:GetNumMessages()))")
        .unwrap();
    assert!(out.contains("hello, 42"), "WowLua printed {out:?}");

    let errs = errors(&mut e);
    assert!(errs.is_empty(), "Lua errors:\n{}", errs.join("\n"));
}
