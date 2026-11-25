# Scope and recommendations

Status: draft for discussion, 2026-11-22.

## Goal

Build a local sandbox that loads and runs World of Warcraft addons outside
the game. The sandbox provides the things the game client normally provides
to addon code: the Lua runtime, the widget API, events, the `C_*` API
namespaces, SavedVariables, and the game's textures, fonts and (later)
models, read from the local install.

The sandbox is a development tool. It does not connect to any server and
it does not simulate the game world. Later we may add simple fakes (units,
auras, combat log events) to drive addons through combat scenarios.

Since 2026-11-23 the sandbox should also *feel* like the game: it opens
full screen at the monitor's resolution (to compare with game
screenshots), starts at Blizzard's character select screen, and enters the
world through a loading screen. A simple 3D scene made from the game's own
models stands in for the world (ground, trees, creatures, the player's race
model). It is a backdrop for the UI, not a world simulation.

A second goal is to view and click addon UIs in a web browser, so that
prototypes can be shown without the game running.

## Non-goals

- No game client, networking, map rendering or gameplay. The 3D view is a
  stand-in scene, not the map the character is on.
- No redistribution of game assets. The sandbox reads them from the local
  install at run time. Nothing extracted from the game is committed here.
- Classic and other flavors are out of scope at first. The design must not
  block them, because the UI source has one branch per flavor.

## Inputs we have

| Input | Location | Use |
|---|---|---|
| Retail client | `/mnt/d/World of Warcraft/_retail_` (build 12.1.5.70961) | Game data through CASC: textures (BLP), DB2 tables, fonts, models |
| CASC storage | `/mnt/d/World of Warcraft/Data` | Shared by all installed flavors |
| Blizzard UI source | `../wow-ui-source-retail` (branch `live`, build 12.1.5.70961). Other flavors are branches of `../wow-ui-source` | Blizzard's own Lua and XML: SharedXML, FrameXML, templates, mixins |
| API documentation | `Blizzard_APIDocumentationGenerated` (643 files) | Machine-readable signatures for every `C_*` function, event and enum |
| Installed addons | `_retail_/Interface/AddOns` | Real-world test cases (BugSack, Clique, Baganator, ...) |

Notes:

- The `Fonts` folder in the install holds only cached `.slug` files. The
  real `.ttf` fonts must be read from CASC.
- The sandbox takes a path to a UI source tree for one flavor, so it does
  not depend on which branch a checkout is on. `../wow-ui-source-retail`
  is the Retail tree.

## The key design idea

The game client is two layers:

1. **C++ engine**: Lua VM, widget objects (Frame, Texture, FontString, ...),
   layout, rendering, events, and the `C_*` API functions.
2. **Blizzard's Lua/XML UI**: SharedXML, FrameXML and about 350 Blizzard
   addons. These define the templates and mixins that third-party addons
   inherit from (`UIPanelButtonTemplate`, `Mixin`, `CreateFromMixins`,
   `EventRegistry`, ...).

We only reimplement layer 1. We load layer 2 directly from
`../wow-ui-source`. This gives high fidelity for very little code, and
every Blizzard file that loads cleanly is a free test.

We also generate the API surface from `Blizzard_APIDocumentationGenerated`.
Every documented function exists from day one, with the correct namespace
and argument checking, and returns typed default values. A scenario file
(or a test) can override return values and fire events. Hand-written
implementations replace the generated stubs one at a time, where behaviour
matters.

## Architecture

```
            +-------------------------------+
 addons --> |  Addon loader (TOC, XML, Lua) |
            +-------------------------------+
            |  Lua 5.1 runtime + WoW stdlib |
            +-------------------------------+
            |  Widget object model, events, |
            |  layout (anchors, strata)     |  <-- headless core
            +-------------------------------+
            |  API stubs (generated) + mocks|
            +-------------------------------+
            |  Game data: CASC, BLP, DB2    |
            +---------------+---------------+
                            | scene (rects, textures, text) + input
              +-------------+-------------+
              |                           |
     Browser viewer               Native window (later,
     (WebSocket + canvas)         needed for 3D models)
```

The core is headless and deterministic. It owns the clock, so tests can
advance time and fire `OnUpdate` exactly. After each tick it produces a
renderer-neutral scene: a sorted list of textured quads and text runs with
resolved screen rectangles. Renderers only draw the scene and send mouse
and keyboard input back.

