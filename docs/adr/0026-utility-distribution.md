# ADR-0026: Utility distribution through the road network

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M5

## Context

Power (plants, distribution via power lines or the road network — to decide), water (pumps
near water, pipes along roads), sewage (treatment plants, along roads), garbage (landfill or
incinerator, trucks driving the road network).

## Options considered

1. **Explicit power lines/pylons + explicit pipes** — more building tools, more UI, more
   failure modes; extra depth but also tedium.
2. **Implicit distribution along all roads (chosen)** — every road carries cables and pipes;
   networks are road connectivity components; placement choices (shoreline pumps, plant
   location, capacity) carry the gameplay.
3. Area-of-effect utilities — contradicts "along roads".

## Decision

- Power, water and sewage networks = road connectivity components (ALGORITHMS §14, §18).
  Each building attaches through its road access point.
- Shortages are allocated deterministically (priority, then network distance from
  the nearest source, then ID) → outer areas fail first, visible in overlays.
- Garbage is collected by trucks (agents or ghost mode) or analytically when traffic is
  disabled.
- Extensibility: the utility graph type accepts extra edges (`UtilityLink`) so explicit
  power lines connecting separate road networks can be added later without redesign.

## Consequences

- Simpler, readable gameplay; utilities overlay shows networks by road coloring.
- Disconnected road networks need their own plants/pumps in v1 (a meaningful constraint).
