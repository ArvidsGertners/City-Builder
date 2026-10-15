# ADR-0035: Public transport model

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M10

## Context

Buses (depot, player-placed stops along roads, player-drawn lines as ordered stop lists,
sharing lanes with traffic, optional bus lanes) then trams (tracks in the road or standalone,
stops, depot, lines). Line management UI, ridership stats, costs/income; citizens choose
transit by time/cost/walking distance; transit reduces car traffic and raises land value;
loose schedules (bunching acceptable).

## Decision

- Stops are entities attached to a segment position/side; lines are entities with ordered
  stop lists, color, vehicle target, depot, fare.
- Each consecutive stop pair's path is routed on the mode's link graph (bus: car graph with
  bus-lane preference; tram: tram-lane subgraph) and cached per line; line invalid if any
  leg is unreachable (UI shows the broken leg).
- Transit vehicles are agents in `TrafficWorld` (`Bus`, `Tram`) dispatched from the depot,
  evenly spaced at start, no timetable (bunching allowed); dwell time = base + boarding
  per passenger; capacity limits boarding.
- Passengers: trips choosing transit (mode choice, ALGORITHMS §16) add `WaitingGroup`s
  (destination stop, count) to the origin stop; vehicles board groups whose destination is
  downstream on the line; transfers handled by re-queuing at the transfer stop.
  Riders are counted, not individually simulated; pedestrians at stops are visual.
- Trams: articulated sections follow the path at fixed offsets; tram lanes can be
  `CarTramShared` (cars and trams share; trams follow IDM like other agents) or dedicated.
- `TransitGraph` for routing (ADR-0011); stop access lists per building (≤ 600 m walk via
  bounded Dijkstra), recomputed when stops or nearby segments change.
- Stats per line/stop: riders/day, history, occupancy, headway, income (fares), cost
  (vehicle-hours × cost); land value bonus near stops scales with line frequency.
- Traffic disabled → transit vehicles in ghost mode (ADR-0010).
- Extensible to metro/train/ferry as further modes with their own link graphs.

## Consequences

- Cheap passenger model that still produces meaningful ridership and car-trip reduction.

## Amendments

- 2026-10-15 (plan review): trips carry an itinerary `legs: SmallVec<[TransitLeg; 3]>`
  (line, board stop, alight stop); non-circular lines run ping-pong; when a stop is full (40 bus
  / 80 tram waiting) new transit trips re-choose mode with a −5 min penalty on transit; buses
  stop in the curb lane (blocking) or in the bus lane where present, trams in their lane; line
  leg paths are frozen at edit (ADR-0048); visual walkers spawn from `WaitingGroup` changes near
  the camera (ADR-0047); land-value bonus near stops
  `0.10 × min(1, 600 s / headway) × (1 − smoothstep(0.6, 1, walk_m / 400))`.
