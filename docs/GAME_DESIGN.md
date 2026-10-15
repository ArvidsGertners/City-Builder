# Game Design & Initial Balance

Status: **Draft values for implementation** (revised after the 2026-10-15 plan review). Rules
are normative; numbers are starting points tuned in M5–M7 and M16 with
`headless_bench --balance` against the **balance targets in §14**. Runtime source of truth:
`crates/cb_sim/data/*.ron` (keep this doc in sync). Units per ADR-0042.

## 1. Core loop

Empty map with 2–3 outside road connections and starting money → build roads → draw plots
along roads (R first) → connect power and water → houses grow from demand → jobs needed →
commercial/industrial → services, utilities, budget, traffic and transit management →
population milestones unlock new tools and building levels.

No hard game over: sustained bankruptcy stalls the city (ALGORITHMS §21). Sandbox: unlimited
money, everything unlocked (CLI `--sandbox` from M7, main-menu toggle from M15). Difficulty
changes **starting money only** (Easy 120 000 / Normal 60 000 / Hard 35 000).

## 2. Time (ADR-0042)

| Quantity | Value |
|---|---|
| Sim tick | 0.1 sim s |
| Game minute | 2 sim s (fixed) → 1 game day = 2 880 sim s = 28 800 ticks |
| Day length (real time at 1×) | 24 min default; options 16 / 24 / 32 / 48 — **pacing only**, results identical |
| Speeds | Pause / 1× / 2× / 4× |
| Season | 4 game days (2–8 option, saved in `SimConfig`) |
| Year | 4 seasons; ageing yearly, education per season |
| Week | 4 days (leisure trips on day 4) |
| Perceived time | `perceived_min = sim_seconds × 0.1` (a 200 sim-s drive = 20 min) — used by UI, happiness, demand |

Vehicles move at nominal speeds in sim time; at the default pace they visibly move at 2× real
speed (diorama pace, assumption B1). A 2 km commute ≈ 200 sim s ≈ 100 game minutes on the
clock; schedules plan for it (ALGORITHMS §16).

## 3. Road types

| Type | Lanes | Width | Speed | Capacity/lane (veh/sim-h) | Cost/m | Upkeep/m/day | Max grade | Min radius | Unlock |
|---|---|---|---|---|---|---|---|---|---|
| Dirt path | 1+1 narrow | 6 m | 30 km/h | 1 000 | 4 | 0.010 | 14 % | 8 m | start |
| Street | 1+1 | 12 m (incl. 2 × 2 m sidewalks) | 50 km/h | 1 600 | 22 | 0.040 | 10 % | 12 m | start |
| One-way street | 2 | 12 m | 50 km/h | 1 600 | 24 | 0.045 | 10 % | 12 m | 600 |
| Avenue | 2+2 + 2 m median | 22 m | 60 km/h | 1 800 | 60 | 0.120 | 8 % | 25 m | 1 200 |
| One-way avenue | 3 | 16 m | 60 km/h | 1 800 | 50 | 0.100 | 8 % | 25 m | 5 000 |
| Tram track (standalone) | 1+1 tram | 8 m | 50 km/h | — | 70 | 0.150 | 6 % | 20 m | 10 000 |

Capacities match IDM saturation flow within 10 % (unit test). Modifiers: slope × (1 + 2 grade),
earthwork 1.5/m³, bridge × 4 per m + 1 200 per pier + 40 per m pier height, tram tracks in road
+45/m, bus lanes +4/m. Upgrade cost = new − 50 % of old; downgrade refunds 0; widening may remove
plot area/buildings (preview lists them). Snow raises road upkeep up to +60 % (no plow service
is modeled; tooltip says "snow clearing costs").

**Minimum node spacing** is computed live by the junction builder for the actual junctions at both
ends and shown while drawing (ADR-0063). Typical values (center to center, 90° X-junctions of the
same type at both ends): dirt 16 m, street 27 m, one-way avenue 38 m, avenue 45 m, avenue with
tram 51 m; a street ending at an avenue needs ≈ 38 m; acute angles need more (two streets at
40° ≈ 50 m). Turns sharper than 135° are not possible at a junction.

**Roundabouts**: preset radius 15–60 m (street-type ring, one-way), yield on entry, metered at
80 % ring occupancy.

