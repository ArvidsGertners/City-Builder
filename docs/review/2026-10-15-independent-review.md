> **Provenance.** Written by an independent review agent that did not take part in creating the plan. It reviewed a read-only snapshot of the plan at commit `053daa3` (branch `claude/great-wozniak-g2s2dd`) against the user's brief. Line references point into that snapshot. Resolutions: [../PLAN_REVIEW.md](../PLAN_REVIEW.md).

# Independent review of the City Builder Phase-1 plan

Reviewer stance: senior game/engine engineer, no stake in the plan. Scope: every file in
`review-snapshot/` (BRIEF, CLAUDE, README, all docs, ADR 0000–0041, the three skills).
Line numbers refer to the snapshot files (`L12–14` = lines 12–14).

Severity: **C** = critical (wrong result, broken requirement or rework of a foundation
later), **H** = high (likely to cost a milestone or fail an acceptance gate), **M** = medium,
**L** = low. Tags in section B: **[DS]** design-shaping (architecture changes if wrong),
**[T]** tunable.

### What I verified against real crate sources (downloaded from crates.io on 2026-10-15)

| Claim in plan | Verdict |
|---|---|
| Bevy 0.20.0 released 2026-10-08; `bevy_math` uses glam ^0.33.2 | ✔ correct |
| `ScheduleBuildSettings::shuffle_seed` exists (TESTING L25, ARCH L353) | ✔ exists, **but only behind `bevy_ecs` feature `debug`** (`#[cfg(feature = "debug")]`, schedule.rs L1887), and Bevy's own TODO says `auto_insert_apply_deferred` prevents true shuffling across sync points |
| `EguiPrimaryContextPass` systems are "under `GameSet::Ui` in `Update`, before `SimDrive`" (ARCH L296/L367, ADR-0024 L162) | ✘ **wrong**: bevy_egui 0.43.1 runs that schedule from `PostUpdate` (`run_egui_context_pass_loop_system` in `EguiPostUpdateSet::EndPass`, lib.rs L1144–1149) |
| `ShaderBuffer`, `MeshTag`, `VisibilityRange`, `ContactShadows`, `GeneratedEnvironmentMapLight`, `MainPassResolutionOverride`, `DistanceFog` exist in 0.20 | ✔ present |
| SSAO requires `Msaa::Off` | ✔ (bevy_pbr ssao/mod.rs L505–509) |
| bevy_egui has AccessKit support | ✔ but only with feature `accesskit` (not in the plan's feature list); bevy_egui's **default** features also pull `bevy_ui`, `bevy_picking`, `webbrowser`, `arboard`, `smithay-clipboard`, none of which are listed in ADR-0023 |
| egui default fonts can show ⌘ ⌥ ⇧ ⌃ glyphs (ART_DIRECTION L195, ADR-0039 L158) | ✘ checked the cmap tables of epaint_default_fonts 0.36.2: **⌥, ⌃, ⌫ are in none of the 4 fonts; ⇧ only in Hack (monospace); ⌘ only in emoji-icon-font**. Ubuntu-Light (the proportional font) has none of them, and lacks → as well |
| A frame cap (30/60/120) is available (ADR-0041 L287–289) | ✘ Bevy has no built-in limiter; `bevy_framepace` 0.22.0 (latest) depends on Bevy **0.19** |
| bevy_kira_audio supports 0.20 (ADR-0019) | ✔ 0.27.1 depends on Bevy 0.20 |
| `PanGesture` on macOS (ADR-0039 L177) | ? Bevy only forwards winit's event. winit documents `PanGesture` as iOS-only; on macOS, two-finger scroll arrives as pixel-unit `MouseWheel`. Needs a check on the M4 |

---

## Top findings (by severity)

1. **G-D1 (C)** `derive_seed` XORs independently mixed inputs (ALGORITHMS L657–658), so
   it is symmetric in `id`/`extra`. Building 3 at level 5 gets the same seed as building 5
   at level 3, and `id == extra` cancels out. This breaks "unique and deterministic"
   buildings and the RNG streams.
2. **G-M1 (C)** The time model is internally inconsistent. Capacities are "/lane/h",
   EMAs have a "10 game minute" half-life and cost epochs are "15 game minutes", while
   vehicles move in sim seconds and 1 game minute = 1 sim second. Read as written, BPR
   capacity and agent discharge differ by ~27×. Commutes take 2–7 *game hours*, so morning
   peaks hold ~15–25k concurrent trips against a 5k agent cap (p≈0.2–0.3). The 12/24/48-min
   day options rescale all trip durations in game time.
3. **G-S1 (C)** The save drops in-flight agents, junction and signal state, route-cache
   contents (which steer route choice), flow EMAs, the accumulating pollution fields and
   incidents. The "save → load → continue = continuous" test (TESTING L71, L81) therefore
   fails from M9 on. Base terrain is regenerated from the seed with platform libm, so saves
   are not portable between the user's Windows PC and Mac (river tracing is a discrete,
   chaotic decision).
4. **D1–D8 (C)** PERFORMANCE.md arithmetic is wrong:
   - 1× runs 0.33 ticks per frame, not 1.2; 4× runs 1, 1 or 2 ticks per frame.
   - Extract is booked as "overlapping" (it runs on the main thread), and there is no
     render-thread budget.
   - 300 searches × 0.2–0.6 ms is 15–45 ms of wall time on 4 workers.
   - The M4 GPU budget is applied to 1.8× the pixels of 1080p.
   - There is no memory budget, although flat-shaded merged buildings plus merged trees
     come to ≈ 1–3 GB of vertex data.
5. **G-T1 (C)** The junction "deadlock impossible" proof is wrong. Exit space is checked but
   not reserved, so simultaneous grants and platoon followers over-commit the target lane
   and stop inside the box. Right-before-left with all approaches occupied stalls until the
   12 s anti-starvation timer fires, and then every request is top priority at once.
6. **G-G1/G2/G3 (C/H)** Terrain/road interface:
   - The junction surface is a single flat `surface_h`, so there are steps at cut lines on
     graded approaches.
   - "Highest-weight-wins" stamp compositing is C0-discontinuous where stamps overlap, which
     produces cliffs between embankments.
   - The 1.5 m flat shoulder is smaller than the 2.83 m cell diagonal, so terrain pokes
     through road edges in cuts at LOD0, and worse at LOD1+.
7. **E-1…E-12 (C/H)** Hidden dependencies:
   - The aggregate commute model (needed from M5, "playable without traffic") needs the
     router and LinkGraph, which are only accepted in M9.
   - Undo in M3 uses "save snapshot types", which come in M5.
   - Ghost service vehicles in M6 need the router, the flow model and vehicle rendering
     (M9).
   - Settings persistence (M15) is used from M1/M2.
   - M7 "locked tools shown" needs the M8 toolbar, and M14 "replay from main menu" needs M15.
8. **G-P1 (H)** The risk-spike phase is mandatory in CLAUDE.md (L50–56), but S1–S5 are
   defined nowhere and "S6" names two different spikes (ADR-0037 L43 vs ADR-0041 L276).
   PROGRESS.md has no phase S and no pass/fail criteria, and CLAUDE says "after M1" while
   the ADRs say "in M1".
9. **G-W1/W2 (H)** Verified API errors: the UI cannot sit in `Update` before `SimDrive` (egui
   runs in PostUpdate), and the shuffled-schedule determinism test will not compile without
   `bevy/debug` in `cb_sim`'s dev features.
10. **G-D2/D3 (H)** Intra-system partitions of "≥ 4 × core count" (ADR-0037 L48–50, PERF
    L101–102) make float merge order machine-dependent, which breaks "same platform"
    determinism between a 4-core CI runner and a 6-core PC. The ADR-0037 claim that "basic
    arithmetic stays identical" across x86-64 and aarch64 ignores glam's SSE2 vs NEON
    horizontal-reduction order.
11. **G-R1/R2 (H)** The ALT heuristic is not admissible: ±10 % cost variants push costs
    below free-flow. The >90 % cache hit rate is unsupported because `variant = hash(trip)`
    changes every day for the same commuter, multiplying keys ×4 against a 32k LRU.
12. **G-C1 (H)** `agent_cap` is a saved sim setting (ADR-0010 L127) and also a graphics-preset
    lever (PERF L91), so graphics settings change simulation results. The determinism test
    #4 only varies `max_visible_vehicles`.
13. **F-0 (H)** Acceptance gates depend on the user's hardware runs (M1, M2, M4, M9, M11,
    M16) and ears (M13), although the brief says not to stop between milestones.
    - "60 FPS with 500 houses on the dev machine profile" (PROGRESS L105) cannot be met on a
      GPU-less container.
    - CPU benchmarks under lavapipe are polluted by software rasterisation competing for the
      same cores.
    - Visual milestones have no shot lists, although the skill requires them in PROGRESS.md.
14. **G-I1…I5 (H)** Input conflicts:
    - Ctrl length-snap (ALGORITHMS L47) violates "never hard-code Ctrl", and Ctrl+click is a
      right-click (cancel) on macOS.
    - Alt+left-drag rotates the camera but also triggers the plot tool's Alt auto-clip.
    - Left-drag pan collides with frontage drag and freehand drawing.
    - The `]`/`[` defaults need AltGr on German layouts, and Macs have no PgUp/PgDn.
    - Shortcut glyphs render as tofu with egui's default fonts (verified).
15. **G-B1…B5 (H)** Brief coverage gaps:
    - There is no command to edit or reshape existing road geometry ("edit/remove with
      intersections updating").
    - There are no per-approach stop signs, only all-way stops.
    - There is no goods-economy / delivery-trip generation model, although the brief asks
      for "trips from … goods economy".
    - There is no rule for road upgrades that widen into plots.
    - Undo refunds 100 % at any age while demolish refunds 0 % after one day, which is an
      exploit.

Counts: A 58 · B 52 · C 5 (each with a spike) · D 14 (+ corrected budget tables) · E 44 ·
F 17 blocks (F-0 cross-cutting + M1–M16, with screenshot lists) · G 118. Total ≈ 310.

---

## A. "Will be handled" without a concrete approach

**A1 · H · GAME_DESIGN.md L3–5 (status)**
- **Plan text:** "numbers are starting points, tuned during M5–M7 and M16 via headless
  balance runs (`headless_bench --balance`)".
- **Missing:** balance targets; without them a "balance run" has nothing to pass.
- **Proposal:** add GAME_DESIGN §14 *Balance targets*, checked by
  `headless_bench --balance` on 3 seeds × 3 scripted build orders ("cautious",
  "sprawl", "dense"):
  - tier 1 (250 pop) reached in 12–25 real min at 1×;
  - money never < 0 before tier 3 with the "cautious" script;
  - daily net ≥ +5 % of upkeep at tiers 2–5 with default taxes;
  - unemployment 3–12 %;
  - each RCI bar in [−0.3, 0.8] for ≥ 80 % of game hours;
  - mean happiness 55–75;
  - abandoned buildings ≤ 5 %;
  - perceived commute p50 12–25 min.

  Report a pass/fail table per run; a failure means tuning the RON.

**A2 · M · ALGORITHMS.md L4–5**
- **Plan text:** "Constants are initial values; tune in the milestone that implements them".
- **Missing:** an owner test and a target metric per constant.
- **Proposal:** add a `docs/CONSTANTS.md` table (constant → owning test → metric). Examples:
  - critical gap 4.0 s → `junction_unsignalized_t_minor_flow`: minor-stream throughput
    ≥ 450 veh/sim-h at major flow 600 veh/sim-h;
  - `a_lat_max` 2.5 → curve speed at r = 25 m ≈ 28 km/h (√(2.5·25) = 7.9 m/s);
  - `stuck_s` 90 s → soak despawn rate < 1/1000 trips.

**A3 · H · ALGORITHMS.md §15 L511–518, L521–523**
- **Plan text:** demand uses "attractiveness combines average happiness, service coverage,
  land value, unemployment (negative), commute time (negative)" plus `base_immigration`,
  `retail_capacity`, `goods_availability`, `export_access`, `workforce_availability`,
  `tourism_stub`, `growth_rate(population)`.
- **Missing:** none of these terms has a definition, a weight or a unit.
- **Proposal:** define each term in GAME_DESIGN with normalisation to [0, 1]:
  - `job_vacancy = (jobs − filled)/max(jobs, 1)`;
  - `housing_vacancy = free_slots/max(slots, 1)`;
  - `attractiveness = 0.30·(2h−1) + 0.20·cov + 0.20·(2·LV−1) − 0.20·min(1, 5·unemp)
    − 0.10·clamp((commute_p50 − 20)/20, 0, 1)`;
  - `base_immigration = 0.3` until 300 pop, then linearly to 0 at 1 500;
  - `retail_capacity = Σ C jobs × 25 customers/day`;
  - `growth_rate(pop) = clamp(0.5 + pop/2 000, 0.5, 6)` new constructions per **sim
    minute**.

  The unit also needs fixing: "each sim second" (L521) or game hour — say which.

**A4 · M · ALGORITHMS.md §16 L527–529**
- **Plan text:** "households arrive … (rate ∝ demand)".
- **Proposal:** households per game hour = `ceil(max(R, 0) × 6 × (1 + pop/1 500))`, capped by
  free housing slots in connected components. Each arrival is a trip from the outside
  connection that is nearest by network to the target lot. Draws use
  `SimRng::for_event(Immigration, hour, k)`.

**A5 · M · ALGORITHMS.md §16 L553–556**
- **Plan text:** "kept lightweight; no individual births/deaths simulation beyond counts".
- **Missing:** this is not a mechanism, yet 50k records need lifecycle rules or the
  population ages into all-senior.
- **Proposal:** run per game year, sliced over the first game day of spring, with
  `SimRng::for_event(Lifecycle, year, citizen_id)`:
  - Child→Student at age-step 1 and Student→Adult at step 3 (moves out if a free R slot
    exists, else stays);
  - Adult→Senior with p = 1/12; Senior removed with p = 0.12;
  - a household with ≥ 2 adults and size < 5 gains a Child with p = 0.15;
  - an empty household is dissolved.

**A6 · H · ALGORITHMS.md §17 L574–583**
- **Plan text:** incident formulas: "`crime = f(1 − police_cov, unemployment, land value)`",
  "medical events from sickness", "spread … with probability", "resolves … over a service
  time", "garbage > threshold".
- **Missing:** these drive two milestones (M6, M9) and none has numbers.
- **Proposal:**
  - crime events/building/game-day = `0.02 × (1 − 0.85·police_cov) × (1 + 2·unemp_local)
    × (1.2 − 0.4·LV)`;
  - sickness: `health −= 0.5·pollution_norm − 0.4·health_cov` per day, with a medical event
    when it falls below 30;
  - fire spread: each burning building ignites neighbours with footprint distance < 6 m at
    p = 0.15 per sim minute × (1 − 0.8·fire_cov), and ×2 in summer clear weather;
  - service time: police 20 sim s, ambulance 30 sim s, fire 60–180 sim s scaled by
    intensity;
  - garbage threshold: 3 days of `garbage_kg_day`.

  All of these go in `services.ron`.

**A7 · H · ALGORITHMS.md §17 L566–569, ADR-0027 L59–60**
- **Plan text:** "capacity-aware sum (saturating) for gameplay"; "scaled by budget and
  capacity utilization".
- **Missing:** an allocation rule.
- **Proposal:** proportional load split.
  - Per building `b` and facility `f` in reach: `w_fb = c_fb / Σ_f' c_f'b`.
  - `load_f = Σ_b demand_b·w_fb`; `util_f = min(1, capacity_f / load_f)`.
  - `coverage_b = 1 − Π_f (1 − c_fb·util_f)`.
  - Reach geometry stays incremental (per emitter). Loads are recomputed globally once per
    game hour, sliced over 10 ticks, which also fixes G-M3 (incremental ≠ full).

**A8 · M · ALGORITHMS.md §18 L591–593, GAME_DESIGN §6 L110–112**
- **Plan text:** "`availability(weather/time)` (wind: wind speed curve; solar …)", "heating
  multiplier", "snow/temperature factor".
- **Proposal:**
  - wind: `P = rated·clamp(((v − 3)/(12 − 3))³, 0, 1)`, 0 above 25 m/s, height bonus
    `1 + 0.004·(h − h_mean)`;
  - solar: `rated·max(0, sin(elev))·(1 − 0.75·cloud)`;
  - heating: `1 + 0.3·clamp((10 − T_c)/18, 0, 1)`.

**A9 · M · ALGORITHMS.md §18 L603–604**
- **Plan text:** "landfills fill and must be emptied/replaced".
- **Missing:** an action and a command.
- **Proposal:** add `PlayerCommand::EmptyLandfill(id)`. Its trucks haul to an incinerator
  and cost 2/t; alternatively a full landfill can only be "closed", becoming a park after
  4 days. A full landfill stops accepting garbage and raises a notification.

**A10 · M · ALGORITHMS.md §18 L599–601, ADR-0026**
- **Plan text:** "outlet pollutes water (water pollution field downstream)".
- **Missing:** "downstream" is undefined for lakes, and the plan never says what water
  pollution does.
- **Proposal:**
  - Build a water-body graph from `WaterField` body ids. Rivers are polylines ordered by
    surface height; lakes are well-mixed nodes.
  - Mass is injected at the outlet's nearest river node and advected downstream with
    5 %/km decay. Lake concentration = mass/volume, with a 2-day half-life.
  - A pump sampling a concentration above 0.3 delivers "polluted water", giving sickness
    +X and a happiness penalty.
  - Rasterise into `water_pollution` only for the overlay.

**A11 · H · ALGORITHMS.md §19 L615–621; GAME_DESIGN §3 L54–58**
- **Plan text:** the land-value formula is a list of signed terms; decorations are given as
  "+LV", "−noise", "−crime slightly".
- **Missing:** magnitudes.
- **Proposal:**
  - LV components each in [0, 1] with weights: services 0.30, parks/decor 0.15, transit
    0.10, terrain base 0.15, road type 0.05, pollution −0.20, noise −0.10, crime −0.10,
    abandoned within 50 m −0.10.
  - Decor per side within 40 m: tree row +0.04, grass strip +0.02, cobblestone +0.03, lamp
    −5 % crime at night.

**A12 · L · ALGORITHMS.md §19 L612–613**
- **Plan text:** "trees within 30 m reduce (absorption term)".
- **Proposal:** `P ← P·(1 − 0.03·tree_density_cell)`, with tree density per 8 m cell taken
  from `PropLayer` (computed once per dirty region).

**A13 · H · ALGORITHMS.md §20 L627–629**
- **Plan text:** `w_lv, w_srv, w_util, w_hap, w_acc` with no values anywhere.
- **Proposal:** R `0.30/0.25/0.15/0.20/0.10`, C `0.25/0.15/0.15/0.30 (profit)/0.15`,
  I `0.10/0.10/0.20/0.35/0.25`, stored in `zones.ron`. Add a test: with a fully served lot
  at LV 0.8, R reaches L3 within 10 game days.

**A14 · C · ALGORITHMS.md §21 L641–643, §15 L514–518**
- **Plan text:** "`Σ businesses profit(level, customers/production)`", "goods_demand −
  production".
- **Missing:** a goods economy. There is no production rate, no stock dynamics, no matching
  of industry to commercial, no truck trip generation and no import/export pricing. The
  brief requires "delivery trucks industry↔commercial; trips from … goods economy".
- **Proposal:**
  - Industry produces `output_per_hour = jobs_filled × 0.5 units`.
  - Commercial consumes `customers × 0.02 units`.
  - Each game hour, for every commercial building with stock < 50 %, request
    `ceil(deficit / 10)` deliveries.
  - Match to the nearest industry by network ETA with stock ≥ 10, else import from an outside
    connection at +30 % cost.
  - Each delivery is a `Truck` trip with 10 units. Industry surplus > 80 % exports via the
    outside connection (income).
  - Profit = sales − wages − goods cost.

  Add `GoodsMarket` to DATA_MODEL §10.

**A15 · M · ALGORITHMS.md §21 L647–649; GAME_DESIGN L14–15**
- **Plan text:** "game-over warning dialog" vs "the city stalls instead".
- **Missing:** a definition of "stall".
- **Proposal:** after 7 days below zero, take the emergency loan if one is available.
  Otherwise:
  - service budgets are forced to 50 %;
  - all construction except demolition and policy changes is blocked;
  - a "Bankrupt" banner is shown;
  - recovery happens when the balance is ≥ 0 for 1 day.

**A16 · L · ALGORITHMS.md §21 L653; GAME_DESIGN L154**
- **Plan text:** "fixed interest per day; repay anytime".
- **Missing:** term, maximum concurrent loans, and whether interest compounds.
- **Proposal:** simple daily interest on the principal; at most 3 loans (one per tier); no
  term; principal repaid in full.

**A17 · H · ALGORITHMS.md §10 L304–306**
- **Plan text:** "both are rotated toward their bisector until they don't (bounded
  iterations)".
