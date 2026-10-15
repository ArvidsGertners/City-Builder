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

Requirements: Rust 1.97 (pinned via `rust-toolchain.toml`) and a GPU with Vulkan, DX12 or
Metal. On Linux:

```bash
sudo apt-get install -y pkg-config libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev
```

```bash
cargo run -p cb_game --release            # play
cargo run -p cb_game --features dev       # with debug tooling
cargo test --workspace                    # tests
```

## Controls (defaults, remappable)

| Action | Input |
|---|---|
| Pan | WASD / arrow keys / left-drag on empty ground |
| Zoom | Mouse wheel |
| Rotate | Hold middle mouse and drag (free) · Q / E tap (90° steps) |
| Pause / speeds | Space · 1 / 2 / 3 |
| Undo / redo | Ctrl+Z / Ctrl+Y |
| Road tool / plot tool / bulldoze | R / Z / B |
| Road mode (straight, curved, freehand) | Tab |
| Road elevation | Page Up / Page Down |
| Follow selected vehicle | F |
| Cancel / menu | Esc |
| Screenshot | F12 |
