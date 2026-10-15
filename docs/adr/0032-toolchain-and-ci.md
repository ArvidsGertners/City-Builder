# ADR-0032: Toolchain pinning and CI

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M1

## Context

`cargo fmt --check`, `cargo clippy -- -D warnings` and `cargo test` on every push and PR for
Windows and Linux, dependency caching, release artifacts on tags. Floating "stable" can
introduce new clippy lints that break `-D warnings` builds unexpectedly.

## Decision

- `rust-toolchain.toml`: `channel = "1.97"`, components `rustfmt`, `clippy`; edition 2024;
  workspace `rust-version = "1.96"` (Bevy 0.20 MSRV). Toolchain bumps are deliberate commits
  that fix new lints in the same change.
- `.github/workflows/ci.yml` on push + pull_request:
  - matrix `ubuntu-latest`, `windows-latest`: checkout → toolchain → `Swatinem/rust-cache`
    (key per OS + lockfile) → Linux system deps (`libasound2-dev libudev-dev libwayland-dev
    libxkbcommon-dev pkg-config`) → `cargo fmt --all -- --check` (Linux only) →
    `cargo clippy --workspace --all-targets --all-features -- -D warnings` →
    `cargo test --workspace`.
  - `sim-isolation` job: `cargo build -p cb_sim` and a script asserting `cargo tree -p cb_sim`
    contains no `bevy_render|bevy_pbr|wgpu|bevy_winit|bevy_egui|bevy_audio`.
  - `render-smoke` job (Linux): install `mesa-vulkan-drivers xvfb`, run
    `xvfb-run cargo run -p cb_game --release -- --smoke-test 300`.
  - `CARGO_INCREMENTAL=0`, `RUSTFLAGS="-D warnings"` only for clippy step via args (not
    global, to keep cache effective).
- `.github/workflows/release.yml` on tags `v*`: build `--profile dist` on Linux and Windows,
  package binary + README + LICENSE (once chosen) into `.tar.gz`/`.zip`, upload to a GitHub
  Release.
- A nightly workflow runs `--ignored` soak tests and the informational bench.

## Consequences

- Reproducible lint/format behavior; fast CI via caching; renders tested at least on lavapipe.
