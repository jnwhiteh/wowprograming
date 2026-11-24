//! Interactions in the full default UI, driven like a user would. These
//! tests load every Blizzard addon from a local UI source checkout and
//! are skipped without one (as on CI). One engine serves all checks,
//! because startup takes about 20 seconds.

use std::path::{Path, PathBuf};

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

fn full_ui(wtf: &Path) -> Option<Engine> {
    let ui = ui_source()?;
    let mut engine = Engine::new(EngineConfig {
        ui_source_dir: Some(ui),
        wtf_dir: wtf.to_path_buf(),
        blizzard_addons: vec!["*".into()],
        scenario: Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios/hud")),
        ..EngineConfig::default()
    })
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
}
