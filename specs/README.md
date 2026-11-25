# Interaction specs

A spec is a Lua file that drives the sandbox like a player and checks the
result. `wowprogramming test` starts the sandbox once, runs the specs in
order and exits with failure if a spec fails or the UI reports a Lua
error. It takes the same options as `run`:

```sh
cargo run --release -- test --scenario scenarios/dummy \
    --addon TomTom --addon WowLua specs/tomtom_waypoint.lua specs/wowlua_run.lua
```

A spec runs with the game's globals and these helpers (also available
as `Sandbox.Name`):

| Helper | What it does |
|---|---|
| `Wait(seconds)` | lets game time pass |
| `Click(frame [, button])` | moves the mouse to a frame and clicks it |
| `Hover(frame)` | moves the mouse to a frame |
| `Press(key)` | presses and releases a key (`"W"`, `"SHIFT-P"`, `"LSHIFT"`) |
| `Hold(key, seconds)` | holds a key while game time passes |
| `KeyDown(key)`, `KeyUp(key)` | press or release a key |
| `Type(text)` | types into the focused edit box |
| `Command(line)` | runs a console line (`"/way 42 60"`, `"/cast Frostbind"`) |
| `Reload()` | reloads the UI |
| `Equal(actual, expected [, message])` | fails unless the values are equal |
| `Near(actual, expected, tolerance [, message])` | fails unless the numbers are close |

A frame is a widget, a global name, or an expression such as
`"PartyFrame.MemberFrame1"`. Use `assert` for other checks. A failure
names the spec file and line.

The specs here use `scenarios/dummy` and the user's own addons (TomTom,
WowLua) from `addons_dir`; they are examples, not part of `cargo test`.
