# ADR-0047: Traffic simulation model v2

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M9; validated by spikes S-T and S-Time
- **Supersedes:** ADR-0010 (keeps the hybrid: 1D lane microsimulation for agents + macro flow
  model for all trips, presentation-only pose LOD, ghost mode when traffic is disabled)

## Context

Review findings C-1, G-T*, G-M8, G-C1, A28–A31, A54, D4, D5, D11: agent cap doubled as a
graphics setting (graphics changed sim results); mandatory lane changes on short links had no
look-ahead; spawning onto occupied lanes was unspecified; background traffic had no effect on
junction throughput; routing had a count cap but no time cap; cycle detection ignored
exit-space and leader waits; vehicles as entities were not budgeted.

## Decision

- **`agent_cap` is a `SimConfig` value** (new-game option 2 000–12 000, default 5 000, saved);
  graphics settings only control `max_visible_vehicles`.
- Units per ADR-0042 (veh/sim-h, sim seconds); capacities per lane: dirt 1 000, street 1 600,
  avenue 1 800 veh/sim-h (validated against IDM saturation flow within 10 % by a unit test).
- **Lane plans**: at route assignment, admissible lanes per link are propagated backward until
  each required lane change has ≥ 40 m of runway; vehicles target the plan from the first
  link. Routing adds +30 s to turn edges needing ≥ 2 changes within 60 m.
- **Spawn queue** per access point: spawn when gap ≥ length + s0 and upstream TTC > 2 s; after
  30 sim s of waiting the trip completes as macro (counted in diagnostics).
- **Background traffic**: desired speed `v0 × t_free / t_bpr(V_bg)` (unchanged) and junction
  release headway per approach `h = 1.8 s × (1 + V_bg / C)`.
- **Gridlock**: stuck = `v < 0.5 m/s` for > 90 sim s outside legitimate waits → despawn (trip
  completes as macro with penalty), structured debug log. Wait-for graph over vehicles with
  `stuck_s > 20 s`, edges: zone holder, exit-space blocker (last vehicle on target lane),
  queue leader; DFS every 10 sim s (sliced); break each cycle by despawning the latest trip
  start (tie VehicleId).
- **Routing load**: time cap in deterministic cost units (settled nodes, ADR-0048), never
  wall-clock.
- **Rendering**: visible agents are pooled entities (≤ `max_visible_vehicles`, default 3 000)
  sharing variant meshes (automatic instancing, `MeshTag` colors); headlight pools are one
  dynamic mesh; counts are inside the 15k renderable-entity budget (ADR-0049).
- **Pedestrians** (visual): spawned near the camera from stop `WaitingGroup` changes and
  access points; cross only when no granted connector intersects the crosswalk within 3 s;
  never block vehicles; not sim agents.
- **Target**: at 50k population at least 40 % of peak trips are agents (`p ≥ 0.4`, spike
  S-Time). Fallback ladder if spike S-T fails (decided by measurement, recorded in a
  superseding ADR): (1) force signals at ≥ 4-approach equal-class junctions; (2) mesoscopic
  link-queue model for non-visible traffic; (3) lower default cap with a larger macro share.

## Consequences

- Graphics settings no longer affect simulation; lane-change failures and spawn conflicts
  have defined behavior; budgets are deterministic and testable.
