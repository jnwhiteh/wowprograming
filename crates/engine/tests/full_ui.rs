//! Interactions in the full default UI, driven like a user would. These
//! tests load every Blizzard addon from a local UI source checkout and
//! are skipped without one (as on CI). One engine serves all checks,
//! because startup takes about 20 seconds.

use std::path::{Path, PathBuf};

use wowprogramming_engine::gamedata::GameData;
use wowprogramming_engine::{Engine, EngineConfig};

fn ui_source() -> Option<PathBuf> {
    let ui = std::env::var_os("WOWPROGRAMMING_UI_SOURCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../wow-ui-source-retail")
        });
    if ui.join("Interface/AddOns/Blizzard_SharedXMLBase").is_dir() {
        Some(ui)
    } else {
        eprintln!("skipping: no UI source at {}", ui.display());
        None
    }
}

/// The game install from `config.local.toml` (or
/// `WOWPROGRAMMING_GAME_DIR`): the global strings come from its data.
fn game_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("WOWPROGRAMMING_GAME_DIR") {
        return Some(PathBuf::from(d));
    }
    let config = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config.local.toml");
    let text = std::fs::read_to_string(config).ok()?;
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with("game_dir"))?;
    let value = line.split_once('=')?.1.trim().trim_matches('"');
    Some(PathBuf::from(value))
}

fn full_ui(wtf: &Path) -> Option<Engine> {
    let ui = ui_source()?;
    let Some(gamedata) = game_dir().and_then(|d| GameData::open(&d).ok()) else {
        eprintln!("skipping: no game data (config.local.toml game_dir)");
        return None;
    };
    let mut engine = Engine::with_gamedata(
        EngineConfig {
            ui_source_dir: Some(ui),
            wtf_dir: wtf.to_path_buf(),
            blizzard_addons: vec!["*".into()],
            scenario: Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios/hud")),
            ..EngineConfig::default()
        },
        Some(std::rc::Rc::new(gamedata)),
    )
    .unwrap();
    engine.start();
    engine.tick(0.1);
    engine.drain_output();
    Some(engine)
}

/// The centre of a frame, from a Lua expression.
fn center(e: &Engine, expr: &str) -> (f64, f64) {
    e.eval(&format!("return ({expr}):GetCenter()")).unwrap()
}

fn click(e: &mut Engine, expr: &str) {
    let (x, y) = center(e, expr);
    e.mouse_move(x, y);
    e.mouse_down("LeftButton");
    e.mouse_up("LeftButton");
}

#[test]
fn full_ui_interactions() {
    let wtf = tempfile::tempdir().unwrap();
    let Some(mut e) = full_ui(wtf.path()) else {
        return;
    };

    // Toplevel windows: clicking the lower of two open panels raises it.
    e.exec("ToggleCharacter('PaperDollFrame') ToggleFriendsFrame()");
    let (lower, upper) = {
        let (c, f): (i64, i64) = e
            .eval("return CharacterFrame:GetFrameLevel(), FriendsFrame:GetFrameLevel()")
            .unwrap();
        if c < f {
            ("CharacterFrame", "FriendsFrame")
        } else {
            ("FriendsFrame", "CharacterFrame")
        }
    };
    click(&mut e, lower);
    let (l, u): (i64, i64) = e
        .eval(&format!(
            "return {lower}:GetFrameLevel(), {upper}:GetFrameLevel()"
        ))
        .unwrap();
    assert!(l > u, "{lower} {l} is not above {upper} {u}");

    // Escape closes the open windows.
    assert!(e.key_down("ESCAPE"));
    e.key_up("ESCAPE");
    let shown: (bool, bool) = e
        .eval("return CharacterFrame:IsShown(), FriendsFrame:IsShown()")
        .unwrap();
    assert_eq!(shown, (false, false));

    // A keyboard-enabled frame that does not propagate blocks the
    // bindings: C does not open the character window.
    e.exec(
        "KeyBlocker = CreateFrame('Frame', nil, UIParent)
         KeyBlocker:EnableKeyboard(true)
         KeyBlocker:SetScript('OnKeyDown', function(_, key) KeyBlockerGot = key end)",
    );
    e.key_down("C");
    e.key_up("C");
    let (got, open): (String, bool) = e
        .eval("return KeyBlockerGot, CharacterFrame:IsShown()")
        .unwrap();
    assert_eq!((got.as_str(), open), ("C", false));
    e.exec("KeyBlocker:SetPropagateKeyboardInput(true)");
    e.key_down("C");
    e.key_up("C");
    let open: bool = e.eval("return CharacterFrame:IsShown()").unwrap();
    assert!(
        open,
        "C should open the character window when the key propagates"
    );
    e.exec("KeyBlocker:Hide()");
    e.key_down("ESCAPE");
    e.key_up("ESCAPE");

    settings_panel(&mut e);
    edit_mode(&mut e);
}

