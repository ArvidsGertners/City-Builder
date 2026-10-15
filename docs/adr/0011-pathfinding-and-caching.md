# ADR-0011: Pathfinding algorithm and route caching

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M5 (macro trips), M9 (agents)

## Context

Tens of thousands of trips per game day (≈ 50–600 route requests per real second at
1×–4× in a 50k city), congestion-aware route choice, one-way roads and turn restrictions,
modes (car, truck, bus, tram, service, walk), incremental network edits, deterministic results.

## Options considered

1. **Lane-level A*** — exact but ~4× larger graph; lane choice is better handled locally.
2. **Contraction hierarchies** — very fast queries but expensive re-preprocessing on every
   edit and with changing congestion weights (customizable CH is complex).
3. **Per-destination shortest-path trees** — great for few destinations; memory O(L) per
   destination, too many destinations here.
4. **A* with ALT landmarks on a segment-direction link graph + LRU route cache with cost
   variants (chosen)**.

## Decision

- `LinkGraph`: links = (segment, direction, mode set); turn edges from lane-graph connectors
  (so turn restrictions/lane availability are respected); costs per ALGORITHMS §12.
- **A\*** with ALT heuristic (16 landmarks, free-flow distances, recomputed in background
  slices on topology change) — admissible because congestion only increases costs.
- **Cost variants**: 4 deterministic ±10 % perturbations; a trip uses variant
  `hash(trip) % 4` → realistic route spread while keeping cacheability.
- **RouteCache**: LRU keyed by `(origin link, dest link, mode, variant)`, 32 k entries,
  values `Arc<[LinkId]>`; topology changes invalidate entries containing removed links
  (reverse index); congestion epochs (15 game minutes) mark entries stale →
  stale-while-revalidate with a per-tick refresh budget.
- **Batching**: requests collected per tick, de-duplicated, solved in parallel over an
  immutable cost snapshot, applied in request order; max 300 searches/tick (overflow next
  tick). Deterministic.
- Bounded Dijkstra (same graph) for coverage, stop access and walking distances.
- Transit: separate small `TransitGraph` (stop × line nodes) with Dijkstra (ADR-0035).

## Consequences

- Predictable per-tick cost, good cache hit rates (homes/workplaces are stable),
  congestion-aware without recomputation storms.
- Routes may lag congestion by up to one epoch; rerouting rules handle severe cases.
- If profiling shows A* dominating, the next step is bidirectional ALT or caching
  per-workplace reverse trees for the top-N destinations (new ADR).
