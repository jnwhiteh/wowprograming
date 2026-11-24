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
   widget type, XML attribute and script handler (`ScriptsType` lists 82
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

## Coverage snapshot (2026-11-24)

Measured with a static scan: script names that the engine passes to
`fire_script`, and widget method names it registers, compared with the
schema and the documentation. The use count is how often Blizzard's Lua
and XML in the UI source set the handler or call the method. The scan has
false positives (methods built from generated names, such as the button
texture setters, look missing), so the first task of the frame runtime
goal is a precise runtime report (see `docs/scope.md`).

**Script handlers in `UI.xsd` that the engine never fires**, with
Blizzard uses: OnDragStart 118, OnKeyDown 73, OnDragStop 54,
OnReceiveDrag 52, OnEnable 51, OnDisable 51, OnHyperlinkClick 30,
OnDoubleClick 17, OnModelLoaded 16, OnHyperlinkEnter 15, OnCursorChanged
14, OnKeyUp 13, OnTooltipCleared 12, OnCooldownDone 12, OnHyperlinkLeave
9, OnScrollRangeChanged 8, OnGamePadButtonDown 6, OnAnimFinished 5, and
about 30 more with fewer than 5 uses.

**Handlers already fired:** OnLoad, OnClick, OnEnter, OnLeave, OnShow,
OnEvent, OnHide, OnUpdate, OnMouseUp, OnMouseDown, OnTextChanged,
OnEnterPressed, OnMouseWheel, OnEscapePressed, OnSizeChanged,
OnEditFocusLost, OnEditFocusGained, OnValueChanged, OnTabPressed,
OnAttributeChanged, OnVerticalScroll, OnHorizontalScroll, OnChar,
PreClick, PostClick.

**Widget methods that do nothing** (an explicit empty function in
`widgets.rs`), with Blizzard uses: RegisterForDrag 93, UnlockHighlight
70, LockHighlight 58, StopMovingOrSizing 53, StartMoving 40, HighlightText
30, SetMaxLetters 23, SetCursorPosition (edit box) 15, EnableKeyboard 13,
SetUserPlaced 12, StartSizing 10, EnableMouseWheel 9, SetClampRectInsets
9, SetClampedToScreen 7, SetResizable 6, SetHyperlinksEnabled 6. Their
getters return fixed values (`IsUserPlaced` false, `IsKeyboardEnabled`
false).

**Documented methods that are only stubs** (likely real gaps): Cooldown
`Clear` 197, `SetCooldown` 19 and the swipe setters; MessageFrame
`AddMessage` 274 (UIErrorsFrame); `DesaturateHierarchy` 25;
`SetHighlightLocked` 13; edit box `SetNumber` 22, `HasText` 8; SimpleHTML
`GetContentHeight` 13.

Totals from the scan for the main widget documentation files: 395
methods implemented, 41 empty, 263 stubs (some of them false positives).

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
  is over the button at release (W OnClick). The sandbox today sends
  OnMouseUp to whatever frame is under the cursor; this is wrong.
- **Double click.** Press, release, press, release over the button, the
  last three quickly (W OnDoubleClick). Needs the Up registration. When
  OnDoubleClick fires it replaces PreClick, OnClick and PostClick for that
  release, so rapid clicks alternate between OnClick and OnDoubleClick.
  The time window is an open question.
- **OnEnter and OnLeave** take `(self, motion)`. `motion` is true for real
  mouse movement and false otherwise, for example when a frame is shown
  under a still cursor (W OnEnter). The sandbox passes true only on mouse
  moves; it should also update the mouse focus when frames show, hide or
  move under a still cursor, with `motion` false.

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
- The distance the mouse must move before a drag starts is an open
  question.

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
- What `alwaysStartFromMouse` does is an open question.

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

  The sandbox should keep its own copy of this file in the scenario's
  saved data, never in the game install.

### Clamping

- `SetClampedToScreen(true)` keeps the frame on screen for `SetPoint`,
  moves and sizing (W SetClampedToScreen). `SetClampRectInsets` adds
  padding inside or beyond the screen edges. The exact math, including
  the sign of the insets, is an open question. A first guess: the frame's
  rectangle, grown by the insets, must lie inside the screen.

### Raising and frame levels

- A toplevel frame (`SetToplevel(true)`, XML `toplevel="true"`) raises its
  frame level when it is clicked or shown (W SetToplevel). How far it
  raises (above all frames of its strata?) is an open question.

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

### Cooldown frames

- `SetCooldown(start, duration[, modRate])`: start is a `GetTime()` value,
  0 or a duration of 0 means no cooldown. It shows the widget, and the
  widget hides itself when the animation ends unless reversed (W
  SetCooldown). OnCooldownDone presumably fires then; open question.

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
