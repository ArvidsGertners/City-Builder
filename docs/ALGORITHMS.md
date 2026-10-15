# Key Algorithms

Status: **Accepted (Phase 1 plan, revised after the 2026-10-15 plan review)**. Each section
names the owning crate/module and ADR. Numbers are initial values; each constant with gameplay
or stability impact has an owning test and target in [CONSTANTS.md](CONSTANTS.md). Section
numbers are stable (other docs reference them).

**Conventions (ADR-0042, ADR-0043).** Sim time in sim seconds (`dt = 0.1 s`); 1 game minute =
2 sim s. Flows and capacities in veh/**sim-hour**. Economy/demand/incidents per **game**
hour/day. Transcendental math via `cb_core::math` (libm); parallel work over
`SIM_PARTITIONS = 64` fixed partitions; event RNG `SimRng::for_event(stream, purpose, t, id)`.

Contents
1. Spline roads · 2. Vertical profile & terrain grading · 3. Bridges · 4. Lane generation ·
5. Intersection geometry · 6. Intersection traffic control · 7. Terrain generation &
deformation · 8. Terrain materials & natural decoration · 9. Plot drawing · 10. Plot
subdivision · 11. Building fitting & generation · 12. Pathfinding · 13. Traffic flow &
congestion · 14. Network connectivity & access · 15. Demand · 16. Citizens, trips & mode choice
· 17. Service coverage & dispatch · 18. Utility distribution · 19. Land value, pollution,
noise · 20. Building leveling · 21. Economy tick · 22. Procedural generation pipeline & seeds ·
23. Cursor picking · 24. Render chunking, LOD & incremental meshing · 25. Lighting state ·
26. Camera controller · 27. Goods economy

---

## 1. Spline roads — `cb_geometry::bezier`, `cb_network::segment_geom`, `cb_game::tools::road` (ADR-0007)

**Representation.** Each segment's horizontal alignment is one cubic Bézier in XZ; elevation
is a separate vertical profile (§2). Continuation (degree-2) nodes enforce collinear tangents
(G1 chains).

**Evaluation.** Bernstein form; curvature `κ = |B'×B''| / |B'|³`; arc-length LUT (32–128
samples by length) with binary search + linear interpolation for `t(s)`.

**Tool modes.**
- *Straight*: click start, click end (`P1 = P0 + d/3`, `P2 = P0 + 2d/3`).
- *Curved*: start, control point `C`, end → quadratic elevated to cubic; when starting from a
  node end, the start tangent is inherited and `C` slides along it.
- *Freehand*: samples every ≥ 2 m while dragging, Chaikin ×2, Schneider fitting (max error
  0.75 m, G1 at splits); pieces become segments with continuation nodes.
- *Roundabout preset*: click center, drag radius (15–60 m) → ring of 4 one-way arcs; entries
  are created where existing roads cross the ring (ADR-0045 ring detection applies).
- Long curves are split **evenly** into `ceil(len / 120 m)` pieces, also if turning > 90°;
  never below 6 m.

**Snapping** (priority order; radius `r = max(r_m, 14 logical px × meters_per_px at the
cursor)`): (1) node (r_m 8 m; elevated nodes by 3D ray–point distance), (2) segment (6 m;
Newton projection from 8 seeds; elevated decks via ray–ribbon, §23), (3) angle snap with
**Shift** (0/30/45/60/90° relative to the snapped road or world axes), (4) length snap
(8 m steps) when the **length-snap toggle** is on (`L`, tool chip). A `SnapTarget` is shared by
preview and command.

**Crossings** (fixed-point, ≤ 2 iterations): (1) solve the new road's profile without
crossings; (2) find intersections with indexed segments (Bézier clipping to 0.05 m) and
classify by Δh at the crossing: `< 2.5 m` → junction, `≥ 5.5 m` → grade-separated (bridge span
per §3), otherwise invalid; (3) for junctions, add a fixed node height (mean of both roads,
clamped to each road's grade limit) and re-solve both profiles; (4) re-classify; still
changing → invalid `road.crossing_height_unstable`. The preview shows Δh and offers quick
fixes *Make junction* (flatten) / *Make overpass* (raise new road by the missing clearance).

**Validity** (`cb_network::validation`, all reasons with locations): min radius per type
(dirt 8 m, street 12 m, avenue 25 m, tram 20 m), max grade (§2), min segment length 6 m, min
junction angle 25°, minimum node spacing per type pair (GAME_DESIGN §3), overlap with
non-crossing corridors, water without bridge eligibility, map bounds, money, unlocks.

**Effects on plots and buildings.** The new corridor and any new junction polygon are
subtracted from overlapping plots (i_overlay difference); affected plots re-validate and
re-subdivide; plots that become invalid and buildings whose footprint intersects the corridor
are listed in the preview and removed on confirm (refund rules ADR-0055).

**Editing** (`EditRoadGeometry`, ADR-0055): drag a node (all incident segments re-solve) or a
segment control point; same validation, junction regeneration and dependent re-pointing as
building. **Upgrade/downgrade** replaces the cross-section; widening subtracts the wider
corridor from plots exactly as above (preview prices removed buildings); narrowing leaves a gap
and frontages re-attach by the 2 m rule (§14).

**Splitting** (de Casteljau at `t`) is exact; profiles split at the matching arc length;
style/type copied; dependents re-pointed by arc-length position.

## 2. Vertical profile & terrain grading — `cb_network::profile`, `cb_network::grading` (ADR-0046)

**Profile solver.** Samples every 4 m along arc length of terrain *base + all stamps except
this road's own* → `g[i]`; endpoint heights fixed (existing node) or terrain + player
elevation offset (new node). Minimize `Σ w_i (h_i − g_i)² + λ Σ (h_{i+1} − 2h_i + h_{i−1})²`
subject to `|h_{i+1} − h_i| ≤ grade_max × 4 m` and the **landing constraint** (grade ≤ 4 % on
`[0, cut_back + 4 m]` from each junction node, ADR-0045). Iteration: Gauss–Seidel smoothing pass
+ forward/backward grade clamping from fixed points; stop when `max |Δh| < 0.01 m` or after 30
passes; if any step still exceeds `grade × 4 m + 1 mm` → invalid "too steep at s = …"
(unit tests with infeasible endpoints). Evaluation: monotone cubic Hermite (Fritsch–Carlson).
Max grades: dirt 14 %, street 10 %, avenue 8 %, tram 6 %.

**Grading.** Ground spans emit a `Corridor` stamp: **core** = carriageway + shoulder (flat
margin ≥ 3.5 m beyond the edge), target = road surface − 0.15 m (carve); **blend zone** to the
original terrain along side slopes ≤ 1:1.5 (fill) / 1:1 (cut) with smoothstep weight.
Compositing per ADR-0046 (cores override; elsewhere max-fill/min-cut). Earthwork volume is
integrated during stamping (`cost += m³ × 1.5`). Road meshes sit on the profile height with
0.6 m skirts; the shoulder area is painted `dirt` in material weights.

## 3. Bridges — `cb_network::bridge`, `cb_procgen::bridge_mesh`

1. After the profile solve, mark bridge candidates: over water; fill `h − g` > 6 m (street) /
   8 m (avenue); fill reaching water; **overlap with another road corridor with Δh ≥ 4 m**
   (overpass; min span = crossed width + 2 × 6 m).
2. Hysteresis for stable previews: enter at the threshold, exit at threshold − 1 m. Merge
   candidates into spans, extend to stable abutments (≥ 4 m beyond `h − g ≤ 1 m`), min span 12 m,
   merge spans closer than 20 m.
3. Re-solve the deck profile on spans (smoothness + grade only, clearance ≥ 4 m over water,
   ≥ 5.5 m over roads).
4. Piers: spacing 24–40 m by type, evenly distributed; a pier inside another corridor or a pier
   exclusion zone shifts along the span in 5 % steps up to ±25 %, then one pier is added or
   removed (keeping max unsupported span 60 m), else invalid `bridge.pier_conflict`. Pier
   height = deck − ground; > 25 m uses the tapered style.
5. Deck 1.2 m (street) / 1.6 m (avenue), railings both sides, **abutment** stamps (core = pad at
   deck height) so embankment, abutment and deck meet (step ≤ 2 cm, spike S-G).
6. Cost: `length × bridge_cost_per_m + piers × (1 200 + 40 × height_m)`.
7. Validation: no start/end inside water, max deck height 40 m, no junctions on bridges (v1).
8. Bridge decks are pickable (§23).

## 4. Lane generation — `cb_network::lanes` (ADR-0008)

1. Cross-section from the road type: ordered elements (sidewalk, curb, car lanes with
   direction, median, bus lane, tram track, verge). Variants with bus lanes / tram tracks exist
   from M3 (UI hidden until M10).
2. Lane centerline = offset of the trimmed centerline sampled adaptively (~1 m on curves,
   2–4 m straight) along the 2D normal, lifted to the profile height → `Polyline3` with
   cumulative length; inner offsets are guaranteed < 0.9 r by the min-radius rule (asserted).
3. Forward lanes follow `t`; backward lanes are reversed.
4. Same-direction neighbor links for lane changes.
5. Speed profile: type limit × curve cap `v ≤ sqrt(2.5 m/s² × r)` (r = 25 m → 7.9 m/s).
6. Lanes are regenerated on segment change; agents re-project by `(segment, dir, index, s/len)`
   or reroute.

## 5. Intersection geometry — `cb_network::junction`, `cb_network::conflicts` (ADR-0045)

1. Order approaches by tangent angle.
2. **Cut-back**: for adjacent approaches intersect A's left edge with B's right edge (untrimmed
   offset curves); `cut(A) = max(neighbors' requirements, half the widest crossing width) +
   corner radius` (street 6 m, avenue 9 m; tram-carrying approaches grow to fit 20 m tram
   connectors). Cap 45 % of segment length → else invalid "intersection too close" (min node
   spacing guide shown).
3. **Polygon**: approach end cross-sections + corner fillets (cubic arcs tangent to edges);
   repair with i_overlay union; simplify.
4. **Surface heights per vertex**: cut-line vertices take their approach profile height;
   interior vertices by mean-value coordinates; triangulated with these heights. Combined with
   the landing constraint (§2) steps at cut lines are ≤ 2 cm.
5. **Crosswalks** 1 m inside each cut line (where sidewalks exist); **stop lines 2 m upstream of
   the crosswalk**; none on continuation nodes.
6. **Connectors**: cubic Bézier from incoming lane end to outgoing lane start, handles 0.4 ×
   chord, curvature limited by lane kind (car 1/5 m, bus 1/8 m, tram 1/20 m); sampled polylines.
7. **Turn rules**: signed angle classes (straight ±30°); rightmost lane → right, leftmost →
   left, middle → straight; single lane shares all; merges allow ≤ 2 source lanes per target;
   if a movement would get no connector under these rules, it is assigned to the nearest
   lane (test: every allowed movement has a connector). Same-approach connectors never cross.
8. **Conflict zones**: pairwise capsule intersection (width 2.0 m) of connectors from different
   approaches + merge zones at shared target lanes; zones within 2 m merge.
9. **Taper** on width-changing continuation nodes: length `max(8 m, 6 × Δhalf_width)`; lane
   drops/adds at the taper start with a mandatory lane-change zone.
10. **Corners for plots**: the sidewalk outer edge including the fillet arc is a valid
    frontage (`FrontageRef::Corner`).
11. Lane-graph patching is atomic per node in `EditSet::Lanes`.

## 6. Intersection traffic control — `cb_traffic::junction_ctl`, `cb_traffic::reservations` (ADR-0045)

Vehicles within braking distance + 10 m of the stop line request their connector each tick.

```
grant(v) iff control_allows(v)
          ∧ ∀ zone z on v.connector: z.free() ∨ z.held_by_same_connector_leader(v)
          ∧ target_lane.free_space − target_lane.reserved_m ≥ v.length + gap
on grant: lock all zones; target_lane.reserved_m += v.length + gap
release : zone when v's rear passes its exit s; reservation when v's rear is length+gap into the target lane
```
- Requests per junction per tick processed in order `(priority class desc, arrival tick asc,
  VehicleId asc)`; anti-starvation (waiting > 12 sim s) moves a request to the front ordered by
  wait time desc.
- **Priority / Yield / Stop per approach** (`ApproachControl`); `Auto` classifies majors by road
  hierarchy (tie: most opposite pair). Minor/yield approaches pass a gap check against major
  vehicles on the **upstream horizon** (lanes/connectors within 100 m, precomputed per approach):
  critical gap left-across 4.0 s, merge 3.0 s, straight-cross 3.5 s (× 1.3 in rain/snow). Stop
  approaches additionally require `v < 0.3 m/s` within 2 m of the stop line.
- **Equal priority**: right-before-left; if all waiting approaches would yield, grant the
  earliest arrival immediately (test: 4-way all occupied never idles > 2 s).
- **All-way stop**: FCFS by stop completion; in the same tick, later vehicles whose zones don't
  conflict with already granted ones are released too.
- **Traffic lights**: movement conflict graph from conflict zones; phases = greedy maximal
  compatible sets seeded by the most opposite approaches; protected left phase when the
  opposing through flow > 300 veh/sim-h; min green 8 s, max 30 s, yellow 3 s, all-red 1.5 s;
  actuated early end after min green if the phase has no demand and others do. Yellow: stop if
  `d_stop(b_max = 4 m/s²) < distance`, and no grant if the vehicle can't clear before all-red
  ends. Tests: no conflicting greens on T, X, Y, 5- and 6-way; every movement green in ≥ 1 phase.
- **Background traffic** reduces grant rate: minimum headway per approach
  `h = 1.8 s × (1 + V_bg / C)`.
- **Roundabouts**: detected rings (ADR-0045) — ring approaches Priority, entries Yield; entry
  denied while ring occupancy > 80 %.
- **Visual pedestrians** cross a crosswalk only if no granted connector intersects the
  crosswalk capsule within the next 3 s; they never block vehicles.

## 7. Terrain generation & deformation — `cb_terrain` (ADR-0046)

**Generation** (deterministic, portable math; ~2–4 s, parallel over 64 row partitions):
1. Macro fBm (4 octaves, 2.5 km) + domain-warped valley axis; height 0–180 m; edge falloff.
2. Ridged hills (3 octaves, 600 m) masked by macro elevation.
3. Detail fBm (2 octaves, 120 m, 2–4 m amplitude).
4. Rivers (1–2): sources on high ground, steepest descent with momentum + meander noise to the
   edge/basin; channel 18–40 m wide, 3–5 m deep, 30 m banks; monotonic water surface.
5. Lakes (1–3): priority-flood depression filling (ties by sample index); basins > 0.05 km²;
   level = spill − 0.5 m.
6. Thermal erosion ×6 (talus 35°).
7. Lowland flattening 20 % toward a smoothed version where slope < 8 %.
8. Outside connections (2–3) along low-slope paths from the edge into the lowland.
9. **Constraints** (re-roll the affected sub-stream up to 8 times, else relax): ≥ 3 km² buildable
   lowland (slope < 8 %); ≥ 1 shoreline with depth ≥ 1.5 m within 400 m (network distance) of
   an outside connection's end (tutorial step 4); 1–2 rivers; 1–3 lakes.
10. Derived: materials, forest mask, min-max quadtree. Base heights, water and forest mask are
    saved (ADR-0044).

**Height sampling** uses the render triangulation. **Deformation**: `H = composite(base,
stamps)` per ADR-0046 (cores override with highest priority; elsewhere `base + max fills +
min cuts`); recomputed only in dirty rects; removing a stamp restores the base bitwise.

## 8. Terrain materials & natural decoration — `cb_terrain::materials`, `cb_terrain::scatter`

**Materials** (u8 weights): rock by slope `smoothstep(28°, 40°)`, sand < 1.5 m above water
within 25 m, dirt by moisture noise and paint stamps (shoulders, construction), forest floor
under forest mask, grass otherwise; season tints in shaders (§25).

**Scatter (presentation only).** Instances are generated lazily per 32 m scatter cell when its
render chunk becomes resident: deterministic RNG `(seed, Scatter, cell)`, Poisson-disk (trees
4–7 m in forest, 15–40 m singles, rocks on rocky slopes, grass clumps 2 m near water/meadows),
filtered by water, slope, forest density and **exclusion** (road corridors + 2 m, plots,
footprints, piers). Dirty regions re-filter. **Simulation never reads instances**: tree density
for pollution absorption and land value comes from a deterministic 8 m density field
`forest_mask × (1 − excluded fraction)` + placed decorations.

## 9. Plot drawing — `cb_game::tools::plot`, `cb_sim::zoning::plots`

- **Frontage drag** (default): hover a road edge (sidewalk outer edge, including junction corner
  fillets), press at `s0`, drag along the road to `s1` (may continue around a corner), then drag
  the depth handle (default R 30 m, C 35 m, I 50 m). Back edge = i_overlay offset of the
  frontage polyline by depth (round joins); side edges straight between frontage ends and offset
  ends; on concave frontage depth ≤ 0.8 × inner radius. Depth beyond `d_max(zone) + 10 m` snaps
  to `d_max` (Shift overrides) with a backyard-percentage hint.
- **Polygon mode**: click vertices; edges within 1.5 m of a road edge snap and become frontage.
- **Editing**: vertex drag (frontage vertices constrained to the road edge), edge drag, depth
  handle, ± sub-plot count, zone change. The preview lists buildings that no longer fit
  (ADR-0014 amendment) and will be removed.
- **Validation**: simple polygon, area 100–40 000 m², frontage ≥ 8 m, no overlap with corridors
  (< 0.5 m²), other plots, service buildings, water; mean slope ≤ 25 %. **Auto-clip** (toggle
  chip, default on): overlaps are subtracted and the clipped result shown before commit.

## 10. Plot subdivision — `cb_geometry::subdivide` (ADR-0014)

Input: plot polygon `P` (CCW), frontage polyline(s) `F`, target lot count `N` (player) or
`round(len(F) / w_target)`, zone parameters `(w_min, w_target, w_max, d_max, a_min)`, seed.
1. **Widths**: `w_i = w_target (1 + j_i)`, `j_i ∈ [−0.25, 0.25]`, rescaled to `len(F)`,
   clamped with redistribution; corner frontage → corner lot 1.3× wide spanning both roads.
2. **Split rays**: road normal averaged over ±`w_i/2`, blended toward the PCA depth axis by
   β = 0.3; crossing pairs rotated toward their bisector in 5° steps, ≤ 8 iterations, else the
   split is removed (lots merge; debug log).
3. **Cut**: lot `i = P ∩ Strip_i` (i_overlay, 1 mm snapping); keep the frontage piece; merge
   other pieces into the neighbor sharing the longest edge (drop if < 5 m²).
4. **Depth**: deeper than `d_max` stays as backyard (no landlocked lots).
5. **Repair/validate**: simplify (0.05 m), remove spikes (< 15°), area ≥ `a_min`, frontage ≥
   `w_min`, rectangularity ≥ 0.55; failing lots merge with the narrower neighbor; repeat.
6. **Stable IDs**: match new lots to old by frontage-interval overlap ratio ≥ 0.6 (greedy);
   matched lots keep their `LotId` and their building if it still fits (ADR-0014 amendment).

Performance: p99 ≤ 2 ms per plot (subdivision + fit), measured in spike S-P.

## 11. Building fitting & generation — `cb_procgen::building` (ADR-0029)

`fn generate(input: &BuildingInput, seed: u64) -> BuildingOutput` (pure). Seed:
`derive_seed(world, Building, lot_id, level, regrowth)` for zoned, `building_id` otherwise.
1. **Setbacks** (R 3/1.5/4 m, C 1/0/2 m, I 4/2/4 m) by i_overlay inset; reduce stepwise; empty
   → green lot.
2. **Massing**: R L1–L3 largest inscribed oriented rectangle (raster 0.5 m + continuous edge
   refinement) + optional wing ≥ 25 m² + annex; R L3 row houses span the frontage; R L4–L5 and
   C use the inset polygon (≤ 12 vertices) with stepped upper volume; I halls + tanks, silos,
   chimneys, docks; services/utilities/transit/special: family generators (set A in M4, set B
   with its system).
3. **Floors**: per GAME_DESIGN §4 with ±1 jitter; floor heights R 3.0, C 3.6, I halls 6–9 m.
4. **Slopes**: `Δh < 0.5 m` pad; `0.5–2.5 m` plinth + pad; `2.5–5 m` (R) stilts/terraces;
   `> 5 m` none. The sim emits the matching pad stamp (shared pure function).
5. **Roofs**: rectangles get gable/hip/half-hip/saltbox/flat; polygons flat/parapet/terrace or
   mono-pitch; roof patches exported for snow; HSV jitter per building.
6. **Façades**: window grid per wall (bay 2.4–3.6 m, rows = floors); walls shorter than 2.4 m
   are blank; reflex corners get a column module; entrance on the frontage façade; window
   geometry carries `window_id` in the packed attribute.
7. **Details**: chimneys, AC units, antennas, signs, dormers, balconies, awnings, pipes (≤ 8-sided
   primitives).
8. **Condition** in the shader (param buffer); *Ruined* swaps to a ruin generator.
9. **Yard**: driveway, path, lawn, beds, hedges/fences (not on C frontage), sheds, trees,
   parking stalls, crates; ≤ 400 triangles.
10. **LOD1**: massing + painted window bands (10–15 % of LOD0). LOD2 per 4 chunks.
11. **Format**: indexed 20 B vertices, no normals (derivative flat shading), baked AO in color
    alpha (ADR-0049). Triangle budgets per type asserted.
12. **Determinism**: bit-identical output for the same input and seed on all platforms
    (portable math, ADR-0043), golden-hash tests.

Vehicles: parametric profiles (hatchback, sedan, van, pickup, SUV, truck variants, fire truck,
ambulance, police, garbage truck, bus 12 m, tram 3 sections), 8-sided wheels, emissive light
faces, color index via `MeshTag`.

## 12. Pathfinding — `cb_traffic::router`, `cb_traffic::route_cache` (ADR-0048)

- **Graph**: links per (segment, direction, mode set); turn edges from connectors; ~10–16k
  links for a large city; mode views car, truck, bus (bus-lane preference), tram, service.
- **Cost** (integer ms): `len / v_eff + turn (right 2 s, straight 0, left 5 s, U 20 s) + class
  preference (dirt +20 %) + lane-plan penalty (+30 s for ≥ 2 changes within 60 m)`;
  `v_eff = v_free × congestion factor` snapshotted per epoch (1 game hour = 120 sim s).
- **Variants**: `+0…+20 %` per link from `hash(link, variant, epoch)`; `variant =
  hash(household_or_citizen, purpose) % 4`.
- **Search**: A* with ALT (16 landmarks on free-flow costs; tick-sliced recompute with a
  deterministic swap; `euclid / v_max` fallback between edge additions and the swap); ties by
  `(f, −g, LinkId)`.
- **Cache**: LRU 32 768 `(origin, dest, mode, variant)` entries tagged `(graph_version, epoch)`;
  only current-epoch entries hit; removed links evict entries via a reverse index; saved.
- **Budget**: 50 000 cost units per tick (≈ 0.65 ms, calibrated in S-T) (1 unit per settled node, 1 per cache hit); FIFO
  overflow; parallel over 64 partitions, results in request order.
- **Failures**: negative cache per `(origin, dest, mode, graph_version)`; SCC one-way traps flag
  buildings `Problems::NoRoute`.
- **Reroutes**: vehicles whose next 3 links have congestion > 0.7, ≤ 20 per tick; a vehicle whose
  next link was removed continues to the next junction and requests a new route there.
- **Pedestrian distances**: undirected segment graph (sidewalks + crossings), bounded Dijkstra
  ≤ 800 m.
- **Transit**: `TransitGraph` nodes per (stop, line) and per stop; ride edges from frozen leg
  paths and average line speed; board (wait = headway/2, ≤ 10 min game time), alight,
  transfer (walk ≤ 300 m + 3 min penalty). Dijkstra from accessible origin stops to accessible
  destination stops; cache keyed by sorted stop sets with walk times, invalidated per transit
  edit.

## 13. Traffic flow & congestion — `cb_traffic` (ADR-0047)

- **Agents** (20 Hz at default pace, 1D lane state, all agents): IDM `a = a_max [1 − (v/v0)^4 −
  (s*/s)²]`, `s* = s0 + vT + vΔv / (2√(a_max b))`; car `a_max 1.6, b 2.5, T 1.2 s, s0 2 m`;
  truck/bus `a_max 1.0, T 1.6 s`. `v0 = min(lane limit(s), curve cap, slope factor, weather
  factor) × t_free / t_bpr(V_bg)`; trucks uphill `v0 × (1 − 0.02 × grade_%)`.
- **Leader**: next vehicle on the lane queue, else last vehicle on the next lane/connector of the
  route within 80 m, else the stop line as a virtual stopped leader until granted.
- **Integration**: semi-implicit Euler, `v ≥ 0`, position clamp `s ≤ s_leader − length − 0.5 m`
  (no overlap).
- **Lane plans** (A30): computed at route assignment; admissible lanes propagated backward with
  ≥ 40 m runway per change; mandatory changes start at the first link of the runway with rising
  urgency; discretionary MOBIL-lite (gain > 0.3 m/s², safe gaps). If a mandatory change fails
  15 m before the stop line: stop and wait; after 20 sim s reroute.
- **Parallel step**: 64 lane partitions read the previous-tick snapshot; the sequential pass
  (lane transfers, grants, lane changes) runs in VehicleId order.
- **Spawning**: per access point queue; spawn when gap ≥ length + s0 and upstream TTC > 2 s;
  after 30 sim s waiting, the trip completes as macro.
- **Admission & sampling**: every trip is routed; it becomes an agent with probability `p`
  (controller holds agents at ≈ 90 % of `agent_cap`, `p ∈ [0.05, 1]`). Sampled-out trips add
  weight to link background volume and finish after the BPR time.
- **Flow model**: `V = EMA(agent entries/sim-h) + EMA(background/sim-h)` (half-life 120 sim s);
  `C = lanes × cap_per_lane × weather factor` (dirt 1 000, street 1 600, avenue 1 800 veh/sim-h,
  matching IDM saturation flow within 10 %, unit-tested); BPR `t = t_free (1 + 0.15 (V/C)^4)`;
  congestion = `max(1 − v_measured/v_free, 1 − t_free/t_bpr(V))`.
- **Feedback**: link costs per epoch, commute estimates (EMA of actual perceived times), demand,
  happiness, dispatch ETAs, noise.
- **Traffic disabled**: `p = 0` for cars/trucks; transit/service vehicles in ghost mode
  (kinematic along route at `v_eff`, no interactions).
- **Gridlock**: stuck (`v < 0.5 m/s` > 90 sim s, excluding red light/stop/queue-with-moving-head)
  → despawn, trip completes as macro with penalty, structured debug log; wait-for graph over
  vehicles stuck > 20 s (edges: zone holder, exit-space blocker, queue leader), DFS every 10 sim s
  (sliced), break cycles by despawning the latest trip start; > 20 despawns per game hour in a
  256 m bucket → "traffic jam" notification at the centroid.

## 14. Network connectivity & access — `cb_sim::roads::connectivity`

- Undirected components (union-find rebuild on topology change, < 2 ms) for utilities and
  coverage; **directed SCCs** (Tarjan on the link graph) for reachability; a component is
  connected if it contains an outside connection; buildings outside the outside connection's
  SCC get `NoRoute`.
- `RoadAccess` = projection of the lot frontage midpoint onto the frontage segment's sidewalk
  edge (or corner fillet), recomputed on segment change.
- Plots losing all frontage segments → `Disconnected`; buildings decline after 3 game days; a new
  segment whose corridor edge runs within 2 m of the old frontage re-attaches automatically.
- Demolish validation lists dependents; UI choices: remove road and plots / road only / cancel.

## 15. Demand — `cb_sim::demand`

Hourly (game), EMA half-life 6 game hours, range −1…1, breakdown stored for tooltips. All
terms normalized to [0, 1] unless signed:
- `job_vacancy = (jobs − filled) / max(jobs, 1)`; `housing_vacancy = free_slots / max(slots, 1)`.
- `attractiveness = 0.30 (2h − 1) + 0.20 cov + 0.20 (2 LV − 1) − 0.20 min(1, 5 unemp)
  − 0.10 clamp((commute_p50 − 20) / 20, 0, 1)` (h = mean happiness/100, cov = mean coverage,
  LV = mean land value, commute in perceived min).
- `base_immigration = 0.3` until 300 population, linearly to 0 at 1 500.
- `R = clamp(0.45 job_vacancy + 0.25 attractiveness − 0.35 housing_vacancy − 0.2 tax_pen_R
  + base_immigration, −1, 1)`.
- `retail_demand = Σ households × spend(wealth)` (Low 15, Mid 25, High 40 customers-units/day);
  `retail_capacity = Σ commercial jobs × 25`; `goods_availability = min(1, local + import supply
  / consumption)` (§27).
- `C = clamp(0.5 (retail_demand − retail_capacity) / max(retail_demand, 1) + 0.2
  goods_availability − 0.2 tax_pen_C, −1, 1)`.
- `I = clamp(0.4 (goods_demand − production) / max(goods_demand, 1) + 0.35
  workforce_availability + 0.15 export_access − 0.2 tax_pen_I, −1, 1)`;
  `workforce_availability = unemployed_matching_edu / max(open I jobs + 50, 1)` clamped;
  `export_access = 1` if an outside connection is reachable within 300 sim s, else 0.3.
- `tax_pen = max(0, (rate − 9 %) / 11 %)`.
- **Development**: per game hour `k = ceil(max(0, D) × growth_rate)`, `growth_rate =
  clamp(0.5 + pop / 2 000, 0.5, 6)` new constructions per zone, chosen by **weighted sampling**
  (weight = LV × access × utilities) with `SimRng::for_event(Growth, purpose, hour, k)` so cities
  grow organically and deterministically.

## 16. Citizens, trips & mode choice — `cb_sim::citizens` (ADR-0016)

- **Immigration**: households per game hour `= ceil(max(R, 0) × 6 × (1 + pop / 1 500))`, capped
  by free slots in connected buildings; each arrives as a trip from the nearest outside
  connection; wealth/education from attractiveness and education coverage.
- **Job matching** (hourly, sliced): unemployed adults sorted `(education desc, CitizenId)`;
  candidate workplaces from the per-workplace-segment bounded Dijkstra cache; score
  `= −routed_estimate − 0.2 × level mismatch`; reject estimates > 300 sim s.
- **Schedules** (ADR-0042): shifts commercial N(08:30, σ 90 game min) truncated [05:30, 11:30]
  (15 % evening), industry N(07:00, σ 60) (10 % night), schools 08:00 ± 20; shopping with
  probability 0.6 × wealth factor per household-day at N(17:30, 90); leisure to parks every 4th
  day. Departure = target arrival − routed estimate (game min) − slack N(10, 5).
- **Mode choice** (multinomial logit, β_t = 1.0 per perceived minute, money × 2):
  `U_car = −(t_car + 3) − cost_car` (if a car is available), `U_transit = −(2 walk + 1.5 wait +
  ride + 5 × transfers) − fare`, `U_walk = −1.3 t_walk` if ≤ 800 m. Sampled with event RNG.
- **Arrival** updates state and the commute EMA.
- **Happiness** (daily, sliced): weights in GAME_DESIGN §10; < 35 for 3 days → 20 %/day move-out
  probability. Before M6/M9 unimplemented factors use neutral stubs (GAME_DESIGN §10).
- **Education**: students advance one level per season if their school has capacity.
- **Lifecycle** (yearly, sliced over spring day 1): Child→Student→Adult steps; Adult→Senior p =
  1/12; Senior removal p = 0.12; birth p = 0.15 per household with ≥ 2 adults and size < 5;
  adults with a free R slot may form a new household; empty households dissolve.

## 17. Service coverage & dispatch — `cb_sim::services` (ADR-0056)

- **Reach** per facility (incremental): bounded Dijkstra up to `reach_m` on the undirected
  segment graph; samples per segment; `c_fb = budget × (1 − smoothstep(0.6, 1, d/reach))`.
- **Allocation** (global, hourly, 10 slices): `w_fb = c_fb / Σ c_f'b`; `load_f = Σ demand_b w_fb`;
  `util_f = min(1, cap_f / load_f)`; `coverage_b = 1 − Π (1 − c_fb util_f)`.
