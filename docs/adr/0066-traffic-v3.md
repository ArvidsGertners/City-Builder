# ADR-0066: Traffic simulation v3 and junction control (validated by spike S-T)

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M9 (agents, control), M5 (macro flow); validated by spike S-T
- **Supersedes:** ADR-0047 (traffic model v2) and the **control section** of ADR-0063 (the
  junction geometry rules of ADR-0063 stay in force). Kept unchanged: 1D lane microsimulation
  with IDM, `agent_cap` as a `SimConfig` value (default 5 000), capacities per lane, lane plans
  with 40 m runway, spawn queues, background-traffic headway scaling, the admission rule
  (control ∧ zones free ∧ target-lane reservation of `length + gap`), right-before-left,
  priority/yield gap checks with a 100 m upstream horizon, actuated signal timings, ring
  detection, the 90 s stuck timeout, macro completion of despawned trips, rendering pools and
  the fallback ladder. The ADR-0047 amendment from spike S-Time (trip scale 6, `p = 1` at 50k)
  still applies.

## Context

Spike S-T (`spikes/s_t/`) built the full v2 model on a synthetic 4 × 4 km city
(10 428 links, 2 922 nodes, 30 601 connectors, 68 849 conflict zones; T/X/Y/5-way, 4-lane
avenues, one-way rows, a 6-entry ring) and measured it against the PROGRESS.md S-T gates. With
the v2 structure (parallel IDM, everything else in one sequential pass) the 5 000-agent step
was 1.45–1.71 ms p95 with a 0.9 ms sequential pass (gates 0.9 / 0.35 ms), junction control
0.4 ms (gate 0.3), and four control rules produced artifacts:

- ring entries yielded only to *moving* ring vehicles, so a ring vehicle stopped at the line
  lost its target-lane space to entries and the ring locked (6-entry ring: 512 veh/sim-h,
  46 despawns per 1 000 trips when saturated);
- ring metering counted ring lanes only, not ring connectors or granted entries;
- the equal-priority deadlock fallback released one vehicle per tick: a 4-way junction with all
  approaches occupied served 1 222 veh/sim-h, below a real all-way stop;
- the wait-for graph used `stuck_s` (which legitimate waits do not advance), so creeping jams
  were never seen as cycles.

## Options considered

1. **Keep v2, apply the fallback ladder** (forced signals, mesoscopic model, cap 3 000) — loses
   agents where the problem was code structure and three fixable rules.
2. **Restructure the step into exact parallel phases and fix the rules** — the model stays
   microscopic at the v2 cap; needs careful proofs that the parallel parts are exact.
3. **Mesoscopic queues for all non-visible traffic** — cheapest per vehicle, but control
   behaviour then differs on and off screen.

## Decision

Option 2.

**Tick structure** (`cb_traffic`, inside the single-threaded `SimTick`, ADR-0061; partitions
are `id % SIM_PARTITIONS` over lanes and connectors, results merged in partition order):

1. *Parallel phase 1* (read-only on sim state): IDM for every occupied lane/connector; per
   vehicle also the flags the sequential pass needs — head crossed the lane end, holds zones or
   a reservation, reached its destination, slow (`v < 0.5 m/s`) and whether the wait is
   legitimate (evaluated on the start-of-tick state), and a pre-filter for discretionary lane
   changes (MOBIL check on the start-of-tick state; re-validated sequentially).
2. *Parallel phase 2*: each partition writes its own vehicles' `(s, v)`, `stuck_s`, `wait_s`
   and per-lane speed sums (every vehicle is in exactly one queue, every queue in one
   partition — no shared writes).
3. *Sequential*: merge partition lists; transfers only for lanes/connectors whose head
   crossed; zone/reservation releases only for flagged vehicles; lane changes and want
   recomputation only for vehicles that entered a lane, changed lanes, need a mandatory change,
   wait for a reroute, or pass the discretionary pre-filter (every 5th tick per vehicle);
   discretionary cooldown is a tick stamp (`lc_until = tick + 40`).
4. *Parallel junction prepass*: collect requests (≤ 3 per lane, each behind granted
   predecessors only) and evaluate `control_allows` and grant feasibility (zones free, target
   space, nothing pending). Both depend only on grants **at the same node**, so this is exact.
5. *Sequential grants*: requests sorted by a packed `u128` key (node, starving, priority,
   request tick, VehicleId); per node the precomputed values are used until the first grant at
   that node, after which `control_allows` and feasibility are re-evaluated.
6. Signals (cached list of signalized nodes), spawning, stuck despawns from the phase-2 kill
   list, routing (ADR-0067).

**Control rule changes** (replacing the control section of ADR-0063; unchanged rules listed in
*Supersedes*):

