# ADR-0009: Intersection handling and traffic control

- **Status:** Superseded by ADR-0045
- **Date:** 2026-10-15
- **Milestone:** M3 (geometry, settings), M9 (control behavior)

## Context

Roads that meet or cross must produce automatic, good-looking junction geometry and lane
connections; control is priority by default with player-set stop signs or traffic lights;
vehicles must never pass through each other and standard junctions must not deadlock.

## Options considered

Geometry:
1. **Precomputed junction templates** (T, X) — robust but cannot handle freeform angles/curves.
2. **Node-based generation from incident roads (chosen)** — cut-backs from edge
   intersections, fillet corners, connectors as Béziers between lane ends.

Control/admission:
1. **Pure car-following with "virtual stop lines"** — cheap but conflicts across approaches
   need ad-hoc collision checks; deadlocks common.
2. **Time-slot reservations (AIM-style)** — elegant and optimal, but expensive and
   sensitive to speed prediction errors.
3. **Conflict-zone locking with exit-space check (chosen)** — space reservations: a
   vehicle locks all conflict zones on its connector atomically before entering, and only
   if its exit lane has room.

## Decision

- Geometry as in ALGORITHMS §5: angular ordering, cut-backs (capped at 45 % of segment
  length → else invalid), fillets, polygon repair, crosswalks, stop lines, lane arrows,
  connectors, pairwise conflict zones (capsule intersection) and merge zones.
- Node kinds: Junction, Continuation (degree 2, no junction mesh; taper on width change),
  DeadEnd (turning area), Outside connection.
- Control as in ALGORITHMS §6: `IntersectionControl::{Auto (priority), AllWayStop,
  Lights(plan override)}`; priority classes from road hierarchy; gap acceptance for minor
  approaches; right-before-left among equals; anti-starvation (12 s).
- Admission rule shared by all control types: `control_allows ∧ exit space ∧ all zones
  free (or shared with same-connector leader)`, processed in deterministic order. Vehicles
  never wait inside a junction → no intra-junction deadlock; residual network gridlock
  handled by ADR-0010's stuck detection.
- Junctions on bridges are disallowed in v1.

## Consequences

- Works for arbitrary angles/curves; one algorithm for all control types; testable
  properties (no deadlock, no conflicting occupancy).
- Throughput is slightly below real-world (conservative locking); tuned via platoon sharing.
- Roundabouts are not a special type in v1 (a ring of one-way roads works with the same
  rules; dedicated roundabout tool could come later).
