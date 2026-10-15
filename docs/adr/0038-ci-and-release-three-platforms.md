# ADR-0038: CI and release on Windows, Linux and macOS (Apple Silicon)

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M1
- **Supersedes:** ADR-0032 (adds the macOS runner and the aarch64-apple-darwin release;
  toolchain pinning and all Linux/Windows jobs are carried over unchanged)

## Context

ADR-0032 defined CI for Linux and Windows. macOS on Apple Silicon is now required
(ADR-0037): fmt, clippy and tests must run on an Apple Silicon runner and tagged releases
must include an `aarch64-apple-darwin` build.

## Options considered

1. **Cross-compile macOS from Linux** — needs the Apple SDK (licensing) and cannot run tests.
2. **Native GitHub-hosted Apple Silicon runner (chosen)** — `macos-15` (arm64), runs tests
   natively; costs more runner minutes (10× multiplier on private repositories).
3. Self-hosted Mac runner — not available.

## Decision

- Toolchain pinning as in ADR-0032: `rust-toolchain.toml` `channel = "1.97"` + rustfmt,
  clippy; `rust-version = "1.96"`.
- `.github/workflows/ci.yml` (push + pull_request), jobs:
  - `check` matrix `[ubuntu-24.04, windows-2025, macos-15]` (macos-15 = Apple Silicon):
    checkout → toolchain → `Swatinem/rust-cache` (key: OS + arch + lockfile) → Linux only:
    apt deps (`pkg-config libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev`) →
    `cargo fmt --all -- --check` (Linux only; formatting is platform-independent) →
    `cargo clippy --workspace --all-targets --all-features -- -D warnings` →
    `cargo test --workspace`. All three must pass.
  - `sim-isolation` (Linux): `cargo build -p cb_sim` + `scripts/check_sim_isolation.sh`
    (fails if `cargo tree -p cb_sim -e normal` contains `bevy_render|bevy_pbr|wgpu|
    bevy_winit|bevy_egui|bevy_audio`).
  - `shader-validation` runs inside `cargo test` on all three OS (if spike S6 proves it
    feasible): every WESL shader is composed via `bevy_shader::ShaderCache` and translated by
    naga to SPIR-V, HLSL and MSL without a GPU, catching backend-specific (notably Metal)
    failures early.
  - `render-smoke` (Linux, blocking): `mesa-vulkan-drivers xvfb`, `xvfb-run cargo run -p
    cb_game --release -- --smoke-test 300`.
  - `render-smoke-macos` (macos-15, `continue-on-error: true`): same command; GitHub's
    Apple Silicon VMs have limited Metal support, so it is informational. Real Apple
    Silicon GPU verification happens on the user's M4 at visual/perf milestones.
- `.github/workflows/release.yml` on tags `v*`:
  - `x86_64-unknown-linux-gnu` (ubuntu-24.04) → `city-builder-<ver>-linux-x86_64.tar.gz`
  - `x86_64-pc-windows-msvc` (windows-2025) → `city-builder-<ver>-windows-x86_64.zip`
  - `aarch64-apple-darwin` (macos-15) → `scripts/macos_bundle.sh` creates
    `CityBuilder.app` (Info.plist from `packaging/macos/Info.plist`, ad-hoc `codesign -s -`)
    → `ditto -c -k --keepParent` → `city-builder-<ver>-macos-arm64.zip`
  - built with `--profile dist`, uploaded via `softprops/action-gh-release`.
- Nightly workflow: `--ignored` soak tests and the informational headless bench on all
  three OS (bench results uploaded as artifacts).

## Consequences

- Every push is verified on all required platforms; macOS minutes are the main CI cost.
- Gatekeeper warning on first launch until notarization is set up (ADR-0037 A-P3).
