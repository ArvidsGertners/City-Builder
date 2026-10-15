# ADR-0014: Plot subdivision algorithm

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M2

## Context

Plots are freeform polygons drawn along (possibly curved) roads; they must be split into
lots of varying sizes and shapes that all touch the road, face it, and are stable under
small edits; the player sets the number of sub-plots.

## Options considered

1. **Recursive OBB splitting (CityEngine-style)** — good for city blocks, but produces
   landlocked lots and ignores curved frontage.
2. **Straight-skeleton subdivision** — great for enclosed blocks with frontage on all
   sides; complex and fragile to implement; overkill for single-road plots.
3. **Voronoi/centroidal partition with frontage seeds** — organic but irregular, can create
   non-convex slivers and weak frontage alignment.
4. **Frontage-strip subdivision (chosen)** — split points along the road-facing edge,
   inward split rays from smoothed road normals, cut by polygon booleans, repair loop.

## Decision

ALGORITHMS §10: jittered widths along the frontage (deterministic), split rays from
averaged normals blended toward the plot's principal axis, ray de-crossing, `P ∩ strip_i`
via **i_overlay** (robust integer-snapped booleans), keep frontage pieces, merge fragments,
validate (area, frontage, rectangularity) and merge failing lots iteratively, stable lot ID
matching by frontage-interval overlap. Corner plots get a wider corner lot.

## Consequences

- Every lot touches the road and naturally follows curves; robust booleans avoid
  hand-written clipping edge cases.
- Deep plots produce large backyards rather than rear lots (by design).
- Enclosed blocks are formed by multiple plots along each road rather than one block
  polygon; a block-fill helper may be added later on top of the same algorithm.
