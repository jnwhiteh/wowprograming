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

#[test]
fn edit_boxes_and_sliders_enable_and_disable() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(
        r#"
        Log = {}
        E = CreateFrame("EditBox", nil, UIParent)
        E:SetSize(200, 30)
        E:SetPoint("BOTTOMLEFT", 100, 100)
        S = CreateFrame("Slider", nil, UIParent)
        for _, f in ipairs({E, S}) do
            f:SetScript("OnEnable", function() table.insert(Log, "enable") end)
            f:SetScript("OnDisable", function() table.insert(Log, "disable") end)
        end
        E:Disable()
        E:Disable()
        S:SetEnabled(false)
        S:Enable()
        "#,
    );
    assert_eq!(log(&e), "disable disable enable");
    assert!(!e.eval::<bool>("return E:IsEnabled()").unwrap());
    // A disabled edit box does not take the focus from a click.
    e.mouse_move(150.0, 115.0);
    e.mouse_down("LeftButton");
    e.mouse_up("LeftButton");
    assert!(!e.has_text_focus());
    e.exec("E:Enable()");
    e.mouse_down("LeftButton");
    e.mouse_up("LeftButton");
    assert!(e.has_text_focus());
}

#[test]
fn keyboard_frames_take_keys_before_bindings() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(
        r#"
        Log = {}
        Bound = 0
        -- A stand-in for the client's binding of 1.
        K1 = CreateFrame("Frame", "Low", UIParent)
        K2 = CreateFrame("Frame", "High", UIParent)
        K2:SetFrameLevel(K1:GetFrameLevel() + 5)
        for _, f in ipairs({K1, K2}) do
            f:EnableKeyboard(true)
            f:SetScript("OnKeyDown", function(self, key)
                table.insert(Log, self:GetName() .. ":down:" .. key)
            end)
            f:SetScript("OnKeyUp", function(self, key)
                table.insert(Log, self:GetName() .. ":up:" .. key)
            end)
        end
        -- High passes on everything except ESCAPE.
        K2:SetScript("OnKeyDown", function(self, key)
            table.insert(Log, "High:down:" .. key)
            self:SetPropagateKeyboardInput(key ~= "ESCAPE")
        end)
        "#,
    );
    // P goes through High (propagates) to Low, which keeps it; only Low
    // gets the release.
    assert!(e.key_down("SHIFT-P"));
    e.key_up("SHIFT-P");
    assert_eq!(log(&e), "High:down:P Low:down:P Low:up:P");
    e.key_down("ESCAPE");
    e.key_up("ESCAPE");
    assert_eq!(log(&e), "High:down:ESCAPE High:up:ESCAPE");
    // Without keyboard frames, the key reaches the bindings (no binding
    // action is loaded here, so nothing takes it).
    e.exec("K1:EnableKeyboard(false) K2:Hide()");
    e.key_down("P");
    assert_eq!(log(&e), "");
    assert!(!e.eval::<bool>("return K1:IsKeyboardEnabled()").unwrap());
}

#[test]
fn a_focused_edit_box_gets_on_key_down_first() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(
        r#"
        Log = {}
        E = CreateFrame("EditBox", nil, UIParent)
        E:SetScript("OnKeyDown", function(_, key) table.insert(Log, "down:" .. key) end)
        E:SetScript("OnEscapePressed", function(self) table.insert(Log, "escape") self:ClearFocus() end)
        K = CreateFrame("Frame", nil, UIParent)
        K:EnableKeyboard(true)
        K:SetScript("OnKeyDown", function() table.insert(Log, "frame") end)
        E:SetFocus()
        "#,
    );
    e.key_down("ESCAPE");
    assert_eq!(log(&e), "down:ESCAPE escape");
    e.key_down("ESCAPE");
    assert_eq!(log(&e), "frame");
}

