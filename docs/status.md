# Project status

This is the handoff document. It records where the project stands now.
Read it before starting work, and update it at the end of every piece of
work (see "Keeping this file current" below).

For goals, design and the full roadmap, see [scope.md](scope.md).

Last updated: 2026-11-22.

## Current state

- **Milestone:** M1 (headless addon runtime) is finished. M2 (XML and
  layout) has not started.
- **Branch:** M1 is on `m1-addon-runtime`, ahead of `main`. It
  is **not merged** into `main` and **not pushed**. The user decides when to
  merge and push.
- **Tests:** 44 pass (`cargo test`). `cargo fmt --check` and
  `cargo clippy --all-targets -- -D warnings` are clean.
- **CI:** a GitHub Actions workflow (Linux, Windows, macOS) exists but has
  never run, because nothing has been pushed.

## What works

- Lua 5.1 (the real C implementation, through `mlua`) with the libraries
  the game hides removed, and the native WoW additions (`strsplit`,
  `bit`, `Mixin`, `debugstack`, ...).
- A watchdog: any call into Lua that runs longer than 5 seconds fails
  with `script ran too long`, even inside `pcall` or a coroutine. Lua
  memory is capped at 1 GB.
- Loading addons from `<Name>.toc`: dependencies first, load on demand,
  `(addonName, namespace)` per file, `ADDON_LOADED`, login events.
- Frames from `CreateFrame`: events, scripts, `HookScript`, `OnShow` and
  `OnHide` through the hierarchy, stored size and anchors (no layout).
- `GetTime`, `C_Timer` and `OnUpdate` on a deterministic engine clock.
- SavedVariables, read on load and written on exit and `/reload`.
- Error and print handlers that match the client, including Blizzard's own
  error handler from `Blizzard_ScriptErrors`.
- `Enum` and `Constants` built from the API documentation files.
- Blizzard addons loaded from the UI source:
  `Blizzard_SharedXMLBase` and `Blizzard_ScriptErrors` load without Lua
  errors. Their XML files are skipped.
- Command line: `wowprogramming run` (console or `--seconds N` headless)
  and `wowprogramming lua <file>`. See the README.

## What does not work yet

- XML files are skipped (M2). Frame templates passed to `CreateFrame` are
  ignored with a warning.
- No layout, textures, font strings or rendering (M2, M3).
- No game data: CASC, BLP, DB2 (M4). `C_UIColor.GetColors()` returns an
  empty list, so named colors such as `NORMAL_FONT_COLOR` are missing.
- Most of the game API is missing (M5). Real addons stop at their first
  unit or locale call: BugSack and !BugGrabber fail on
  `UnitNameUnmodified` and `GetLocale`.
- Taint and secret values are not emulated. `issecure()` returns true.

## Next steps

1. The user decides whether to merge `m1-addon-runtime` into `main`.
2. Start M2 on a new branch, `m2-xml-layout`: XML loading (templates,
   `inherits`, `mixin`, `KeyValues`, `Scripts`, `Layers`, `Anchors`),
   textures and font strings as regions, and anchor-based layout. The
   first target is to load the three XML files in `Blizzard_SharedXMLBase`
   and then `Blizzard_SharedXML`.

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
