# ADR-0007: Road representation — cubic Bézier alignment + vertical profile

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M2

## Context

Freeform roads (straight, curved, freehand) with snapping, splitting at crossings,
elevation following terrain under a max slope, offsets for lanes and edges, smooth chains,
and cheap evaluation for meshing, traffic and picking.

## Options considered

1. **Catmull-Rom / centripetal splines over clicked points** — passes through points, but
   splitting and local edits change neighbors; tangent control awkward.
2. **Clothoids / arcs + straights** (road-engineering exact) — nice offsets, but complex to
   fit freehand strokes and to split; harder intersection math.
3. **One cubic Bézier per segment in 3D** — couples elevation into the curve; terrain
   following then distorts the horizontal shape.
4. **Cubic Bézier in XZ + separate vertical profile (chosen)** — exact splitting
   (de Casteljau), easy tangent continuity, well-known intersection/projection algorithms,
   freehand fitting via Schneider; the vertical profile is solved independently like real
   road design (horizontal and vertical alignment).

## Decision

- `RoadSegment::curve: CubicBezier2` (XZ) per graph segment; long/sharp curves are split
  (≤ 120 m, ≤ 90° turning per segment).
- `RoadSegment::profile: VerticalProfile` = heights sampled every 4 m along arc length,
  evaluated with monotone cubic Hermite interpolation; solved at placement (ALGORITHMS §2)
  and **saved** (no re-derivation drift).
- Arc-length LUT per segment for `t(s)`; offsets for edges/lanes are computed as sampled
  polylines (not offset Béziers).
- Validity rules: min radius per type (curvature check), max grade, min length, min
  junction angle.

## Consequences

- Robust editing operations and simple save data.
- Offset curves are polylines (memory: ~1 sample/m/lane — acceptable).
- Clothoid transitions are not modeled; very tight S-curves rely on the min-radius rule.

## Amendments

- 2026-10-15 (plan review): long or sharp curves are split evenly into `ceil(len / 120 m)`
  pieces (never below the 6 m minimum). Existing geometry is reshaped with the
  `EditRoadGeometry` command (move node, drag control point; ADR-0055), sharing validation,
  junction regeneration and dependent re-pointing with `BuildRoad`.