- **Incidents** (rates per game day unless noted; constants in `services.ron`):
  - crime events per building = `0.02 × (1 − 0.85 police) × (1 + 2 unemp_local) × (1.2 − 0.4 LV)`;
  - fire risk per building per game hour = `base(type, level) × (1 − 0.8 fire) × weather`
    (+ lightning: storm Poisson strikes weighted by height, ignite 30 % unless fire coverage
    > 0.7);
  - fire growth: intensity ramps 0 → 1 over 60 sim s; damage rate = intensity / 100 per sim s
    (0.3 damage at 60 s, ruined at damage ≥ 1 after ≈ 130 sim s untreated — longer than a
    typical in-reach response of 60–100 sim s); a truck on site lowers intensity by 1/30 per
    sim s; spread to buildings whose footprints are within 6 m with p = 0.15 per 60 sim s ×
    (1 − 0.8 fire) (× 2 in summer clear); test `fire_contained_with_station_within_reach`;
  - sickness: per citizen daily `health −= 0.5 pollution_norm − 0.4 health_cov` (clamped), medical
    event below 30;
  - garbage overflow at stored > 3 days of production.
- **Dispatch** (every 20 ticks, slot 7): nearest facility by directed ETA with a free vehicle;
  service times police 20 sim s, ambulance 30, fire 60–180 by intensity. Vehicles are agents
  (or ghosts). Garbage trucks tour buildings above threshold within reach (nearest-neighbor,
  10 t capacity) and return.