- **Missing:** the bound and the fallback.
- **Proposal:** at most 8 iterations of 5°. If the rays still cross, delete the split
  (merge the two lots) and log at debug. A property test asserts that no output rays cross.

**A18 · L · ALGORITHMS.md §10 L312–314**
- **Plan text:** "For very deep plots the tool suggests a lower depth."
- **Proposal:** when depth > `d_max(zone) + 10 m`, the depth handle snaps to `d_max` (Shift
  overrides) and a hint shows the backyard percentage.

**A19 · H · ALGORITHMS.md §10 L319–321; ARCHITECTURE L522**
- **Plan text:** "(and building, if its footprint still fits)"; "stable lot IDs kept where
  geometry matches".
- **Missing:** a definition of "fits"; this governs what survives every plot edit.
- **Proposal:** fits ⇔ `footprint ⊆ inset(new_lot, setbacks − 0.5 m)` ∧ |Δaccess| ≤ 3 m ∧
  the slope class is unchanged. Otherwise the building is demolished with no refund, a
  household eviction goes into `homeless_queue`, and the lot regrows. The plot preview
  shows the buildings that will be removed.

**A20 · H · ALGORITHMS.md §9 L281–284**
- **Plan text:** "side edges follow the local road normal, blended … (normals averaged over
  a window; side edges never intersect within the depth)".
- **Missing:** the window size, and the case of depth > inner radius (the back edge
  degenerates on concave frontage).
- **Proposal:** build the back edge with an `i_overlay` offset of the frontage polyline by
  `depth` (round joins), and make the side edges straight between the frontage ends and the
  offset ends. On concave frontage, clamp `depth ≤ 0.8·r_inner`. The averaging window is
  0.5·depth.

**A21 · M · ALGORITHMS.md §1 L43–48**
- **Plan text:** snapping "within screen-space radius scaled by zoom" with world radii
  "8 m"/"6 m".
- **Proposal:** `r_world = max(r_listed_m, 14 logical px × m_per_px(cursor depth))`.
  Elevated nodes snap in 3D: distance from the cursor ray to the node, not from the ground
  hit.

**A22 · H · ALGORITHMS.md §1 L50–55**
- **Plan text:** the crossing class is decided from Δh "at the crossing".
- **Missing:** the new road's profile depends on the crossings and vice versa; no order is
  given.
- **Proposal:**
  1. Solve a tentative profile with no crossings.
  2. Classify each crossing.
  3. For junction crossings, add a fixed-height constraint (the mean of both, within each
     road's grade) and re-solve both profiles.
  4. Re-classify; at most 2 iterations, otherwise invalid with reason
     `road.crossing_height_unstable`.

**A23 · M · ALGORITHMS.md §2 L79–82**
- **Plan text:** "Converges quickly for n ≤ 60 samples".
- **Missing:** a stopping rule and an infeasibility report.
- **Proposal:** stop when max |Δh| < 0.01 m or after 30 passes. If any
  `|h_{i+1} − h_i| > grade·4 m + 1e-3` remains, report "too steep at s = …". Add unit tests
  with forced infeasible endpoints.

**A24 · M · ALGORITHMS.md §3 L107–109**
- **Plan text:** "pier positions avoid other roads' corridors (shift along span ±25 %)".
- **Missing:** what happens if a pier still collides.
- **Proposal:** try ±25 % in 5 % steps, then insert or remove one pier (respecting 60 m max
  unsupported span), then fail with `bridge.pier_conflict{segment}`.

**A25 · L · ALGORITHMS.md §5 L149–151**
- **Plan text:** "Two-way continuation nodes with width change get a short taper".
- **Proposal:** taper length = `max(8 m, 6 × Δhalf_width)`, consumed from both segments as a
  cut-back. Lane drops/adds happen at the taper start, with a mandatory lane-change zone.

**A26 · H · ALGORITHMS.md §6 L212–216**
- **Plan text:** "Phase plan auto-generated from approaches: pair opposing approaches".
- **Missing:** a phase plan for T-junctions, 5-way, Y and offset junctions.
- **Proposal:** build the movement conflict graph from the conflict zones. Greedily pack
  movements into phases as maximal compatible sets, seeded by the two most-opposite
  approaches. Add a protected left phase if a left movement conflicts with an opposing
  through movement whose flow > 300 veh/sim-h. Tests: no conflicting greens on T, X, Y, 5-
  and 6-way; every movement green in ≥ 1 phase.

**A27 · M · ALGORITHMS.md §6 L218**
- **Plan text:** "Pedestrian phases are implicit (visual pedestrians cross on green)".
- **Missing:** behaviour at priority and all-way-stop junctions.
- **Proposal:** visual pedestrians cross only when no vehicle has a grant on a connector
  intersecting the crosswalk capsule within the next 3 s. They never block vehicles; state
  that they are not sim agents.

**A28 · M · ALGORITHMS.md §13 L470–471**
- **Plan text:** "and a reduced junction release rate".
- **Proposal:** minimum headway between grants per approach =
  `h0·(1 + V_bg/C)` with `h0 = 1.8 s`, applied in `junction_ctl`.

**A29 · H · ALGORITHMS.md §13 L488–490**
- **Plan text:** "Cycle detection (wait-for graph via junction requests)".
- **Missing:** most network gridlocks are exit-space and leader waits, not request-on-zone
  waits.
- **Proposal:**
  - Graph nodes are vehicles with `stuck_s > 20 s`.
  - Edges: v→w if w holds a zone v needs; v→(last vehicle on target lane) if v is blocked by
    exit space; v→leader if v is queued behind it.
  - Run DFS every 10 s, sliced; break each cycle by despawning the vehicle with the latest
    trip-start tick (tie: VehicleId).
  - Log the cycle length and node ids.

**A30 · H · ALGORITHMS.md §13 L452–456**
- **Plan text:** "mandatory (route needs a lane for the next turn: change within the last
  150 m …)".
- **Missing:** segments can be 6 m long (L59) and junctions close together, so the 150 m
  zone spans several links; no look-ahead is defined.
- **Proposal:** at route assignment compute a **lane plan**, the set of admissible lanes per
  link, propagated backward across links until the cumulative length ≥ 40 m per required
  lane change. The vehicle aims at the plan from the first link. Routing adds +30 s to turn
  edges that need ≥ 2 changes within 60 m.

**A31 · M · ALGORITHMS.md §12 L425–427**
- **Plan text:** "remaining route cost … exceeds the best alternative by > 30 % (checked at
  most every 30 s per vehicle, budgeted)".
- **Missing:** the budget, and how the alternative is computed.
- **Proposal:** only vehicles whose next 3 links have congestion > 0.7 are candidates. Each
  candidate costs one A* search inside the routing time budget, at most 20 per tick. Use a
  cached route if the cache entry is fresh.

**A32 · H · ALGORITHMS.md §12 L409–411; ADR-0011 L178–179**
- **Plan text:** landmarks are "recomputed on topology change in a background slice".
- **Missing:** the slicing, and the heuristic used while the tables are stale.
- **Proposal:**
  - Recompute 32 Dijkstras (16 landmarks × forward/back), 2 per tick, tick-sliced (not
    async, for determinism), and swap atomically.
  - Edge removals keep stale ALT admissible (distances only grow).
  - After any edge **addition**, use `h = euclid / v_max` until the swap, because a stale
    ALT can overestimate.

**A33 · H · ARCHITECTURE.md §13 L548**
- **Plan text:** "vehicles regenerated from in-flight trips".
- **Missing:** where on the route a regenerated vehicle goes, and its speed and grants.
- **Proposal:** see G-S1. Save the agent store; if you refuse, restart in-flight trips at
  their origin with the elapsed time credited, and state that save/load continuation is not
  bit-exact with traffic on.

**A34 · M · ARCHITECTURE.md §13 L546–547**
- **Plan text:** "fields recomputed or loaded".
- **Proposal:** decide now. Save `ground_pollution`, `water_pollution` and the LV smoothing
  state as u16-quantised 512² rasters (1.5 MB before LZ4); noise is recomputed.

**A35 · M · ARCHITECTURE.md §7 L417**
- **Plan text:** "shader warmup".
- **Proposal:** during `Loading`, render one hidden instance per material × specialisation
  (prepass, shadow, LOD-dither on/off, overlay on/off, transparent) with an offscreen
  camera until `PipelineCache` has no queued or creating pipelines (cap 10 s). Log the count
  and add it to the benchmark report. On Metal, pipeline creation hitches are 50–300 ms
  each if they happen mid-game.

**A36 · M · ARCHITECTURE.md §7 L418**
- **Plan text:** "menu over an animated procedurally generated backdrop city".
- **Missing:** the city generator is not scheduled anywhere (also needed for the M4 10k
  scene and the M9 bench).
- **Proposal:** create `cb_sim::testing::citygen` in M4: an organic road generator plus
  plots plus forced growth, deterministic by seed. Reuse it for the benchmark, the soak
  tests and the backdrop (300 buildings).

**A37 · M · ADR-0004 L55; ART_DIRECTION L38–40**
- **Plan text:** "calibrated once in M2 against a reference screenshot".
- **Missing:** the reference and the metrics.
- **Proposal:** metrics on the M2 noon shot:
  - median linear luminance after tonemapping 0.18–0.24;
  - lit grass within ΔE00 < 6 of #8DB06A;
  - shadow/lit ratio 0.35–0.50;
  - clipped pixels ≤ 0.3 %.

  For the night shot, mean luminance is 0.30–0.45 × noon. Compute these with a
  `--screenshot-metrics` mode that writes JSON.

**A38 · H · ADR-0019 L135–137; RISKS R11**
- **Plan text:** "Quality depends on careful DSP tuning; a listening pass by the user".
- **Missing:** objective checks the agent can run itself.
- **Proposal:** a `cb_audio` test suite:
  - integrated loudness per sound within ±1 LU of target (UI −20 LUFS, one-shots −18,
    ambient beds −28, sirens −16);
  - true peak ≤ −1 dBTP;
  - loop-seam step < −60 dBFS;
  - |DC| < 1e-3;
  - spectral centroid within the per-class range;
  - no NaN/denormals.

  Also write spectrogram PNGs for the agent's own review. The user listening pass becomes
  optional and non-blocking.

**A39 · L · ADR-0020 L177–178**
- **Plan text:** "skipped while a modal command is in progress".
- **Proposal:** "modal" = tool drag active ∨ dialog open ∨ `CommandQueue` non-empty. Retry
  every 5 s and force a save after a 60 s delay, between frames.

**A40 · H · ADR-0035 L117–121; DATA_MODEL §8**
- **Plan text:** "transfers handled by re-queuing at the transfer stop".
- **Missing:**
  - Trip has no itinerary field;
  - "downstream" is undefined for non-circular lines;
  - stop capacity overflow ("40 waiting") is not handled;
  - whether a bus stops in the lane or in a bay is unspecified.
- **Proposal:**
  - `Trip.legs: SmallVec<[TransitLeg; 3]>` (line, board stop, alight stop).
  - Non-circular lines run ping-pong (forward stop list, then reversed).
  - When a stop is full, new transit trips re-choose a mode and the walk/car utility gets a
    −5 min penalty.
  - Buses stop in the curb lane (blocking) unless the segment has a bus lane, in which case
    they stop in the bus lane.
  - Trams stop in their lane.

**A41 · L · ADR-0035 L126–127**
- **Plan text:** "land value bonus near stops scales with line frequency".
- **Proposal:** `bonus = 0.10 × min(1, 600 s/avg_headway_s) × (1 − smoothstep(0.6, 1, walk_m/400))`.

**A42 · L · ART_DIRECTION L72**
- **Plan text:** "roads partly cleared".
- **Proposal:** per-segment shader parameter
  `road_snow = snow_cover × (1 − clamp(volume_vph/400, 0, 0.8))`, written to the road param
  buffer hourly.

**A43 · L · ART_DIRECTION L179**
- **Plan text:** "clusters when > 3 overlap".
- **Proposal:** cluster on a 48-logical-px screen grid per frame; the badge shows the count
  and the worst severity; clicking zooms to the cluster's bounding box.

**A44 · L · GAME_DESIGN L201**
- **Plan text:** "throttled per kind".
- **Proposal:** at most 1 per kind per 30 real s and at most 1 per subject per game day;
  critical notifications are never throttled.

**A45 · L · ALGORITHMS §13 L488–489**
- **Plan text:** "> 20 despawns/game-hour in one area".
- **Proposal:** an area is a 256 m bucket; the notification is placed at the despawn
  centroid.

**A46 · M · PERFORMANCE L18–19**
- **Plan text:** "autosave hitch ≤ 30 ms", "road commit visible ≤ 100 ms".
- **Missing:** a measuring scenario.
- **Proposal:** bench scenarios `bench_autosave_max` (snapshot time p99) and
  `bench_road_commit` (100 random 200 m roads; time from command to mesh visible = p95 of
  frame index delta × frame time).

**A47 · H · TESTING L74**
- **Plan text:** "commute EMA reasonable, population grows".
- **Proposal:** concrete thresholds, see F-9.

**A48 · M · RISKS R2 L11**
- **Plan text:** fallback "lower junction request frequency (10 Hz) and vectorize IDM".
- **Missing:** a 10 Hz junction step breaks the 20 Hz invariant of the grant/lock pass; the
  plan says nothing about it.
- **Proposal:** use the fallback ladder in C-1.

**A49 · M · RISKS R12 L21**
- **Plan text:** "tune factor in M9".
- **Proposal:** pass criteria from spike C-4.

**A50 · H · RISKS R15 L24**
- **Plan text:** "depth over breadth … defer stretch".
- **Missing:** a cut list and triggers.
- **Proposal:** a scope-cut ladder in PROGRESS.md, applied in order whenever a milestone runs
  more than 1.5× its estimate:
  1. generative music;
  2. Ultra real point lights / TAA;
  3. freehand road mode;
  4. articulated trams (single-body tram);
  5. puddles/splashes;
  6. visual pedestrians;
  7. cloud shadows;
  8. landmark buildings.

**A51 · M · ADR-0031 L208–211**
- **Plan text:** the in-house inspector shows components "via `bevy_reflect`".
- **Missing:** the important state (`TrafficWorld`, `Population`, `LaneGraph`, `Fields`)
  lives in pure-crate types that cannot derive `Reflect` (no Bevy dependency).
- **Proposal:** a `DebugView` trait implemented in `cb_game::debug` for each resource
  (paged egui tables plus a lookup by ID); schedule TrafficWorld/LaneGraph views in M3/M9.

**A52 · H · ALGORITHMS §1 L57–61, §14 L499–505**
- **Plan text:** "buildings/plots in the corridor (warn; demolish on confirm)".
- **Missing:** whether the whole plot goes or only the overlap; what happens to existing
  plots when a *new junction's* cut-back and polygon trims their frontage.
- **Proposal:** clip plots by the new corridor and junction polygons (i_overlay difference),
  re-validate and re-subdivide. If a plot becomes invalid, remove it after confirmation. The
  preview lists clipped and removed plots and buildings.

**A53 · H · GAME_DESIGN §3 L51; PROGRESS M3 L111–112**
- **Plan text:** "upgrade/downgrade in place".
- **Missing:** street (12 m) → avenue (22 m) widens the corridor by 5 m per side into plots
  and buildings; no rule.
- **Proposal:** the same rule as A52, with the preview showing the cost of removed
  buildings. Downgrades shrink the corridor, and the plot frontage re-attaches via the 2 m
  rule (L503).

**A54 · M · ADR-0010 L143; ALGORITHMS §13**
- **Plan text:** "vehicles despawn into buildings and spawn from them".
- **Missing:** spawning onto an occupied lane.
- **Proposal:** a per-access-point spawn queue. Spawn when the gap ≥ length + s0 at the spawn
  `s` and the upstream vehicle's time-to-collision is > 2 s. If the queue wait exceeds 30 sim
  s, complete the trip as macro (counted in diagnostics).

**A55 · M · skills/visual-verification §3; ART_DIRECTION L5–6**
- **Missing:** a capture "settle" rule; async mesh jobs and pipeline compiles cause missing
  buildings or pink frames.
- **Proposal:** capture only when mesh jobs pending = 0 ∧ pipelines pending = 0 ∧ ≥ 30
  frames since the last change ∧ (crossfades finished, or LOD crossfade disabled for
  captures).

**A56 · M · ALGORITHMS §24 L686–699**
- **Plan text:** "one entity per chunk per LOD".
- **Missing:** a residency policy; see D-9 (memory).
- **Proposal:** generate and upload LOD0 only for chunks within 1.3 × the LOD0 range; evict
  beyond 1.6 ×. LOD1 for all chunks; LOD2/3 always.

**A57 · L · ADR-0021 L25–27**
- **Plan text:** "generated/checked constant module (`keys.rs`)".
- **Proposal:** a `cb_l10n/build.rs` parses `locales/en/*.ftl` and emits
  `pub const TOOL_ROAD_STRAIGHT: Key`. A test diffs the en and de key sets and the
  placeholders.

**A58 · M · PROGRESS M8 L170; ART L206**
- **Plan text:** "tooltips everywhere".
- **Proposal:** make it structural. `ui::widgets::{button, toggle, slider}` require a
  `tooltip: Key` argument, and a grep-based test fails on direct `ui.button(` /
  `ui.add(egui::Button` in `cb_game/src/ui/**`.

---
## B. Unconfirmed assumptions

The user confirmed: the brief (incl. 4×4 km, 10k/50k/5k at 60 FPS mid-range, milestone
order, macOS required), Rust/Bevy 0.20, no assets. Everything below is an assumption the
plan makes on the user's behalf.

