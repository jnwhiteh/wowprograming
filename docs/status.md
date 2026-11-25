# Project status

This is the handoff document. It records where the project stands now.
Read it before starting work, and update it at the end of every piece of
work (see "Keeping this file current" below).

For goals, design and the full roadmap, see [scope.md](scope.md).

Last updated: 2026-11-25 (real addons: TomTom, Clique and WowLua work; maps, movement, bindings, restricted environment).

## Current state

- **Real addons goal: in progress** (set by the user on 2026-11-25),
  on branch `client-state`: TomTom, Clique and WowLua must run without
  errors and work, and the player must be able to move so TomTom's
  navigation can be tried. Done criteria in scope.md ("Real addons").
  Progress: all six done criteria hold (`tests/real_addons.rs`): the
  three addons load and run without Lua errors, maps come from the
  UiMap tables, the player walks with W, A, S, D, Q, E, TomTom's arrow
  leads to a `/way` waypoint, Clique casts from a click, WowLua runs
  code. See "Next steps" for what to try next.
- **Client state goal: criteria 1 to 6 done** on branch `client-state`
  (2026-11-25): unit-frame clicks, buff removal, a player debuff,
  tooltips from the model, slash commands and macros. Criterion 7
  (Clique) is now part of the real addons goal. The branch is not
  merged and not pushed. The model (`lua/clientstate.lua`) works with
  an invented character in `scenarios/dummy`; where real data is
  needed is listed in [client-runtime.md](client-runtime.md), "Client
  state: invented data and where real data is needed".