#[test]
fn edit_boxes_edit_at_the_cursor() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(
        r#"
        Log = {}
        E = CreateFrame("EditBox", nil, UIParent)
        E:SetAutoFocus(false)
        E:SetScript("OnCursorChanged", function(_, x, y, w, h) table.insert(Log, "cursor") end)
        E:SetMaxLetters(8)
        E:SetText("hello")
        E:SetFocus()
        "#,
    );
    let text = |e: &Engine| -> (String, i64) {
        e.eval("return E:GetText(), E:GetCursorPosition()").unwrap()
    };
    // SetText puts the cursor at the end.
    assert_eq!(text(&e), ("hello".into(), 5));
    e.key_down("LEFT");
    e.key_down("LEFT");
    e.text_input("XY");
    assert_eq!(text(&e), ("helXYlo".into(), 5));
    e.key_down("BACKSPACE");
    e.key_down("DELETE");
    assert_eq!(text(&e), ("helXo".into(), 4));
    // The letter limit holds for typing.
    e.text_input("123456");
    assert_eq!(text(&e), ("helX123o".into(), 7));
    // Typing replaces highlighted text.
    e.exec("E:HighlightText()");
    e.text_input("new");
    assert_eq!(text(&e), ("new".into(), 3));
    e.exec("E:HighlightText(0, 1)");
    e.key_down("BACKSPACE");
    assert_eq!(text(&e), ("ew".into(), 0));
    // Shift with arrows highlights; HOME and END jump.
    e.key_down("END");
    e.key_down("SHIFT-LEFT");
    e.key_down("SHIFT-LEFT");
    let selected: String = e.eval("return E:GetHighlightText()").unwrap();
    assert_eq!(selected, "ew");
    e.exec("E:SetCursorPosition(1) E:SetNumeric(true)");
    e.text_input("a1b2");
    assert_eq!(text(&e), ("e12w".into(), 3));
    assert!(log(&e).contains("cursor"));
    // SetMaxLetters cuts longer text.
    e.exec("E:SetMaxLetters(2)");
    assert_eq!(text(&e), ("e1".into(), 2));
}

#[test]
fn edit_boxes_take_the_focus_when_shown() {
    let wtf = tempfile::tempdir().unwrap();
    let e = started(wtf.path());
    e.exec(
        r#"
        A = CreateFrame("EditBox", nil, UIParent)
        A:Hide()
        B = CreateFrame("EditBox", nil, UIParent)
        B:SetAutoFocus(false)
        B:Hide()
        "#,
    );
    assert!(!e.has_text_focus());
    e.exec("B:Show()");
    assert!(!e.has_text_focus());
    e.exec("A:Show()");
    assert!(e.eval::<bool>("return A:HasFocus()").unwrap());
}

#[test]
fn dragging_a_slider_thumb_changes_the_value() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(
        r#"
        Log = {}
        SL = CreateFrame("Slider", nil, UIParent)
        SL:SetOrientation("HORIZONTAL")
        SL:SetSize(200, 20)
        SL:SetPoint("BOTTOMLEFT", 100, 100)
        SL:SetThumbTexture("Interface\\Buttons\\WHITE8X8")
        SL:GetThumbTexture():SetSize(20, 20)
        SL:SetMinMaxValues(0, 100)
        SL:SetValueStep(10)
        SL:SetObeyStepOnDrag(true)
        SL:SetScript("OnValueChanged", function(_, v, user)
            table.insert(Log, tostring(v) .. ":" .. tostring(user))
        end)
        "#,
    );
    let thumb_left =
        |e: &Engine| -> f64 { e.eval("return (SL:GetThumbTexture():GetLeft())").unwrap() };
    assert_eq!(thumb_left(&e), 100.0);
    // A press on the track moves the thumb's centre there: 50 at the middle.
    e.mouse_move(200.0, 110.0);
    e.mouse_down("LeftButton");
    assert_eq!(log(&e), "50:true");
    assert_eq!(thumb_left(&e), 190.0);
    // Dragging follows the mouse in steps of 10, and stops at the end.
    e.mouse_move(236.0, 110.0);
    e.mouse_move(400.0, 110.0);
    e.mouse_up("LeftButton");
    assert_eq!(log(&e), "70:true 100:true");
    // After the release the mouse does not move it.
    e.mouse_move(100.0, 110.0);
    assert_eq!(log(&e), "");
    // SetValue does not round, and is not user input.
    e.exec("SL:SetValue(33)");
    assert_eq!(log(&e), "33:false");
    // Disabled sliders ignore the mouse.
    e.exec("SL:Disable()");
    e.mouse_move(110.0, 110.0);
    e.mouse_down("LeftButton");
    e.mouse_up("LeftButton");
    assert_eq!(log(&e), "");
}