Planned workspace layout. Crates are named `wowprogramming-<part>`; the
command line tool is `wowprogramming`.

```
crates/
  casc/        read local CASC storage, resolve paths and FileDataIDs
  formats/     BLP, DB2 (WDC5), later M2 and skins
  lua/         Lua 5.1 environment and WoW stdlib additions
  ui/          TOC and XML loading, widgets, layout, events
  api/         generator for API stubs from the documentation, mock layer
  server/      HTTP and WebSocket host for the browser viewer
  cli/         command line entry point: run, test, extract
web/           browser viewer (TypeScript, canvas)
docs/
```

## Language: Rust (decided)

Rust is the better fit, mainly because of the Lua runtime.

- **Lua fidelity.** WoW runs a modified Lua 5.1. The `mlua` crate embeds
  the real PUC-Rio Lua 5.1 C source (feature `lua51` + `vendored`). That
  gives exact behaviour for string patterns, `string.format`, number
  formatting, error messages and `coroutine`. Go's main option,
  `gopher-lua`, is a separate reimplementation with small differences
  that addon code will hit. Go with cgo to real Lua works, but cgo makes
  Windows builds harder.
- **Taint later.** Secure execution and taint need changes inside the VM.
  With vendored C Lua we can patch the VM source in this repo.
- **3D and the browser.** `wgpu` gives one renderer for Vulkan, Metal,
  DirectX 12 and WebGPU. Rust also compiles to WebAssembly, so a later
  "static prototype" export could run the whole sandbox in a browser with
  no server.
- **Cross-platform.** One `cargo build` on Linux, macOS and Windows. CI can
  build all three.

Go would be fine for the CASC reader, the HTTP server and most tooling.
The Lua runtime is what decides it. The cost of Rust is the learning curve.
This project's code is mostly data parsing and object models with clear
ownership (the Lua state owns the widget tree), which is a kind part of
Rust to learn on.

## Browser support

Two stages:

1. **Live viewer.** The native sandbox serves a small web page. The page
   receives the scene over a WebSocket, draws it on a canvas, and sends
   clicks and key presses back. Textures are served as PNG, converted from
   BLP on request. This is the quickest way to see something and to click
   it.
2. **Static export (later).** Compile the sandbox to WebAssembly and bundle
   it with an addon and the textures it uses, to share as a self-contained
   page. C Lua on WebAssembly needs the Emscripten target. We must prove
   this works before we depend on it. Bundled game textures are fine for
   personal use but must not be published.

## Roadmap

Each milestone ends with something that runs.

**M0: Project setup (done).** Cargo workspace, Lua 5.1 runtime that
removes the libraries the game hides, CI on Linux, Windows and macOS,
config file for paths (game install, UI source tree), this document.

**M1: Headless addon runtime (done on branch `m1-addon-runtime`).**
- Lua 5.1 with the natively implemented WoW additions: `strsplit`,
  `strjoin`, `strconcat`, `string.trim`, `table.wipe`, `bit`, `Mixin`,
  `CreateFromMixins`, `debugstack`, `GetTime`, `debugprofilestop`, print
  and error handlers, `securecall`, `hooksecurefunc`, `C_Timer`, `C_Log`,
  `C_AddOns`. Aliases such as `tinsert` come from Blizzard's `Compat.lua`.
- TOC parsing: one `<Name>.toc` per addon, `## Key: Value` metadata,
  dependencies and SavedVariables. Trailing `[...]` tags on file lines are
  removed and ignored.
- Script watchdog: a script that runs too long fails with
  `script ran too long`, even inside `pcall` or a coroutine. Lua memory
  is capped.
- Addon load order, `ADDON_LOADED`, `PLAYER_LOGIN`, `PLAYER_ENTERING_WORLD`.
- `CreateFrame`, `RegisterEvent`, `SetScript`, `OnEvent`, `OnUpdate`.
- SavedVariables written on exit and read on load.
- Slash commands and a console to type them.
- Load `Blizzard_SharedXMLBase` from the UI source.
- Pulled forward because SharedXMLBase needs them at load time: `Enum` and
  `Constants` built from the API documentation, and `C_CurveUtil`.
