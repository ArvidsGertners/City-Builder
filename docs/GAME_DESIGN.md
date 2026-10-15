# Game Design & Initial Balance

Status: **Draft values for implementation**. Rules are normative; numbers are starting
points, tuned during M5–M7 and M16 via headless balance runs (`headless_bench --balance`).
Source of truth at runtime: `crates/cb_sim/data/*.ron` (keep this doc in sync).

## 1. Core loop

Empty map with 2–3 outside road connections and starting money → build roads → draw plots
along roads (R first) → connect power and water → houses grow from demand → jobs needed →
commercial/industrial → services, utilities, budget, traffic and transit management →
population milestones unlock new tools and building levels.

Lose-ish condition: sustained bankruptcy (warning dialogs, forced loans); no hard game over
in v1 — the city stalls instead. Sandbox: unlimited money, everything unlocked.

## 2. Time

| Quantity | Default | Notes |
|---|---|---|
| Sim tick | 50 ms sim time (20 Hz) | ADR-0006 |
| Game day | 24 real minutes at 1× (options: 12 / 24 / 48) | 1 game minute per sim second at default |
| Speeds | Pause / 1× / 2× / 4× | |
| Season | 4 game days (options: 2–8) | Spring, Summer, Autumn, Winter |
| Year | 4 seasons | Ageing/education steps per season/year |
| Week | 4 days (leisure/park trips on day 4) | No weekday names in v1 |

**Time compression.** Vehicles move at real speeds in sim seconds while the calendar runs
~60× faster than real time, so a 2 km drive (~170 sim s) spans ~170 game minutes on the
clock. Rules that follow from this (ADR-0006):
- Departures are scheduled backward from the desired arrival using the trip's estimated
  duration in *game* minutes, so arrivals cluster around shift starts; departures spread
  over the hours before (visible morning traffic from ~5:00, peak 6:00–8:00).
- Gameplay-facing commute time is **perceived time** `perceived_min = sim_seconds × 0.1`
  (170 s → 17 min), independent of the day-length setting. Happiness, demand and the UI use
  perceived minutes only.

## 3. Road types

| Type | Lanes | Width | Speed | Cap./lane/h | Cost/m | Upkeep/m/day | Max grade | Min radius | Unlock |
|---|---|---|---|---|---|---|---|---|---|
| Dirt path | 1+1 (narrow) | 6 m | 30 km/h | 450 | 4 | 0.010 | 14 % | 8 m | start |
| Street | 1+1 | 12 m (incl. 2×2 m sidewalks) | 50 km/h | 800 | 22 | 0.040 | 10 % | 12 m | start |
| One-way street | 2 | 12 m | 50 km/h | 800 | 24 | 0.045 | 10 % | 12 m | 600 |
| Avenue | 2+2 + 2 m median | 22 m | 60 km/h | 900 | 60 | 0.120 | 8 % | 25 m | 1 200 |
| One-way avenue | 3 | 16 m | 60 km/h | 900 | 50 | 0.100 | 8 % | 25 m | 5 000 |
| Tram track (standalone) | 1+1 tram | 8 m | 50 km/h | — | 70 | 0.150 | 6 % | 20 m | 10 000 |

Modifiers: slope `cost × (1 + 2·grade)`, earthwork 1.5 / m³, bridge `× 4` per meter +
1 200 per pier (+40 per m of pier height), tram tracks in road +45/m, bus lanes +4/m.
Upgrade cost = new − 50 % of old; downgrade refunds 0.
Snow increases road upkeep by up to +60 % (proportional to snow cover).

Surfaces (per segment): dark asphalt (default), light asphalt (+2/m), cobblestone (+8/m,
−5 km/h, +land value), dirt (dirt path only). Decorations (per segment, per side):
sidewalks (default on streets+), curbs, grass strip (+3/m, +LV), tree row (spacing 8–24 m,
45 per tree, +LV, −noise), street lamps (spacing 20–50 m, 60 per lamp, −crime slightly,
night lighting).

