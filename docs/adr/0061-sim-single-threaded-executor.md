# ADR-0061: Single-threaded executor for the sim schedules

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M1

## Context

ARCHITECTURE §12 planned Bevy's multi-threaded executor for system-level parallelism in
`SimEdit`/`SimTick`, with heavy work additionally parallelised *inside* systems over
`SIM_PARTITIONS = 64` (ADR-0043). M1 measurements in the container (4 cores, release build)
of the skeleton `SimTick` (8 chained sets, 7 explicit `ApplyDeferred` sync points, one system):

| Executor | 1 000 ticks | per tick |
|---|---|---|
| multi-threaded | 63–79 ms | ≈ 70 µs |
| single-threaded | 0.83 ms | ≈ 0.8 µs |

The multi-threaded cost is thread hand-off at every sync point, not work. At 4× on a 16-minute
day (120 ticks/s) it would spend ≈ 8 ms per real second doing nothing, it grows with the
number of sets/sync points, and it made the M1 test `driver_runs_1000_ticks_in_lt_100ms`
flaky. The per-tick work that matters (routing, car following, reach, fields) is already
designed as partition-parallel inside single systems, so inter-system concurrency buys little.

## Options considered

1. **Keep the multi-threaded executor** — concurrency between independent systems (e.g.
   weather and stats); ~70 µs fixed overhead per tick and per schedule run; order between
   non-conflicting systems varies run to run (harmless with ambiguity = Error, but harder to
   profile).
2. **Single-threaded executor for `SimEdit` and `SimTick` (chosen)** — ~1 µs overhead,
   one fixed system order per build (stable profiles, simpler reasoning); parallelism only
   where it pays, via `ComputeTaskPool::scope` over the 64 partitions. Loses incidental
   concurrency between small systems.
3. **Fewer sync points** (drop `ApplyDeferred` between sets) — reduces but does not remove the
   overhead, and weakens the explicit-structure rule.

## Decision

- `SimEdit` and `SimTick` use `SingleThreadedExecutor` (set in
  `cb_sim::schedule::add_sim_schedules`).
- Parallelism in sim code is **only** intra-system: `ComputeTaskPool::scope` over
  `SIM_PARTITIONS`, merged in partition order (unchanged, ADR-0043).
- Ambiguity detection stays `Error` and sets stay chained with explicit `ApplyDeferred`, so the
  order remains fully specified; the shuffle test (`schedule_shuffle_same_hash`, cb_sim feature
  `debug_schedule`, enabled by `cb_game/dev`) still guards against hidden order dependence.
- Game schedules (`Update`, rendering) keep Bevy's multi-threaded executor.

## Consequences

- Negligible per-tick overhead; `headless_bench` and the perf overlay measure real work.
- A heavy system that is not partition-parallel serialises the tick: the per-tick budget table
  (PERFORMANCE §6) already assumes partitioned routing/IDM; review any new system > 0.3 ms.
- Revisit if profiling at M9 shows several independent ≥ 0.3 ms systems that cannot be
  partitioned (then: a dedicated multi-threaded sub-schedule for those sets only).