**Surfaces** (per segment): dark asphalt (default), light asphalt (+2/m), cobblestone (+8/m,
−5 km/h, +LV), dirt (dirt path only). **Segment decorations** (per side): sidewalks, curbs, grass
strip (+3/m), tree rows (8–24 m spacing, 45 per tree), street lamps (20–50 m, 60 per lamp, −5 %
crime at night). **Free decorations** (decoration tool, `PlaceProp`): tree 40, tree cluster 150,
bench 30, planter 50, fountain 2 000, flower bed 80 — each adds land value within 30–60 m
(fountain +0.05, tree +0.01, others +0.005–0.02).

## 4. Zones and building levels

| Zone | Levels | Typology by level | Floors | Capacity |
|---|---|---|---|---|
| Residential | 1–5 | L1 cottage · L2 detached house · L3 row house / duplex · L4 small apartment · L5 apartment block | 1–2 · 2 · 2–3 · 3–5 · 5–8 | households 1 · 1–2 · 2–4 · 6–16 · 16–48 (by footprint) |
| Commercial | 1–3 | L1 corner shop · L2 shop row / offices · L3 department store / office tower | 1–2 · 2–4 · 4–10 | jobs 4–10 · 10–40 · 40–160 |
| Industrial | 1–3 | L1 workshop · L2 factory hall · L3 plant with tanks/chimneys | 1 hall · 1–2 · 2 + stacks | jobs 8–20 · 20–60 · 60–200 |

Lot widths: R 10–18 m (target 13), C 14–30 m (20), I 24–60 m (36), narrowed on curves so a lot's
frontage turns ≤ 25° (ADR-0064); max depths R 45, C 50, I 80 m;
min area R 120, C 200, I 600 m². Level caps by lot area: R L4 ≥ 400 m², R L5 ≥ 700 m²; C L3 ≥
600 m²; I L3 ≥ 2 000 m². Leveling thresholds `up = [0.35, 0.5, 0.62, 0.75]`, down = up − 0.15;
weights in ALGORITHMS §20. Job education requirements: C L1 edu 0+, C L2 edu 1+, C L3 edu 2+
(60 %); I L1 0, I L2 1, I L3 2 (40 %). Before M7 everything is unlocked implicitly; M7 enables
tiers.

## 5. Services

| Service | Building | Reach (network) | Capacity | Vehicles | Cost | Upkeep/day | Unlock |
|---|---|---|---|---|---|---|---|
| Police | Police station | 900 m | 1 500 residents | 3 cars | 9 000 | 600 | 600 |
| Police | Police HQ | 1 600 m | 6 000 | 8 | 30 000 | 1 800 | 10 000 |
| Fire | Fire station | 1 200 m | 2 500 buildings-risk | 3 trucks | 10 000 | 650 | 600 |
| Fire | Fire HQ | 2 000 m | 8 000 | 8 | 32 000 | 1 900 | 10 000 |
| Health | Clinic | 800 m | 1 200 | 2 ambulances | 8 000 | 550 | 250 |
| Health | Hospital | 1 800 m | 8 000 | 8 | 40 000 | 2 400 | 5 000 |
| Education | Elementary school | 900 m | 300 students | — | 10 000 | 500 | 250 |
| Education | High school | 1 500 m | 800 | — | 25 000 | 1 300 | 2 500 |
| Education | University | city-wide | 2 000 | — | 70 000 | 3 500 | 10 000 |
| Leisure | Small park / Plaza / Large park | 400 / 400 / 800 m | — | — | 1 500 / 3 000 / 9 000 | 60 / 120 / 300 | 0 / 250 / 5 000 |
| Admin | Town hall | — | — | — | free, auto-placed at start near the first outside connection's end (player may move it once, free) | 200 | start |

Budget sliders 50–150 % scale capacity, reach (× 0.8–1.1), vehicles and upkeep. Incident
formulas: ALGORITHMS §17.

## 6. Utilities

| Building | Output / capacity | Placement | Pollution | Cost | Upkeep/day | Unlock |
|---|---|---|---|---|---|---|
| Diesel generator | 2 MW | anywhere | medium, noise | 4 000 | 300 | start |
| Wind turbine | 0–1.5 MW (wind curve) | hills preferred | low noise | 5 000 | 150 | 1 200 |
| Coal power plant | 25 MW | anywhere | high | 30 000 | 1 600 | 2 500 |
| Solar farm | 0–6 MW (sun, clouds) | anywhere, large | none | 22 000 | 400 | 5 000 |
| Gas power plant | 40 MW | anywhere | medium | 55 000 | 2 600 | 20 000 |
| Water pump | 300 m³/h | shoreline (depth ≥ 1.5 m within 15 m) | — | 3 500 | 200 | start |
| Water tower (groundwater) | 120 m³/h | anywhere | — | 4 500 | 250 | 2 500 |
| Sewage outlet | 300 m³/h | shoreline | water pollution high | 2 500 | 120 | start |
| Sewage treatment plant | 800 m³/h, 85 % treatment | shoreline | low | 28 000 | 1 200 | 2 500 |
| Landfill | 400 t storage, 2 trucks | anywhere | ground medium | 6 000 | 350 | start |
| Incinerator | 60 t/day, 4 trucks, +6 MW | anywhere | high | 35 000 | 1 400 | 5 000 |

