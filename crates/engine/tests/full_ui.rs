//! Interactions in the full default UI, driven like a user would. These
//! tests load every Blizzard addon from a local UI source checkout and
//! are skipped without one (as on CI). One engine serves all checks,
//! because startup takes about 20 seconds.

use std::path::{Path, PathBuf};

use wowprogramming_engine::gamedata::GameData;
use wowprogramming_engine::{Engine, EngineConfig, Output};

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

fn full_ui(wtf: &Path, scenario: &str) -> Option<Engine> {
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
            // A third-party window with BugSack's code, named with --addon.
            addons_dir: Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/AddOns")),
            only_addons: Some(vec!["MovableWindow".into()]),
            scenario: Some(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../scenarios")
                    .join(scenario),
            ),
            ..EngineConfig::default()
        },
        Some(std::rc::Rc::new(gamedata)),
    )
    .unwrap();
    engine.start();
    // The default UI loads and runs a few seconds without a Lua error
    // (errors at login and from OnUpdate scripts fill the console).
    let mut errors = Vec::new();
    for _ in 0..50 {
        engine.tick(0.1);
        errors.extend(engine.drain_output().into_iter().filter_map(|o| match o {
            Output::Error(e) => Some(e),
            _ => None,
        }));
    }
    assert!(errors.is_empty(), "Lua errors:\n{}", errors.join("\n"));
    Some(engine)
}

/// The centre of a frame, from a Lua expression.
fn center(e: &Engine, expr: &str) -> (f64, f64) {
    e.eval(&format!("return ({expr}):GetCenter()")).unwrap()
}

fn click(e: &mut Engine, expr: &str) {
    click_with(e, expr, "LeftButton");
}

fn click_with(e: &mut Engine, expr: &str, button: &str) {
    let (x, y) = center(e, expr);
    e.mouse_move(x, y);
    e.mouse_down(button);
    e.mouse_up(button);
}

