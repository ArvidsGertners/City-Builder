# ADR-0056: Service coverage v2 — incremental reach, global capacity allocation

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M6
- **Supersedes:** ADR-0027 (keeps network-distance coverage via bounded Dijkstra per facility,
  sampling at road access, directed ETA for dispatch)

## Context

Review findings A7, G-M3: "capacity-aware saturating sum" was undefined, and once capacity
couples facilities, "incremental == full recompute" cannot hold per emitter.

## Decision

- **Reach** (incremental, per facility): bounded Dijkstra on the undirected segment graph up
  to `reach_m`; per reached segment up to 8 distance samples; `c_fb = budget_factor ×
  falloff(d / reach)`, `falloff(x) = 1 − smoothstep(0.6, 1.0, x)`. Recomputed only for facilities
  whose reached set touches dirty segments.
- **Allocation** (global, once per game hour, sliced over 10 ticks): for building `b` and
  facilities `f` reaching it: `w_fb = c_fb / Σ_f' c_f'b`; `load_f = Σ_b demand_b × w_fb`;
  `util_f = min(1, capacity_f / load_f)`; `coverage_b = 1 − Π_f (1 − c_fb × util_f)`.
- Display overlay uses `max_f c_fb` (reach) and a capacity indicator per facility.
- Tests: incremental reach == full reach for random edit sequences; allocation is one pure
  function used in both paths; capacity split proportional.

## Consequences

- Well-defined capacity behavior; the incremental optimization stays provably correct.
