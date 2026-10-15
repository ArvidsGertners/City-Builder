# Key Algorithms

Status: **Accepted (Phase 1 plan)**. Each section names the crate/module that owns it and
the ADR that justifies the choice. Constants are initial values; tune in the milestone
that implements them and record changes here.

Contents
1. Spline roads · 2. Vertical profile & terrain grading · 3. Bridges · 4. Lane generation ·
5. Intersection geometry · 6. Intersection traffic control · 7. Terrain generation &
deformation · 8. Terrain materials & natural decoration · 9. Plot drawing · 10. Plot
subdivision · 11. Building fitting & generation · 12. Pathfinding · 13. Traffic flow &
congestion · 14. Network connectivity & access · 15. Demand · 16. Citizens, trips & mode
choice · 17. Service coverage & dispatch · 18. Utility distribution · 19. Land value,
pollution, noise · 20. Building leveling · 21. Economy tick · 22. Procedural generation
pipeline & seeds · 23. Cursor picking · 24. Render chunking, LOD & incremental meshing ·
25. Lighting state from time & weather · 26. Camera controller

---

## 1. Spline roads — `cb_geometry::bezier`, `cb_network::segment_geom`, `cb_game::tools::road` (ADR-0007)

**Representation.** A segment's horizontal alignment is one cubic Bézier in XZ
(`P0..P3`). Elevation is a separate vertical profile (§2). Node tangent continuity:
a segment end tangent is `P1 - P0` / `P3 - P2`; at degree-2 "continuation" nodes the tool
enforces collinear tangents so chains are G1.

**Evaluation.** Position/derivative by Bernstein form; curvature
`κ = |B'×B''| / |B'|³`. Arc-length LUT with 32–128 samples (adaptive to length),
binary search + linear interpolation for `t(s)`. Uniform resampling for meshing and lanes.

**Tool modes.**
- *Straight*: click start, click end → `P1 = P0 + d/3`, `P2 = P0 + 2d/3`.
- *Curved*: click start, click a control point `C`, click end → quadratic with control `C`,
  elevated to cubic (`P1 = P0 + 2/3(C-P0)`, `P2 = P3 + 2/3(C-P3)`). When starting from an
  existing node end, the start tangent is inherited and the control point slides along it.
- *Freehand*: record cursor samples every ≥ 2 m, smooth (Chaikin ×2), fit with Schneider's
  algorithm (max error 0.75 m, split at the worst point, enforce G1 at splits); each fitted
  piece becomes a segment with continuation nodes between them. Pieces shorter than 8 m
  are merged.
- Long curves are auto-split so no segment exceeds 120 m or 90° of turning
  (keeps offsets accurate and junction math local).

**Snapping** (priority order, within screen-space radius scaled by zoom):
1. existing node (8 m) → reuse node; 2. existing segment (6 m) → project onto curve
(Newton on `|B(t)-p|²` from 8 seeded starts), split segment at `t`; 3. angle snap of the new
tangent to 0/30/45/60/90° relative to the snapped road or to world axes when Shift is held;
4. length snap (8 m steps) with Ctrl. Snapping produces a `SnapTarget` used by preview and
command identically.

**Crossing detection.** New curve vs. indexed segments: candidate pairs from the spatial
index, then recursive Bézier clipping (bounding-box subdivision to 0.05 m) to find all
intersections `(t_new, t_old)`. For each: if `|Δh| < 2.5 m` at the crossing → split both and
create a junction node (profiles adjusted to meet at the node); if `Δh ≥ 5.5 m` →
grade-separated, no junction (overpass clearance ok); otherwise → invalid "vertical
clearance too small".

**Validity checks** (`cb_network::validation`, returns all reasons with locations):
min radius per type (`κ_max = 1/r_min`; dirt 8 m, street 12 m, avenue 25 m), max grade
(§2), min segment length 6 m, min junction angle 25° between incident roads, overlap with
non-crossing roads (corridor polygon intersection), buildings/plots in the corridor (warn;
demolish on confirm), water without bridge eligibility, map bounds, money.

**Splitting** (de Casteljau at `t`) preserves the curve exactly; profiles are split at the
corresponding arc length; style and type copied; dependents (plots, stops) are re-pointed by
arc-length position.

## 2. Vertical profile & terrain grading — `cb_network::profile`, `cb_network::grading`

**Goal.** A smooth road surface that follows terrain where possible, never exceeds the
type's max grade (dirt 14 %, street 10 %, avenue 8 %, tram 6 %), and meets fixed heights at
nodes.

**Profile solver.**
1. Sample terrain *base + existing stamps except this road's own* every 4 m along arc length
   → `g[i]`. Over water use the water surface + deck clearance as a soft target and mark
   samples as bridge candidates.
2. Endpoint constraints: node heights (existing node → fixed; new node → terrain height +
   player elevation offset).
3. Solve for `h[i]` minimizing `Σ w_i (h_i - g_i)² + λ Σ (h_{i+1} - 2h_i + h_{i-1})²`
   (smoothness) subject to `|h_{i+1} - h_i| ≤ grade_max · 4 m`. Implementation: iterate
   (≤ 30 passes) a Gauss–Seidel smoothing step followed by forward/backward grade clamping
   (max-slope propagation from fixed points). Converges quickly for n ≤ 60 samples per segment.
4. Evaluate with a monotone cubic Hermite (Fritsch–Carlson) between samples → no overshoot.
5. If endpoint constraints themselves violate max grade over the segment length → invalid
   ("too steep"), shown with the offending grade.

**Grading.** For ground spans, emit a `Corridor` stamp: the road surface within
`half_width + shoulder (1.5 m)` is set exactly to the profile height; beyond, terrain blends
from road height to original terrain along a side slope of max 1:1.5 (fill) or 1:1
(cut), with smoothstep easing so embankments look soft. Compositing is order-independent:
each sample takes the stamp with the highest `(priority, weight)` and ties resolve by stamp
ID (see §7). Cut/fill volume is integrated during stamping and feeds the cost
(`cost += volume_m3 × earthwork_cost`).

**Road surface on terrain.** Road meshes sit 0.05 m above the graded surface and include a
skirt (0.4 m down) so small LOD differences never show gaps; terrain under the road surface
is painted `dirt` in the material weights (shoulders read as gravel).

## 3. Bridges — `cb_network::bridge`, `cb_procgen::bridge_mesh`

1. After profile solve, classify samples: *bridge candidate* if over water, or if
   `h - g > fill_max` (6 m street, 8 m avenue), or if grading would push fill into water.
