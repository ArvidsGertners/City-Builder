# ADR-0063: Junction geometry v3 (validated by spike S-J)

- **Status:** Accepted (control section superseded by ADR-0066)
- **Date:** 2026-10-15
- **Milestone:** M3 (geometry), M9 (control behaviour)
- **Supersedes:** ADR-0045 — geometry rules change as below; the control rules are carried over
  unchanged (restated in full).

## Context

Spike S-J (`spikes/s_j`, 10 000 random junctions per distribution + 12 canonical cases, see
PROGRESS Phase S) found four problems with the ADR-0045 geometry:

1. The static minimum-node-spacing table (GAME_DESIGN) contradicts the cut-back rule: two street
   X-junctions need ≈ 12 m cut-back each, i.e. ≥ 27 m between nodes under the 45 % cap, while
   the table said 22 m; with mixed widths (a street joining a tram avenue needs a 20 m cut-back)
   no static per-type table is right. With fixed 30–60 m segments 72 % of junctions are rejected.
2. Near-U-turn movements (120–150° into a neighbour 40–60° away) forced large cut-backs and
   produced connectors leaving the junction surface.
3. A fixed "+ corner radius" over-reserves on obtuse corners and adjacent cut lines / corners of
   curved roads can intersect even when kerbs never cross.
4. Connectors from one lane to different exits (alternatives) can cross after diverging; rarely
   (0.02 %) connectors from different lanes of one approach cross on curved geometry.

## Options considered

1. **Fallback per the spike plan** — degree ≤ 4, min angle 40°, parameterized T/X templates:
   loses 5/6-way junctions and freeform angles the brief asks for.
2. **Fix the rules and validate geometrically (chosen)** — live spacing guide, movement limit,
   corner allowance by angle, iterative growth until the geometry is valid, explicit handling
   of crossing alternatives.

## Decision

**Geometry**
- Per-vertex junction surface heights, landing grade ≤ 4 % over `[0, cut_back + 4 m]`, step at
  cut lines ≤ 2 cm (unchanged; measured 0.0 mm incl. 2 % camber with a vertex every ≤ 1 m).
- **Cut-back** per approach: `max(kerb-crossing requirement, ½ widest other approach) +
  corner allowance`, corner allowance `min(r, r / tan(α/2))` for the corner's interior angle α
  (r = street 6 m, avenue 9 m, dirt 4 m). The kerb-crossing search covers the whole segment; a
  cut-back beyond 45 % of the segment is `ValidationError::TooClose`.
- **Iterative growth** (2 m steps, ≤ 40 iterations) until all hold: connector curvature ≤ car
  1/5 m, bus 1/8 m, tram 1/20 m; every connector within the polygon (0.3 m); polygon simple
  (intersecting cut lines / corners grow their approaches). Exceeding the cap → `TooClose`;
  exhausting iterations → `ConnectorTooSharp`.
- **Corners**: quadratic fillet through the kerb-line intersection on convex corners; on reflex
  (open) corners the polygon follows both curved kerbs back to their meeting point.
- **Movements**: deflection > 135° is not allowed at a junction (no connector; routing uses
  another junction or a turning circle). Every other movement gets a connector.
- **Lane assignment**: per incoming approach, targets are ordered by deflection (sharpest right
  → sharpest left) and source lanes are assigned monotonically (right targets from right lanes).
  If two connectors from *different* lanes of one approach still cross, the second is re-sourced
  from the first one's lane. Connectors from the **same** lane are alternatives and may cross;
  they get a conflict zone between them like connectors of different approaches.
- **Spacing guide**: the road tool shows the live minimum segment length computed by the
  junction builder for the actual junction(s) at both ends (no static table; GAME_DESIGN lists
  typical values only).
- Stop lines 2 m upstream of crosswalks; crosswalks 1 m inside each cut line where sidewalks
  exist; width-change taper `max(8 m, 6 × Δhalf_width)`; plot frontage may follow a corner fillet
  (`FrontageRef::Corner`) (unchanged).
- Performance target unchanged (p99 ≤ 0.4 ms per junction); the spike reaches p50 0.17 ms /
  p99 1.4 ms with unoptimized code — M3 adds an angle-aware initial cut-back estimate and
  per-approach incremental growth (only changed approaches recompute).

**Control**
- Per-approach `ApproachControl { Priority, Yield, Stop }`; node control
  `Auto` (classified from road hierarchy) `| Custom(per-approach) | AllWayStop | Lights(plan)`.
- **Admission rule** (all control types): grant iff control allows ∧ every conflict zone on the
  connector is free (or held by the same-connector leader) ∧ the target lane can **reserve**
  `length + gap`. Each grant increments the target lane's `reserved_m`; it is released when the
  vehicle's rear has travelled `length + gap` onto the target lane. Platoon followers need their
  own reservation. With reservations, no admitted vehicle can stop inside a conflict zone; a
  property test asserts `no_vehicle_stops_inside_conflict_zone`.
- Equal-priority junctions: right-before-left; if every waiting approach would yield, the
  earliest arrival (arrival tick, then VehicleId) is granted immediately. Anti-starvation
  (> 12 s waiting) uses its own ordering key (wait time desc, VehicleId).
- All-way stop: FCFS by stop-completion tick with greedy release of later vehicles whose zones
  don't conflict in the same tick.
- Yield/priority gap check uses a per-approach upstream horizon (lanes/connectors within 100 m)
  built when the lane graph is patched.
- Signals: phases from the movement conflict graph by greedy maximal compatible sets, seeded
  by the most opposite approaches; protected left when the opposing through flow exceeds 300
  veh/sim-h; min green 8 s, max 30 s, yellow 3 s, all-red 1.5 s (sim seconds); yellow
  decision uses `b_max = 4 m/s²`; no grant if the vehicle cannot clear before all-red ends.
- **Roundabouts**: a cycle of one-way segments with 3–8 nodes and radius 15–60 m is detected as
  a ring: ring approaches get Priority, entries Yield, and entries are metered (denied) while
  ring occupancy > 80 %. A roundabout preset in the road tool draws such a ring.
- Junctions on bridges remain disallowed in v1.

## Consequences

- Freeform 2–6-way junctions at ≥ 25° stay possible; realistic layouts (S-J "realistic" set:
  55 % 3-way, 35 % 4-way, mostly streets, gentle curves, live guide) reject 0.65 % — all as typed
  errors with the guide showing the fix; 0 invalid polygons, 0 connectors off the surface,
  0 crossing lanes, curvature limits held.
- Players see a computed minimum length while drawing; dense street grids need ≈ 27 m
  between street junctions, more next to avenues (≈ 38–51 m).
- Revisit if M3 profiling cannot reach p99 ≤ 0.4 ms (then: rebuild junction geometry off the
  main thread with the previous geometry shown until ready).