## 18. Utility distribution — `cb_sim::utilities` (ADR-0026)

- Networks = undirected road components; consumers/producers attach via `RoadAccess`.
- **Power** (hourly): wind `P = rated × clamp(((v − 3)/(12 − 3))³, 0, 1)`, 0 above 25 m/s, height
  bonus `1 + 0.004 (h − h_mean)`; solar `rated × max(0, sin(sun_elev)) × (1 − 0.75 cloud)`;
  coal/gas constant × budget; demand × heating `1 + 0.3 clamp((10 − T_c) / 18, 0, 1)`.
  Shortage allocation: services/utilities first, then network distance from the nearest plant
  ascending, ties BuildingId.
- **Water**: pumps need surface water within 15 m with depth ≥ 1.5 m; water towers anywhere
  (groundwater). Polluted source (concentration > 0.3) delivers polluted water (sickness +0.2/day,
  happiness −5).
- **Sewage**: outlets/treatment plants at shorelines. **Water pollution**: water-body graph from
  body IDs (rivers as ordered polylines by surface height, lakes as well-mixed nodes); outlet mass
  injected at the nearest river node, advected downstream with 5 %/km decay; lake concentration
  = mass/volume with a 2-day half-life; rasterized into `water_pollution` for the overlay.
- **Garbage**: accumulation per building; trucks (M9) or analytic hourly collection
  `min(stored, capacity share within reach)` before M9 / with traffic disabled (test: trucks
  match analytic within 10 %). Full landfill stops accepting and notifies; `EmptyLandfill`
  hauls to an incinerator (2 per t) or the landfill can be closed (becomes a park after 4 days).
