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

A second goal is to view and click addon UIs in a web browser, so that
prototypes can be shown without the game running.

## Non-goals

- No game client, networking, world rendering or gameplay.
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

**M6 and later.** Fake units and combat log scenarios. `PlayerModel` and
`ModelScene` with M2 rendering in a native `wgpu` window. Taint and secure
frames. WebAssembly export. Classic flavors.

## Backlog

Not needed yet. Each item is added when a concrete goal requires it.

- TOC path placeholders (`[Family]`, `[Game]`, `[TextLocale]`) and load
  conditions (`AllowLoadGameType`, `ExcludeLoadGameType`,
  `AllowLoadTextLocale`).
- Flavor-specific TOC files for third-party addons
  (`<Name>_Mainline.toc`, `<Name>_Classic.toc`, ...).
- `C_UIColor.GetColors()` returns nothing until DB2 reading (M4) can read
  the `GlobalColor` table. Until then named colors such as
  `NORMAL_FONT_COLOR` do not exist.
- Lua shortens chunk names longer than 60 characters in its own error
  messages (`...erface/AddOns/...`). Fixing this needs our own copy of the
  Lua 5.1 source with a larger `LUA_IDSIZE`, which taint support will need
  anyway.
- Exact order of the login events (`PLAYER_LOGIN`, `PLAYER_ENTERING_WORLD`,
  `VARIABLES_LOADED`) should be checked against the client.

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
