# ADR-0068: Macro commute model — daily shortest-path assignment with BPR day costs

- **Status:** Accepted
- **Date:** 2026-10-17
- **Milestone:** M5

## Context

M5 needs commute estimates (job matching rejects > 300 sim s, happiness commute factor,
departure times), link volumes (noise, BPR costs) and one-way trap detection before traffic
agents exist (M9). ADR-0067 decides how *agent trips* are routed (8 resumable ALT A* slots,
15-minute epochs, revalidating route cache); it does not say how the aggregate model before and
beside agents assigns commuters. Constraints:

- Results must be bit-identical across OSes and across save/load (ADR-0043, ADR-0044): the
  model may only depend on saved state and on state derivable from it, never on when a sliced
  search happened to finish.
- Tick budget (PERFORMANCE): no per-day batch spike; a 50k city has ~20k households.
- Job matching runs in 200 slices per hour and needs estimates to every open workplace.

## Options considered

1. **Route every commute through the ADR-0067 slots and cache** — one machinery for M5 and
   M9. But results then depend on cache contents and slot timing (the cache and the slot queue
   would both have to be saved), 15-minute epochs churn costs for a model whose volumes change
   daily, and job matching would need one A* per (seeker, candidate) pair.
2. **Daily all-or-nothing assignment with shortest-path trees** — one bounded tree per home
   reaches all of its workers' workplaces (early stop once all are settled); job matching grows
   one tree per seeker home bounded by the maximum commute and reads every candidate from it.
   Costs are per-day BPR costs from the previous day's volumes, so every tree is a pure function
   of (graph, day costs) and can be recomputed at any time with the same result.
3. **Straight-line estimates until M9** — cheap, but ignores rivers without bridges, one-ways and
   congestion, and the M5 acceptance (routing, departure histogram) cannot be met.

## Decision

Option 2, in `cb_sim::traffic` (bridge) and `cb_traffic::tree` (pure search).

- **Graph**: one link per (segment, driving direction with car lanes), nodes and segments in ID
  order; speed = road type limit, capacity = car lanes × capacity per lane, turn penalties from
  `cb_traffic::graph`. Rebuilt in `EditSet::Connectivity` whenever `NetworkChanged` was written
  (and after load); per-link accumulators survive rebuilds by link key `(segment id << 1) | dir`.
- **Access** (ALGORITHMS §14): the lot's frontage midpoint projected onto the nearest of its
  plot's frontage segments. A trip leaves towards the ends the segment can be driven to and
  arrives from the ends that drive past the building, each with the partial segment time; plus
  `terminal_sim_s` (10) per trip end. Access is cached and cleared on every network or zoning
  edit (it depends on nothing else).
- **Day costs**: `t = t_free × BPR(V_peak, C)` (factor clamped to `[1, 5]`), with
  `V_peak = car commuters on the link / trip_scale × peak_hour_share (0.3) × 30` vehicle
  entities per sim hour (agents count entities, ADR-0062). Fixed for the game day; recomputed
  at the day boundary from that day's totals and after a graph rebuild.
- **Daily assignment**: every `FLOW_PERIOD = 2` ticks one of 14 400 slices; slice `k` handles the
  households with `id % 14 400 == k` (direct lookups, no scan). It is the one phase-table row
  that runs beside other slots (ARCHITECTURE §5.3, nominal ≤ 0.05 ms). Per home one tree with early stop;
  each worker gets `commute_sim_s` (or `NO_ROUTE`), its tree path plus both partial access links
  add the household's car share (`min(1, cars / workers)`), and its departure
  `shift start − commute_sim_s / 2 (game min) − slack N(10, 5)` enters a 48-bin histogram.
  The published `CommuteStats` (departures, commuters, no-route count, perceived p50/p95) and
  link volumes are those of the last complete day.
- **Job matching**: estimates from one tree per seeker home bounded by `max_sim_s − 2 ×
  terminal` (300 sim s total); score `−estimate − 0.2 × mismatch × max_sim_s`; a hire samples the
  shift start (ADR-0062 data in `citizens.ron`, `SimRng` purpose 4) and stores the estimate.
- **Traps**: nodes outside the largest strongly connected component; a building whose leaving
  ends are all trapped has no route (`traffic::building_no_route`), and so does a worker whose
  workplace no tree reaches.
- **Feedback**: happiness commute score = mean over workers of `1 − (perceived − 10) / 50`
  clamped (no route = 0, no workers = neutral 0.7); noise line sources use
  `base_volume(kind) + mean daily segment volume` (both directions).
- **Saved state** (M5.8, ADR-0044 traffic placeholders): today's and the last day's link
  totals by link key, the day accumulators and the published stats. Graph, access cache and
  trees are derived.

## Consequences

- Deterministic and load-safe by construction; no route cache or slot queue to save in M5.
- All-or-nothing assignment ignores route spreading (variants) and evening asymmetry; the noise
  volume mirrors morning trips for the way back. M9 agents replace link volumes with measured
  EMAs (ALGORITHMS §13) and commute estimates with experienced times; the trees stay for job
  matching and for sampled-out trips.
- A household's commute is refreshed once per day; a road edit is felt by job matching
  immediately and by existing commuters within a day.
- Revisit if the per-slice tree cost exceeds the tick budget at 50k (bound the assignment tree,
  or move it to the 64 fixed partitions, ADR-0061) or if M9 measurements diverge from the day
  costs by more than the 10 % `ghost_vehicle_eta_matches_flow_model` tolerance.
