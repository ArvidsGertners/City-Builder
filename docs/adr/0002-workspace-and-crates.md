# ADR-0002: Cargo workspace and crate layout

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M1

## Context

Requirements: simulation fully separated from rendering and testable headless; geometry
and traffic in their own modules/crates with thorough tests; fast iteration despite Bevy's
compile times; future options (dedicated server, mod tools, multiplayer) not blocked.

## Options considered

1. **Single crate, modules** — simplest; separation only by convention; every test
   compiles all of Bevy; slow incremental builds.
2. **Three crates** (core / sim / game) — enforced sim/render split, but geometry, traffic
   and procgen tests still compile the ECS and each other.
3. **Fine-grained workspace (10 crates)** — `cb_core`, `cb_geometry`, `cb_terrain`,
   `cb_network`, `cb_traffic`, `cb_procgen`, `cb_l10n`, `cb_audio` (pure, no Bevy), `cb_sim`
   (headless ECS), `cb_game` (binary).

## Decision

Option 3. Dependency rules (enforced by CI `cargo tree` checks):
- Pure crates depend only on `glam` (same version as `bevy_math`, currently 0.33), `serde`,
  `thiserror`, `smallvec`, and crate-specific deps from ADR-0023 — never on Bevy.
- `cb_sim` must not (transitively) depend on `bevy_render`, `bevy_pbr`, `bevy_winit`,
  `bevy_audio`, `bevy_egui`, `wgpu`.
- Only `cb_game` produces a binary. Shared settings live in `[workspace.dependencies]`,
  `[workspace.lints]`, `[profile.*]`.
- Profiles: `dev` opt-level 1 for workspace crates, 3 for dependencies; `release` with
  thin LTO, codegen-units 4 (fast enough to build in CI); `dist` profile (fat LTO, cu 1,
  strip) for tagged releases.

## Consequences

- Compiler-enforced separation; pure crates test in seconds; parallel compilation.
- More manifests and public APIs to design; cross-crate refactors touch several crates.
- Types shared between pure crates live in the lowest crate that needs them
  (e.g. cross-sections in `cb_network`, `MeshData` in `cb_geometry`).
