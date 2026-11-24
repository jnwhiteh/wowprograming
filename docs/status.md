# Project status

This is the handoff document. It records where the project stands now.
Read it before starting work, and update it at the end of every piece of
work (see "Keeping this file current" below).

For goals, design and the full roadmap, see [scope.md](scope.md).

Last updated: 2026-11-24.

## Current state

- **Goal in progress (started 2026-11-23, user's request):** the sandbox
  should feel like the game. It opens borderless full screen at the
  monitor's native size (3440x1440 on the user's machine), starts at
  Blizzard's character select screen, enters the world through a loading
  screen, and draws a 3D scene from game models behind the UI. The user
  chose "real game models" (option A), falling back to own art or simple
  shapes where that gets hard (terrain especially).
- **Where it stands:** all four parts work end to end (tested under Xvfb
  at 3440x1440): `gui --glue --scenario scenarios/charselect` shows
  character select with 13 characters from the account's addon data and
  the selected character's race model standing in a clearing with trees;
  Enter World shows the map's loading screen, then the full game UI over
  the same scene seen from behind the player. Not yet tried on the user's
  desktop.
- **Branch:** `character-select`, from `main` (the spellbook work is
  merged). Commit each tested step on this branch without asking (the
  user's rule for this project); still ask before pushing or merging.
- **Earlier goals on hold:** spellbook (functionally complete) and HUD
  (player, target, party frames, action bars). See "Next steps".
- **Data:** addon captures (version 2, build 12.1.5.70961) for the Monk
  (Ohnoregard-Draenor), Evoker (Ohnodwagon-Draenor) and Paladin
  (Ohnohealme-Draenor). The Evoker capture has a party of 5. No capture
  has a target, raid units or trait data; those captures are deferred.
- **Reference data for this goal** (in the game folders, never commit
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
  `C_EditMode.GetLayouts()`), PLAYER_LOGIN, PLAYER_ENTERING_WORLD, then
  UIParent is shown (hidden while loading, so visible frames get OnShow),
  LOADING_SCREEN_DISABLED, SPELLS_CHANGED.
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
  while the loading screen shows.
- Window: borderless and covers the monitor (`--windowed` for a normal
  window). The engine keeps the UI height in UI units and takes the
  monitor's aspect ratio and pixel size (`Engine::set_display_size`, fires
  DISPLAY_SIZE_CHANGED). F12 shows or hides the console. Third-party
  addons still load from `addons_dir`; `--no-addons` leaves them out.
- 3D view (`world.rs`, `formats/m2.rs`): M2 models and skins (vertices,
  textures, materials, sections, batches), creature skins from
  CreatureDisplayInfo, race models through ChrRaceXChrModel and ChrModel,
  a multi-threaded software rasterizer (near-plane clipping, depth buffer,
  alpha test and blending, sun light, fog, sky gradient). The demo scene:
  a grass ground (Elwynn tileset), Elwynn trees, bushes and rocks, a deer
  and a rabbit, the player's race model at the origin. About 50 ms per
  frame at 1720x720. Drawn at half resolution behind the UI
  (`render::render_over`); on character select the camera faces the
  character, in the game it is behind the player.
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
- Control bridge: `exec`, `eval`, `dofile`, `key`, `type`, `click`,
  `hover`, `mouse`, `under`, `tree`, `tick`,
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
- Cooldown swipes, portraits (a placeholder portrait is shown), minimap,
  3D models inside UI frames (PlayerModel, ModelScene).
- 3D view: models are in their bind pose (arms out); character skin and
  hair are flat colors per race; no equipment; the ground is a flat plane;
  textures have no mipmaps (distant grass shimmers); the scene is the same
  for every map. Only the selected character stands on character select
  (the game shows the warband group in a camp scene).
- Character select: the list shows a frame above the first card that the
  game does not show; warband groups are not shown. (The screen is at most
  2:1 wide, 2880x1440 on the user's monitor, like the game; class names
  are class colored.) Back, Shop, Menu and the other
  top buttons are not tried.
- Entering the world blocks the window for about 20 s; the loading bar
  does not move. There is no way back to character select (logout).
- The Talents tab (needs trait data); spec sample abilities are captured
  but not checked yet.
- Health and power were secret during the capture, so they are not
  replayed: `UnitHealth` returns the stand-in 100 while `UnitHealthMax`
  is the captured 898820 (the Monk's health bar looks almost empty).
- About 55 full-UI startup errors in systems outside the goals (chat
  frames, communities, tutorials, guild UI, pet battles, LFG). The Evoker
  scenario has 59: being in a party creates more model scenes, so the
  model camera error (`OrbitCameraMixin.lua:41`) repeats more often. Some
  repeat every tick (PersonalResourceDisplay).
- The first action button shows no key label; the chat frame background
  is white; a stray "Issue Reporter" button shows.
- Compact raid frames are not tried yet (`scenarios/hud` is a party with
  classic party frames).
- Taint and secret values.

## Next steps

Character select and world goal (branch `character-select`):

1. Ask the user to try `gui --glue --scenario scenarios/charselect
   --no-addons` on the desktop (see [running-the-gui.md](running-the-gui.md))
   and act on the report.
2. 3D view: play the Stand animation (read sequences and bones; animation
   data is in `.anim` files listed in the AFID chunk), so models stop
   standing in the bind pose. Then mipmaps for textures.
3. Character skin textures: compose from `ChrCustomizationMaterial` and
   related tables (FileDataIDs in the listfile, layouts in WoWDBDefs), or
   pick one baked texture per race as a stand-in.
4. Character select: the frame above the first card, then several
   characters (the warband group) in the scene.
5. Logout (`Logout()`/`/logout` in the game returns to character select);
   loading in the background so the loading bar moves.
6. Use the character's map: terrain from the map's ADT files is a large
   step; until then pick a scene per zone type.

On hold (spellbook and HUD goals): check the captured tooltips and sample
abilities; HUD problems (first action button key label, white chat
background, stray "Issue Reporter" button, outlined text width); compare
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
| `scenarios/charselect`, `scenarios/world` | Character select from account data; any character without its own scenario |
| `tools/capture/!WowProgrammingTrace` | In-game addon that records login and zone change events |
| `crates/engine/src/lua/client.lua` | Client functions written in Lua (quality colors, placeholder enums) |
| `crates/engine/src/lua/scenario.lua` | Scenario helpers (`S.LoadStandardCapture`, `S.Replay`, ...) |
| `crates/engine/src/data/cvar_defaults.txt` | CVar defaults for runs without a CVar capture |
| `tools/capture/WowProgrammingCapture` | In-game capture addon |
| `tools/compare/compare_frames.lua` | Compares sandbox rectangles with a capture |
| `crates/engine/src/apidoc.rs` | `Enum` and `Constants` from the API documentation |
| `crates/engine/src/savedvars.rs` | SavedVariables reading and writing |
| `crates/engine/src/lua/*.lua` | Engine helpers written in Lua (prelude, curves, API doc builder) |
| `crates/engine/tests/` | Integration tests and fixture addons |
| `crates/cli` | The `wowprogramming` command |

## Lessons learned

Things that cost time once and should not cost it again.

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
- Never run `git stash` to compare against the old state: all work was
  uncommitted, and stashing hid it (it was restored at once).

## Keeping this file current

Update this file in the same commit as the work it describes:

- Change "Current state" when a milestone, branch or merge state changes.
- Move items between "What works" and "What does not work yet".
- Rewrite "Next steps" so a new agent can start without asking.
- Add a row to "Decisions" for every decision the user makes.
- Add to "Lessons learned" when something non-obvious cost time.
- Update the date at the top.
