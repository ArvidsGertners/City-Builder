# ADR-0040: Per-OS file locations

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M1

## Context

Saves, settings, logs and crash reports must live where each OS expects them, now including
macOS (ADR-0037). Earlier docs used a single `<data_dir>`. Writing into the app bundle or the
working directory is wrong on all platforms (bundles are read-only and signed).

## Options considered

1. **`directories`/`dirs` crate** — correct known-folder APIs, but macOS paths are derived
   from a reverse-DNS qualifier (e.g. `…/org.citybuilder.CityBuilder`) and Linux "state" dirs
   aren't covered uniformly; an extra dependency for ~60 lines of logic.
2. **Own resolver from documented environment variables and conventions (chosen)** — pure
   function, fully unit-tested for all three OS on any host.

## Decision

`cb_game::platform::paths::resolve(os, env, exe_dir) -> AppPaths` (pure; `env` is an
injectable lookup), directories created lazily on first write:

| Purpose | Windows | Linux (XDG) | macOS |
|---|---|---|---|
| Settings `settings.ron` | `%APPDATA%\CityBuilder\` | `$XDG_CONFIG_HOME/city-builder/` (default `~/.config/city-builder/`) | `~/Library/Application Support/CityBuilder/` |
| Saves + autosaves | `%APPDATA%\CityBuilder\saves\` | `$XDG_DATA_HOME/city-builder/saves/` (default `~/.local/share/…`) | `~/Library/Application Support/CityBuilder/saves/` |
| Logs | `%LOCALAPPDATA%\CityBuilder\logs\` | `$XDG_STATE_HOME/city-builder/logs/` (default `~/.local/state/…`) | `~/Library/Logs/CityBuilder/` |
| Crash reports | `%LOCALAPPDATA%\CityBuilder\crashes\` | `$XDG_STATE_HOME/city-builder/crashes/` | `~/Library/Logs/CityBuilder/crashes/` |
| Player screenshots | `%LOCALAPPDATA%\CityBuilder\screenshots\` | `$XDG_DATA_HOME/city-builder/screenshots/` | `~/Library/Application Support/CityBuilder/screenshots/` |
| Cache (reserved) | `%LOCALAPPDATA%\CityBuilder\cache\` | `$XDG_CACHE_HOME/city-builder/` | `~/Library/Caches/CityBuilder/` |

- Fallbacks: missing `%APPDATA%`/`%LOCALAPPDATA%` → `%USERPROFILE%\AppData\{Roaming,Local}`;
  relative or empty XDG variables are ignored per the XDG spec; missing `$HOME` → system temp
  dir with a logged warning and an in-game notice.
- **Portable mode**: a `portable.txt` next to the executable (on macOS: next to the `.app`
  bundle) puts everything under `<that dir>/userdata/{settings.ron,saves,logs,crashes,
  screenshots,cache}`.
- Overrides for development/tests: `CB_USERDATA=<dir>` env var (same layout as portable).
- Settings UI has "Open saves folder", "Open logs folder" (platform `open`/`explorer`/
  `xdg-open`).
- Crash reporter and logger resolve paths first in `main()` before Bevy starts; if path
  creation fails, logging falls back to stderr and the crash report goes to the temp dir.

## Consequences

- Correct, documented locations; trivial to test; no dependency.
- Windows Known Folder redirection only affects Documents/Pictures, which we don't use.