/// Settings: drag the list's scroll bar thumb, then a slider's thumb.
fn settings_panel(e: &mut Engine) {
    e.exec("SettingsPanel:Open()");
    e.tick(0.1);
    e.exec(
        "SettingsScrollBar = SettingsPanel.Container.SettingsList.ScrollBar
         SettingsScrollBox = SettingsPanel.Container.SettingsList.ScrollBox",
    );
    let before: f64 = e
        .eval("return SettingsScrollBox:GetScrollPercentage()")
        .unwrap();
    assert_eq!(before, 0.0);
    let thumb = center(e, "SettingsScrollBar:GetThumb()");
    e.drag(thumb, (0.0, -50.0), "LeftButton", 10);
    let (bar, list): (f64, f64) = e
        .eval("return SettingsScrollBar:GetScrollPercentage(), SettingsScrollBox:GetScrollPercentage()")
        .unwrap();
    assert!(
        bar > 0.0 && (bar - list).abs() < 1e-9,
        "scroll bar {bar}, list {list}"
    );
    e.exec("SettingsScrollBar:SetScrollPercentage(0)");
    e.tick(0.1);

    // The first visible slider (Mouse Look Speed in the Controls page).
    e.exec(
        "local function walk(f)
             if SettingsSlider or not f:IsVisible() then return end
             if f:GetObjectType() == 'Slider' then SettingsSlider = f return end
             for _, c in ipairs({ f:GetChildren() }) do walk(c) end
         end
         walk(SettingsPanel)",
    );
    let (value, min, max): (f64, f64, f64) = e
        .eval("local min, max = SettingsSlider:GetMinMaxValues() return SettingsSlider:GetValue(), min, max")
        .unwrap();
    let thumb = center(e, "SettingsSlider:GetThumbTexture()");
    let width: f64 = e.eval("return SettingsSlider:GetWidth()").unwrap();
    e.drag(thumb, (width / 2.0, 0.0), "LeftButton", 10);
    let after: f64 = e.eval("return SettingsSlider:GetValue()").unwrap();
    assert!(
        after > value && after <= max,
        "slider {value} -> {after} in {min}..{max}"
    );
    e.exec("SettingsPanel:Close(true)");
}

/// Enter Edit Mode, drag the player frame, save it as a new layout,
/// leave, and find it in the same place after /reload.
fn edit_mode(e: &mut Engine) {
    e.exec("ShowUIPanel(EditModeManagerFrame)");
    let before: (f64, f64) = e
        .eval("return PlayerFrame:GetLeft(), PlayerFrame:GetBottom()")
        .unwrap();
    let from = center(e, "PlayerFrame.Selection");
    e.drag(from, (300.0, 200.0), "LeftButton", 10);
    let moved: (f64, f64) = e
        .eval("return PlayerFrame:GetLeft(), PlayerFrame:GetBottom()")
        .unwrap();
    assert_eq!(moved, (before.0 + 300.0, before.1 + 200.0));
    // The active layout is a preset, so Save asks for a new layout name.
    click(e, "EditModeManagerFrame.SaveChangesButton");
    assert!(
        e.eval::<bool>("return EditModeLayoutDialog.LayoutNameEditBox:HasFocus()")
            .unwrap()
    );
    e.text_input("Sandbox Layout");
    click(e, "EditModeLayoutDialog.AcceptButton");
    e.tick(0.1);
    let (preset, name): (bool, String) = e
        .eval("return EditModeManagerFrame:IsActiveLayoutPreset(), EditModeManagerFrame:GetActiveLayoutInfo().layoutName")
        .unwrap();
    assert_eq!((preset, name.as_str()), (false, "Sandbox Layout"));
    e.key_down("ESCAPE");
    e.key_up("ESCAPE");
    assert!(
        !e.eval::<bool>("return EditModeManagerFrame:IsShown()")
            .unwrap()
    );
    let after: (f64, f64) = e
        .eval("return PlayerFrame:GetLeft(), PlayerFrame:GetBottom()")
        .unwrap();
    assert_eq!(after, moved);
    e.reload();
    e.tick(0.1);
    let reloaded: (f64, f64) = e
        .eval("return PlayerFrame:GetLeft(), PlayerFrame:GetBottom()")
        .unwrap();
    assert_eq!(reloaded, moved);
}