- Status flips (hysteresis 10 %) emit `BuildingChanged`.

## 19. Land value, pollution, noise — `cb_sim::fields` (ADR-0028)

- 512 × 512 rasters (8 m), bilinear sampling, double-buffered, sliced per the phase table.
- **Ground pollution** (hourly): `P ← 0.97 P + Σ sources` + 3×3 blur, then
  `P ← P (1 − 0.03 × tree_density_cell)`.
- **Noise** (hourly, recomputed, sliced): roads as line sources
  `L = 10 log10(V_per_sim_h) + type − 10 log10(d/d0)`, industry/construction/night commercial as
  point sources `− 20 log10(d/d0)`; tree rows −2 dB.
- **Land value** (hourly, smoothed half-life 1 game day), components in [0, 1]: services 0.30,
  parks/decorations 0.15, transit 0.10, terrain base (water view +, slope −) 0.15, road type
  0.05, pollution −0.20, noise −0.10, crime −0.10, abandoned within 50 m −0.10. Decoration per
  side within 40 m: tree row +0.04, grass strip +0.02, cobblestone +0.03; placed props per
  GAME_DESIGN; transit bonus per ADR-0035 amendment.

## 20. Building leveling — `cb_sim::buildings::leveling`

Daily, sliced: `score = w_lv LV + w_srv services + w_util utilities + w_hap (happiness or profit)
+ w_acc accessibility`, weights R `0.30/0.25/0.15/0.20/0.10`, C `0.25/0.15/0.15/0.30/0.15`,
I `0.10/0.10/0.20/0.35/0.25` (`zones.ron`). Up if `score ≥ up[L]` for 3 consecutive days, all
utilities, level unlocked, lot area ≥ min; down below `up − 0.15` for 5 days; decline →
abandon → recover as before (3/4/2 days). Test: a fully served lot at LV 0.8 reaches R L3 within
10 game days. `blocked_by` explains in the info panel.

