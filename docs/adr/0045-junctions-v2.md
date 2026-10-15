# ADR-0045: Junction geometry and traffic control v2

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M3 (geometry, controls data, roundabouts), M9 (behavior); validated by spikes S-J and S-T
- **Supersedes:** ADR-0009 (keeps node-based generation, cut-backs, fillets, connectors,
  conflict zones, conflict-zone locking)

## Context

Review findings G-T1–G-T7, G-G1, G-G6, G-G7, A25–A27, E19, B5, B30: exit space was checked
but not reserved (vehicles could stop inside a junction, so "deadlock impossible" was
false); right-before-left stalled when every approach was occupied; only all-way stops
existed although players expect per-approach stop/yield signs; the junction surface was a
single height (steps on slopes); tram turning radii were not considered; phase plans were
only defined for 4-way junctions; roundabouts were unsupported.

## Decision

**Geometry**
- As ADR-0009, plus: **per-vertex junction surface heights** — vertices on each approach cut
  line take that approach's profile height; interior vertices are interpolated
  (mean-value coordinates over the cut-line vertices); approach profiles are constrained to a
  *landing* grade ≤ 4 % over `[0, cut_back + 4 m]` from the node. Step at cut lines ≤ 2 cm.
- Stop lines sit 2 m upstream of crosswalks.
- Connector minimum radius per lane kind: car 5 m, bus 8 m, tram 20 m; cut-backs grow to fit
  tram connectors on tram-carrying approaches.
- Width-change taper on continuation nodes: `max(8 m, 6 × Δhalf_width)`.
- Minimum node spacing per road-type pair is published in GAME_DESIGN and shown as a guide.
- Plot frontage can follow a junction corner fillet (`FrontageRef::Corner { node, corner }`).

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

- Deadlock-free admission inside junctions with a testable invariant; residual network gridlock
  is handled by ADR-0047. Spike S-J/S-T measure rejection rates, throughput and cost; their
  fallbacks (degree/angle caps, forced signals) need a superseding ADR if taken.