#[test]
fn scroll_frames_report_range_changes() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(
        r#"
        Log = {}
        SF = CreateFrame("ScrollFrame", nil, UIParent)
        SF:SetSize(100, 100)
        SF:SetPoint("CENTER")
        local child = CreateFrame("Frame", nil, SF)
        child:SetSize(100, 300)
        SF:SetScrollChild(child)
        SF:SetScript("OnScrollRangeChanged", function(_, x, y)
            table.insert(Log, x .. "," .. y)
        end)
        SFChild = child
        "#,
    );
    e.tick(0.01);
    assert_eq!(log(&e), "0,200");
    e.tick(0.01);
    assert_eq!(log(&e), "");
    e.exec("SFChild:SetHeight(150) SF:UpdateScrollChildRect()");
    assert_eq!(log(&e), "0,50");
}

#[test]
fn set_text_reports_the_change_on_the_next_frame() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(
        r#"
        Log = {}
        T = CreateFrame("EditBox", nil, UIParent)
        T:SetAutoFocus(false)
        T:SetScript("OnTextSet", function() table.insert(Log, "set") end)
        T:SetScript("OnTextChanged", function(self, user)
            table.insert(Log, "changed:" .. self:GetText() .. ":" .. tostring(user))
        end)
        T:SetText("a")
        T:SetText("b")
        "#,
    );
    assert_eq!(log(&e), "set set");
    e.tick(0.01);
    assert_eq!(log(&e), "changed:b:false");
    // The same text again is no change.
    e.exec("T:SetText('b')");
    e.tick(0.01);
    assert_eq!(log(&e), "set");
    // Typing reports at once.
    e.exec("T:SetFocus()");
    e.text_input("c");
    assert_eq!(log(&e), "changed:bc:true");
}

#[test]
fn cooldowns_sweep_and_finish() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(
        r#"
        Log = {}
        CD = CreateFrame("Cooldown", nil, UIParent)
        CD:SetSize(40, 40)
        CD:SetPoint("CENTER")
        CD:Hide()
        CD:SetScript("OnCooldownDone", function() table.insert(Log, "done") end)
        CD:SetCooldown(GetTime(), 2)
        "#,
    );
    let sweep = |e: &Engine| {
        e.scene().items.iter().find_map(|i| match i {
            wowprogramming_engine::scene::Item::Quad(q) => q.sweep,
            _ => None,
        })
    };
    assert!(e.eval::<bool>("return CD:IsShown()").unwrap());
    assert_eq!(sweep(&e), Some((0.0, false)));
    e.tick(1.0);
    let (elapsed, _) = sweep(&e).unwrap();
    assert!((elapsed - 0.5).abs() < 1e-6, "{elapsed}");
    let (start, duration): (f64, f64) = e.eval("return CD:GetCooldownTimes()").unwrap();
    assert_eq!(duration, 2000.0);
    assert!(start > 0.0);
    e.tick(1.1);
    assert_eq!(log(&e), "done");
    assert!(!e.eval::<bool>("return CD:IsShown()").unwrap());
    assert_eq!(sweep(&e), None);
    // Clear ends a cooldown without OnCooldownDone; 0 duration clears.
    e.exec("CD:SetCooldown(GetTime(), 5) CD:Clear()");
    e.tick(6.0);
    e.exec("CD:SetCooldown(0, 0)");
    assert_eq!(log(&e), "");
    assert!(!e.eval::<bool>("return CD:IsShown()").unwrap());
}

