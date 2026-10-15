# City Builder

A stylized low-poly city builder in Rust and Bevy 0.20: freeform spline roads with junctions
and bridges, plots drawn along roads that subdivide into irregular lots, unique procedurally
generated buildings, citizens commuting by car, bus and tram, services and utilities,
day/night and weather. No grid, no tiles, and no image or model assets: everything is
generated at runtime.

**Status:** M1 (project setup) is done: workspace, foundation crates, headless simulation
skeleton, game bootstrap (per-OS paths, logging, crash reports, settings, localization),
smoke test, screenshot pipeline and debug tooling. The game window shows a placeholder
scene; gameplay starts with M2. See [docs/PROGRESS.md](docs/PROGRESS.md).

## Documentation

- [CLAUDE.md](CLAUDE.md): conventions and the session protocol
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): architecture overview and links to all docs
- [docs/adr/](docs/adr/README.md): architecture decision records

## Build and run

Platforms: Windows 10/11 (x86-64), Linux (x86-64, X11/Wayland), macOS 14+ on Apple
Silicon. Requirements: Rust 1.97 (pinned via `rust-toolchain.toml`) and a GPU with DX12,
Vulkan or Metal.

```bash
# Linux
sudo apt-get install -y pkg-config libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev
# macOS
xcode-select --install
# Windows: Visual Studio 2022 Build Tools ("Desktop development with C++")
```

macOS release builds are ad-hoc signed but not notarized: on first launch, right-click
`CityBuilder.app` → Open (or `xattr -dr com.apple.quarantine CityBuilder.app`).

```bash
cargo run -p cb_game --release            # play
cargo run -p cb_game --features dev       # with debug tooling (F1 perf, F2 sim controls, F3 log, F4 inspector)
cargo test --workspace                    # tests
cargo test --workspace --features dev     # tests incl. shuffled-schedule determinism
```

Useful options (`cb_game --help` lists all): `--seed <n>`, `--sandbox`, `--language en|de`,
`--log "info,cb_sim=debug"` (or `CB_LOG`), `--resolution 1600x900`,
`--smoke-test 60` (render N frames, exit non-zero on GPU validation errors),
`--screenshot-suite M1 --out <dir>` (see `scripts/screenshot.sh`).

Files (settings, saves, logs, crash reports) go to the per-OS locations listed in
[CLAUDE.md](CLAUDE.md#debugging-help); a `portable.txt` next to the executable (macOS: next
to the `.app`) keeps everything in `./userdata/`, and `CB_USERDATA=<dir>` overrides both.
After a crash, the next start shows the report path with *Copy report* and *Open folder*.

## Controls (defaults, remappable; implemented from M2)

| Action | Windows / Linux | macOS |
|---|---|---|
| Pan | WASD / arrows · left-drag on empty ground (no tool active) · Space + left-drag | same, plus two-finger scroll on a trackpad |
| Zoom | Mouse wheel | Mouse wheel / pinch |
| Rotate (free) | Middle-drag · right-drag · Alt + left-drag | Middle-drag · right-drag · ⌥ + left-drag · two-finger rotate |
| Rotate (90° steps / continuous) | tap Q / E · hold Q / E | tap Q / E · hold Q / E |
| Pause / speeds | Space · 1 / 2 / 3 | Space · 1 / 2 / 3 |
| Undo / redo | Ctrl+Z / Ctrl+Y (or Ctrl+Shift+Z) | ⌘Z / ⌘⇧Z |
| Quick save | Ctrl+S | ⌘S |
| Road tool / plot tool / bulldoze | R / Z / B | R / Z / B |
| Road mode (straight, curved, freehand) | Tab | Tab |
| Road elevation | Alt + wheel · , / . · Page Up / Page Down | ⌥ + wheel · , / . |
| Angle snap / length snap | hold Shift / toggle L | hold ⇧ / toggle L |
| Delete selection | Delete | ⌫ |
| Follow selected vehicle | F | F |
| Cancel | Esc or right-click | Esc or right-click (two-finger click) |
| Screenshot | F12 | ⌘⇧2 |

Letter shortcuts follow the key label on your keyboard layout (e.g. German QWERTZ);
movement keys use physical positions.
