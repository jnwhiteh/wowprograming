# Project notes

A sandbox that runs World of Warcraft addons outside the game. Read
`docs/scope.md` first for the design and roadmap.

## Paths

- Retail client: `/mnt/d/World of Warcraft/_retail_` (Windows: `D:\World of Warcraft\_retail_`).
- CASC data: `/mnt/d/World of Warcraft/Data`.
- Blizzard UI source: `../wow-ui-source`, one branch per flavor (`live` is Retail).
  Do not change its checked-out branch. Use a worktree if another branch is needed.
- Installed addons for testing: `_retail_/Interface/AddOns`.

## Rules

- Never commit files extracted from the game (textures, DB2, fonts, models).
- Read game data only. Never write into the game install.
- Prefer loading Blizzard's own Lua/XML from the UI source over
  reimplementing it. Reimplement only what the C++ client provides.
- Code must build on Linux, macOS and Windows.
- Commit to `main` for small changes. Use feature branches for larger work.
  Never force-push.
