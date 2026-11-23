# Project status

This is the handoff document. It records where the project stands now.
Read it before starting work, and update it at the end of every piece of
work (see "Keeping this file current" below).

For goals, design and the full roadmap, see [scope.md](scope.md).

Last updated: 2026-11-23.

## Current state

- **Goal in progress:** the spellbook as a native GUI app (P opens it).
  It works for the Monk, Evoker and Paladin scenarios: tabs, paging, maximize and
  minimize, close and Escape, spell tooltips, search (typing, preview,
  Enter for full results, clear button), the settings menu (Hide
  Passives), flyouts (Hero's Path) and the Specialization tab. The
  scenarios now load the **whole default UI** (`blizzard_addons = ["*"]`),
  as the client does; the spellbook needs action bars, menus and
  tooltips. About 55 startup errors remain, none in the spellbook.
- **Next goal (started):** player and target frames, party and raid
  frames, action buttons. `scenarios/hud` shows the player frame, target
  frame, target-of-target, party frames (described units, no capture
  needed) and the main action bar with spell icons and key labels.
- **Data:** addon captures (version 2, build 12.1.5.70961) are imported
  for the Monk (Ohnoregard-Draenor), the Evoker (Ohnodwagon-Draenor) and
  the Paladin (Ohnohealme-Draenor, new `scenarios/paladin`). None had a
  target or a group, so the target, party and raid frames are not in them.
  The spellbook matches the game except for text width: the assistant
  label (68 vs 80.8) moves the search box 12.8 units, and the page text
  is 6.7 narrower.
- **Branch:** `spellbook-gui`, from `main`. Nothing is pushed. Commit
  each tested step on this branch without asking (the user's rule for
  this project); still ask before pushing or merging.
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
- Login sequence: PLAYER_LOGIN, PLAYER_ENTERING_WORLD,
  EDIT_MODE_LAYOUTS_UPDATED (from `C_EditMode.GetLayouts()`),
  VARIABLES_LOADED, UPDATE_BINDINGS, then UIParent is shown (it is hidden
  while loading, like the client's loading screen, so visible frames get
  OnShow) and LOADING_SCREEN_DISABLED.
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

- Text is about 10% narrower than in game (needs `capture.fonts`).
- Cooldown swipes, portraits (a placeholder portrait is shown), minimap,
  3D models.
- The Talents tab (needs trait data); spec sample abilities are captured
  but not checked yet.
- Health and power were secret during the capture, so they are not
  replayed: `UnitHealth` returns the stand-in 100 while `UnitHealthMax`
  is the captured 898820 (the Monk's health bar looks almost empty).
- About 55 full-UI startup errors in systems outside the goals (chat
  frames, communities, tutorials, guild UI, pet battles, LFG). Some
  repeat every tick (PersonalResourceDisplay).
- The first action button shows no key label; the chat frame background
  is white; a stray "Issue Reporter" button shows.
- Compact raid frames are not tried yet (`scenarios/hud` is a party with
  classic party frames).
- Taint and secret values.

## Next steps

1. Use `capture.fonts` to measure text; compare again (the remaining
   spellbook differences are all text width).
2. Check the captured tooltips and spec sample abilities in the
   spellbook. Give health a value consistent with the captured maximum.
3. HUD: compare `scenarios/hud` and the captured `trees` (PlayerFrame,
   TargetFrame, party, action bars); add a raid scenario (IsInRaid,
   raid1..N units) for CompactRaidFrameContainer; cooldown swipes.
4. Try the window on the user's machine.

How to run: see [running-the-gui.md](running-the-gui.md). The spellbook
and HUD scenarios take about 20 s to start; a helper that restarts a
`--serve` sandbox and waits for the bridge saves time.

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
  `run --serve` and `ctl`. Under Xvfb the window also needs
  `libxkbcommon-x11-0`, which is not installed yet.
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
- The client draws text at `floor(size * pixels per unit)` pixels; line
  heights in captures are that pixel size converted back (12 at 1.2 px per
  unit gives 11.67).
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
- The capture addon runs Blizzard event handlers tainted. In the game
  this makes health and power secret and floods error addons until
  `/reload`; the data is still saved.
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