| ID | Assumption (where) | Tag | Why it matters / what to ask |
|---|---|---|---|
| B1 | Time model: day = 24 real min at 1×, 1 game min = 1 sim s, vehicles at real speed, perceived commute = 0.1 × sim s (GAME_DESIGN L22–36, ADR-0006 L133–139) | DS | Drives trip concurrency (peak ≈ 15–25k trips at 50k pop vs 5k agents), schedules, all rates. Spike C-4 before M5. Ask: is "a commute visibly takes ~3 game hours" acceptable? |
| B2 | Day-length options 12/24/48 min change game-time-per-tick, not vehicle speed (ADR-0006 L135–136) | DS | At 12 min a 2 km trip = 5.7 game h; schedules break. Alternative: day length scales a "vehicle time dilation" factor. |
| B3 | "Lock time of day" is presentation-only; sim clock keeps running (ADR-0006 L144–145, ADR-0018 L80–81) | DS | User may expect the clock (and commute/night behaviour) to freeze; visuals then contradict sim (noon light, night traffic). Ask. |
| B4 | Right-hand traffic; right-before-left; left-turn yield (ALGORITHMS §6) | DS | Hard-coded side affects lane offsets, turn rules, priority. Make `DrivingSide` a catalog value now or confirm RHT. |
| B5 | Only all-way stops; no per-approach (two-way) stop signs (DATA_MODEL L88 vs ALGORITHMS §6) | DS | Brief: "player stop signs". Most players mean per-approach signs. |
| B6 | No road-geometry editing (move node / reshape curve), only build/upgrade/style/demolish (ARCH L452–459) | DS | Brief: "edit/remove with intersections updating". |
| B7 | Utilities implicit along every road; no power lines (ADR-0026, RISKS Q3) | DS | Removes a whole tool family; brief allowed ADR choice but the gameplay consequence ("disconnected networks need own plants") is unconfirmed. |
| B8 | Agents beyond `agent_cap` are statistically sampled (camera-independent), not "aggregate off-screen" (ADR-0010 L127–131) | DS | Brief wording suggests camera-based LOD. At 50k pop most peak traffic is invisible macro flow; ask whether visible jams must reflect real load. |
| B9 | Routing on segment-direction link graph, not lane graph (ADR-0011) | DS | Brief says "pathfinding on lane graph … decided in ADR" — acceptable, but lane plans become a separate risk (A30). |
| B10 | Pedestrians are visual only; no walking agents for walk/transit legs (ADR-0035 L120) | T | Brief: "pedestrians simple agents to stops (visual-only far)" implies near ones go to stops. |
| B11 | Reference PC = GTX 1660 Super / RX 5600 XT, 6-core, 16 GB, 1080p (PERF L14) | DS | The user's actual PC GPU is unknown; their runs at M2/M4/M9/M11/M16 won't validate a 2019 target if it is a 2024 GPU. Ask model; if faster, use a frame-time scaling rule or GPU downclock. |
| B12 | Apple M4 = base 10-core GPU, 16 GB, built-in 13–14" Retina, possibly fanless Air (PERF L15–17) | DS | User said only "Apple M4 Mac". Mac mini (external display), iMac (8- or 10-core GPU), MBA (throttles), MBP 14 (5.9 MP panel → Auto = 75 %, not 85 %) need different budgets. Ask exact model + display. |
| B13 | User will run smoke/benchmark/screenshot suites and do a listening pass at M1(R9), M2, M4, M9, M11, M13, M16 (TESTING L29, RISKS R9/R10/R11/R17–19) | DS | Process-shaping; conflicts with "implement without stopping". Make these non-blocking, batched, and agreed up front. |
| B14 | Relaxing 4× to ≥ 50 FPS (PERF L13) | T | Brief says 60 FPS; confirm. |
| B15 | "60 FPS" = 1080p, *High* preset, 1× (PERF L13) | T | Brief doesn't name preset/resolution. |
| B16 | Low preset caps agents at 2 000 (PERF L91) | T | Violates "5k+ simultaneously simulated" on Low; confirm Low is out of the requirement. |
| B17 | Min OS: Win10 22H2, glibc 2.35, macOS 14 (ADR-0037 A-P1) | T | |
| B18 | Bundle ID `org.citybuilder.CityBuilder` (A-P2); no notarization (A-P3) | T | Gatekeeper + App Translocation break "portable.txt next to the .app" (G-O2). |
| B19 | Repository license undecided; releases built anyway (RISKS Q8, ADR-0032 L31) | T | Tagged releases without a LICENSE; also needed to ship egui fonts' licenses (Ubuntu Font Licence, Hack/Bitstream, OFL/Apache for Noto/emoji). |
| B20 | GitHub macOS runner minutes are acceptable (ADR-0038 L84–85, 10× on private repos) | T | Every push runs 3 OS + release-mode smoke; ask repo visibility/budget. |
| B21 | Currency shown as `$` (en) and `€` (de) for the same money (RISKS Q5) | T | |
| B22 | Calendar "Year 3, Spring, Day 2", 24 h clock in both locales (ADR-0021 L28–30) | T | en users may expect 12 h. |
| B23 | Art style "Townscaper/Islanders spirit, central-European proportions" (ART L10–13) | DS | Typology/generator design depends on it; confirm with 3 mood screenshots in M2 before building M4 generators. |
| B24 | egui default fonts only; OFL font needs user OK (RISKS Q6, ADR-0024 L176–177) | DS | Verified glyph gaps (⌘⌥⇧⌃⌫, →) — either user approves a font asset or glyphs are procedural icons. |
| B25 | No hard game over (GAME_DESIGN L14–15) | T | |
| B26 | Starting conditions: 2–3 outside road stubs, pre-built, town hall free (ALGORITHMS L242–244, GAME_DESIGN L90) | T | Town hall placement rule not specified (auto? player?). |
| B27 | Progression thresholds and unlock contents (GAME_DESIGN §8) | T | Traffic lights at 1 200 pop but stop signs at 250; trams at 10 000. |
| B28 | Undo depth 100, undo refunds 100 % at any age (ADR-0015 L153–157) | T | Exploit vs demolish 0 % (G-B5). |
| B29 | Demolish refund 25 % within 1 game day (GAME_DESIGN L155) | T | |
| B30 | Max junction angle/degree rules: min 25°, no junctions on bridges, no special roundabout (ALGORITHMS L59–60, ADR-0009 L84, L91–92) | DS | Players build roundabouts and bridge ramps with side streets; rejecting them is a gameplay limit. |
| B31 | Grade separation only via manual PgUp/PgDn elevation, Δh ≥ 5.5 m (ALGORITHMS L52–55, RISKS Q9) | T | No interchange ramps/auto-overpass. |
| B32 | Water bodies are immutable; no land reclamation/dams (ALGORITHMS L255–257) | DS | Shoreline gameplay (pumps/outlets) depends on it. |
| B33 | Terrain resolution 2 m (ADR-0012) | DS | 6 m dirt paths span 3 samples; cut/fill edges stair-step (G-G3). |
| B34 | Building identity seeded by `BuildingId + level`, not lot ID (DATA_MODEL L182) | T | Brief: "deterministic from world seed + plot/lot ID". Regrowth on same lot looks different. |
| B35 | Citizens: households 1–5, cars/household by wealth, lifecycle "lightweight" (GAME_DESIGN §10) | T | |
| B36 | Mode choice logit form and β (ALGORITHMS L542–546) | T | |
| B37 | Shift distributions N(8:00, 50 min) etc. (ALGORITHMS L536–541) | T | With 1 game min = 1 sim s, σ=50 game min is only 50 s of real driving time → peaks are extremely sharp (C-4). |
| B38 | Walk thresholds: walk mode < 800 m, stop access ≤ 600 m, transfer ≤ 300 m, LV transit ≤ 400 m (ALGORITHMS L431, L436, ADR-0035 L125) | T | Inconsistent numbers; confirm. |
| B39 | Commercial opening hours 7–21 (ADR-0018 L76–77) | T | |
| B40 | Weather Markov tables, effects magnitudes (GAME_DESIGN §11) | T | |
| B41 | Weather off = always clear but seasons still change visuals (GAME_DESIGN L192–193) | T | |
| B42 | Q/E only snap 90° on tap; holding does nothing (ALGORITHMS L732–733) | T | Brief: "rotate MMB drag or Q/E" suggests continuous Q/E rotation too. |
| B43 | Right-drag rotates, right-click cancels; left-drag on empty ground pans while a tool is active (ADR-0039 L169, L180–181) | T | See G-I3. |
| B44 | Fire spreads 6 m (ALGORITHMS L581) | T | With R side setbacks 1.5 m, whole rows burn. |
| B45 | Autosave every 10 min real, 3 slots (ADR-0020 L176–177) | T | |
| B46 | Saves need not be portable between OSes (ADR-0017 L34–38, TESTING L85) | DS | User owns Windows and Mac; moving a save is likely. |
| B47 | Lavapipe screenshots are representative and agent self-review suffices for "must look good" (skill visual-verification §6) | DS | M2 gate is subjective; propose objective checks (F-0). |
| B48 | Agent can read CI results via GitHub MCP (skill milestone-completion §9) | T | If not available, step 9 can't complete → milestone never "done". |
| B49 | Max city ≈ 10k buildings / 50k citizens; no upper cap enforced (PERF §1) | T | What happens at 100k? Add a soft cap or degrade rule. |
| B50 | Sandbox = unlimited money + all unlocked (GAME_DESIGN L15) | T | |
| B51 | Seasons 4 days, year 16 days (GAME_DESIGN L24–26) | T | Education/ageing pace depends on it. |
| B52 | Effort fits 16 milestones for one agent without estimates (whole plan) | DS | No estimates exist; see G-X1. |

---

## C. The five highest-risk parts (ranked) with spikes

I replaced "bridges" with the **time model and trip scale** (it silently invalidates
traffic volumes, balance and agent counts and is cheap to test), and folded bridges into
the terrain/road-interface risk where their real problems (abutments, profile coupling,
stamp overlap) live.

### C-1 (rank 1) Traffic microsimulation at scale: junction admission, lane plans, gridlock, routing load

**Why risky here.**
- Admission is a novel conflict-zone locking scheme (ADR-0009) layered on IDM and a
  lane-change model, on freeform junctions with segments as short as 6 m.
- The deadlock proof is wrong (G-T1): exit space is not reserved, and right-before-left
  stalls when every approach is occupied.
- Lane plans across short links are unspecified (A30).
- Routing has no time cap, only a count cap of 300.
- Parallel stepping reads a previous-tick snapshot, and the sequential pass handles all
  lane transfers, so it may dominate.
- Background macro flow slows agents only via v0 scaling, so agent queues and macro queues
  disagree.
- Agent state is not saved (G-S1).

**Failure modes:**
- vehicles frozen in junction boxes;
- 12 s stalls at every equal-priority junction;
- mandatory lane-change failures causing chain stops;
- network gridlock that the despawn timer (90 s) turns into a visible "teleporting" city;
- routing spikes after a bulldoze (thousands of reroutes);
- sequential-pass cost exceeding 1 ms;
- p collapsing to 0.2 at peak, so jams are invisible.

**Spike S-T (spikes/traffic, headless, no rendering):**
- **Build:**
  - a synthetic 4×4 km network generator: an organic grid with 6 000 segments, mixed
    T/X/Y/5-way, 4-lane avenues, one-ways and a 6-entry ring;
  - straight-line lane polylines plus Bézier connectors;
  - conflict zones;
  - the full admission rule with exit-space **reservation**;
  - IDM, mandatory and MOBIL lane changes, lane plans;
  - ALT A* with variants and an LRU cache;
  - OD from synthetic homes and jobs;
  - stuck detection and the wait-for cycle breaker.
- **Skip:** real junction meshes, terrain, transit, economy, save.
- **Test cases:**
  1. 4-way equal priority, all approaches saturated, 1 h;
  2. signalized avenue×avenue at V/C 0.9 and 1.2;
  3. a 25 m block between two signals where 60 % of vehicles need 2 lane changes;
  4. a 2→1 lane merge on an avenue;
  5. a 6-entry ring at V/C 1.0;
  6. full network with 5 000 / 8 000 / 12 000 agents, 2 game days;
  7. bulldoze a central avenue mid-peak;
  8. 1 vs 4 vs 8 worker threads.
- **Pass/fail criteria:**

  | Metric | Pass |
  |---|---|
  | Vehicle step, 5 000 agents (container, 4 threads) | p95 ≤ 0.9 ms; sequential pass ≤ 0.35 ms |
  | Junction control | ≤ 0.3 ms |
  | Routing | ≤ 0.8 ms wall p99 per tick with a time cap, including case 7 |
  | Overlaps / conflicting zone occupancy | 0 |
  | Saturated signal discharge | ≥ 1 500 veh/sim-h/lane green |
  | Unsignalized equal junction (case 1) | never idle > 6 s while vehicles wait |
  | Despawns at V/C ≤ 1.0 | < 1 per 1 000 trips |
  | Case 2 at V/C 1.2 | flow recovers within 180 s after demand drops |
  | Thread counts 1/4/8 | identical state hash |
- **Time-box:** 5 agent-days.
- **Fallback, in order:**
  1. Signals forced at junctions with ≥ 4 approaches of equal class, and roundabouts
     modelled as a special controller.
  2. A mesoscopic link model (point queues with capacity per lane, junction transfer by
     capacity). Agents become visual interpolation along queue positions near the camera,
     with a deterministic queue state in the sim.
  3. Agent cap 3 000 with a larger macro share.

  Record the choice in a superseding ADR-0010/0009.

### C-2 (rank 2) Intersection geometry on freeform, sloped, mixed-width roads (incl. tram radii)

**Why risky here.**
- Cut-backs come from Bézier edge intersections of untrimmed curves, with a fillet per
  corner and polygon repair by self-union.
- Short segments (6 m) and the 45 % cut-back cap interact badly: a 30 m segment between two
  avenue junctions can't fit two 9 m corner radii plus crosswalks.
- The junction surface is flat (`surface_h`, G-G1).
- Tram connectors need r ≥ 20 m (GAME_DESIGN L47), but the connector clamp is 1/5 m
  (ALGORITHMS L161), so trams through junctions are undefined until M10.
- Lane-to-lane connector rules ("each target lane receives at most 2 source lanes") can
  leave movements without connectors on asymmetric junctions.

**Failure modes:**
- self-intersecting or overlapping junction polygons;
- connectors leaving the polygon or crossing on the same approach;
- missing movements, which become routing failures;
- steps or gaps between approach meshes and the junction surface;
- excessive "intersection too close" rejections that frustrate players;
- junction regeneration cost spikes on drag previews (every mouse move).

**Spike S-J (spikes/junctions, pure Rust + SVG/OBJ dumps):**
- **Build:** a random junction generator (2–6 approaches, angles 25–180°, widths 6/12/16/22 m
  plus tram, approach curvature up to 1/r_min, segment lengths 6–60 m, grades up to
  max_grade in random directions). Run cut-back, polygon, crosswalks, connectors, conflict
  zones, a plane-fitted surface, and a mesh of approach end strips.
- **Skip:** traffic control, rendering, undo.
- **Test cases:**
  - 10 000 random junctions;
  - canonical T/X/Y-30°/5-way/avenue×street/avenue×avenue/one-way pair;
  - two junctions 30 m apart;
  - 8 % slope junction;
  - tram straight and tram turn on an avenue median.
- **Pass/fail criteria:**
  - 0 panics; 100 % of cases produce valid geometry or a typed `ValidationError`;
  - "standard" distribution (angles ≥ 40°, segment length ≥ 30 m) rejection ≤ 3 %;
  - polygon simple after repair; every allowed movement has a connector;
  - same-approach connectors never cross; connector κ ≤ 1/5 m (car) and ≤ 1/20 m (tram);
  - connectors inside polygon ∪ approach strips within 0.3 m;
  - |approach mesh edge − junction surface| ≤ 0.02 m at cut lines on 8 % slopes;
  - p99 generation ≤ 0.4 ms per junction (preview-safe).
- **Time-box:** 4 agent-days.
- **Fallback:**
  - cap the degree at 4 and the min angle at 40°;
  - parameterised T/X templates for these degrees (rotated, scaled), with freeform only
    for curve continuations;
  - force a flat 6 m "landing" of the profile at both ends so the planar surface works;
  - tram turns only at dedicated wide-radius junctions.

### C-3 (rank 3) Freeform plots → lots → buildings (subdivision + fitting + stability)

**Why risky here.**
- Strip subdivision with PCA-blended rays (β = 0.3) behaves differently on convex, concave,
  S-curved and corner frontages.
- Plot side and back edges are built from averaged normals (A20).
- Corner plots spanning two segments need two ray families that intersect.
- Lots must still produce good houses through rect_fit, with rectangularity ≥ 0.55.
- Stable IDs under edits drive whether buildings survive (A19).
- New junctions and road upgrades later clip plots (A52/A53).
- This is the brief's "Manor Lords style" headline feature, and M2 is gated on it looking
  good.

**Failure modes:**
- slivers and fan-shaped lots on curves;
- crossing rays on concave curves;
- corner lots with two frontages producing L-shapes that rect_fit fills badly;
- lot IDs churning on a 0.5 m vertex drag, so buildings flicker or are demolished;
- buildings overlapping lot boundaries or facing the wrong road;
- p99 time per plot too high for live preview.

**Spike S-P (spikes/plots, pure Rust, SVG contact sheets):**
- **Build:** frontage generators (straight; convex r = 30–300 m; concave r = 25–300 m;
  S-curve; corner around 60–120° junction corners; plots spanning 2 segments), depth
  15–80 m, the subdivider with N auto and N ± 3, rect_fit, and L1–L3 massing footprints.
- **Edit sequences:** vertex drag 0.25/1/3 m, N ± 1, depth ± 5 m.
- **Skip:** terrain, roofs, rendering beyond SVG.
- **Inputs:** 10⁴ random plots plus a fixed showcase set of 24.
- **Pass/fail criteria:**
  - 0 panics; 100 % of lots touch the frontage with ≥ w_min;
  - area conserved within ±0.5 %; no lot overlaps;
  - ≥ 95 % of lots have rectangularity ≥ 0.55 before merging, and ≤ 3 % are merged;
  - ≥ 90 % of R lots fit an L1–L3 footprint ≥ 50 m² with setbacks;
  - ID stability: drags ≤ 1 m keep ≥ 90 % of LotIds, and N ± 1 keeps ≥ (N − 2)/N;
  - p99 ≤ 2 ms per plot (subdivision + fit);
  - the showcase contact sheet is reviewed by the agent against a written checklist
    (no fan > 25° between adjacent side edges; corner lot frontage on both roads).
- **Time-box:** 4 agent-days.
- **Fallback:**
  - frontage-drag only (no free polygon tool);
  - lots as offset strips with parallel side edges (no PCA blend) on concave curves;
  - a dedicated corner-lot generator (quarter-rectangle along both frontages);
  - polygon-extruded "townhouse rows" for lots failing rect_fit.

### C-4 (rank 4, replaces "bridges") Time model and trip scale

**Why risky here.**
- 1 game minute = 1 sim second while vehicles drive at real speed (GAME_DESIGN L28–36).
  A 2 km trip takes 170 game minutes; a cross-map trip of 5 km takes ~7 game hours.
- With shifts at N(8:00, 50 game min), ~20k morning arrivals land within ~200 sim s while
  each trip lasts ~170 sim s, so ≈ 17k concurrent trips (plus school trips) against
  `agent_cap` 5 000, giving p ≈ 0.25.
- Unit ambiguity runs through the rates:
  - capacity "800/lane/h";
  - flow EMA half-life "10 game minutes", which is only 10 sim s;
  - cost epoch "15 game minutes" = 15 sim s;
  - rerouting "30 s";
  - stuck timeout "90 s";
  - fire growth "dt";
  - all of which sit beside hourly economy and incident rates.
- Day-length options change all of it.

**Failure modes:**
- commuters departing at 01:00;
- "night traffic" never quiet;
- shopping trips ending at 23:00;
- BPR capacity inconsistent with agents by ~27×;
- noisy flow estimates driving route flapping;
- balance numbers tuned at one day length breaking at another;
- visible traffic not representing the real load.

**Spike S-Time (spikes/timemodel, headless Rust or a script, no rendering):**
- **Build:**
  - synthetic 50k-citizen city (homes and jobs from the city generator layout, or a
    random clustered layout on 4×4 km);
  - trip durations from network distance at road speeds, plus 20 % junction delay;
  - the plan's scheduler (backward scheduling, shifts, shopping, school);
  - a point-queue link model for congestion feedback;
  - day lengths 12/24/48.
- **Measure:**
  - commute duration in game hours (p50/p95);
  - departure-time histogram;
  - concurrent trips over the day;
  - required p at agent_cap 5 000;
  - fraction of workers late by > 30 game min;
  - fraction departing before 04:00;
  - night (00–04) share of traffic;
  - peak/off-peak ratio.
- **Pass/fail criteria (at 24 min):**
  - p95 departure ≥ 04:30;
  - ≥ 90 % arrive within ±30 game min of shift start;
  - night share ≤ 10 %;
  - peak/off-peak ≥ 3;
  - p_min ≥ 0.4 at 50k pop (i.e. ≥ 40 % of peak trips are visible agents);
  - results within ±20 % across 12/24/48 min.
- **Time-box:** 2 agent-days.
- **Fallback options (pick by measurement, then write a superseding ADR-0006):**
  1. a vehicle time-dilation factor k = 2–4 (vehicles move k× in sim time; perceived =
     0.1 × k × sim s);
  2. a slower clock (1 game min = 2 sim s; day = 48 real min);
  3. a commute cap in job matching (reject jobs with estimated trip > 240 sim s);
  4. a higher agent cap with a meso model (C-1 fallback 2).

  Fix units in all docs: every flow in **veh per sim-hour**; every EMA and epoch in **sim
  seconds**.

### C-5 (rank 5) Road–terrain interface: grading, junction surfaces, LOD, bridges and abutments

**Why risky here.**
- Terrain is a 2 m heightfield with 4 LODs. Roads are separate meshes 5 cm above "exactly
  stamped" samples.
- The flat shoulder is 1.5 m, less than the 2.83 m cell diagonal (G-G3).
- Stamp compositing picks a single winning stamp per sample, so adjacent embankments of two
  roads produce cliffs (G-G2).
- Junction surfaces are planar at node height while approaches are graded (G-G1).
- Bridges re-solve the deck profile separately, and their abutment pads must meet the
  embankment, the deck and pier footprints.
- Grade separation does not force a bridge span over the lower road (G-G4).
- LOD1+ terrain under-samples corridors, so terrain pokes through roads at distance.
- RISKS R5's check covers only LOD0 lane samples.

**Failure modes:**
- terrain triangles through road surfaces at edges or in the distance;
- floating road skirts on fills;
- cliffs between parallel roads;
- steps at junctions;
- abutment gaps;
- lower roads buried under an overpass embankment;
- undo not restoring identical heights after compositing changes;
- re-stamp and re-mesh hitches when dragging road previews.

**Spike S-G (spikes/grading, Rust + lavapipe renders):**
- **Build:** seeded 2049² terrain with
  - 150 random roads (cut/fill);
  - 30 junctions on 4–10 % slopes;
  - 5 pairs of parallel roads 8–20 m apart at different heights;
  - 6 bridges (river, 30 m valley, an overpass over a street);
  - the stamp composite variants under test (max-weight vs. continuous min/max-fill-cut
    blend);
  - chunk meshes LOD0–3 with skirts.