Consumption per L1 household / per job: power 1.2 / 0.8 kW, water 0.25 / 0.15 m³/h, sewage 90 %
of water, garbage 2.5 / 1.5 kg/day; × 1.0–1.6 with level. Curves (wind, solar, heating) in
ALGORITHMS §18. Full landfill: `EmptyLandfill` (haul to incinerator, 2 per t) or close (park after
4 days).

## 7. Public transport

| Item | Capacity | Speed | Cost | Upkeep | Fare default | Unlock |
|---|---|---|---|---|---|---|
| Bus depot | 20 buses | — | 12 000 | 500/day | — | 1 200 |
| Bus stop | 40 waiting | — | 300 | 20/day | — | 1 200 |
| Bus | 60 | lane speed | — | 45 per vehicle-game-hour | 2.00 | 1 200 |
| Tram depot | 12 trams | — | 30 000 | 1 100/day | — | 10 000 |
| Tram stop | 80 waiting | — | 900 | 40/day | — | 10 000 |
| Tram (3 sections) | 180 | ≤ 50 km/h | — | 90 per vehicle-game-hour | 2.50 | 10 000 |

Lines: ≤ 30 per kind, 2–40 stops, 12-color CVD-safe palette, 1–20 vehicles, ping-pong unless
circular. Bus lanes are a segment style.

## 8. Progression (population milestones)

| Tier | Population | Name key | Unlocks |
|---|---|---|---|
| 0 | 0 | `tier.hamlet` | dirt path, street, residential (L1–L2), diesel generator, water pump, sewage outlet, landfill, small park, free decorations (tree, bench), bulldoze, undo |
| 1 | 250 | `tier.village` | commercial (L1), clinic, elementary school, plaza, tree rows, grass strips, stop/yield signs, R L3 |
| 2 | 600 | `tier.large_village` | industrial (L1–L2), police station, fire station, one-way street, street lamps, cobblestone, roundabout preset |
| 3 | 1 200 | `tier.town` | avenue, traffic lights, buses (depot, stops, lines), wind turbine, C L2, loan tier 1 |
| 4 | 2 500 | `tier.large_town` | high school, coal plant, water tower, sewage treatment, R L4, I L3, light asphalt, signal timing editor |
| 5 | 5 000 | `tier.small_city` | hospital, large park, one-way avenue, bus lanes, solar farm, incinerator, C L3, fountain, loan tier 2 |
| 6 | 10 000 | `tier.city` | trams (tracks, stops, depot, lines), university, police HQ, fire HQ, R L5 |
| 7 | 20 000 | `tier.large_city` | gas power plant, landmark buildings (2), loan tier 3 |
| 8 | 35 000 | `tier.metropolis` | landmark buildings (2 more), all decorations |
| 9 | 50 000 | `tier.capital` | "Capital" title, statistics export |

## 9. Economy

| Parameter | Default |
|---|---|
| Starting money | 60 000 (difficulty changes only this) |
| Tax rates | R/C/I 9 %; range 1–25 % |
| Household income/day (L1…L5) | 90, 120, 150, 190, 230 × wealth (0.8 / 1.0 / 1.4) |
| Business profit | from the goods model (ALGORITHMS §27): sales − wages − goods cost |
| Wages per job per day | C 30/40/55, I 28/38/50 by level |
| Loans | tier 1: 25 000 @ 0.08 %/day; tier 2: 60 000 @ 0.07 %/day; tier 3: 150 000 @ 0.06 %/day; simple interest, ≤ 1 per tier |
| Demolish refund | 25 % within 1 game day of construction, else 0 |
| Undo refund | 100 % within the first game hour, linearly to 25 % at 1 game day; undo expires after 1 game day (ADR-0055) |
| Bankruptcy | 7 days < 0 → emergency loan, else forced 50 % budgets + construction freeze until ≥ 0 for 1 day |

## 10. Citizens