2. Merge candidates into spans; extend each span to the nearest stable abutment
   (≥ 4 m beyond where `h - g ≤ 1 m`) and enforce min span 12 m. Spans closer than 20 m merge.
3. On bridge spans the profile is re-solved with the deck as a free curve (no terrain term,
   only smoothness + grade + water clearance ≥ 4 m for navigation feel, ≥ 5.5 m over roads).
4. Supports: spacing by type (24–40 m) adjusted so piers are evenly spaced within the span;
   pier positions avoid other roads' corridors (shift along span ±25 %) and are placed on
   terrain or the river bed. Pier height = deck − ground; piers taller than 25 m use a
   wider tapered style.
5. Structure: deck thickness 1.2 m (street) / 1.6 m (avenue), parapet/railing both sides,
   abutment stamps (Pad) at both ends so the embankment meets the deck cleanly.
6. Cost: `length × bridge_cost_per_m(type) + piers × pier_cost(height)` (scales with length,
   GAME_DESIGN §2).
7. Validation: max span length without support 60 m (pier in deep water allowed), max deck
   height 40 m, bridges cannot start/end inside water, junctions on bridges are disallowed
   in v1 (clear error).

## 4. Lane generation — `cb_network::lanes` (ADR-0008)

1. Cross-section from the road type: ordered list from left edge to right edge, e.g. street:
   `[sidewalk 2.0, curb, car(back) 3.0, car(fwd) 3.0, curb, sidewalk 2.0]`; avenue adds
   median 2.0 (grass/tram tracks), bus lanes replace the outer car lanes when enabled, tram
   tracks replace inner lanes with `CarTramShared` or a dedicated `Tram` lane per type option.
2. Lane centerline = offset of the segment centerline by `offset_m`, computed on the trimmed
   range `t_range`: sample the centerline at ~1 m spacing on curves (2–4 m on straights,
   adaptive by curvature), offset each sample along the 2D normal, lift to the profile
   height plus cross-slope (none in v1), producing a `Polyline3` with cumulative lengths.
   Inner-side offsets at tight curves are clamped where `offset > 0.9 r` (cannot occur with
   min radius checks; asserted in tests).
3. Direction: forward lanes run along `t`, backward lanes are reversed polylines.
4. Neighbor links (`left`/`right`) for same-direction car lanes enable lane changes.
5. Speed limit = type limit × curve factor (`v ≤ sqrt(a_lat_max · r)`, `a_lat_max = 2.5 m/s²`)
   stored per lane as a piecewise-constant profile along `s` when curvature varies.
6. Lane IDs are regenerated on segment change; dependents (vehicles on the lane) are
   re-projected by `(segment, direction, index, s/length)` or rerouted.

## 5. Intersection geometry — `cb_network::junction`, `cb_network::conflicts` (ADR-0009)

Input: node position, incident segments with their curves, widths (from cross-sections)
and directions. Output: `JunctionGeometry`.

1. **Order approaches** by the angle of each segment's tangent leaving the node.
2. **Cut-back.** For each pair of angularly adjacent approaches `(A, B)`, intersect A's left
   edge curve with B's right edge curve (Bézier–Bézier clipping, edges offset from the
   untrimmed curves). The required cut distance for A is the arc length to the intersection
   plus a corner radius `r_c` (street 6 m, avenue 9 m); `cut(A) = max(cut from left
   neighbor, cut from right neighbor, min_cut = half the widest crossing width)`. Two-way
   continuation nodes with width change get a short taper instead of a junction.
   Cut-backs are capped at 45 % of the segment length; exceeding it → invalid
   ("intersection too close").
3. **Polygon.** Walk approaches in angular order: approach end cross-section (right corner →
   left corner), then a corner fillet (cubic arc between A's left corner and B's right corner
   with tangents along the edges), giving a simple polygon. Repair self-intersections with
   i_overlay (union with itself) and simplify.
4. **Surface height.** Node height; approach profiles end at node height (enforced in §2).
5. **Crosswalks and stop lines** at each approach's cut line (if sidewalks exist), set back
   1 m; crosswalks are omitted on continuation nodes.
6. **Connectors.** For each incoming lane → allowed outgoing lane (turn rules below), a
   cubic Bézier from the lane end to the target lane start with tangents along both lanes,
   handle length `0.4 × chord` (clamped to keep curvature under `1/r_min_turn` = 1/5 m);
   sampled to a `Polyline3`. U-turns only at dead ends (turning loop) and optionally at
   avenues with median gaps (v1: off).
7. **Turn rules** (`turn_rules.rs`): classify each in→out by signed angle (straight within
   ±30°, right, left). Lane assignment per approach with `n` incoming lanes: rightmost lane
   gets right turns, leftmost gets left turns, middle lanes straight; when `n = 1` all
   movements share the lane; when the target road has fewer lanes, lanes merge (each target
   lane receives at most 2 source lanes, preferring no crossing connectors). Connectors from
   the same approach never cross each other (verified by test over random junctions).
8. **Conflict zones.** For every pair of connectors from *different* approaches, intersect
   their polylines (segment–segment, with width = vehicle width 2.0 m → treat as capsules).
   Each intersection becomes a `ConflictZone` with arc-length entry/exit positions on both
   connectors. Merging connectors (same target lane) get a *merge zone* at the target lane
   start. Zones within 2 m merge.
9. **Lane-graph patch.** Removing a node's connectors/zones and inserting the new ones is
   atomic within `EditSet::Lanes`; vehicles on removed connectors snap to the nearest
   connector of the same in/out lanes or are rerouted.

## 6. Intersection traffic control — `cb_traffic::junction_ctl`, `cb_traffic::reservations` (ADR-0009)

All control types share one admission mechanism: **conflict-zone locking with
exit-space check**. A vehicle approaching a junction (within its braking distance + 10 m of
the stop line) issues a *request* for its connector each tick until granted:

```
grant(v) iff
   control_allows(v)                                    // light green / stop completed / priority gap ok
 ∧ exit_space(target lane) ≥ v.length + min_gap         // never block the box
 ∧ ∀ zone z on v.connector: z.free() ∨ z.locked_by_compatible_follower(v)
 ∧ no higher-priority request this tick claims a zone v needs
```

