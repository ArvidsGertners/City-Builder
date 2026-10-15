# Owned Constants

Constants that affect stability, feel or balance, each with the test that owns it and the
metric it must satisfy. Change a value only together with its test target; record the change
in the PROGRESS.md log. Values live in code consts or `crates/cb_sim/data/*.ron`.

| Constant | Value | Owner test | Target metric |
|---|---|---|---|
| Sim tick `dt` | 0.1 sim s | `idm_stable_at_dt` | no oscillation/negative speed over 10⁶ steps |
| `SIM_SECONDS_PER_GAME_MINUTE` | 2.0 | `calendar_day_is_28800_ticks` | 28 800 ticks per game day |
| `SIM_PARTITIONS` | 64 | `results_identical_1_4_8_threads` | identical state hash |
| Driver tick cap | `ceil(1.5 × nominal)` ≥ 1 | `driver_caps_ticks_per_frame` | 1× → 1, 4× (L 24) → 2 |
| IDM car `a, b, T, s0` | 1.6, 2.5, 1.2 s, 2 m | `idm_saturation_flow_matches_capacity` | street lane 1 600 veh/sim-h ± 10 % |
| `a_lat_max` (curve speed) | 2.5 m/s² | `curve_speed_r25` | 7.9 m/s at r = 25 m |
| Critical gaps (left/merge/cross) | 4.0 / 3.0 / 3.5 s | `junction_unsignalized_minor_flow` | minor throughput ≥ 450 veh/sim-h at major 600 |
| Anti-starvation | 12 sim s | `equal_priority_4way_all_occupied_no_idle_gt_2s` | no approach idle > 2 s while waiting |
| Signal timings | min 8, max 30, yellow 3, all-red 1.5 s | `signal_discharge_rate` | ≥ 1 500 veh/sim-h/lane of green |
| Junction release headway `h0` | 1.8 s | `background_headway_scaling` | flow reduced by V_bg/C as specified |
| Stuck timeout | 90 sim s | `traffic_multi_day_large` | despawns < 1 per 1 000 trips |
| Lane-plan runway | 40 m per change | `lane_plan_feasible_on_short_links` | ≥ 99 % plans feasible on citygen networks |
| Spawn TTC / wait | 2 s / 30 sim s | `spawn_queue_never_overlaps` | 0 overlaps; macro fallback counted |
| Flow EMA half-life | 120 sim s | `flow_ema_tracks_step_change` | 90 % of step within 400 sim s |
| Cost epoch | 1 game hour (120 sim s) | `route_flapping_bounded` | ≤ 10 % of commuters change route per epoch in steady state |
| Routing budget | 50 000 units/tick | `routing_budget_wall_time` | ≈ 0.65 ms nominal, ≤ 0.8 ms p99 wall (container proxy) |
| Route cache size | 32 768 | `route_cache_hit_rate` | ≥ 80 % in steady-state 50k bench |
| Variant cost range | +0…+20 % | `alt_heuristic_admissible_with_variants` | no overestimate |
| Trip scale | 6 citizens' trips per vehicle trip (ADR-0062) | spike S-Time / `trip_scale_50k_peak` | 50k city: peak concurrent vehicles 4–7 % of population (realistic count, user requirement), on-time ≥ 90 %, peak ≤ agent cap, congestion p95 2–4× |
| Agent cap default | 5 000 | spike S-Time / `agent_share_at_50k` | ≥ 40 % of peak trips are agents (S-Time: 100 % at trip scale 6) |
| Shift starts | commercial N(08:45, σ 75) [06:30, 11:30]; industry N(07:30, σ 45) [06:00, 11:00] | spike S-Time | night share ≤ 10 %; peak/off-peak (10–13 h) ≥ 3 |
| Perceived time factor | 0.1 min per sim s | `commute_p50_balance` | p50 12–25 perceived min |
| Corridor carve / shoulder | 0.15 m / 3.5 m | `road_above_terrain_all_lods` | terrain ≤ road − 0.03 m under carriageway |
| Landing grade | 4 % | `junction_surface_continuous_8pct` | cut-line step ≤ 2 cm |
| Cut-back cap | 45 % of segment | `junction_rejection_rate_standard` | ≤ 3 % rejections on realistic layouts with the live guide (S-J: 0.65 %) |
| Max junction turn | 135° deflection (ADR-0063) | `junction_random_10k_valid_or_typed_error` | every allowed movement has a connector; no growth runaway |
| Cut-back growth step | 2 m, ≤ 40 iterations | `junction_generation_p99` | p99 ≤ 0.4 ms (S-J: 1.4 ms, M3 optimization) |
| Ray de-crossing | 8 × 5° | `subdivide_random_10k_invariants` | no crossing rays; ≤ 3 % merged lots |
| Rectangularity min | 0.55 | `subdivide_random_10k_invariants` | ≥ 95 % of lots pass before merging |
| Lot match overlap | 0.6 | `lot_ids_stable_under_small_drags` | ≥ 90 % IDs kept for ≤ 1 m drags |
| Setbacks R/C/I | 3/1.5/4, 1/0/2, 4/2/4 m | `building_footprint_within_lot_10k` | 100 % within lot; ≥ 90 % R lots fit L1–L3 ≥ 50 m² |
| Fire growth | 60 s ramp, damage/100 | `fire_contained_with_station_within_reach` | contained when station within reach |
| Pollution decay | 0.97 / game h | `pollution_decays_0_97_per_hour` | exact |
| Demand EMA half-life | 6 game h | `demand_monotonic_responses` | monotonic responses on synthetic inputs |
| Growth rate | `clamp(0.5 + pop/2000, 0.5, 6)` / game h | `balance_tier1_time` | tier 1 in 12–25 real min |
| Leveling thresholds | up `[0.35, 0.5, 0.62, 0.75]`, down −0.15 | `leveling_fully_served_reaches_l3` | L3 within 10 game days at LV 0.8 |
| Undo refund decay | 100 % → 25 % over 1 game day | `undo_refund_decays` | exact; `undo_after_window_rejected` |
| Render scale target | 2.2 MP integrated / 3.7 MP discrete | `auto_render_scale_table` | table in ADR-0053 |
| LOD band width | ≤ 10 % of range | `lod_bands_short` | config assert |
| Upload budget | 8 MB/frame | `--benchmark` report | p99 render-thread ≤ budget |
| Env-map regen | ≤ 1 per 2 real s | `env_map_throttle` | exact |
| Audio loudness targets | UI −20, one-shots −18, beds −28, sirens −16 LUFS | `audio_loudness_targets` | ±1 LU, peak ≤ −1 dBTP |