- Test: a hello-world addon prints on login and keeps a counter across runs.

Result: SharedXMLBase and Blizzard_ScriptErrors load without Lua errors
(their XML files are skipped). Real addons such as BugSack stop at their
first unit or locale API call (`UnitNameUnmodified`, `GetLocale`), which
M5 provides.

**M2: XML and layout.**
- XML loading against the UI schema: templates, `inherits`, `mixin`,
  `KeyValues`, `Scripts`, `Layers`, `Anchors`.
- Rect resolution from anchors, scale, strata and frame levels, show/hide.
- Load more of SharedXML and the common templates.
- Test: layout results match known positions for simple frames.

**M3: Browser viewer.**
- Scene output, WebSocket server, canvas renderer.
- Solid colour textures and text first (browser fonts as a placeholder).
- Mouse input: `OnEnter`, `OnLeave`, `OnClick`, `OnMouseDown`, dragging.
- Test: a clickable button addon works in the browser.

**M4: Game data.**
- Local CASC reader: `.build.info`, build config, encoding, root, indices,
  BLTE decoding. Community listfile (`wowdev/wow-listfile`) for path to
  FileDataID lookup.
- BLP decoding (palettized, DXT1/3/5, ARGB) to RGBA.
- DB2 (WDC5) reading for `UiTextureAtlas`, `UiTextureAtlasMember` and
  related tables, so `SetAtlas` works.
- Real game fonts from CASC.
- Test: Blizzard templates render with real art.

**M5: Generated API surface.**
- Generator that reads the API documentation and emits stubs, enums and
  event definitions.
- Scenario files (TOML or Lua) to set player, unit and group state and to
  fire events.
- Test: real installed addons load without errors.

**Visual goal: Character Frame and spellbook.** After M5, load Blizzard's
own Character Frame (`Blizzard_UIPanels_Game`) and spellbook
(`Blizzard_PlayerSpells`) and show them in the browser. This needs the
loader to pick `<Name>_Mainline.toc` for Blizzard addons that have no plain
`.toc`.

**Character select and world (started 2026-11-23, branch
`character-select`).**
- Borderless full-screen window at the monitor's native resolution.
- Glue environment: Blizzard's glue addons (`AllowLoad: Glue`), character
  select filled from the account's addon data (Syndicator, the trace
  addon), glue client functions in `lua/glue.lua`.
- Enter World: the loading screen from `LoadingScreens.db2` (drawn like the
  client, measured from a video), then a new game session with the
  character's scenario and the login events in the traced order.
- 3D view: M2 models and BLP textures from the game data, a software
  rasterizer, behind character select and the game UI.
- Test: a fresh login matches the traced event order; character select
  and the loading screen match the screenshots.
- Paused on 2026-11-24 with all four parts working. The remaining 3D
  work is in the backlog. The UI matters more than the scene behind it.

**Frame runtime (goal chosen 2026-11-24, done 2026-11-24 on branch
`frame-runtime`; all done criteria below hold).** Make the generic widget
behavior of the C++ client work, so that windows, buttons, edit boxes,
sliders and scroll bars behave as in the game. Facts, sources and open
questions are in [client-runtime.md](client-runtime.md). This goal needs
no game data and no recordings, so an agent can work on it without the
user: every step is checked by a headless test.

Done when all of these hold:

- A runtime coverage report (`wowprogramming coverage`) lists every
  script handler in `UI.xsd` and every documented widget method, with
  its state in the engine (implemented, empty, documented stub, missing)
  and its number of uses in Blizzard's code. In the areas of this goal,
  no empty method and no unfired handler with 5 or more uses remains.
  Models, game pads, movies and the `OnTooltipSet*` handlers are out of
  scope.
