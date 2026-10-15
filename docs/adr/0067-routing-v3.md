# ADR-0067: Routing v3 — sliced batch searches, 15-minute epochs (validated by spike S-T)

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M5 (router, cache, macro trips), M9 (agents, reroutes); validated by spike S-T
- **Supersedes:** ADR-0048. Kept unchanged: link graph per segment direction, integer-ms costs
  with turn penalties and class preference, `+0…+20 %` cost variants (`variant` stable per
  commuter), ALT A* with 16 landmarks and their tick-sliced recompute rules, tie-breaking
  `(f, −g, LinkId)`, LRU of 32 768 entries with removed-link eviction, the saved cache, negative
  cache + SCC one-way traps, congestion-based reroute candidates, routed job matching, transit.

## Context

Spike S-T measured ADR-0048 on a 10 428-link city with 5 000 agents:

- A settled node costs ≈ 130 ns single-threaded in the container, not the assumed 13 ns. So
  50 000 units per tick is ≈ 6.6 ms, not 0.65 ms. Routing p99 was 3–3.8 ms against the 0.8 ms
  gate, and one long search (up to 15 000 settled nodes) can exceed any per-tick time on its
  own, so a count budget alone cannot bound the tick.
- A 1-game-hour epoch (1 200 ticks) reacts too slowly to a forming peak. With 15-minute epochs
  the same 5 000-agent city had 3.5× fewer stuck despawns (31 → 8.9 per 1 000 trips) and 13 %
  more arrivals, and oversaturated nodes fell from 386 to 263. With free-flow costs only,
  arrivals dropped by 30 % — congestion-aware costs matter.
- Mandatory reroutes (missed lane change after 20 s, removed next link) ran synchronous
  searches inside the sequential lane pass.
- Lane plans were recomputed for every cache hit.

## Options considered

1. **Smaller per-tick count budget** — bounds the average, not one long search.
2. **Resumable (sliced) searches in fixed slots, advanced in parallel** — bounded per-tick
   work in deterministic units for any search length; a few ticks of latency.
3. **Background async tasks** — non-deterministic completion tick (rejected by ADR-0043/0061).

## Decision

Option 2, plus a shorter epoch.

- **Epoch** = **15 game minutes** (300 ticks = 30 sim s). Link costs are snapshotted at each
  epoch start from the measured mean speed per link (clamped to `[0.2, 1] × v_free`). Variants
  hash `(link, variant, epoch)` as before.
- **Search slots**: `ROUTE_SLOTS = 8` resumable ALT A* searches (per-slot scratch: one
  24-byte record per link; CSR adjacency; landmark distances interleaved per link). Each tick
  every running slot advances **`ROUTE_SLICE = 1 200` settled nodes**, slots in parallel on the
  task pool. Finished results are delivered in slot order. Slot count and slice are constants,
  so results and delivery ticks do not depend on the thread count.
- **Order**: reroute requests are served first, then trip requests FIFO. Cache hits resolve
  immediately, up to **4 000 hit units per tick**. A hit costs 1 unit, + path length / 8 when
  revalidated; computing a missing lane plan costs the route length. A miss takes an idle slot,
  and duplicate keys attach to the running slot. Measured latency is p50 1 tick and p99 7 ticks
  (0.7 sim s).
- **Reroutes** are asynchronous: the vehicle stops at the line with no connector wanted and
  queues a multi-source search. The sources are the exits of its current lane, each with its
  entry cost. A result whose vehicle moved meanwhile is dropped and re-requested; a reroute
  that finds no route despawns the vehicle as stuck. Reroute candidates per ADR-0048 (congested
  next links, ≤ 20 per tick) use the same queue.
- **Topology change**: running trip searches restart on the new graph; running reroutes are
  re-queued.
- **Lane plans** are cached in the route-cache entry (topology-only data). Removed-link eviction
  also drops their plans.
- **Cache policy**: _filled from the final run (Epoch vs Revalidate, day-2 hit rate)_.

## Consequences

- Routing time per tick is bounded by `ceil(8 / threads) × 1 200 × t_unit` plus hits
  (container: p99 0.70–0.85 ms at 4 threads) independent of search length; route latency is a
  few ticks and reported in diagnostics.
- M5/M9 tests: `routing_slots_deterministic_1_4_8_threads`, `sliced_search_equals_full_search`,
  `reroute_async_delivered_or_despawned`, `epoch_costs_snapshot_every_300_ticks`.
- Revisit if route-queue latency p99 exceeds 30 ticks in the 50k benchmark (raise slots) or the
  min-spec PC exceeds 0.8 ms p99 (lower the slice).