## 4. Zones and building levels

| Zone | Levels | Typology by level | Floors | Capacity |
|---|---|---|---|---|
| Residential | 1–5 | L1 cottage · L2 detached house · L3 row house / duplex · L4 small apartment · L5 apartment block | 1–2 · 2 · 2–3 · 3–5 · 5–8 | households: 1 · 1–2 · 2–4 · 6–16 · 16–48 (by footprint) |
| Commercial | 1–3 | L1 corner shop · L2 shop row / offices · L3 department store / office tower | 1–2 · 2–4 · 4–10 | jobs 4–10 · 10–40 · 40–160 |
| Industrial | 1–3 | L1 workshop · L2 factory hall · L3 plant with tanks/chimneys | 1 hall · 1–2 · 2 + stacks | jobs 8–20 · 20–60 · 60–200 |

Lot widths (frontage): R 10–18 m (target 13), C 14–30 m (target 20), I 24–60 m (target 36);
max depths R 45, C 50, I 80 m; min area R 120 m², C 200 m², I 600 m².
Level caps by lot area: R L4 needs ≥ 400 m², R L5 ≥ 700 m²; C L3 ≥ 600 m²; I L3 ≥ 2 000 m².

Leveling thresholds (score 0..1): up `[0.35, 0.5, 0.62, 0.75]`, down = up − 0.15.
Education requirements for jobs: C L1 edu 0+, C L2 edu 1+, C L3 edu 2+ (60 %),
I L1 edu 0, I L2 edu 1, I L3 edu 2 (40 %).

## 5. Services

| Service | Building | Reach (network) | Capacity | Vehicles | Cost | Upkeep/day | Unlock |
|---|---|---|---|---|---|---|---|
| Police | Police station | 900 m | 1 500 residents | 3 cars | 9 000 | 600 | 600 |
| Police | Police HQ | 1 600 m | 6 000 | 8 | 30 000 | 1 800 | 10 000 |
| Fire | Fire station | 1 200 m | — | 3 trucks | 10 000 | 650 | 600 |
| Fire | Fire HQ | 2 000 m | — | 8 | 32 000 | 1 900 | 10 000 |
| Health | Clinic | 800 m | 1 200 | 2 ambulances | 8 000 | 550 | 250 |
| Health | Hospital | 1 800 m | 8 000 | 8 | 40 000 | 2 400 | 5 000 |
| Education | Elementary school | 900 m | 300 students | — | 10 000 | 500 | 250 |
| Education | High school | 1 500 m | 800 | — | 25 000 | 1 300 | 2 500 |
| Education | University | city-wide | 2 000 | — | 70 000 | 3 500 | 10 000 |
| Leisure | Small park / Plaza / Large park | 400 / 400 / 800 m | — | — | 1 500 / 3 000 / 9 000 | 60 / 120 / 300 | 0 / 250 / 5 000 |
| Admin | Town hall | — | — | — | (free, placed at start) | 200 | start |

Budget sliders 50–150 % scale effectiveness (capacity, reach × 0.8–1.1, vehicles) and upkeep.

## 6. Utilities

| Building | Output / capacity | Placement | Pollution | Cost | Upkeep/day | Unlock |
|---|---|---|---|---|---|---|
| Diesel generator | 2 MW | anywhere | medium, noise | 4 000 | 300 | start |
| Wind turbine | 0–1.5 MW by wind | hills preferred (+height bonus) | noise low | 5 000 | 150 | 1 200 |
| Coal power plant | 25 MW | anywhere | high | 30 000 | 1 600 | 2 500 |
| Solar farm | 0–6 MW by sun & clouds | anywhere, large footprint | none | 22 000 | 400 | 5 000 |
| Gas power plant | 40 MW | anywhere | medium | 55 000 | 2 600 | 20 000 |
| Water pump | 300 m³/h | shoreline | — | 3 500 | 200 | start |
| Water tower (groundwater) | 120 m³/h | anywhere | — | 4 500 | 250 | 2 500 |
| Sewage outlet | 300 m³/h | shoreline | water pollution high | 2 500 | 120 | start |
| Sewage treatment plant | 800 m³/h, 85 % treatment | shoreline | low | 28 000 | 1 200 | 2 500 |
| Landfill | 400 t storage, 2 trucks | anywhere | ground medium | 6 000 | 350 | start |
| Incinerator | 60 t/day, 4 trucks, +6 MW | anywhere | high | 35 000 | 1 400 | 5 000 |