- Requests are processed in a deterministic order per junction per tick:
  `(priority class desc, arrival tick asc, VehicleId asc)`. Grants lock **all** zones on the
  connector at once (no partial holds) and unlock each zone as the vehicle's rear passes its
  exit position. Because a vehicle never waits *inside* a junction for a zone (it holds all it
  needs before entering) and never enters without exit space, deadlock inside a junction is
  impossible; a property test runs random arrivals on standard junctions (T, X, 5-way,
  avenue × street, roundabout chain) for 10⁵ ticks and asserts progress.
- A follower on the *same* connector may share its leader's zones (platoons), so throughput
  is not limited to one car per junction.
- **Priority (default "Auto").** Approach class = road hierarchy (avenue > street > dirt);
  ties broken by straightness (the two most opposite approaches of highest class are major).
  Minor approaches must also pass a *gap check*: no major-approach vehicle whose ETA to any
  shared zone is below the critical gap (left turn across 4.0 s, merge 3.0 s, straight
  cross 3.5 s; ×1.3 in rain/snow). Major left turns yield to opposing major straight/right.
  Equal-class junctions with no clear major road use *right-before-left* among approaches
  with waiting vehicles, with an anti-starvation rule (any request waiting > 12 s gets
  top priority class).
- **All-way stop.** Vehicles must reach `v < 0.3 m/s` within 2 m of the stop line; then the
  request enters an FCFS queue keyed by stop completion tick.
- **Traffic lights.** Phase plan auto-generated from approaches: pair opposing approaches;
  phase A = opposing straights + rights (+ permissive lefts with gap check), phase B = the
  other pair; protected left phase added when an approach has a dedicated left lane and ≥ 2
  lanes. Timing: min green 8 s, max green 30 s, yellow 3 s, all-red 1.5 s; actuated:
  a phase ends early after min green if no vehicles are waiting on it and someone waits
  elsewhere. Players can override per junction (cycle scale, phase order) in the
  intersection tool. Pedestrian phases are implicit (visual pedestrians cross on green).
- **Behavior near junctions.** IDM treats the stop line as a virtual stopped leader until
  the grant arrives; yellow → stop if able (`d_stop < distance`), otherwise proceed.
- **Gridlock outside junctions** (cycles of full lanes): see §13.

## 7. Terrain generation & deformation — `cb_terrain` (ADR-0012)

**Generation pipeline** (deterministic from `world_seed`, ~2–4 s for 2049² on 6 cores,
parallel over rows):
1. *Macro shape*: low-frequency fBm (4 octaves, 2.5 km wavelength) + a seeded "valley axis"
   field (domain-warped) so every map has a usable lowland and some hills. Height range
   0–180 m; a map-edge falloff keeps borders moderate.
2. *Hills & ridges*: ridged noise (3 octaves, 600 m) masked by macro elevation.
3. *Detail*: fBm 2 octaves at 120 m, amplitude 2–4 m (gentle undulation for low-poly
   readability; nothing below ~10 m wavelength).
4. *Rivers* (0–2): pick sources on high ground far from each other, trace steepest descent
   with momentum + meander noise to the map edge or a basin; carve a channel
   (width 18–40 m, depth 3–5 m, smooth banks over 30 m) and record the water surface along
   the path (monotonically decreasing).
5. *Lakes* (1–3): find basins with priority-flood depression filling; choose basins with
   area > 0.05 km², set lake level = spill height − 0.5 m; deepen centers slightly.
6. *Erosion-lite*: 6 iterations of thermal erosion (talus 35°) to soften cliffs; no
   hydraulic erosion (cost vs. benefit for a stylized look).
7. *Flatten buildable lowland*: blend 20 % toward a smoothed version in low-slope areas so
   cities have room.
8. *Outside connections*: 2–3 road stubs from map edges along low-slope paths toward the
   lowland, ending 150–300 m inside the map, pre-built (free), with node kind `Outside`.
9. Derived layers: materials (§8), forest mask (fBm thresholded, avoiding steep slopes and
   water), min-max quadtree.

**Height sampling** uses the render triangulation (each cell split along the same diagonal
as the mesh) so queries match the visible LOD0 surface.

**Deformation.** Final height `H(p) = composite(base(p), stamps covering p)`:
for each sample, evaluate all covering stamps (via a stamp spatial hash) → each yields
`(target_h, weight, priority)`; pick the highest `(priority, weight)`; ties → lower stamp ID;
result `H = lerp(base, target_h, weight)`. Order-independent, so undo (removing a stamp) is
exact: recompute the stamp's AABB from base + remaining stamps. Water bodies are never
modified by stamps in v1 (roads over water become bridges; buildings can't be placed on
water).

## 8. Terrain materials & natural decoration — `cb_terrain::materials`, `cb_terrain::scatter`

**Materials** per sample (u8 weights): rock by slope (`smoothstep(28°, 40°)`), sand by
height above water (`< 1.5 m` within 25 m of water), dirt by a moisture-noise band and road
shoulders/construction paint stamps, forest floor under forest mask, grass otherwise.
Season tints are applied in the shader (§25), not in the data.

**Scatter.** The map is split into internal scatter cells (32 m). Each cell gets a
deterministic RNG `(seed, Domain::Scatter, cell index)` and places candidates with
Bridson-style Poisson disk sampling (min distance by kind: trees 4–7 m in forests, 15–40 m
singles in meadows, rocks on rocky slopes, grass clumps 2 m near water/meadows). Candidates
are filtered by water, slope, forest density, and **exclusion**: any road corridor (+2 m),
plot polygon, building footprint, bridge pier. Exclusion is re-evaluated only for cells in a
dirty region, so building over trees removes them and undo restores them. Instances are
never saved.

## 9. Plot drawing — `cb_game::tools::plot`, `cb_sim::zoning::plots`

Two interaction modes producing the same `Plot` data:
- **Frontage drag** (default): hover a road edge (sidewalk outer edge polyline), click to start
  at arc position `s0`, drag along the road to `s1` (follows the curve; may continue around
  a junction corner onto the next segment, producing two frontage refs), then drag the depth
  handle (default by zone: R 30 m, C 35 m, I 50 m). The back edge is the frontage offset by
  depth, side edges follow the local road normal, blended to avoid crossing on concave
  frontages (normals averaged over a window; side edges never intersect within the depth).
- **Polygon**: click vertices; any edge within 1.5 m of a road edge snaps to it and becomes
  frontage (vertices slide along the road edge).
- **Editing**: drag vertices (frontage vertices constrained to the road edge), drag an edge
  to move it parallel, handles for depth; +/- sub-plot count; zone type change re-generates
  lots and buildings.
