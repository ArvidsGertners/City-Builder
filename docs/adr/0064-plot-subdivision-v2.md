# ADR-0064: Plot subdivision v2 (validated by spike S-P)

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M2 (subdivision), M4 (building fitting)
- **Supersedes:** ADR-0014 — the frontage-strip family, i_overlay booleans, the repair loop,
  stable-ID matching and the "still fits" rule are kept; the ray rule, corner handling, width
  generation, edit behaviour and the persisted state change as below.

## Context

Spike S-P (`spikes/s_p`, 10 000 random plots over six frontage families + a 24-plot showcase +
3 000 plots × 10 edit kinds, PROGRESS Phase S) ran the ADR-0014 algorithm and found:

1. **The PCA blend (β = 0.3) is harmful on curves.** Blending every ray toward one plot axis
   compresses the fan of interior lots and dumps it on the end lots (35–37° on a 150° arc), skews
   lots on corner arms, and on deep concave plots bends rays into the plot's own side edge,
   producing sliver lots that the repair loop then merged into one (85 % area loss in the
   original merge code). On straight frontage it is a no-op.
2. **Concave arcs tighter than the depth** (junction fillets, kinks) need a medial-axis seam;
   without it the strips of both arms overlap and pieces touch two frontage intervals.
3. **Fan > 25° is unavoidable** on tight curves with lots ≥ `w_min`, and lots spanning a
   25–40° kink fan by the whole kink.
4. **ID stability** fails at the edges: re-spacing from scratch on a ±1 lot change keeps only
   ~40 % of IDs; storing *post-repair* lots as the edit state makes merged (over-wide) lots
   infeasible and re-spaces neighbours; repair merges flip near the 0.55 threshold.
5. **Building fitting dominates the time** (p50 3.3 ms, p99 22 ms per plot with capsule-union
   setbacks and an 8-orientation 0.5 m raster).

## Options considered

1. **Fallbacks from the spike plan** — frontage-drag only, parallel side edges on concave
   curves, a dedicated corner-lot generator, extruded townhouse rows: each removes a symptom
   but parallel edges on curves violate the "lots follow curves" requirement and a separate
   corner generator duplicates the cut machinery.
2. **Keep the strip family, fix the rules (chosen)** — projection-style rays, seams, a shared
   constrained width solver for creation and edits, persisted split state with hysteresis, and
   a faster fit.

## Decision

**Plot polygon** (ALGORITHMS §9): one-sided offset band of the frontage (per-segment depth
quads + wedges at convex vertices, union), minus the road corridor on the outer side (auto-clip),
keeping the component that holds the frontage. Concave frontage that is not a junction fillet or
kink keeps depth ≤ 0.8 × radius.

**Subdivision** (ALGORITHMS §10, `cb_geometry::subdivide`):
1. **Rays** are the length-weighted mean inward normal over ±½ the narrower adjacent lot
   (β = 0: no PCA blend). Rays from points on a seam arc aim exactly at the arc centre.
2. **Seams**: every concave arc with radius < depth gets a seam from its centre along the
   bisector (away from the arc) to the plot boundary. Rays are truncated at the plot boundary
   and at seams; a ray ending at a seam start is sliced as one string with that seam.
3. **Widths**: jittered `w_target (1 ± 0.25)`, rescaled, clamped to `[w_min, w_max]`; then
   the **constrained solver** (Gauss–Seidel projection of split positions onto per-lot bounds,
   ≤ 400 passes, pinned splits fixed) enforces `[w_min, hi]` with
   `hi = min(w_max, max(w · 25° / turn, w_min + 2 m))` — lots on curves narrow so the fan
   (frontage turn per lot) stays ≤ 25° where whole lots ≥ `w_min` allow it. If infeasible, a
   split is added in the widest lot (≤ 4 times).
4. **Kinks** (non-corner arcs turning 25–45° over < 10 m) get a pinned split at their midpoint.
5. **Corners** (concave arc turning ≥ 45°): the corner lot spans the fillet plus
   `0.65 · w_target` on each side (pinned splits); arms are subdivided separately; the seam
   separates the arms behind the corner lot. Corner lots are exempt from the fan rule.
6. **Cut**: slice the plot by rays and seams (i_overlay, 1 mm grid); assign pieces by frontage
   contact; orphan pieces join the neighbour sharing the longest edge (< 5 m² dropped).
7. **Repair**: lots with rectangularity < 0.55, area < `a_min` or frontage < `w_min` merge with
   the narrower *adjacent* lot; merges union the raw slice pieces (never derived polygons) and
   are skipped if the union is not one polygon (never drop area).

**Persisted state and edits**: a plot stores its **pre-repair split list** (road-edge positions),
the splits its repair removed, and its lot count; lots are derived. An edit re-projects the
splits onto the new frontage, re-applies the stored merges first, then:
- same count: the constrained solver with **edit slack** ×1.1 on `w_max` and the fan cap; if the
  count no longer fits, the best ±1 change below is applied automatically;
- **±1 lot**: every insert (split a lot) / remove (merge two) / dissolve (a lot's width goes half
  to each neighbour) position is solved and the one keeping the most lot IDs wins; if no position
  is feasible the edit is rejected (the ± control is disabled);
- repair uses rectangularity ≥ 0.50 and area ≥ 0.9 `a_min` for existing lots (hysteresis).
IDs are matched by frontage-interval overlap / longer interval ≥ 0.6 (greedy), as before.

**Building fitting** (ALGORITHMS §11 steps 1–2): fit on the lot simplified to 0.1 m; setbacks by
exact half-plane clipping for convex lots, else lot minus the union of inward edge strips and
wedges at reflex vertices; largest inscribed rectangle over ≤ 4 orientations (frontage tangent +
longest inset edges) on an adaptive raster (cell ≥ 0.5 m, ≤ 40 cells across) with continuous
edge refinement; R L1–L3 need ≥ 50 m² with both sides ≥ 6 m; setbacks reduce ×0.75, ×0.5
before the lot stays green.

## Consequences

- Spike results (10 000 plots, 86 573 lots): 0 panics; 0 lots below `w_min` frontage; area error
  ≤ 0.007 %; 0 overlaps (intersections thicker than 5 mm; union re-snapping leaves ≤ 2.5 mm
  slivers); rectangularity ≥ 0.55 for 99.2 % before merging; 1.5 % merged; 98.3 % of R lots fit;
  footprints always inside the lot; fan > 25° on 0.74 % of non-corner lots (all on curves with
  `r < 1.2 w_min / 25°`); p50 0.30 ms, p99 1.63 ms per plot (subdivision + fit, container CPU).
  Edits: ≤ 1 m drags keep 99.8 % of IDs; N ± 1 meets `(N − 2)/N` on 99.5 % of feasible edits.
- The fan rule is stated as `fan ≤ max(25°, 1.2 · w_min / r)`: tight curves (R r < 28 m,
  C r < 39 m, I r < 66 m) can exceed 25° slightly.
- Merged lots may be wider than `w_max`; edits tolerate 10 % over `w_max`.
- Spike-only shortcuts that the real code must not copy: f64 + std trig (production uses
  `cb_core::math` and the determinism rules of ADR-0043), plot polygons from a generator rather
  than the plot tool.
- Revisit if the plot tool adds polygon-mode plots with frontage on two roads that are not joined
  by a fillet (two frontage chains), or if building fitting moves to async spawn time.