Consumption (per level-1 household / per job): power 1.2 kW / 0.8 kW, water 0.25 / 0.15 m³/h,
sewage 90 % of water, garbage 2.5 / 1.5 kg/day; scale ×1.0–1.6 with level. Winter heating:
power +30 % × snow/temperature factor.

## 7. Public transport

| Item | Capacity | Speed | Cost | Upkeep | Fare default | Unlock |
|---|---|---|---|---|---|---|
| Bus depot | 20 buses | — | 12 000 | 500/day | — | 1 200 |
| Bus stop | 40 waiting | — | 300 | 20/day | — | 1 200 |
| Bus | 60 | lane speed | — | 45 per vehicle-hour | 2.00 | 1 200 |
| Tram depot | 12 trams | — | 30 000 | 1 100/day | — | 10 000 |
| Tram stop | 80 waiting | — | 900 | 40/day | — | 10 000 |
| Tram (3 sections) | 180 | ≤ 50 km/h | — | 90 per vehicle-hour | 2.50 | 10 000 |

Lines: up to 30 per kind, 2–40 stops, color from a 12-color line palette (CVD-safe), vehicles
1–20 per line. Bus lanes are a segment style option (buses, service vehicles only).

## 8. Progression (population milestones)

| Tier | Population | Name key | Unlocks |
|---|---|---|---|
| 0 | 0 | `tier.hamlet` | dirt path, street, residential zoning (L1–L2), diesel generator, water pump, sewage outlet, landfill, small park, bulldoze, undo |
| 1 | 250 | `tier.village` | commercial zoning (L1), clinic, elementary school, plaza, tree rows, grass strips, stop signs, R L3 |
| 2 | 600 | `tier.large_village` | industrial zoning (L1–L2), police station, fire station, one-way street, street lamps, cobblestone |
| 3 | 1 200 | `tier.town` | avenue, traffic lights, buses (depot, stops, lines), wind turbine, C L2, loans tier 1 |
| 4 | 2 500 | `tier.large_town` | high school, coal plant, water tower, sewage treatment, R L4, I L3, light asphalt, intersection timing editor |
| 5 | 5 000 | `tier.small_city` | hospital, large park, one-way avenue, bus lanes, solar farm, incinerator, C L3, loans tier 2 |
| 6 | 10 000 | `tier.city` | trams (tracks, stops, depot, lines), university, police HQ, fire HQ, R L5 |
| 7 | 20 000 | `tier.large_city` | gas power plant, landmark buildings (2), loans tier 3 |
| 8 | 35 000 | `tier.metropolis` | landmark buildings (2 more), all decorations |
| 9 | 50 000 | `tier.capital` | "Capital" title, city statistics export |

Milestone notification shows tier name, summary, unlocked items with icons, optional hint
to the relevant tool (tutorial hook).

## 9. Economy

| Parameter | Default |
|---|---|
| Starting money | 60 000 (Easy 120 000, Hard 35 000) |
| Tax rates | R 9 %, C 9 %, I 9 %; range 1–25 % |
| Household income/day (L1…L5 base) | 90, 120, 150, 190, 230 (× wealth 0.8/1.0/1.4) |
| Business profit/day per job (C/I L1…L3) | C 40/55/75, I 35/50/70 |
| Loans | tier 1: 25 000 @ 0.08 %/day; tier 2: 60 000 @ 0.07 %/day; tier 3: 150 000 @ 0.06 %/day |
| Demolish refund | 25 % within 1 game day of construction, else 0 |
| Undo refund | 100 % of recorded cost |

