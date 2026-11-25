# Client runtime: what the C++ client provides to the UI

This document is the reference for reimplementing the parts of the game
client that the user interface runs on. It records where the facts come
from, what the sandbox covers, the rules found so far and the questions
that are still open. Read it before working on input, frames, the cursor,
casting or tooltips.

Written 2026-11-24. Update it when a rule is found, a question is
answered or coverage changes.

## Why this matters

Blizzard's Lua and XML already implement almost all of the interface. They
stand on a layer that the C++ client provides. The sandbox loads the Lua
and XML well, but much of that lower layer is empty: a button press runs
Blizzard's code correctly and then ends in a function that does nothing.
The goal is a client that behaves like the real one for the interface,
not a game. Combat rules, the 3D world and the network are out of scope.

The client layer has three parts. They are built in this order, because
each one needs the one before it:

1. **Frame runtime.** Generic widget behavior: mouse and keyboard input,
   dragging, moving and sizing frames, clamping, raising, focus, edit box
   editing, sliders, scroll bars, hyperlinks, cooldown swipes, message
   frames. Needs no game data. Defined by `UI.xsd` and the widget API
   documentation.
2. **Client state.** What the UI asks the client about the player: the
   cursor (what is picked up), action slots, casting, cooldowns, power,
   the target. A small simulation, not combat rules: every spell
   succeeds, nothing dies unless a scenario says so. Spell data comes
   from DB2 tables.
3. **Tooltip data.** Retail tooltips are drawn by Blizzard Lua
   (`TooltipDataProcessor`) from lines that `C_TooltipInfo` returns. If
   the sandbox builds those lines from DB2, Blizzard's code draws real
   tooltips.

## Sources of truth

Use them in this order. Each later source costs more than the one before.

1. **`Blizzard_SharedXML/UI.xsd`** in the UI source. It defines every
   widget type, XML attribute and script handler (`ScriptsType` lists 75
   handlers). A handler in the schema is a promise that the client fires
   it under some condition.
2. **Widget API documentation**, `Blizzard_APIDocumentationGenerated/
   Simple*APIDocumentation.lua` and `FrameAPI*Documentation.lua`. Every
   widget method with its arguments, return values, nilability, and flags
   such as `IsProtectedFunction` and `SecretArguments`. The engine
   already builds stubs from these files (`apidoc.rs`, `WIDGET_DOCS`).
   `InputDocumentation.lua`, `GameCursorDocumentation.lua` and
   `CursorDocumentation.lua` cover mouse state and the cursor.
3. **Usage strings in `Wow.exe`** (`strings Wow.exe | grep "^Usage: "`)
   for undocumented client functions, with every return value in order.
4. **Blizzard's Lua and XML as the consumer.** How Blizzard code calls a
   function shows what it needs. For example, Edit Mode calls
   `StartMoving` in `OnDragStart`, then reads the frame's points to snap
   it. Every Blizzard addon that works unchanged is a test.