## 21. Economy tick — `cb_sim::economy`

- **Hourly accrual**: household taxes `income(wealth, level) × rate_R / 24`; business profit
  taxes (§27) `× rate_C/I`; road upkeep `m × upkeep × (1 + 0.6 snow_cover)`; building upkeep ×
  budget; transit vehicle-hours × rate − fares; loan interest. Money in cents, banker's rounding,
  residual carried.
- **Daily statement**, warnings when the projected balance < 0 within 3 days.
- **Bankruptcy**: after 7 days below zero take an emergency loan if available; otherwise
  service budgets are forced to 50 %, construction (except demolition and policy) is blocked, a
  banner is shown; recovery when the balance is ≥ 0 for 1 day. Sandbox never.
- **Costs** charged on command apply; **refunds**: undo per ADR-0055 (100 % first game hour,
  linearly to 25 % at 1 game day); demolish 25 % within 1 day of construction, else 0.
- **Loans**: simple daily interest on principal, at most one per tier (≤ 3), no term, repaid in
  full.

## 22. Procedural generation pipeline & seeds — ADR-0043

- `derive_seed` by sequential absorption (ADR-0043); domains: Terrain, River, Lake, Scatter,
  Plot, Lot, Building, Yard, Vehicle, Citizen, Name, Weather, Audio, Event, Growth, Immigration,
  Lifecycle.
