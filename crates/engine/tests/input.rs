//! Mouse input rules: capture, click registrations, double clicks,
//! dragging, moving, sizing, clamping and raising. The rules and their
//! sources are in docs/client-runtime.md.

use std::path::Path;

use wowprogramming_engine::{Engine, EngineConfig, Output};

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

/// The `Log` table that test frames append to, joined with spaces.
fn log(engine: &Engine) -> String {
    engine
        .eval::<String>("local s = table.concat(Log, ' '); Log = {}; return s")
        .unwrap()
}

fn errors(engine: &Engine) -> Vec<String> {
    engine
        .drain_output()
        .into_iter()
        .filter_map(|o| match o {
            Output::Error(s) => Some(s),
            _ => None,
        })
        .collect()
}

/// Two buttons side by side: A at x 100..200, B at x 300..400, y 100..200.
/// Every handler appends to `Log`.
const TWO_BUTTONS: &str = r##"
    Log = {}
    local function make(name, x)
        local b = CreateFrame("Button", name, UIParent)
        b:SetSize(100, 100)
        b:SetPoint("BOTTOMLEFT", x, 100)
        for _, s in ipairs({"OnMouseDown", "OnMouseUp", "PreClick", "OnClick", "PostClick",
                            "OnDragStart", "OnDragStop", "OnReceiveDrag", "OnEnter", "OnLeave"}) do
            b:SetScript(s, function(self, ...)
                local args = {}
                for i = 1, select("#", ...) do args[i] = tostring((select(i, ...))) end
                table.insert(Log, name .. ":" .. s .. "(" .. table.concat(args, ",") .. ")")
            end)
        end
        return b
    end
    make("A", 100)
    make("B", 300)
"##;

#[test]
fn a_click_runs_its_scripts_in_order() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(TWO_BUTTONS);
    e.mouse_move(150.0, 150.0);
    log(&e);
    e.mouse_down("LeftButton");
    e.mouse_up("LeftButton");
    assert_eq!(
        log(&e),
        "A:OnMouseDown(LeftButton) A:OnMouseUp(LeftButton,true) A:PreClick(LeftButton,false) \
         A:OnClick(LeftButton,false) A:PostClick(LeftButton,false)"
    );
}

#[test]
fn the_pressed_frame_captures_the_release() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(TWO_BUTTONS);
    e.mouse_move(150.0, 150.0);
    e.mouse_down("LeftButton");
    e.mouse_move(350.0, 150.0);
    log(&e);
    e.mouse_up("LeftButton");
    // A gets the release with upInside false, and no click; B gets nothing.
    assert_eq!(log(&e), "A:OnMouseUp(LeftButton,false)");
}

#[test]
fn click_registrations_choose_which_clicks_fire() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(TWO_BUTTONS);
    e.mouse_move(150.0, 150.0);
    log(&e);
    // The default registration is LeftButtonUp only.
    e.mouse_down("RightButton");
    e.mouse_up("RightButton");
    assert_eq!(
        log(&e),
        "A:OnMouseDown(RightButton) A:OnMouseUp(RightButton,true)"
    );
    // A Down registration clicks on the press.
    e.exec("A:RegisterForClicks('AnyDown')");
    e.mouse_down("RightButton");
    assert_eq!(
        log(&e),
        "A:OnMouseDown(RightButton) A:PreClick(RightButton,true) \
         A:OnClick(RightButton,true) A:PostClick(RightButton,true)"
    );
    e.mouse_up("RightButton");
    assert_eq!(log(&e), "A:OnMouseUp(RightButton,true)");
    // No arguments: no clicks at all.
    e.exec("A:RegisterForClicks()");
    e.mouse_down("LeftButton");
    e.mouse_up("LeftButton");
    assert_eq!(
        log(&e),
        "A:OnMouseDown(LeftButton) A:OnMouseUp(LeftButton,true)"
    );
}

#[test]
fn double_clicks_replace_every_second_click() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(
        r#"
        Log = {}
        local b = CreateFrame("Button", "D", UIParent)
        b:SetSize(100, 100)
        b:SetPoint("BOTTOMLEFT", 100, 100)
        b:SetScript("OnClick", function() table.insert(Log, "click") end)
        b:SetScript("OnDoubleClick", function(_, button) table.insert(Log, "double:" .. button) end)
        "#,
    );
    e.mouse_move(150.0, 150.0);
    for _ in 0..3 {
        e.mouse_down("LeftButton");
        e.mouse_up("LeftButton");
        e.tick(0.1);
    }
    assert_eq!(log(&e), "click double:LeftButton click");
    // Too slow for a double click.
    e.tick(1.0);
    e.mouse_down("LeftButton");
    e.mouse_up("LeftButton");
    assert_eq!(log(&e), "click");
}