- **Automated sampler:** for each lane sample and each edge sample (±0.5 m beyond the
  carriageway), compute the road surface height − rendered terrain height (barycentric on
  that LOD's mesh), at every LOD that would be visible at that distance.
- **Render:** 12 fixed lavapipe shots.
- **Skip:** buildings, plots, traffic.
- **Pass/fail criteria:**
  - under the carriageway at LOD0: terrain ≤ road − 0.03 m everywhere;
  - LOD1: ≤ road − 0.0 m for ≥ 99.9 % of samples, with the rest hidden by skirts in the
    renders;
  - outside corridors, |Δh| between adjacent samples ≤ tan(max side slope)·2 m + 0.05 m (no
    cliffs);
  - junction-to-approach step ≤ 0.02 m;
  - abutment-to-deck step ≤ 0.02 m;
  - piers never inside another corridor;
  - overpass: the lower road's corridor height is unchanged by the upper road;
  - remove-stamp restores the base exactly (bitwise);
  - re-stamp + LOD0 re-mesh of a 300 m road ≤ 25 ms async and ≤ 1.5 ms on the main thread.
- **Time-box:** 4 agent-days.
- **Fallback:**
  - carve a 0.15 m "trench" under every corridor plus a road mesh with depth bias and 0.6 m
    skirts;
  - conservative (min) downsampling inside corridor masks for LOD1+;
  - continuous composite:
    - fill stamps `max(base, target)` blended by weight;
    - cut stamps `min(...)`;
    - priority used only for pads vs. roads;
  - bridges limited to straight horizontal spans with ramps on embankments.

**Honourable mentions (not ranked):** undo/redo against a running simulation (ADR-0015
L149–156; evictions, regrowth, refunds); M4 Retina fill rate (D-6); AI-only visual sign-off
on lavapipe (F-0).

---
## D. Frame-time budgets (PERFORMANCE.md) — critique and corrected tables

**D1 · C · PERFORMANCE L33 — ticks per frame are wrong.**
- The plan says "4.0 ms at 1× (≈ 1.2 ticks/frame), 9 ms at 4×".
- At 60 FPS and 20 Hz the real figures are:
  - 1×: **0.33** ticks/frame. Two frames in three run no tick, and one frame runs one tick.
  - 2×: 0.67 ticks/frame.
  - 4×: **1.33** ticks/frame, so frames run 1, 1, 2 ticks.
  - The 4× target of 50 FPS gives 1.6 ticks/frame, so frames run 1–2 ticks.
- The per-tick table sums to 1.2 + 0.3 + 0.3 + 0.8 + 0.6 + 0.1 = **3.3 ms**.
  - At 1× that is 1.1 ms on average but **3.3 ms in the worst frame**.
  - At 4× it is 4.4 ms on average and **6.6 ms in the worst frame**.
- So 1× is overbudgeted and 4× is budgeted against a fictitious 2.7 ticks/frame.
- Budget **worst frames**, not averages. Frame pacing and hitches are what players see.

**D2 · H · ADR-0006 L128–130 — `max_ticks_per_frame = 12` allows spirals.**
- A 150 ms hitch at 4× queues 12 ticks, which at 3.3 ms each is ≈ 40 ms. That makes the next
  frame long, which queues more ticks.
- Fix: cap at `ceil(1.5 × speed × dt_frame_nominal/dt)`, i.e. 1, 2 and 3 ticks at 1×, 2× and
  4×, and drop the remainder.
- Also add an adaptive rule: if `SimEdit + ticks` on this frame would exceed 7 ms, defer one
  tick to the next frame. The accumulator is preserved, so this stays deterministic.

**D3 · H · PERFORMANCE L36 — extract and render-thread work are misattributed.**
- The plan has "Bevy extract + prepare (render world) 4.0 ms, pipelined, overlaps next
  frame".
- Under Bevy's pipelined rendering, **extract runs on the main thread**. It is a sync point
  that borrows both worlds and blocks until the render thread has finished the previous
  frame.
- **Prepare, queue, sort and graph encoding run on the render thread.**
- Two consequences:
  - The main-thread table must include extract, about 1–2 ms with 3 000 moving visual
    agents.
  - There must be a **separate render-thread budget**, which is currently missing. A render
    thread over 16.6 ms stalls the main thread at extract.

**D4 · H · PERFORMANCE L46, ALGORITHMS L420–423 — the routing worst case is ~10–40× the
average.**
- The cap is 300 searches/tick at 0.2–0.6 ms each, which is 60–180 ms of CPU.
- On Bevy's default compute pool, a 6-thread i5-9600K gets only about 4 workers after the IO
  and async pools take theirs. That is **15–45 ms of wall time in one tick**.
- Triggers:
  - a bulldoze that invalidates thousands of cache entries plus forced reroutes (L425);
  - a topology change at peak hour;
  - the ALT landmark rebuild.
- The 64/tick stale-refresh budget (L417–418) is not in the table at all.
- Fix:
  - a **time cap**: routing ≤ 0.8 ms wall per tick, with leftover requests queued FIFO;
  - deterministic accounting by charging a fixed cost unit per settled node, not wall
    clock;
  - vehicles whose next link was removed continue to the next junction and wait there.

**D5 · M · PERFORMANCE L43 — "~150 ns/agent, 6-way parallel".**
- Bevy's compute pool on 6 hardware threads leaves about 4 workers.
- The sequential pass (lane transfers, lane changes, grants, all sorted by VehicleId) is not
  parallel and handles roughly 5–15 % of agents per tick, at 0.2–0.4 ms.
- The per-tick vehicle point-grid rebuild (DATA_MODEL L375–377) is missing. It costs about
  0.1–0.2 ms; it should be lazy and presentation-side.
- Budget the step at 0.3 ms parallel plus 0.4 ms sequential.

**D6 · C · PERFORMANCE L50–61, L96–99; ADR-0041 L271–273 — the GPU budget does not transfer
to the M4.**
- The 11.6 ms budget is for a GTX 1660 Super at 1080p (2.07 MP). The plan applies it to
  "≤ 3.7 MP rendered", which is 1.8× the pixels on a GPU with ~0.85× the FP32 throughput and
  ~0.36× the memory bandwidth (120 vs 336 GB/s).
- Pixel- and bandwidth-bound passes (SSAO, bloom, post, opaque shading, water) scale
  accordingly. The M4 would land at 15–18 ms, not 11.6 ms.
- The "Auto" arithmetic is also wrong:

  | Display | Native MP | At 85 % | At 75 % | At 67 % | Correct "Auto" by the plan's own rule |
  |---|---|---|---|---|---|
  | MBA 13" 2560×1664 | 4.26 | 3.08 ✓ | — | — | 85 % ✓ |
  | MBP 14" 3024×1964 | 5.94 | 4.29 ✗ | 3.34 ✓ | — | **75 %, not 85 %** |
  | MBA 15" 2880×1864 | 5.37 | 3.88 ✗ | 3.02 ✓ | — | 75 % |
  | 4K 3840×2160 | 8.29 | — | — | 3.72 ✗ at 0.67 (3.69 ✓ only at exactly 2/3) | **50 % unless the step is exactly 2/3** |

- Fix:
  - set Auto's target to **≤ 2.2 MP** (1080p-equivalent) on the M4 base GPU;
  - make the M4 default preset "Medium": 2 cascades, half-resolution SSAO, 4 bloom mips;
  - budget GPU time per machine (table below);
  - define the steps as exact fractions.

**D7 · H · PERFORMANCE L26–37 — the CPU table is missing items.**
- PostUpdate engine work: transform propagation, CPU visibility and frustum culling over
  every renderable entity, and `VisibilityRange` evaluation. This is about 1–1.5 ms at
  ≤ 15k renderable entities.
- egui output processing in PostUpdate. (egui itself is counted, but its placement is wrong,
  see G-W1.)
- Bevy system-dispatch overhead for about 400–600 systems: 0.3–0.6 ms.
- Mesh-job polling and upload scheduling.
- Tool preview on drag frames: junction regeneration, profile solve, subdivision, about
  1–4 ms.
- Audio-director sampling of the spatial indices.
- Debug overlays, which must be listed as excluded from the release budget.
- **Edit frames** (SimEdit) are not budgeted at all. The plan gives only 0.2 ms for the
  steady state.

**D8 · H · PERFORMANCE §3 — there are no explicit hourly, daily or seasonal spike slots.**
- "Staggered phases" (ARCH L327–329) is asserted, but all hourly systems are keyed off
  `HourElapsed`, so they fire on the same tick:
  - economy accrual over 30k entries: ~0.5 ms;
  - utility allocation with a multi-source Dijkstra over 16k links plus a sort of 10k
    consumers: 3–5 ms;
  - noise "recomputed from sources each game hour (not accumulated)" (L614) at 10k line
    sources × ~600 cells each: 10–25 ms if not sliced;
  - demand;
  - job matching.
- At season boundaries, ageing and education of 50k citizens adds 2–3 ms.
- Fix: a **phase table** in ARCHITECTURE §5. A game hour is 1 200 ticks, so give each
  sub-rate system a fixed tick offset and slice count:
  - economy @0;
  - demand @60;
  - utilities @120 in 4 slices;
  - noise over 64 slices from @200;
  - job matching over 200 slices;
  - dispatch every 20 ticks @7.
- Add a test asserting that no tick schedules more than 0.6 ms of nominal sub-rate work.

**D9 · C · There is no memory or VRAM budget arithmetic, and the plan likely exceeds its
own limits.**
- Flat shading forces three unique vertices per triangle.
- Buildings: 10k × (≈800 tris + ≤400 yard) ≈ 11M tris ≈ 33M verts × 36 B (pos, normal,
  color, slot, flags) ≈ **1.2 GB** if all LOD0 meshes are resident, doubled if the
  main-world copies are kept.
- Trees "merged per chunk per LOD" (ADR-0003 L204–205): forests at Poisson 4–7 m on 20–35 %
  of 16 km² give 90–150k trees × 120 tris ≈ 11–18M tris ≈ **1.2–1.9 GB**.
- Grass clumps every 2 m near water and meadows give 10⁵–10⁶ instances.
- The limits are ≤ 2.5 GB VRAM on PC and ≤ 3.5 GB total on M4 (PERF L16).
- Fix:
  - **instance** trees, rocks and grass from about 12 variant meshes with an instance
    buffer: 32 B × 300k ≈ 10 MB;
  - make LOD0 buildings resident only near the camera (A56);
  - quantise vertices to 16 B (unorm16 positions per chunk AABB, oct-encoded normals, RGBA8
    color, u16 slot and flags);
  - set `RenderAssetUsages::RENDER_WORLD` for all procgen meshes;
  - add a memory table to PERFORMANCE.md and a memory line to the benchmark report.

**D10 · M · PERFORMANCE L71 understates the props.**
- It says "100k+ trees/props". By the scatter parameters (ALGORITHMS L268–270) the real
  number is 300k–700k instances.
- PropLayer holds these as a `Vec` (DATA_MODEL L67–75), even though §22 says "lazy per
  chunk".
- Decide: lazy per chunk, generated within 1 km of the camera, with exclusion masks
  recomputed when a chunk is generated.

**D11 · M · Entity counts are unbudgeted.**
- Road decorations are "instanced meshes … automatic GPU instancing" (ALGORITHMS L697,
  ADR-0003 L208–210). That is one entity per lamp and per road tree: about 10–50k entities
  for 300 km of road.
- Add roughly 2k road and junction mesh entities, 4k terrain chunk-LOD entities, 3k vehicle
  views, pedestrians and icons.
- Each costs CPU culling, transform work and extraction.
- Set a cap of **≤ 15k renderable entities**. Merge or instance decorations per chunk with
  an instance buffer, and render vehicles as one instanced draw per variant, fed by a
  storage buffer written in ViewSync, not as 3 000 entities.

**D12 · M · Async procgen competes for the same cores.**
- `AsyncComputeTaskPool` gets 1–2 threads.
- Building generation costs 1–3 ms per building, and a level-up wave of 200 buildings is
  0.2–0.6 s of async CPU.
- Chunk re-merge costs 3–8 ms per chunk.
- Terrain re-mesh costs about 1–2 ms per chunk-LOD.
- These run alongside sim parallel sections and the render thread on 6 hardware threads.
- Fix:
  - at most 1 job in flight per async thread;
  - an **upload budget of ≤ 8 MB per frame** on the render thread;
  - prioritise jobs by distance to the camera;
  - count job CPU in the benchmark.

**D13 · M · `GeneratedEnvironmentMapLight` regeneration is a periodic GPU spike.**
- The plan regenerates "when ΔE > 1.5" (ALGORITHMS L712–714).
- At 4× through dawn and dusk this happens every few seconds. Each regeneration is a GPU
  filtering pass of about 0.5–2 ms.
- Throttle to at most 1 regeneration per 2 real seconds, and budget it as a spike.

**D14 · M · Pipelined rendering adds a frame of latency.**
- Input-to-photon latency is about 2–3 frames (35–50 ms).
- "Tool preview update ≤ 1 frame" (PERF L20) should be restated as "≤ 1 frame from input
  to preview mesh in the main world".

### Corrected per-thread budgets (16.6 ms per thread, worst frame, max city, High PC / Medium M4)

The numbers are my engineering estimates; validate them with the C-1 and C-5 spikes and
the benchmark runs on the user's machines. M4 CPU columns assume 4 P-cores plus 6 E-cores,
with intra-system work split into a **fixed 64 partitions** (G-D2).

**Main thread (wall-clock; includes parallel sections inside exclusive sim systems)**

| Work | PC 1× | PC 4× | M4 1× | M4 4× | Notes |
|---|---|---|---|---|---|
| Input, action map, camera | 0.2 | 0.2 | 0.15 | 0.15 | |
| Picking + tool preview (steady) | 0.5 | 0.5 | 0.4 | 0.4 | drag frames: preview work moved to async; ≤ 2.0 on the main thread |
| SimEdit (steady / edit frame) | 0.1 / ≤ 3.0 | 0.1 / ≤ 3.0 | same | same | derived work queued with a per-frame budget; ticks deferred on edit frames (D2) |
| SimTick (ticks this frame × 3.0) | 3.0 (1 tick) | 6.0 (2 ticks) | 3.0 | 6.0 | per-tick table below |
| ViewSync (≤ 3 000 agent poses into an instance buffer, chunk swaps, job polling) | 1.0 | 1.0 | 0.9 | 0.9 | parallel pose computation |
| Presentation (lighting director, weather FX params, overlays, audio director, toasts) | 0.5 | 0.5 | 0.5 | 0.5 | |
| egui (PostUpdate: layout, tessellation, output) | 1.5 | 1.5 | 1.3 | 1.3 | 2.0 with big tables open → virtualise |
| Engine PostUpdate (transforms, visibility, VisibilityRange) | 1.2 | 1.2 | 1.0 | 1.0 | needs ≤ 15k renderables (D11) |
| Extract (sync point) | 1.5 | 1.5 | 1.3 | 1.3 | |
| Schedule overhead | 0.5 | 0.5 | 0.4 | 0.4 | |
| **Sum (steady)** | **10.0** | **13.0** | **9.05** | **12.05** | |
| **Headroom** | 6.6 | **3.6** | 7.55 | 4.55 | edit frames at 4× must defer one tick (D2) |

**Render thread**

| Work | PC | M4 (Metal) | Notes |
|---|---|---|---|
| Prepare (view and light uniforms, per-chunk param buffers, vehicle instance buffer) | 1.5 | 1.6 | |
| Specialise and queue (opaque, prepass, shadow ×3) | 1.5 | 1.6 | cached specialisation, so no per-frame pipeline lookups |
| Sort, batch, GPU-preprocess dispatch | 1.0 | 1.2 | |
| Graph encode (≈ 3k main + 3k prepass + 3×1.5k shadow draws) | 4.0 | 5.0 | indirect draws, which wgpu emulates on Metal; verify on the M4 |
| egui render | 0.4 | 0.5 | |
| Asset uploads (≤ 8 MB/frame) | 1.5 | 1.0 | unified memory makes uploads cheaper on the M4 |
| Submit, present | 0.5 | 0.6 | |
| **Sum** | **10.4** | **11.5** | |
| **Headroom** | 6.2 | 5.1 | |

**GPU (PC GTX 1660S at 1080p = 2.07 MP; M4 base at Auto ≤ 2.2 MP rendered, egui native)**

| Pass | PC High | M4 Medium | Notes |
|---|---|---|---|
| Shadows (PC 3 × 2048²; M4 2 × 2048², far cascade LOD1) | 2.5 | 2.0 | the TBDR depth-only path is efficient on Apple GPUs |
| Depth and normal prepass | 1.5 | 1.7 | |
| GPU culling / preprocessing | 0.3 | 0.3 | |
| Opaque forward | 4.5 | 5.0 | |
| SSAO (PC Medium full-res; M4 half-res) | 1.0 | 0.9 | |
| Water, sky, transparents, precipitation, light pools | 1.0 | 1.2 | additive light-pool overdraw at night needs a cap |
| Bloom, tonemap, SMAA | 0.8 | 1.0 | M4: 4 mips |
| Upscale composite + CAS (only when s < 1) | 0 | 0.3 | |
| egui (M4 at native 4–6 MP) | 0.3 | 0.4 | |
| Environment-map regen (amortised, D13) | 0.1 | 0.1 | |
| **Sum** | **12.0** | **12.9** | |
| **Headroom** | 4.6 | 3.7 | MBA sustained throttling is about 15–25 %, so the M4 headroom is just enough; measure warm (PERF L17) |

**Per-tick SimTick budget (wall, PC 4 workers / M4 4P + 6E): ≤ 3.0 ms per tick**

| System | Budget | Rule |
|---|---|---|
| Clock, weather | 0.02 | |
| Schedule (time wheel) | 0.10 | ≤ 2 000 due activities per tick; overflow carried to the next tick |
| Trips + mode choice + admission | 0.25 | |
| Routing + refresh + reroute | **0.80 hard cap** | cost-unit based (D4) |
| Flow model | 0.10 | incremental per link event |
| Vehicle step: parallel / sequential | 0.30 / 0.40 | 64 fixed partitions |
| Junction control | 0.30 | active junctions only |
| Transit dwell and boarding | 0.10 | |
| Stuck counters + cycle-DFS slice | 0.05 | |
| **One** staggered sub-rate slot (dispatch, utilities, economy, demand, job matching, fields, leveling, happiness, lifecycle) | **0.60 hard cap** | phase table (D8) |
| ApplyDeferred / structural | 0.10 | |
| Stats, notifications | 0.05 | |
| **Total** | **3.17 → trim routing to 0.65 at 4× to reach 3.0** | |

Also add a **hitch budget**: autosave snapshot ≤ 10 ms, edit frames ≤ 20 ms total, no frame
over 33 ms at p99.9 in the 60 s benchmark. Report p50/p95/p99/p99.9 and the maximum per
thread. The current report (PERF L113–116) has no per-thread split.

---
## E. Milestone order — hidden dependencies (order fixed; fixes move or stub enabling work)

| ID | Sev | Where | Hidden dependency | Fix (keep milestone order) |
|---|---|---|---|---|
| E1 | H | CLAUDE L50–56 vs PROGRESS (no phase S), ADR-0037 L43, ADR-0041 L276 | Spike phase "right after M1" has no entries, S1–S5 undefined, "S6" means two things, and ADRs place S6 inside M1 | Add **Phase S** between M1 and M2 to PROGRESS with S-T, S-J, S-P, S-Time, S-G (section C) + S-Shader (WESL→MSL/HLSL validation) + S-RenderScale (MainPassResolutionOverride vs offscreen composite); each with criteria, time-box, outcome row |
| E2 | H | CLAUDE L141–144 (all mutations via commands) vs ADR-0015 milestone "M3" | M2 road/plot tools must already emit `PlayerCommand`s with `validate/apply`; CommandQueue, CommandSeq, CommandResult, ValidationError+l10n keys | Move command framework (without undo) to **M2**; M3 adds history/undo |
| E3 | C | ARCH L473–475, ADR-0015 L143–144 vs PROGRESS M5 L150 | Undo (M3) records removed-object snapshots "using the save snapshot types" — snapshot module is M5 | Create `cb_sim::save::snapshot` types for nodes/segments/plots/lots/buildings/stamps/props-exclusions in **M3** (no file format yet); M5 adds format/migration |
| E4 | H | ALGORITHMS L650–653, PROGRESS M2 L97 "live cost", M3 L122 "refunds" vs M5 economy | Live cost, charging and refunds need `Economy{money}`, `Money`, catalog costs (roads.ron) | Stub `EconomyPlugin` (money + ledger of construction only) and `Catalog` (roads/zones costs) in **M2**; M5 extends |
| E5 | C | PROGRESS M5 L143 "trips (macro only, agents in M9)", ADR-0011 milestone "M5", brief "fully playable with traffic disabled" vs M9 L180 "Routing (ALT A*, cache, variants, batching)" | Macro trips complete "after the BPR travel time" of a **route**; commute estimates, demand commute term, job matching refinement need LinkGraph + router + flow model in M5 | Move LinkGraph (M3), router + cache + batching + flow model (macro) to **M5**; M9 adds agents/IDM/junction control only. Re-word M9 criterion |
| E6 | H | PROGRESS M6 L157 "service vehicles in ghost mode until M9" | Ghost mode = kinematic along a route at flow-model speed (ALGORITHMS L479–481): needs router (E5), `VehicleStore`, and **vehicle rendering** (AgentViewPlugin, instanced vehicle meshes, pose from lane `s`) — all M9 | Put `VehicleStore` + ghost kinematics + vehicle view pool (instanced, from M4 procgen vehicles) into **M6**; or accept invisible service vehicles until M9 and say so in M6 criteria |
| E7 | H | PROGRESS M6 (fires) vs ART_DIRECTION §10 L212–213 | Fire/smoke VFX (`fx_view.rs`) not in any milestone's acceptance | Add to M6 acceptance (burning, smoke column, ruined swap) |
| E8 | H | ARCH L553–556; PROGRESS M15 L229–232 | Settings resource + `settings.ron` persistence used from M1 (log level per ADR-0022 L59–60), M2 (InputMap bindings, graphics preset for perf, render scale ADR-0041), M8 (UI scale, CVD mode), M11 (cycle length, lock time), M12 (weather toggles), M13 (volumes) | Move `settings/{mod,persist,apply}.rs` (load/save/versioning, no UI) to **M1**; minimal dev settings panel M2; full settings UI stays M15 |
| E9 | H | ADR-0024 milestone "M8 (game UI)" vs M2 tool feedback, M3 panels/dialogs | M2 needs validity reason labels + cost label; M3 needs road customization panel (L120–121), intersection tool panel, demolish dependency dialog (L123–124), undo labels; theme/widgets exist only in M8 | Move `ui/theme.rs`, `ui/widgets.rs`, `ui/dialogs.rs`, `tooltips.rs` base to **M2**; M3 builds road panel/intersection panel/dialog on it; M8 = HUD, overlays, info views, polish |
| E10 | H | PROGRESS M7 L163 "locked tools shown with unlock condition", "milestone notification" | Toolbar with flyouts and notification toasts are M8 | Either move minimal toolbar + toast stack to **M7**, or move these two criteria to M8 and make M7 headless-testable (`progression_unlocks_at_thresholds`) |
| E11 | H | PROGRESS M14 L224 "replay from main menu"; GAME_DESIGN L216–217 "Hints toggle in settings" | Main menu and settings UI are M15 | M14: replay via pause-menu entry + `--tutorial` CLI; M15 adds main-menu button; hints toggle in dev settings panel until M15 |
| E12 | M | PROGRESS M14; ADR-0024 | Tutorial highlights "relevant UI element" (GAME_DESIGN L206) need stable UI anchors (egui `Id` → last-frame `Rect` registry) | Add `ui::anchors` registry in **M8** (every toolbar/panel widget registers an anchor key) |
| E13 | M | GAME_DESIGN L208 tutorial step 4 "water pump on the shore" | Needs worldgen guarantee: shore within ~400 m of an outside connection, buildable | Add worldgen constraint in **M2** (reject/re-roll seed sub-stream until satisfied); test `worldgen_outside_connection_near_shore` |
| E14 | H | PROGRESS M4 L136–137 "10k-building synthetic scene"; PERF L120–123; skill milestone-completion §5 (bench from M2) | Needs a city generator (organic roads + plots + forced growth) and a benchmark harness with camera path; generator is mentioned only in PERF §6 | Build `citygen` + `--benchmark` scene-by-command-script in **M2** (500 houses) and scale in **M4**; benchmark *save* only after M5 |
| E15 | M | skill milestone-completion §5 "from M2 on --benchmark <scene>"; PERF L111 "loads the benchmark save" | Save format is M5 | Benchmark scenes defined as seed + command scripts until M5 (same as screenshot suites) |
| E16 | M | PROGRESS M2 L98–100 "generated houses" vs M5 "lot development by demand" | Houses must appear on lots in M2 without demand | M2 sim stub: `ZoningPlugin` "instant growth" rule (each empty lot grows L1–L3 after N ticks) behind `SimConfig::growth_mode = Debug`; replaced in M5 |
| E17 | M | PROGRESS M4 L132–133 level variants/abandoned visuals before M5 logic | Needs `DebugCommand::SetBuildingLevel/Condition` | Add to M4 debug tooling |
| E18 | M | PROGRESS M3 L117–118 lights "auto plan, timing override" | Phase-plan generator is pure (fine), but signal heads showing phases need the M9 controller; timing editor unlocked at tier 4 (M7) | M3: generate plan + store override; signal heads render static red/green per phase via a stub phase clock (pure function of tick); M9 replaces with controller |
| E19 | H | GAME_DESIGN L47 tram track min radius 20 m, M10 trams vs M3 junction generator (connector clamp 5 m, ALGORITHMS L161) | Junction cut-backs/connectors must account for tram turning radius and track geometry; otherwise junction generator is reworked in M10 | Include `LaneKind::Tram` connectors with per-kind min radius and cut-back requirements in **M3** junction generator and spike S-J |
| E20 | M | DATA_MODEL L112 `bus_lanes`, `tram_tracks` in SegmentStyle (M3) vs implementation M10 | Cross-section swapping and lane kinds must exist in M3 for the lane graph | Implement cross-section variants (incl. bus/tram lanes) in M3, hide UI until M10 |
| E21 | M | PROGRESS M3 L111 "road hierarchy" vs progression unlocks M7 | Before M7 everything unlocked implicitly | State explicitly: `Progression` defaults to "all unlocked" until M7 introduces tiers; tests in M7 flip default |
| E22 | H | TESTING §5 test 2 (save/load continuation) from M5 vs agents in M9 | Test passes in M5–M8, breaks in M9 unless agent/junction/cache state is saved | Decide in **M5** how traffic state is saved (G-S1) so M9 doesn't break the format |
| E23 | M | PROGRESS M5 L150 "fixture v1" | Format will change every milestone until M15; per-version fixtures + migrations from M5 is churn | Policy: fixtures regenerated freely until first public release (or M15); version bumps + migrations mandatory only after that; keep the migration framework test with a synthetic v0→v1 |
| E24 | M | PROGRESS M5 L141 "fixed timestep driver with interpolation α" duplicates M1 L75 | Interpolation α only matters for agents (M9) | Move α to M9 or M6 (ghost vehicles) |
| E25 | M | PROGRESS M5 L149 happiness from services (M6), commute (E5), noise from traffic (M9), crime (M6) | Stubbed factors not specified | List stub values per factor in M5 (e.g., services = 0.5 neutral) and a test that switching stubs to real systems doesn't change formulas |
| E26 | M | ALGORITHMS §18 garbage trucks; PROGRESS M5 "analytic until M9" | Fine, but M6 dispatch/ghost vehicles (E6) could also carry garbage trucks; specify which milestone switches garbage to trucks | Switch in M9 with test `garbage_trucks_match_analytic_within_10pct` |
| E27 | M | PROGRESS M8 L171–173 overlays with "CVD-safe palettes" vs M15 accessibility settings | CVD mode switching mechanism + high-contrast theme must exist in M8 | Implement palette sets + switch (dev panel) in **M8**; settings UI in M15 |
| E28 | M | ADR-0041 milestone "M15 (settings UI)", M2 implementation | Render scale changes at runtime require recreating offscreen targets — design in M2 | Put runtime render-scale change + test in M2 |
| E29 | M | PROGRESS M11 L198–199 "configurable cycle length; lock time-of-day option" | Settings UI M15; `SimConfig` change command needed | Add `PlayerCommand::SetSimConfig` (non-undoable) in M11 + dev panel |
| E30 | M | PROGRESS M12 L206–207 "forecast UI; disable/lock options" | Top bar (M8) OK; toggles need E29 | same |
| E31 | M | PROGRESS M13 L218 "channel volumes" | Volume UI = settings M15 | Dev panel in M13; persist via E8 |
| E32 | M | PROGRESS M10 L192 trams "tracks in road (avenue/street option)" | Upgrading existing roads to tram style changes cross-section → lane graph + junction regen (E19) + plots (A53) | Covered if E19/E20/A53 done in M3 |
| E33 | M | PROGRESS M9 L184 "follow-camera on vehicles" vs ALGORITHMS L736–739 (needs vehicle picking via view pool) | Fine; but vehicle info panel placeholder in M8 needs agent IDs | OK; note vehicle info panel real in M9 |
| E34 | H | ADR-0010 agent cap in PERF presets (graphics settings M15) | If agent cap stays a preset lever, M9 determinism tests need settings → see G-C1 | Remove agent cap from graphics presets before M9 |
| E35 | M | PROGRESS M2 L101–103 "time-of-day presets via debug panel" vs lighting director M11 | M2 needs keyframe interpolation code anyway | Implement `LightingState` + keyframes in M2 (static), M11 only animates |
| E36 | M | ART_DIRECTION §7 overlays world desaturation 25 % | All world materials need an `overlay_mix` uniform from M2 (WorldMaterial) or M8 re-touches every shader | Add `overlay_mix`, `night_factor`, `wetness`, `snow` uniforms (no-ops) to WorldMaterial/TerrainMaterial in M2 |
| E37 | M | ADR-0003 per-building param buffer (M4) vs M2 houses | M2 houses rendered how? If per-entity in M2 and merged in M4, picking/highlight/LOD code is written twice | Use chunk merging + slot buffer from M2 (500 houses is enough to exercise it) |
| E38 | M | ADR-0034 picking objects via spatial indices (M2) vs vehicles (M9) and stops (M10) | OK, but elevated-road picking (G-O1) needed in M3 with bridges | Add ray-vs-deck picking to M3 |
| E39 | M | PROGRESS M1 L73–74 "next start shows 'last session crashed' notice" | ADR-0022 L72–74 places it in the main menu (M15) | M1: egui notice window at startup; M15 moves it to menu |
| E40 | M | PROGRESS M1 L77–79 render smoke "exercises every material/shader path" (TESTING L90–92) | Materials appear M2–M12; smoke scene must grow | Add "extend `--smoke-test` scene" as a mandatory step in milestone-completion skill |
| E41 | M | ADR-0019 bank synthesized at Boot (M13) vs UI sounds needed in M8 | Optional; but `AudioPlugin` + channels + bank infra could start in M8 with UI clicks | Leave in M13 but reserve `UiIntent::PlaySound` hooks in M8 |
| E42 | M | GAME_DESIGN §8 sandbox (M7) vs main menu new game (M15) | CLI `--sandbox` exists (README) | OK; state it in M7 criteria |
| E43 | H | Brief: M9 "game fully playable with traffic disabled" — implies M5–M8 must already be playable as a game (commutes, services, congestion proxy) | Without E5 the aggregate commute model doesn't exist until M9 | E5 |
| E44 | M | ADR-0039 gesture/trackpad (M2) can only be validated on the user's M4 | "R19 user test on M4 at M2" blocks M2 | Make it a non-blocking "pending user verification" item (F-0) |

---

## F. Acceptance-criteria quality (M1–M16)

**F-0 · H · cross-cutting process problems**

1. *User-dependent gates.*
   - **Problem:** R9 (M1), R10/R17/R19 (M2) and TESTING L29 (M2, M4, M9, M11, M16) make
     milestones wait on runs from the user, and R11 makes M13 wait on a listening pass.
     That contradicts "implement everything without stopping" (BRIEF L17).
   - **Replacement:** a status ☑ᵤ "done, pending user verification". The agent batches the
     requests into one message per milestone with exact commands and continues.
     Discrepancies the user reports become bugs.
2. *GPU-less numbers.*
   - **Problem:** "60 FPS with 500 houses on the dev machine profile" (L105) and "renders
     within GPU budget" (L137) cannot be measured in the container. CPU numbers under
     lavapipe are polluted, because lavapipe rasterises on the same cores.
   - **Replacement:** a `--benchmark --no-gpu-work` mode (render target 64×64, shadows
     off) that measures main-thread and render-thread CPU budgets in the container, and GPU
     budgets only from the user's runs.
3. *Shot lists missing.*
   - **Problem:** the skill (visual-verification §2) says shot lists and must-be-visible
     items are "copied in docs/PROGRESS.md", but none exist.
   - **Replacement:** the lists below.
4. *Objective visual checks.*
   - **Problem:** an agent reviewing PNGs needs measurable checks for "looks good".
   - **Replacement:**
     - a `--screenshot-metrics` JSON per shot with luminance histogram, clipped %, % pure
       black, and % magenta or NaN pixels;
     - a **semantic ID pass**: the same camera rendered with class colours (terrain, water,
       road, R, C, I, service, vehicle, tree, UI), with per-shot assertions such as
       "vehicle pixels ≥ 1.5 %";
     - pairwise ΔE between class mean colours in the beauty pass ≥ 12, which supports
       ART_DIRECTION pillar 2.

   All shots: 1600×900, UI hidden unless noted, deterministic scenario
   `suite_<M>.ron` = seed + command script + fast-forward tick. "d" = camera distance to
   focus; pitch is from horizontal.

**F-1 · M1**
- "Workspace with all crates compiles" → add `cargo build --workspace --all-targets` on 3 OS
  plus `scripts/check_sim_isolation.sh` (test name `sim_isolation_no_render_deps`).
- "opens a window … clear-colored scene and an FPS readout" → `--smoke-test 60` exits 0 on
  lavapipe and logs `frames=60 validation_errors=0`.
- "Logs written to `<data_dir>/logs/`" contradicts ADR-0040 (logs are in
  `~/.local/state`, `~/Library/Logs` or `%LOCALAPPDATA%`) → test
  `paths_resolve_{windows,linux,macos}` plus an integration test that a log line appears in
  `AppPaths::logs`.
- "ring buffer keeps the last 500 lines" → test `ring_buffer_keeps_last_500`.
- "Headless test runs 1 000 SimTicks" → `driver_runs_1000_ticks_in_lt_100ms`,
  `driver_step_once_advances_exactly_one_tick`, `driver_caps_ticks_per_frame`.
- "Dev tooling … inspector stub" is undefined → "the inspector lists ≥ 1 sim entity kind with
  its Reflect fields; the perf overlay shows frame time p50/p95, sim tick time and entity
  count".
- Add a CI-duration criterion: the full pipeline is ≤ 25 min on all OS. Otherwise the agent
  waits on CI every milestone.
- Add the Phase S definitions (E1) as an M1 deliverable.

**F-2 · M2 (visual gate)**
- "Seeded terrain: hills, valleys, 1–2 rivers, 1–3 lakes" → test
  `worldgen_features_seed_{1..20}` asserts river count ∈ [1, 2], lake count ∈ [1, 3],
  buildable lowland with slope < 8 % covering ≥ 3 km², and generation ≤ 4 s.
- "chunked LOD rendering with skirts" → automated crack check: render the semantic pass at
  LOD boundaries and assert 0 background pixels inside the terrain silhouette.
- "water with depth color, shoreline foam, fresnel" → shots M2-07 and M2-08.
- "Trees … removed under roads/plots … restored" → `scatter_excluded_by_corridor_plus_2m`,
  `scatter_restored_after_stamp_removed`.
- "Camera … smoothing, bounds" → tests `camera_pan_rotation_relative`,
  `camera_zoom_clamped_25_2400`, `camera_q_tap_snaps_to_next_90`,
  `camera_focus_clamped_to_bounds`, `camera_never_below_terrain_plus_6m` (synthetic
  input).
- "Road tool … validity feedback (slope, curvature, overlap)" → each reason has a test
  returning that `ValidationError` key, plus the preview shot M2-12.
- "Residential plot tool … subdivision into irregular lots; houses that fit lots" →
  spike S-P metrics as regression tests: `subdivide_random_10k_invariants`,
  `building_footprint_within_lot_10k`.
- "Lighting & art style in place" → shots M2-09…M2-11 plus the A37 calibration metrics.
- "Screenshot set reviewed against ART_DIRECTION.md" → the shot list below, with per-shot
  verdicts in `docs/screenshots/M2/index.md`, plus metrics JSON.
- "60 FPS with 500 houses on the dev machine profile" → container: main thread ≤ 8 ms p95
  and render thread ≤ 8 ms p95 in `--benchmark m2_slice --no-gpu-work`. User PC and M4: GPU
  ≤ 12 ms p95 at 1080p and Auto (☑ᵤ).
- **Shots (suite M2, seed 1234, scripted: a curved street on an 8 % slope with a fill
  embankment, a straight street in a 3 m cut, a street joining a river shore, residential
  plots on convex, concave and corner frontage, ~60 houses, noon clear unless noted):**

  | ID | Camera | Must be visible |
  |---|---|---|
  | M2-01 | overview d 2 000 m, pitch 60°, yaw 0 | ≥ 1 river, ≥ 1 lake, hills with rock on steep slopes, forest masses, sand banks; no chunk seams; horizon fog without fog over the city |
  | M2-02 | neighbourhood d 300 m, pitch 45°, yaw 0, zoning view on | curved street, all three plots with lot lines, houses facing the frontage |
  | M2-03/04/05 | d 120 m, pitch 35°, yaw 0/120/240, same block | backyards (fence, shed, garden, driveway); houses good from all sides; no house crossing a lot line; varied roofs |
  | M2-06 | fill embankment, d 40 m, pitch 20° | smooth embankment, no terrain through the road, skirts invisible, a house on a plinth or stilts |
  | M2-07 | cut, d 40 m, pitch 20° | 1:1 cut face with dirt/rock material; road edges clean |
  | M2-08 | shore, d 80 m, pitch 30° | depth gradient, foam band, sky fresnel, sand |
  | M2-09/10/11 | M2-02 framing at 08:30, 19:00, 23:00 (debug preset) | soft long morning shadows; warm sunset; blue night with ambient floor; houses still readable |
  | M2-12 | road tool, d 150 m, UI on | invalid ghost (striped, coral) with reason label "too steep" and live cost label |
  | M2-13 | plot tool editing, d 120 m, UI on | vertex handles, depth handle, sub-plot ± control, subdivision lines |
  | M2-14 | LOD band, d 360 m (crossfade zone) | no holes or popping artefacts in the frame |

  Metric gates: 0 magenta/NaN pixels; pure black (< 8/255) ≤ 0.5 % in daylight shots; A37
  calibration.

**F-3 · M3**
- "Road hierarchy … upgrade/downgrade in place" → tests `upgrade_street_to_avenue_rebuilds_lanes`,
  `upgrade_clips_plots_and_prices_demolition` (A53), `downgrade_refund_zero`.
- "Lanes generated … debug overlay shows" → `lanes_count_offsets_per_cross_section` for
  5 types × one-way, plus shot M3-10.
- "Automatic junctions" → the S-J metrics as tests: `junction_random_10k_valid_or_typed_error`,
  `junction_connectors_cover_all_allowed_movements`, `junction_surface_continuous_8pct`.
- "Intersection tool" → `light_plan_no_conflicting_greens_{t,x,y,5way}`,
  `intersection_override_persists_in_snapshot`.
- "Bridges … length-scaled cost" → `bridge_cost_monotonic_in_length`,
  `bridge_piers_outside_corridors`, `bridge_abutment_step_le_2cm`,
  `overpass_does_not_modify_lower_corridor` (G-G4).
- "Segment customization … decorations follow curves/elevation" →
  `decor_spacing_within_5pct_on_curves`, `decor_height_matches_profile_1cm`.
- "Undo/redo … for all road and plot actions" → a generated test per `PlayerCommand`
  variant: `undo_<cmd>_restores_state_hash`, `redo_<cmd>_equals_apply`; depth-limit test;
  `undo_blocked_reports_reason`.
- "Bulldozing … dependency warning dialog" → `demolish_validate_lists_dependents`, plus shot
  M3-12.
- **Shots (noon clear, d in m, pitch 40° unless noted):**

  | ID | Subject | Must be visible |
  |---|---|---|
  | M3-01 | T-junction street×street, d 60 | cut-back corners, fillets, crosswalks, stop lines, lane arrows |
  | M3-02 | X avenue×street with lights, d 80 | median opening, signal heads, turn arrows |
  | M3-03 | Y at 30°, d 60 | no overlapping meshes, acute corner clean |
  | M3-04 | 5-way, d 80 | simple polygon, crosswalks on all arms |
  | M3-05 | junction on 8 % slope, d 40, pitch 15° | no steps between approach and junction surface |
  | M3-06 | river bridge with 3 piers, d 200, pitch 25° | deck, railings, piers in the water, abutments meeting embankments |
  | M3-07 | valley bridge > 25 m piers, d 250 | tapered tall piers, deck profile smooth |
  | M3-08 | overpass over a street, d 80, pitch 20° | lower road unaffected, clearance visible |
  | M3-09 | decorated avenue: tree rows, lamps, grass strip, cobblestone, d 50 | spacing even on a curve; lamps upright on slope |
  | M3-10 | debug overlay lanes/connectors/conflicts on the 5-way, d 60, UI on | all connectors drawn, zones distinct |
  | M3-11 | one-way pair + dirt path, d 120 | arrows; dirt path material |
  | M3-12 | bulldoze dialog, UI on | three choices, dependents highlighted |
  | M3-13/14/15 | M3-02 at yaw 0/120/240 | consistent from all sides |

**F-4 · M4**
- "Commercial and industrial plots/lots/buildings with their generators and palettes" →
  `procgen_{c,i}_levels_within_lot_10k`, `procgen_triangle_budget_per_type` (ART L28
  polycounts as asserts).
- "signature silhouettes" (subjective) → silhouette test: render each service/utility type's
  LOD1 mask from 4 yaws at 128²; mean IoU between any two types ≤ 0.6, and IoU between a
  service type and the zoned buildings of the same footprint ≤ 0.6.
- "free placement with rotation and road-edge snapping, footprint validation" →
  `place_building_snaps_to_road_edge_within_2m`, `place_building_rejects_overlap`.
- "construction animation; run-down/abandoned and ruined visuals" → shots M4-05/06 plus
  `condition_change_does_not_remesh` (a counter assertion).
- "Procgen debug scene" → `--procgen-gallery --smoke-test 120` exits 0, plus shot M4-07.
- "10k-building synthetic scene renders within GPU budget" → container: CPU budgets as in F-2
  with citygen 10k; GPU ☑ᵤ: ≤ 12 ms p95 PC, ≤ 13 ms p95 M4; memory ≤ the D9 table.
- **Shots:**
  - M4-01: R L1–L5 row, d 120, yaw 30°;
  - M4-02: C L1–L3 + I L1–L3 rows, d 150;
  - M4-03: lineup of all services and utilities, d 200;
  - M4-04: same at yaw 180°;
  - M4-05: construction at 50 % (scaffold + reveal);
  - M4-06: active vs declining vs abandoned vs ruined, side by side;
  - M4-07: gallery grid UI on;
  - M4-08: 10k city overview, d 2 000, pitch 60°;
  - M4-09: 10k city mid, d 400;
  - M4-10: angled and L-shaped lots, d 80 (buildings follow lot shape).

**F-5 · M5**
- "Clock/calendar/seasons" → `calendar_day_is_28800_ticks_at_24min`,
  `season_changes_every_4_days`.
- "immigration/emigration … jobs matching … schedules … trips" →
  `growth_scenario_flat_3days`: pop ≥ 400 with 120 R lots and power/water; unemployment ≤ 15 %;
  the morning departures histogram peaks between 05:30 and 07:30.
- "RCI demand with breakdown" → monotonicity tests (TESTING L66) plus
  `demand_breakdown_sums_to_value`.
- "Economy" → `ledger_sum_equals_money_delta`, `bankruptcy_after_7_days_triggers_loan`.
- "Utilities" → `power_brownout_outer_first`, `pump_requires_depth_1_5m_within_15m`,
  `disconnected_component_unserved`.
- "Fields … happiness" → `pollution_decays_0_97_per_hour`,
  `noise_inverse_distance_line_source`, `happiness_factors_weights_sum_100`.
- "Save/load core" → `save_roundtrip_hash_equal`, `save_load_continue_equals_continuous`
  (with the G-S1 state included), `save_corrupt_returns_error`, `migration_v0_to_v1`.
- Perf: `headless_bench --city 50k --no-traffic` gives a tick p95 ≤ 1.0 ms and p99.9 ≤ 2.5 ms
  (RISKS R7).

**F-6 · M6**
- "network-distance coverage, capacity and budgets" → `coverage_river_without_bridge_uncovered`,
  `coverage_capacity_split_proportional` (A7), `coverage_incremental_equals_full`
  (defined with global load recompute).
- "incidents (crime, fire with spread/burning/ruins, sickness)" → `fire_spreads_within_6m_p`,
  `fire_resolved_when_truck_arrives`, `unresolved_fire_ruins_building`,
  `crime_rate_drops_with_coverage`.
- "education progression" → `students_advance_one_level_per_season_with_capacity`.
- "service vehicles in ghost mode" → E6; shot M6-02.
- **Shots:**
  - M6-01: a burning building with a smoke column, d 80, day;
  - M6-02: the same at 23:00 with fire glow and a fire truck (ghost);
  - M6-03: a ruined building;
  - M6-04: the coverage debug overlay of police, d 600.

**F-7 · M7**
- "Population tiers and unlocks" → `progression_unlocks_at_thresholds` (table-driven from
  `progression.ron`), `every_unlockable_in_exactly_one_tier`.
- "locked tools shown" (E10) → move to M8, or provide a toolbar stub plus a UI-on shot.
- "sandbox" → `sandbox_money_unlimited_all_unlocked`.

**F-8 · M8**
- "Top bar … tooltips everywhere, all strings localized" →
  - `ui_no_string_literals` (grep test over `cb_game/src/ui/**` for `"…"` in label calls);
  - A58 structural tooltips;
  - `l10n_keys_complete_en_de`;
  - a German overflow check: render every panel in de at UI scale 1.0 and 1.5, and assert that
    no label rect exceeds its panel (egui layout query in a headless egui context test).
- "Overlays … with legends, CVD-safe palettes and redundant patterns" →
  `palette_cvd_separation`: simulate deuteranopia, protanopia and tritanopia with Machado
  2009 matrices; good-vs-bad ΔE00 ≥ 20, adjacent sequential steps ΔE00 ≥ 5.
- "Problem icons … clustering, fading" → `icons_cluster_when_gt3_in_48px`, plus shot.
- **Shots:**
  - M8-01: full HUD at noon, UI scale 1.0 (en);
  - M8-02: the same in German;
  - M8-03: UI scale 1.5;
  - M8-04: high-contrast theme;
  - M8-05…M8-17: one per overlay (power, water, sewage, garbage, police, fire, health,
    education, land value, pollution, noise, happiness, slope; traffic placeholder), d 900,
    pitch 60°, with legend;
  - M8-18: land value in deuteranopia mode;
  - M8-19: problem-icon cluster;
  - M8-20: budget panel with graphs.

**F-9 · M9**
- "Vehicle agents …" → per-trip-kind spawn counters in `traffic_multi_day` (cars, delivery
  trucks, service, garbage are all > 0).
- "Routing, lane following …, no overlaps" → `idm_no_negative_speed_no_overlap_1e6_steps`,
  `junction_no_conflicting_occupancy`, `lane_plan_feasible_on_short_links`,
  `alt_heuristic_admissible_with_variants` (G-R1).
- "Congestion per segment feeding …" → `congestion_increases_commute_estimate`,
  `dispatch_eta_increases_with_congestion`.
- "traffic-disabled fallback verified" → `same_city_traffic_off_runs_10_days`: population
  within ±15 % of the traffic-on run, and no NaN costs.
- "Gridlock protection" → `gridlock_cycle_detected_and_broken`, `stuck_despawn_logged`.
- "Bench: 5 000 agents within budget; 10-day soak test green" → `headless_bench --city 50k`:
  vehicle step p95 ≤ 0.7 ms, routing ≤ 0.8 ms p99, tick p99 ≤ 3.0 ms (4 workers). Soak
  `traffic_multi_day_large`:
  - 10 game days;
  - despawns < 1/1 000 trips;
  - commute perceived p50 ∈ [12, 30] min;
  - p95 < 60 min;
  - population growth ≥ +20 %;
  - no panic;
  - determinism hash identical for 1 and 4 threads.
- **Shots:**
  - M9-01: signalized avenue junction at the 07:30 peak, d 60 (queues at red, a platoon
    crossing on green);
  - M9-02: an unsignalized T, d 40 (a minor-road car waiting for a gap);
  - M9-03: a lane change on an avenue, d 50;
  - M9-04: the congestion overlay over the city, d 1 200, with chevrons;
  - M9-05: follow-camera on a truck, UI on;
  - M9-06: pedestrians on sidewalks and at a crosswalk, d 25;
  - M9-07: the same junction as M9-01 at 03:00 (sparse traffic).

  Semantic pass: vehicle pixels ≥ 1.5 % in M9-01.

**F-10 · M10**
- "Buses … dwell/boarding, ridership stats, costs/income" → `bus_line_ridership_counts_match_boardings`,
  `line_cost_equals_vehicle_hours_times_rate`.
- "Trams …" → `tram_articulated_sections_stay_on_track_1cm`, `tram_turn_radius_ge_20m`.
- "transit reduces car trips; land value near stops" → A/B scenario
  `transit_line_reduces_car_trips`: car mode share down ≥ 5 percentage points in its
  catchment; LV up within 400 m.
- **Shots:**
  - M10-01: bus at a stop with waiting pedestrians, d 30;
  - M10-02: bus lane, d 50;
  - M10-03: tram on the avenue median turning at a junction, d 60;
  - M10-04: tram depot, d 120;
  - M10-05: line manager UI;
  - M10-06: transit overlay with line colours, d 1 000.

**F-11 · M11**
- "Continuous sun/moon … per keyframes" → `lighting_state_matches_keyframes_at_key_times`
  (ΔE ≤ 1).
- "configurable cycle length; lock time-of-day" → `day_length_12_24_48_preserves_perceived_commute`
  (if B2 is kept), `lock_time_freezes_lighting_only`.
- "Lit windows (deterministic per building)" → `window_lighting_hash_stable_across_runs`; the
  lit fraction at 23:00 is ∈ [0.25, 0.55] for R.
- "night readable" → mean luminance at 23:00 ≥ 0.30× noon, and zone class ΔE ≥ 10 at night
  (semantic pass).
- "Commute peaks and commercial activity visibly follow the clock" → agent-count histogram:
  peak hour ≥ 3× the 02:00 hour; commercial customers between 10:00 and 19:00 ≥ 70 % of daily.
- **Shots:**
  - M11-01…08: the same framing (d 400, pitch 45°) at 05:00, 06:30, 08:30, 12:00, 16:30,
    19:00, 20:00 and 23:00;
  - M11-09: night street close-up, d 30 (window lights, lamp pools, headlights and tail
    lights);
  - M11-10: night tram stop, d 30 (lit interior and stop);
  - M11-11: night overview, d 1 800 (city readable).

**F-12 · M12**
- "each weather; seasons with probabilities; forecast" → `weather_markov_matches_season_table_1000_days`
  (χ² p > 0.01), `forecast_matches_realized_48h`.
- "Visuals" → the shot list.
- "Sim effects" → `rain_reduces_v0_by_15pct`, `snow_upkeep_up_to_60pct`,
  `lightning_ignites_30pct_unless_covered`, `solar_wind_factors`.
- "reduce-flashes" → `lightning_flash_le_120ms_and_capped_when_reduced`.
- **Shots (d 300, pitch 45°):**
  - M12-01…06: clear, cloudy, rain, thunderstorm (frame with lightning), fog and snow, all at
    12:00;
  - M12-07: rain at 23:00;
  - M12-08: snow at 23:00;
  - M12-09…12: spring, summer, autumn, winter at noon clear (foliage, grass tint);
  - M12-13: wet-road close-up, d 25 (puddles and darkening);
  - M12-14: snow close-up, d 40 (roofs, trees, roads partly cleared);
  - M12-15: rain with reduce-effects on.

**F-13 · M13**
- "Synthesized bank" → the A38 audio QA suite (`audio_loudness_targets`, `audio_loops_seamless`,
  `audio_no_clipping`).
- "Ambient director mixes by zoom, time, weather, on-screen content" → a deterministic mixer
  test: given camera states and weather, the expected bed weights fall within ±0.05.
- The listening pass becomes ☑ᵤ.

**F-14 · M14**
- "Guided steps … contextual hints … replay" → `tutorial_scripted_run_completes`: synthetic
  input events drive all chapter-1 steps in a windowless app with the UI context and assert
  each step completes. Plus `hints_shown_once_per_tool`, `reset_hints_works`.
- **Shots:** M14-01…06, one per step highlight (UI on).

**F-15 · M15**
- "Main menu …" → `menu_new_game_with_seed_starts_ingame`,
  `menu_load_restores_hash`.
- "Settings: …" → `settings_roundtrip_all_fields`, `settings_invalid_field_falls_back`,
  `keymap_conflict_detected_with_primary_per_os`.
- "Save/load UI with slots … autosave with rotating slots" → `autosave_rotates_3_slots`,
  `autosave_hitch_le_30ms` (bench).
- "German localization complete" → 0 fallback lookups during a scripted UI walkthrough
  (count Fluent fallbacks) plus the overflow test from F-8.
- **Shots:** M15-01 main menu (en), M15-02 main menu (de), M15-03 settings, M15-04 load
  dialog with metadata.

**F-16 · M16**
- "Performance targets met on reference profile" → defined as `bench_city`, 60 s path, with
  p95 per-thread ≤ the D tables and p99.9 frame ≤ 33 ms on PC and M4 (☑ᵤ), plus container
  CPU numbers.
- "Bug backlog cleared …" → plus a 2-hour scripted soak at 4×: 0 panics, memory growth
  ≤ 5 %.
- "Final art pass via screenshot review" → the union of all suites re-rendered, metrics
  within thresholds, and a diff list against previous milestones.

---
## G. Other problems

### G-D Determinism holes

**G-D1 · C · ALGORITHMS L657–658 (derive_seed)**
- **Plan text:** `mix64(world_seed ⊕ mix64(domain) ⊕ mix64(id) ⊕ mix64(extra))`.
- **Problem:** XOR of independently mixed terms is commutative, so `(id, extra)` and
  `(extra, id)` collide. The building seed (DATA_MODEL L182) gives building 3 at L5 the
  same seed as building 5 at L3. When `id == extra` the two terms cancel, so every
  object with `id == level` shares one seed per domain. Domains collide in the same way
  whenever `mix64(d1) ⊕ mix64(id1) = mix64(d2) ⊕ mix64(id2)`.
- **Fix:** sequential absorption:
  `h = mix64(world ^ K1); h = mix64(h ^ domain_tag); h = mix64(h ^ id); h = mix64(h ^ extra)`
  with distinct odd constants. Add a test asserting no collisions over 10⁶ (id, extra)
  pairs and that swapping arguments changes the seed.

**G-D2 · H · ADR-0037 L48–50; PERFORMANCE L101–102; ARCH L355–357**
- **Plan text:** "intra-system partitions ≥ 4 × core count" alongside "fixed, deterministic
  partitioning".
- **Problem:** any partial float reduction (field updates, coverage sums, ledger, stats)
  then depends on the core count, so the 4-core CI runner, the 6-core PC and the 10-core
  M4 diverge even on one OS and one build.
- **Fix:** a constant `SIM_PARTITIONS = 64`, independent of the hardware. Results are
  reduced in partition order. Add test `results_identical_1_4_8_threads`.

**G-D3 · M · ADR-0037 L58–60**
- **Plan text:** "Rust does not contract a*b+c into FMA … so basic arithmetic stays
  identical".
- **Problem:** glam uses SSE2 on x86-64 and NEON on aarch64 (`Vec3A`/`Vec4`/`Mat4`), and
  their horizontal reductions (dot, length) add in different orders. Results differ in the
  last ulp even without libm.
- **Fix:** accept that saves cannot be bit-identical across architectures (so save the
  base terrain, G-S2), or enable glam's `scalar-math` in pure crates. Then measure the cost.

**G-D4 · H · DATA_MODEL L25, L133, L154, L291–292, L303–305 vs CLAUDE L161–162**
- **Problem:** `HashMap` (std RandomState) is used in `IdMap`, `LaneGraph.by_segment`,
  `Connectivity`, `TransitNetwork`, `Coverage` and `emitter_reach`. Several of these will be
  iterated, for example coverage combination or "for each reached segment". CLAUDE forbids
  RandomState for anything that affects the simulation.
- **Fix:** use dense `Vec` indexed by ID, or `BTreeMap`, or `HashMap<K, V, FixedHasher>`
  (bevy_platform). Add clippy `disallowed-types = ["std::collections::HashMap",
  "std::collections::HashSet"]` in sim crates, with an allow-list for lookup-only maps.

**G-D5 · M · ALGORITHMS L409–411 "background slice" landmark recompute**
- **Problem:** if this runs on the async pool and swaps in when it finishes, routes depend
  on wall-clock time.
- **Fix:** tick-sliced recomputation and a deterministic swap tick (A32).

**G-D6 · M · ARCH L470–472; ADR-0015 L138–140**
- **Problem:** commands are applied in SimEdit once per frame *before* the k ticks, so the
  tick a command lands on depends on frame timing. The design says the stream "can be
  reused" for replay or lockstep, but replay needs `apply_before_tick` recorded and honoured.
  SimEdit's derived work must also be a pure function of (state, commands), never of frames.
- **Fix:** record `apply_tick` with each command. In replay, run SimEdit exactly before that
  tick. Add test `replay_command_log_reproduces_hash`.

**G-D7 · H · TESTING L25, ARCH L353–354 (verified)**
- **Problem:** `shuffle_seed` exists only with `bevy_ecs/debug`. ADR-0001 L111–113 doesn't
  enable it for cb_sim, so the test won't compile. Bevy's own TODO also notes that
  auto-inserted sync points limit the shuffling.
- **Fix:** add `bevy = { features = ["debug"] }` as a cb_sim dev-dependency feature, and
  disable `auto_insert_apply_deferred` in sim schedules, since sets already have explicit
  ApplyDeferred points.

**G-D8 · M · TESTING L84 (determinism test 4)**
- **Problem:** the test varies only `max_visible_vehicles`. It must also vary the window
  size, frame rate (driver tick distribution), graphics preset (G-C1), camera path and
  thread count.

**G-D9 · L · ALGORITHMS L664**
- **Plan text:** `SimRng::for_event(stream, tick_or_day, id)`.
- **Problem:** the same (stream, day, id) used twice in a day for different decisions
  yields correlated draws.
- **Fix:** add a `purpose: u16` salt to every call site, and check uniqueness with a
  debug-only collision registry.

**G-D10 · M · ADR-0017 L37–38 "If lockstep multiplayer is ever added, sim math switches to libm"**
- **Problem:** switching later changes every golden hash and every save's regenerated
  terrain.
- **Fix:** use the `libm` crate from day one in `cb_terrain` (noise, erosion, river
  meander) and in all `cb_sim` transcendental functions. The cost is negligible, and saves
  and terrain become portable.

### G-S Save/load holes

**G-S1 · C · DATA_MODEL L385–390; ARCH L541–548; TESTING L71, L81**
- **Problem:** the save omits state that influences future results:
  - agents (lane, s, v, route cursor, lane plan, held zones);
  - junction controllers (phase, timers, queues, reservations, FCFS order);
  - route-cache contents (stale-while-revalidate means cached routes steer choices);
  - pending routing queue;
  - flow EMAs;
  - admission controller `p` and its integrator;
  - ground and water pollution fields (accumulating, L611–613);
  - `Incidents` (fires burning, crimes open);
  - service vehicles out;
  - transit riders aboard (`TransitRun.passengers`);
  - landmark tables.

  The "save at day 3 → load → equal to continuous" test fails from M9, and fires vanish on
  load.
- **Fix:** save all of these. Agents use about 64 B × 12k ≈ 0.8 MB, and lanes are
  identified by `(SegmentId, dir, index)` with `s/len`. Treat the route cache as
  derived-but-saved, or make routing cache-independent: always compute fresh at the
  epoch boundary, deterministically, and never serve stale entries.

**G-S2 · H · DATA_MODEL L43; ADR-0012 L27; ADR-0017 L34–38; B46**
- **Problem:** base terrain is regenerated from the seed on load. With platform libm (and
  glam SIMD, G-D3), a Windows save opened on the Mac can produce a different river path,
  because steepest-descent tracing with momentum (L232–236) and priority-flood with ties are
  chaotic. Roads would then end up in water, and lots would change slope class.
- **Fix:** one of
  1. save the base heightfield as quantised u16 deltas, about 8 MB raw and 2–4 MB after LZ4
     (worth it); or
  2. make worldgen cross-platform deterministic (libm, scalar glam, fixed partitions) and
     add a CI job comparing `terrain_hash(seed)` across all 3 OS for 5 seeds.

**G-S3 · M · ADR-0020 L158–160**
- **Problem:** "MessagePack via rmp-serde with named fields" requires
  `Serializer::with_struct_map()`. The rmp-serde default writes structs as arrays, and then
  `#[serde(default)]` additive migration silently breaks.
- **Fix:** state it in the ADR and add a test that decodes a struct-map fixture with a
  missing field.

**G-S4 · M · ARCH L541–548**
- **Problem:** the load order "new world gen from seed → apply stamps → spawn entities →
  rebuild derived" lacks validation of loaded invariants: dangling IDs, a segment
  referencing a missing node, lots outside their plot. A crafted or corrupt save could
  panic downstream systems.
- **Fix:** add a `validate_snapshot()` pass returning typed errors, and a fuzz test feeding
  mutated fixtures.

**G-S5 · L**
- **Problem:** the camera pose, active overlay and open line panel are not saved, so a
  reload drops the player at a default view.
- **Fix:** save `PresentationState` in the header or body extension (not sim state).

**G-S6 · M · ADR-0020 L171–175**
- **Problem:** catalog numbers live in embedded RON, so loading an old save into a newer
  build silently rebalances it. That is acceptable, but the save header should record
  `catalog_hash`, and the load UI should warn on a mismatch for bug triage.

**G-S7 · M**
- **Problem:** undo history is not saved (correct per the brief), but the undo-snapshot
  types embed sim components. If a component gains a field, old in-memory undo entries are
  fine, yet *replay logs* that embed snapshots need versioning too.
- **Fix:** version `PlayerCommand` serialization with the save format.

### G-M Model, units and gameplay-logic problems

**G-M1 · C · GAME_DESIGN L22, L28–36, L40–47; ALGORITHMS L468–473, L414–418, L483–486**
- **Problem:** see C-4. Capacities are given per hour without saying whether that is a game
  hour or a sim hour; physical discharge is about 1 800 veh per *sim* hour per lane, which
  is only about 30 per *game* hour. The EMA half-life "10 game minutes" is 10 sim s, which
  is very noisy, and the cost epoch "15 game minutes" is 15 sim s.
- **Fix:** define every traffic rate in veh/sim-h and every EMA or epoch in sim seconds
  (e.g. half-life 120 sim s, epoch 120 sim s). Add a unit test asserting that the BPR
  free-flow capacity equals the IDM-measured saturation flow within 10 %.

**G-M2 · H · GAME_DESIGN L33–36**
- **Plan text:** "perceived_min = sim_seconds × 0.1, independent of the day-length setting".
- **Problem:** scheduling backward in *game* minutes still depends on the day length, so at
  12 min/day departures shift by 2× more game time. Peaks, night traffic and balance then
  change with a setting the player expects to be cosmetic.
- **Fix:** C-4, or remove 12/48 until it is validated.

**G-M3 · H · ADR-0027 L62–63; TESTING L65**
- **Problem:** "incremental == full" cannot hold once capacity utilisation couples
  emitters, because one emitter's change alters others' effective coverage.
- **Fix:** split the computation (A7): reach geometry is incremental per emitter; loads and
  utilisation are recomputed globally each game hour. Test incremental reach == full reach,
  and load computation is the same function in both paths.

**G-M4 · H · ALGORITHMS L611–613, L622; ADR-0028 L91–92**
- **Problem:** a diffusion blur applied in 64-row slices to the live field creates seams and
  order artefacts, because some rows are already updated when their neighbours read them.
- **Fix:** double-buffer (read A, write B, swap after the 8th slice), and make the per-hour
  update an explicit slice schedule (D8). Noise is "recomputed each game hour" (L614–616),
  which contradicts "fields updated in slices", so slice it too.

**G-M5 · M · DATA_MODEL L234–239; ADR-0016 L187–188**
- **Problem:** the "32 bytes target" doesn't add up:
  - household 4, age 1, edu 1;
  - `Option<BuildingId>` workplace and school at 8 each without a niche;
  - shift 4;
  - `CitizenState::Travelling(TripId)` with a generational TripId at 12;
  - health 1, commute 2.

  That is 41 bytes, padded to 44.
- **Fix:** use NonZeroU32 IDs, merge workplace and school into one `Option<Place>`, and use
  a 32-bit TripId. Add `const _: () = assert!(size_of::<Citizen>() <= 32);`.

**G-M6 · M · ALGORITHMS L499–502**
- **Problem:** connectivity "ignoring direction; one-way reachability issues are surfaced by
  routing failures". Failing routes are not cached, so every failed trip retries each tick
  and eats the 300-search budget. Buildings reachable only one way get immigrants who can
  never commute.
- **Fix:** compute strongly connected components on the directed LinkGraph (Tarjan,
  O(V+E)), flag buildings outside the outside-connection SCC as "one-way trap", and
  negatively cache failures per (origin, dest, graph_version).

**G-M7 · M · ALGORITHMS L580–582; GAME_DESIGN L69**
- **Problem:** fire spreads within 6 m, while R side setbacks are 1.5 m, so neighbouring
  houses are 3 m apart. A single fire burns a whole street block before a truck arrives:
  1.2 km of reach is about 100 sim s, which is 100 game minutes.
- **Fix:** time fire growth in sim seconds consistent with response times, and add a test
  `fire_contained_with_station_within_reach`.

**G-M8 · M · ALGORITHMS L462–473; ADR-0010 L148–150**
- **Problem:** macro background traffic slows agents only by scaling v0 and "a reduced
  junction release rate". It never creates queues or spillback, so jams at p ≈ 0.25 are
  visually absent and gridlock dynamics differ from the macro estimate. The plan calls this
  "slightly invisible", but at 50k pop the peak is 75 % invisible.
- **Fix:** C-4 plus the C-1 fallback 2, or render "ghost density" (instanced dummy cars
  spaced by background density) near the camera so visuals match the load.

**G-M9 · M · ALGORITHMS L531–535**
- **Problem:** job matching uses "coarse straight distance × detour factor". On a map with
  rivers and few bridges this underestimates badly, so workers get matched across a river
  with a 7-game-hour commute.
- **Fix:** use the per-component bounded Dijkstra distance from the workplace's access point
  (cached per workplace segment), or reject matches whose routed trip exceeds 300 sim s.

**G-M10 · L · ALGORITHMS L614–616**
- **Plan text:** "inverse-square in dB".
- **Problem:** line sources (roads) attenuate by about 3 dB per doubling of distance and
  point sources by about 6 dB. Pick one formula: `L(d) = L0 − 10·log10(d/d0)` for lines and
  `− 20·log10` for points.

**G-M11 · M · GAME_DESIGN L52, L189; ALGORITHMS L644**
- **Problem:** "snow factor" upkeep scales up to 60 %, but snow clearing is not a modelled
  service. That is fine, but the roads are "partly cleared" visually (A42), which reads as if
  plows exist. Say so in the tooltip.

**G-M12 · M · ALGORITHMS L521–523**
- **Plan text:** lot development "picks empty lots … deterministic order by (weight desc,
  LotId)".
