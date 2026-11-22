# Project status

This is the handoff document. It records where the project stands now.
Read it before starting work, and update it at the end of every piece of
work (see "Keeping this file current" below).

For goals, design and the full roadmap, see [scope.md](scope.md).

Last updated: 2026-11-22.

## Current state

- **Goal in progress:** a native GUI app that loads Blizzard's spellbook
  and opens it with **P**. It works for the **Monk** scenario: P opens the
  spellbook with the Monk spells, icons, category tabs (Monk, General) and
  frame tabs (Specialization, Spellbook).
- **Branch:** `spellbook-gui`, from `main`. `main` includes M1. Nothing is
  pushed.
- **Tests:** `cargo test`, `cargo fmt --check` and
  `cargo clippy --all-targets -- -D warnings` pass.

## What works

- Everything from M1 (Lua 5.1 runtime, watchdog, addon loading, frames,
  timers, SavedVariables, console).
- TOC: `<Name>_Mainline.toc`, `[Family]`/`[Game]` placeholders, Retail load
  conditions, `[Bootstrap]` files.
- XML loader (`xml.rs`): templates, `inherits`, intrinsics, mixins,
  parent keys and arrays, key values, scripts (method, function, body,
  prepend/append), layers, anchors (applied after the tree is built),
  fonts and font families, button textures and text, animations.
- Widgets (`widgets.rs`): frames, textures, font strings, fonts, buttons,
  check buttons, edit boxes, sliders, status bars, scroll frames, animation
  groups. Missing methods get generated stubs from the widget API docs.
- Layout (`layout.rs`): rectangles from anchors, sizes and scales; text
  measurement and wrapping with the game fonts (`text.rs`).
- Game data (`crates/casc`, `crates/formats`, `gamedata.rs`): local CASC
  reader, BLP decoding, DB2 (WDC5) reading. Atlases come from
  UiTextureAtlas/UiTextureAtlasMember; global strings from GlobalStrings.db2.
- Stubs for every documented `C_*` and global function; `/stubs` lists the
  stubs that were called.
- Scene (`scene.rs`), software renderer for headless screenshots
  (`render.rs`, `run --screenshot out.png`), native window
  (`crates/gui`, `wowprogramming gui`) with a console panel.
- Key bindings from `Bindings_Standard.xml`; P runs `TOGGLESPELLBOOK`.
- In-game capture scripts in `tools/capture/`.
- Control bridge (`control.rs`): `run --serve` or `gui` listen on
  127.0.0.1:47811; `wowprogramming ctl <command>` sends `exec`, `eval`,
  `key`, `click`, `hover`, `under`, `tree`, `screenshot`, `quit`.
  Drive the sandbox through this instead of reading screenshots.
- Scenarios: a folder with `scenario.toml` (Blizzard addons and files to
  load) and `scenario.lua` (game state; replays captured API answers).
- The spellbook frame loads and renders its art; it has no spells yet.

## What does not work yet

- Evoker scenario: no `capture.txt` yet (user must run
  `tools/capture/spellbook_capture.lua` on the Evoker).
- Class colors and `LocalizedClassList`: not captured yet. Next capture
  should export `RAID_CLASS_COLORS`, `LocalizedClassList()` and
  `GetSpecializationRoleEnum(i)`.
- Masks, additive blending in the window, flipbook animations, tooltips.
- Startup errors from unrelated Blizzard code (store, edit mode,
  `SettingsPanel` in the micro menu).
- Frame rectangles are not yet compared with the captured ones
  (`frames` in the capture; screen 2866.67x1200 for the Monk).
- Taint and secret values.

## Next steps

1. Check the Monk spellbook in the GUI: General tab, paging, the
   Specialization tab. Fix errors through the control bridge.
2. Compare sandbox rectangles with `capture.frames` and fix layout
   differences.
3. Capture the Evoker and class data (see above).
4. Fix the drawing gaps listed above.

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

## Keeping this file current

Update this file in the same commit as the work it describes:

- Change "Current state" when a milestone, branch or merge state changes.
- Move items between "What works" and "What does not work yet".
- Rewrite "Next steps" so a new agent can start without asking.
- Add a row to "Decisions" for every decision the user makes.
- Add to "Lessons learned" when something non-obvious cost time.
- Update the date at the top.