- These interactions work in the full UI and have tests:
  1. Drag a movable window (a fixture addon with the same code as
     BugSack's window; real BugSack loaded with `--addon` as a check). It follows the mouse, stays on screen when
     clamped, and its position comes back after `/reload` from the
     sandbox's own `layout-local.txt`.
  2. Edit Mode: enter it, drag a HUD frame (the player frame), leave it.
     The new position is saved through `C_EditMode` and comes back after
     `/reload`.
  3. Clicking the lower of two open toplevel windows raises it.
  4. Keyboard: a keyboard-enabled frame gets OnKeyDown and blocks key
     bindings unless it propagates; Escape closes the open window.
  5. Settings panel: drag a slider thumb and a scroll bar thumb; the
     value and the list change.
  6. Edit box: cursor movement, text selection (`HighlightText`),
     `SetMaxLetters`.
  7. Action buttons show a cooldown swipe after `SetCooldown`, and
     OnCooldownDone fires at the end.
  8. Unit tests for click rules: mouse capture, no click after a drag,
     double click, `RegisterForClicks`, OnEnter with `motion` false.

Steps, in order:

1. The coverage report, so that progress can be measured.
2. A `drag` command for the control bridge (press, move in steps,
   release) and an engine test helper that does the same.
3. Mouse capture and click rules (OnMouseUp to the pressed frame with
   `upInside`, click registrations, double click).
4. Dragging (`RegisterForDrag`, OnDragStart, OnDragStop, OnReceiveDrag).
5. Moving and sizing (`StartMoving`, `StartSizing`,
   `StopMovingOrSizing`, re-anchoring to the nearest UIParent point,
   clamping), then user-placed positions in `layout-local.txt`.
6. Toplevel raising, OnEnable and OnDisable, `LockHighlight`.
7. Keyboard input (`EnableKeyboard`, OnKeyDown, OnKeyUp, propagation
   before bindings).
8. Edit Mode end to end.
9. Edit box editing, sliders and scroll bar thumbs.
10. Cooldown frames, MessageFrame (`UIErrorsFrame`), hyperlinks.

Working rules for this goal: when a behavior is not known, implement the
best guess, mark it "inferred" in `client-runtime.md`, and add a precise
question to its "Open questions" list. Do not ask the user to record
sessions for single questions; one probe session answers the list later.

**Direction after the frame runtime (proposed 2026-11-24).** Four goals,
each building on the one before:

1. Frame runtime: the widgets behave like the game's (above).
2. Client state: the UI reacts to a simulated player (below).
3. Security model: taint, protected functions, combat lockdown and
   secret values, so addon authors see "action blocked" errors in the
   sandbox before they ship.
4. Tools for addon authors: scripted interaction tests that run headless
   in CI, reload on file change, and showing a UI in the browser.

**Client state: a training dummy session (next goal after the frame
runtime).** The user's character stands in front of a training dummy,
and the interface works as in the game. The C++ client holds the state
that Blizzard's Lua asks about and changes it when the UI acts. This
goal builds that state as one model in the engine:

- **The model.** Player, units (the player, a training dummy, the
  captured party), spells known, action slots, cooldowns, auras, power,
  the cursor, the target. Filled from the capture and DB2 tables. Every
  client function reads it, and every change fires the events that the
  API documentation lists, with their documented payloads.
- **The character's own setup, read only from the WTF folder:** macros, key bindings and Edit Mode layouts (see
  "Sources of truth" in [client-runtime.md](client-runtime.md)).
- **Simple rules, from data.** Cast times, cooldowns, costs and auras
  come from DB2 (SpellMisc, SpellCastTimes, SpellCooldowns, SpellPower,
  SpellAuraOptions, SpellDuration). Every spell succeeds. The dummy never
  dies. No per-spell code: a spell that needs special handling goes to
  the backlog.
- **Out of scope:** damage formulas, movement, range from positions,
  threat, loot, quests, other players.

Done when these work in the full UI, each with a headless test:

1. Drag a spell from the spellbook to an action bar; it is still there
   after `/reload`.
2. Target the dummy (Tab or a click on its frame); the target frame
   shows it. Clear the target with Escape.
3. Press an action key. The cast bar fills for the spell's cast time,
   the global cooldown swipe shows on all buttons, the spell's own
   cooldown shows on its button, power goes down and comes back.
   Instant, cast and channeled spells all work. Escape and
   `/stopcasting` cancel a cast (Blizzard's Escape handler calls
   `SpellStopCasting()` before it closes windows, in
   `Blizzard_Game/Shared/Game.lua`).
4. Buffs and debuffs. A spell that buffs the player shows the buff in
   the buff frame with its timer and stacks; right-click removes it. A
   spell that applies a debuff to the dummy shows it on the target frame
   with its timer, and it ends when the time runs out. A scenario can put
   a debuff on the player (for example a dispellable magic debuff), and
   the debuff frame shows it with its type color.
