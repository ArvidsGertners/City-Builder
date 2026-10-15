# ADR-0065: Terrain v3 — road–terrain interface (validated by spike S-G)

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M2 (terrain, corridors), M3 (bridges, junctions)
- **Supersedes:** ADR-0046 — the saved base, cores + blend zones, order-independent compositing
  (`base + max fills + min cuts`), 0.15 m carve, ≥ 3.5 m shoulder, overpass bridging and immutable
  water are kept; the LOD downsampling rule changes and the bridge-end, blend and skirt rules
  below are added.

## Context

Spike S-G (`spikes/s_g`: 2049² seeded terrain with a river and a 30 m valley, 270 roads incl.
150 random, 30 junctions on 4–10 % slopes, 5 parallel pairs, 2 river + 2 valley bridges,
2 overpasses; automated sampler over 545 083 lane/edge samples at LOD0–3; 12 lavapipe shots)
found:

1. **Min-downsampling** ("min inside corridor-core cells") keeps every LOD under the road but
   drags whole coarse cells to road level: up to 13 m sags beside roads on hillsides, chunk
   cracks up to 14 m, and 24 % of road-edge samples at LOD3 more than 1.25 m below the road.
2. **Bridge ends**: a corridor that stops on the line through the span start leaves a wall where
   its side slopes end, and a road-level fill under the deck pokes through the deck.
3. **Spans fed back into themselves**: re-solving the deck without terrain attraction lifted the
   profile, and recomputing spans from it turned whole roads into viaducts.
4. **Clearance bounds** applied after grade clamping produced steps steeper than the grade, so
   overpasses were rejected as "too steep".
5. **Later roads under existing decks** put piers in their corridors; decks have no ground
   footprint, so an owner grid alone does not see them.
6. A smoothstep blend zone of width `|e| / slope` is 1.5× steeper than the nominal side slope.
7. Stamp extents taken from the centreline fill clip blend zones beside the road (terrain falls
   away sideways), leaving a step at the stamp's edge.

## Options considered

1. **Fallback from the spike plan** — deeper carve + depth-biased road mesh with 0.6 m skirts,
   conservative LOD downsampling everywhere near corridors, straight horizontal spans: hides
   the symptoms and keeps the sag.
2. **Fix the rules (chosen)** — constrained LOD decimation, explicit bridge-end geometry, a
   terrain-following span classification, grade-feasible clearance envelopes, footprint-aware
   pier placement, LOD-scaled skirts.

## Decision

- **LOD1+ terrain = constrained decimation**: plain decimation, then the smallest lowering of
  coarse vertices (least squares over the barycentric weights, Gauss–Seidel, ≤ 12 passes) so
  that at every core sample the coarse surface is ≤ core height − 5 mm. Since the carriageway's
  fine triangles are all-core (shoulder ≥ cell diagonal), every LOD stays under the road.
  Update cost: 0.09 ms per chunk for LOD1–3 together.
- **Blend zone width** `1.6 · |e| / slope` (fill 1:1.5, cut 1:1): the steepest point of the
  smoothstep stays within the nominal side slope.
- **Stamp extent** grows (×1.6, ≤ 3×) until no blend weight > 0.01 touches its bounding box.
- **Bridges**: spans are classified once from the terrain-following profile (thresholds and
  hysteresis per ALGORITHMS §3), then the deck is re-solved without terrain attraction and with
  clearance; spans are not recomputed from the deck profile. Ground runs next to a span end in a
  **soft end cap** (blend-only, target ramping from road − carve to deck underside − carve over
  the first 4 m beyond the end); the **abutment pad** core is deck underside − carve under the
  first 4 m of the deck. Deck and road share one profile (step ≤ 0.3 mm measured).
- **Profile clearance** lower bounds (water + 4 m, road + 5.5 m, + deck) are first turned into a
  grade-feasible envelope (forward/backward max with the grade step), then enforced every pass.
- **Roads under decks** need ≥ 4.5 m below the deck underside, else a validation error; piers are
  re-placed whenever a road is built under a span, avoiding every other road's footprint
  (ground corridor core or deck + 1 m), with the existing ±25 % shift / ±1 pier rule.
- **Skirts by LOD**: terrain chunk skirts 3 / 4 / 6 / 8 m (LOD0–3; measured crack p99.9
  0.6 / 1.6 / 3.3 m, max 2.5 / 3.2 / 4.7 m); road edge skirts 0.6 / 1.0 / 2.0 / 4.0 m matching
  the chunk's terrain LOD (measured terrain depth below the road edge p99.9 0.17 / 0.97 / 1.79 /
  3.62 m).
- **Two roads at different heights** whose cores are closer than `|Δh| / tan(side slope)` need a
  retaining wall the composite cannot provide: placement validation reports
  `road.too_close_height` (walls are a later feature). S-G saw 1 such sample pair in 270 roads
  (58 with the max-weight composite).

## Consequences

- Spike results: LOD0 terrain ≤ road − 0.03 m on 100 % of carriageway samples (2 of 545 083
  sidewalk-edge samples at −0.019/−0.027 m, under the skirt); LOD1/2/3 ≤ road on 100 %;
  0 cliffs outside corridors and decks (all cliffs before the fixes were bridge ends, clipped
  blends and two-road interactions); junction cut-line step 0.7 mm; abutment step 0.3 mm; 0 piers
  in corridors; the overpass leaves all 3 219 lower-core samples bitwise unchanged; removing all
  stamps restores the base bitwise; re-stamp + profile + composite + LOD0 re-mesh of a 300 m road
  p50 3.5 ms, max 10 ms (budget 25 ms async).
- The main-thread apply was measured only as a buffer-copy proxy (p50 0.32 ms, with allocation
  outliers above 1.5 ms); M2 measures the real Bevy mesh replacement against the 1.5 ms budget.
- Spike-only shortcuts that the real code must not copy: f32 + std math (production uses
  `cb_core::math` and ADR-0043 rules), Catmull-Rom centrelines instead of the ADR-0007 Béziers,
  a simplified junction surface (angular blend; ADR-0063 defines the real one), synthetic terrain.
- Revisit if terrain generation (§7) produces sharper features than the spike's (skirt depths are
  data-driven), or if retaining walls are added.
