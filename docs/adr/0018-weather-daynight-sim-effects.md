# ADR-0018: Weather and day/night effects on the simulation

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M5 (schedules), M11, M12

## Context

The simulation must react to time of day (commute peaks, less traffic at night,
commercial activity by time) and weather (slower vehicles, snow maintenance cost, storm fire
risk, slight happiness effects), with a forecast and options to disable or lock weather.
Visuals are fully driven by the same state.

## Options considered

1. **Visual-only weather** — misses the brief.
2. **Physically simulated weather (fronts, humidity)** — unnecessary complexity.
3. **Seasonal Markov chain with continuous intensities (chosen)**.

## Decision

- Weather is **sim state** (`WeatherState`, saved): kind, intensity, transition progress,
  wind, temperature, snow cover (accumulates in snow, melts above 2 °C), wetness (rain
  raises, drying by sun/wind). Advanced in `TickSet::Weather`.
- The schedule for the next 48 game hours is pre-rolled deterministically from
  `SimRng::for_event(Weather, day)` using season probability tables (GAME_DESIGN §11) with
  min/max durations; the forecast UI shows it.
- Effect factors exposed by `WeatherSimPlugin` as a `WeatherEffects` resource:
  vehicle speed and gap multipliers, road upkeep multiplier, fire-risk multiplier,
  lightning strikes (storms: Poisson process; targets weighted by building height, 30 %
  ignite unless fire coverage high), happiness modifier, solar/wind output factors, heating
  demand.
- Day/night effects come from the calendar: citizen schedules (shifts, shopping peaks),
  commercial opening hours (sales only 7:00–21:00 for L1–L2, extended for L3), industry
  shifts, reduced trip rates at night, street lamps reduce crime slightly at night.
- Options (`SimConfig`, saved): weather enabled (off → always clear; seasons still advance
  visually), lock weather kind. Presentation options: reduce weather effects (particle
  density, flashes), lock time-of-day visuals (ADR-0006).

## Consequences

- Weather matters for gameplay but never dominates (small factors); deterministic and
  forecastable.
- Lighting/FX read the same state → visuals always match simulation.
