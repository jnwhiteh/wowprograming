# wowprograming

A local sandbox for running World of Warcraft addons outside the game.

The sandbox emulates what the game client provides to addon code: the Lua
5.1 runtime, the widget API, events, the `C_*` API namespaces and
SavedVariables. It reads textures, fonts and other data from a local game
install. Addon UIs can be viewed and clicked in a web browser.

Status: early setup. See [docs/scope.md](docs/scope.md) for goals, design and
roadmap.

## Requirements

- A local World of Warcraft install (Retail first).
- A checkout of the Blizzard UI source (`wow-ui-source`) for the same flavor.
- Rust (stable, edition 2024).

No game files are stored in this repository.

## Development

Build and test with `cargo build` and `cargo test`. Run a Lua file in the
sandbox with `cargo run -- lua path/to/file.lua`. Copy
`config.example.toml` to `config.local.toml` to set local paths.

Work goes on `main` for small changes and on feature branches for larger
pieces of work. Do not force-push.
