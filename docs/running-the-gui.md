# Running the GUI

The GUI opens a window with the sandboxed game UI on top and a console at
the bottom. With a scenario loaded, press **P** to open Blizzard's
spellbook.

## Before the first run

1. Rust from Homebrew must be on the path in WSL:

   ```sh
   export PATH=/home/linuxbrew/.linuxbrew/bin:$PATH
   ```

2. `config.local.toml` in the repo root must name the game install and the
   UI source. It is not committed. Example:

   ```toml
   game_dir = "/mnt/d/World of Warcraft/_retail_"
   ui_source_dir = "../wow-ui-source-retail"
   addons_dir = "/mnt/d/World of Warcraft/_retail_/Interface/AddOns"
   ```

## Start the window

From the repo root:

```sh
cargo run --release -- gui --scenario scenarios/monk --no-addons
```

- The first build takes a few minutes. Startup takes a few seconds: it
  reads the game data (about 2 s) and loads the Blizzard UI.
- `--scenario scenarios/monk` loads the Monk capture. The Evoker scenario
  (`scenarios/evoker`) has no `capture.txt` yet.
- `--no-addons` skips your installed third-party addons. Leave it out to
  load them too (expect many errors; they are not the target yet).
- The window opens through WSLg on your Windows desktop.

## Using it

- **P** opens and closes the spellbook. **Escape** closes open panels.
  Keys go to the game UI only when the console input line does not have
  focus; click on the UI area first.
- The mouse works on the UI: hover, click tabs, page buttons and the
  maximize/minimize button.
- The console at the bottom accepts the same lines as the in-game chat
  box for scripts:
  - `/dump PlayerSpellsFrame:IsShown()`
  - `/run PlayerSpellsUtil.ToggleSpellBookFrame()`
  - any Lua code, such as `print(UnitClass("player"))`
  - `/stubs` lists generated API stubs that were called (functions the
    sandbox does not implement yet).
- Tick **Show hover** to see the name of the frame under the mouse.
- Errors and warnings appear in the console in red and yellow.

## Driving it from another terminal

The running window also listens on 127.0.0.1:47811. From another WSL
terminal:

```sh
./target/release/wowprogramming ctl key P
./target/release/wowprogramming ctl eval "PlayerSpellsFrame:IsShown()"
./target/release/wowprogramming ctl tree PlayerSpellsFrame.SpellBookFrame 2
./target/release/wowprogramming ctl under          # frames under the mouse
./target/release/wowprogramming ctl screenshot /tmp/shot.png 0.5
./target/release/wowprogramming ctl quit
```

## Without a window

`run` does the same work without a window. `--serve` keeps it running for
`ctl`; `--screenshot` renders a PNG with the software renderer:

```sh
./target/release/wowprogramming run --scenario scenarios/monk --no-addons --serve
./target/release/wowprogramming run --scenario scenarios/monk --no-addons \
    --seconds 0.1 --exec "/run PlayerSpellsUtil.ToggleSpellBookFrame()" \
    --screenshot /tmp/spellbook.png
```

## Known gaps

- Class colors are white until `RAID_CLASS_COLORS` is captured.
- Masks, additive blending and flipbook animations are not drawn
  correctly (for example, the page-corner art shows all its frames).
- Tooltips do not show.
- A few errors at startup come from Blizzard code the spellbook does not
  need (store, edit mode, settings panel).