- **Validation**: simple polygon (no self-intersections), area 100–40 000 m², frontage ≥ 8 m
  total, no overlap with road corridors (polygon ∩ corridor area < 0.5 m²), other plots,
  service buildings, water; max mean slope 25 %. Overlapping parts can be auto-clipped
  (i_overlay difference) when the player holds Alt — the clipped result must still be valid.

## 10. Plot subdivision — `cb_geometry::subdivide` (ADR-0014)

Input: plot polygon `P` (CCW), ordered frontage polyline(s) `F` with road normals, target lot
count `N` (player) or automatic (`N = round(len(F) / w_target(zone))`), zone parameters
(`w_min, w_target, w_max, d_max`), seed.

1. **Widths.** Draw `N` widths `w_i = w_target · (1 + j_i)`, `j_i ∈ [-0.25, 0.25]`
   (deterministic RNG), rescale to `len(F)`, clamp to `[w_min, w_max]` with iterative
   redistribution; corner frontages (two roads) give the corner lot `1.3×` width.
2. **Split rays.** At each split arc position on `F`, compute the inward direction as the
   road normal averaged over ±`w_i/2` and blended toward the plot's principal depth axis
   (PCA of `P`) by `β = 0.3` to avoid fan-out on convex curves and crossing on concave curves.
   Rays are checked pairwise; if two rays intersect inside `P`, both are rotated toward their
   bisector until they don't (bounded iterations).
3. **Cut.** Lot `i` = `P ∩ Strip_i`, where `Strip_i` is the quadrilateral between ray `i`
   and ray `i+1` extended past `P`'s bounds. Boolean ops via i_overlay with 1 mm snapping.
   Multi-piece results: keep the piece touching the frontage; other pieces are merged into
   the adjacent lot that shares the longest edge (or dropped as yard if < 5 m²).
4. **Depth.** If a lot is deeper than `d_max` the rear part is kept as the lot's backyard
   (lots must touch the road; no landlocked lots). For very deep plots the tool suggests a
   lower depth.
5. **Repair & validate.** Each lot: simplify (collinear removal 0.05 m), remove spikes
   (interior angle < 15°), check area ≥ `a_min(zone)`, frontage ≥ `w_min`, rectangularity
   (area / OBB area ≥ 0.55); failing lots are merged with the narrower neighbor and the
   step repeats (terminates because N decreases).
6. **Stable lot IDs.** When re-subdividing after an edit, new lots are matched to old lots
   by frontage-interval overlap (Hungarian-free greedy by overlap ratio ≥ 0.6); matched lots
   keep their `LotId` (and building, if its footprint still fits), others are new.

Edge cases covered by tests: self-intersecting input (rejected upstream, subdivider returns
`Err`), tiny plots (N clamps to 1), sharp concave frontage, frontage around a junction
corner, polygons with collinear/duplicate vertices, slivers, N larger than possible.

## 11. Building fitting & generation — `cb_procgen::building` (ADR-0029)

`fn generate(input: &BuildingInput, seed: u64) -> BuildingOutput` (pure).
`BuildingInput { lot_polygon (lot-local frame: +X along frontage, +Z into the lot), frontage
segment, zone/type, level, condition hint, height samples (closure-free: a small grid of
terrain heights over the lot AABB), style overrides }`. `BuildingOutput { lod0: MeshData,
lod1: MeshData, massing: Footprint + heights, windows: Vec<WindowRef>, roofs: Vec<RoofPatch>,
emitters: Vec<Emitter> (chimney smoke, signs), door: Vec3, yard: YardOutput }`.

1. **Setbacks.** Inset the lot by front/side/rear setbacks (R: 3/1.5/4 m, C: 1/0/2 m,
   I: 4/2/4 m) with i_overlay offset; if empty → reduce setbacks stepwise; if still empty →
   no building (lot shown as yard/green).
2. **Massing.**
   - *Residential L1–L3*: main module = largest inscribed rectangle aligned with frontage
     (`rect_fit`: rasterize polygon at 0.5 m in the lot frame, maximal rectangle via
     histogram stack, then refine edges continuously by binary search against the true
     polygon). Optional wing (L-shape) from the remaining area if ≥ 25 m², and garage/annex
     modules. Row houses (L3) span the full frontage width with party walls.
   - *Residential L4–L5, Commercial*: footprint = inset polygon itself (follows irregular and
     angled lot shapes), simplified to ≤ 12 vertices, optionally with a stepped upper volume
     (inset 2–3 m) for taller levels; flat roofs.
   - *Industrial*: wide hall = largest rectangle; plus tanks (cylinders), silos, chimneys,
     loading docks along the frontage-side long edge.
   - *Service/utility/transit/special*: dedicated generators with fixed signature silhouettes
     parametrized by footprint size (fire station: garage doors + hose tower; police: flat
     block + antenna + blue band; clinic/hospital: white blocks + cross sign + helipad on
     hospital; school: wing + gym + yard; power plants: cooling towers / stacks / turbines /
     solar arrays; water tower; pump house at shore; sewage: round basins; landfill: terraced
     mound; incinerator: stack; depots: sawtooth hall with bays; stops: shelters).
3. **Floors & heights.** Floors from zone/level table (GAME_DESIGN §4) with ±1 jitter;
   floor height 3.0 m (R), 3.6 m (C), 6–9 m halls (I).
4. **Slopes / foundations.** From height samples over the footprint: `Δh < 0.5 m` → flatten
   pad stamp (falloff 3 m); `0.5–2.5 m` → plinth (stone base, top at max height) + pad at min;
   `2.5–5 m` (R only) → stilts/terraced base (posts or stepped platforms); `> 5 m` → no
   building. The sim emits the matching `Pad` stamp (same rule, shared pure function).
5. **Roofs.** Per rectangular module: gable, hip, half-hip, saltbox, or flat (weights per
   zone/level/style); arbitrary polygons: flat with parapet, or mono-pitch for small
   annexes. Roof patches with their normals are exported for snow accumulation; roof color
   from the palette with per-building jitter in HSV (±3° hue, ±5 % value).
6. **Façades.** Each wall quad gets a window grid: columns by `floor(width / bay_width)`
   (bay 2.4–3.6 m), rows = floors; entrance on the frontage façade near the frontage center.
   Window style per type (punched windows R, curtain-wall glass C with mullion bands, high
   strip windows I). Windows are geometry (inset quads with frame) carrying a vertex
   attribute `window_id` (per-building index + random seed) for night lighting.
