# ADR-0001: Engine — Bevy 0.20

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M1

## Context

The brief mandates Rust (stable, edition 2024), the latest stable Bevy and an ECS
architecture; fully 3D with procedural meshes, custom shaders, Windows + Linux, a 2019
mid-range GPU minimum. Bevy 0.20.0 was released on 2026-10-08 (verified on crates.io; MSRV
1.96, our toolchain 1.97). The ecosystem lags new releases by weeks (bevy_egui 0.43 already
supports 0.20; bevy-inspector-egui does not yet).

## Options considered

1. **Bevy 0.20** — mandated; mature ECS, pipelined renderer with GPU preprocessing and
   multi-draw batching, PBR with CSM, SSAO, contact shadows, fog, bloom, tonemapping, custom
   materials (`ExtendedMaterial`), WESL shaders, storage buffers (`ShaderBuffer`), per-
   instance `MeshTag`, `VisibilityRange` LOD crossfade, headless `MinimalPlugins`.
   Cons: breaking releases every ~3–4 months, one-week-old release.
2. **Bevy 0.19.1** — ecosystem fully caught up (inspector), but not "latest stable".
3. Fyrox / Godot-gdext / custom wgpu engine — contradict the brief or cost far more.

## Decision

Use **Bevy 0.20** (`bevy = "0.20"` in `[workspace.dependencies]`, `Cargo.lock` committed).
`cb_game` uses `default-features = false` with an explicit feature list (3D rendering,
PBR, post-processing, anti-aliasing, tonemapping LUTs, SMAA LUTs, gizmos, state, audio + wav,
x11 + wayland, multi_threaded, png for screenshots, `trace_tracy` opt-in). Not enabled:
glTF, scenes, animation, sprites, bevy_ui widgets/feathers, mesh picking, gamepad, web.
`cb_sim` depends on `bevy` with `default-features = false` and only `std`,
`async_executor`, `multi_threaded`, `bevy_state` (bevy_app/bevy_ecs/bevy_time/bevy_tasks/
bevy_math/bevy_reflect come with the core). Exact feature list is finalized in M1 by
`cargo tree` review and recorded here.

Bevy upgrades happen only between milestones and need a superseding ADR noting migration
work (WESL shader changes, API renames).

## Consequences

- We write custom shaders in **WESL** (Bevy 0.20 removed naga_oil); keep them few and small.
- Debug inspector is in-house until bevy-inspector-egui supports 0.20 (ADR-0031).
- Early-release bugs are possible; R9 in RISKS.md tracks them. We verify shadows, SSAO,
  storage buffers and egui on lavapipe in M1 and ask for a real-GPU check on Windows in M2.

## Amendments

- 2026-10-15 (plan review): `cb_sim`'s dev-dependency on `bevy` enables `debug` (needed for
  `ScheduleBuildSettings::shuffle_seed`, ADR-0043). `cb_game` additionally enables `gestures`
  (trackpad pinch/rotate, ADR-0052). bevy_egui feature selection is in ADR-0051. "No push
  constants" (ADR-0037) applies to *our* shaders; Bevy's own GPU preprocessing may use them.