- **Problem:** with a fixed weight order the highest-LV area always grows first, so cities
  grow in one blob. The order also needs a random tiebreak to look organic.
- **Fix:** weighted sampling with `SimRng::for_event(Growth, tick, k)` (deterministic).

### G-T Traffic and junction control errors

**G-T1 · C · ALGORITHMS L186–199; ADR-0009 L80–83**
- **Problem:** the grant rule checks `exit_space(target lane) ≥ v.length + min_gap`, but
  nothing reserves that space. Two approaches can each be granted into the same target lane
  in one tick (processed sequentially, yet the check reads the same free space), and
  platoon followers "share their leader's zones" without their own exit check. Vehicles
  then stop inside conflict zones, holding locks, and other approaches wait on those locks.
  That is intra-junction deadlock, so "impossible by construction" is false.
- **Fix:** keep a per-target-lane `reserved_m` that each grant increments by length + gap.
  It is released once the vehicle's rear has travelled `length + gap` onto the target lane.
  Followers need their own reservation. Add property test
  `no_vehicle_stops_inside_conflict_zone`.

**G-T2 · H · ALGORITHMS L207–209**
- **Problem:** right-before-left among equals deadlocks whenever every approach has a waiting
  vehicle: each one yields to its right. The 12 s anti-starvation then lifts *all* of them to
  top priority at once, which needs a tiebreak.