7. **Details**: chimneys (R, I), AC units & antennas (C, R4+), rooftop signs (C), dormers
   (R L2), balconies (R L4+), awnings and shop signs on ground floor (C), pipes & vents (I).
   All primitives are low-poly (≤ 8-sided cylinders), flat-shaded, vertex-colored.
8. **Condition** is applied in the shader (desaturation, darkening, boarded windows mask
   via `window_id` parity, roof patches) so state changes don't regenerate meshes;
   *Ruined* swaps to a ruin generator (broken walls, debris).
9. **Yard** (`yard.rs`) fills `lot − footprints`: driveway (frontage → garage), path to door,
   lawn, garden beds, hedges/fences along side and rear edges (not frontage for C), sheds
   (R), trees (Poisson, avoiding footprints), parking stalls (C/I, rows aligned to frontage),
   crates/containers (I). Budget: ≤ 400 triangles per yard.
10. **LOD1**: massing boxes/polygons with roofs, no windows (window bands painted as
    vertex-color stripes), no details, ~10–15 % of LOD0 triangles. **LOD2** (chunk level):
    merged simplified massing per chunk.
11. **Determinism**: all choices from `Pcg32::new(derive_seed(world, Domain::Building,
    building_id, level))`; same input → bit-identical `MeshData` on the same platform
    (golden hash tests).

Vehicles (`vehicle.rs`): parametric profiles (hood/cabin/trunk lengths & heights) extruded
with a bevel; wheels as 8-sided cylinders; variants: hatchback, sedan, van, pickup, SUV;
truck (cab + box/tanker/flatbed); fire truck (ladder), ambulance, police car, garbage
truck; bus (12 m, windows, line-color band); tram (3 articulated sections). Lights are
emissive-flagged faces. Per-instance color index via `MeshTag`.

## 12. Pathfinding — `cb_traffic::router`, `cb_traffic::route_cache` (ADR-0011)

**Graph.** `LinkGraph`: one link per (segment, travel direction, mode-set), turn edges for
allowed movements through nodes (from the lane-graph connectors), so lane-level detail stays
local. ~2 links per segment → ~10–16k links for a large city. Mode views: car, truck
(no restrictions in v1), bus (prefers bus lanes), tram (tram-lane subgraph), service
(may use bus lanes).

**Cost.** `c(link) = len / v_eff + turn_penalty`, `v_eff = v_free × congestion_factor`, where
`congestion_factor` comes from the flow model / measured agent speed EMA (§13); turn
penalties: right 2 s, straight 0, left 5 s, U 20 s; road-class preference (dirt +20 %);
weather factor. **Variants**: 4 deterministic cost perturbations (±10 % per link from a
hash of `(link, variant, epoch)`); each trip picks variant `hash(trip) % 4` → route
diversity without per-trip uncacheable costs.

**Search.** A* with ALT heuristic (16 landmarks chosen by farthest-point on the free-flow
graph, recomputed on topology change in a background slice); heuristic uses free-flow
times so it stays admissible under congestion. Bounded Dijkstra for coverage and
access queries.

**Cache.** `RouteCache: LRU<(origin_link, dest_link, mode, variant), Arc<[LinkId]>>`,
capacity 32 k entries. Entries carry `(graph_version, cost_epoch)`; topology changes
invalidate entries containing removed links (reverse map link → entries); cost epochs
(every 15 game minutes) make entries *stale*: stale entries are still used, and refreshed in
the background budget (max 64 per tick) so congestion changes propagate without spikes.

**Batching & determinism.** Route requests are collected per tick in `TickSet::Trips`,
de-duplicated by key, solved in parallel against an immutable snapshot of costs, and
applied in request order. Budget 300 searches/tick; overflow waits for the next tick
(trip departs slightly later, deterministic).

**Rerouting.** A vehicle reroutes when its next link is removed, or at a junction when
its remaining route cost (current costs) exceeds the best alternative by > 30 % (checked at
most every 30 s per vehicle, budgeted).

**Pedestrians.** Walking distances use the undirected segment graph (sidewalk lengths +
crossings), bounded Dijkstra up to 800 m. Visual pedestrians follow sidewalk lane
polylines and junction crosswalks.

**Transit routing.** `TransitGraph`: nodes per (stop, line) and per stop; edges: ride
(in-vehicle time from line path length / average speed), board (wait = headway/2, capped
10 min), alight, transfer (walk between stops ≤ 300 m + 3 min penalty). Dijkstra
(≤ few thousand nodes) from the origin's accessible stops (with walk times) to the
destination's accessible stops. Results cached per (origin stop set hash, dest stop set
hash) per transit epoch.

## 13. Traffic flow & congestion — `cb_traffic` (ADR-0010)

**Agent dynamics (all agents, 20 Hz, 1D per lane).**
- IDM: `a = a_max [1 − (v/v0)^4 − (s*(v, Δv)/s)²]`, `s* = s0 + vT + vΔv / (2√(a_max b))`,
  params per vehicle kind (car: a 1.6, b 2.5, T 1.2 s, s0 2 m; truck/bus: a 1.0, T 1.6 s).
  `v0 = min(lane limit(s), curve limit, slope factor (uphill −10 %/5 % grade for trucks),
  weather factor)`.
- Leader lookup: `LaneQueue` keeps vehicles sorted by `s`; leader is the next vehicle on the
  same lane, else the last vehicle on the next connector/lane of the route within look-ahead
  (≤ 80 m), else a virtual stopped leader at the stop line if no grant.
- Integration: semi-implicit Euler with `v ≥ 0`, position clamped so no overlap
  (`s_follower ≤ s_leader − length − 0.5`): guarantees no vehicles pass through each other.
- Lane changes: *mandatory* (route needs a lane for the next turn: change within the last
  150 m, increasing urgency) and *discretionary* (MOBIL-lite: gain > 0.3 m/s², safe gap
  front/back). A change is a logical switch of lane with a 2.5 s visual lateral blend
  (presentation only). If a mandatory change fails until 15 m before the stop line the
  vehicle stops and waits (or reroutes after 20 s).
- Parallelization: lanes are partitioned into deterministic groups (by lane index ranges);
  each group steps its vehicles reading neighbors from the previous-tick snapshot;
  cross-lane interactions (lane changes, junction grants) are resolved in a sequential pass
  sorted by `VehicleId`.

