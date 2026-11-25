//! Interaction specs (`Engine::run_spec`, `wowprogramming test`): Lua
//! files that drive the UI and check the result.

use std::path::Path;

use wowprogramming_engine::{Engine, EngineConfig};

fn started(wtf: &Path) -> Engine {
    let mut engine = Engine::new(EngineConfig {
        wtf_dir: wtf.to_path_buf(),
        blizzard_addons: Vec::new(),
        only_addons: Some(Vec::new()),
        ..EngineConfig::default()
    })
    .unwrap();
    engine.start();
    engine
}

fn spec(dir: &Path, name: &str, code: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, code).unwrap();
    path
}

#[test]
fn a_spec_clicks_waits_and_checks() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = started(dir.path());
    e.exec(
        "Clicks = 0
         local b = CreateFrame('Button', 'SpecButton', UIParent)
         b:SetSize(100, 40) b:SetPoint('CENTER')
         b:SetScript('OnClick', function() Clicks = Clicks + 1 end)",
    );
    let ok = spec(
        dir.path(),
        "ok.lua",
        "Click('SpecButton') Click(SpecButton)
         local t = GetTime() Wait(2)
         Near(GetTime() - t, 2, 0.05, 'time passed')
         Equal(Clicks, 2)",
    );
    assert_eq!(e.run_spec(&ok), Ok(()));

    let bad = spec(dir.path(), "bad.lua", "Equal(Clicks, 3, 'clicks')");
    let err = e.run_spec(&bad).unwrap_err();
    assert!(err.contains("bad.lua:1"), "{err}");
    assert!(err.contains("clicks: expected 3, got 2"), "{err}");

    // A Lua error in UI code during the spec fails it too.
    let ui_error = spec(
        dir.path(),
        "ui_error.lua",
        "C_Timer.After(0.1, function() error('boom') end) Wait(0.5)",
    );
    let err = e.run_spec(&ui_error).unwrap_err();
    assert!(err.contains("boom"), "{err}");
}
