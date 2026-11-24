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