- **Fix:** if all waiting approaches would yield, grant the earliest arrival (arrival tick,
  then VehicleId) immediately. Anti-starvation uses its own key (wait time desc). Add test
  `equal_priority_4way_all_occupied_no_idle_gt_2s`.

**G-T3 · M · ALGORITHMS L203–206**
- **Problem:** the minor-approach gap check needs the ETA of major vehicles up to
  4.0 s × 22 m/s ≈ 90 m upstream, which crosses previous links and junctions when segments
  are short. The look-ahead traversal and its cost are not specified.
- **Fix:** a per-approach "upstream horizon" list built when the lane graph is patched
  (lanes and connectors within 100 m), scanned in O(vehicles on horizon).

**G-T4 · M · ADR-0009 L91–92**
- **Plan text:** "a ring of one-way roads works with the same rules".
- **Problem:** a small ring fails on several counts:
  - min radius 12 m, min segment 6 m, cut-back cap 45 % and min junction angle 25° make
    radii under 25 m invalid;
  - entry nodes classify as "street × street" with the ring as the straight pair;
  - every entry has 3 approaches;
  - exit-space blocking around a full ring is the classic gridlock (only the 90 s despawn
    resolves it).
- **Fix:** add a roundabout controller (yield-on-entry, entry metering when the ring
  occupancy exceeds 80 %), or state in the plan that roundabouts are unsupported and give
  validation guidance.

