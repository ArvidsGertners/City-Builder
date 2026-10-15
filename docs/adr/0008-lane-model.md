# ADR-0008: Lane model

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M3

## Context

Lanes must be generated from the road type, stored in the road graph for traffic, support
one-way roads, avenues, bus lanes, tram tracks (in-road and standalone), sidewalks for
pedestrians, and be updated incrementally when segments change.

## Options considered

1. **Lanes as ECS entities** — queryable, but tens of thousands of small entities, and
   traffic hot loops need contiguous data.
2. **Lanes implicit (computed on demand from segment + index)** — no storage, but repeated
   offset computation in hot loops.
3. **Explicit lane polylines in a `LaneGraph` resource (chosen)** — dense storage, fast
   position queries, connectors and conflict zones live next to lanes.

## Decision

- Road types define an ordered **cross-section** (`cb_network::cross_section`): lane kinds
  (Car, Bus, Tram, CarTramShared, Sidewalk; Bike reserved), widths, direction, medians,
  curbs, tracks, verges.
- For each segment, `cb_network::lanes` produces lanes as `Polyline3` with cumulative
  arc length on the trimmed range, plus same-direction neighbor links; lane speed profile
  includes curve speed limits.
- `LaneGraph` holds lanes, junction connectors and conflict zones with generational IDs;
  patched incrementally per dirty segment/node; `version` increments on any patch.
- Routing does not use lanes directly: `LinkGraph` (segment-direction level, ADR-0011) is
  derived from the lane graph; lane choice is local to the vehicle.
- Bus lanes and tram tracks are segment style options that swap lane kinds; standalone tram
  tracks are a road type with only tram lanes. Rails (future trains) and canals (ferries)
  fit as new lane kinds/graphs without changing the model.

## Consequences

- Fast, cache-friendly traffic queries; simple incremental patching.
- Lane IDs are not stable across edits (vehicles re-project by (segment, dir, index, s/len)).