#[test]
fn full_ui_interactions() {
    let wtf = tempfile::tempdir().unwrap();
    let Some(mut e) = full_ui(wtf.path(), "hud") else {
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
    cooldown_swipe(&mut e);
    let window = movable_window(&mut e);
    edit_mode(&mut e);
    // edit_mode reloaded the UI: the moved window is where it was left.
    let after: (f64, f64) = e
        .eval("return MovableWindowFrame:GetLeft(), MovableWindowFrame:GetBottom()")
        .unwrap();
    assert_eq!(after, window);
}

/// Drag the fixture window (BugSack's window code) and off the screen
/// edge: it follows the mouse and stays on screen. Returns where it ends.
fn movable_window(e: &mut Engine) -> (f64, f64) {
    let pos = |e: &Engine| -> (f64, f64) {
        e.eval("return MovableWindowFrame:GetLeft(), MovableWindowFrame:GetBottom()")
            .unwrap()
    };
    let start = pos(e);
    let from = center(e, "MovableWindowFrame");
    // The press and the first move (past the drag threshold) do not move
    // the window: BugSack's OnDragStart is StartMoving itself, so the
    // button name arrives as alwaysStartFromMouse.
    e.mouse_move(from.0, from.1);
    e.mouse_down("LeftButton");
    e.mouse_move(from.0 - 10.0, from.1);
    e.mouse_move(from.0 - 110.0, from.1 + 50.0);
    e.mouse_up("LeftButton");
    assert_eq!(pos(e), (start.0 - 100.0, start.1 + 50.0));
    // Far off the left edge: clamped to the screen.
    e.drag(
        center(e, "MovableWindowFrame"),
        (-5000.0, 0.0),
        "LeftButton",
        10,
    );
    let clamped = pos(e);
    assert_eq!(clamped.0, 0.0);
    clamped
}

/// An action button shows a cooldown swipe after SetCooldown, and
/// OnCooldownDone fires at the end.
fn cooldown_swipe(e: &mut Engine) {
    e.exec(
        "ActionButton1.cooldown:HookScript('OnCooldownDone', function() CooldownDoneCount = (CooldownDoneCount or 0) + 1 end)
         ActionButton1.cooldown:SetCooldown(GetTime(), 1.5)",
    );
    e.tick(0.5);
    let sweeps: Vec<f32> = e
        .scene()
        .items
        .iter()
        .filter_map(|i| match i {
            wowprogramming_engine::scene::Item::Quad(q) => q.sweep.map(|s| s.0),
            _ => None,
        })
        .collect();
    assert_eq!(sweeps.len(), 1, "{sweeps:?}");
    assert!((sweeps[0] - 1.0 / 3.0).abs() < 0.02, "{sweeps:?}");
    e.tick(1.1);
    let done: i64 = e.eval("return CooldownDoneCount or 0").unwrap();
    assert_eq!(done, 1);
    assert!(
        !e.eval::<bool>("return ActionButton1.cooldown:IsShown()")
            .unwrap()
    );
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
    // Edit box editing in Blizzard's dialog: letters="30" from the XML,
    // typing replaces a highlight, the cursor moves with HOME and END.
    let max: i64 = e
        .eval("return EditModeLayoutDialog.LayoutNameEditBox:GetMaxLetters()")
        .unwrap();
    assert_eq!(max, 30);
    e.text_input("xyz");
    e.exec("EditModeLayoutDialog.LayoutNameEditBox:HighlightText()");
    e.text_input("Sandbox Layoutt");
    e.key_down("BACKSPACE");
    e.key_down("HOME");
    let (text, cursor): (String, i64) = e
        .eval("local b = EditModeLayoutDialog.LayoutNameEditBox return b:GetText(), b:GetCursorPosition()")
        .unwrap();
    assert_eq!((text.as_str(), cursor), ("Sandbox Layout", 0));
    e.key_down("END");
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

/// Lua errors printed since the last call.
fn errors(e: &mut Engine) -> Vec<String> {
    e.drain_output()
        .into_iter()
        .filter_map(|o| match o {
            Output::Error(e) => Some(e),
            _ => None,
        })
        .collect()
}

/// The shown GameTooltip's lines as "left | right" (empty when hidden),
/// after hovering a frame.
fn tooltip(e: &mut Engine, expr: &str) -> Vec<String> {
    let (x, y) = center(e, expr);
    e.mouse_move(x, y);
    e.tick(0.05);
    let text: String = e
        .eval(
            "if not GameTooltip:IsShown() then return '' end
             local out = {}
             for i = 1, GameTooltip:NumLines() do
                 local l, r = _G['GameTooltipTextLeft' .. i], _G['GameTooltipTextRight' .. i]
                 out[i] = (l:GetText() or '') .. ' | ' .. (r:IsShown() and r:GetText() or '')
             end
             return table.concat(out, '\\n')",
        )
        .unwrap();
    text.lines().map(str::to_string).collect()
}

fn press(e: &mut Engine, key: &str) {
    assert!(e.key_down(key), "no binding for {key}");
    e.key_up(key);
    e.tick(0.05);
}

/// The client state goal in scenarios/dummy: an invented character in
/// front of a training dummy, driven by keys and the mouse.
#[test]
fn training_dummy_session() {
    let wtf = tempfile::tempdir().unwrap();
    let Some(mut e) = full_ui(wtf.path(), "dummy") else {
        return;
    };

    // 4. The scenario's curse on the player shows in the debuff frame
    // with the magic border.
    let (shown, border): (bool, String) = e
        .eval("local f = DebuffFrame.auraFrames[1] return f:IsShown(), f.DebuffBorder:GetAtlas()")
        .unwrap();
    assert_eq!(
        (shown, border.as_str()),
        (true, "ui-debuff-border-magic-icon")
    );

    // 1. Drag Blinkstep from the spellbook to action button 5.
    press(&mut e, "P");
    e.tick(0.2);
    let button = "(function() for _, f in PlayerSpellsFrame.SpellBookFrame.PagedSpellsFrame:EnumerateFrames() do \
        if f.spellBookItemInfo and f.spellBookItemInfo.name == 'Blinkstep' then return f.Button end end end)()";
    // 5. Its tooltip comes from the model, in the client's layout.
    assert_eq!(
        tooltip(&mut e, button),
        [
            "Blinkstep | ",
            " | ",
            "Instant | 15 sec cooldown",
            "Steps forward through the weave. Not on the global cooldown. | ",
            // Added by Blizzard's spellbook for spells not on a bar.
            "You haven't added this to your action bars | ",
        ]
    );
    let from = center(&e, button);
    let to = center(&e, "ActionButton5");
    e.drag(from, (to.0 - from.0, to.1 - from.1), "LeftButton", 10);
    e.tick(0.1);
    let (kind, id, held): (String, i64, bool) = e
        .eval("local t, id = GetActionInfo(5) return t, id, GetCursorInfo() ~= nil")
        .unwrap();
    assert_eq!((kind.as_str(), id, held), ("spell", 1900005, false));
    let icon: bool = e
        .eval("return ActionButton5.icon:IsShown() and ActionButton5.icon:GetTexture() == C_Spell.GetSpellTexture(1900005)")
        .unwrap();
    assert!(icon, "action button 5 does not show the Blinkstep icon");
    press(&mut e, "ESCAPE");

    // 2. Tab targets the dummy and the target frame shows it.
    press(&mut e, "TAB");
    let (name, shown): (String, bool) = e
        .eval("return UnitName('target'), TargetFrame:IsShown()")
        .unwrap();
    assert_eq!((name.as_str(), shown), ("Straw Sentinel", true));

    // 3. Key 1 casts Emberlance (2 s): the cast bar fills, the global
    // cooldown shows on the other buttons, mana goes down.
    let mana_before: i64 = e.eval("return UnitPower('player')").unwrap();
    press(&mut e, "1");
    e.tick(1.0);
    let (casting, bar, value): (String, bool, f64) = e
        .eval("return UnitCastingInfo('player'), PlayerCastingBarFrame:IsShown(), PlayerCastingBarFrame:GetValue()")
        .unwrap();
    assert_eq!((casting.as_str(), bar), ("Emberlance", true));
    assert!((0.8..1.3).contains(&value), "cast bar value {value}");
    let (gcd, blink): (f64, f64) = e
        .eval("local _, d = ActionButton3.cooldown:GetCooldownTimes() local _, b = ActionButton5.cooldown:GetCooldownTimes() return d, b")
        .unwrap();
    assert_eq!(
        (gcd, blink),
        (1500.0, 0.0),
        "global cooldown, Blinkstep has none"
    );
    let mana: i64 = e.eval("return UnitPower('player')").unwrap();
    assert_eq!(mana, mana_before - 2000);
    e.tick(1.2);
    assert!(
        e.eval::<Option<String>>("return UnitCastingInfo('player')")
            .unwrap()
            .is_none()
    );

    // Frostbind: a debuff on the dummy, shown on the target frame, and
    // its own 12 s cooldown on the button.
    press(&mut e, "2");
    e.tick(0.1);
    let (debuff, cd): (String, f64) = e
        .eval("local _, d = ActionButton2.cooldown:GetCooldownTimes() return C_UnitAuras.GetAuraDataByIndex('target', 1, 'HARMFUL').name, d")
        .unwrap();
    assert_eq!((debuff.as_str(), cd), ("Frostbind", 12000.0));
    let shown: bool = e
        .eval(
            "local icon = C_Spell.GetSpellTexture(1900002)
             local function find(f)
                 for _, r in ipairs({ f:GetRegions() }) do
                     if r:IsObjectType('Texture') and r:IsVisible() and r:GetTexture() == icon then return true end
                 end
                 for _, c in ipairs({ f:GetChildren() }) do
                     if find(c) then return true end
                 end
                 return false
             end
             return find(TargetFrame)",
        )
        .unwrap();
    assert!(shown, "the target frame shows no debuff");

    // Aegis of Dawn twice: a buff with two stacks in the buff frame.
    e.tick(1.5);
    press(&mut e, "3");
    e.tick(1.6);
    press(&mut e, "3");
    e.tick(0.1);
    let (count, shown): (String, bool) = e
        .eval("local f = BuffFrame.auraFrames[1] return f.Count:GetText(), f:IsShown()")
        .unwrap();
    assert_eq!((count.as_str(), shown), ("2", true));
    // 5. Tooltips for an action, a buff, a debuff and a unit.
    assert_eq!(
        tooltip(&mut e, "ActionButton2"),
        [
            "Frostbind | ",
            "1,500 Mana | 40 yd range",
            "Instant | 12 sec cooldown",
            "Binds the enemy in frost, slowing it by 50% for 8 sec. | ",
        ]
    );
    let buff = tooltip(&mut e, "BuffFrame.auraFrames[1]");
    assert_eq!(
        buff[..2],
        [
            "Aegis of Dawn | ",
            "Absorbs damage. Stacks up to 3 times. | "
        ]
    );
    assert!(
        buff[2].ends_with("|4second:seconds; remaining | "),
        "{buff:?}"
    );
    let debuff = tooltip(&mut e, "DebuffFrame.auraFrames[1]");
    assert_eq!(debuff[0], "Hex of Brittle Bones | Magic");
    // Over the portrait: the health bar only shows its text.
    let unit = tooltip(&mut e, "TargetFrame.TargetFrameContainer.Portrait");
    assert_eq!(unit[..2], ["Straw Sentinel | ", "Level ?? Mechanical | "]);
    // A right click on the buff cancels it.
    click_with(&mut e, "BuffFrame.auraFrames[1]", "RightButton");
    e.tick(0.1);
    let left: i64 = e
        .eval("return #C_UnitAuras.GetUnitAuras('player', 'HELPFUL')")
        .unwrap();
    assert_eq!(left, 0, "the buff is still there after a right click");

    // Starfall Torrent is channeled; the cast bar drains.
    e.tick(1.6);
    press(&mut e, "4");
    e.tick(0.5);
    let (channel, channeling): (String, bool) = e
        .eval("return UnitChannelInfo('player'), PlayerCastingBarFrame.channeling")
        .unwrap();
    assert_eq!((channel.as_str(), channeling), ("Starfall Torrent", true));
    e.tick(3.0);

    // Escape stops a cast first, then clears the target.
    press(&mut e, "1");
    e.tick(0.5);
    press(&mut e, "ESCAPE");
    let (casting, target): (Option<String>, bool) = e
        .eval("return UnitCastingInfo('player'), UnitExists('target')")
        .unwrap();
    assert_eq!((casting, target), (None, true));
    press(&mut e, "ESCAPE");
    let (target, frame): (bool, bool) = e
        .eval("return UnitExists('target'), TargetFrame:IsShown()")
        .unwrap();
    assert_eq!((target, frame), (false, false));

    // 2. A click on the player frame targets the player (through the
    // health bar, which lets clicks through).
    click(&mut e, "PlayerFrame");
    e.tick(0.1);
    let (me, frame): (bool, bool) = e
        .eval("return UnitIsUnit('target', 'player'), TargetFrame:IsShown()")
        .unwrap();
    assert_eq!((me, frame), (true, true));
    press(&mut e, "ESCAPE");

    // The debuff on the dummy ends after 8 s.
    e.tick(5.0);
    let left: i64 = e
        .eval("return #C_UnitAuras.GetUnitAuras('dummy', 'HARMFUL')")
        .unwrap();
    assert_eq!(left, 0);

    // 6. Slash commands go through Blizzard's chat parser.
    e.run_command("/target Straw");
    e.tick(0.1);
    let name: String = e.eval("return UnitName('target')").unwrap();
    assert_eq!(name, "Straw Sentinel");
    e.run_command("/cast Emberlance");
    e.tick(0.5);
    let casting: Option<String> = e.eval("return UnitCastingInfo('player')").unwrap();
    assert_eq!(casting.as_deref(), Some("Emberlance"));
    e.run_command("/stopcasting");
    e.tick(0.1);
    let casting: Option<String> = e.eval("return UnitCastingInfo('player')").unwrap();
    assert_eq!(casting, None);
    e.tick(1.5);

    // A macro on button 6: Frostbind on the hostile target, then
    // Blinkstep. The button shows the macro's name.
    let label: String = e.eval("return ActionButton6.Name:GetText()").unwrap();
    assert_eq!(label, "Bind & Blink");
    press(&mut e, "6");
    e.tick(0.1);
    let (debuff, blink): (String, f64) = e
        .eval("return C_UnitAuras.GetAuraDataByIndex('target', 1, 'HARMFUL').name, C_Spell.GetSpellCooldown(1900005).duration")
        .unwrap();
    assert_eq!((debuff.as_str(), blink), ("Frostbind", 15.0));
    let lines = tooltip(&mut e, "ActionButton6");
    assert_eq!(lines[1], "Frostbind | ", "{lines:?}");

    e.run_command("/logout");
    assert!(e.take_logout(), "/logout did not ask to log out");

    // The arranged bar is still there after /reload.
    e.reload();
    e.tick(0.1);
    let (kind, id): (String, i64) = e.eval("return GetActionInfo(5)").unwrap();
    assert_eq!((kind.as_str(), id), ("spell", 1900005));
    let errs = errors(&mut e);
    assert!(errs.is_empty(), "Lua errors:\n{}", errs.join("\n"));
}
