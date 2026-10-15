# ADR-0016: Citizen simulation granularity

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M5

## Context

≥ 50k citizens with households, home, workplace, education, happiness and daily schedules
tied to the day/night cycle; they generate trips; their needs drive migration. Must fit the
CPU budget at 4× and stay deterministic.

## Options considered

1. **Every citizen an ECS entity with per-tick agent logic** — idiomatic but wasteful
   (most citizens are idle most of the time) and heavy on save/load and iteration order.
2. **Statistical per-building populations** — very cheap, but no individual trips, no
   per-person education/jobs, weak feedback.
3. **Household + compact citizen records in SoA tables with an event-driven schedule
   (chosen)**.

## Decision

- `Population` resource: `households` and `citizens` in dense generational stores; citizen
  record ≤ 32 bytes (household, age group, education, workplace/school, shift, state,
  health, commute estimate).
- Behavior is **event-driven**: a minute-resolution `TimeWheel` holds upcoming activities;
  per tick only due activities are processed (trip generation, arrivals). No per-citizen
  per-tick work.
- Household-level decisions: move in/out, car ownership, shopping/leisure, happiness.
  Person-level: job/school, education, shift, commute.
- Daily/hourly evaluations (happiness, job matching) are sliced over ticks.
- Vehicles carry trips, not citizens: a car trip references one or more citizens; transit
  riders are counted at stops/vehicles.
- Visible individuality: names and short bios generated from seed + CitizenId on demand in
  the info panel.

## Consequences

- O(events) cost; 50k+ citizens fit comfortably; easy to save (tables) and deterministic
  (stable IDs, sorted processing).
- No individual pathing for pedestrians beyond visual agents.