#[test]
fn a_drag_replaces_the_release_and_click() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(TWO_BUTTONS);
    e.exec("A:RegisterForDrag('LeftButton')");
    e.mouse_move(150.0, 150.0);
    log(&e);
    e.drag((150.0, 150.0), (200.0, 0.0), "LeftButton", 10);
    let l = log(&e);
    assert!(
        l.starts_with("A:OnMouseDown(LeftButton) A:OnDragStart(LeftButton)"),
        "{l}"
    );
    assert!(l.ends_with("A:OnDragStop() B:OnReceiveDrag()"), "{l}");
    assert!(!l.contains("OnClick") && !l.contains("OnMouseUp"), "{l}");
}

#[test]
fn a_small_move_is_still_a_click() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(TWO_BUTTONS);
    e.exec("A:RegisterForDrag('LeftButton')");
    e.mouse_move(150.0, 150.0);
    log(&e);
    e.drag((150.0, 150.0), (2.0, 1.0), "LeftButton", 2);
    let l = log(&e);
    assert!(l.contains("A:OnClick(LeftButton,false)"), "{l}");
    assert!(!l.contains("OnDragStart"), "{l}");
}

#[test]
fn frames_appearing_under_a_still_mouse_get_on_enter_without_motion() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(TWO_BUTTONS);
    e.exec("A:Hide()");
    e.mouse_move(150.0, 150.0);
    log(&e);
    e.exec("A:Show()");
    e.tick(0.01);
    assert_eq!(log(&e), "A:OnEnter(false)");
    e.mouse_move(350.0, 150.0);
    assert_eq!(log(&e), "A:OnLeave(true) B:OnEnter(true)");
}

/// A movable window that moves with the left button, like BugSack's.
const WINDOW: &str = r#"
    W = CreateFrame("Frame", "TestWindow", UIParent)
    W:SetSize(200, 100)
    W:SetPoint("CENTER")
    W:EnableMouse(true)
    W:SetMovable(true)
    W:SetClampedToScreen(true)
    W:RegisterForDrag("LeftButton")
    W:SetScript("OnDragStart", function(self) self:StartMoving() end)
    W:SetScript("OnDragStop", function(self) self:StopMovingOrSizing() end)
"#;

#[test]
fn a_movable_window_follows_the_mouse_and_anchors_to_the_nearest_point() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(WINDOW);
    // The screen is 1920x1080; the window's centre is at 960, 540.
    e.drag((960.0, 540.0), (-700.0, 300.0), "LeftButton", 10);
    let (l, b, w, h): (f64, f64, f64, f64) = e.eval("return W:GetRect()").unwrap();
    assert_eq!((l, b, w, h), (160.0, 790.0, 200.0, 100.0));
    let (point, rel, x, y, placed): (String, String, f64, f64, bool) = e
        .eval(
            "local p, r, rp, x, y = W:GetPoint(1)
             return p, r:GetName() .. ':' .. rp, x, y, W:IsUserPlaced()",
        )
        .unwrap();
    assert_eq!(
        (point.as_str(), rel.as_str(), x, y, placed),
        ("TOPLEFT", "UIParent:TOPLEFT", 160.0, -190.0, true)
    );
    assert_eq!(e.eval::<i64>("return W:GetNumPoints()").unwrap(), 1);
}

#[test]
fn a_clamped_window_stays_on_screen() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(WINDOW);
    e.drag((960.0, 540.0), (-2000.0, 2000.0), "LeftButton", 10);
    let (l, b): (f64, f64) = e.eval("local l, b = W:GetRect() return l, b").unwrap();
    assert_eq!((l, b), (0.0, 980.0));
    // SetPoint far off screen is clamped too; insets let a part go off.
    e.exec("W:ClearAllPoints() W:SetPoint('BOTTOMLEFT', 5000, -5000)");
    let (l, b): (f64, f64) = e.eval("local l, b = W:GetRect() return l, b").unwrap();
    assert_eq!((l, b), (1720.0, 0.0));
    e.exec("W:SetClampRectInsets(0, -50, 0, 30)");
    let (l, b): (f64, f64) = e.eval("local l, b = W:GetRect() return l, b").unwrap();
    assert_eq!((l, b), (1770.0, -30.0));
}

#[test]
fn start_moving_needs_a_movable_frame() {
    let wtf = tempfile::tempdir().unwrap();
    let e = started(wtf.path());
    let msg: String = e
        .eval(
            "local f = CreateFrame('Frame')
             local ok, err = pcall(f.StartMoving, f)
             return err",
        )
        .unwrap();
    assert!(msg.contains("Frame is not movable"), "{msg}");
}