**Agent admission & sampling (aggregate beyond the cap).** Every trip gets a route (cached).
A trip becomes an agent with probability `p`, adjusted by a controller to keep active agents
≈ 90 % of `agent_cap` (`p ∈ [0.05, 1]`, sampled with counter-based RNG). Non-agent trips
contribute their weight to link background volume in the flow model and complete after the
BPR travel time. At small and medium cities `p = 1`: every trip is a visible car.

**Flow model.** Per link: `V_total = EMA(agent entries/h) + EMA(background/h)` (half-life 10
game minutes), capacity `C = lanes × cap_per_lane(type) × weather factor`.
BPR: `t = t_free (1 + 0.15 (V/C)^4)`. Agents feel background traffic via
`v0_agent = v0 × t_free / t_bpr(V_bg)` and a reduced junction release rate. Congestion
metric per segment for overlays and feedback:
`congestion = max(1 − v_measured / v_free (agents, EMA), 1 − t_free / t_bpr(V_total))`.

**Feedback.** Link costs (§12), household commute estimates (EMA of actual trip times, or
BPR estimate for non-agent trips), demand (commute time term), happiness, service response
times (dispatch ETA uses current costs), land value (noise from volume).

**Traffic disabled.** No car agents; all trips are macro (p = 0). Transit and service
vehicles run in **ghost mode**: kinematic along their route at `v_eff` from the flow model,
no interactions, so lines and sirens remain visible and timing stays congestion-aware.

**Gridlock protection.** Each agent tracks `stuck_s` (v < 0.5 m/s while not stopped at a
red light / stop sign / in a stop queue with a moving head). If `stuck_s > 90 s`
(configurable), it is despawned (its trip completes as macro with a penalty) and an entry
`{vehicle, kind, lane/connector, position, reason, waiting_for}` is logged at `debug`
and collected in `GridlockLog` (shown in the debug overlay; > 20 despawns/game-hour in one
area emits a "traffic jam" notification). Cycle detection (wait-for graph via junction
requests) runs every 10 s and resolves a detected cycle by despawning the youngest
participant.

## 14. Network connectivity & access — `cb_sim::roads::connectivity`

- Components: BFS/union over nodes and segments on topology change (O(V+E), < 2 ms for 20k
  segments), for the car network (ignoring direction; one-way reachability issues are
  surfaced by routing failures) and separately for tram tracks.
- A component is *connected* if it contains an outside connection. Buildings on unconnected
  components get `Problems::NoRoad`-class "not connected" and no immigrants/goods.
- Lot/building access: `RoadAccess` = projection of the lot's frontage midpoint onto the
  frontage segment's sidewalk edge; recomputed when the segment changes. Plots whose all
  frontage segments are removed enter `AccessState::Disconnected` → buildings decline after
  3 game days unless reconnected (a new segment whose corridor runs along the old frontage
  within 2 m re-attaches automatically).
- Demolish warning: before removing segments, the command's `validate` lists dependent plots
  and stops; the UI asks *Remove road and plots* / *Remove road only* / *Cancel*.

## 15. Demand — `cb_sim::demand`

Computed every game hour, smoothed (EMA, half-life 6 game hours), range −1..1, displayed
as RCI bars:
- `R = clamp(0.45·job_vacancy_ratio + 0.25·attractiveness − 0.35·housing_vacancy_ratio
  − 0.2·tax_penalty_R + base_immigration, −1, 1)` where attractiveness combines average
  happiness, service coverage, land value, unemployment (negative), commute time (negative).
- `C = clamp(0.5·(retail_demand − retail_capacity)/retail_demand + 0.2·goods_availability
  − 0.2·tax_penalty_C + 0.15·tourism_stub, −1, 1)` with
  `retail_demand = Σ households × spending(wealth)`.
- `I = clamp(0.4·(goods_demand − production)/goods_demand + 0.35·workforce_availability
  + 0.15·export_access − 0.2·tax_penalty_I, −1, 1)`.
- `tax_penalty = max(0, (rate − 9 %) / 11 %)` (neutral up to 9 %, strong at 20 %).
- Breakdown terms are stored for the UI tooltip ("why is demand low?").
Lot development uses demand: each sim second, a budget of new constructions
`k = ceil(max(0, D) × growth_rate(population))` picks empty lots of that zone, weighted by
land value × access × utilities, deterministic order by `(weight desc, LotId)`.

## 16. Citizens, trips & mode choice — `cb_sim::citizens` (ADR-0016)

- **Immigration**: when R demand > 0 and housing is available in connected buildings,
  households arrive via outside connections (rate ∝ demand), with wealth and education drawn
  from the city's attractiveness and education coverage.
- **Job matching** (each game hour, budgeted): unemployed adults sorted by
  `(education desc, CitizenId)` are matched to vacancies by best score
  `= −commute_estimate − 0.2·level_mismatch`, using a precomputed per-workplace list and a
  coarse travel-time estimate (straight distance × network detour factor, refined to the
  actual route on the first commute). Over-educated workers accept lower jobs at a happiness
  penalty.
- **Schedule**: each worker has a shift (start ~ N(8:00, 50 min) for C/office-like,
  N(7:00, 40) for I, 15 % evening shifts for C, 10 % night for I); students 8:00–14:00;
  shopping trips with probability per household per day (0.6 × wealth factor) at ~ N(17:30,
  90 min); leisure to parks on weekends-equivalent (every 4th day). Activities are inserted
  into the `TimeWheel` with departure times computed backward from the desired arrival
  using the commute estimate, which produces morning and evening peaks.
- **Trip generation**: due activities become `TripRequest`s. Mode choice (multinomial
  logit): `U_car = −(t_car + 3 min parking-proxy)·β_t − cost_car`,
  `U_transit = −(walk·2 + wait·1.5 + ride + transfers·5 min)·β_t − fare`,
  `U_walk` if distance < 800 m. Car availability per household (`cars`). The chosen mode is
  sampled with counter-based RNG.
- **Arrival** updates citizen state and the commute estimate (EMA).
- **Happiness** (daily, sliced): weighted sum of factors (housing level, employment,
  utilities served, garbage, services coverage, commute time, pollution, noise, crime, taxes,
  land value, parks, weather ±3 %) → 0..100. Persistent unhappiness (< 35 for 3 days) → move
  out probability; abandoned buildings evict.
- **Education**: students progress one level per school year (= 1 game season) if their
  school has capacity; adults may enroll in university when coverage exists.
- **Ageing/lifecycle**: per game year age groups advance; seniors retire; households with
  children form new households when children become adults (if housing exists) — kept
  lightweight; no individual births/deaths simulation beyond counts.