#[test]
fn message_frames_show_and_fade_messages() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(
        r#"
        MF = CreateFrame("MessageFrame", nil, UIParent)
        MF:SetSize(300, 100)
        MF:SetPoint("CENTER")
        MF:SetTimeVisible(2)
        MF:SetFadeDuration(1)
        MF:AddMessage("first", 1, 0, 0)
        MF:AddMessage("second", 0, 1, 0, 1, 7)
        "#,
    );
    let texts = |e: &Engine| -> Vec<(String, f32)> {
        e.scene()
            .items
            .iter()
            .filter_map(|i| match i {
                wowprogramming_engine::scene::Item::Text(t) => Some(t),
                _ => None,
            })
            .flat_map(|t| t.lines.iter())
            .map(|l| {
                let s: String = l.runs.iter().map(|r| r.text.as_str()).collect();
                let a = l.runs.first().and_then(|r| r.color).map_or(1.0, |c| c[3]);
                (s, a)
            })
            .collect()
    };
    assert_eq!(
        texts(&e),
        vec![("first".to_string(), 1.0), ("second".to_string(), 1.0)]
    );
    assert!(e.eval::<bool>("return MF:HasMessageByID(7)").unwrap());
    e.tick(2.5);
    let t = texts(&e);
    assert!((t[0].1 - 0.5).abs() < 0.01, "{t:?}");
    e.tick(1.0);
    assert!(texts(&e).is_empty());
    assert!(!e.eval::<bool>("return MF:HasMessageByID(7)").unwrap());
    e.exec("MF:AddMessage('x') MF:Clear()");
    assert!(texts(&e).is_empty());
}

#[test]
fn hyperlinks_fire_enter_leave_and_click() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(
        r#"
        Log = {}
        HF = CreateFrame("Frame", nil, UIParent)
        HF:SetSize(400, 40)
        HF:SetPoint("CENTER")
        HF:EnableMouse(true)
        HF:SetHyperlinksEnabled(true)
        local fs = HF:CreateFontString()
        fs:SetFont("Fonts\\FRIZQT__.TTF", 14, "")
        fs:SetPoint("LEFT")
        fs:SetText("Get |cff0070dd|Hitem:19019|h[Thunderfury]|h|r now")
        HF:SetScript("OnHyperlinkEnter", function(_, link, text) table.insert(Log, "enter:" .. link .. ":" .. text) end)
        HF:SetScript("OnHyperlinkLeave", function(_, link) table.insert(Log, "leave:" .. link) end)
        HF:SetScript("OnHyperlinkClick", function(_, link, text, button) table.insert(Log, "click:" .. link .. ":" .. button) end)
        "#,
    );
    // Find the link's place in the drawn text.
    let (x, y) = e
        .scene()
        .items
        .iter()
        .find_map(|i| match i {
            wowprogramming_engine::scene::Item::Text(t) => {
                let line = &t.lines[0];
                let mut pen = line.x;
                for r in &line.runs {
                    let w: f64 = r.advances.iter().sum();
                    if r.link.is_some() {
                        return Some((pen + w / 2.0, line.y_top - t.size / 2.0));
                    }
                    pen += w;
                }
                None
            }
            _ => None,
        })
        .expect("a link run");
    e.mouse_move(x - 200.0, y);
    log(&e);
    e.mouse_move(x, y);
    assert_eq!(log(&e), "enter:item:19019:[Thunderfury]");
    e.mouse_down("RightButton");
    e.mouse_up("RightButton");
    assert_eq!(log(&e), "click:item:19019:RightButton");
    e.mouse_move(x + 300.0, y);
    assert_eq!(log(&e), "leave:item:19019");
}

