# Running the GUI

The GUI opens a borderless window that covers the monitor at its native
resolution, like the game's "windowed (fullscreen)" mode, so it can be
compared with game screenshots. **F12** shows or hides a console at the
bottom. `--windowed` opens a normal window with the console shown. With a scenario loaded, press **P** to open Blizzard's
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

## Start at character select

From the repo root:

```sh
cargo run --release -- gui --glue --scenario scenarios/charselect
```

- Character select lists the characters found in your account's addon data
  (Syndicator, and the `!WowProgrammingTrace` addon for level and zone).
  The selected character's race model stands in a clearing behind the
  list. Click a card to select a character.
- **Enter World** shows the loading screen of the character's map, then
  the game UI for that character (about 20 seconds; the window does not
  respond meanwhile). The Monk, Evoker and Paladin use their captures;
  other characters use `scenarios/world`.
- To return to character select, press Escape and click **Log Out**, or
  type `/run Logout()` in the console (F12).

## Start in the world


```sh
cargo run --release -- gui --scenario scenarios/monk
```

- The first build takes a few minutes. Startup takes about 20 seconds: it
  reads the game data and loads the whole default UI (the scenarios use
  `blizzard_addons = ["*"]`). `scenarios/hud` shows unit frames, party
  frames and action bars with described units.
- `--scenario scenarios/monk` loads the Monk capture; `scenarios/evoker`
  loads the Evoker.
- `--scenario scenarios/dummy` is a training dummy session with an
  invented character and spells. Press **P** and drag a spell to the
  bar, **Tab** to target the dummy, **1** to **4** to cast, **Escape** to
  stop a cast, then to clear the target. To make new captures, see [capture.md](capture.md).
- Only the default UI loads. To load a third-party addon from
  `addons_dir` for testing, name it: `--addon BugSack` (repeat for more).
  Which addons a character has enabled in the game does not matter.
- `--watch` reloads the UI when a `.lua`, `.xml` or `.toc` file of a
  loaded third-party addon changes (save the file in your editor and the
  sandbox runs `/reload` within a second). Example for addon work:
  `cargo run --release -- gui --scenario scenarios/dummy --addon TomTom --watch`.
- Walk with W, S (back), A and D (turn), Q and E (strafe), Num Lock
  (autorun). M opens the world map. In `scenarios/dummy` the character
  stands in Goldshire; `/way 42 60` sets a TomTom waypoint.
- The window opens through WSLg on your Windows desktop. Close it with
  Alt+F4, or type `/quit` in the console.

## Using it

- **P** opens and closes the spellbook. **Escape** closes open panels.
  Keys go to the game UI only when the console input line does not have
  focus; click on the UI area first.
- The mouse works on the UI: hover, click tabs, page buttons and the
  maximize/minimize button.
- The console (F12) accepts the same lines as the in-game chat
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
./target/release/wowprogramming ctl screenshot /tmp/book.png 1.5 PlayerSpellsFrame
./target/release/wowprogramming ctl dofile tools/compare/compare_frames.lua
./target/release/wowprogramming ctl quit
```

## Without a window

`run` does the same work without a window. `--serve` keeps it running for
`ctl`; `--screenshot` renders a PNG with the software renderer:

```sh
./target/release/wowprogramming run --scenario scenarios/monk --serve
./target/release/wowprogramming run --scenario scenarios/monk \
    --seconds 0.1 --exec "/run PlayerSpellsUtil.ToggleSpellBookFrame()" \
    --screenshot /tmp/spellbook.png
```

## Known gaps

- The window draws the UI with the engine's software renderer (the same
  one that makes screenshots). It uses all CPU cores; a large window can
  still lower the frame rate.
- Text is about 10% narrower than in the game until a capture provides
  font measurements.
- The Single-Button Assistant and the specialization sample abilities need
  the next capture.