## 17. Service coverage & dispatch — `cb_sim::services` (ADR-0027)

- **Coverage** is measured along the road network: each service building runs a bounded
  multi-target Dijkstra on the *undirected* segment graph from its access point up to
  `reach_m` (travel distance; e.g. police 900 m, fire 1200 m, clinic 800 m). For every
  reached segment, coverage samples (up to 8 along its length) store
  `c = budget_factor × capacity_factor × falloff(d / reach)` with
  `falloff(x) = 1 − smoothstep(0.6, 1.0, x)`. Multiple emitters combine with `max` for
  display and with a capacity-aware sum (saturating) for gameplay.
- Buildings sample coverage at their `RoadAccess` (segment, t) → `ServiceExposure`.
- **Capacity**: each facility serves up to `capacity` buildings/citizens; if demand in its
  reach exceeds capacity, effective coverage scales by `capacity / demand`.
- **Incremental**: emitters store the set of segments reached; any dirty segment in that set
  (or adjacent to its frontier) triggers recomputation for that emitter only.
- **Response times**: dispatch ETA uses the *directed* link graph with current costs,
  so congestion and one-ways matter for vehicles.
- **Incidents**: fire risk per building per hour `= base(type, level) × (1 − 0.8·fire_cov)
  × weather_factor` (+ lightning strikes during storms targeting tall buildings, weighted by
  height); crime events from `crime = f(1 − police_cov, unemployment, land value)`; medical
  events from sickness (pollution, health coverage). Garbage overflow when stored garbage >
  threshold.
- **Dispatch** (1 Hz): for each open incident, the nearest facility (by ETA) with a free
  vehicle sends one; the vehicle drives as an agent (or ghost), resolves on arrival over a
  service time; fires grow (damage += intensity·dt) and spread to buildings within 6 m with
  probability; unresolved fire at damage ≥ 1 → `Ruined`. Garbage trucks: tour building
  (nearest-neighbor over buildings with garbage > threshold within reach, capacity 10 t),
  return to facility.

## 18. Utility distribution — `cb_sim::utilities` (ADR-0026)

- Distribution is **implicit along roads**: every road carries power cables, water and
  sewage pipes. Networks are the road connectivity components (§14); utility buildings and
  consumers attach via `RoadAccess`.
- **Power** (hourly): per component, `supply = Σ plant output × availability(weather/time)`
  (wind: wind speed curve; solar: sun elevation × cloudiness; coal/gas: constant × budget);
  `demand = Σ building kW` (heating multiplier in winter). If `supply ≥ demand` everyone is
  served; else allocate deterministically: priority (services & utilities first), then by
  network distance from the nearest plant ascending (outer areas brown out first), ties by
  BuildingId.
- **Water**: pumps need shoreline placement (surface water within 15 m with depth ≥ 1.5 m)
  or groundwater (water tower/well, lower capacity, anywhere). Same allocation scheme.
- **Sewage**: treatment plants and outlets need shoreline; outlet pollutes water (water
  pollution field downstream) — treatment plants reduce it by `treatment`. Same allocation.
- **Garbage**: buildings accumulate `garbage_kg_day`; collected by trucks (agents) or, when
  traffic is disabled, analytically per hour (`collected = min(stored, facility capacity
  share within reach)`); landfills fill and must be emptied/replaced; incinerators produce
  power and pollution.
- Status per building (`UtilityStatus`) drives problem icons and happiness; changes emit
  `BuildingChanged` only when status flips (hysteresis 10 %).

## 19. Land value, pollution, noise — `cb_sim::fields` (ADR-0028)

- Fields are 512 × 512 rasters (8 m) with bilinear sampling.
- **Ground pollution**: sources (industry by output, coal plants, landfills, incinerators,
  sewage outlets) splat Gaussian kernels; per game hour `P ← 0.97·P + Σ sources` with a
  3×3 blur step (diffusion); trees within 30 m reduce (absorption term).
- **Noise**: recomputed from sources each game hour (not accumulated): segment traffic
  volume (line source along segment, `L = 10·log10(V) + type`), industry, construction sites,
  commercial at night; attenuated by distance (inverse-square in dB) and slightly by trees.
- **Land value** (game hourly, smoothed with half-life 1 day):
  `LV = base(terrain: water view +, slope −) + Σ service coverage terms + parks/decorations
  (tree rows, grass strips within 40 m) + transit stop proximity (network ≤ 400 m) +
  road type (avenue +, dirt −) − pollution − noise − crime − abandoned neighbors`. Stored
  per raster cell, sampled per lot.
- Fields are updated in slices (64 rows per sim second) to avoid spikes.

## 20. Building leveling — `cb_sim::buildings::leveling`

Evaluated daily (sliced across the day) per building:
`score = w_lv·LV + w_srv·services + w_util·utilities_ok + w_hap·happiness_or_profit +
w_acc·accessibility(road type, transit, commute)`, each normalized 0..1.
- Level up from L to L+1 if `score ≥ up_threshold[L]` for 3 consecutive days, all utilities
  served, level unlocked by progression, and lot area ≥ `min_area[zone][L+1]`.
- Level down if `score < down_threshold[L]` (hysteresis: `down = up − 0.15`) for 5 days.
- Decline → abandonment: missing road access, power or water for 3 days, or happiness < 20
  for 5 days → `Declining` (visual run-down) → `Abandoned` after 4 more days (residents leave,
  jobs removed). Conditions restored → recover to `Active` over 2 days.
- Level change triggers regeneration with a new seed component (`level` in the seed) and
  the construction animation; capacity changes are applied when construction completes.
- `blocked_by` flags explain in the info panel why a building doesn't level.

## 21. Economy tick — `cb_sim::economy`

