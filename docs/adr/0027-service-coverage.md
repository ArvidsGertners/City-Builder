# ADR-0027: Service coverage measured along the road network

- **Status:** Superseded by ADR-0056
- **Date:** 2026-10-15
- **Milestone:** M6

## Context

Coverage areas for police, fire, health, education (and parks, transit stops) must be
measured along the road network, update incrementally, respect capacity, and drive
gameplay (crime, fire risk, sickness, education) and overlays.

## Options considered

1. **Euclidean radius** — explicitly rejected by the brief.
2. **Per-building shortest path to the nearest facility on demand** — exact but too many
   queries per update.
3. **Bounded Dijkstra per facility over segments, coverage samples per segment (chosen)**.

## Decision

- Bounded Dijkstra from each facility's access point over the undirected segment graph up
  to `reach_m` (travel distance), writing up to 8 samples per reached segment; falloff
  `1 − smoothstep(0.6, 1.0, d/reach)`; scaled by budget and capacity utilization.
- Buildings sample coverage at their road access (segment, t).
- Incremental: each facility stores its reached segment set; dirty segments in or adjacent
  to it trigger recomputation of only that facility; a test asserts incremental == full.
- Vehicle response times use the *directed* link graph with live costs (one-ways and
  congestion matter for dispatch), separate from coverage.

## Consequences

- Correct "across the river" behavior, cheap updates, clear overlays (roads colored by
  coverage, buildings tinted by their sample).
