# City Builder

A stylized low-poly city builder in Rust and Bevy 0.20: freeform spline roads with junctions
and bridges, plots drawn along roads that subdivide into irregular lots, unique procedurally
generated buildings, citizens commuting by car, bus and tram, services and utilities,
day/night and weather. No grid, no tiles, and no image or model assets: everything is
generated at runtime.

**Status:** Phase 1 (planning) is complete and awaiting approval. There is no game code
yet. See [docs/PROGRESS.md](docs/PROGRESS.md).

## Documentation

- [CLAUDE.md](CLAUDE.md): conventions and the session protocol
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): architecture overview and links to all docs
- [docs/adr/](docs/adr/README.md): architecture decision records

## Build and run (from M1 on)

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
cargo run -p cb_game --features dev       # with debug tooling
cargo test --workspace                    # tests
```

## Controls (defaults, remappable)

| Action | Windows / Linux | macOS |
|---|---|---|
| Pan | WASD / arrows / left-drag on empty ground | same, plus two-finger scroll on a trackpad |
| Zoom | Mouse wheel | Mouse wheel / pinch |
| Rotate (free) | Middle-drag · Alt+left-drag · right-drag | Middle-drag · ⌥+left-drag · right-drag · two-finger rotate |
| Rotate (90° steps) | Q / E | Q / E |
| Pause / speeds | Space · 1 / 2 / 3 | Space · 1 / 2 / 3 |
| Undo / redo | Ctrl+Z / Ctrl+Y (or Ctrl+Shift+Z) | ⌘Z / ⌘⇧Z |
| Quick save | Ctrl+S | ⌘S |
| Road tool / plot tool / bulldoze | R / Z / B | R / Z / B |
| Road mode (straight, curved, freehand) | Tab | Tab |
| Road elevation | Page Up / Page Down or ] / [ | ] / [ |
| Delete selection | Delete | ⌫ |
| Follow selected vehicle | F | F |
| Cancel / menu | Esc (right-click cancels tools) | Esc (right-click cancels tools) |
| Screenshot | F12 | ⌘⇧2 |

Letter shortcuts follow the key label on your keyboard layout (e.g. German QWERTZ);
movement keys use physical positions.