5. Tooltips from data for spells, action buttons, auras and units,
   drawn by Blizzard's `TooltipDataProcessor`.
6. Slash commands and macros: `/cast`, `/target`, `/reload`, `/logout`,
   and a macro on an action bar.
7. Clique works: a click on a party frame casts the bound spell on that
   party member.

Progress (2026-11-25): the model is `crates/engine/src/lua/clientstate.lua`,
tested with invented data in `scenarios/dummy` (test
`training_dummy_session`). Criteria 1 to 4 work, except `/stopcasting`
(Escape works), which needs the chat frame's slash commands (criterion
6). A click on the player frame targets the player; the dummy has no
frame of its own (no nameplates), so Tab targets it. Criteria 5 to 7
are not started. The data each step still needs is listed in
[client-runtime.md](client-runtime.md) ("Client state: invented data
and where real data is needed").

Unknown behavior is handled as in the frame runtime goal: a best guess
marked "inferred", and a question for the probe list. Some questions
here (event order for one cast) may justify a short probe session; ask
the user first.

Later, in this goal or after it: bags and items (item tooltips, using a
consumable, items on action bars), the combat log for damage meters.

**M6 and later.** Fake units and combat log scenarios. `PlayerModel` and
`ModelScene` with M2 rendering in a native `wgpu` window. Taint and secure
frames. WebAssembly export. Classic flavors.

## Backlog

Not needed yet. Each item is added when a concrete goal requires it.

- Frame runtime leftovers: cooldown countdown numbers (with
  `SetCountdownFont`), the edge and bling textures, swipe rotation and
  `SetTexCoordRange`; placing the edit box cursor at a click; edit box
  history keys and IME; drawing indented word wrap; OnKeyDown for
  modifier keys alone; the game menu on a second Escape; slider paging
  with `stepsPerPage`; the documented stubs that `coverage` still lists
  (SimpleHTML, Line endpoints, ColorSelect, `DesaturateHierarchy`,
  `FontString:SetRotation`).
- Real Edit Mode layouts and other character data from the game's WTF
  folder (`edit-mode-cache-*.txt`), read only (client state goal).

- TOC path placeholders (`[Family]`, `[Game]`, `[TextLocale]`) and load
  conditions (`AllowLoadGameType`, `ExcludeLoadGameType`,
  `AllowLoadTextLocale`).
- Flavor-specific TOC files for third-party addons
  (`<Name>_Mainline.toc`, `<Name>_Classic.toc`, ...).
- `C_UIColor.GetColors()` returns nothing without a capture. The
  `GlobalColor` table is `dbfilesclient/globalcolor.db2` (FileDataID
  4328468); reading it would remove the need for a capture.
- Lua shortens chunk names longer than 60 characters in its own error
  messages (`...erface/AddOns/...`). Fixing this needs our own copy of the
  Lua 5.1 source with a larger `LUA_IDSIZE`, which taint support will need
  anyway.
- 3D view: animation (the Stand pose instead of the bind pose), composed
  character skin textures (`ChrCustomization*` tables), equipment models,
  real terrain from the map's ADT files, mipmaps for distant textures.
- Character select: warband groups and their scenes (`WarbandScene*`
  tables), the camp scene, several characters at once; the frame above
  the first card.
- Character textures from the customization tables. Blocker: the DB2
  reader returns no relation value for ChrCustomizationOption (check the
  relationship section in `formats/db2.rs`).
- Load the game session in the background so the loading bar moves.

## Risks

- **Layout fidelity** is the largest effort on the UI side: anchor
  resolution, pixel snapping, text measurement and wrapping, and scroll
  frames. We will compare against screenshots from the real client.
- **Taint and secret values** (new in 12.0) need VM changes. We stub
  `issecurevariable`, `InCombatLockdown` and similar until then.
- **Encrypted CASC files** need TACT keys. Most interface art is not
  encrypted, so this can wait.
- **API breadth.** There are thousands of functions. Generated stubs keep
  addons loading, but behaviour will only be as good as what we implement.

## Open questions

1. Should scenarios be written in Lua (familiar to addon authors) or in a
   data format such as TOML?
2. Which real addons should be the first test targets after hello world?