5. **Files the client writes, read only.** Never write into the game
   install. The character folder `WTF/Account/<account>/<realm>/
   <character>/` holds:
   - `layout-local.txt`: positions of frames the user moved. It shows the
     client's own result of a move (see "Moving and sizing" below).
   - `AddOns.txt`: which addons are enabled for the character
     (`Name: enabled` or `Name: disabled`, one per line). The sandbox
     does not use it: only the default UI loads, plus addons named with
     `--addon` (user's decision, 2026-11-24).
   - `macros-cache.txt`: the character's macros (`VER 3 <id> "<name>"
     "<icon>"`, the macro lines, `END`).
   - `bindings-cache.wtf`: key bindings (`bind <key> <command>`).
   - `edit-mode-cache-character.txt`, `click-bindings-cache.txt`,
     `cooldownmanager.txt`, `config-cache.wtf` (character CVars),
     `chat-cache.txt`.
   The account folder has more of the same for account-wide data. These
   files are real data for the client state goal, with no capture
   needed. Treat them as the account's personal data: read them at run
   time, never copy them into the repo.
6. **warcraft.wiki.gg.** Community documentation of behavior: event
   order, defaults, edge cases. Each rule taken from it below names its
   page. The wiki is usually right but not always complete.
7. **An in-game probe, last.** Recording sessions is slow for the user,
   gives single traces, and can produce huge files. Do not record broad
   traces of interactions. Instead, collect precise questions in "Open
   questions" below. When enough have collected, write one small probe
   addon that answers all of them in one short session and saves only
   the answers (a few values each). Ask the user before each probe
   session.

When two sources disagree, the higher-numbered source that observed the
real client wins over a lower one that only describes it (a probe answer
beats the wiki; `layout-local.txt` beats the wiki).

## Coverage

`wowprogramming coverage` prints the report (`--all` lists every row). It
reads the widget metatables of a fresh engine at run time: a documented
method is *implemented*, *empty* (registered with the `empty!` macro or
`empty_fn` in `widgets.rs`: it does nothing or returns a fixed value),
a documented *stub* (built from the API documentation), or *missing*. A
handler is *fired* if it is in `frames::FIRED_SCRIPTS`; a unit test keeps
that list equal to the names passed to `fire_script` in the source. Use
counts are how often Blizzard's Lua and XML call `:Method(` or name the
handler (`"OnX"`, `<OnX`). They count by name over all widget types, so
`SetEnabled` counts button calls too.

Areas: *frames* (this goal), *later* (GameTooltip methods and the
`OnTooltipSet*` handlers, for the client state goal), *out* (models,
model scenes, map scenes, the minimap, movies, the browser, game pads,
IME). A *gap* is an empty or missing method, or an unfired handler, in
the frames area with 5 or more uses. The goal is done for coverage when
no gap remains.

Report at the end of the frame runtime goal (2026-11-24): 42 handlers
fired, 570 methods implemented, 23 empty, 431 stubs, 16 missing (in the
frames area: 563 implemented, 23 empty, 204 stubs). **No gaps.** Handler
names now count only in `<OnX` elements and `...Script("OnX"` calls, so
Lua methods that share a handler's name (OnAnimFinished) no longer
count. The unfired handlers in the frames area all have fewer than 5
uses (OnArrowPressed, OnColorSelect, OnMinMaxChanged, OnAnimStarted,
OnMessageScrollChanged).

First report (2026-11-24): 75 handlers in `ScriptsType`, 27 fired;
1040 documented methods, 436 implemented, 76 empty, 512 stubs, 16
missing (790 in the frames area). 45 gaps. The largest: OnDragStart 175
uses, OnKeyDown 110, OnReceiveDrag 80, OnDragStop 70, OnHyperlinkClick
41, OnDoubleClick 23, OnHyperlinkEnter 22, OnCursorChanged 20, OnKeyUp
18, OnScrollRangeChanged 15, OnCooldownDone 14, OnTooltipCleared 12,
OnAnimFinished 10; edit box `Enable`/`Disable`/`SetEnabled`/`IsEnabled`,
`RegisterForDrag`, `IsCollapsed`, frame `LockHighlight`, `StartMoving`,
`StopMovingOrSizing`, `HighlightText`, `Raise`, `SetMaxLetters`, edit box
cursor position, `EnableKeyboard`, `SetUserPlaced`, `StartSizing`,
clamping. Stubs with many uses in the area: Slider enable methods,
MessageFrame `AddMessage` and `Clear`, Cooldown `Clear`, `SetCooldown`
and the swipe setters, Animation `SetParent`, `Restart`, `SetPlaying`.

## Rules for the frame runtime

Sources: W = warcraft.wiki.gg page named after the function or handler
(for example `API:Frame_StartMoving`, `UIHANDLER_OnClick`); L =
`layout-local.txt`; B = Blizzard code.

### Mouse buttons and clicks

- A new button is registered for `LeftButtonUp` only (W
  RegisterForClicks). `RegisterForClicks()` with no arguments removes all
  registrations; each call replaces the previous set. It controls
  OnClick, PreClick, PostClick and OnDoubleClick, not OnMouseDown,
  OnMouseUp or OnDragStart.
- Order for one click: OnMouseDown, OnMouseUp, PreClick, OnClick,
  PostClick (W OnClick). OnClick takes `(self, button, down)`.
- **Mouse capture.** The frame that got OnMouseDown also gets OnMouseUp,
  even if the cursor left it. OnMouseUp takes `(self, button, upInside)`
  (W OnMouseUp). OnClick for an Up registration fires only if the cursor
  is over the button at release (W OnClick). A Down registration clicks
  at the press, with `down` true. Implemented in `input.rs`. While one
  button is held, the sandbox ignores presses of another (inferred).
- **Double click.** Press, release, press, release over the button, the
  last three quickly (W OnDoubleClick). Needs the Up registration. When
  OnDoubleClick fires it replaces PreClick, OnClick and PostClick for that
  release, so rapid clicks alternate between OnClick and OnDoubleClick.
  The sandbox uses 0.5 s between the two releases, the Windows default
  (inferred), and only replaces the click when the frame has an
  OnDoubleClick handler (inferred; otherwise action buttons would lose
  every second click).
- **OnEnter and OnLeave** take `(self, motion)`. `motion` is true for real
  mouse movement and false otherwise, for example when a frame is shown
  under a still cursor (W OnEnter). The sandbox checks the mouse focus
  once per frame, after layout or visibility changed, and fires with
  `motion` false (the timing, next frame, is inferred).
- **Clicks and motion are separate.** OnEnter and OnLeave go to the top
  frame with mouse motion or clicks enabled; a press goes to the top
  frame with clicks enabled. A frame with only motion
  (`SetMouseClickEnabled(false)`, `enableMouseMotion="true"`) lets
  presses through to the frame below (B: the player frame's health bar
  does this so a click on it targets the player; W
  SetMouseClickEnabled).
- **Click bindings.** `SecureUnitButton_OnClick` asks
  `C_ClickBindings.GetBindingType` first and ignores a `target` or
  `menu` click unless it returns `Interaction` (B). The default profile
  is left click Target and right click Open menu: every character's
  `click-bindings-cache.txt` has the lines `1 0 3 1` and `4 0 3 2`
  (button code, modifiers, `Enum.ClickBindingType`, action: Target 1,
  OpenContextMenu 2; button 1 is LeftButton, 4 is RightButton). A spell
  binding looks like `1 48 1 2061`. The sandbox keeps the profile in
  `lua/client.lua` and does not read the file yet.

### Dragging

- `RegisterForDrag(...buttons)` names the buttons that can start a drag;
  default none, no arguments clears, each call replaces (W
  RegisterForDrag). The frame must also take mouse clicks.
- OnDragStart takes `(self, button)`. After a drag starts, OnMouseUp and
  OnClick for that button do not fire; OnDragStop fires at release (W
  RegisterForDrag, W OnClick).
- OnReceiveDrag fires on the mouse-enabled frame under the cursor when a
  drag is released over it. It does not clear the cursor (W
  OnReceiveDrag). It also fires when the drag was a frame move, with
  nothing on the cursor.
- The sandbox starts a drag when the mouse has moved 4 pixels from the
  press, the Windows default (inferred). At release, OnDragStop fires on
  the dragged frame first, then OnReceiveDrag on the frame under the
  mouse (order inferred). A button stops looking pushed when its drag
  starts (inferred).

### Moving and sizing

- `StartMoving([alwaysStartFromMouse])` errors with `Frame:StartMoving():
  Frame is not movable` unless `SetMovable(true)` was called. It sets the
  user-placed flag and clears all anchors (W StartMoving). While moving,
  the frame follows the cursor by the cursor's movement.
- `StopMovingOrSizing()` anchors the frame by its point that is closest
  to the matching UIParent point (W StopMovingOrSizing). The nine points
  count, not only the corners: `layout-local.txt` holds anchors `LEFT` and
  `CENTER` (L). Hiding the frame or an ancestor while moving also stops
  the move (W StartMoving).
- `StartSizing([resizePoint, alwaysStartFromMouse])`, default point
  `BOTTOMRIGHT` (W StartSizing). Sizing respects `SetResizeBounds`.
- `StartSizing` errors with `Frame:StartSizing(): Frame is not
  resizable` unless `SetResizable(true)` was called (message inferred
  from the StartMoving one).
- The sandbox moves the frame by the mouse's movement since the press,
  so it does not lag behind after the drag threshold; with
  `alwaysStartFromMouse` it uses the movement since the call (inferred
  from the name). While moving or sizing, the frame has one anchor,
  TOPLEFT to UIParent's TOPLEFT (inferred).

### User-placed positions

- The client saves named, user-placed frames in `layout-local.txt` per
  character, and restores them for frames created before PLAYER_LOGIN.
  Anonymous frames are not restored. `SetUserPlaced(false)` right after
  `StartMoving` prevents saving (W SetUserPlaced, W StartMoving).
- File format (L):

  ```
  Version: 1
  Frame: GroupLootHistoryFrame
  FrameLevel: 1
  Anchor: LEFT
  X: 127
  Y: 78
  W: 261
  H: 590
  ```

  The sandbox keeps its own copy in its WTF folder
  (`<wtf>/Account/<account>/<realm>/<character>/layout-local.txt`,
  `layoutcache.rs`), never in the game install. It restores named,
  movable frames after the addons load and before the login events, and
  saves at `/reload` and logout, keeping entries of frames that do not
  exist in the session (timing and keeping inferred). The size comes
  back only for resizable frames (inferred). `FrameLevel` is 1 in every
  file seen; the sandbox writes 1 and ignores it. Values are whole
  numbers (L).
- Real BugSack creates `BugSackFrame` when the sack first opens, after
  PLAYER_LOGIN. By the rule above its position is saved but not
  restored, and the sandbox behaves so (checked with `--addon BugSack`).
  The test fixture `MovableWindow` creates the same window at load time,
  and its position comes back.

### Clamping

- `SetClampedToScreen(true)` keeps the frame on screen for `SetPoint`,
  moves and sizing (W SetClampedToScreen). `SetClampRectInsets` adds
  padding inside or beyond the screen edges. The insets
  `(left, right, top, bottom)` move the frame's edges in screen
  directions (positive is right and up), and that rectangle must lie
  inside the screen (B: `SetClampRectInsets(-10, 10, 10, -10)` grows the
  rectangle by 10 on every side; `(0, 200 - width, 10, 0)` keeps only the
  left 200 units and 10 above the top on screen). The sandbox clamps the
  rectangle in layout; `GetPoint` still returns the offsets that were set
  (inferred).

### Raising and frame levels

- A toplevel frame (`SetToplevel(true)`, XML `toplevel="true"`) raises its
  frame level when it is clicked or shown (W SetToplevel). The sandbox's
  `Raise` puts the frame one level above every other visible frame of
  its strata and moves its children by the same amount; `Lower` does the
  opposite (inferred). A click raises the nearest toplevel frame at or
  above the clicked one.

### Enabled state

- Buttons, edit boxes and sliders have `Enable`, `Disable`, `SetEnabled`
  and `IsEnabled`; OnEnable and OnDisable fire only on a change (B: the
  documentation lists the methods for all three).
- Edit boxes take mouse clicks without `enableMouse` (inferred: only 5
  of 189 edit boxes in Blizzard's XML set it). A disabled edit box does
  not take the focus from a click and loses it when disabled (inferred).

### Edit boxes

Implemented in `editbox.rs`.

- Positions are byte offsets (`GetCursorPosition`;
  `GetUTF8CursorPosition` counts characters) (W, the documentation names
  both).
- `autoFocus` defaults to true (UI.xsd). An edit box with autoFocus
  takes the keyboard focus when it becomes visible, after its OnShow
  (order inferred). The new layout dialog of Edit Mode relies on it.
- Typing inserts at the cursor and replaces highlighted text; OnChar
  then fires for each inserted character. `SetMaxLetters` and
  `SetMaxBytes` limit typing, `SetText` and `Insert`; a lower limit cuts
  the current text (inferred). Numeric edit boxes drop non-digits.
- `SetText` (and `SetNumber`, `Insert`) puts the cursor at the end and
  clears the highlight, and fires OnTextSet at once. OnTextChanged(false)
  follows on the next frame, once, if the text then differs from the
  text last reported (inferred: Blizzard's currency transfer box calls
  `SetNumber` in its OnLoad, before its parent's OnLoad, and its
  OnTextChanged handler fails if it runs inside the call). Typed text
  fires OnTextChanged(true) at once. `HighlightText()` selects all; `(start, end)` selects a
  range; a negative end means the end; equal positions clear it.
  `SetCursorPosition` clears the highlight (inferred).
- Keys: BACKSPACE, DELETE, LEFT, RIGHT, HOME, END (to the line ends in
  multi-line boxes), SHIFT with arrows and HOME/END grows the highlight,
  ENTER (OnEnterPressed; a multi-line box without that handler inserts a
  new line, inferred), ESCAPE, TAB, SPACE (OnSpacePressed).
- OnCursorChanged gets `(x, y, width, height)` of the cursor: x is the
  pen position after the text before the cursor, y is minus the line
  height times the line number, width 1 (width inferred).
- Not yet: clicking places the cursor at the click (the cursor stays
  where it was), history keys, IME.

### Sliders and scroll frames

Implemented in `slider.rs` and `widgets.rs`.

- Sliders are vertical unless told otherwise, and the thumb is drawn in
  the OVERLAY layer (UI.xsd defaults). Sliders take the mouse without
  `enableMouse` (inferred).
- The thumb keeps its own size and moves along the slider, inside it:
  left to right, or top to bottom for vertical sliders (inferred; its
  anchors are ignored).
- A left press on an enabled slider grabs the thumb where it was
  pressed; a press beside the thumb moves the thumb's centre to the
  mouse. While the button is held, the value follows the mouse, rounded
  to the value step if `SetObeyStepOnDrag(true)`; OnValueChanged gets
  `userInput` true (inferred). `SetValue` clamps but does not round
  (inferred).
- Blizzard's scroll bars (`ScrollBarMixin`) are Lua: the thumb's
  OnMouseDown installs an OnUpdate that reads `GetCursorPosition`. They
  need frames to pass while the mouse moves; `Engine::drag` and the
  `drag` command advance one frame (1/60 s) per step.
- OnScrollRangeChanged `(xrange, yrange)` fires once per frame when a
  scroll frame's range (scroll child size minus frame size) changed, and
  at once from `UpdateScrollChildRect` (inferred).

### Message frames

Implemented in `messageframe.rs`.

- `AddMessage(text, r, g, b[, a, messageID])` adds a line; without a
  colour the line is white (inferred). A message with the ID of an
  earlier one replaces it (inferred). `Clear` removes all.
- Defaults from UI.xsd: fading on, 10 s visible (`displayDuration`), 3 s
  fade, fade power 1, insert mode BOTTOM. While fading, the alpha falls
  as `(1 - t) ^ power` (shape inferred) and the line is dropped at the
  end. TOP puts new lines at the top.

### Hyperlinks

Implemented in `hyperlink.rs`; the text layout keeps the `|Hdata|h`
data on each run.

- A frame with `SetHyperlinksEnabled(true)` (or `hyperlinksEnabled` in
  XML) gets the scripts for links in the visible font strings of itself
  and its child frames (and its own lines if it is a message frame)
  (scope of child frames inferred).
- OnHyperlinkEnter `(link, text, region, left, bottom, width, height)`
  and OnHyperlinkLeave `(link, text)` fire as the mouse moves over and
  off a link; OnHyperlinkClick `(link, text, button, region, left,
  bottom, width, height)` fires at a release inside the pressed frame,
  before OnClick (order inferred). The rectangle is in the frame's units
  (inferred).

### Edit Mode

- The client keeps the saved layouts. `C_EditMode` is in
  `lua/client.lua`: `GetLayouts` returns the account layouts, then the
  character layouts, and the active layout index (counting Blizzard's
  presets first); `SaveLayouts` drops the presets that Blizzard's Lua
  passes in; `GetAccountSettings` returns every
  `Enum.EditModeAccountSetting` (0, grid spacing 100 from
  `Constants.EditModeConsts`). The sandbox stores them in its own
  SavedVariables file `WowProgramming_EditMode.lua` (account and
  character scope), written at `/reload` and logout. The real client
  keeps them on the server and in `edit-mode-cache-*.txt`; reading the
  account's real layouts belongs to the client state goal.
- `SetActiveLayout` and `OnLayoutAdded(index, activate)` make the client
  send EDIT_MODE_LAYOUTS_UPDATED with the layouts on the next frame;
  Blizzard's Edit Mode applies the active layout then (inferred:
  `SelectLayout` does nothing else). After `OnLayoutDeleted` the sandbox
  activates the first preset if the deleted layout was active
  (inferred).
- Selection frames come from XML with `registerForDrag`, `toplevel` and
  `dontSavePosition`; the XML loader now applies those and the other
  frame attributes of UI.xsd (`resizable`, `clampedToScreen`,
  `enableKeyboard`, `propagateKeyboardInput`).

### Keyboard

- `EnableKeyboard(true)` lets a shown frame get OnKeyDown and OnKeyUp
  `(self, key)`. The order is: the edit box with focus first, then
  keyboard-enabled frames by strata and frame level, top first (W
  EnableKeyboard).
- A frame consumes keys unless `SetPropagateKeyboardInput(true)`; then
  the key goes on to the next frame. Key bindings run last, on WorldFrame.
  A visible keyboard-enabled frame that does not propagate blocks all
  bindings. A frame that propagated OnKeyDown does not get the matching
  OnKeyUp. A frame can call `SetPropagateKeyboardInput` inside OnKeyDown
  to consume only some keys (W SetPropagateKeyboardInput).
- For an edit box, OnKeyDown fires before OnChar, OnEscapePressed and
  OnTextChanged (W OnKeyDown).
- Implemented in `input::key_down` and `key_up`. The focused edit box
  takes every key (inferred); keyboard frames are tried in drawing order,
  top first; OnKeyDown and OnKeyUp get the key without modifiers (`P`
  for `SHIFT-P`, inferred). The sandbox has no OnKeyDown for the
  modifier keys themselves yet.
- Escape runs Blizzard's `CloseAllWindows`, which closes every open
  panel at once. The sandbox does not open the game menu on a second
  Escape (the game menu is not loaded).

### Cooldown frames

- `SetCooldown(start, duration[, modRate])`: start is a `GetTime()` value,
  0 or a duration of 0 means no cooldown. It shows the widget, and the
  widget hides itself when the animation ends (W SetCooldown). The
  sandbox (`cooldown.rs`) then fires OnCooldownDone, after hiding, so a
  handler can start the next cooldown (order inferred). `Clear` and a
  zero cooldown hide the widget without OnCooldownDone (inferred).
- The swipe covers the part of the turn not yet passed, clockwise from
  12 o'clock; reversed, it covers the part passed. It is drawn with the
  swipe colour (and texture, if set) over the ARTWORK layer of the
  cooldown's level, under OVERLAY (inferred).
- Defaults: swipe and bling on, edge off (inferred: Blizzard XML sets
  `drawSwipe="false"` and `drawBling="false"` but never `"true"`, although
  UI.xsd says false), swipe colour black at 0.8 alpha (inferred;
  `CooldownFrameTemplate` sets 0.64).
- `GetCooldownTimes` returns milliseconds. `Pause`, `Resume` and
  `SetPaused` freeze the swipe. Not yet: countdown numbers, the edge and
  bling textures, rotation.

### Loading XML and client answers

- A frame from an XML file that is visible after its OnLoad gets OnShow
  right after, children before parents (inferred: Blizzard's personal
  resource display is created visible, sets itself up only in OnShow, and
  Edit Mode uses that setup at every login). Frames from `CreateFrame`,
  with or without a template, do not get it.
- XML problems such as an anchor to a missing `relativeKey` (Blizzard's
  `DressUpFrames.xml` uses `$parentKey.BlackBackground`) are warnings,
  not Lua errors: the client writes them to its FrameXML log. The anchor
  is skipped (inferred).
- The client runs the `[Bootstrap]` files of every load-on-demand
  Blizzard addon at startup. They define the `*_LoadUI` functions.
- `string.format` takes positional arguments (`%2$s`); localized combat
  log strings use them.
- Undocumented client functions without an implementation return no
  values at all, not one nil: Blizzard code counts some results (the
  chat options pass `GetChannelList()` to a vararg loop).
- `GetDefaultScale()` is 768 divided by the physical screen height
  (inferred; the client may clamp it).

### Client state

The model is `crates/engine/src/lua/clientstate.lua`. A scenario turns it
on through `S.State` (`scenarios/dummy` is the example); scenarios that
replay a capture keep their captured answers. These rules are inferred
from Blizzard code and the API documentation unless a source is named:

- Events that follow a request (cursor, slots, casts, auras, the target)
  arrive on the next frame, in the order the model sent them, as with the
  glue functions. The real order for one cast is a probe question.
- Picking up a spell or an action fires ACTIONBAR_SHOWGRID; an empty
  cursor fires ACTIONBAR_HIDEGRID (`ActionBarMixin` raises the bars for
  these cursor types). CURSOR_CHANGED gets `(isDefault, newType, oldType,
  0)` with `Enum.UICursorType` values.
- `PlaceAction` puts the cursor's spell into the slot; the slot's old
  action goes on the cursor. `UseAction` with something on the cursor
  places it (`SECURE_ACTIONS.action` calls `UseAction` for a click with
  a held spell). `PickupAction` while holding something swaps the two.
- `GetCursorInfo` for a spell returns `"spell", slot, "spell", spellID,
  spellID`.
- A cast: cost paid and cooldowns started at the start; UNIT_SPELLCAST_SENT,
  then START (cast), CHANNEL_START (channel) or SUCCEEDED (instant); at
  the end STOP and SUCCEEDED. Auras from a cast spell land at SUCCEEDED,
  or at the start of a channel. `SpellStopCasting` sends INTERRUPTED and
  STOP. Times in `UnitCastingInfo` are milliseconds of `GetTime()`.
- The global cooldown is 1.5 s for every spell unless the spell says
  `gcd = 0`. A button shows the spell's own cooldown or the global one,
  whichever ends later.
- Escape runs `ToggleGameMenu`. Blizzard's handlers then stop a cast
  (`SpellStopCasting` returns true), close windows, clear the target
  (`ClearTarget` returns whether there was one), and last open the game
  menu (`Blizzard_Game/Shared/Game.lua`, `Blizzard_GameMenuEsc`).
- Aura slots from `GetAuraSlots` are the aura instance IDs; a second cast
  of a stacking aura refreshes it and adds a stack.
- Tooltips: the model answers `C_TooltipInfo.GetSpellByID`,
  `GetSpellBookItem`, `GetAction`, `GetUnit` and the aura getters.
  Spell and action lines follow the Monk capture: name (line type
  SpellName, empty grey right text); cost and range ("1,500 Mana", "40
  yd range"; without a cost the range moves left; an empty line when
  neither); cast time and cooldown ("Instant", "2.5 sec cast",
  "Channeled"; "12 sec cooldown", "15 min cooldown" from 60 s); or a
  SpellPassive line "Passive"; then the description (SpellDescription,
  gold 1, 0.82, 0, wrapped). Aura tooltips (name with the dispel type on
  the right, description in gold, time left rounded up in
  `SPELL_TIME_REMAINING_*`) and unit tooltips (UnitName with
  `unitToken`, "Level ?? Mechanical" or "Level 80 Night Elf Mage
  (Player)", the faction) are inferred: no capture has them.
- A hidden GameTooltip has no owner (inferred). Blizzard's aura buttons
  refresh the tooltip from OnUpdate while they own it; keeping the owner
  showed the tooltip again after OnLeave hid it.
- Text: `|4singular:plural;` picks the singular after the number 1
  (`SPELL_TIME_REMAINING_MIN`). It is resolved when drawn; `GetText`
  returns the escape.
- Macros and slash commands (`lua/macros.lua`): the client runs every
  macro line through the callback that `Blizzard_ChatFrameBase` gives
  `C_Macro.SetMacroExecuteLineCallback` (B: MacroExecutionManager.lua);
  it sends the line from a private edit box, so Blizzard's parser runs
  secure commands (kept in the addon's private `SecureCmdList`: `/cast`,
  `/target`, `/stopcasting`, `/logout`) and `SlashCmdList` alike.
  `RunMacroText` skips `#` lines. The sandbox console sends a slash
  command it does not handle itself (`/run`, `/dump`, `/reload`,
  `/stubs`) to the same callback when `hash_ChatTypeInfoList` (which
  Blizzard fills with every command, secure ones too) knows it.
  `ImportListToHash` moves registered commands into a hidden proxy table,
  so `pairs(SlashCmdList)` lists almost nothing; do not search it.
- `SecureCmdOptionParse`: clauses split by `;`, bracket groups OR'ed,
  conditions in a group AND'ed, `@unit` or `target=unit` sets the unit
  for `exists`, `help`, `harm`, `dead`. Also `combat`, `mod`, `button`,
  `channeling`, `known`, `group`, `pet`, `form:0`; the player is
  outdoors, unmounted and not stealthed. Unknown conditions are false,
  and true with `no` in front (inferred). The click button is always 1.
- Macros: `State.AddMacro`; account macros take indices 1 to 120,
  character macros 121 and up. A macro shows the spell named by
  `#showtooltip`, else the first `/cast` or `/use` whose options hold
  (inferred): its icon (without one of its own), cooldown, usability and
  tooltip. A macro tooltip is a NestedBlock line and then the spell's
  lines (data type Macro), as in the Monk capture. The button shows the
  macro's name (`UsesActionText`).
- `TargetUnit(name)` matches the start of a name unless the second
  argument asks for an exact match (inferred; `/target Straw`).
- `UnitCanAttack(a, b)`: described units have one `canAttack` flag; two
  different units can attack each other when either has it.
- Power comes back at 5% of the maximum every half second outside a cast.
  This is a simple rule, not the game's.
- `Blizzard_EnvironmentCleanup` removes globals from the global
  environment that the client's secure environment keeps
  (`secretunwrap`, `SecureMixin`, ...). The sandbox runs secure
  addons in the global environment, so it puts those six back.

### Maps, position and movement

`lua/maps.lua`, data from `maps.rs`. Sources: the UiMap tables (U),
WoWDBDefs layouts, Bindings_Standard.xml (B); the rest is inferred.

- UiMap (FileDataID 1957206) gives name, parent, type and flags;
  UiMapAssignment (1957219) ties a part of a map (`UiMin`..`UiMax`) to a
  world box (`Region`, min x, y, z then max x, y, z) on a continent
  (`MapID`); UiMapGroupMember (1957204) gives floors (U). The sandbox
  takes the assignment that covers most of a map and extends its box to
  the whole map. Elwynn Forest comes out at 3470.84 x 2314.62 yards, the
  size the addon community uses.
- World x points north and world y west. A map position runs left to
  right along falling world y and top to bottom along falling world x.
  `C_Map.GetWorldPosFromMapPos` returns a vector of (world x, world y);
  `UnitPosition("player")` returns world x, world y, z, continent
  (HereBeDragons reads them this way).
- The best map for a position is the smallest zone, dungeon or micro
  map that contains it (system 0), else the smallest map. The client
  also uses the area the player stands in; maps that overlap (city
  floors) may differ from the game.
- Movement: the bindings call `MoveForwardStart` and friends (B). Run
  speed 7 yards per second, backward 4.5, turning 180 degrees per
  second (inferred from the client's base speeds). Facing is radians,
  0 north, turning left adds. Forward and backward cancel; strafing adds
  a sideways part. No terrain, collision, jumping or falling.
- Events: PLAYER_STARTED_MOVING and PLAYER_STOPPED_MOVING,
  PLAYER_STARTED_TURNING and PLAYER_STOPPED_TURNING, ZONE_CHANGED when
  the best map changes, ZONE_CHANGED_NEW_AREA when the zone name
  changes. Their order relative to each other in one frame is inferred.

- World map art: UiMapXMapArt (1957217, phase 0 only) gives a map's
  art, UiMapArt (1957202) its style, UiMapArtStyleLayer (1957208) the
  layer and tile sizes and zoom range, UiMapArtTile (1957210) the tile
  textures by row and column (U). `GetMapArtLayerTextures(map, layer)`
  counts layers from 1 (Blizzard_MapCanvasDetailLayer) and returns the
  tiles row by row. Exploration overlays are not read yet: the world
  map shows the base art only.
- `UnitPositionFrame` (the player arrow) is drawn by the client. The
  sandbox draws each added unit as a texture on the frame at its map
  position, turned to the player's facing, and moves it every frame.
  `UI-WorldMapArrow` is not an atlas in the game data; the sandbox uses
  `Interface\WorldMap\WorldMapArrow` (inferred). `GetMouseOverUnits`
  returns nothing (one `nil` breaks Blizzard's tooltip code).
- Addon textures: a path under `Interface\AddOns\<name>\` that is not
  in the game data is read from the addons folder, as BLP or TGA (types
  2, 3, 10 and 11), the extension optional and the case ignored.

### Key bindings

`lua/bindings.lua`. A key runs the newest priority override binding,
then the newest other override binding, then its normal binding
(inferred from the API documentation). Commands: a Bindings_Standard
action, `CLICK Name:Button`, `SPELL`, `MACRO`, `ITEM`. Actions with
`runOnUp="true"` get `"up"` on release (B); a `CLICK` binding clicks
down and up as the button's click registrations ask. Binding changes
fire UPDATE_BINDINGS on the next frame. Bindings are not saved and not
read from the character's `bindings-cache.wtf` yet.

Modifier keys: the sandbox keeps left and right Shift, Ctrl and Alt.
Pressing one alone fires MODIFIER_STATE_CHANGED with its name and 1 or 0
(`LSHIFT`, `RCTRL`); bound key names put modifiers in the order
`ALT-CTRL-SHIFT-` (both from the API documentation and Blizzard code).
Modifier keys pressed alone do not reach OnKeyDown yet. `MakeModifiers`
returns 0: the bit values are unknown (click-bindings-cache.txt has 3
and 48 for modified spell bindings, a probe question).

### Restricted environment

Blizzard_RestrictedAddOnEnvironment runs secure handler snippets in Lua.
It needs from the client: `newproxy`, frame methods that work on a
"surrogate" table holding the frame's `[0]` userdata, `IsProtected`
(the sandbox reports `protected="true"` from XML templates; a frame is
protected when it or an ancestor is), `C_Widget.IsFrameWidget`,
`loadstring_untainted` (plain `loadstring` until taint is tracked) and
the override binding functions. With these, Clique's snippets run.

### Client state: invented data and where real data is needed

`scenarios/dummy` uses an invented character (Ilyndra, a night elf
mage), invented spells (Emberlance, Frostbind, Aegis of Dawn, ...) and an
invented training dummy (the Straw Sentinel). Only the icons come from
the game. This lets the interaction work go on without any capture.
Where the invented data stops being good enough, in order of need:

1. **Spell data for real characters** (static, no capture): SpellName,
   SpellMisc, SpellCastTimes, SpellCooldowns, SpellPower, SpellDuration,
   SpellAuraOptions from DB2. Needs the FileDataIDs from the community
   listfile and the column layouts from WoWDBDefs. Then a captured
   character's own spells can drive the same model.
2. **Tooltip line templates.** The model builds spell and action
   tooltips from the Monk capture's layout (cost, range, cast time,
   cooldown and passive lines are all in it). Aura and unit tooltips are
   inferred; a capture with a target and a buff would confirm both (the
   deferred "capture with a target").
3. **Event order for one cast** (probe question 22 below).
4. **The real training dummy**: its name, level (`??` is -1), creature
   type, classification and tooltip. A capture with a dummy targeted.
5. **Portraits**: `SetPortraitTexture` draws the unit's 3D model. The
   sandbox shows a question mark. Either the 3D renderer draws a head
   shot, or a template image per race and creature type is used.
6. **Failure messages**: which `UI_ERROR_MESSAGE` type IDs go with "Not
   yet recovered" and "Not enough mana" (`Enum.GameError` values; static
   data in the API documentation, no capture).
7. **Macros, bindings and click bindings** come from the character's WTF
   files (`macros-cache.txt`, `bindings-cache.wtf`,
   `click-bindings-cache.txt`), read only. No capture needed. The model
   can hold macros and the default click bindings already; nothing reads
   these files yet.

## Open questions for a probe session

Collect questions here. Do not record anything until there are enough to
justify one session. Each question names what the probe measures.

1. Drag threshold: how far must the mouse move after OnMouseDown before
   OnDragStart fires? (Probe: a frame registered for drag that logs
   `GetCursorPosition()` at OnMouseDown and OnDragStart, dragged slowly.)
2. Double-click time window.
3. Clamp math: final rectangle of a clamped frame with known insets after
   `SetPoint` far off screen.
4. Toplevel: frame levels of two toplevel frames before and after
   clicking the lower one.
5. `alwaysStartFromMouse`: where the frame goes when StartMoving is called
   with the cursor outside it, with and without the flag.
6. OnCooldownDone: does it fire for `SetCooldown(0, 0)`, at the end, and
   after `Clear()`?
7. OnEnter `motion` = false: does showing a frame under a still cursor
   fire OnEnter at once or on the next frame?
8. Drag release order: does OnDragStop fire before or after
   OnReceiveDrag on the frame under the mouse? Does OnReceiveDrag fire
   on the dragged frame itself when it is under the mouse?
9. Clamping and `GetPoint`: after `SetPoint` puts a clamped frame off
   screen, does `GetPoint` return the requested offsets or the clamped
   ones?
10. Anchors while moving: `GetNumPoints` and `GetPoint(1)` of a frame
    between `StartMoving` and `StopMovingOrSizing`.
11. Double click without an OnDoubleClick handler: do two quick clicks
    give two OnClick calls?
12. A second mouse button pressed while the first is held over a
    frame: which scripts fire?
13. Raise: the frame level after `Raise()` for a frame below two others
    in the same strata, and its child's level.
14. layout-local.txt: is a named, movable frame created after
    PLAYER_LOGIN (`BugSackFrame`) restored when it is created? When does
    the client restore (before VARIABLES_LOADED or PLAYER_LOGIN) and
    write the file? What does `FrameLevel` mean?
15. `StartMoving` used directly as an OnDragStart handler (BugSack) gets
    the button name as `alwaysStartFromMouse`: does the window jump to
    keep the press offset, or follow from where the drag started?
16. Can `SetFocus()` focus a disabled edit box?
17. Edit Mode: after `C_EditMode.SetActiveLayout`, does
    EDIT_MODE_LAYOUTS_UPDATED arrive, and with `reconcileLayouts`
    true or false? Which layout is active after deleting the active one?
18. Edit box: does OnTextChanged after `SetText` arrive in the call or on
    the next frame (and the order with OnTextSet);
    does `SetCursorPosition` clear the highlight; does a multi-line
    edit box with an OnEnterPressed handler also insert the new line?
19. Slider: does a press beside the thumb jump to the mouse, or page by
    `stepsPerPage`? Does `SetValue` round to the value step?
20. OnShow at load: does a frame defined in XML that is visible after its
    OnLoad get OnShow at once? Does a frame from `CreateFrame` with a
    template that has an OnShow handler get it? (Probe: an addon XML
    frame and a template that log OnLoad and OnShow.)
21. `GetDefaultScale()` at 1080 and 1440 physical pixels.
22. One cast, in order: the events (with arguments) from pressing an
    action key with a 2 s cast through its end, for an instant spell, and
    for `/stopcasting` mid-cast. Include SPELL_UPDATE_COOLDOWN,
    ACTIONBAR_UPDATE_COOLDOWN, CURRENT_SPELL_CAST_CHANGED and
    UNIT_POWER_FREQUENT, and which arrive in the same frame as the key.
23. Picking up a spell from the spellbook and dropping it on a slot: the
    events and the `GetCursorInfo()` values at each step.

## Later layers (not part of the current goal)

The client state goal in [scope.md](scope.md) ("Client state: a training
dummy session") covers the first four items below.

- **Cursor and actions.** `PickupSpell`, `PickupAction`, `PlaceAction`,
  `GetCursorInfo`, `ClearCursor`, CURSOR_CHANGED, ACTIONBAR_SLOT_CHANGED.
  Drag a spell from the spellbook to an action bar.
- **Casting.** `UseAction`, `CastSpellByID` and the secure action button
  path: UNIT_SPELLCAST_START, the cast bar, the global cooldown,
  SPELL_UPDATE_COOLDOWN, ACTIONBAR_UPDATE_COOLDOWN,
  UNIT_SPELLCAST_SUCCEEDED, power cost. Cast times, cooldowns and costs
  from DB2 (SpellMisc, SpellCastTimes, SpellCooldowns, SpellPower).
- **Targeting.** `TargetUnit` from a unit frame click,
  PLAYER_TARGET_CHANGED, a training dummy unit in scenarios.
- **Tooltips.** `C_TooltipInfo.GetSpellByID`, `GetAction`, `GetUnit`,
  `GetItemByID` returning lines built from DB2. Spell descriptions use `$`
  variables; support the common ones first.
- **Taint and protected functions**, after the VM work in the backlog.