## 10. Citizens

| Parameter | Value |
|---|---|
| Household size | 1–5 (weights 0.28, 0.34, 0.16, 0.15, 0.07) |
| Cars per household | wealth Low 0.6, Mid 1.0, High 1.5 (rounded stochastic) |
| Value of time β_t | 1.0 per perceived minute; fares in money × 2 |
| Move-out | happiness < 35 for 3 days → 20 %/day probability |
| Happiness weights | housing 10, employment 15, utilities 15, garbage 5, services 15, commute 12, pollution 8, noise 5, crime 6, taxes 5, land value/parks 4 |

## 11. Weather and seasons

Base daily probabilities by season (Markov chain with persistence 0.6 and min duration 2 h,
max 14 h):

| Season | Clear | Cloudy | Rain | Thunderstorm | Fog | Snow | Temp range °C |
|---|---|---|---|---|---|---|---|
| Spring | 35 | 30 | 25 | 3 | 7 | 0 | 6–18 |
| Summer | 50 | 20 | 12 | 15 | 3 | 0 | 16–30 |
| Autumn | 25 | 30 | 25 | 4 | 16 | 0 | 4–15 |
| Winter | 22 | 30 | 5 | 0 | 13 | 30 | −8–4 |

Effects:

| Weather | Vehicle speed | Road upkeep | Fire risk | Happiness | Other |
|---|---|---|---|---|---|
| Clear | ×1.00 | — | ×1.0 (summer ×1.2) | +1 % | solar ×1.0 |
| Cloudy | ×1.00 | — | ×0.9 | 0 | solar ×0.45 |
| Rain | ×0.85, gap ×1.3 | — | ×0.6 | −1 % | solar ×0.25, wet roads |
| Thunderstorm | ×0.80, gap ×1.3 | — | lightning strikes | −2 % | wind ×1.5 |
| Fog | ×0.80 | — | ×1.0 | −1 % | visibility (fog FX) |
| Snow | ×0.70, gap ×1.5, b ×0.6 | +60 % × cover | ×0.8 | −1 % (−3 % if heating short) | heating demand |

Forecast: next 48 game hours (deterministic pre-rolled schedule, shown as icons in the top bar).
Settings: weather on/off (off = always clear, seasons still change visuals unless "lock
season" is set), lock weather type.

## 12. Notifications (severity)

Info: milestone, new line created, building leveled (only first of each type).
Warning: budget projected negative, power/water shortage, garbage overflow area, traffic jam
area, unhappy residents leaving, landfill nearly full.
Critical: fire, building burned, bankruptcy, severe weather (storm).
Notifications have a location (click → camera jumps) and are throttled per kind.

## 13. Tutorial outline (M14)

Chapter 1 — first minutes (optional, offered on first new game; replayable from the main
menu; each step has a highlight on the relevant UI element and completes on the action):
1. Camera: pan, zoom, rotate (Q/E and MMB), each detected by input.
2. Draw a street from the outside connection (road tool, straight then curved mode).
3. Draw a residential plot along the street; adjust its sub-plot count.
4. Place a diesel generator and a water pump on the shore; read the power/water overlay.
5. Watch houses grow; read the RCI bars and their tooltip breakdown.
6. Speed controls and the budget panel.
Later chapters trigger on unlocks: commercial & industrial (tier 1–2), services & coverage
overlays (tier 2), traffic & congestion overlay + intersection tool (tier 3), buses and line
management (tier 3), trams (tier 6).
Contextual hints: the first time each tool, overlay or unlock is used a short dismissible
hint card appears; "Hints" toggle in settings; "Reset hints" button.
