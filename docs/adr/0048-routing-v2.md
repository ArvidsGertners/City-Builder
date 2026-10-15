# ADR-0048: Pathfinding and route caching v2

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M5 (router, cache, macro trips), M9 (agents, reroutes); validated by spike S-T
- **Supersedes:** ADR-0011 (keeps the segment-direction link graph, A* with ALT, LRU cache,
  per-tick batching)

## Context

Review findings G-R1–G-R4, G-M6, G-M9, A31, A32, D4, G-D5: ±10 % cost variants made the
free-flow ALT heuristic inadmissible; `variant = hash(trip)` changed daily per commuter and
invalidated the hit-rate assumption; stale-while-revalidate made results depend on cache
contents; 300 searches/tick could cost 15–45 ms wall; landmark recomputation on the async pool
was non-deterministic; one-way traps and failed routes retried forever; job matching used
straight-line estimates across rivers.

## Decision

- **Costs** are integer milliseconds: `free_flow + congestion + turn_penalty + class preference`.
  Variants only **increase** cost: `+0 … +20 %` per link from `hash(link, variant, epoch)`, so
  free-flow ALT bounds stay admissible. `variant = hash(household_or_citizen, purpose) % 4` —
  stable per commuter.
- **Epoch** = 1 game hour (120 sim s). Link costs are snapshotted at the epoch start; every
  route is a pure function of `(graph_version, epoch_costs, key)`. A* breaks ties by
  `(f, −g, LinkId)`.
- **Cache**: LRU of 32 768 entries keyed `(origin, dest, mode, variant)`, each tagged with
  `(graph_version, epoch)`; entries from a previous epoch are misses (no stale serving).
  Entries containing removed links are evicted on topology change. The cache is **saved**
  (ADR-0044) because hits/misses affect budget accounting and therefore departure timing.
- **Budget**: per tick 50 000 *cost units* (1 unit = one settled node; ≈ 0.65 ms on the reference
  CPU, calibrated in spike S-T; a cache hit costs 1 unit). Requests beyond the budget wait FIFO (deterministic delay).
  Searches run in parallel over `SIM_PARTITIONS` with results applied in request order.
- **Landmarks** (16): recomputed tick-sliced (2 Dijkstras per tick) after topology change with
  a deterministic swap tick; between an edge *addition* and the swap the heuristic falls back
  to `euclid / v_max`; after removals stale tables remain admissible.
- **Failures**: negative cache per `(origin, dest, mode, graph_version)`; directed strongly
  connected components (Tarjan) mark buildings in one-way traps (`Problems::NoRoute`).
- **Reroutes**: candidates are vehicles whose next 3 links have congestion > 0.7, at most 20
  searches per tick within the same budget.
- **Job matching** uses routed estimates (bounded Dijkstra per workplace access segment,
  cached per graph version) and rejects matches above 300 sim s (150 game min).
- **Transit**: line leg paths are frozen at line edit and re-routed only on topology change;
  transit-graph cache keys include sorted stop sets with walk times.

## Consequences

- Admissible, deterministic, cache-transparent routing with bounded per-tick cost; spike S-T
  measures hit rate, units-to-time calibration and peak behavior.
