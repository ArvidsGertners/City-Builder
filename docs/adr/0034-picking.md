# ADR-0034: Cursor picking

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M2

## Context

Picking on terrain for every tool every frame, plus selection of buildings, roads, plots,
vehicles and stops. Buildings/props are merged per chunk (ADR-0003), so mesh raycasting via
Bevy's mesh picking cannot identify individual buildings and would be slow on large meshes.

## Decision

- CPU picking in `cb_game::picking` using sim data: ray from `Camera::viewport_to_world`;
  terrain hit by min-max quadtree ray march with exact triangle tests (same triangulation as
  rendering); water planes; objects via spatial indices (building massing prisms with 3D
  DDA over cells, road curve projection near the ground hit, point-in-polygon for plots/lots,
  OBB tests for visible vehicles, stop radii). Priority by active tool (ALGORITHMS §23).
- Result cached per frame in `CursorHit` resource; tools and UI read it.
- Bevy's `bevy_picking` is not enabled for world objects (egui handles UI hit testing).

## Consequences

- Fast (< 0.1 ms), precise, independent of render LOD/merging.