- **Console errors fixed (2026-11-24, branch `client-state`):** the user
  reported constant errors in the console. The default UI now loads and
  runs without Lua errors in all scenarios (`monk`, `evoker`, `paladin`,
  `hud`, `world`), after `/reload`, and from character select into the
  world for all 13 characters (about 57 distinct errors before). The
  full-UI test fails on any Lua error in the first 5 seconds.
  `run --glue --seconds N --exec "/run SelectCharacter(i) EnterWorld()"`
  now follows Enter World headless, as the window does. Main causes: a
  captured load guard (`g_middleClassLoaded`) stopped `middleclass` from
  loading; `[Bootstrap]` files of load-on-demand addons did not run;
  stubs returned one nil instead of nothing; `string.format` lacked
  positional arguments; CVar defaults were missing without a capture;
  map widget methods and named button textures were missing; class
  resources reported the main power's maximum. New inferred rules are in
  [client-runtime.md](client-runtime.md) ("Loading XML and client
  answers").
- **Desktop feedback (2026-11-24):** fixes from the user's first desktop
  run (character select list art and text shadows, loading screen
  progress, PTR issue reporter, cast bars at login, buff and game menu
  tooltips, help tips, leaving Edit Mode, magnified textures). Merged
  into `main` by the user's request (fast-forward, not pushed); the
  branch `desktop-feedback` is deleted.
- **Frame runtime goal: done on 2026-11-24**, merged into `main` by the
  user's request (fast-forward, not pushed). All done
  criteria in [scope.md](scope.md) ("Frame runtime") hold:
  `wowprogramming coverage` shows no gap (it started with 45), and every
  interaction has a test. The full-UI test
  (`crates/engine/tests/full_ui.rs`) drags the fixture window with
  BugSack's code (clamped, back after `/reload`), raises a toplevel
  window by a click, closes windows with Escape, blocks a key binding
  with a keyboard frame, drags a Settings slider and the Settings list's
  scroll bar, edits text in Blizzard's Edit Mode dialog, checks an action
  button's cooldown swipe and OnCooldownDone, and moves the player frame
  in Edit Mode and finds it after `/reload`. Rules, inferred behavior and
  the open questions for a probe session are in
  [client-runtime.md](client-runtime.md).
- **Direction (proposed 2026-11-24):** client state (a
  training dummy session: drag spells to the bar, instant, cast and
  channeled spells, cooldowns, buffs and debuffs, target, tooltips, macros, Clique), then the security model
  (taint, protected functions, secret values), then tools for addon
  authors (headless interaction tests, reload on change, browser view).
  Details in scope.md under "Direction after the frame runtime".
- **Paused goal: character select and world** (started 2026-11-23). All
  four parts work end to end (tested under Xvfb at 3440x1440):
  `gui --glue --scenario scenarios/charselect` shows character select
  with 13 characters from the account's addon data and the selected
  character's race model in a clearing with trees; Enter World shows the
  map's loading screen, then the full game UI over the same scene. Not
  yet tried on the user's desktop. The user decided the C++ foundations
  of the UI matter more than 3D rendering; the remaining 3D work is in
  the backlog in scope.md.
- **Branches:** one branch per goal or major milestone, merged into
  `main` when the goal is complete (the user's rule, 2026-11-24). The
  frame runtime goal is on `frame-runtime`, from `character-select`
  (so merging it also brings the character select work into `main`).
  `character-select` (paused, all parts working) is not merged into
  `main` yet. Commit each tested step on the goal's branch without
  asking; still ask before pushing or merging.
- **Earlier goals on hold:** spellbook (functionally complete) and HUD
  (player, target, party frames, action bars). See "Next steps".
- **Data:** addon captures (version 2, build 12.1.5.70961) for the Monk
  (Ohnoregard-Draenor), Evoker (Ohnodwagon-Draenor) and Paladin
  (Ohnohealme-Draenor). The Evoker capture has a party of 5. No capture
  has a target, raid units or trait data; those captures are deferred.
- **Reference data for the character select goal** (in the game folders, never commit
  it): character select and in-game screenshots at 3440x1440 in
  `_retail_/Screenshots/WoWScrnShot_112326_*.png` (225345 and 225508 are
  character select; Print Screen works on that screen); a 33 s video
  (character select, loading screen, Arcantina, portal, loading screen,
  Silvermoon) in
  `/mnt/c/Users/jnwhi/Videos/Captures/World of Warcraft 2026-11-23 22-55-52.mp4`;
  login traces in `SavedVariables/!WowProgrammingTrace.lua` (Evoker: 1
  login, 3 reloads, zone changes; Monk: 1 login).
- **Tests:** `cargo test`, `cargo fmt --check` and
  `cargo clippy --all-targets -- -D warnings` pass.

## What works

- Client state model (`crates/engine/src/lua/clientstate.lua`, `S.State`
  in scenarios): spells, spellbook skill lines, action slots (saved per
  character in the sandbox's `WowProgramming_ClientState` file), the
  cursor (`PickupSpellBookItem`, `PickupAction`, `PlaceAction`,
  `GetCursorInfo`, show and hide grid), the target (`TargetUnit`,
  `TargetNearestEnemy` on Tab, `ClearTarget`), casting (`UseAction`,
  `CastSpellByID`, `CastSpellByName`, `SpellStopCasting`,
  `UnitCastingInfo`, `UnitChannelInfo`), the global cooldown and spell
  cooldowns, mana cost and return, auras (`C_UnitAuras` by index, slot,
  instance ID and spell ID; UNIT_AURA with update info; expiry; stacks;
  `CancelUnitBuff`). Rules and gaps in client-runtime.md ("Client
  state").
- `scenarios/dummy`: Ilyndra, an invented night elf mage, with two
  spellbook sections of invented spells and the Straw Sentinel training
  dummy. Run it with `gui --scenario scenarios/dummy`.
- Presses go to the top frame with clicks enabled; frames with only
  mouse motion let clicks through (the player frame's health bar).
  `C_ClickBindings` has the client's default profile (left click
  targets, right click opens the menu), so secure unit frames target on
  a click.
- Real addons: TomTom, Clique and WowLua (from `addons_dir`, with
  `--addon`) load and work in the dummy scenario.
- World map (M): map art tiles from the UiMapArt tables, TomTom's
  pins, the player arrow (`lua/unitposition.lua`). Addons' own BLP and
  TGA textures load from the addons folder (`formats/tga.rs`).
- Maps and movement (`lua/maps.lua`, `maps.rs`): `C_Map` from the
  UiMap tables (info, children, parents, groups, world size, world and
  map positions, map rectangles, best map, player position), zone
  texts, `UnitPosition`, `GetPlayerFacing`, `IsPlayerMoving`,
  `GetUnitSpeed`; the movement keys move and turn the player over time
  with movement and zone events. `WowProgrammingWorld.SetPlayerMapPosition`
  places the player from a scenario. `ctl keydown` and `ctl keyup` hold
  a key.
- `xpcall(f, handler, ...)` passes the extra arguments, as in WoW.
- Key bindings (`lua/bindings.lua`): default keys including movement,
  `SetBinding*`, override bindings (`SetOverrideBinding*`,
  `ClearOverrideBindings`, priority first), `CLICK`, `SPELL`, `MACRO`
  commands, and `runOnUp` actions on key release.
- Blizzard's restricted environment runs secure handler snippets:
  `IsProtected` reports `protected="true"` from templates,
  `C_Widget.IsFrameWidget`, `loadstring_untainted`. `issecurevariable`
  reports a table key as tainted by the calling third-party addon
  (an interim rule for TaintLess).
- Slash commands and macros (`lua/macros.lua`): the console sends slash
  commands to Blizzard's chat parser through the client's macro line
  callback, so `/cast`, `/target`, `/stopcasting` and `/logout` work in
  the console and in macros. `SecureCmdOptionParse` evaluates macro
  options. `State.AddMacro` adds macros; a macro on a bar shows its
  name, its spell's icon and tooltip, and runs on a key press
  (`scenarios/dummy` button 6, "Bind & Blink").
- Tooltips from the model (`State.tooltipFunctions` in
  `clientstate.lua`) for spellbook items, actions, buffs, debuffs and
  units; Blizzard's `TooltipDataProcessor` draws them. A hidden tooltip
  has no owner. `|4singular:plural;` text escapes are resolved.
- Right-click on a buff cancels it; `State.ApplyAura` puts a debuff from
  another source on the player (the dummy scenario's magic curse).
- Escape runs Blizzard's `ToggleGameMenu`: stop a cast, close windows,
  clear the target, then the game menu.
- `C_CreatureInfo.GetClassInfo` and `GetRaceInfo` (English names).

- Everything from M1 (Lua 5.1 runtime, watchdog, addon loading, frames,
  timers, SavedVariables, console).
- Full default UI: every Blizzard addon the client loads at startup, in the
  client's order: secure-environment addons (`UseSecureEnvironment`) first
  (run in the normal environment; `GetGlobalEnvironment` and friends in
  `lua/client.lua`), then `LoadFirst` addons, then the rest by name.
  `--full-ui` forces this for any scenario. Startup takes about 20 s
  (mostly file reads from `/mnt/d`; 2 s of CPU).
- Login sequence in the client's order, taken from the traces (logins and
  reloads agree): UPDATE_BINDINGS, DISPLAY_SIZE_CHANGED, UI_SCALE_CHANGED,
  VARIABLES_LOADED, EDIT_MODE_LAYOUTS_UPDATED (from
  `C_EditMode.GetLayouts()`), PLAYER_LOGIN, PLAYER_ENTERING_WORLD,
  LOADING_SCREEN_DISABLED, SPELLS_CHANGED. UIParent stays shown while the
  UI loads (Blizzard's cast bar hides itself at login only when visible).
- Startup runs in steps (`Engine::begin_start`, `start_step`,
  `start_progress`): setup, the addon list, one Blizzard addon per step
  (dependencies first), addons, login. The window runs steps for 30 ms
  per frame behind the loading screen and shows the step and percentage
  in the bar.
- Atlases with slice data (UiTextureAtlasElementSliceData.db2) and
  `SetTextureSliceMargins` draw as nine parts. Named XML colors
  (`color="BLACK_FONT_COLOR"`) and `C_UIColor.GetColors` come from
  GlobalColor.db2. Global strings are unescaped (`\n`, `\32`). Textures
  are sampled bilinearly.
- CVar bitfields (`GetCVarBitfield`, `SetCVarBitfield`) work; closed
  tutorials (`closedInfoFrames`, `closedInfoFramesAccountWide`) are read
  from the account's and the character's `config-cache.wtf`, read only.
- `OnlyBetaAndPTR` addons (the issue reporter) do not load.
- Glue environment (`--glue`): loads the Blizzard addons whose TOC says
  `AllowLoad: Glue` or `Both`, applies `[AllowLoad glue]`/`[AllowLoad
  game]` tags on files and dependencies, includes glue-only GlobalStrings,
  sends FRAMES_LOADED, LOGIN_STATE_CHANGED and CHARACTER_LIST_UPDATE.
  `lua/glue.lua` holds the glue client functions: always logged in and
  connected (`C_Login.GetState`), the character list API
  (`GetNumCharacters`, `GetCharacterGUID`, `GetBasicCharacterInfo`,
  `GetCharacterInfo` with the 45 values from the Wow.exe usage string,
  `SelectCharacter`, `EnterWorld`), answers delivered on the next frame
  as in the client. Widget types `MapScene` and `ModelFFX` exist.
- `scenarios/charselect` reads the account's data at run time, read only:
  Syndicator's SavedVariables (names, race, class, sex, faction), the
  trace profiles (level, zone, GUID, map, equipment) and
  `character-list-order.txt`. Characters without a trace show level 90
  and no zone. Constants come from the Evoker capture.
- Enter World: the window shows the map's loading screen
  (`loading.rs`: LoadingScreens.db2 and LoadingScreenSkin.db2, layout
  measured from the video; old maps use one wide image), then builds a
  new engine with the character's scenario: the one whose
  `scenario.toml` has `character = "Name-Realm"`, else `scenarios/world`
  (described player from `S.character`). The build blocks for about 20 s
  while the loading screen shows. `Logout()` in the game returns to
  character select (a new glue session; tested: enter, log out, pick
  another character, enter again).
- Window: borderless and covers the monitor (`--windowed` for a normal
  window). The engine keeps the UI height in UI units and takes the
  monitor's aspect ratio and pixel size (`Engine::set_display_size`, fires
  DISPLAY_SIZE_CHANGED). F12 shows or hides the console. Only the
  default UI loads; third-party addons load from `addons_dir` only when
  named with `--addon NAME`.
- 3D view (`world.rs`, `formats/m2.rs`): M2 models and skins (vertices,
  textures, materials, sections, batches), creature skins from
  CreatureDisplayInfo, race models through ChrRaceXChrModel and ChrModel,
  the first frame of Stand (`m2::stand_pose`; character bones are in the
  skeleton file named by the SKID chunk: SKB1 bones, SKS1 sequences),
  a multi-threaded software rasterizer (near-plane clipping, depth buffer,
  alpha test and blending, sun light, fog, sky gradient). The demo scene:
  a grass ground (Elwynn tileset), Elwynn trees, bushes and rocks, a deer
  and a rabbit, the player's race model at the origin. About 50 ms per
  frame at 1720x720. Drawn at half resolution behind the UI
  (`render::render_over`); on character select the camera faces the
  character, in the game it is behind the player.
- `wowprogramming coverage`: script handlers in `UI.xsd` and documented
  widget methods with their state in the engine and their uses in
  Blizzard code; lists the frame runtime's gaps (`--all` for every row).
- Tools: `wowprogramming db2 <FileDataID> [ids]` prints table rows,
  `image`, `extract`, `model`, `world` and `loading-screen` help inspect
  game files. Table FileDataIDs come from the community listfile (in the
  scratchpad, not in the repo).
- XML: templates (with their own `parentKey`, `parentArray` and
  `parent`), intrinsics, `<Mixins>`, `<MaskedTextures>`, `<TextInsets>`,
  `<ViewInsets>`, all animation attributes, attributes without
  OnAttributeChanged during construction, `inherit="prepend"` (inherited
  handler first) and `"append"`. Script and Include paths fall back to the
  addon folder.
- Widgets: method groups merge base first, derived types override.
  Buttons take mouse input by default; mouse scripts enable mouse input.
  `SetToDefaults`, three-part `GetAttribute`, edit box text input
  (OnChar, Backspace, Enter, Escape, Tab), text insets,
  `SetStatusBarTexture` with atlas names, ModelScene view insets.
- GameTooltip (`lua/tooltip.lua`): lines as TextLeftN/TextRightN (reusing
  the template's first two), sizing on Show, owner anchors, hidden when
  empty. Spell tooltip data falls back to name and subtext without a
  capture.
- Layout: only opposite edges set a size; textures without size take
  their atlas or image size; XML textures with neither anchors nor size
  fill their parent; button text centres, status bar textures fill;
  OnSizeChanged fires after layout, once per real change; scroll scripts
  fire only on change. Text: whole-pixel font sizes, em-based glyphs,
  words never break without non-space wrap, fixed-height text is cut with
  an ellipsis.
- Text width follows the client's rules, found from the captured font
  measurements (`text.rs`, `Measurer`): glyphs are drawn at the font
  height rounded to the *nearest* whole pixel; each advance is rounded up
  to a whole pixel; kerning is truncated toward zero; a string ends at
  the last glyph's outline edge (rounded up) plus one pixel; a shadow to
  the right adds its offset in whole pixels. About 95% of single
  characters and most captured strings match exactly; the rest are
  within 1 to 2 pixels. The renderer places glyphs at the measured
  advances. A hidden font string reports its text size from `GetWidth`.
- Drawing: regions sort by strata, frame level, layer, sublevel across
  frames (so useParentLevel borders stay behind text), then reverse
  creation order within a frame. Masks, rotation, flipbooks, animations,
  atlas-relative texture coordinates. The window uses the same software
  renderer as screenshots (not tried in a window yet).
- API: documented stubs; about 4000 undocumented client functions found
  in `Wow.exe` (globals and `C_X.Func`), `GetNum*` return 0; documented
  object types (SecondsFormatter) return stub objects. Hand-written
  client functions in `lua/client.lua` (quality colors, secret-value
  checks, mapvalues, securecopy, function containers, GetBindingText,
  UnitPowerType and other defaults). CVars with defaults; key bindings
  with default keys for the main bar.
- Scenarios (`lua/scenario.lua`): `S.LoadStandardCapture`,
  `S.ApplyCapture`, `S.Replay` (restores documented mixins),
  `S.SetUnit` (described units for all common Unit* functions),
  `S.SetAction` (action bar slots), `S.FireEvent`, `S.AddAtlases`,
  `S.ApplyCVars`, `S.SpellbookStandIns` (for reduced UIs).
- Capture addon (`tools/capture/WowProgrammingCapture`) and
  `wowprogramming import-capture`; see [capture.md](capture.md).
- Message frames (`messageframe.rs`): lines with colours and IDs that
  fade out. Hyperlinks (`hyperlink.rs`): OnHyperlinkEnter, Leave and
  Click for links in the text of frames with hyperlinks enabled.
- Cooldown frames (`cooldown.rs`): the swipe is drawn and shrinks over
  time; the widget hides and OnCooldownDone fires at the end.
- Sliders (`slider.rs`): the thumb follows the value; pressing and
  dragging changes the value. Scroll frames report range changes.
- Edit boxes (`editbox.rs`): cursor, highlight, letter and byte limits,
  numeric input, autoFocus, editing keys, OnCursorChanged, OnTextSet.
- Edit Mode: enter, drag a HUD frame, save a new layout, leave; layouts
  are kept by `C_EditMode` in `lua/client.lua` and saved in the
  sandbox's `WowProgramming_EditMode.lua`.
- Keyboard: the focused edit box, then keyboard-enabled frames (top
  first, OnKeyDown, propagation, OnKeyUp to the frame that took the
  press), then key bindings.
- Mouse input (`input.rs`, `movement.rs`, tests in
  `crates/engine/tests/input.rs`): the pressed frame captures the release
  (OnMouseUp with `upInside`), click registrations (Up and Down), double
  clicks, OnEnter and OnLeave with `motion` false when frames change
  under a still mouse, `RegisterForDrag`, OnDragStart, OnDragStop,
  OnReceiveDrag, `StartMoving`, `StartSizing` with resize bounds,
  `StopMovingOrSizing` (nearest UIParent point), clamping with insets,
  `Raise`, `Lower`, toplevel frames raised on click and show, frame
  `LockHighlight`. Moved frames are saved in the sandbox's own
  `layout-local.txt` and restored after `/reload` (`layoutcache.rs`).
  Several details are inferred (see client-runtime.md).
- Control bridge: `exec`, `eval`, `dofile`, `key`, `keydown`, `keyup`, `type`, `click`,
  `hover`, `drag`, `down`, `up`, `mouse`, `under`, `tree`, `tick`,
  `screenshot <path> [scale] [frame]`, `stubs`, `quit`. `hover` and
  `click` take one word: store frames in a global first.
- Image tools: ImageMagick (`magick`) is installed with Homebrew, for
  cropping and comparing screenshots.

## What does not work yet

- Outlined text (`OUTLINE`, action button key labels) is 5 to 7 pixels
  narrower than in game; the outline's extra width is not modelled.
- After `Font:SetFontObject`, the sandbox drops the font object's own
  shadow; the game seems to keep it (ObjectiveTrackerHeaderFont strings
  are one pixel wider in the captures).
- Frame runtime leftovers (backlog in scope.md): cooldown countdown
  numbers, edge and bling; the cursor does not move to a click in an
  edit box; no OnKeyDown for modifier
  keys alone; indented word wrap is stored but not drawn.
- Portraits (a placeholder portrait is shown), minimap,
  3D models inside UI frames (PlayerModel, ModelScene).
- 3D view: models hold the first frame of Stand (no animation playback;
  Stand stored in `.anim` files is not read, which leaves those models in
  the bind pose); character skin is the race's first skin texture
  (`world::default_skin`, a table of FileDataIDs for the classic races and
  Dracthyr; other races get a flat color), hair is a flat color, other
  replaceable textures (Dracthyr wings) are grey; no equipment; the ground is a flat plane;
  textures have no mipmaps (distant grass shimmers); the scene is the same
  for every map. Only the selected character stands on character select
  (the game shows the warband group in a camp scene).
- Character select: the list shows a frame above the first card that the
  game does not show; warband groups are not shown. (The screen is at most
  2:1 wide, 2880x1440 on the user's monitor, like the game; class names
  are class colored.) Back, Shop, Menu and the other
  top buttons are not tried.
- Entering the world takes about 20 s; the bar moves by addon count, so
  it is slow at first. `/logout` is unknown (the chat frame's slash commands do
  not load); the game menu's Log Out button and `/run Logout()` work.
- The Talents tab (needs trait data); spec sample abilities are captured
  but not checked yet.
- About 55 full-UI startup errors in systems outside the goals (chat
  frames, communities, tutorials, guild UI, pet battles, LFG). The Evoker
  scenario has 59: being in a party creates more model scenes, so the
  model camera error (`OrbitCameraMixin.lua:41`) repeats more often. Some
  repeat every tick (PersonalResourceDisplay).
- The first action button shows no key label; the chat frame background
  is white.
- Compact raid frames are not tried yet (`scenarios/hud` is a party with
  classic party frames).
- Taint and secret values.
- Client state: no Clique, no macro frame tried (`/macro`, dragging a
  macro from the macro window), macros and bindings not read from the
  character's WTF files, no icon at the mouse pointer while a spell is
  held, no spell data from DB2 (only invented spells). Modifier keys
  always read as up (`IsShiftKeyDown` is a stub), so modified clicks and
  click bindings with modifiers cannot be tried.

## Next steps

Console errors: ask the user to run the GUI again and report any error
that is left (interactions such as opening panels can still raise some;
the headless runs only cover loading and idle play).

Real addons goal (scope.md): the done criteria hold. To make the
simulator more complete, in order:

1. The world map works (M): map art, TomTom's waypoint pin, the player
   arrow. Still missing: exploration overlays
   (`C_MapExplorationInfo.GetExploredMapTextures`, from the
   WorldMapOverlay tables), the minimap (TomTom's minimap pins are placed
   but the Minimap widget draws no terrain), zooming the map with the
   mouse wheel (try it).
2. Clique's options window (`/clique`, its spellbook tab and binding
   editor) and Clique on party frames: a party scenario (`scenarios/hud`
   has a party) with a spell bound to a click on a party frame.
3. TomTom's own in-game test addon `TomTomTest` (load it with
   `--addon TomTomTest`): run its suites and fix what fails.
4. WowLua: the other toolbar buttons (pages, undo, lock), its saved
   pages after `/reload`.
5. Save key bindings (`SaveBindings`) and read the character's
   `bindings-cache.wtf`, `macros-cache.txt` and
   `click-bindings-cache.txt`, read only.
6. Then the security model goal: taint (`issecurevariable` and
   `loadstring_untainted` are stand-ins today), protected functions,
   combat lockdown, secret values.

Client state leftovers: the macro window (`CreateMacro`, `EditMacro`),
the held spell's icon at the mouse pointer, real spell data from DB2.
Ask the user to try `gui --scenario scenarios/dummy --addon TomTom
--addon Clique --addon WowLua` on the desktop (W, A, S, D to walk,
`/way 42 60`, `/wowlua`).

Desktop feedback (merged). Not reproduced: buff tooltips "adding lines until full"
(with the fixes, buff tooltips show the aura name, description and time
left, and the game menu tooltip keeps its size); ask for the exact frame
if it happens again. The headless `run` has no character name, so it does
not read closed tutorials; the GUI's Enter World does.

Frame runtime goal: done and merged. Leftovers of the frame
runtime are in the backlog in scope.md. When the open questions in
client-runtime.md are worth a session (there are 21), write the probe
addon described there and ask the user before running it.

Paused character select and world goal: ask the user to try
`gui --glue --scenario scenarios/charselect` on the desktop
(see [running-the-gui.md](running-the-gui.md)) when convenient. The
remaining 3D steps (animation, mipmaps, customization textures, terrain)
are in the backlog in scope.md.

On hold (spellbook and HUD goals): check the captured tooltips and sample
abilities; HUD problems (first action button key label, white chat
background, outlined text width); compare
party frames with the capture; a raid scenario; capture addon support for
raid units and trait data.

How to run: see [running-the-gui.md](running-the-gui.md).

## Decisions

Decisions already made with the user. Do not reopen them without a reason.

| Date | Decision | Why |
|---|---|---|
| 2026-11-22 | Rust, with `mlua` and vendored Lua 5.1 | Exact Lua behaviour; VM can be patched for taint later; `wgpu` and WebAssembly for rendering and the browser |
| 2026-11-22 | Reimplement only what the C++ client provides; load Blizzard's Lua and XML from the UI source | High fidelity for little code; every Blizzard file is a test |
| 2026-11-22 | Headless core; the browser is the first viewer | Quickest path to clickable prototypes |
| 2026-11-22 | Tool name `WowProgramming`; crates `wowprogramming-<part>` | Chosen by the user |
| 2026-11-22 | Scenario files are written in Lua | Familiar to addon authors; can express loops and conditions. The test world grows from more and more scenario files |
| 2026-11-22 | One `<Name>.toc` per addon; no TOC conditionals or placeholders | Build only what a concrete goal needs; the rest is in the backlog |
| 2026-11-22 | Script watchdog and memory limit from the start | A runaway script must never hang a session |
| 2026-11-22 | Visual goal after M5: Blizzard's Character Frame and spellbook | Chosen by the user |
| 2026-11-23 | The sandbox opens full screen at native resolution, starts at character select and enters the world | Compare with game screenshots; feel like the game (user's request) |
| 2026-11-23 | 3D world from real game models (option A); own art or simple shapes where that is hard, terrain first | Chosen by the user |
| 2026-11-23 | Character list read at run time from the account's addon data (Syndicator, trace addon), not copied into the repo | Personal data stays in the game folder |
| 2026-11-23 | Early events recorded by a separate `!WowProgrammingTrace` addon | The `!` makes it load before other third-party addons (user's advice) |
| 2026-11-24 | Only the default UI loads. Third-party addons load only when named (`--addon`), for testing; never from the character's enabled list in the game | The default UI is the focus now; addons are for testing and later work |
| 2026-11-24 | One branch per goal or major milestone, merged into `main` when the goal is complete | User's rule |
| 2026-11-24 | Next goal: the frame runtime (the C++ foundations of the UI), not 3D rendering | The user cares about the interface and its interactions, not the game world |
| 2026-11-25 | Next goal: a full simulator for addon development; TomTom, Clique and WowLua must run without errors, with a movable player for TomTom | User's request ("Real addons" in scope.md) |
| 2026-11-24 | Facts come from static sources first: `UI.xsd`, the widget API documentation, `Wow.exe`, Blizzard code, files the client writes, warcraft.wiki.gg. In-game recording is the last resort: questions are collected and answered in one small probe session | Recording is slow for the user, gives single traces and can produce huge files |

## Environment

- Work happens in WSL on a Windows machine. The repo is on the Windows
  drive at `/mnt/d/src/WowProgramming`.
- Rust 1.100 comes from Homebrew in WSL (no `rustup`). Non-interactive
  shells need `export PATH=/home/linuxbrew/.linuxbrew/bin:$PATH`.
- Retail client: `/mnt/d/World of Warcraft/_retail_`, build 12.1.5.70961.
- Blizzard UI source for Retail: `../wow-ui-source-retail` (branch `live`,
  same build). `../wow-ui-source` has the other flavors on other branches.
- Pushing from WSL does not work: the SSH key needs a passphrase and `gh`
  is not logged in. The user pushes.
- The capture addon reaches the game through a junction from
  `_retail_/Interface/AddOns/WowProgrammingCapture` to
  `tools/capture/WowProgrammingCapture` (fixed by the user on
  2026-11-23).

## Code map

| Path | Contents |
|---|---|
| `crates/lua` | Lua runtime: restricted stdlib, WoW stdlib additions (`stdlib.rs`), watchdog, error messages |
| `crates/engine/src/lib.rs` | `Engine`: startup, ticks, events, console commands, reload, shutdown |
| `crates/engine/src/state.rs` | Engine state stored as Lua app data; output lines |
| `crates/engine/src/toc.rs` | TOC parser |
| `crates/engine/src/addons.rs` | Addon discovery, load order, `C_AddOns` |
| `crates/engine/src/frames.rs` | `CreateFrame`, frame methods, events, scripts, visibility |
| `crates/engine/src/timers.rs` | `C_Timer` |
| `crates/engine/src/natives.rs` | Native globals; `safe_call` for calling Lua with error handling |
| `crates/engine/src/animfx.rs` | Visual effects of playing animations (alpha, offset, scale, rotation, flipbook) |
| `crates/engine/src/clientnames.rs` | Undocumented client functions from `Wow.exe` and the UI source |
| `crates/engine/src/scene.rs`, `render.rs` | Scene (quads, text, masks) and the multi-threaded software renderer |
| `crates/engine/src/loading.rs` | Loading screen from LoadingScreens.db2; small RGBA image helper |
| `crates/engine/src/world.rs` | 3D view: model loading, demo scene, software rasterizer |
| `crates/formats/src/m2.rs` | M2 model and skin file reader |
| `crates/engine/src/lua/glue.lua` | Glue client functions (login state, character list, EnterWorld) |
| `crates/engine/src/lua/clientstate.lua` | Client state model: spells, spellbook, action slots, cursor, target, casting, cooldowns, power, auras (`S.State`) |
| `scenarios/dummy` | Training dummy session with an invented character and spells |
| `scenarios/charselect`, `scenarios/world` | Character select from account data; any character without its own scenario |
| `tools/capture/!WowProgrammingTrace` | In-game addon that records login and zone change events |
| `crates/engine/src/lua/client.lua` | Client functions written in Lua (quality colors, placeholder enums, click bindings) |
| `crates/engine/src/lua/maps.lua`, `maps.rs` | `C_Map` from the UiMap tables; player position, facing and movement |
| `crates/engine/src/lua/bindings.lua`, `bindings.rs` | Key map, override bindings, the binding API |
| `crates/engine/tests/real_addons.rs` | TomTom, Clique and WowLua in the full UI (skipped without them) |
| `crates/engine/src/lua/macros.lua` | Macro line execution through Blizzard's chat parser, `SecureCmdOptionParse`, `RunMacroText` |
| `crates/engine/src/lua/scenario.lua` | Scenario helpers (`S.LoadStandardCapture`, `S.Replay`, ...) |
| `crates/engine/src/data/cvar_defaults.txt` | CVar defaults for runs without a CVar capture |
| `tools/capture/WowProgrammingCapture` | In-game capture addon |
| `tools/compare/compare_frames.lua` | Compares sandbox rectangles with a capture |
| `crates/engine/src/input.rs`, `movement.rs` | Mouse input and click rules; moving, sizing, clamping, raising |
| `crates/engine/src/layoutcache.rs` | User-placed positions in the sandbox's `layout-local.txt` |
| `crates/engine/src/editbox.rs` | Edit box editing: cursor, highlight, limits, keys, autoFocus |
| `crates/engine/src/slider.rs` | Slider thumb placement and dragging |
| `crates/engine/src/cooldown.rs` | Cooldown frames: swipe, OnCooldownDone |
| `crates/engine/src/messageframe.rs`, `hyperlink.rs` | Message frames; hyperlink scripts |
| `crates/engine/src/coverage.rs` | The coverage report (`wowprogramming coverage`) |
| `crates/engine/src/apidoc.rs` | `Enum` and `Constants` from the API documentation |
| `crates/engine/src/savedvars.rs` | SavedVariables reading and writing |
| `crates/engine/src/lua/*.lua` | Engine helpers written in Lua (prelude, curves, API doc builder) |
| `crates/engine/tests/` | Integration tests and fixture addons |
| `crates/cli` | The `wowprogramming` command |

## Lessons learned

Things that cost time once and should not cost it again.

- The client writes the positions of moved frames to
  `WTF/Account/<account>/<realm>/<character>/layout-local.txt`. Files the
  client writes are ground truth that costs no recording session (they
  showed that a moved frame is anchored at the nearest of the nine
  UIParent points, not only corners). Look for such files before asking
  for a probe.
- Matching method names in the engine source by text misses methods
  registered under generated names (the button texture setters). Judge
  coverage at run time, not by text search.

- mlua can hold only about 8000 Lua values from Rust at once. Widget
  tables and script handlers live in Lua tables (`State::objects`,
  `State::script_table`), not in Rust fields.
- DB2 files have no path hashes in the root file; open them by FileDataID
  (UiTextureAtlas 897470, UiTextureAtlasMember 897532, GlobalStrings 1394440).
- Never open a GUI window on the user's desktop for testing. Use
  `run --serve` and `ctl`, or the GUI under `xvfb-run -a -s "-screen 0
  3440x1440x24"` (unset `WAYLAND_DISPLAY` first).
- Read capture text from the Windows clipboard with
  `powershell.exe -NoProfile -Command "Get-Clipboard -Raw"`; put scripts on
  it with `clip.exe < file`. Don't make the user paste large text.
- Captured return values often come back with extra optional arguments in
  the call; the replay retries with fewer trailing arguments.
- Many client functions are undocumented (glyphs, Kiosk, action bar
  highlights). Add them to `api.rs`; scan Blizzard files for undefined
  globals with `ctl eval` instead of fixing one error per run.

- Blizzard's error handler calls `addframetext` and `C_Log.LogErrorMessage`.
  If a native function that an error handler needs is missing, Lua drops
  the error silently. Tests that check "no errors" must also check that a
  known error is still reported.
- `mlua`'s `set_hook` only covers the main thread. Use `set_global_hook`
  so coroutines inherit the watchdog.
- Errors raised by Rust functions reach Lua as `Value::Error` objects, not
  strings. They are converted in the `pcall` wrappers and the error handler
  path.
- Rust raw strings `r#"..."#` end early when the Lua code contains `"#`
  (for example `select("#", ...)`). Use `r##"..."##`.
- Before relying on a Blizzard global, check whether it is defined in Lua
  in the UI source or provided by the client. `Mixin` and
  `CreateFromMixins` are native in current Retail.
- `Wow.exe` holds the names of native Lua functions as plain strings;
  Lua-defined functions are absent. That is how undocumented client
  functions are found.
- Reading thousands of small files from `/mnt/d` is slow in WSL (about
  8 s for the UI source). Cache results derived from it.
- In the client, `SetTexCoord` on an atlas texture is relative to the
  atlas, and `SetAtlas` keeps the coordinates. NineSlice relies on this.
- ab_glyph's `PxScale` is the ascent-to-descent height, and
  `pt_to_px_scale` assumes 96 DPI. FreeType (the client) sizes fonts by
  the em square: use `text::em_scale`.
- The client draws text at `size * pixels per unit` rounded to the
  nearest pixel (not rounded down: 13 at 1.2 gives 16 px); line heights in
  captures are that pixel size converted back (12 at 1.2 gives 11.67).
- To find client text rules, compare the capture's `fonts` table with the
  font files: one character alone versus ten in a row separates the
  advance from the end-of-string extra. The capture's test string had no
  shadow; Blizzard's font objects do, which adds a pixel.
- Do not make hidden frames report their own size instead of their
  anchored size: UIParent is hidden while loading, and Blizzard code
  divides by `UIParent:GetWidth()` (the action bar scale became 0).
- Blizzard_PrintHandler replaces `print`; without a chat frame its output
  is lost unless the console hooks it.
- Many old API functions moved into namespaces (`HasAction` is now
  `C_ActionBar.HasAction`); the old globals are Lua aliases in the
  Deprecated addons.
- Compare layout against capture data instead of judging screenshots: the
  comparison found the missing border, the tab and text size problems.
- Generated stubs return "" for strings and false for booleans; "" is
  true in Lua (ResurrectGetOfferer showed a popup), and false from a
  check such as `C_RestrictedActions.CheckAllowProtectedFunctions` makes
  Blizzard code silently give up. When something silently does nothing,
  look for a stub in the path (`/stubs` lists them).
- Undocumented natives that Blizzard code relies on for values (not just
  calls): `mapvalues`, `securecopy`, `GetBindingText`, metatable getters.
  A stub returning nothing breaks them quietly.
- Several client behaviours were inferred from Blizzard code, not seen:
  reverse creation order within a layer (action button icons),
  `inherit="prepend"` order (StanceBar), XML textures filling their
  parent, layers interleaved across frames at one level (tooltips). If a
  capture contradicts one of them, the capture wins.
- A capture records every client call, including ones that act or load
  (`C_AddOns.LoadAddOn`, `Set...`, `Register...`). Replaying those broke
  the sandbox (the spellbook never loaded), so `S.Replay` skips
  `C_AddOns` and action verbs, and skips records that hold `<secret>`.
- A capture can record functions that return objects the sandbox owns
  (`GetFontStringMetatable`, `GetCurrentKeyBoardFocus`). Replayed, they
  returned empty tables: font string methods vanished, and one error
  stopped `CompactRaidFrameManager_OnLoad` early, which caused more.
  `S.Replay` and the capture addon skip them. If a widget method is
  suddenly nil, check `S.replayed` for the function that returned it.
- The capture addon runs Blizzard event handlers tainted. In the game
  this makes health and power secret and floods error addons until
  `/reload`; the data is still saved.
- Homebrew's ImageMagick has no X11 support (`import` fails). Grab an
  Xvfb display with `ffmpeg -f x11grab -video_size WxH -i $DISPLAY
  -frames:v 1 out.png`. `xdotool` and `libxkbcommon-x11-0` are installed.
- Without a window manager (Xvfb), a fullscreen request does nothing; the
  GUI also moves and resizes the window to the monitor by hand.
- Wow.exe holds a usage string for most client functions, with every
  return value in order (`strings Wow.exe | grep "^Usage: "`), for
  example the 45 values of `GetCharacterInfo`. Check it before guessing
  an undocumented function's shape.
- Glue client functions answer requests with events (SelectCharacter ->
  UPDATE_SELECTED_CHARACTER). Firing them inside the call nested deep
  enough to overflow Lua's C stack; deliver them on the next frame.
- Generated stubs for undocumented functions return "" or nothing. In
  glue code that broke screen selection (`GetGameModeGlueScreenName`
  returned "") and loops (`GetSavedAccountList`). An edit box must not
  fire OnTextChanged when SetText sets the same text (AccountLogin loops).
- DB2 tables have no names in the root file; their FileDataIDs come from
  the community listfile and the column layouts from WoWDBDefs (the
  layout block for build 12.1.5.70961). Inline ID columns appear as
  value 0; non-inline IDs do not. Relation columns appear as a column and
  again as the last value.
- The loading screen's main image is 16:9 with transparent bands; drawn
  at the full screen height it matches the game exactly.
- M2 geosets differ by model: the Dracthyr dragon keeps its body in
  groups such as 34xx and 41xx. Showing the first variant of every group
  (except tabard and cape) works for all models tried.
- The commit command must not run after a failed check: use
  `if checks; then git commit ...; fi`, not `checks; git commit`.
- WoW's `xpcall` passes extra arguments (`xpcall(f, handler, self)`);
  AddonCore-based addons (TomTom, Clique) fail with "self is nil"
  without it.
- Real addons are the fastest way to find missing client behavior:
  load one with `--addon NAME --seconds 3` and read the first error.
- Commands Blizzard registers are moved out of `SlashCmdList` into a
  hidden proxy table, and secure ones (`/cast`) never enter it. Run
  slash commands through the client's macro line callback, not by
  searching `SlashCmdList`.
- Scenario unit functions answer from their first unit. Functions that
  compare two units (`UnitCanAttack("player", "target")`) need their own
  rule, or macro conditions such as `[harm]` silently fail.
- Never run `git stash` to compare against the old state: all work was
  uncommitted, and stashing hid it (it was restored at once).

- Blizzard code can tell which client behavior is right: CastingBarFrame
  hides itself at login only if `IsVisible()`, so UIParent must be shown
  while the UI loads. Read the Blizzard code that misbehaves before
  hiding a frame by hand.
- Client-written files encode CVar bitfields six flags per character from
  '@' (flag 1 is the lowest bit of the first character); a flag the user
  set in game confirmed the encoding.
- GlobalStrings.db2 keeps Lua escapes (`\n`, `\"`, `\\`, `\32`) but texture
  paths use single backslashes; unescape only those sequences.
- Tables returned by replayed functions must be copies: Blizzard's Edit
  Mode appends its presets to the table `C_EditMode.GetLayouts` returns.
- `ctl hover` does not work on a GUI window: the window sends its own
  pointer position every frame. Test hover with `run --serve`.
- Captured "numbers" include globals that Blizzard Lua sets, such as the
  load guard `g_middleClassLoaded`; setting them before the addons load
  stops the addon. Find such causes by watching assignments with a
  metatable on `_G` from a copy of a scenario.
- To find console errors, run each scenario headless, group the
  `[error]` lines with `sort | uniq -c`, and print the Blizzard source
  line of each: most come from a client function that returns nothing.
- `Blizzard_EnvironmentCleanup` sets client functions such as
  `secretunwrap` to nil in the global environment; the client's secure
  environment keeps them. A "call global (a nil value)" error for a
  function the sandbox defines may come from there.
- `cargo test` does not rebuild the `wowprogramming` binary. Run
  `cargo build` before checking a change with `run` or `ctl`.
- `ctl` needs `--` before arguments that start with a minus sign
  (`ctl -- drag 961 860 401 -792`).
- Some strings are missing from GlobalStrings.db2 as the sandbox reads it
  (probably in encrypted sections), such as
  `SOCIAL_ENABLE_DISCORD_FUNCTIONALITY`; `client.lua` defines stand-ins.

## Keeping this file current

Update this file in the same commit as the work it describes:

- Change "Current state" when a milestone, branch or merge state changes.
- Move items between "What works" and "What does not work yet".
- Rewrite "Next steps" so a new agent can start without asking.
- Add a row to "Decisions" for every decision the user makes.
- Add to "Lessons learned" when something non-obvious cost time.
- Update the date at the top.
