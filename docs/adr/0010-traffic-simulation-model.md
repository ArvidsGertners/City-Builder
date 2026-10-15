# ADR-0010: Traffic simulation model — agents, LOD, trip generation

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M9

## Context

Commuter cars, trucks and service vehicles from the citizen and goods simulation; lane
following, lane changes, following distance, speed limits, slopes/curves; congestion fed
back into the city; ≥ 5k simultaneous agents at 60 FPS; agent LOD; fully playable with
traffic disabled (aggregate commute model); determinism independent of the camera.

## Options considered

1. **Full microsimulation in 2D world space** (steering, collision avoidance) — expensive,
   hard to make deadlock-free.
2. **Cellular automaton on lanes** — cheap, but jerky and grid-like motion.
3. **Pure macroscopic flow (static assignment)** — cheap, deterministic, but no visible
   agents.
4. **Hybrid: 1D lane-based microsimulation for agents + macroscopic flow model for
   everything (chosen)**.

## Decision

- **Agents**: every agent is simulated at 20 Hz in 1D on its lane/connector (`s`, `v`)
  with IDM car-following, route-driven + MOBIL-lite lane changes (logical switch,
  visual blend), junction admission (ADR-0009). State in SoA (`VehicleStore`), lanes keep
  ordered queues for O(1) leaders. Overlap is impossible by construction (position clamp
  behind leader).
- **Agent LOD** without camera influence on outcomes:
  - *Presentation LOD*: world poses, lateral lane-change animation, wheel/indicator
    detail and pedestrians are computed only for agents in view (frustum + margin, up to
    `max_visible_vehicles`); far agents cost only their 1D update.
  - *Aggregate LOD*: beyond `agent_cap` (a saved sim setting, default 5 000), trips are
    sampled out: each trip becomes an agent with probability `p` (adaptive controller
    holding agents at ~90 % of the cap); sampled-out trips are macroscopic: they add their
    weight to link background volume and complete after the BPR travel time. At typical
    city sizes `p = 1`.
- **Flow model** (always on): per-link volume EMA (agents + background), BPR travel time,
  congestion metric = max(agent-measured slowdown, BPR estimate) → overlays, link costs,
  commute estimates, demand, happiness, service ETAs, noise.
- **Trip generation** from the citizen time wheel (commute, school, shopping, leisure)
  and the goods economy (deliveries industry → commercial, exports/imports via outside
  connections, garbage, service dispatch).
- **Traffic disabled**: `p = 0` for cars and trucks; transit/service vehicles run in
  *ghost mode* (kinematic along route at flow-model speed, no interaction), so the game
  stays fully playable and lines/sirens stay visible.
- **Gridlock protection**: stuck timeout (90 s) despawn with structured debug log,
  wait-for-cycle breaking, jam notifications (ALGORITHMS §13).
- **Parking**: vehicles despawn into buildings and spawn from them (no parking sim in v1).

## Consequences

- Deterministic regardless of camera; scalable via the cap; good-looking motion near the
  camera; aggregated behavior remains consistent with visible traffic.
- With heavy sampling, congestion may appear slightly "invisible" (agents slowed by
  background volume); acceptable and documented.
- Parking simulation, bikes and pedestrians-as-traffic can be added as new modes/lane kinds.