- New game: terrain (§7, with constraints) → outside connections → initial money/unlocks; scatter
  lazily per resident chunk.
- Event randomness: `SimRng::for_event(stream, purpose, tick_or_day, id)`.
- Meshes are pure functions of saved data + seeds, cached in memory only (LRU, ADR-0049).

## 23. Cursor picking — `cb_terrain::raycast`, `cb_game::picking` (ADR-0034)

1. `picking::cursor_ray`: logical cursor → NDC → world camera (render-scale aware, ADR-0053) →
   `Camera::viewport_to_world`.
2. Terrain: min-max quadtree ray march (2 m → 1 km levels), exact triangle tests; water planes.
3. Objects: buildings via 3D DDA over spatial-hash cells against massing prisms; ground roads
   by curve projection near the ground hit; **elevated roads/bridges** (elevation > 1 m) by
   ray–ribbon tests against the deck (profile height ± half width); plots/lots by
   point-in-polygon; visible vehicles by OBBs from the view pool; stops by radius.
4. Priority by tool (road tool: nodes > segments; inspect: vehicles > buildings > roads >
   plots). Result cached per frame in `CursorHit`.

## 24. Render chunking, LOD & incremental meshing — `cb_game::render` (ADR-0049)

- 128 m render chunks (internal). Vertex format: indexed 20 B (position, color+AO, packed
  slot/flags), flat shading from derivatives; terrain smooth with normals.