- **Gap check**: a non-granted priority vehicle within request range (`v²/2b + 10 m`) whose
  connector targets the **same lane** as the yielding movement always blocks it, moving or not.
  Other non-granted priority vehicles in request range are blocked and are skipped, as before.
- **Equal priority**: if every waiting approach would yield, grant the earliest arrival, then
  greedily every later waiting request in `(request tick, VehicleId)` order whose zones are
  still free. The zone check excludes conflicts with the vehicles just granted.
- **Zone claims**: a request that control allows but zones/space block, waiting > 6 sim s,
  claims its zones for the rest of that node's pass (later requests may not take them).
- **Grants** are refused while the vehicle still holds zones or a reservation of its previous
  connector.
- **Roundabout metering**: entry denied while ring occupancy > **70 %**, occupancy =
  Σ (length + s0) of vehicles on ring lanes, on connectors into ring links, and granted entries
  still on their approach, over ring lane length + ring→ring connector length.
- **Signals**: the spike ran protected-left phases at every signal (4 phases at avenue ×
  avenue). The ADR-0063 rule "protected left only when opposing through flow > 300 veh/sim-h"
  stays the target. M9 enables permissive lefts only once the test
  `signal_permissive_left_gap_acceptance` passes (left-turners gap-check the opposing through
  movement; no zone conflicts). Until then, protected left is used always.

**Gridlock**: stuck = slow for > 90 sim s *outside legitimate waits*. A wait is legitimate if
the lane head is moving, is at a red signal, or waits at a node that granted within the last
10 sim s. The wait-for graph uses raw slow time `wait_s > 20 s` (every slow tick counts). Its
edges are the zone holder, the exit-space blocker and the queue leader, and it is searched
every 10 sim s.

**Lane plans**: a reroute (from the current lane's exits) always admits the current lane on
the first link (`plan[0] |= current lane`), else a vehicle that missed its change loops
reroute → wrong lane → reroute.

**Measured at the gates** (container, 4 threads, 5 000 agents, city seed 6, 2 game days unless
noted; numbers in PROGRESS.md S-T outcome):

| Gate | Result |
|---|---|
| Vehicle step p95 ≤ 0.9 ms, sequential ≤ 0.35 ms (5 000 agents) | 0.58–0.65 ms, sequential 0.19–0.22 ms ✓ (v2: 1.45–1.71 / 0.9) |
| Junction control p95 ≤ 0.3 ms | 0.27–0.29 ms ✓ (v2: 0.40) |
| 0 overlaps, 0 conflicting zone occupancy | 0 / 0 in every case and run ✓ |
| Signal discharge ≥ 1 500 veh/sim-h/lane of green | 1 621 (mean headway 2.22 s) ✓ |
| Case 1 never idle > 6 s while vehicles wait | 1.2 s; 4-way capacity 1 846 veh/sim-h ✓ |
| Case 2 at V/C 1.2 recovers ≤ 180 sim s | 74 s (V/C 0.9: 108 s) ✓ |
| Despawns < 1 per 1 000 trips at V/C ≤ 1.0 | cases 1–5 at V/C ≤ 1: 0; city at 1 500 agents (0 links, 20/2 915 nodes with peak V/C > 1): 0.67 ✓ |
| Identical hash 1 / 4 / 8 threads | identical ✓ |

Beyond the gates: the same city with 5 000 agents is oversaturated (437–460 of 2 917 nodes with
peak V/C > 1) and despawns 18.5–21 per 1 000 trips; 8 000 agents: 893 nodes > 1, 58 per 1 000,
step 0.99 ms; 12 000: 1 225 nodes > 1, 103 per 1 000, step 1.15 ms, junctions 0.74 ms.

## Consequences

- The v2 cap and microscopic behaviour hold everywhere; no fallback-ladder step is needed for
  CPU time.
- Parallel phases rely on two invariants that M9 must test: one queue per vehicle
  (`vehicle_in_exactly_one_queue`), and control/feasibility depend only on same-node grants
  (`junction_prepass_matches_sequential`: hash-identical against a sequential reference).
- Despawns are the release valve when demand exceeds junction capacity. They stay below
  1 per 1 000 trips where peak node V/C ≤ 1. In oversaturated cities they rise with the
  oversaturated share, so the traffic overlay and the "traffic jam" notification must make
  oversaturated junctions obvious (M9).
- Revisit if the M9 soak tests (`traffic_multi_day_large`) exceed the despawn target at
  V/C ≤ 1, or if junction p95 exceeds 0.3 ms on citygen networks (next step: the
  sequential grant pass over nodes in parallel, since zones, target lanes and requesting
  vehicles are node-local).