**G-T5 · M · ALGORITHMS L452–456**
- **Problem:** mandatory lane changes happen "within the last 150 m"; with links of 6–30 m,
  see A30. Lane changes are never allowed on connectors, and a junction 20 m downstream can
  require a lane change across 2 lanes in 20 m, which is infeasible.
- **Fix:** lane plans plus a routing penalty (A30).

**G-T6 · M · ALGORITHMS L210–211**
- **Problem:** the all-way stop "FCFS queue keyed by stop completion tick" also needs the
  conflict-zone rule. Two non-conflicting movements (opposing straights) should go together,
  otherwise throughput halves.
- **Fix:** dequeue greedily: the first vehicle, then any later vehicle whose zones don't
  conflict, within the same tick.

**G-T7 · L · ALGORITHMS L219–220**
- **Plan text:** "yellow → stop if able (`d_stop < distance`)".
- **Problem:** IDM braking `b` = 2.5 m/s² may require more distance than is left.
- **Fix:** use the comfortable deceleration `b_max` = 4 m/s² for the decision, and never
  grant when the vehicle can't clear the junction before all-red ends.

**G-T8 · L · ALGORITHMS L444–445**
- **Plan text:** "uphill −10 %/5 % grade for trucks".
- **Problem:** the expression is ambiguous: is it −10 % speed per 5 % grade?
- **Fix:** write `v0_truck = v0 × (1 − 0.02 × grade_percent)` for grades > 0.

### G-G Geometry and terrain-interface errors

**G-G1 · C · DATA_MODEL L93; ALGORITHMS L156**
- **Problem:** `JunctionGeometry.surface_h: f32` is one height for the whole junction
  polygon, and the "approach profiles end at node height". The approach mesh, however,
  ends at the cut line, which is `cut_back` metres from the node. On a 10 % grade with a
  9 m cut-back the cut line sits 0.9 m below or above `surface_h`, a visible step or gap.
- **Fix:** either constrain each approach's profile to be flat (grade 0) over
  `[0, cut_back + 4 m]` from the node (simple, restricts junctions on slopes), or fit a
  plane or bilinear surface through the approach cut-line heights and triangulate the
  junction polygon with per-vertex heights. Test: step ≤ 2 cm.

**G-G2 · C · ALGORITHMS L251–255; ADR-0012 L28–29**
- **Plan text:** "pick the highest (priority, weight)".
- **Problem:** where two stamps overlap, the winning stamp flips abruptly where the weights
  cross, so the height jumps from road A's blend to road B's blend. This creates cliffs
  between parallel or neighbouring embankments and pads.
- **Fix:** composite fill stamps with `max` and cut stamps with `min` over the blended
  targets, which is order-independent and C0-continuous. Use priority only to let road
  corridors override pads within the carriageway mask. Test: no adjacent-sample jump above
  the slope bound outside the corridors (C-5).

**G-G3 · H · ALGORITHMS L87–89, L95–97; ADR-0012 L38–39**
- **Problem:** the corridor sets terrain "exactly" to profile height within half-width +
  1.5 m, but the samples are 2 m apart (2.83 m diagonal). The interpolated terrain between
  the last flat sample and the first cut sample (1:1 cut, up to 2 m higher) rises *inside*
  the road edge, so terrain shows through the sidewalk in cuts. LOD1+ (4/8/16 m) averages
  even more. The R5 check samples only lane centrelines at LOD0.
- **Fix:** lower the terrain under the carriageway and shoulder by 0.15 m. Make the flat
  margin ≥ 1 cell diagonal plus 0.5 m (3.5 m). Downsample corridor cells with `min` for
  LOD1+. Extend the R5 check to edge samples and all LODs.

**G-G4 · H · ALGORITHMS L50–55, L101–104**
- **Problem:** a crossing with Δh ≥ 5.5 m is "grade-separated", but bridge spans are only
  created over water, valleys or fill-into-water. The upper road's corridor *fill stamp*
  therefore buries the lower road. Corridor-vs-corridor priority ties resolve by stamp ID,
  not by "a road always beats an embankment".
- **Fix:** add a classification rule: any span where the corridor polygon overlaps another
  corridor with Δh ≥ 4 m becomes a bridge span (with min span = width + 2 × 6 m). Test
  `overpass_does_not_modify_lower_corridor`.

**G-G5 · M · ALGORITHMS L52–55**
- **Problem:** in the crossing band 2.5 m ≤ Δh < 5.5 m the move is "invalid", but the
  player can't see Δh while drawing.
- **Fix:** show the computed Δh in the preview label, and offer "auto-junction" (flatten
  both) or "auto-overpass" (raise the new road) as quick fixes.

**G-G6 · M · ALGORITHMS L149–151; L59–60**
- **Problem:** the cut-back cap is 45 % of segment length and the min segment is 6 m. Two
  avenue junctions need ≥ 2 × (9 m + half widest crossing 11 m) = 40 m, so most avenue grids
  below 45 m spacing are rejected.
- **Fix:** document the minimum spacing per type pair in GAME_DESIGN, and show it as a
  snapping guide.

**G-G7 · M · ALGORITHMS L141–142 vs L156–157**
- **Problem:** crosswalks "set back 1 m" at each cut line, and stop lines at the same place.
  Stop lines must sit behind crosswalks, about 2 m upstream. Also, lots and plots in the new
  junction polygon are unhandled (A52).

**G-G8 · M · ALGORITHMS L280–281**
- **Problem:** the plot frontage drag "may continue around a junction corner onto the next
  segment". The junction polygon and fillet sit between those frontages, so the plot
  polygon must follow the fillet curve.
- **Fix:** frontage = sidewalk outer edge **including** the fillet arc. Then FrontageRef
  needs a `(node, corner)` variant, which `FrontageRef` (DATA_MODEL L166) doesn't have.

**G-G9 · L · ALGORITHMS L38–41**
- **Problem:** freehand pieces shorter than 8 m are merged, and the auto-split rule is
  ≤ 120 m or ≤ 90°. After auto-split, pieces can fall below the 6 m minimum near merge
  points.
- **Fix:** split evenly to `ceil(len/120)` pieces.

**G-G10 · M · ALGORITHMS L339–347; ADR-0029**
- **Problem:** R L4–L5 and C use "footprint = inset polygon simplified to ≤ 12 vertices".
  Concave lot polygons (inner curves) give concave footprints, and the plan has no
  façade-window logic for very short edges (< 2.4 m bay) or reflex corners.
- **Fix:** façade edges shorter than 2.4 m become blank walls. Reflex corners get a column
  module. Test: no window quad crosses an edge end.

### G-R Routing problems

**G-R1 · H · ALGORITHMS L405–407, L409–411; ADR-0011 L178–179**
- **Problem:** the variants are "±10 % per link", so costs can drop to 0.9 × free-flow.
  A free-flow ALT bound then overestimates and the heuristic is inadmissible, giving
  suboptimal routes. The test "ALT admissibility" (TESTING L59) will fail.
- **Fix:** compute landmarks on `0.9 × t_free`, or make variants only increase cost
  (`+0…+20 %`).

**G-R2 · H · ALGORITHMS L406–407; ADR-0011 L180–181, L194–195**
- **Problem:** `variant = hash(trip) % 4`. Trip IDs change daily, so a stable commuter
  cycles through 4 keys and the working set is 4× larger: about 50k stable OD pairs × 4 plus
  shopping, against 32k LRU entries. The "> 90 % hit rate" claim (PERF L46) is
  unsupported; I'd expect 30–60 %.
- **Fix:** `variant = hash(citizen_or_household, purpose) % 4`. Size the cache from the
  spike (C-1), and report hit rate in the bench.

**G-R3 · M · ALGORITHMS L433–438**
- **Problem:** transit results are "cached per (origin stop set hash, dest stop set hash)".
  Stop sets per building change whenever a stop is added, so the whole cache invalidates on
  every stop edit, which is fine, but the stop-set hash must be order-independent (sorted).
  Walk times are part of the cost, yet they are not in the key.

**G-R4 · M · ADR-0035 L112–114**
- **Problem:** line legs are routed "on the mode's link graph … cached per line". When a
  congestion epoch changes costs, do buses re-route? If yes, the line path changes and stop
  ETAs shift. If no, say so.
- **Fix:** freeze line paths at line edit and re-route only on topology change.

### G-C Contradictions between documents

**G-C1 · H · ADR-0010 L127–128 vs PERFORMANCE L85–94**
- **Problem:** `agent_cap` is "a saved sim setting" in ADR-0010 but a graphics-preset lever
  in the performance table (Low 2 000 … Ultra 12 000). Either the graphics preset changes
  the simulation (determinism, balance), or the preset value is ignored for loaded cities.
- **Fix:** move `agent_cap` to `SimConfig` (new game option, default 5 000). Only
  `max_visible_vehicles` belongs to graphics.

**G-C2 · M · ADR-0006 L144–145; ADR-0018 L80–81 vs BRIEF L42**
- **Problem:** "lock time of day" is visual-only, so lit windows (driven by `night_factor`
  from `LightingState`) disagree with sim activity, such as noon visuals with night-shift
  traffic. Confirm with the user (B3).

**G-C3 · M · ADR-0035 L125 vs ALGORITHMS L431, L436**
- **Problem:** stop access ≤ 600 m vs pedestrian Dijkstra 800 m vs walk mode < 800 m vs
  transfer ≤ 300 m vs LV ≤ 400 m. Put all walk thresholds in one table in GAME_DESIGN.

**G-C4 · M · PROGRESS M15 L234 / ADR-0021 milestone "M15 (complete German)" vs CLAUDE L198–199**
- **Problem:** "A test fails if a key is missing in any locale" is enforced from M1, so
  German is always complete by construction.
- **Fix:** either allow `de` to contain English placeholders marked `# TODO-de`, with a test
  counting them that must reach 0 at M15, or drop "complete German" from M15.

**G-C5 · M · TESTING L93–94 vs CLAUDE L45–48 and skill visual-verification §5**
- **Problem:** screenshots go to `target/screenshots/` in one place and to
  `docs/screenshots/<M>/` (committed) in the others.
- **Problem:** TESTING L94 lists visual milestones M2, M3, M4, M8, M11, M12, M16, which
  misses M6 (fires), M9 (traffic), M10 (transit) and M14 (tutorial UI).

**G-C6 · M · ART_DIRECTION L5–6 / skill §2 vs ART L42–52**
- **Problem:** the screenshot times 07:00/12:00/18:30/23:00 fall between keyframes
  (08:30, 19:00), and the skill says "fixed exposure" (L116) while the director drives
  exposure offsets of up to +2.6 EV.
- **Fix:** use the director's exposure and keyframe times.

**G-C7 · M · ART_DIRECTION L44, L54**
- **Problem:** at night `amb_rel 0.32` with +2.6 EV exposure gives an effective ambient of
  0.32 × 2^2.6 ≈ 1.94, which is 2.4× brighter than noon ambient (0.80). Emissive windows
  then wouldn't pop.