- **Residency rings**: LOD0 within 1.3× range (evict beyond 1.6×), LOD1 within 1.3× its range,
  LOD2+ everywhere. LOD ranges: terrain 350 / 900 / 2 000 m; buildings 450 / 1 400 m; trees
  300 / 900 m; bands ≤ 10 %, hard switch for shadow casters.
- **Buildings & yards**: per-building meshes on the async pool (LRU 256 MB), merged per chunk per
  material class; `ATTRIBUTE_PACKED` slot → per-chunk `ShaderBuffer` record (condition,
  construction progress, highlight, overlay value, window seed, flags); state changes update the
  buffer only; re-merge debounced 250 ms; a freshly built building shows as a temporary
  standalone entity until its chunk merge lands.
- **Roads**: per chunk merged surfaces/markings/curbs/sidewalks + decorations (trees, lamps,
  light pools) in the props mesh.
- **Props/trees**: merged per chunk per LOD; trees full / simplified / cross-quads; wind sway in
  the vertex shader.
- **Vehicles**: pooled entities with shared variant meshes + `MeshTag`; headlight pools in one
  dynamic mesh; problem icons in one dynamic billboard mesh.
- **Uploads** ≤ 8 MB/frame; jobs prioritized by distance; generation counters drop stale results.

## 25. Lighting state from time & weather — `cb_game::lighting` (ADR-0050, ADR-0018)

