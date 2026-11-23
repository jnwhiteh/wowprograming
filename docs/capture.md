# Capturing game data

Some data exists only inside the running game client: the answers of API
functions for your character, CVar defaults, some atlases, text metrics and
the edit mode layout. The WowProgrammingCapture addon records it in one go.
The sandbox replays it in a scenario.

## How to capture

1. Copy the folder `tools/capture/WowProgrammingCapture` into
   `World of Warcraft/_retail_/Interface/AddOns/`. (The sandbox never writes
   into the game folder, so this step is manual.)
2. Log in on the character. Make sure the addon is enabled on the character
   select screen.
3. For unit frames: target something, ideally a training dummy or an NPC
   with a health bar. For party and raid frames: be in a party, if you can.
4. Out of combat, type `/wpcapture`. The spellbook opens for a moment. After
   about two seconds the chat shows "capture done".
5. Type `/reload`. This writes the data to
   `WTF/Account/<ACCOUNT>/SavedVariables/WowProgrammingCapture.lua`. It also
   clears the taint that the recording leaves behind (the recording
   replaces API functions for a moment; protected actions can fail until
   the reload).
6. Repeat for other characters. Each character has its own entry in the
   file.

Then import it into a scenario:

```sh
./target/release/wowprogramming import-capture \
    "/mnt/d/World of Warcraft/_retail_/WTF/Account/<ACCOUNT>/SavedVariables/WowProgrammingCapture.lua" \
    scenarios/monk --character "Name-Realm"
```

Without `--character`, the newest capture is used. The command writes
`capture.txt`; `client_constants.txt` is no longer needed for new captures.

## Wanted captures

| Scenario | Character | Setup |
|---|---|---|
| `scenarios/monk` | the Monk | target a training dummy; spellbook minimized as before |
| `scenarios/evoker` | the Evoker | same |
| unit and raid frames | any character | in a party of 2 to 5, with a target |

## What the addon records

- **calls**: every client API call that the default UI makes while its
  frames refresh. The addon wraps the client functions, calls the
  `OnEvent` handlers of all frames for the usual login and unit events,
  opens the spellbook, then restores the functions. It also calls some
  functions directly: specializations, action slots, units, and for every
  spellbook slot and flyout spell its info, tooltip
  (`C_TooltipInfo.GetSpellBookItem`, `GetSpellByID`, `GetAction`) and
  description. Secret values are stored as `<secret>`.
- **trees**: rectangles, art, text and fonts of the shown parts of the
  spellbook, player, target, focus, pet, party and raid frames, action
  bars, micro menu, bags bar, buffs, minimap, objective tracker and chat.
  `tools/compare/compare_frames.lua` compares them with the sandbox.
- **atlases**: `C_Texture.GetAtlasInfo` for every atlas in those frames.
  The game tables on disk lack some (for example the metal frame border).
- **constants**: named colors, `Enum`, `Constants` and global numbers.
- **enums**: enums that do not show up when walking `Enum`, by name.
- **cvars**: every CVar with value and default, from
  `ConsoleGetAllCommands()` and `C_CVar.GetCVarInfo`.
- **fonts**: width of each ASCII character (alone and ten times), line
  height and some sample strings, for the main fonts at sizes 8 to 32.
  The client draws hinted text at whole pixel sizes; the sandbox cannot
  reproduce hinting, so it can use these widths instead.
- **meta**: build, screen and physical size, UI scale, class colors and
  names, whether a target and a group exist.

## Data the sandbox still needs (and why)

Found while getting the spellbook and the full UI to load. The addon
records all of these.

| Data | Used by | Found because |
|---|---|---|
| `C_AssistedCombat.IsAvailable`, `GetActionSpell`, `GetRotationSpells` | spellbook top bar | The game shows the Single-Button Assistant there; the search box and settings button sit 134 units further left |
| `C_SpecializationInfo.GetSpellsDisplay(specID)` | Specialization tab | "Sample Abilities" is empty |
| `C_SpecializationInfo.CanPlayerUseTalentUI`, `IsPlayerInitialSpec` | Talents tab | Assumed true/false in the scenarios until captured |
| CVar values and defaults | text size, many settings | `GetCVarDefault` has no data; `cvar_defaults.txt` holds a few by hand |
| `Enum.StoreError` and other glue enums | `VASErrorLookup.lua` | Missing from the walked `Enum`; placeholders until captured |
| Atlases missing from `UiTextureAtlasMember` | frame borders | Not in the DB2 file or the hotfix cache |
| Text widths per font and size | text layout | Measured widths are about 10% narrower than in game (`Page 1/5`: 56.7 vs 63.3) |
| Font size of captured font strings | layout comparison | The old capture recorded only the font file |
| Unit data for player, target and party | unit frames, raid frames | Needed for the next goals |
| Action bar slots and edit mode layouts | action bars, HUD positions | Needed for the next goals |

## Older scripts

`tools/capture/spellbook_capture.lua` and
`tools/capture/client_constants_capture.lua` are the first versions, pasted
into WowLua. The existing `capture.txt` files come from them; the scenarios
still read that format.
