# ADR-0029: Procedural building generation approach

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M2 (residential), M4 (all types)

## Context

Every building unique and deterministic, fitted to arbitrary lot polygons (non-rectangular,
L-shaped, angled), facing the road, adapting to slopes, with per-type rules and signature
silhouettes, level-based typologies, exposed window layout and roof shapes, good from all
sides, LOD, engine-independent `generate(lot_polygon, params, seed) -> BuildingMeshData`.

## Options considered

1. **Shape grammars (CGA-like) interpreter** — very expressive, but building an interpreter
   + rule files is a project in itself and harder to debug.
2. **Straight-skeleton roofs on arbitrary footprints** — elegant roofs for any polygon;
   implementation is notoriously fragile.
3. **Module composition in code (chosen)** — footprint = composition of oriented rectangle
   modules (pitched roofs) or the inset lot polygon (flat roofs), with typed Rust generators
   per building family; façades from window grids; details from parametric primitives.

## Decision

- `cb_procgen::building::generate(&BuildingInput, seed) -> BuildingOutput` (ALGORITHMS §11):
  setbacks → massing (rect fitting / polygon) → floors → foundations by slope → roofs →
  façades/windows (with `window_id` attributes) → details → yard → LOD1.
- Pitched roofs (gable, hip, half-hip, saltbox) only on rectangle modules; arbitrary
  polygons get flat/parapet/terrace or mono-pitch roofs. Module joins avoid coplanar
  overlaps (height offsets ≥ 2 cm, roofs intersect cleanly).
- Per-family generators for service/utility/transit/special buildings with fixed
  signature elements.
- Outputs exported for the renderer: windows (night lighting), roof patches (snow), emitter
  points (smoke, signs), door/driveway (vehicle spawn), massing (sim picking/coverage).
- The sim calls only the cheap `massing()` path (shared code) to stay consistent with the
  rendered building.

## Consequences

- Fully debuggable Rust; easy to extend with new families; deterministic.
- Roof variety on very irregular polygons is limited to flat styles (acceptable: those are
  mostly commercial/apartment lots). A straight skeleton may be added later via a new ADR.

## Amendments

- 2026-10-15 (plan review): zoned buildings are seeded from `LotId` (+ level + regrowth counter)
  as the brief requires; façade edges shorter than 2.4 m are blank walls and reflex corners get a
  column module (no window quad crosses an edge end); per-type triangle budgets from
  ART_DIRECTION §2 are asserted in tests. Signature silhouettes are checked by an LOD1 mask IoU
  test (≤ 0.6 between types). Generator sets: A (police, fire, clinic, school, diesel, coal,
  pump, outlet, landfill) in M4; set B (the rest) may land with the milestone that introduces
  the system (M6/M10).