- Sun from time of day + season (declination ±23°, latitude 48° N); moon opposite with phase.
- `LightingState = blend(keyframes(t), weather(kind, intensity), season)`; keyframes and
  modifiers from ART_DIRECTION §3–4 (pre-exposure values); weather transitions over 20–40
  game minutes; cubic interpolation in time, OKLab for colors.
- Outputs: directional light (sun ↔ moon crossfade below −4° elevation), ambient floor, env map
  (≤ 1 regeneration per 2 real s), focus-distance fog + height fog, sky uniforms, exposure,
  bloom, night factor, wetness, snow cover, cloud shadow parameters, season tints, per-segment
  road snow `snow_cover × (1 − clamp(V/400, 0, 0.8))` (hourly into the road param buffer).
- Night readability: calibration metrics in ADR-0050.

## 26. Camera controller — `cb_game::camera`

- State: `focus` (XZ), `focus_h` (smoothed max(terrain, water, deck) at focus), `yaw`,
  `distance` (25–2 400 m), `pitch = pitch_curve(distance)` (≈ 35° → 65°) + offset (±15° via
  vertical rotate-drag); critically damped springs to targets.
- **Pan**: WASD/arrows along the yaw basis (speed ∝ distance); left-drag on empty ground when no
  tool owns the drag (ADR-0052) or Space + left-drag anytime, keeping the grabbed ground point
  under the cursor; two-finger trackpad scroll (pixel-unit wheel); optional edge scrolling.
- **Zoom**: line-unit wheel and pinch, exponential toward the cursor ground point, clamped.
- **Rotate**: MMB drag, right-drag, Alt/⌥ + left-drag, two-finger rotate (free); Q/E tap
  (< 200 ms) snaps to the next 90°; holding Q/E rotates continuously (deg/s setting).
- Terrain following and collision guard (≥ 6 m above terrain along the camera position); focus
  clamped to map bounds − 50 m.
- **Follow**: clicking a vehicle sets `follow: VehicleId`; focus = interpolated position + heading
  look-ahead; pan input, Esc or despawn ends follow.
- Speeds, smoothing, inversion and bindings come from settings and the `InputMap`.

## 27. Goods economy — `cb_sim::economy::goods` (ADR-0057)

- Industry produces `0.5 units × filled_jobs` per game hour into stock (capacity 40 × level);
  commercial consumes `0.02 units × customers`; daily customers from `retail_demand` share
  (weighted by LV and accessibility).
- Hourly (phase slot): commercial with stock < 50 % requests `ceil(deficit / 10)` deliveries;
  matched to the nearest industry by cached ETA with stock ≥ 10, else imported via the nearest
  outside connection at +30 % cost; industry stock > 80 % exports the surplus (income).
- Each delivery is a `Truck` trip with 10 units (agent or macro); stock moves on arrival.
- Profit per game hour = sales − wages − goods cost (+ exports); taxed at the zone rate; feeds
  leveling (profit term) and demand (`goods_availability`, `production`, `export_access`).
- Conservation test: produced = sold + stocked + exported + in transit.
