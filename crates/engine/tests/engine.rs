use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use wowprogramming_engine::{Engine, EngineConfig, LoadState, Output};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/AddOns")
}

fn config(only: &[&str], wtf: &Path) -> EngineConfig {
    EngineConfig {
        addons_dir: Some(fixtures()),
        wtf_dir: wtf.to_path_buf(),
        blizzard_addons: Vec::new(),
        only_addons: Some(only.iter().map(|s| s.to_string()).collect()),
        ..EngineConfig::default()
    }
}

fn started(only: &[&str], wtf: &Path) -> Engine {
    let mut engine = Engine::new(config(only, wtf)).unwrap();
    engine.start();
    engine
}

fn prints(output: &[Output]) -> Vec<String> {
    output
        .iter()
        .filter_map(|o| match o {
            Output::Print(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

fn errors(output: &[Output]) -> Vec<String> {
    output
        .iter()
        .filter_map(|o| match o {
            Output::Error(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn hello_world_counts_logins_across_sessions() {
    let wtf = tempfile::tempdir().unwrap();

    let mut engine = started(&["HelloWorld"], wtf.path());
    let out = engine.drain_output();
    assert_eq!(
        prints(&out),
        vec!["Hello from HelloWorld, login #1"],
        "{out:?}"
    );
    engine.shutdown();

    let saved = std::fs::read_to_string(
        wtf.path()
            .join("Account/SANDBOX/SavedVariables/HelloWorld.lua"),
    )
    .unwrap();
    assert_eq!(saved, "HelloWorldDB = {\n\t[\"logins\"] = 1,\n}\n");

    let engine = started(&["HelloWorld"], wtf.path());
    assert_eq!(
        prints(&engine.drain_output()),
        vec!["Hello from HelloWorld, login #2"]
    );
}

#[test]
fn slash_commands_dispatch_to_handlers() {
    let wtf = tempfile::tempdir().unwrap();
    let mut engine = started(&["HelloWorld"], wtf.path());
    engine.drain_output();
    engine.run_command("/hello there");
    engine.run_command("/HW again");
    engine.run_command("/nosuchcommand");
    assert_eq!(
        prints(&engine.drain_output()),
        vec![
            "You said: there",
            "You said: again",
            "Unknown command: /nosuchcommand"
        ]
    );
}

#[test]
fn dependencies_load_first() {
    let wtf = tempfile::tempdir().unwrap();
    let engine = started(&["DepChild", "DepParent"], wtf.path());
    let order: Vec<String> = engine.eval("return LoadOrder").unwrap();
    assert_eq!(order, vec!["DepParent", "DepChild"]);
}

#[test]
fn missing_dependency_blocks_only_that_addon() {
    let wtf = tempfile::tempdir().unwrap();
    let engine = started(&["MissingDep", "DepParent"], wtf.path());
    assert_eq!(
        engine.addon_state("MissingDep"),
        Some(LoadState::Failed("DEP_MISSING".into()))
    );
    assert_eq!(engine.addon_state("DepParent"), Some(LoadState::Loaded));
    assert!(
        engine
            .eval::<Option<bool>>("return MissingDepLoaded")
            .unwrap()
            .is_none()
    );
}

#[test]
fn load_on_demand_waits_for_load_addon() {
    let wtf = tempfile::tempdir().unwrap();
    let engine = started(&["LodAddon"], wtf.path());
    assert_eq!(engine.addon_state("LodAddon"), Some(LoadState::NotLoaded));
    let (loaded, reason): (bool, Option<String>) = engine
        .eval("return C_AddOns.LoadAddOn('LodAddon')")
        .unwrap();
    assert!(loaded, "{reason:?}");
    assert!(engine.eval::<bool>("return LodLoaded").unwrap());
}

#[test]
fn addon_metadata_api() {
    let wtf = tempfile::tempdir().unwrap();
    let engine = started(&["HelloWorld"], wtf.path());
    let (version, title, loaded): (String, String, bool) = engine
        .eval(
            "local name, title = C_AddOns.GetAddOnInfo('HelloWorld')
             return C_AddOns.GetAddOnMetadata('HelloWorld', 'version'), title,
                    select(2, C_AddOns.IsAddOnLoaded('HelloWorld'))",
        )
        .unwrap();
    assert_eq!(
        (version.as_str(), title.as_str(), loaded),
        ("1.0", "Hello World", true)
    );
    let count: usize = engine.eval("return C_AddOns.GetNumAddOns()").unwrap();
    assert_eq!(count, 7);
}

#[test]
fn syntax_error_does_not_stop_later_files() {
    let wtf = tempfile::tempdir().unwrap();
    let engine = started(&["Broken"], wtf.path());
    let errs = errors(&engine.drain_output());
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(
        errs[0].starts_with("Interface/AddOns/Broken/SyntaxError.lua:1:"),
        "{}",
        errs[0]
    );
    assert!(engine.eval::<bool>("return BrokenAfterLoaded").unwrap());
}

#[test]
fn error_in_one_handler_does_not_stop_others() {
    let wtf = tempfile::tempdir().unwrap();
    let engine = started(&[], wtf.path());
    engine.exec(
        r#"
        local a = CreateFrame("Frame")
        a:RegisterEvent("TEST_EVENT")
        a:SetScript("OnEvent", function() error("boom") end)
        local b = CreateFrame("Frame")
        b:RegisterEvent("TEST_EVENT")
        b:SetScript("OnEvent", function(self, event, x) Received = x end)
        "#,
    );
    engine.fire_event("TEST_EVENT", 42);
    let errs = errors(&engine.drain_output());
    assert_eq!(errs.len(), 1);
    assert!(errs[0].contains("boom"), "{}", errs[0]);
    assert_eq!(engine.eval::<i64>("return Received").unwrap(), 42);
}

#[test]
fn custom_error_handler_receives_errors() {
    let wtf = tempfile::tempdir().unwrap();
    let engine = started(&[], wtf.path());
    engine.exec(
        r#"
        seterrorhandler(function(msg) Caught = msg end)
        C_Timer.After(0, function() error("late") end)
        "#,
    );
    let mut engine = engine;
    engine.tick(0.016);
    let caught: String = engine.eval("return Caught").unwrap();
    assert!(caught.ends_with("late"), "{caught}");
    assert!(errors(&engine.drain_output()).is_empty());
}

#[test]
fn infinite_loop_in_handler_times_out_and_engine_continues() {
    let wtf = tempfile::tempdir().unwrap();
    let mut cfg = config(&[], wtf.path());
    cfg.script_timeout = Duration::from_millis(200);
    let mut engine = Engine::new(cfg).unwrap();
    engine.start();
    engine.exec(
        r#"
        local a = CreateFrame("Frame")
        a:RegisterEvent("TEST_EVENT")
        a:SetScript("OnEvent", function() while true do end end)
        local b = CreateFrame("Frame")
        b:RegisterEvent("TEST_EVENT")
        b:SetScript("OnEvent", function() SecondRan = true end)
        "#,
    );
    let start = Instant::now();
    engine.fire_event("TEST_EVENT", ());
    assert!(start.elapsed() < Duration::from_secs(5));
    let errs = errors(&engine.drain_output());
    assert!(
        errs.iter().any(|e| e.contains("script ran too long")),
        "{errs:?}"
    );
    assert!(engine.eval::<bool>("return SecondRan").unwrap());
}

#[test]
fn timers_run_on_the_engine_clock() {
    let wtf = tempfile::tempdir().unwrap();
    let mut engine = started(&[], wtf.path());
    engine.exec(
        r#"
        Log = {}
        C_Timer.After(0, function() table.insert(Log, "after0") end)
        C_Timer.After(1, function() table.insert(Log, "after1") end)
        local n = 0
        C_Timer.NewTicker(0.5, function(t) n = n + 1 table.insert(Log, "tick" .. n) end, 3)
        Cancelled = C_Timer.NewTimer(0.25, function() table.insert(Log, "cancelled") end)
        Cancelled:Cancel()
        "#,
    );
    let log = |e: &Engine| -> Vec<String> { e.eval("return Log").unwrap() };
    assert!(
        log(&engine).is_empty(),
        "nothing runs before the first tick"
    );
    engine.tick(0.1);
    assert_eq!(log(&engine), vec!["after0"]);
    for _ in 0..20 {
        engine.tick(0.1);
    }
    assert_eq!(
        log(&engine),
        vec!["after0", "tick1", "after1", "tick2", "tick3"]
    );
    assert!(
        engine
            .eval::<bool>("return Cancelled:IsCancelled()")
            .unwrap()
    );
}

#[test]
fn on_update_runs_only_for_visible_frames() {
    let wtf = tempfile::tempdir().unwrap();
    let mut engine = started(&[], wtf.path());
    engine.exec(
        r#"
        Total, Calls = 0, 0
        Parent = CreateFrame("Frame")
        local child = CreateFrame("Frame", nil, Parent)
        child:SetScript("OnUpdate", function(self, elapsed)
            Total, Calls = Total + elapsed, Calls + 1
        end)
        "#,
    );
    engine.tick(0.25);
    engine.tick(0.25);
    engine.exec("Parent:Hide()");
    engine.tick(0.25);
    let (total, calls): (f64, i64) = engine.eval("return Total, Calls").unwrap();
    assert_eq!((total, calls), (0.5, 2));
    let now: f64 = engine.eval("return GetTime()").unwrap();
    assert_eq!(now, 1000.75);
}

#[test]
fn show_and_hide_fire_scripts_through_the_hierarchy() {
    let wtf = tempfile::tempdir().unwrap();
    let engine = started(&[], wtf.path());
    engine.exec(
        r#"
        Log = {}
        local parent = CreateFrame("Frame", "TestParent")
        local child = CreateFrame("Frame", "$parentChild", parent)
        for _, f in ipairs({ parent, child }) do
            f:SetScript("OnShow", function(self) table.insert(Log, self:GetName() .. ":show") end)
            f:SetScript("OnHide", function(self) table.insert(Log, self:GetName() .. ":hide") end)
        end
        parent:Hide()
        parent:Hide()
        assert(not child:IsVisible() and child:IsShown())
        parent:Show()
        child:Hide()
        "#,
    );
    let log: Vec<String> = engine.eval("return Log").unwrap();
    assert_eq!(
        log,
        vec![
            "TestParent:hide",
            "TestParentChild:hide",
            "TestParent:show",
            "TestParentChild:show",
            "TestParentChild:hide"
        ]
    );
    assert!(errors(&engine.drain_output()).is_empty());
}

#[test]
fn frames_are_tables_with_type_methods() {
    let wtf = tempfile::tempdir().unwrap();
    let engine = started(&[], wtf.path());
    let ok: bool = engine
        .eval(
            r#"
            local b = CreateFrame("Button", "MyButton", UIParent)
            b.custom = 5
            assert(MyButton == b and b.custom == 5)
            assert(b:GetObjectType() == "Button" and b:IsObjectType("Frame"))
            assert(b:GetParent() == UIParent)
            assert(type(b[0]) == "userdata")
            b:SetPoint("CENTER", UIParent, "TOP", 10, -20)
            local p, rel, rp, x, y = b:GetPoint(1)
            assert(p == "CENTER" and rel == UIParent and rp == "TOP" and x == 10 and y == -20)
            b:SetPoint("left", 5, 6)
            assert(b:GetNumPoints() == 2)
            assert(not pcall(b.GetName, nil))
            assert(not pcall(CreateFrame, "NoSuchType"))
            -- Hooking a type's method affects frames of that type only.
            local hooked = 0
            hooksecurefunc(getmetatable(b).__index, "Show", function() hooked = hooked + 1 end)
            b:Show()
            CreateFrame("Frame"):Show()
            return hooked == 1
            "#,
        )
        .unwrap();
    assert!(ok);
}

#[test]
fn hook_script_runs_after_handler() {
    let wtf = tempfile::tempdir().unwrap();
    let engine = started(&[], wtf.path());
    engine.exec(
        r#"
        Log = {}
        local f = CreateFrame("Frame")
        f:RegisterEvent("E")
        f:SetScript("OnEvent", function() table.insert(Log, "handler") end)
        f:HookScript("OnEvent", function() table.insert(Log, "hook") end)
        "#,
    );
    engine.fire_event("E", ());
    let log: Vec<String> = engine.eval("return Log").unwrap();
    assert_eq!(log, vec!["handler", "hook"]);
}

#[test]
fn unit_events_filter_by_unit() {
    let wtf = tempfile::tempdir().unwrap();
    let engine = started(&[], wtf.path());
    engine.exec(
        r#"
        Units = {}
        local f = CreateFrame("Frame")
        f:RegisterUnitEvent("UNIT_HEALTH", "player")
        f:SetScript("OnEvent", function(_, _, unit) table.insert(Units, unit) end)
        "#,
    );
    engine.fire_event("UNIT_HEALTH", "target");
    engine.fire_event("UNIT_HEALTH", "player");
    let units: Vec<String> = engine.eval("return Units").unwrap();
    assert_eq!(units, vec!["player"]);
}

#[test]
fn reload_keeps_saved_variables() {
    let wtf = tempfile::tempdir().unwrap();
    let mut engine = started(&["HelloWorld"], wtf.path());
    engine.exec("TempGlobal = 1");
    engine.run_command("/reload");
    let out = engine.drain_output();
    assert_eq!(
        prints(&out),
        vec![
            "Hello from HelloWorld, login #1",
            "Hello from HelloWorld, login #2"
        ]
    );
    assert!(
        engine
            .eval::<Option<i64>>("return TempGlobal")
            .unwrap()
            .is_none()
    );
}

#[test]
fn dump_prints_values() {
    let wtf = tempfile::tempdir().unwrap();
    let mut engine = started(&[], wtf.path());
    engine.run_command("/dump 1 + 1, { a = true }");
    assert_eq!(
        prints(&engine.drain_output()),
        vec![
            "Dump: 1 + 1, { a = true }",
            "[1] = 2",
            "[2] = {\n\t[\"a\"] = true,\n}"
        ]
    );
}

#[test]
fn errors_from_native_functions_are_strings() {
    let wtf = tempfile::tempdir().unwrap();
    let engine = started(&[], wtf.path());
    let (ok, msg, kind): (bool, String, String) = engine
        .eval(
            r#"local ok, msg = pcall(function() CreateFrame("Frame"):SetScale("x") end)
               return ok, msg, type(msg)"#,
        )
        .unwrap();
    assert!(!ok);
    assert_eq!(kind, "string");
    assert_eq!(msg, "bad argument #2 to '?' (number expected, got string)");
}

#[test]
fn error_traces_stop_at_addon_code() {
    let wtf = tempfile::tempdir().unwrap();
    let mut engine = started(&[], wtf.path());
    engine.run_command("/run local function f() error('deep') end f()");
    let errs = errors(&engine.drain_output());
    assert_eq!(
        errs,
        vec![
            "[string \"local function f() error('deep') end f()\"]:1: deep\n\
             [C]: in function `error'\n\
             [string \"local function f() error('deep') end f()\"]:1: in function `f'\n\
             [string \"local function f() error('deep') end f()\"]:1: in main chunk"
        ]
    );
}

#[test]
fn curves_interpolate() {
    let wtf = tempfile::tempdir().unwrap();
    let engine = started(&[], wtf.path());
    let values: Vec<f64> = engine
        .eval(
            r#"local c = C_CurveUtil.CreateCurve()
               c:AddPoint(1, 100) c:AddPoint(0, 0)
               local s = c:Copy() s:SetType(1)
               return { c:Evaluate(-1), c:Evaluate(0.25), c:Evaluate(2), s:Evaluate(0.5) }"#,
        )
        .unwrap();
    assert_eq!(values, vec![0.0, 25.0, 100.0, 0.0]);
}

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

/// Loads Blizzard_SharedXMLBase from a local UI source checkout, together
/// with an addon that has a syntax error. Skipped when the checkout is not
/// present (for example on CI).
#[test]
fn loads_blizzard_shared_xml_base() {
    let Some(ui) = ui_source() else { return };
    let wtf = tempfile::tempdir().unwrap();
    let mut engine = Engine::new(EngineConfig {
        ui_source_dir: Some(ui),
        blizzard_addons: vec!["Blizzard_SharedXMLBase".into()],
        ..config(&["Broken"], wtf.path())
    })
    .unwrap();
    engine.start();
    let out = engine.drain_output();
    for line in &out {
        eprintln!("{line}");
    }
    assert_eq!(
        engine.addon_state("Blizzard_SharedXMLBase"),
        Some(LoadState::Loaded)
    );
    assert_eq!(
        engine.addon_state("Blizzard_ScriptErrors"),
        Some(LoadState::Loaded)
    );
    // Blizzard's error handler is installed, and errors must still be shown.
    let errs = errors(&out);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].contains("Broken/SyntaxError.lua:1:"), "{}", errs[0]);
    // The only warnings are for XML files, which are not supported yet.
    for o in &out {
        if let Output::Warning(w) = o {
            assert!(w.contains("XML files are not supported yet"), "{w}");
        }
    }
    let (healer, meta, auction): (i64, i64, f64) = engine
        .eval(
            "return Enum.LFGRole.Healer, Enum.LFGRoleMeta.NumValues,
                    Constants.AuctionConstants.DEFAULT_AUCTION_PRICE_MULTIPLIER",
        )
        .unwrap();
    assert_eq!((healer, meta, auction), (1, 3, 1.5));
}

#[test]
fn only_opposite_edges_set_a_size() {
    let wtf = tempfile::tempdir().unwrap();
    let engine = started(&[], wtf.path());
    let ok: bool = engine
        .eval(
            r#"
            local parent = CreateFrame("Frame", nil, UIParent)
            parent:SetSize(200, 100)
            parent:SetPoint("CENTER")
            -- TOP with LEFT and RIGHT: the width comes from the edges, the
            -- height stays the frame's own.
            local f = CreateFrame("Frame", nil, parent)
            f:SetHeight(30)
            f:SetPoint("TOP")
            f:SetPoint("LEFT")
            f:SetPoint("RIGHT")
            local w, h = f:GetSize()
            return w == 200 and h == 30
            "#,
        )
        .unwrap();
    assert!(ok);
}

#[test]
fn buttons_and_mouse_scripts_enable_the_mouse() {
    let wtf = tempfile::tempdir().unwrap();
    let engine = started(&[], wtf.path());
    let ok: bool = engine
        .eval(
            r#"
            local b = CreateFrame("Button")
            local f = CreateFrame("Frame")
            local before = f:IsMouseEnabled()
            f:SetScript("OnMouseDown", function() end)
            return b:IsMouseEnabled() and not before and f:IsMouseEnabled()
            "#,
        )
        .unwrap();
    assert!(ok);
}

#[test]
fn animations_change_alpha_and_position_while_playing() {
    let wtf = tempfile::tempdir().unwrap();
    let mut engine = started(&[], wtf.path());
    engine.exec(
        r#"
        AnimFrame = CreateFrame("Frame", nil, UIParent)
        AnimFrame:SetSize(10, 10)
        AnimFrame:SetPoint("BOTTOMLEFT", 100, 100)
        AnimTex = AnimFrame:CreateTexture()
        AnimTex:SetAllPoints()
        AnimTex:SetColorTexture(1, 1, 1)
        local g = AnimFrame:CreateAnimationGroup()
        local a = g:CreateAnimation("Alpha")
        a:SetFromAlpha(0)
        a:SetToAlpha(1)
        a:SetDuration(1)
        local t = g:CreateAnimation("Translation")
        t:SetOffset(50, 0)
        t:SetDuration(1)
        g:Play()
        "#,
    );
    engine.tick(0.5);
    let scene = engine.scene();
    let quad = scene
        .items
        .iter()
        .find_map(|i| match i {
            wowprogramming_engine::scene::Item::Quad(q) => Some(q.clone()),
            _ => None,
        })
        .expect("the texture is drawn");
    assert!(
        (quad.color[3] - 0.5).abs() < 0.05,
        "alpha {}",
        quad.color[3]
    );
    assert!(
        (quad.rect.left - 125.0).abs() < 1.0,
        "left {}",
        quad.rect.left
    );
}

#[test]
fn masks_and_atlas_coordinates_reach_the_scene() {
    let wtf = tempfile::tempdir().unwrap();
    let engine = started(&[], wtf.path());
    engine.exec(
        r#"
        local f = CreateFrame("Frame", nil, UIParent)
        f:SetSize(10, 10)
        f:SetPoint("CENTER")
        local t = f:CreateTexture()
        t:SetAllPoints()
        t:SetColorTexture(1, 0, 0)
        local m = f:CreateMaskTexture()
        m:SetSize(4, 4)
        m:SetPoint("CENTER")
        t:AddMaskTexture(m)
        "#,
    );
    let scene = engine.scene();
    let quad = scene
        .items
        .iter()
        .find_map(|i| match i {
            wowprogramming_engine::scene::Item::Quad(q) => Some(q.clone()),
            _ => None,
        })
        .expect("the texture is drawn");
    assert_eq!(quad.masks.len(), 1);
    assert!((quad.masks[0].rect.width - 4.0).abs() < 1e-6);
}

#[test]
fn replay_keeps_widget_metatables_and_keyboard_focus() {
    // Captures can record functions that return objects the sandbox owns.
    // The recorded copies are plain tables and must not replace them.
    let wtf = tempfile::tempdir().unwrap();
    let scenario = tempfile::tempdir().unwrap();
    std::fs::write(
        scenario.path().join("scenario.lua"),
        r##"local dir, S = ...
S.Replay({
    GetFontStringMetatable = { ["#"] = 1, [""] = { n = 1, ["1"] = { __index = {} } } },
    GetCurrentKeyBoardFocus = { ["#"] = 1, [""] = { n = 1, ["1"] = { chatFrame = {} } } },
    GetLocale = { ["#"] = 1, [""] = { n = 1, ["1"] = "xxXX" } },
})
"##,
    )
    .unwrap();
    let mut cfg = config(&[], wtf.path());
    cfg.scenario = Some(scenario.path().to_path_buf());
    let mut engine = Engine::new(cfg).unwrap();
    engine.start();
    let got: (String, bool, String) = engine
        .eval(
            "return type(GetFontStringMetatable().__index.SetText), \
             GetCurrentKeyBoardFocus() == nil, GetLocale()",
        )
        .unwrap();
    assert_eq!(got, ("function".into(), true, "xxXX".into()));
}

#[test]
fn coverage_report_classifies_methods_and_handlers() {
    let Some(ui) = ui_source() else { return };
    let wtf = tempfile::tempdir().unwrap();
    let engine = Engine::new(EngineConfig {
        ui_source_dir: Some(ui.clone()),
        blizzard_addons: Vec::new(),
        ..config(&[], wtf.path())
    })
    .unwrap();
    let xsd =
        std::fs::read_to_string(ui.join("Interface/AddOns/Blizzard_SharedXML/UI.xsd")).unwrap();
    let uses = wowprogramming_engine::coverage::Uses::default();
    let report = wowprogramming_engine::coverage::collect(engine.lua(), &xsd, &uses).unwrap();
    use wowprogramming_engine::coverage::MethodState;
    let state = |doc: &str, m: &str| {
        report
            .methods
            .iter()
            .find(|r| r.doc == doc && r.method == m)
            .map(|r| r.state)
    };
    assert_eq!(
        state("SimpleScriptRegionResizingAPI", "SetPoint"),
        Some(MethodState::Implemented)
    );
    assert_eq!(
        state("SimpleColorSelectAPI", "GetColorRGB"),
        Some(MethodState::Stub)
    );
    let on_click = report
        .handlers
        .iter()
        .find(|h| h.name == "OnClick")
        .unwrap();
    assert!(on_click.fired);
    assert!(report.handlers.len() > 70);
}

#[test]
fn animation_targets_follow_child_keys() {
    let wtf = tempfile::tempdir().unwrap();
    let engine = started(&[], wtf.path());
    let result: String = engine
        .eval(
            r#"
            local f = CreateFrame("Frame", "AnimOwner", UIParent)
            f.Glow = f:CreateTexture()
            local g = f:CreateAnimationGroup()
            local keyed = g:CreateAnimation("Alpha")
            keyed:SetChildKey("Glow")
            local missing = g:CreateAnimation("Alpha")
            missing:SetChildKey("Nothing")
            local plain = g:CreateAnimation("Alpha")
            return tostring(keyed:GetTarget() == f.Glow) .. " "
                .. tostring(missing:GetTarget() == f) .. " "
                .. tostring(plain:GetTarget() == f)
            "#,
        )
        .unwrap();
    assert_eq!(result, "true true true");
}

#[test]
fn parent_level_children_follow_their_parent() {
    let wtf = tempfile::tempdir().unwrap();
    let engine = started(&[], wtf.path());
    let result: String = engine
        .eval(
            r#"
            local p = CreateFrame("Frame", nil, UIParent)
            local border = CreateFrame("Frame", nil, p)
            border:SetUsingParentLevel(true)
            local child = CreateFrame("Frame", nil, p)
            p:SetFrameLevel(7)
            return border:GetFrameLevel() .. " " .. child:GetFrameLevel()
                .. " " .. tostring(border:IsUsingParentLevel())
            "#,
        )
        .unwrap();
    assert_eq!(result, "7 8 true");
}