- **Fix:** define whether `*_rel` is pre- or post-exposure. If pre-exposure, reduce the
  night EV to about +1.0 to +1.3 or the night `amb_rel` to about 0.12.

**G-C8 · L · ARCHITECTURE §5.2 L366–367 vs ADR-0024 L162–163 vs bevy_egui (G-W1)**
- **Problem:** the UI is placed in three incompatible ways across these sources.

**G-C9 · L · PROGRESS L62 (M0 "ADRs 0001–0036") vs 41 ADRs**
- **Problem:** stale. RISKS has R16 listed after R20 (L29) and no R16 in sequence.

**G-C10 · M · PROGRESS M1 L70 "`<data_dir>/logs/`" vs ADR-0040**
- **Problem:** stale per-OS location text.

**G-C11 · L · ALGORITHMS §26 L732 (rotate: MMB) vs README L47 / ADR-0039 L169**
- **Problem:** README and ADR-0039 also rotate with Alt+left-drag and right-drag.

**G-C12 · M · CLAUDE L52 ("phase S, right after M1") vs ADR-0037 L43 / ADR-0041 L276 ("S6 in M1")**
- **Problem:** "S6" names two different spikes (G-P1).

**G-C13 · L · ARCH L290–301**
- **Plan text:** "Render world (frame N−1) extract/prepare/render".
- **Problem:** extract for frame N happens on the main thread at the end of frame N (D3).

**G-C14 · L · ADR-0001 L124 ("real-GPU check on Windows in M2") vs RISKS R9 L18 ("M1 … on a real GPU")**

### G-W Technically wrong or dubious claims

**G-W1 · H · ARCH L296, L367; ADR-0024 L162–163 (verified)**
- **Problem:** `EguiPrimaryContextPass` runs in `PostUpdate` (bevy_egui 0.43.1 lib.rs
  L1144–1149). UI systems therefore run *after* `GameSet::SimDrive`, so commands issued
  from the UI apply next frame. `wants_pointer_input` read in `GameSet::Input` is last
  frame's value.
- **Fix:** document the 1-frame UI latency, and gate world input on the previous frame's
  `EguiWantsInput`, which bevy_egui writes in PostUpdate.

**G-W2 · H · ART_DIRECTION L192–195; ADR-0039 L157–158 (verified)**
- **Problem:** egui 0.36 default fonts don't contain ⌥ ⌃ ⌫. ⇧ is only in Hack (monospace)
  and ⌘ only in emoji-icon-font, so the platform glyph formatter renders tofu, and so do
  egui's own shortcut labels in menus.
- **Fix:** render modifier glyphs as procedural SDF icons inline (the icon atlas already
  exists), or ask the user to approve an OFL font with these glyphs (RISKS Q6).

**G-W3 · M · ADR-0041 L287–289 (verified)**
- **Problem:** "frame cap 30/60/120/unlimited" has no mechanism. Bevy has no limiter, and
  bevy_framepace 0.22 targets Bevy 0.19.
- **Fix:** an own limiter in `cb_game::platform::window`: spin-sleep in `Last` against a
  target frame time, with `PresentMode::AutoNoVsync` when capping below the refresh rate.
  On macOS ProMotion, vsync runs at 120 Hz, so a 60 cap needs sleeping. Test the frame-time
  variance.

**G-W4 · M · ADR-0039 L177 "`PanGesture` (macOS) → pan"**
- **Problem:** winit documents PanGesture as iOS-only. On macOS, two-finger scroll is pixel
  `MouseWheel` (already handled). This is harmless but misleading; drop it or verify on the
  M4.

**G-W5 · M · ADR-0039 L166**
- **Plan text:** "⌘Q (intercepted → save prompt)".
- **Problem:** winit's default macOS app menu handles ⌘Q via `terminate:`. Whether Bevy
  0.20/winit delivers a cancellable `WindowCloseRequested` instead is unverified.
- **Fix:** add to the macOS verification list for the user (☑ᵤ). Fallback: autosave on
  `AppExit`/`applicationWillTerminate`.

**G-W6 · M · ADR-0001 L107–110; ADR-0023 L107 (verified)**
- **Problem:** bevy_egui's default features enable `bevy_ui`, `picking`, `open_url`
  (`webbrowser`) and `manage_clipboard` (`arboard`, `smithay-clipboard`), which pulls in
  crates the plan says are disabled and adds unlisted dependencies.
- **Fix:** `bevy_egui = { default-features = false, features = ["render",
  "default_fonts", "manage_clipboard"] }` (+ `accesskit` if claimed), and list
  `arboard`/`smithay-clipboard` in ADR-0023.

**G-W7 · M · ADR-0041 L274–277 vs ADR-0001 L110 (sprites, bevy_ui disabled)**
- **Problem:** "a composite camera draws it full-screen" needs a fullscreen pass. With
  sprites and bevy_ui off there is no built-in way.
- **Fix:** a small custom render-graph node or `Mesh3d` quad with an unlit material on an
  orthographic camera, plus a test of picking through it (already planned).

**G-W8 · M · ART_DIRECTION L74–76**
- **Plan text:** "fog start ≥ 120 m from camera focus".
- **Problem:** `DistanceFog` is camera-distance based. At zoom 2 400 m, any fog strong
  enough to show "silhouettes fade by 250–400 m" (L71) would erase the city.
- **Fix:** custom fog in the World and Terrain materials based on distance to focus
  (`max(0, |p − focus| − r0)`), with DistanceFog only for the sky.

**G-W9 · M · ADR-0004 L46–47**
- **Plan text:** "shoreline foam from depth prepass difference" in a transparent water pass.
- **Problem:** this needs to sample the prepass depth while the main pass uses the depth
  attachment. Verify the Bevy 0.20 binding for prepass depth in transparent materials
  (read-only depth attachment). Fallback: compute water depth from the terrain heightfield
  texture (`water_level − terrain_h`), which is deterministic and cheaper, and is what the
  plan already has as data.

**G-W10 · M · ADR-0005 L88–93; CLAUDE L141–144**
- **Plan text:** "enforced by the compiler".
- **Problem:** cb_game can still `commands.entity(sim_entity).despawn()`, remove components,
  `Query<&mut SimComp>` and replace whole values (`*c = c.clone()`), or `ResMut<SimRes>`.
- **Fix:** a CI test grepping cb_game for `Query<.*&mut (RoadSegment|…)>`,
  `ResMut<(Terrain|Population|…)>` and `.despawn(` on sim-marked entities, or make sim
  components non-`Clone` and keep the sim schedules exclusive.

**G-W11 · H · ADR-0038 L97; CLAUDE L90**
- **Problem:** `cargo clippy --workspace --all-targets --all-features` enables `trace_tracy`
  (tracy-client-sys compiles C++, against the ADR-0023 policy and slow on 3 OS), `dev`,
  `remote`, and later `inspector`. RISKS R16 L29 mentions a local `dynamic_linking` feature;
  if that is a cargo feature, CI turns it on too.
- **Fix:** explicit feature matrices: `--features dev`, then a default build;
  `trace_tracy` only in nightly.

**G-W12 · L · skill visual-verification L140–141**
- **Plan text:** "lavapipe renders … SSAO identical".
- **Problem:** this is unproven. Precision and driver differences in derivatives and
  compute shaders are common.
- **Fix:** compare lavapipe and GPU shots once at M2 (☑ᵤ) with metrics (F-0), and record
  the delta.

**G-W13 · L · ADR-0037 L39–40**
- **Plan text:** "No push constants".
- **Problem:** Bevy's own GPU culling path requires `Features::IMMEDIATES` (formerly push
  constants; verified in gpu_preprocessing.rs). The rule should read "our shaders use no
  push constants".

**G-W14 · M · ALGORITHMS L686–687**
- **Problem:** `VisibilityRange` "dithered crossfade" renders both LODs during the band
  (double cost in prepass, shadow and opaque), and the dithering interacts with SMAA, which
  causes shimmer.
- **Fix:** keep bands short (≤ 10 % of the range), and use hard switches for shadow casters.

**G-W15 · M · PERFORMANCE L63–64**
- **Plan text:** "batched … multi-draw indirect".
- **Problem:** on Metal, wgpu emulates multi-draw (no `MULTI_DRAW_INDIRECT_COUNT`), so the
  draw count still costs render-thread time.
- **Fix:** budget render-thread encoding per draw (D tables) and measure on the M4.

### G-I Input and UX conflicts

**G-I1 · H · ALGORITHMS L47**
- **Plan text:** "length snap (8 m steps) with Ctrl".
- **Problem:** this hard-codes Ctrl (CLAUDE L124–128), and on macOS Ctrl+click is a
  right-click, which cancels the tool (ADR-0039 L180–181). Shift is used for angle snap.
- **Fix:** bind length snap to a remappable `Action::SnapLength` defaulting to Alt/⌥.

**G-I2 · H · ALGORITHMS L291–292 vs ADR-0039 L169**
- **Problem:** Alt-held auto-clip in the plot tool conflicts with Alt+left-drag rotate.
- **Fix:** make auto-clip a toggle chip or the default behaviour with a confirmation, not a
  held modifier.

**G-I3 · H · ADR-0039 L167; ALGORITHMS L277–284, L36–37**
- **Problem:** left-drag on "empty ground" pans, but frontage drag, freehand drawing and
  vertex or edge drag are all left-drags on ground.
- **Fix:** an explicit precedence rule. If a tool is active and the press hits a tool
  target (road edge, handle) or the tool is in drawing mode, the tool owns the drag. Pan
  only with no tool, or with Space+drag. Add test `input_tool_drag_precedence`.

**G-I4 · M · ADR-0039 L171; README L54**
- **Problem:** the elevation keys `]`/`[` are AltGr+8/9 on German layouts, and as
  `Physical` they map to the `+`/`ü` keys, which mismatches the shown labels. Macs have no
  PgUp/PgDn.
- **Fix:** default elevation to `Alt/⌥ + wheel` plus `Physical(Comma/Period)`, and render the
  key label from the active layout (winit logical key) so German users see the right caps.

**G-I5 · L · ALGORITHMS L732–733 vs BRIEF L44**
- **Problem:** Q/E have no continuous rotation (B42).
- **Fix:** hold for continuous rotation at a configurable deg/s, and tap (< 200 ms) to snap.

**G-I6 · M · README L56 / ADR-0039 L172**
- **Problem:** "Delete/⌫ bulldoze selection". ⌫ (Backspace) is also egui text editing; it
  is suppressed only when egui wants keyboard input (ADR-0039 L184–185). Inspector search
  boxes are a risk; include them in the suppression test.

### G-B Brief coverage gaps

**G-B1 · H · BRIEF L45 "edit/remove with intersections updating"**
- **Problem:** there is no `MoveNode`, `EditSegmentCurve` or `SplitSegment` player command
  (ARCH L452–459), and no tool mode for reshaping.
- **Fix:** add `EditRoadGeometry { segment/node, new control points/position }`, sharing
  validation and undo with BuildRoad, plus the junction, plot and stop re-pointing rules.

**G-B2 · H · BRIEF L45 "player stop signs or traffic lights"**
- **Problem:** only `AllWayStop` exists. DATA_MODEL L88 mentions "Yield overrides", but no
  algorithm uses it.
- **Fix:** per-approach `ApproachControl { Priority, Yield, Stop }`, with the major/minor
  override set in the intersection tool and covered by the same admission rule.

**G-B3 · C · BRIEF L49 "trips from citizen sim and goods economy"; "delivery trucks industry↔commercial"**
- **Problem:** there is no goods model (A14).

**G-B4 · H · BRIEF L45 "upgrade/downgrade"**
- **Problem:** there is no widening rule (A53).

**G-B5 · H · BRIEF L55 "Undo … refunds"**
- **Problem:** a 100 % refund at any age (ADR-0015 L157) versus 0 % demolish after one day
  (GAME_DESIGN L155) is an exploit: build, use it, undo for full money as long as the
  command is within the last 100. Undoing a plot after growth also removes buildings and
  households without rules.
- **Fix:** undo is allowed for 10 real minutes or 1 game day, whichever is shorter; the
  refund decays like demolish after that; households evicted by undo go to `homeless_queue`.
  Add test `undo_after_window_rejected`.

**G-B6 · M · BRIEF L53 tool bar "decorations"**
- **Problem:** decorations exist only as segment styles. There is no free-placed decoration
  (tree, bench, fountain) and no command.
- **Fix:** add `PlaceProp` (PropKind, pos, yaw) to the commands, with an LV effect, or state
  that "decorations" means segment styles plus parks.

**G-B7 · M · BRIEF L49 "pedestrians simple agents to stops"**
- **Problem:** the plan has visual-only pedestrians near the camera (B10). There is no rule
  linking them to actual transit boardings at a stop.
- **Fix:** spawn visual walkers from the `WaitingGroup` delta at stops near the camera.

**G-B8 · L · BRIEF L47 "unique and deterministic from world seed + plot/lot ID"**
- **Problem:** the plan seeds by `BuildingId` (B34). Document the deviation in ADR-0029.

**G-B9 · M · BRIEF L26 "render screenshots from several camera angles (later times of day and weather)"**
- **Problem:** this is met only once the shot lists exist (F).

**G-B10 · L · BRIEF L57 settings "resolution"**
- **Problem:** neither ADR-0041 nor the settings plan defines windowed-resolution choices
  versus render scale. List the supported modes (borderless fullscreen, windowed sizes).

### G-P Process and documentation problems that hurt a single GPU-less agent

**G-P1 · H · Spikes**
- **Problem:** see E1. PROGRESS "Next step" (L13–29) goes straight from M1 to M2 and never
  mentions spikes, so a cold-start agent will skip them.

**G-P2 · H · Lavapipe as the only renderer**
- **Problem:** a 1600×900 frame with CSM 3×2048², SSAO and bloom costs about 0.5–3 s on
  lavapipe, so screenshot suites of 15–25 shots plus settle frames take about 10–30 min.
  The CI `--smoke-test 300` in release adds roughly 15–30 min per push on top of the
  release build.
- **Fix:** smoke test at 640×360 for 60 frames in CI, the full suite locally only, and
  per-push caching of the release build.

**G-P3 · H · PERF L120–126; skill §5**
- **Problem:** CPU budgets "under Xvfb+lavapipe give CPU-side numbers only", but lavapipe
  rasterises on the same CPU cores, so main-thread and render-thread numbers are
  meaningless.
- **Fix:** `--no-gpu-work` benchmark mode (F-0).

**G-P4 · M · skill milestone-completion §9**
- **Problem:** step 9 requires all CI jobs, including macOS, to be green before "done". With
  macOS runner queues and costs, decide whether `render-smoke-macos` is excluded (it is
  `continue-on-error`) and what to do on a runner outage (retry policy, a "CI-pending"
  status).

**G-P5 · M · No effort estimates anywhere**
- **Problem:** without them neither the user nor the agent can detect a milestone running
  away (A50 ladder).
- **Fix:** add an estimate column (agent-days) to PROGRESS plus the actual spent.

**G-P6 · M · CLAUDE L5–21 session protocol**
- **Problem:** the protocol requires reading ARCHITECTURE (574 lines) and PROGRESS every
  session, plus topic docs. Context cost is high but acceptable. The danger is that
  PROGRESS "exact next step" is the only resume point and nothing enforces it.
- **Fix:** a pre-commit check (or a skill step) that fails if PROGRESS "Last updated" is
  older than the latest commit touching `crates/`.

**G-P7 · M · TESTING L29, RISKS (several)**
- **Problem:** user verification requests are scattered across 6 places.
- **Fix:** one "User verification queue" table in PROGRESS, listing item, milestone, exact
  command, expected output and status.

**G-P8 · L · ARCH L90**
- **Plan text:** spikes are "deleted after use".
- **Problem:** that loses evidence.
- **Fix:** keep spike code on a tag or branch (`spike/<name>`) and record the link in
  PROGRESS.

### G-O Other

**G-O1 · H · ALGORITHMS L675–679; ADR-0034**
- **Problem:** road picking "query segments near the ground hit (6 m)" fails for bridges and
  elevated roads. At pitch 35° the cursor over a 20 m-high deck hits the ground about 28 m
  away. Snapping to elevated nodes has the same issue.
- **Fix:** ray-vs-ribbon test against segment deck surfaces (profile height ± half-width)
  for spans with `StructureSpan::Bridge`/elevation > 1 m, and 3D distance ray↔node for
  snapping.

**G-O2 · M · ADR-0037 L51–55; ADR-0040 L231–233**
- **Problem:** an unsigned, quarantined `.app` launched from Downloads runs under **App
  Translocation** (a randomised read-only path), so "portable.txt next to the .app" and any
  exe-relative logic break.
- **Fix:** detect translocation (path contains `/AppTranslocation/`) and ignore portable
  mode with a notice. Document `xattr -dr` (already in the README).

**G-O3 · M · ADR-0019 L125–129**
- **Problem:** `bevy_audio` without an output device (CI, the container) must not error-spam
  or panic.
- **Fix:** a headless audio test using `cb_audio` only, and `AudioPlugin` disabled when no
  device is present (`--no-audio`).

**G-O4 · M · ADR-0019 L119**
- **Problem:** a 25–40 MB PCM bank synthesised "during Boot" takes about 1–4 s on one async
  thread, competing with world generation (≤ 6 s target, PERF L19).
- **Fix:** synthesise lazily per category on first use, with a disk cache in
  `AppPaths::cache` keyed by `(bank_version, seed)`, which counts as a runtime-generated
  cache, not an asset.

**G-O5 · M · GAME_DESIGN §13 / ADR-0021**
- **Problem:** tutorial strings depend on unlock tiers and key bindings, so text must embed
  the current binding glyphs (`{ $key }`) from the formatter, which is also affected by
  G-W2.

**G-O6 · M · ART_DIRECTION L205**
- **Plan text:** "Reduced-motion setting disables slides".
- **Problem:** camera smoothing, lightning flashes and wind sway are also motion. Scope the
  setting (UI motion versus world motion) in the accessibility section.

**G-O7 · M · ADR-0036 L159–160**
- **Problem:** catalog validation checks that "localization keys exist", but ARCH L277 does not
  list `cb_l10n` as a dependency of `cb_sim`.
- **Fix:** make `cb_l10n` a dev-dependency of `cb_sim` for that test (both are headless), or move
  the check into a `cb_game` test.

**G-O8 · L · PERF L18**
- **Problem:** "≤ 30 ms autosave hitch" sits beside D9's memory concerns. Snapshotting
  50k-citizen tables is a `memcpy`, about 2 ms. Entity snapshots of ~25k entities via
  queries take about 5–10 ms. That fits, but measure it in M5.

**G-O9 · L · DATA_MODEL L323–324; ARCH L557–560**
- **Problem:** `SimConfig.difficulty` exists, but difficulty only affects starting money
  (GAME_DESIGN L150).
- **Fix:** define the other effects or keep it money-only and say so.

**G-O10 · M · ADR-0022 L69–70**
- **Plan text:** "last 20 commands — updated in a OnceLock<Mutex<…>> each frame".
- **Problem:** locking per frame is fine, but the panic hook must use `try_lock` to avoid a
  deadlock when the panic happens while the lock is held.

### G-X Scope traps for a single agent

**G-X1 · H**
- **Problem:** the plan implements its own RNG, noise, ear clipping, spatial hash, input
  manager, path resolver, inspector, UTC formatter, SDF icon atlas (≈ 30 icons drawn in
  code), procedural audio bank (≈ 40 sounds), generative music, Fluent glue with codegen,
  roundabout-free junction control, freehand curve fitting, articulated trams, 5-level
  procedural typologies × 3 zones plus about 20 signature service buildings, plus 2 UI
  themes. Each is fine alone; together they are a scope trap without estimates (G-P5).
- **Fix:** adopt the A50 ladder and track estimate against actual.

**G-X2 · M · ADR-0023 L121–128**
- **Problem:** rejecting `proptest`/`criterion` means hand-rolled shrinking-free random
  tests. Failures in junction and subdivision property tests then hand back huge random
  cases that are hard to minimise.
- **Fix:** at least write a tiny shrinker (halve the counts and the angle ranges) in
  `cb_core::test_util`.

**G-X3 · M · PROGRESS M4**
- **Problem:** about 20 service, utility, transit and special generators plus R/C/I L1–L5
  fit in one milestone with a 10k-building performance gate.
- **Fix:** split the acceptance into "silhouette set A (police, fire, health, school, power
  ×2, water, sewage, garbage)" and "set B (the rest)", with set B allowed to finish in M6/M10
  where those systems arrive.

**G-X4 · M · ADR-0019 L130–131**
- **Problem:** generative music is "a stretch goal in M13", and the music channel "exists
  from the start". Remove the music content from M13 acceptance explicitly (the brief says
  "music optional").
