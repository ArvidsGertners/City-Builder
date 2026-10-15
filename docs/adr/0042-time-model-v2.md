# ADR-0042: Time model v2 — fixed sim/game ratio, day length as pure pacing

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M1 (driver), M5 (calendar, schedules); validated by spike S-Time
- **Supersedes:** ADR-0006

## Context

The independent plan review (docs/review/, findings C-4, G-M1, G-M2, D1, D2) showed that
ADR-0006 was internally inconsistent: traffic capacities "per hour" and EMA/epoch lengths
"in game minutes" mixed with vehicles moving in sim seconds at 1 game minute = 1 sim second
(≈ 27× unit error); commutes lasted 2–7 game hours, so morning peaks held 15–25k concurrent
trips against a 5k agent cap; the 12/24/48-minute day options rescaled every trip in game
time and changed balance; tick arithmetic per frame was wrong; and `max_ticks_per_frame = 12`
allowed a spiral after hitches.

## Options considered

1. **Keep ADR-0006, tune numbers** — does not remove the unit ambiguity or the dependence of
   game behavior on the day-length setting.
2. **Slower calendar only** (e.g. 48-minute days) — halves trip game-durations but makes seasons
   and progression slow; still couples behavior to the setting.
3. **Fixed sim/game ratio + day length as real-time pacing only (chosen)** — all game rules are
   defined in sim/game time with one constant ratio; the day-length setting only changes how
   fast the whole simulation runs relative to the wall clock.

## Decision

- **Three time bases.**
  - *Real time*: only the driver reads it.
  - *Sim time* (`SimSeconds`): physics time. **Fixed tick `dt = 0.1 sim s`.** Vehicles move at
    their nominal speeds in sim time (50 km/h = 13.9 m/s).
  - *Game time* (calendar): **1 game minute = 2 sim seconds** (constant
    `SIM_SECONDS_PER_GAME_MINUTE = 2.0`) → 20 ticks per game minute, 1 200 per game hour,
    28 800 per game day.
- **Day length (16 / 24 / 32 / 48 real minutes, default 24) is pacing only**: sim seconds per
  real second = `(48 / L) × speed` (speed ∈ {0, 1, 2, 4}); ticks per real second at 1× =
  `480 / L` (L = 24 → 20 Hz). Simulation results are **identical for every L** (the setting
  lives in `Settings`, not `SimConfig`). At L = 24 vehicles visibly move at 2× real speed —
  the "diorama" pace (assumption B1).
- **Perceived commute** (UI, happiness, demand) = `sim_seconds × 0.1` minutes (= game minutes
  × 0.2): a 200 sim-s, 2 km drive reads as 20 minutes.
- **Units rule.** Every traffic flow/capacity is in **vehicles per sim-hour**; every EMA
  half-life, cost epoch, timeout and growth rate is in **sim seconds** or explicit game
  units; economy, incidents and demand are per **game hour/day**. Field names carry the unit
  (`_per_sim_h`, `_sim_s`, `_game_h`). Values: flow EMA half-life 120 sim s, cost epoch = 1
  game hour (120 sim s), stuck timeout 90 sim s.
- **Schedules** are computed in game time: arrival target − routed duration estimate (game
  min) − slack N(10, 5) game min. Shift starts: commercial N(08:30, σ 90 game min) truncated
  [05:30, 11:30] with 15 % evening shifts; industry N(07:00, σ 60) with 10 % night shifts;
  schools 08:00 ± 20 min. Spike S-Time validates peak shape and concurrency; parameters are
  data (`citizens.ron`).
- **Driver.** Accumulates `real_dt × (48/L) × speed`; runs `SimTick` while `acc ≥ dt`, at
  most `cap = max(1, ceil(1.5 × nominal_ticks_per_frame))` ticks per frame (1×: 1, 4× at
  L = 24: 2); the excess is dropped (game slows briefly, no spiral). If `SimEdit` + ticks
  would exceed 7 ms on this frame, one tick is deferred (accumulator kept). Deferral changes
  only *when* ticks run in real time, never their content → deterministic.
- **Structural completeness.** `SimEdit` (commands + structural derived updates) always
  completes fully before the next `SimTick`; if it is expensive the driver defers ticks, never
  splits structural work across ticks. Each applied command records `apply_tick` (the tick
  count at application); replay runs `SimEdit` exactly before that tick.
- **Sub-rate phase table** (ticks per game hour = 1 200): every sub-rate system has a fixed
  tick offset and slice count (ARCHITECTURE §5.3) so no tick carries more than 0.6 ms of
  sub-rate work; a test asserts the nominal schedule.
- "Lock time of day" stays a **presentation** option (lighting frozen; calendar continues) —
  assumption B3, to be confirmed by the user.

## Consequences

- No unit ambiguity; balance and determinism independent of the day-length option.
- Shorter days cost more CPU at high speed (L = 16 at 4× = 120 ticks/s); settings warn.
- Vehicles visibly move faster than real at short day lengths (accepted stylization).