#[test]
fn small_region_and_tooltip_rules() {
    let wtf = tempfile::tempdir().unwrap();
    let e = started(wtf.path());
    let ok: bool = e
        .eval(
            r#"
            local f = CreateFrame("Frame", nil, UIParent)
            f:SetCollapsesLayout(true)
            local shownCollapsed = f:IsCollapsed()
            f:Hide()
            local hiddenCollapsed = f:IsCollapsed()
            local w = {}
            f:SetWindow(w)
            local tip = CreateFrame("GameTooltip", "TestTip", UIParent)
            local cleared = 0
            tip:HookScript("OnTooltipCleared", function() cleared = cleared + 1 end)
            tip:SetOwner(UIParent, "ANCHOR_NONE")
            tip:AddLine("x")
            tip:ClearLines()
            return not shownCollapsed and hiddenCollapsed and f:GetWindow() == w and cleared == 2
            "#,
        )
        .unwrap();
    assert!(ok);
}

/// A child with only mouse motion enabled (`SetMouseClickEnabled(false)`,
/// as on the player frame's health bar) gets OnEnter, but a press goes
/// to the click-enabled frame below it.
#[test]
fn clicks_pass_through_motion_only_frames() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(TWO_BUTTONS);
    e.exec(
        r#"
        local bar = CreateFrame("StatusBar", "Bar", A)
        bar:SetAllPoints()
        bar:SetScript("OnEnter", function() table.insert(Log, "Bar:OnEnter") end)
        bar:SetScript("OnMouseDown", function() table.insert(Log, "Bar:OnMouseDown") end)
        bar:SetMouseClickEnabled(false)
        "#,
    );
    e.mouse_move(150.0, 150.0);
    assert_eq!(log(&e), "Bar:OnEnter");
    e.mouse_down("LeftButton");
    e.mouse_up("LeftButton");
    assert_eq!(
        log(&e),
        "A:OnMouseDown(LeftButton) A:OnMouseUp(LeftButton,true) A:PreClick(LeftButton,false) \
         A:OnClick(LeftButton,false) A:PostClick(LeftButton,false)"
    );
    assert!(errors(&e).is_empty());
}

/// Modifier keys pressed alone change IsShiftKeyDown and friends and fire
/// MODIFIER_STATE_CHANGED.
#[test]
fn modifier_keys_are_tracked() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec(
        "Log = {} local f = CreateFrame('Frame') f:RegisterEvent('MODIFIER_STATE_CHANGED')
         f:SetScript('OnEvent', function(_, _, key, down) table.insert(Log, key .. down) end)",
    );
    assert!(e.key_down("RCTRL"));
    let held: (bool, bool, bool) = e
        .eval("return IsControlKeyDown(), IsRightControlKeyDown(), IsShiftKeyDown()")
        .unwrap();
    assert_eq!(held, (true, true, false));
    e.key_up("RCTRL");
    e.set_modifiers(true, false, false);
    assert!(e.eval::<bool>("return IsShiftKeyDown()").unwrap());
    assert_eq!(log(&e), "RCTRL1 RCTRL0 LSHIFT1");
}

/// SaveBindings writes the changed bindings to the sandbox's
/// bindings-cache.wtf in the client's format; they come back after a
/// reload.
#[test]
fn saved_bindings_come_back_after_reload() {
    let wtf = tempfile::tempdir().unwrap();
    let mut e = started(wtf.path());
    e.exec("SetBinding('K', 'TOGGLESPELLBOOK') SetBinding('P', nil) SaveBindings(2)");
    let file = wtf.path().join("Account/SANDBOX/bindings-cache.wtf");
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(text, "bind K TOGGLESPELLBOOK\nbind P NONE\n");
    e.reload();
    let (k, p): (String, String) = e
        .eval("return GetBindingAction('K'), GetBindingAction('P')")
        .unwrap();
    assert_eq!((k.as_str(), p.as_str()), ("TOGGLESPELLBOOK", ""));
}
