# ADR-0006: Fixed-timestep simulation and the time model

- **Status:** Superseded by ADR-0042
- **Date:** 2026-10-15
- **Milestone:** M1 (driver), M5 (calendar)

## Context

Requirements: fixed-timestep sim with pause/1×/2×/4×, single-step for debugging, smooth
rendering, commute peaks tied to the day/night cycle, configurable cycle length, editing
while paused. Vehicles must move at believable speeds while a game day lasts minutes.

## Options considered

1. **Bevy `FixedUpdate`** — built-in, but couples sim rate to `Time<Fixed>` semantics,
   harder to run "edit while paused" separately, and to step deterministically in tests.
2. **Custom schedules + own driver (chosen)** — explicit control of tick count per frame,
   catch-up limits, stepping, and two schedules (edit vs. tick).
3. Variable timestep — non-deterministic; rejected.

## Decision

- **`SimTick`**: fixed `dt = 50 ms` of *sim time* (20 Hz). The driver accumulates
  `real_dt × speed` (0, 1, 2, 4) and runs `SimTick` while `acc ≥ dt`, at most
  `max_ticks_per_frame = 12`; if exceeded, the remainder is dropped (game slows instead of
  spiraling). `SimStepRequest` runs exactly one tick while paused.
- **`SimEdit`**: runs once per frame regardless of speed; applies commands and structural
  derived updates (ARCHITECTURE §5.1).
- **Two clocks.** *Sim time* (ticks × 50 ms) governs physics: vehicle motion, dwell
  times, fire growth. *Game time* (calendar) advances `ms_per_tick` game-ms per tick:
  default 1 game minute per sim second (day = 24 real minutes at 1×), configurable
  (12/24/48 min) via `SimConfig::day_length_real_min` (saved with the city).
  Consequence: a 170 s drive spans ~170 game minutes. Gameplay uses **perceived commute
  time** = sim seconds × 0.1, and departures are scheduled backward from desired arrivals
  in game minutes (GAME_DESIGN §2).
- **Sub-rates**: run conditions `every_n_ticks(n, phase)` (20 → 1 Hz) and calendar
  messages (`HourElapsed`, `DayElapsed`); daily/hourly heavy work is sliced over ticks.
- **Interpolation**: agents store previous/current tick state; presentation renders with
  `α = acc / dt`.
- "Lock time of day" is a **presentation** setting (lighting frozen); the sim calendar
  continues so the economy and schedules keep working.

## Consequences

- Deterministic stepping in tests (`Scenario::run_ticks`), predictable performance at 4×
  (80 ticks/s), clear separation of editing and simulation.
- Time compression is an explicit design rule documented for tuning (R12).
