# ADR-0031: Debug tooling and inspector

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M1, extended in every milestone

## Context

Requirements: in-game inspector (e.g. bevy-inspector-egui) behind a debug feature flag;
FPS/performance overlay; sim stepping and time scaling; time-of-day and weather overrides;
procgen debug scene; debug overlays for road graph, lanes, connectors, vehicle paths,
spatial index, plot subdivision. As of 2026-10-15, bevy-inspector-egui's latest release
(0.37) targets Bevy 0.19 / bevy_egui 0.40 and is incompatible with Bevy 0.20.

## Options considered

1. **Git dependency on an unreleased bevy-inspector-egui branch** — unreproducible, may not
   exist yet, brittle.
2. **Wait** — blocks a requirement.
3. **In-house minimal inspector now, adopt bevy-inspector-egui when released (chosen)**.
4. `bevy_remote` (BRP) + external inspector — useful complement, not in-game.

## Decision

- Feature `dev` (off in release artifacts) enables: perf overlay (frame time graph, CPU set
  timings from tracing span aggregation, sim tick p50/p95, routing stats, entity/agent/
  triangle counts), sim controls (pause/step/time scale ×0.25–×16, tick budget), time-of-day
  and weather overrides (`DebugCommand`s), log console (ring buffer), gizmo overlays
  (road graph, lanes, connectors, conflict zones, vehicle paths, spatial hash cells,
  subdivision rays/lots, stamps), procgen gallery scene, screenshot capture.
- **In-house inspector**: egui window listing sim objects by kind/ID with search; shows all
  components of the selected entity via `bevy_reflect` (`ReflectComponent` →
  `PartialReflect` tree rendered read-only; numeric/bool fields of selected debug-safe
  resources editable through `DebugCommand`s so sim invariants stay enforced).
- Feature `inspector` (adds bevy-inspector-egui) is introduced by a follow-up commit as soon
  as a crates.io release supports Bevy 0.20; tracked in PROGRESS.md known issues.
- Feature `remote` enables `bevy_remote` for external tooling (optional).

## Consequences

- The debug requirement is met without waiting on the ecosystem; slightly more code.

## Amendments

- 2026-10-15 (plan review): pure-crate resources (`TrafficWorld`, `Population`, `LaneGraph`,
  `Fields`) cannot derive `Reflect`; the inspector uses a `DebugView` trait implemented in
  `cb_game::debug` (paged egui tables + lookup by ID). LaneGraph view lands in M3, TrafficWorld
  view in M9. Debug overlays and tooling are excluded from release budgets.
