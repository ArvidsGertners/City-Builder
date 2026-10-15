# ADR-0062: Trip scale — one vehicle trip per six citizens' trips

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** Phase S (S-Time); implemented in M5/M9

## Context

ADR-0042 keeps vehicles at nominal speed in sim time and states capacities in veh/sim-h, while
the calendar runs 30× faster than sim physics (1 game minute = 2 sim s). Spike S-Time
(`spikes/s_time`, 50 000 citizens on 4 × 4 km, every citizen one car trip each way) measured:

| Setting | Commute perceived p50/p95 | Congestion ratio p50/p95 | On time ±30 game min | Peak concurrent trips | p at cap 5 000 |
|---|---|---|---|---|---|
| 1 trip per citizen (as planned) | 66.8 / 560.7 min | 5.3 / 27.9 | 25 % | 45 633 | 0.11 |
| same, capacity × 30 (no congestion) | 17.0 / 37.2 | 1.0 / 1.0 | 100 % | 19 870 | 0.25 |
| **trip scale 6** + tuned shifts | **18.6 / 39.5** | **1.01 / 2.83** | **92.6 %** | **3 071** | **1.0** |

One car trip per citizen compresses a whole morning commute into a few sim minutes: physical
lanes cannot carry it, and even without congestion the concurrent trip count is 4× the agent
cap. Realism alone (households sharing cars, transit and walking, students) explains a factor of
≈ 2; the rest is the price of the time compression.

## Options considered

1. **Trip scale k (chosen)** — each simulated vehicle trip represents k citizens' trips of the
   same purpose; capacities stay physical (IDM-validated). Pace unchanged; congestion stays
   meaningful (bad networks jam, good ones flow); every trip can be an agent at 50k.
2. **Capacity × k in the flow model only** — same congestion numbers, but agents would still be
   physical vehicles at 1 per trip: concurrency (p = 0.25) and visual density do not fit.
3. **Different sim/game ratio** (e.g. 1 game minute = 4 sim s) — halves trip durations on the
   clock, but doubles the visible vehicle speed the user approved (2× at a 24-minute day), and
   does not fix capacity by itself. Not chosen without the user's decision.

## Decision

- **`TRIP_SCALE = 6`** (constant in `citizens.ron`, not a player setting): trip generation
  produces one vehicle-trip entity per 6 citizen trips of the same household cohort, purpose and
  departure window; the entity's schedule is the cohort's. Population, jobs, happiness and
  statistics still count every citizen; transit ridership counts entity riders × 6.
- Commute estimates, demand and happiness use the entity's experienced time for all citizens it
  represents.
- Shift-start data (ADR-0042 parameters are data): commercial N(08:45, σ 75 game min)
  truncated [06:30, 11:30], 15 % evening; industry N(07:30, σ 45) truncated [06:00, 11:00],
  10 % night; schools 08:00 ± 20; gravity job matching with 1.5 km decay.
- Agent sampling (ADR-0047) remains as a safety valve; at 50k citizens `p = 1`.

## Consequences

- A 50k city shows ≈ 3 000 vehicles at the morning peak; small towns proportionally fewer.
- Two S-Time gates are missed and accepted: earliest 5 % of morning departures at 04:05 (gate
  04:30) because commutes span 1.5–3.3 game hours on the clock (ADR-0042, assumption B1); and
  peak/off-peak 2.6 over 10:00–15:00 (gate 3) because school returns at 14:30 fall into that
  window; over 10:00–13:00 it is 4.6. Night share (00–04) is 2.7 % (gate ≤ 10 %).
- Revisit if M9 traffic with agents shows unrealistic emptiness or density, or if the user
  prefers a different pace (option 3).

## Amendments

- 2026-10-15 (user decision): the user wants a **realistic number of visible cars**. Trip scale 6
  is kept because it is what produces a realistic count: a 50k city peaks at ≈ 3 100 vehicles
  (6.2 % of the population on the road at once, comparable to a real town's rush hour), and
  busy arterials run near capacity at the peak, so on-screen density looks like rush-hour
  traffic. One car trip per citizen would put ≈ 45 000 cars (90 % of residents) on the road
  simultaneously. Calibration target (owned by `trip_scale_50k_peak`, M5/M9): peak concurrent
  vehicles 4–7 % of population and arterial V/C 0.8–1.2 at the morning peak on a reasonable
  network; `TRIP_SCALE` is data and is retuned against this target, never to "thin out" traffic.