| Parameter | Value |
|---|---|
| Household size | 1–5 (weights 0.28, 0.34, 0.16, 0.15, 0.07) |
| Cars per household | wealth Low 0.6, Mid 1.0, High 1.5 (stochastic rounding) |
| Mode choice | logit, β_t = 1.0 per perceived minute, money × 2 (ALGORITHMS §16) |
| Move-out | happiness < 35 for 3 days → 20 %/day |
| Happiness weights | housing 10, employment 15, utilities 15, garbage 5, services 15, commute 12, pollution 8, noise 5, crime 6, taxes 5, land value/parks 4 (sum 100) |
| Lifecycle | ALGORITHMS §16 |

**Neutral stubs before the real system exists** (formulas unchanged when the stub is replaced;
test `happiness_stub_replacement_keeps_formula`): services coverage 0.5 (until M6), commute =
routed macro estimate (M5 has routing), noise from traffic = flow-model volumes (M5), crime 0.2
(until M6).

**Walking thresholds (one table)**: walk mode ≤ 800 m; stop access ≤ 600 m (network); transfer
walk ≤ 300 m; land-value transit bonus ≤ 400 m; visual pedestrians within 400 m of the camera
focus.

## 11. Weather and seasons

Base daily probabilities by season (Markov chain, persistence 0.6, duration 2–14 game hours):

| Season | Clear | Cloudy | Rain | Thunderstorm | Fog | Snow | Temp range °C |
|---|---|---|---|---|---|---|---|
| Spring | 35 | 30 | 25 | 3 | 7 | 0 | 6–18 |
| Summer | 50 | 20 | 12 | 15 | 3 | 0 | 16–30 |
| Autumn | 25 | 30 | 25 | 4 | 16 | 0 | 4–15 |
| Winter | 22 | 30 | 5 | 0 | 13 | 30 | −8–4 |

| Weather | Vehicle speed | Road upkeep | Fire risk | Happiness | Other |
|---|---|---|---|---|---|
| Clear | × 1.00 | — | × 1.0 (summer × 1.2) | +1 % | solar × 1.0 |
| Cloudy | × 1.00 | — | × 0.9 | 0 | solar × 0.45 |
| Rain | × 0.85, gap × 1.3 | — | × 0.6 | −1 % | solar × 0.25, wet roads |
| Thunderstorm | × 0.80, gap × 1.3 | — | lightning strikes | −2 % | wind × 1.5 |
| Fog | × 0.80 | — | × 1.0 | −1 % | visibility |
| Snow | × 0.70, gap × 1.5, b × 0.6 | +60 % × cover | × 0.8 | −1 % (−3 % if heating short) | heating demand |

Forecast: next 48 game hours (deterministic). Weather off = always clear (seasons still tint
visuals); lock weather kind.

## 12. Notifications

Severity: Info (milestone, first line created, first level-up per type), Warning (budget
projected negative, power/water shortage, garbage overflow area, traffic jam area, residents
leaving, landfill nearly full), Critical (fire, building burned, bankruptcy, severe weather).
Each has a location (click → camera jumps). Throttle: ≤ 1 per kind per 30 real s and ≤ 1 per
subject per game day; Critical is never throttled.

## 13. Tutorial outline (M14)

Chapter 1 (offered on first new game; replayable from the pause menu and `--tutorial` from M14,
from the main menu in M15): camera (pan, zoom, rotate — Q/E and drag/trackpad), draw a street from
the outside connection (straight then curved), draw a residential plot and change its sub-plot
count, place a diesel generator and a water pump at the shore (worldgen guarantees a shore within
400 m), watch growth and read the RCI tooltip, speed controls and the budget panel. Later chapters
on unlocks: C/I (tiers 1–2), services & coverage overlays (tier 2), traffic overlay + intersection
tool (tier 3), buses (tier 3), trams (tier 6). Contextual first-use hints with a toggle (dev panel
until M15) and "reset hints". Shortcut glyphs come from the binding formatter.

## 14. Balance targets (checked by `headless_bench --balance`)

3 seeds × 3 scripted build orders (cautious, sprawl, dense); a run fails if any target fails:

| Metric | Target |
|---|---|
| Time to tier 1 (250 pop) at 1×, default day | 12–25 real minutes |
| Money with "cautious" script before tier 3 | never < 0 |
| Daily net income at tiers 2–5, default taxes | ≥ +5 % of daily upkeep |
| Unemployment | 3–12 % |
| Each RCI bar within [−0.3, 0.8] | ≥ 80 % of game hours |
| Mean happiness | 55–75 |
| Abandoned buildings | ≤ 5 % |
| Perceived commute p50 / p95 | 12–25 min / < 60 min |
| Share of peak trips simulated as agents at 50k pop | ≥ 40 % |