#[test]
fn sizing_follows_the_mouse_within_the_bounds() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(
        r#"
        R = CreateFrame("Frame", nil, UIParent)
        R:SetSize(200, 100)
        R:SetPoint("BOTTOMLEFT", 100, 100)
        R:EnableMouse(true)
        R:SetResizable(true)
        R:SetResizeBounds(150, 80, 300, 0)
        R:SetScript("OnMouseDown", function(self) self:StartSizing("BOTTOMRIGHT") end)
        R:SetScript("OnMouseUp", function(self) self:StopMovingOrSizing() end)
        "#,
    );
    // Drag the bottom right corner 50 right and 30 down.
    e.drag((290.0, 110.0), (50.0, -30.0), "LeftButton", 5);
    let (l, b, w, h): (f64, f64, f64, f64) = e.eval("return R:GetRect()").unwrap();
    assert_eq!((l, b, w, h), (100.0, 70.0, 250.0, 130.0));
    // The maximum width holds.
    e.drag((340.0, 80.0), (500.0, 0.0), "LeftButton", 5);
    let w: f64 = e.eval("return R:GetWidth()").unwrap();
    assert_eq!(w, 300.0);
}

#[test]
fn hiding_a_moving_frame_stops_the_move() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(WINDOW);
    e.mouse_move(960.0, 540.0);
    e.mouse_down("LeftButton");
    e.mouse_move(1000.0, 540.0);
    e.exec("W:Hide()");
    e.mouse_move(1200.0, 540.0);
    e.mouse_up("LeftButton");
    let l: f64 = e.eval("return (W:GetRect())").unwrap();
    assert_eq!(l, 900.0);
}

#[test]
fn clicking_the_lower_toplevel_window_raises_it() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(
        r#"
        local function window(name, x)
            local f = CreateFrame("Frame", name, UIParent)
            f:SetToplevel(true)
            f:SetSize(300, 300)
            f:SetPoint("BOTTOMLEFT", x, 100)
            f:EnableMouse(true)
            return f
        end
        window("Lower", 100)
        window("Upper", 200)
        "#,
    );
    // Upper was shown last, so it is raised above Lower.
    let (lower, upper): (i64, i64) = e
        .eval("return Lower:GetFrameLevel(), Upper:GetFrameLevel()")
        .unwrap();
    assert!(upper >= lower);
    e.mouse_move(150.0, 150.0);
    e.mouse_down("LeftButton");
    e.mouse_up("LeftButton");
    let (lower, upper): (i64, i64) = e
        .eval("return Lower:GetFrameLevel(), Upper:GetFrameLevel()")
        .unwrap();
    assert!(lower > upper, "{lower} {upper}");
    // The overlap now belongs to Lower.
    e.mouse_move(250.0, 150.0);
    assert_eq!(e.mouse_focus_name().as_deref(), Some("Lower"));
    assert!(errors(&e).is_empty());
}

#[test]
fn a_moved_window_comes_back_after_reload_from_layout_local() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = Engine::new(EngineConfig {
        wtf_dir: wtf.path().to_path_buf(),
        addons_dir: Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/AddOns")),
        blizzard_addons: Vec::new(),
        only_addons: Some(vec!["MovableWindow".into()]),
        ..EngineConfig::default()
    })
    .unwrap();
    e.start();
    // The window is 500x310 in the centre of the 1920x1080 screen.
    // BugSack's OnDragStart is StartMoving itself, so the button name
    // arrives as alwaysStartFromMouse: the window follows the mouse from
    // where the drag started (5 units after the press).
    e.mouse_move(960.0, 540.0);
    e.mouse_down("LeftButton");
    e.mouse_move(955.0, 540.0);
    e.mouse_move(460.0, 240.0);
    e.mouse_up("LeftButton");
    let rect = "local l, b, w, h = MovableWindowFrame:GetRect() return l, b, w, h";
    let moved: (f64, f64, f64, f64) = e.eval(rect).unwrap();
    assert_eq!(moved, (215.0, 85.0, 500.0, 310.0));
    e.reload();
    let file = wtf
        .path()
        .join("Account/SANDBOX/Sandbox/Player/layout-local.txt");
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(
        text,
        "Version: 1\nFrame: MovableWindowFrame\nFrameLevel: 1\nAnchor: BOTTOMLEFT\n\
         X: 215\nY: 85\nW: 500\nH: 310\n"
    );
    let restored: (f64, f64, f64, f64) = e.eval(rect).unwrap();
    assert_eq!(restored, moved);
    assert!(
        e.eval::<bool>("return MovableWindowFrame:IsUserPlaced()")
            .unwrap()
    );
}