- **Hourly accrual** (`HourElapsed`): taxes `Σ households income(wealth, level) × rate_R
  + Σ businesses profit(level, customers/production) × rate_C/I` / 24; upkeep of roads
  (`m × upkeep/m × (1 + snow_factor)`), buildings (service/utility upkeep × budget factor),
  transit operating cost per vehicle-hour, minus fares collected; loan interest. All in
  `Money` cents with explicit rounding (banker's rounding per transaction, residual carried).
- **Daily statement** (`DayElapsed`): ledger snapshot by category for the budget panel
  graphs; budget warnings when projected balance < 0 in 3 days; bankruptcy rule: negative
  balance for 7 days → forced emergency loan (if available) else game-over warning dialog
  (sandbox: never).
- **Construction costs** are charged when the command applies (roads: length × type cost ×
  slope factor `1 + 2·grade` + earthwork + bridges + decorations); refunds on undo = recorded
  cost; demolish refunds 25 % of current value for roads/buildings < 1 day old, otherwise 0.
- **Loans**: tiers unlocked by population; fixed interest per day; repay anytime.

## 22. Procedural generation pipeline & seeds — ADR-0017

- `derive_seed(world_seed, domain, id, extra) = mix64(world_seed ⊕ mix64(domain) ⊕
  mix64(id) ⊕ mix64(extra))` with a stable 64-bit mixer (SplitMix64 finalizer). Domains:
  Terrain, River, Lake, Scatter, Plot, Lot, Building, Yard, Vehicle, Citizen, Name, Weather,
  Audio, Event.
- New game: terrain (§7) → outside connections → scatter (lazy per chunk) → initial money and
  unlocks → backdrop stats.
- World content that depends on time (weather days, incidents) uses
  `SimRng::for_event(stream, tick_or_day, id)` (counter-based, no stored state).
- Meshes are pure functions of saved data + seeds and are cached by
  `(kind, id, generation)` in memory only.

## 23. Cursor picking — `cb_terrain::raycast`, `cb_game::picking` (ADR-0034)

1. Cursor → camera ray (Bevy `Camera::viewport_to_world`).
2. **Terrain hit**: march the ray over the min-max quadtree (levels 2 m → 1 km); skip nodes
   whose max height is below the ray segment; at leaf cells intersect the two triangles
   exactly (same split as rendering). Also intersect water surfaces (flat per body). Returns
   the nearest hit in < 20 µs typical.
3. **Objects**: buildings — 3D DDA over spatial-hash cells along the ray up to the terrain
   hit, test footprint prisms (massing) → nearest; roads — query segments near the ground hit
   (6 m), project to curves, pick the closest within its half-width; plots/lots — point in
   polygon at the ground hit; vehicles — ray vs. oriented boxes of *visible* agents (from the
   view pool); stops — small radius around ground hit.
4. Priority depends on the active tool (road tool prefers nodes/segments; inspect prefers
   vehicles > buildings > roads > plots).

## 24. Render chunking, LOD & incremental meshing — `cb_game::render` (ADR-0003)

- Render chunks: 128 m squares (32 × 32 over the map) — an internal render partition only.
- **Terrain**: one entity per chunk per LOD (65², 33², 17², 9² vertices; skirts 2 m),
  `VisibilityRange` with dithered crossfade (LOD0 < 350 m, LOD1 < 900 m, LOD2 < 2 km, LOD3
  beyond; scaled by quality preset).
- **Buildings & yards**: generated per building on the async pool, merged per chunk into one
  mesh per material class (opaque world, glass/window). Each vertex carries `building_slot`
  (u16 index into the chunk's slot table) → per-building params in a `ShaderBuffer`
  (`condition, construction_progress, highlight, overlay_value, window_seed, snow_mask_scale`).
  Re-merge is debounced (250 ms) and only for dirty chunks; state-only changes update the
  buffer. LOD1 merged mesh per chunk; LOD2 is one merged massing mesh per 4 chunks.
- **Roads**: one mesh per segment and per junction (surface + markings via vertex
  attributes + curbs/sidewalks), grouped per chunk; decorations (trees, lamps) as instanced
  meshes (shared mesh + material, automatic GPU instancing).
- **Props/trees**: merged per chunk per LOD (trees have 3 LODs: full, simplified, billboard
  cross-quads at distance), wind sway in the vertex shader.
- **Vehicles**: shared mesh per variant, instanced, `MeshTag` = color index.
- Mesh jobs carry `(key, generation)`; stale results are dropped.

## 25. Lighting state from time & weather — `cb_game::lighting` (ADR-0004, ADR-0018)

- Sun direction from time of day + season (declination ±23°, fixed latitude 48°N);
  moon opposite with phase cycle per season.
- `LightingState = blend(keyframes(time_of_day), weather_modifier(kind, intensity),
  season_modifier)`; keyframes and modifiers are data from ART_DIRECTION.md. Smoothly
  interpolated (cubic in time; weather transitions over 20–40 game minutes).
- Outputs per frame: directional light (direction, color, illuminance, shadow on/off — the
  same light switches from sun to moon below −4° elevation with an intensity crossfade),
  ambient + procedural environment cubemap (regenerated when the sky colors changed by
  ΔE > 1.5, on the async pool, 6 × 64² source → `GeneratedEnvironmentMapLight`), fog
  color/density, sky shader uniforms (zenith, horizon, sun/moon disks, stars, cloud
  coverage), exposure, bloom intensity, night factor (windows, lamps, headlights), wetness,
  snow cover, cloud shadow parameters, season tints.
- Night readability: exposure is raised at night so the scene renders as a blue-tinted
  "day-for-night" palette (ART_DIRECTION §3), minimum ambient enforced.

## 26. Camera controller — `cb_game::camera`

- State: `focus: Vec2` (XZ), `focus_h` (smoothed terrain height at focus), `yaw`,
  `distance` (zoom, clamped 25 m – 2 400 m), `pitch = pitch_curve(distance)` (≈ 35° close →
  65° far; MMB vertical drag adds an offset ±15°), plus *target* values; the rendered camera
  follows targets with critically damped springs (configurable stiffness; zoom and rotation
  separate).
- Pan: WASD/arrows move the target focus along the yaw-rotated basis, speed ∝ distance;
  left-drag on empty ground pans by keeping the grabbed ground point under the cursor
  (re-raycast against the plane at the grab height each frame). Edge scrolling optional.
- Zoom: wheel scales distance exponentially toward the cursor's ground point (focus shifts
  proportionally), clamped.
- Rotate: MMB drag changes yaw freely (and pitch offset); Q/E taps set the target yaw to the
  next multiple of 90° (relative to the current yaw); holding Q/E does nothing extra.
- Terrain following: `focus_h` tracks `max(terrain, water)` at the focus with smoothing;
  collision guard keeps the camera ≥ 6 m above terrain along its position (pitch raised if
  needed). Focus is clamped to the map bounds minus 50 m.
- Follow mode: clicking a vehicle/tram sets `follow: VehicleId`; focus target = its
  interpolated position (+ look-ahead along heading); any pan input or Esc ends follow; the
  agent despawning ends follow gracefully.
- Speeds, smoothing and inversion are settings; all bindings come from the `InputMap`.
