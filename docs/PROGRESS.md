# Progress

Read at the start of every session (after CLAUDE.md and ARCHITECTURE.md). Update after every
milestone and before ending a session (skill `milestone-completion`).

## Current status

- **Phase:** M4 all zone types, special buildings, leveling visuals ☑ (CI green on all three OSes;
  UV-5, UV-11 open) → M5 in progress. M3 ☑ (UV-10 open). M2 ☑
  (UV-2/3/4 open). Phase S ☑ — S-Render ☑, S-Time ☑ (fallback
  ADR-0062), S-J ☑ (ADR-0063), S-P ☑ (ADR-0064), S-G ☑ (ADR-0065), S-T ☑ (ADR-0066/0067).
- **M2 done so far (pure crates):** `cb_terrain` (worldgen with rivers/lakes/outside connections,
  stamps + order-independent compositing, chunk meshes LOD0–3 with skirts, raycast, scatter;
  `worldgen_features_seed_1_to_20`, `terrain_generation_deterministic` pass, worst generation
  1.75 s); `cb_geometry` (Béziers, arc length, polylines, polygons, i_overlay booleans, ear
  clipping, MeshData, Schneider fitting, **frontage + ADR-0064 subdivision + building fitting**:
  `subdivide_random_10k_invariants`, `building_footprint_within_lot_10k`,
  `lot_ids_stable_under_small_drags`, `lot_ids_n_plus_minus_one_keep_n_minus_2`,
  `subdivide_fan_le_25_or_tight_curve`, `subdivide_corner_lot_fronts_both_roads` pass);
  `cb_network` (cross-sections, segment geometry, profile solver, validation reasons with l10n
  keys, crossing fixed point); `cb_procgen` (palette + HSV jitter, packed surface attribute,
  painter with baked AO, R L1–L3 houses with gable/hip roofs, windows, doors, chimneys, wings,
  duplexes, pad/plinth/stilt foundations, yards ≤ 400 tris, trees, road segment sweep;
  `houses_within_lots_and_budgets`, `procgen_golden_hashes`). The determinism probe prints
  terrain, subdivision and M2-scene hashes.
- **M2 done so far (`cb_sim`, `cb_game`):** sim world state (commands incl. `EditPlot`, read-only
  `preview_road`/`preview_plot`/`preview_edit_plot`, `citygen::grid` + `citygen::m2_features`);
  renderer (world/terrain/water materials, focus fog, lighting keyframes calibrated per
  ADR-0050 amendment, exposure-scaled sky dome), camera, picking (`ScriptedCursor` for suites),
  view sync (terrain/water/roads/junctions/houses/scatter, shared 2 ms rebuild budget), road tool
  (straight/curved/freehand, ghost with stripes, reason + cost label), plot tool (frontage drag,
  end/depth handles, ± lots, depth ±, delete), bulldoze, toolbar + money HUD + toasts, zoning
  overlay (draped fills, outlines, lot lines), `--benchmark` (per-set timings) and the 14-shot M2
  suite (`at:`/`hour:`/`ui:`/`zoning:`/`tool:`/`crack_check:` per shot), off-thread chunk
  meshing, merged far terrain, junction terrain stamps. M2 complete, CI green on all three OSes.
- **Last updated:** 2026-10-17
- **Verified environment:** Bevy 0.20.0 (2026-10-08), bevy_egui 0.43.1 (Bevy 0.20), rustc 1.97.1;
  `bevy-inspector-egui` 0.37 targets Bevy 0.19 (ADR-0031); `mesa-vulkan-drivers` (lavapipe) and
  Xvfb installable in the container for rendering without a GPU.

## Next step

**M5 — Simulation core.** First action: in `crates/cb_sim/src/clock.rs` add the owning tests
`calendar_day_is_28800_ticks` and `season_changes_every_4_days` for `SimClock::calendar` and the
`HourElapsed`/`DayElapsed`/`SeasonChanged` messages (headless, `crates/cb_sim/tests/calendar.rs`),
then start the population model in `crates/cb_sim/src/citizens/{population,households}.rs`
(`CitizensPlugin`, ARCHITECTURE §3: households per residential lot from `ZoneSpec` capacities,
seeds via `derive_seed` from the `LotId` with a new household domain) feeding
`growth_scenario_flat_3days`. Read ADR-0044 (save core) before touching `save/`. Carry-overs: the
city10k centre is hilly (empty commercial blocks, see `docs/screenshots/M4/index.md`); split
`cb_game/src/tools.rs` further (road/edit/plot/bulldoze submodules like `tools/place.rs`).

## Process conventions (ADR-0058)

- Status: ☐ not started · ◐ in progress · ☑ done · **☑ᵤ done, pending user verification**
  (never blocks) · **☑ CI-pending** (runner outage; re-check next session).
- Every milestone/spike has an estimate (agent-days) and records the actual. Over 1.5× →
  apply the **scope-cut ladder** (bottom of this file) and log it.
- Requests to the user are batched in the **User verification queue**; reported
  discrepancies become bugs in **Known bugs**.
- Each visual milestone produces `docs/screenshots/<M>/` via `visual-verification`. Shots are
  1600 × 900, UI hidden unless noted, deterministic suite `<M>` (seed + command script +
  fast-forward tick) defined in `crates/cb_game/src/debug/screenshot_suites.ron`. "d" = camera
  distance to focus, pitch from horizontal. Every suite also passes the metric gates: 0
  magenta/NaN pixels; pure black (< 8/255) ≤ 0.5 % in daylight shots; semantic-pass assertions
  listed per shot.
- CPU budgets are measured in the container with `--benchmark <scene> --no-gpu-work`: the
  game's own sets (input + sim-drive + view-sync + present) against their budget, with the
  `empty` engine baseline recorded alongside; full-frame CPU and GPU budgets only from user runs
  (☑ᵤ; ADR-0058 amendment).

## Milestones

| # | Milestone | Estimate (agent-days) | Actual | Status |
|---|---|---|---|---|
| 0 | Phase 1 planning package + independent review | 2 | 2 | ☑ |
| 1 | Project setup | 4 | 1 | ☑ |
| S | Risk spikes (S-Render, S-Time, S-J, S-P, S-G, S-T) | 22 | ≈ 4 | ☑ (S-Render ☑, S-Time ☑ fallback, S-J ☑ ADR-0063, S-P ☑ ADR-0064, S-G ☑ ADR-0065, S-T ☑ ADR-0066/0067) |
| 2 | Vertical slice | 12 | ≈ 6 | ☑ (☑ᵤ UV-2/3/4 open) |
| 3 | Full road system | 12 | ≈ 6 | ☑ (☑ᵤ UV-10 open) |
| 4 | All zone types, special buildings, leveling visuals | 10 | ≈ 3 | ☑ (☑ᵤ UV-5, UV-11 open) |
| 5 | Simulation core | 12 | | ☐ |
| 6 | Services | 6 | | ☐ |
| 7 | Progression | 3 | | ☐ |
| 8 | UI, overlays, info views | 10 | | ☐ |
| 9 | Traffic | 12 | | ☐ |
| 10 | Public transport | 8 | | ☐ |
| 11 | Day/night cycle | 5 | | ☐ |
| 12 | Weather | 7 | | ☐ |
| 13 | Audio | 5 | | ☐ |
| 14 | Tutorial | 4 | | ☐ |
| 15 | Menus, settings, accessibility, save/load, localization | 8 | | ☐ |
| 16 | Polish, performance, bug fixing | 8 | | ☐ |

Every milestone additionally requires: fmt check, clippy `-D warnings` (default features and
`--features dev`), `cargo test --workspace` green locally and in CI on Linux, Windows and macOS;
docs updated; commit `M<N>: …`; the `milestone-completion` skill completed.

---

### M0 — Phase 1 planning package ☑
CLAUDE.md, skills, ARCHITECTURE, DATA_MODEL, ALGORITHMS, GAME_DESIGN, ART_DIRECTION,
PERFORMANCE, TESTING, RISKS, CONSTANTS, ASSUMPTIONS, PLAN_REVIEW (+ raw independent review in
`docs/review/`), ADRs 0001–0058, README.

### M1 — Project setup (estimate 4, actual 1) ☑
Includes enabling work moved forward: settings persistence (from M15), Phase S definitions.
- [x] `cargo build --workspace --all-targets` on 3 OS; CI job `sim-isolation` runs
      `scripts/check_sim_isolation.sh` (fails on `bevy_render|bevy_pbr|wgpu|bevy_winit|bevy_egui|bevy_audio` in `cargo tree -p cb_sim -e normal`).
      — Linux local; Linux/Windows `ci.yml`, macOS via nightly dispatch (see log). Script also
      rejects `bevy_ui`/`winit`; verified it matches on `cb_game` (13 hits) and passes on `cb_sim`.
- [x] `cb_core`: tests `derive_seed_no_collisions_1e6`, `derive_seed_order_sensitive`,
      `rng_streams_reproducible`, `math_facade_reference_values`, `lookup_map_has_no_iteration`
      (compile-fail doc test), `spatial_hash_matches_brute_force`, `gametime_from_tick`. — 24 tests + doc test green.
- [x] Paths: `paths_resolve_windows`, `paths_resolve_linux`, `paths_resolve_macos`,
      `paths_portable_mode`, `paths_cb_userdata_override`, `paths_app_translocation_ignores_portable`.
      — `cb_game::platform::paths` (pure resolver, all layouts tested on every OS).
- [x] Logging: `ring_buffer_keeps_last_500`; integration test `log_line_written_to_app_paths_logs`.
- [x] Crash: `crash_report_contains_all_sections` (subprocess with `--debug-panic`): summary,
      message+location, thread, backtrace, version+git hash, OS/arch, adapter, settings summary,
      sim summary, last 200 log lines; next start shows the "last session crashed" egui window.
      — test runs the real binary with `CB_USERDATA`; the notice is `ui::crash_notice` (en/de).
      Adapter/sim sections read "(not yet known)" when the panic precedes the renderer/sim.
- [x] Settings: `settings_roundtrip_all_fields`, `settings_invalid_field_falls_back`,
      `settings_unknown_version_migrates_or_defaults`. — per-field fallback via `ron::Value` overlay.
- [x] l10n: en + de bundles, `build.rs` key constants; `l10n_keys_complete_en_de`,
      `l10n_placeholders_match`, `format_number_money_date_time_en_de`.
- [x] Sim skeleton: `driver_runs_1000_ticks_in_lt_100ms`, `driver_step_once_advances_exactly_one_tick`,
      `driver_caps_ticks_per_frame`, `driver_day_length_is_pacing_only` (same hash for L = 16/24/48),
      `schedule_shuffle_same_hash` (3 seeds, single-threaded), ambiguity detection = Error.
      — plus `schedule_ambiguity_is_error`; 1 000 ticks = 1.2 ms after ADR-0061 (was ~70 ms with
      the multi-threaded executor). Shuffling is active with feature `debug_schedule` (`dev` leg).
- [x] `cb_game --smoke-test 60 --resolution 640x360` exits 0 on lavapipe and logs
      `frames=60 validation_errors=0`; window title localized. — log: `window: 640x360 "City Builder"`,
      adapter llvmpipe (Vulkan, CPU); release build also verified (see log).
- [x] Dev tooling (`--features dev`): perf overlay shows frame time p50/p95, sim tick time,
      entity count; sim controls pause/step/speed/day length; log console; inspector lists ≥ 1 sim
      entity kind with Reflect fields. — `docs/screenshots/M1/M1-04-dev-tools.png`; the inspector
      lists the reflected fields of the sim resources (`SimClock`, `SimConfig`, `WorldSeed`,
      `SimSpeed`, `SimPacing`); sim entities do not exist before M2.
- [x] CI: `ci.yml` (Linux + Windows × {default, dev}; macOS only on main; sim-isolation,
      render-smoke Linux, cross-OS determinism Linux + Windows), `nightly.yml` (schedule +
      dispatch: macOS check × {default, dev}, macOS smoke informational, 3-OS determinism),
      `release.yml` (validated via `workflow_dispatch` with `publish: false` producing all three
      artifacts); full pipeline ≤ 25 min. — results in the log.
- [x] `scripts/screenshot.sh M1` renders one test shot to `docs/screenshots/M1/` (pipeline proof).
      — 3 shots + `metrics.json` + `index.md` (0 magenta, 0 % black, 0 % clipped).
- [x] `LICENSE` (proprietary), `THIRD_PARTY_NOTICES.md`, Inter font embedded with
      `font_covers_required_glyphs` test (ADR-0059). — test parses the cmap and also checks every
      character used in the en/de `.ftl` files.
- [x] README build/run/controls; PROGRESS updated.

### Phase S — Risk spikes (estimate 22; order as listed)
Throwaway code in `spikes/<name>/`; tag `spike/<name>` before removal; outcome row filled in;
fallbacks require a superseding ADR.

**S-Render — material pipeline, vertex format, render scale, shader validation (3 days)**
- Build: minimal Bevy 0.20 app with `WorldMaterial` (`ExtendedMaterial`, WESL) using the
  ADR-0049 vertex layout (pos f32x3, color unorm8x4, packed u32), derivative flat normals in
  forward + prepass, per-chunk `ShaderBuffer` slot lookup, CSM shadows, SSAO, SMAA, bloom,
  TonyMcMapface; focus fog; offscreen render-scale target + composite vs
  `MainPassResolutionOverride`; GPU-free shader validation (bevy_shader `ShaderCache` + naga
  backends); screenshot capture with the settle rule, `--screenshot-metrics`, semantic pass.
- Pass: lavapipe renders with 0 validation errors; faceted shading correct (per-face normal
  variance from the prepass normal buffer ≤ 0.02); SSAO without black artifacts (pure black ≤
  0.5 %); shadows with custom layout; ≤ 25 KB GPU memory per 1k-triangle building;
  `cursor_ray_dpi_render_scale` unit test passes for scale factors {1, 2} × render scales {1, 2/3};
  shader validation translates all spike shaders to MSL/HLSL/SPIR-V (or is documented infeasible).
- Fallback: standard Bevy layout with normals (duplicated vertices) + tighter residency; offscreen
  composite if the override fails; validation via smoke tests only.
- Outcome (2026-10-15, actual 0.5 day): **PASS, no fallback.** Evidence in
  `docs/screenshots/S/` (index.md) and `spikes/s_render/` (kept as M2 reference; tag
  `spike/s-render` and delete once M2's `WorldMaterial` lands).
  - 0 wgpu validation/shader errors on lavapipe; faceted shading from derivatives in forward
    **and** prepass (within-face normal variance 0.00019 ≤ 0.02); SSAO without black artifacts
    (0 % pure black); CSM shadows from custom-layout meshes; per-slot `ShaderBuffer` lookup works.
  - Memory: 1 000-triangle building = 605 shared vertices × 20 B + 3 000 u16 indices = 17.7 KiB
    (≤ 25 KB; excludes allocator slab rounding).
  - `cursor_ray_dpi_render_scale` passes for scale factors {1, 2} × render scales {1, 2/3}.
  - Render scale: offscreen target + composite works; `MainPassResolutionOverride` is an
    upscaler hook (render world only) and has no effect alone → **composite** (ADR-0053 amendment).
  - GPU-free shader validation **feasible**: record pipeline variants (shader + defs) in
    `specialize`, compose them with Bevy's own `ShaderCache<(), ()>` (WESL → WGSL; add the
    `AVAILABLE_STORAGE_BUFFER_BINDINGS*` globals Bevy splices in later), then naga →
    SPIR-V + MSL + HLSL: 5 variants → 3 modules, 0 failures. For the game: commit a variant
    manifest (shader path + defs) captured by the lavapipe smoke run, and a test that composes
    and translates it on every OS (`naga` as a dev-dependency, ADR-0023 row at M2).
  - Findings for M2: push `VERTEX_COLORS` in `specialize` so Bevy's `VertexOutput` carries the
    color; linearize sRGB albedo on the CPU before packing unorm8; multiply AO into rgb in the
    vertex stage; the forward `VertexOutput` has no free field for per-fragment flags (carry them
    via the `uv` slot with `VERTEX_UVS_A` if needed); the prepass `FragmentOutput` only exists
    under `PREPASS_FRAGMENT` (guard the import); fog colour must be pre-exposure scene luminance
    (≈ albedo × E / π), not a display colour (ADR-0050 amendment); Bevy logs a harmless
    "default normal prepass expects vertex normals" warning for the custom material.


**S-Time — time model and trip scale (2 days)**
- Build: headless synthetic 50k-citizen city (clustered homes/jobs on 4 × 4 km), trip durations
  from network distance at road speeds + 20 % junction delay, ADR-0042 scheduler, point-queue
  link model for congestion feedback.
- Measure: commute duration (game min) p50/p95, departure histogram, concurrent trips over the day,
  required `p` at cap 5 000, late arrivals, departures before 04:00, night (00–04) share,
  peak/off-peak ratio.
- Pass: p95 departure ≥ 04:30; ≥ 90 % arrive within ±30 game min of shift start; night share
  ≤ 10 %; peak/off-peak ≥ 3; `p ≥ 0.4` at 50k; results identical for day lengths 16/24/48 (by
  construction, asserted).
- Fallback (by measurement): wider shift σ; commute cap in job matching; higher cap with meso
  model (S-T fallback 2); as last resort a different sim/game ratio (superseding ADR-0042).
- Outcome (2026-10-15, actual 0.3 day): **PASS with fallback (ADR-0062: trip scale 6 +
  tuned shift data)**; two gates missed and accepted (see below). Code `spikes/s_time/`
  (deterministic, 2.6 s per run; tag `spike/s-time` before removal).
  - Baseline as planned (1 car trip per citizen): commute perceived p50/p95 66.8/560.7 min,
    congestion 5.3×/27.9×, on time 25 %, peak concurrent 45 633, p = 0.11 → **fail**.
  - Chosen (trip scale 6, commercial N(08:45, σ 75) [06:30, 11:30], industry N(07:30, σ 45)
    [06:00, 11:00], gravity 1.5 km): perceived p50/p95 18.6/39.5 min (free flow 16.4),
    congestion p50/p95 1.01/2.83, on time ±30 game min 92.6 % ✓, night share 2.7 % ✓,
    peak concurrent 3 071 → p = 1.0 ✓, results identical for L = 16/24/48 ✓ (asserted).
  - Missed: morning departure p05 = 04:05 (gate 04:30) — commutes span 1.5–3.3 game hours on the
    clock under ADR-0042; peak/off-peak 2.6 over 10–15 h (gate 3) because school returns at
    14:30 fall into the window; 4.6 over 10–13 h. Accepted in ADR-0062; CONSTANTS updated.
  - Departures per game hour (chosen setting): `[148,49,36,221,549,1198,1658,1569,616,495,416,673,763,779,2844,1649,1460,729,366,144,57,24,68,155]`.


**S-J — junction geometry (4 days)**
- Build: random junction generator (2–6 approaches, angles 25–180°, widths 6/12/16/22 m + tram,
  curvature up to 1/r_min, segment lengths 6–60 m, grades to max in random directions); cut-back,
  polygon, crosswalks, connectors (per-kind radii), conflict zones, per-vertex surface, approach
  end strips; SVG/OBJ dumps.
- Cases: 10 000 random; canonical T, X, Y-30°, 5-way, avenue×street, avenue×avenue, one-way pair,
  two junctions 30 m apart, 8 % slope, tram straight + tram turn on avenue median, roundabout ring.
- Pass: 0 panics; 100 % valid geometry or typed `ValidationError`; rejection ≤ 3 % on the
  standard distribution (angles ≥ 40°, segments ≥ 30 m); simple polygons; every allowed movement
  has a connector; same-approach connectors never cross; κ ≤ 1/5 m car, ≤ 1/20 m tram; connectors
  within polygon ∪ approach strips (0.3 m); cut-line step ≤ 0.02 m at 8 %; p99 ≤ 0.4 ms per
  junction.
- Fallback: degree ≤ 4 and min angle 40°; parameterized T/X templates; flat 6 m landings; tram
  turns only at dedicated wide junctions.
- Outcome (2026-10-15, actual 0.5 day): **PASS with rule changes (ADR-0063 supersedes
  ADR-0045 geometry)**; fallback (degree ≤ 4, templates) not needed. Code `spikes/s_j/`
  (deterministic; `cargo run --release -- --svg DIR`); evidence `docs/screenshots/S/S-J-*`.
  - Canonical: T, X, 5-way, avenue×street, avenue×avenue, one-way pair, junctions 30 m apart,
    8 % slope, tram straight, tram turn on the avenue median, roundabout ring node → all valid,
    0 violations, cut-line step 0.0 mm; Y-30 at 60 m → `TooClose` (needs ≈ 63 m; guide shows it).
  - 0 panics in 40 000 random junctions; 100 % valid geometry or typed error; 0 invalid polygons.
  - Rejections: realistic layouts with the live spacing guide **0.65 %** ✓ (harsher guided set
    1.18 % ✓); with fixed 30–60 m segments 72 % → static spacing table replaced by the live guide.
  - Realistic set: every allowed movement has a connector ✓; different lanes never cross ✓;
    curvature limits ✓; connectors within the polygon ✓ (growth); step ≤ 2 cm ✓.
    Harsher set: 4 crossings + 1 curvature case in 9 882 (0.05 %) → M3 regression cases.
  - Timing ✗: p50 0.17 ms, p99 1.4 ms (target 0.4 ms) → M3: angle-aware initial cut-back,
    per-approach incremental growth; fallback async rebuild (ADR-0063 consequences).
  - Rule changes: movements > 135° not allowed; corner allowance `min(r, r/tan(α/2))`;
    iterative growth for curvature / containment / simplicity; quadratic convex fillets and
    kerb-following reflex corners; monotone lane assignment with re-sourcing; same-lane
    alternatives may cross (conflict zone between them).


**S-P — plots → lots → buildings (4 days)**
- Build: frontage generators (straight; convex r 30–300 m; concave r 25–300 m; S-curve; corner
  60–120°; two segments), depth 15–80 m, subdivider with N auto and N ± 3, rect_fit, L1–L3
  massing; edit sequences (vertex drag 0.25/1/3 m, N ± 1, depth ± 5 m); SVG contact sheets.
- Inputs: 10⁴ random plots + 24-plot showcase set.
- Pass: 0 panics; 100 % lots touch frontage ≥ w_min; area conserved ±0.5 %; no overlaps; ≥ 95 %
  rectangularity ≥ 0.55 before merging, ≤ 3 % merged; ≥ 90 % R lots fit an L1–L3 footprint
  ≥ 50 m²; drags ≤ 1 m keep ≥ 90 % LotIds, N ± 1 keeps ≥ (N − 2)/N; p99 ≤ 2 ms per plot;
  showcase sheet reviewed against: no fan > 25° between adjacent side edges, corner lots front
  both roads.
- Fallback: frontage-drag only; parallel side edges on concave curves; dedicated corner-lot
  generator; polygon-extruded townhouse rows for lots failing rect_fit.
- Outcome (2026-10-15, actual 0.5 day): **PASS with rule changes (ADR-0064 supersedes
  ADR-0014)**; no fallback needed. Code `spikes/s_p/` (deterministic;
  `cargo run --release -- --random 10000 --edits 3000 --svg DIR`); evidence
  `docs/screenshots/S/S-P-*` (showcase, first 24 random, 24 worst by orphans/merges/min rect).
  - 10 000 random plots (86 573 lots): 0 panics ✓; 0 lots with frontage < w_min ✓; max area
    error 0.007 % ✓; 0 overlaps ✓ (intersection thicker than 5 mm; union re-snapping leaves
    ≤ 2.5 mm slivers); rectangularity ≥ 0.55 before merging 99.2 % ✓; merged 1.5 % ✓; R lots
    fitting an L1–L3 footprint ≥ 50 m² (both sides ≥ 6 m) 98.3 % ✓; footprints always inside
    the lot ✓; p50 0.30 ms, p99 1.63 ms, max 8.5 ms per plot (subdivision + fit) ✓.
  - Per family (rect pass / merged / R fit): straight 100/0.4/100, convex 100/0.2/100, concave
    100/0.6/94.7, S-curve 100/0.5/97.9, corner 98.8/2.5/99.3, two-segment 96.8/4.3/97.7 %.
  - Edits (3 000 plots × 10 kinds, 28 598 re-subdivisions, same invariants all ✓): drags
    ±0.25/±1 m keep 99.8 % of LotIds ✓ (±3 m 99.3 %); depth ±5 m 99.8 %; N + 1 keeps 94.4 %,
    N − 1 89.6 %, `(N − 2)/N` met on 99.5 % / 99.8 % of feasible edits; 845 / 511 requests were
    infeasible (already at the max/min count) → rejected, control disabled.
  - Showcase: corner lots front both roads 4/4 ✓; max fan 27° on the tightest curves (concave
    r 25 m, convex r 30 m) → rule stated as `fan ≤ max(25°, 1.2 · w_min / r)`; 0.74 % of random
    non-corner lots exceed 25°, all on such curves.
  - Rule changes (ADR-0064): no PCA blend (β = 0; the blend caused slivers, skewed corner arms
    and end-lot fans); medial-axis seams for concave arcs with r < depth; constrained width
    solver with a curvature fan cap and pinned splits at kinks/corners; persisted pre-repair
    split state + stored merges; best-position search for ±1 lots; edit hysteresis (×1.1
    bounds, rect 0.50); plot auto-clip against adjacent corridors; fit fast paths (convex
    half-plane inset, adaptive raster, ≤ 4 orientations) — fit had been 90 % of the time.

**S-G — road–terrain interface incl. bridges (4 days)**
- Build: seeded 2049² terrain; 150 random roads (cut/fill); 30 junctions on 4–10 % slopes; 5
  parallel road pairs 8–20 m apart at different heights; 6 bridges (river, 30 m valley, overpass
  over a street); ADR-0046 compositing vs the old max-weight variant; terrain chunk LOD0–3 with
  skirts; automated sampler (road surface − rendered terrain at lane and edge samples ±0.5 m, at
  every LOD visible at that distance); 12 lavapipe shots into `docs/screenshots/S/`.
- Pass: under carriageway LOD0 terrain ≤ road − 0.03 m everywhere; LOD1 ≤ road for ≥ 99.9 % of
  samples (rest hidden by skirts in shots); outside corridors adjacent-sample |Δh| ≤ tan(max side
  slope) × 2 m + 0.05 m; junction and abutment steps ≤ 0.02 m; piers never inside corridors;
  overpass leaves the lower corridor unchanged; stamp removal restores base bitwise; re-stamp +
  LOD0 re-mesh of a 300 m road ≤ 25 ms async and ≤ 1.5 ms main thread.
- Fallback: deeper carve + depth-biased road mesh with 0.6 m skirts; conservative LOD
  downsampling everywhere near corridors; straight horizontal bridge spans with ramps.
- Outcome (2026-10-15, actual 0.5 day): **PASS with rule changes (ADR-0065 supersedes
  ADR-0046)**; no fallback needed. Code `spikes/s_g/` (deterministic; `cargo run` prints the
  metrics, `-- --shots DIR` renders the 12 shots under lavapipe); evidence `docs/screenshots/S/S-G-*`.
  - Scenario: 2049² terrain (river, 30 m valley), 270 roads (150 random), 30 junctions on 4–10 %
    slopes, 5 parallel pairs (gaps 9–19 m, Δh 1.9–3.9 m), 2 river + 2 valley bridges, 2 overpasses.
  - Sampler (545 083 lane/edge samples): LOD0 terrain ≤ road − 0.03 m on 100 % of carriageway
    samples ✓ (2 sidewalk-edge samples at −0.019/−0.027 m, under the skirt); LOD1/2/3 ≤ road on
    100 % ✓ (plain decimation: 99.95 %).
  - 0 cliffs outside corridors and decks ✓ (max-weight composite: 58 two-road cliffs; continuous:
    1 two-road interaction → `road.too_close_height` validation); junction cut-line step 0.7 mm ✓;
    abutment step 0.3 mm ✓; 0 piers in corridors ✓; overpass leaves 3 219/3 219 lower-core samples
    bitwise unchanged ✓; stamp removal restores the base bitwise ✓.
  - Timing: 300 m road re-stamp + profile + composite + LOD0 re-mesh p50 3.2 ms, max 9.4 ms ✓
    (≤ 25 ms async); main-thread apply only as a buffer-copy proxy (p50 0.28, p95 0.66, max 1.7 ms)
    → real Bevy mesh replacement measured in M2.
  - Rule changes (ADR-0065): constrained LOD decimation instead of min-downsampling (min sagged up
    to 13 m beside hillside roads; LOD3 road-edge gaps 24 % → 8 %); blend width 1.6·|e|/slope;
    adaptive stamp extent; spans classified from the terrain-following profile only (the deck
    re-solve had turned roads into viaducts); grade-feasible clearance envelope (overpasses had
    been rejected); bridge soft end caps + abutment pad at deck underside; clearance 4.5 m and
    pier re-placement for roads under decks; skirts by LOD (terrain 3/4/6/8 m, road
    0.6/1.0/2.0/4.0 m).

**S-T — traffic at scale (5 days)**
- Build: synthetic 4 × 4 km network (6 000 segments; mixed T/X/Y/5-way; 4-lane avenues;
  one-ways; a 6-entry ring), straight lanes + Bézier connectors, conflict zones, ADR-0045
  admission with reservations, IDM, lane plans + MOBIL-lite, ALT A* with variants + LRU + cost
  units, synthetic OD, stuck detection + cycle breaker.
- Cases: (1) 4-way equal priority saturated 1 game h; (2) signalized avenue×avenue at V/C 0.9 and
  1.2; (3) 25 m block between signals, 60 % need 2 lane changes; (4) 2→1 merge; (5) 6-entry ring
  at V/C 1.0; (6) full network with 5 000 / 8 000 / 12 000 agents for 2 game days; (7) bulldoze a
  central avenue mid-peak; (8) 1 / 4 / 8 threads.
- Pass: vehicle step for 5 000 agents p95 ≤ 0.9 ms with sequential pass ≤ 0.35 ms (container,
  4 threads); junction control ≤ 0.3 ms; routing ≤ 0.8 ms p99 per tick incl. case 7; 0 overlaps
  and 0 conflicting zone occupancy; saturated signal discharge ≥ 1 500 veh/sim-h/lane of green;
  case 1 never idle > 6 s while vehicles wait; despawns < 1 per 1 000 trips at V/C ≤ 1.0; case 2
  at V/C 1.2 recovers within 180 sim s after demand drops; identical state hash for 1/4/8 threads;
  route cache hit rate reported (target ≥ 80 % in case 6).
- Fallback ladder: forced signals at ≥ 4-approach equal-class junctions + roundabout controller;
  mesoscopic link-queue model for non-visible traffic; default cap 3 000.
- Outcome (2026-10-15, actual ≈ 1.5 days): **PASS with rule changes (ADR-0066 supersedes
  ADR-0047 and the control section of ADR-0063; ADR-0067 supersedes ADR-0048)**; no fallback-
  ladder step needed. Code `spikes/s_t/` (`--case 1…8`, `--case 61 --cap N --days D` single city
  run, `--case 62` router micro-benchmark; `ST_DEBUG=1` prints every stuck despawn).
  - Network: 10 428 links, 2 922 nodes, 30 601 connectors, 68 849 conflict zones (54 × 54
    jittered grid, avenues every 8th line with signals at avenue × avenue, one-way rows, 6 %
    removed segments, diagonals, 6-entry ring); commuter OD homes → jobs near avenues/centre.
  - Isolated cases ✓: (1) 4-way equal never idle > 1.2 s (≤ 6), capacity 1 846 veh/sim-h;
    (2) discharge 1 621 veh/sim-h/lane of green (headway 2.22 s), V/C 1.2 recovers in 74 s
    (≤ 180), V/C 0.9 in 108 s; (3) 25 m block 0 despawns; (4) 2→1 merge 1 002 veh/sim-h,
    0 despawns; (5) ring saturated 1 572 veh/sim-h, 0 despawns; at V/C 1.0 0 despawns. 0
    overlaps and 0 zone conflicts in every case and run.
  - City, 5 000 agents, 2 game days, 4 threads ✓: vehicle step p95 0.58–0.65 ms (gate 0.9),
    sequential 0.19–0.22 ms (0.35), junctions 0.27–0.29 ms (0.3), routing p99 0.70–0.81 ms
    (0.8; case 7 bulldoze 0.76 ms). Case 8: identical state hash at 1/4/8 threads ✓.
  - Despawn gate ✓ at V/C ≤ 1.0: city at 1 500 agents (0 links, 20/2 915 nodes with peak V/C > 1)
    0.67 per 1 000 trips; cases 1–5 0. At 5 000 agents the synthetic grid is oversaturated
    (437–460 nodes > 1): 18.5–21 per 1 000 as the release valve; 8 000: 893 nodes > 1, 58 per
    1 000, step 0.99 ms, junctions 0.53 ms; 12 000: 1 225 nodes > 1, 103 per 1 000, step 1.15 ms,
    junctions 0.74 ms, routing 1.01 ms.
  - Cache (reported; target ≥ 80 %): revalidate policy 12 % overall, 18.7 % day 2 (epoch-only
    0.6 % with 15-min epochs). The harness draws a random household per trip, so only 26 % of
    requests repeat a key; 46 % of repeats hit, the rest fail the 115 % re-cost (other time of
    day); no LRU evictions. Re-measure with daily commuters in M5/M9.
  - Changes (ADR-0066/0067): exact parallel phases (IDM + flags, apply, junction prepass with
    same-node re-evaluation) took the step from 1.45–1.71 to 0.58 ms; router CSR + interleaved
    landmarks, 8 resumable search slots × 1 200 settled nodes per tick (one settled node
    ≈ 130 ns, so ADR-0048's 50 000-unit budget was ≈ 6.6 ms), async reroutes, cached lane plans;
    15-min epochs (5k despawns 31 → 8.9 per 1 000 vs 1-h epochs on 0.4 day); gap check: a
    priority vehicle at the line wanting the same target lane always blocks (ring 512 → 1 572
    veh/sim-h, saturated ring 46 → 0 despawns per 1 000); ring metering incl. connectors and
    granted entries at 70 %; equal-priority greedy release (1 222 → 1 846 veh/sim-h); reroute
    plans admit the current lane; wait-for graph on raw slow time.
  - Open: permissive signal lefts (protected always until M9 test), cache hit rate with real
    commuters, junction p95 at 8k+ (next step: parallel grants per node), real-CPU timing on the
    min-spec PC (UV).

### M2 — Vertical slice (estimate 12; must look good before moving on)
Includes enabling work: command framework (ADR-0055), economy stub (money + construction ledger)
and catalog for roads/zones, UI foundation (theme, widgets with structural tooltips, dialogs,
toasts, minimal toolbar: road, plot, bulldoze), dev settings panel, citygen (500 houses) +
`--benchmark` harness, instant-growth stub (`SimConfig::growth_mode = Debug`: empty lots grow
L1–L3 after N ticks), static `LightingState` + keyframes, no-op shader uniforms for overlay/
night/wetness/snow, chunk merging + slot buffer from the start, runtime render-scale change,
worldgen constraints, input map v3.
- [x] Worldgen: `worldgen_features_seed_1_to_20` (rivers ∈ [1, 2], lakes ∈ [1, 3], buildable
      lowland ≥ 3 km², shore within 400 m of an outside connection, generation ≤ 4 s);
      `terrain_generation_deterministic`; `terrain_hash_cross_os` in the cross-OS job.
      — tests green; `determinism_probe` prints terrain hashes (+ `m2_scene` hash).
- [x] Terrain rendering: semantic-pass crack check at LOD boundaries (0 background pixels inside
      the terrain silhouette in M2-14).
      — implemented as `crack_check` shots (magenta background, no sky dome; `holes` metric =
      magenta enclosed by geometry in its column): M2-14b and the whole-map M2-14c, holes 0.
      The full flat-colour semantic pass arrives with its first class assertions (M5/M8).
- [x] Scatter: `scatter_excluded_by_corridor_plus_2m`, `scatter_restored_after_stamp_removed`,
      `tree_density_field_matches_exclusion`.
- [x] Camera: `camera_pan_rotation_relative`, `camera_zoom_clamped_25_2400`,
      `camera_q_tap_snaps_to_next_90`, `camera_q_hold_rotates_continuously`,
      `camera_focus_clamped_to_bounds`, `camera_never_below_terrain_plus_6m`,
      `input_tool_drag_precedence`, `scroll_unit_policy_pixel_pans_line_zooms`,
      `primary_modifier_maps_per_os`.
- [x] Road tool: straight/curved/freehand; each validation reason has a test returning its
      `ValidationError` key (`road_invalid_too_steep`, `_too_sharp`, `_overlap`, `_too_short`,
      `_water_no_bridge`, `_crossing_clearance`); `profile_solver_*` (incl. infeasible endpoints),
      `grading_continuous_compositing`, `crossing_fixed_point_converges`.
      — all green; tool previews call `preview_road` (`road_preview_matches_build`).
- [x] Plot tool (residential): `subdivide_random_10k_invariants`, `building_footprint_within_lot_10k`,
      `lot_ids_stable_under_small_drags`, `lot_ids_n_plus_minus_one_keep_n_minus_2`,
      `subdivide_fan_le_25_or_tight_curve`, `subdivide_corner_lot_fronts_both_roads`,
      `plot_clipped_by_new_corridor`, procgen golden hashes (S-P metrics and the six frontage
      families of `spikes/s_p` as fixtures; ADR-0064).
      — all green; golden hash `subdivision_golden_hash_six_families` (fixture
      `cb_geometry::subdivide::reference_families_hash`, also in the cross-OS probe); house
      golden hashes `procgen_golden_hashes`; plot tool UI: drag, end/depth handles, ± lots,
      depth ±, delete, `EditPlot` (`edit_plot_keeps_lot_ids_and_previews_agree`).
- [x] Lighting/art: calibration metrics of ADR-0050 on M2-02 (noon) and M2-11 (night).
      — lit grass ΔE00 3.1 to #8DB06A; noon mean/median luminance 0.32/0.38 (range amended for
      grass-dominated frames); night/noon mean 0.31; clipped ≤ 0.01 %; zone ΔE n/a (one zone).
- [x] Perf: container `--benchmark m2_slice --no-gpu-work`: game sets (input + sim-drive +
      view-sync + present) ≤ 4 ms p95, `empty` baseline recorded (ADR-0058 amendment, user
      decision 2026-10-16). ☑ᵤ: main and render thread ≤ 8 ms p95 (CPU), GPU ≤ 12 ms p95 at 1080p
      on PC and at Auto on M4.
      — 2026-10-16 release, llvmpipe, seed 1234: game sets p50 1.18 / p95 3.90 ms; `empty`
      baseline game sets 0.82 / 2.90, main 6.9 / 11.4 ms; m2_slice main 8.9 / 14.4 ms
      (pre-update 3.0 / 6.4, post-update 4.2 / 7.0). Full frame: UV-3 (☑ᵤ).
- [x] Shots (suite M2, seed 1234; scripted: curved street on an 8 % slope with fill embankment,
      straight street in a 3 m cut, a street to a river shore, residential plots on convex,
      concave and corner frontage, ~60 houses; noon clear unless noted):

  | ID | Camera | Must be visible |
  |---|---|---|
  | M2-01 | overview d 2 000, pitch 60°, yaw 0 | ≥ 1 river, ≥ 1 lake, hills with rock on steep slopes, forest masses, sand banks; no chunk seams; distant fog without fog over the town |
  | M2-02 | neighbourhood d 300, pitch 45°, zoning view on | curved street, three plots with lot lines, houses facing the frontage |
  | M2-03/04/05 | d 120, pitch 35°, yaw 0/120/240, same block | backyards (fence, shed, garden, driveway); good from all sides; no house crossing a lot line; varied roofs |
  | M2-06 | fill embankment d 40, pitch 20° | smooth embankment, no terrain through the road, skirts invisible, a house on a plinth or stilts |
  | M2-07 | cut d 40, pitch 20° | cut face with dirt/rock material; clean road edges |
  | M2-08 | shore d 80, pitch 30° | depth gradient, foam band, sky fresnel, sand |
  | M2-09/10/11 | M2-02 framing at 08:30, 19:00, 23:00 (debug preset) | long soft morning shadows; warm sunset; blue night with ambient floor, houses readable |
  | M2-12 | road tool d 150, UI on | invalid ghost (striped coral) with "too steep" label and live cost |
  | M2-13 | plot tool editing d 120, UI on | vertex handles, depth handle, sub-plot ± control, subdivision lines |
  | M2-14 | LOD band d 360 (crossfade zone) | no holes or popping artifacts |

  — `docs/screenshots/M2/index.md`: 16 shots (M2-14b/c crack checks added), all gates green,
  reviewed. Deviations: the shore street ends at the lake next to the town (the river is outside
  the site; M2-08 shows the lake shore); M2-01 is framed on the map centre (d 3 600 m) so the
  river is in frame; cut 6.1 m and fill 6.7 m are deeper than the nominal 3 m.
- Enabling work: done — command framework, economy stub + catalog, toolbar/tooltips/toasts,
  citygen + `--benchmark` (with per-set timings and the `empty` baseline), Debug growth,
  `LightingState` + keyframes, worldgen constraints, far-terrain merging, off-thread meshing.
  **Moved** to the milestone that first needs them (internal work, no brief feature cut):
  dialogs → M3 (bulldoze dialog M3-12); building chunk merging, slot buffer and the 500/10k-house
  citygen → M4 (condition tint, 10k benchmark); runtime render-scale change → M15 (settings);
  no-op shader uniforms for overlay/night/wetness/snow → added with those features (M5, M11,
  M12); input map v3 actions beyond camera and tools → with their tools (M3+).

### M3 — Full road system (estimate 12)
Includes enabling work: `cb_sim::save::snapshot` types (for undo), bus/tram cross-section
variants (UI hidden until M10) and tram connector radii, signal phase-plan generation with a stub
phase clock for signal heads, ray–deck picking, LaneGraph `DebugView`, progression default "all
unlocked" until M7.
- [x] Hierarchy + upgrade/downgrade: `upgrade_street_to_avenue_rebuilds_lanes`,
      `upgrade_clips_plots_and_prices_demolition`, `downgrade_refund_zero`.
- [x] Lanes: `lanes_count_offsets_per_cross_section` (5 types × one-way × bus/tram variants).
- [x] Junctions (S-J metrics as tests): `junction_random_10k_valid_or_typed_error`,
      `junction_connectors_cover_all_allowed_movements`, `junction_surface_continuous_8pct`,
      `ring_detected_as_roundabout`.
- [x] Controls: `light_plan_no_conflicting_greens_{t,x,y,5way,6way}`,
      `every_movement_green_in_some_phase`, `approach_control_persists_in_snapshot`.
- [x] Bridges: `bridge_cost_monotonic_in_length`, `bridge_piers_outside_corridors`,
      `bridge_abutment_step_le_2cm`, `overpass_does_not_modify_lower_corridor`,
      `bridge_classification_hysteresis` (plus `river_bridge_spans_water_with_clearance_and_piers`,
      `no_junction_on_a_bridge`; synthetic `TerrainSetup::Valley`).
- [x] Decorations: `decor_spacing_within_5pct_on_curves`, `decor_height_matches_profile_1cm`
      (plus `segment_style_costs_decorations_and_slows_cobblestone`).
- [x] Editing: `edit_road_geometry_regenerates_junctions`, `edit_road_geometry_repoints_dependents`.
- [x] Undo/redo: per command variant `undo_<cmd>_restores_state_hash`, `redo_<cmd>_equals_apply`;
      `undo_depth_limit_100`, `undo_blocked_reports_reason`, `undo_refund_decays`,
      `undo_after_window_rejected`, `replay_command_log_reproduces_hash`
      (`crates/cb_sim/tests/undo.rs`).
- [x] Bulldoze: `demolish_validate_lists_dependents`; dialog shot M3-12 (three choices, dependent
      plots outlined).
- [x] Road tools in the game (added in M3.8 — the commands above had no UI): road tool
      roundabout preset (`BuildRoundabout`, atomic, one undo step:
      `build_roundabout_command_makes_entries_and_undoes_in_one_step`,
      `build_roundabout_rolls_back_when_an_arc_fails`); Edit road tool (U) — upgrade/downgrade,
      decorate, junction control, move node — with sim previews (shots M3-17, M3-18).
- [x] Markings follow control (`markings_follow_control_and_ring_roles`; ring entries with
      give-way lines and splitter islands, nothing on the ring).
- [x] Shots (noon clear; d in m; pitch 40° unless noted):

  | ID | Subject | Must be visible |
  |---|---|---|
  | M3-01 | T street×street d 60 | cut-back corners, fillets, crosswalks, stop lines behind crosswalks, lane arrows |
  | M3-02 | X avenue×street with lights d 80 | median opening, signal heads, turn arrows |
  | M3-03 | Y at 30° d 60 | no overlapping meshes, clean acute corner |
  | M3-04 | 5-way d 80 | simple polygon, crosswalks on all arms |
  | M3-05 | junction on 8 % slope d 50, pitch 18°, yaw 90° (houses blocked lower views) | no steps between approaches and junction surface |
  | M3-06 | river bridge with 3 piers d 200, pitch 25° | deck, railings, piers in water, abutments meeting embankments |
  | M3-07 | valley bridge, piers > 25 m, d 250 | tapered tall piers, smooth deck profile |
  | M3-08 | overpass over a street d 80, pitch 20° | lower road unaffected, clearance visible |
  | M3-09 | decorated avenue (tree rows, lamps, grass strip, cobblestone) d 50 | even spacing on a curve; lamps upright on slope |
  | M3-10 | lanes/connectors/conflicts debug overlay on the 5-way d 60, UI on | all connectors drawn, zones distinct |
  | M3-11 | one-way pair + dirt path d 150, pitch 55° | arrows; dirt path material |
  | M3-12 | bulldoze dialog, UI on | three choices, dependents highlighted |
  | M3-13/14/15 | M3-02 at yaw 0/120/240 | consistent from all sides |
  | M3-16 | roundabout d 80 | ring, yield markings on entries, splitter islands |
  | M3-17 | edit tool, upgrade a street to avenue, UI on | segment highlight, cost label, mode bar |
  | M3-18 | road tool, roundabout preset r 35, UI on | ring ghost, radius and cost label |

### M4 — All zone types, special buildings, leveling visuals (estimate 10)
Includes: citygen 10k scene, `DebugCommand::{SetBuildingLevel, SetCondition}`, free decoration
tool (`PlaceProp`), town hall auto-placement, generator set A (police, fire, clinic, school,
diesel, coal, pump, outlet, landfill); set B may land with its system (M6/M10).
- [x] `procgen_{r,c,i}_levels_within_lot_10k`, `procgen_triangle_budget_per_type`,
      `facade_no_window_crosses_edge` (`cb_procgen/tests/zoned.rs`).
- [x] Silhouettes: `silhouette_iou_between_service_types_le_0_6` (LOD1 masks, 4 yaws, 128²) and
      vs zoned buildings of the same footprint ≤ 0.6 (`silhouette_iou_vs_zoned_same_footprint_le_0_6`;
      worst mean 0.505 between kinds, 0.598 vs zoned over 8 seeds).
- [x] Placement: `place_building_snaps_to_road_edge_within_2m`, `place_building_rejects_overlap`,
      `place_prop_lv_effect` (`cb_sim/tests/buildings.rs`).
- [x] Condition: `condition_change_does_not_remesh` (counter) and
      `construction_reveal_rises_with_progress` (`cb_game::view`).
- [x] Plot tool, freehand (user request 2026-10-16; ALGORITHMS §9): **polygon mode** (click
      vertices; edges within 1.5 m of a road edge snap and become frontage) and **full editing**
      (drag any vertex or edge; frontage vertices stay on the road edge; depth handle and ± lots
      as in M2). Plots store a free outline besides frontage spans (DATA_MODEL). Tests:
      `plot_polygon_mode_snaps_frontage_1_5m`, `plot_polygon_mode_rejects_no_frontage`,
      `plot_vertex_drag_keeps_lot_ids`, `plot_edge_drag_keeps_lot_ids`,
      `plot_freehand_validation_keys` (self-intersection, area, overlap); shot M4-0x plot tool
      with a hand-drawn plot and its handles. Lot lines stay automatic (± count only; user
      decision 2026-10-16). Evidence: the five tests in `cb_sim/tests/plots_freehand.rs`; shot
      M4-11 (polygon mode, L plot with vertex and edge handles).
- [x] Gallery: `--procgen-gallery --smoke-test 120` exits 0 (`frames=120 validation_errors=0`).
- [x] Perf: container CPU budgets (PERFORMANCE §3) with citygen 10k under `--no-gpu-work`; memory
      within PERFORMANCE §8; ☑ᵤ GPU ≤ 12 ms p95 PC, ≤ 13 ms p95 M4. Evidence: game sets p95
      `m4_city10k` 2.57 ms, `bench_city` 1.92 ms, `empty` 1.33 ms (≤ 4 ms); peak RSS of the city10k
      smoke under lavapipe 2.63 GB (CPU + software-GPU memory together; unified budget 3.5 GB),
      headless 10k sim 61 MB; `headless_bench --city 10k` tick p95 0.035 ms; GPU → UV-5.
- [x] Shots: suites `M4`, `M4gallery`, `M4city` reviewed in `docs/screenshots/M4/index.md`
      (24 shots, gates clean).
  Shot list:

  | ID | Subject | Must be visible |
  |---|---|---|
  | M4-01 | R L1–L5 row d 120, yaw 30° | cottage → apartment block typology progression |
  | M4-02 | C L1–L3 + I L1–L3 rows d 150 | cool vs muted palettes; glass fronts, signs; halls, tanks, chimneys |
  | M4-03/04 | all set-A services/utilities lineup d 200, yaw 0 / 180 | distinct silhouettes from both sides |
  | M4-05 | construction at 50 % | scaffold + vertical reveal |
  | M4-06 | active / declining / abandoned / ruined side by side | visibly run-down progression |
  | M4-07 | procgen gallery grid, UI on | random lots × types × levels, vehicles per type |
  | M4-08 | 10k city overview d 2 000, pitch 60° | dense city, LOD2 readable, no popping |
  | M4-09 | 10k city mid d 400 | zones distinguishable at a glance |
  | M4-10 | angled and L-shaped lots d 80 | footprints follow lot shapes |

### M5 — Simulation core (estimate 12)
Includes enabling work: link graph + router + cache + batching + macro flow model (aggregate
commute model), goods economy, save core per ADR-0044 (format v0) incl. placeholders for traffic
state, happiness stubs, `SetSimConfig` + dev panel, cross-OS determinism job becomes blocking.
- [x] Calendar: `calendar_day_is_28800_ticks`, `season_changes_every_4_days`
      (`cb_sim/tests/calendar.rs`).
- [ ] Growth: `growth_scenario_flat_3days` — pop ≥ 400 with 120 R lots + power + water;
      unemployment ≤ 15 %; morning departure histogram peaks 05:30–07:30.
- [x] Demand: `demand_monotonic_responses`, `demand_breakdown_sums_to_value` (`cb_sim::demand`).
- [ ] Economy: `ledger_sum_equals_money_delta`, `bankruptcy_after_7_days_triggers_loan`,
      `goods_units_conserved`, `deliveries_prefer_nearest_supplier`, `import_when_no_supplier`.
- [ ] Utilities: `power_brownout_outer_first`, `pump_requires_depth_1_5m_within_15m`,
      `disconnected_component_unserved`, `water_pollution_advects_downstream`.
- [ ] Fields/happiness: `pollution_decays_0_97_per_hour`, `noise_line_and_point_attenuation`,
      `happiness_factors_weights_sum_100`, `happiness_stub_replacement_keeps_formula`,
      `fields_double_buffer_no_seams`.
- [ ] Routing (macro): `astar_equals_dijkstra_random_graphs`, `alt_heuristic_admissible_with_variants`,
      `route_cache_invalidation_on_removed_links`, `routing_budget_deterministic`,
      `one_way_trap_flagged`.
- [ ] Save: `save_roundtrip_hash_equal`, `save_load_continue_equals_continuous`,
      `save_corrupt_returns_error`, `save_fuzz_no_panic`, `migration_v0_to_v1_synthetic`,
      `struct_map_missing_field_defaults`.
- [ ] Determinism: `same_seed_same_commands_same_hash_daily`, `results_identical_1_4_8_threads`,
      cross-OS job green.
- [ ] Perf: `headless_bench --city 50k --no-traffic` tick p95 ≤ 1.0 ms, p99.9 ≤ 2.5 ms;
      `phase_table_nominal_le_0_6ms_per_tick`.

### M6 — Services (estimate 6)
Includes: `VehicleStore` + ghost kinematics + pooled vehicle view (instanced, α interpolation) so
service vehicles are visible; fire/smoke VFX; set-B service generators as needed.
- [ ] `coverage_river_without_bridge_uncovered`, `coverage_capacity_split_proportional`,
      `coverage_incremental_reach_equals_full`.
- [ ] `fire_spreads_within_6m`, `fire_resolved_when_truck_arrives`, `unresolved_fire_ruins_building`,
      `fire_contained_with_station_within_reach`, `crime_rate_drops_with_coverage`,
      `students_advance_one_level_per_season_with_capacity`, `ghost_vehicle_eta_matches_flow_model`.
- [ ] Shots: M6-01 burning building with smoke column d 80 (day) — flames, smoke, nearby fire truck;
      M6-02 same at 23:00 (debug preset) — fire glow readable at night; M6-03 ruined building;
      M6-04 police coverage debug overlay d 600 — coverage follows roads, not circles.

### M7 — Progression (estimate 3)
- [ ] `progression_unlocks_at_thresholds` (table-driven from `progression.ron`),
      `every_unlockable_in_exactly_one_tier`, `sandbox_money_unlimited_all_unlocked` (`--sandbox`),
      `locked_tool_shows_unlock_condition` (UI anchor test).
- [ ] Shots: M7-01 milestone toast with unlock list (UI on); M7-02 locked toolbar entry tooltip.

### M8 — UI, overlays, info views (estimate 10)
Includes: `ui::anchors` registry, CVD palette sets + switch (dev panel), high-contrast theme,
`UiIntent::PlaySound` hooks.
- [ ] `ui_no_string_literals`, `ui_tooltips_structural` (no raw egui buttons in `ui/`),
      `l10n_keys_complete_en_de`, `ui_german_no_overflow_at_scale_1_and_1_5`.
- [ ] `palette_cvd_separation` (Machado 2009 for deuteranopia/protanopia/tritanopia: good vs bad
      ΔE00 ≥ 20; sequential adjacent steps ΔE00 ≥ 5).
- [ ] `icons_cluster_when_gt3_in_48px`, info panel for building/road/vehicle placeholder.
- [ ] Shots: M8-01 full HUD noon UI 1.0 (en); M8-02 same in German; M8-03 UI scale 1.5; M8-04
      high-contrast theme; M8-05…M8-17 one per overlay (power, water, sewage, garbage, police, fire,
      health, education, land value, pollution, noise, happiness, slope, traffic from flow model)
      d 900, pitch 60°, legend visible; M8-18 land value in deuteranopia mode with contour/hatch
      patterns; M8-19 problem-icon cluster; M8-20 budget panel with graphs.

### M9 — Traffic (estimate 12)
Includes: agent state saved, garbage trucks replace analytic collection, vehicle info panel.
- [ ] Spawn counters in `traffic_multi_day` > 0 for cars, delivery trucks, service vehicles,
      garbage trucks.
- [ ] `idm_no_negative_speed_no_overlap_1e6_steps`, `junction_no_conflicting_occupancy`,
      `no_vehicle_stops_inside_conflict_zone`, `equal_priority_4way_all_occupied_no_idle_gt_2s`,
      `lane_plan_feasible_on_short_links`, `spawn_queue_never_overlaps`,
      `idm_saturation_flow_matches_capacity`.
- [ ] Feedback: `congestion_increases_commute_estimate`, `dispatch_eta_increases_with_congestion`,
      `garbage_trucks_match_analytic_within_10pct`.
- [ ] Disabled: `same_city_traffic_off_runs_10_days` (population within ±15 % of traffic-on, no
      NaN costs).
- [ ] Gridlock: `gridlock_cycle_detected_and_broken`, `stuck_despawn_logged`.
- [ ] Save: `save_load_continue_equals_continuous_with_traffic`.
- [ ] Perf: `headless_bench --city 50k`: vehicle step p95 ≤ 0.7 ms, routing p99 ≤ 0.8 ms, tick
      p99 ≤ 3.0 ms (4 workers); soak `traffic_multi_day_large`: 10 game days, despawns < 1/1 000
      trips, perceived commute p50 ∈ [12, 30] min and p95 < 60 min, population +20 %, no panic,
      identical hash for 1 and 4 threads. ☑ᵤ GPU benchmark.
- [ ] Shots: M9-01 signalized avenue junction at 07:30 peak d 60 (queues at red, platoon on green;
      semantic vehicle pixels ≥ 1.5 %); M9-02 unsignalized T d 40 (minor car waiting for a gap);
      M9-03 lane change on an avenue d 50; M9-04 congestion overlay d 1 200 with chevrons; M9-05
      follow camera on a truck (UI on); M9-06 pedestrians at a crosswalk d 25; M9-07 M9-01 framing
      at 03:00 (sparse traffic).

### M10 — Public transport (estimate 8)
- [ ] `bus_line_ridership_counts_match_boardings`, `line_cost_equals_vehicle_hours_times_rate`,
      `transit_routing_transfers`, `stop_overflow_rechooses_mode`.
- [ ] `tram_articulated_sections_stay_on_track_1cm`, `tram_turn_radius_ge_20m`.
- [ ] `transit_line_reduces_car_trips` (car mode share −5 pp in catchment; LV up within 400 m).
- [ ] Shots: M10-01 bus at a stop with waiting pedestrians d 30; M10-02 bus lane d 50; M10-03 tram
      on avenue median turning at a junction d 60; M10-04 tram depot d 120; M10-05 line manager UI;
      M10-06 transit overlay with line colors d 1 000.

### M11 — Day/night cycle (estimate 5)
- [ ] `lighting_state_matches_keyframes_at_key_times` (ΔE ≤ 1), `lock_time_freezes_lighting_only`,
      `window_lighting_hash_stable_across_runs` (lit fraction at 23:00 ∈ [0.25, 0.55] for R),
      night metrics of ADR-0050, `peak_hour_agents_ge_3x_02h`,
      `commercial_customers_10_to_19_ge_70pct`.
- [ ] Shots: M11-01…08 same framing (d 400, pitch 45°) at 05:00, 06:30, 08:30, 12:00, 16:30,
      19:00, 20:00, 23:00; M11-09 night street d 30 (windows, lamp pools, headlights, tail lights);
      M11-10 night tram stop d 30 (lit interior and stop); M11-11 night overview d 1 800 (city
      readable, zones distinguishable).

### M12 — Weather (estimate 7)
- [ ] `weather_markov_matches_season_table_1000_days` (χ² p > 0.01), `forecast_matches_realized_48h`,
      `rain_reduces_v0_by_15pct`, `snow_upkeep_up_to_60pct`, `lightning_ignites_30pct_unless_covered`,
      `solar_wind_factors`, `lightning_flash_le_120ms_and_capped_when_reduced`.
- [ ] Shots (d 300, pitch 45°): M12-01…06 clear, cloudy, rain, thunderstorm (with lightning), fog,
      snow at 12:00; M12-07 rain 23:00; M12-08 snow 23:00; M12-09…12 spring/summer/autumn/winter
      noon clear (foliage, grass tint); M12-13 wet road close-up d 25 (darkening, puddles); M12-14
      snow close-up d 40 (roofs, trees, roads partly cleared); M12-15 rain with reduce-effects on.

### M13 — Audio (estimate 5)
- [ ] `audio_loudness_targets`, `audio_loops_seamless`, `audio_no_clipping_no_dc_no_nan`,
      `audio_spectral_centroid_ranges`, `ambient_mixer_weights_deterministic` (±0.05 for given
      camera/weather states), `no_audio_device_runs_headless`.
- [ ] Spectrogram PNGs in `docs/screenshots/M13/` reviewed; listening pass ☑ᵤ. Music not required.

### M14 — Tutorial (estimate 4)
- [ ] `tutorial_scripted_run_completes` (synthetic input drives all chapter-1 steps in a windowless
      app with the UI context), `hints_shown_once_per_tool`, `reset_hints_works`; replay from the
      pause menu and `--tutorial`.
- [ ] Shots: M14-01…06 one per chapter-1 step highlight (UI on).

### M15 — Menus, settings, accessibility, save/load, localization (estimate 8)
- [ ] `menu_new_game_with_seed_starts_ingame`, `menu_load_restores_hash`,
      `keymap_conflict_detected_with_primary_per_os`, `autosave_rotates_3_slots`,
      `autosave_hitch_le_10ms_snapshot` (bench), `settings_ui_covers_all_fields`.
- [ ] German: 0 Fluent fallback lookups in a scripted UI walkthrough; overflow test green.
- [ ] Save format v1 frozen with golden fixture; migration policy active from here.
- [ ] Shots: M15-01 main menu (en); M15-02 main menu (de); M15-03 settings; M15-04 load dialog with
      metadata; M15-05 overlay in tritanopia mode; M15-06 UI scale 2.0 + high contrast.

### M16 — Polish, performance, bug fixing (estimate 8)
- [ ] `bench_city` 60 s path: p95 per thread ≤ PERFORMANCE §3–5 tables and p99.9 frame ≤ 33 ms on
      PC and M4 (☑ᵤ) plus container CPU numbers.
- [ ] 2-hour scripted soak at 4×: 0 panics, memory growth ≤ 5 %.
- [ ] Union of all screenshot suites re-rendered, metric gates green, diff list vs previous
      milestones in `docs/screenshots/M16/index.md`.
- [ ] Known bugs: 0 crashers, 0 gameplay blockers; balance targets (GAME_DESIGN §14) pass.

---

## User verification queue (batched, non-blocking)

| ID | Milestone | Request | Exact command / action | Expected | Status |
|---|---|---|---|---|---|
| UV-0 | M0 | Identify hardware | PC: Ryzen 5 7500F, GTX 1660 Ti, 16 GB single-channel; Mac: MacBook Pro 14" M4 base, 16 GB | — | answered |
| UV-1 | M1 | Real-GPU smoke | `cargo run -p cb_game --release -- --smoke-test 300` on Windows and on the M4 (also look at the window title and, after `cargo run -p cb_game --features dev`, the F1/F2 windows) | exit 0, prints `frames=300 validation_errors=0` | **open — please run when convenient** |
| UV-2 | M2 | Screenshots on real GPUs | `scripts/screenshot.sh M2` (or `cb_game --screenshot-suite M2 --out shots/`) on PC and M4 | images comparable to `docs/screenshots/M2/` | planned |
| UV-3 | M2 | Benchmark | `cargo run -p cb_game --release -- --benchmark m2_slice` and `-- --benchmark empty` on PC and M4; paste both reports (also in the logs folder as `benchmark-*.txt`) | main ≤ 8 ms p95, frame ≤ 16.7 ms p95 (GPU ≤ 12 ms) | planned |
| UV-4 | M2 | Mac input | trackpad pinch, two-finger scroll/rotate, ⌥-drag, ⌘Z/⌘⇧Z, ⌘Q save prompt | as ADR-0052 | planned |
| UV-10 | M3 | Road tools on real GPUs | `cb_game --screenshot-suite M3 --out shots/` on PC and M4; then in a new game: Edit road (U) → Upgrade a street to an avenue, Junction → Traffic lights, Decorate with trees/lamps, Move node; Road → Roundabout | images comparable to `docs/screenshots/M3/`; tools behave as in the shots; signal lamps cycle | planned |
| UV-5 | M4 | 10k benchmark | `cargo run -p cb_game --release -- --benchmark m4_city10k` on PC and M4; paste the report | frame ≤ 16.7 ms p95, GPU ≤ 12 ms p95 PC / ≤ 13 ms M4 | **open — please run when convenient** |
| UV-11 | M4 | Buildings on real GPUs | `scripts/screenshot.sh M4`, `M4gallery` and `M4city` (or `cb_game --screenshot-suite <suite> --out shots/`) on PC and M4; in a new game try Buildings (P) and Decorate (O) and the plot tool's Outline mode (Tab) | images comparable to `docs/screenshots/M4/`; no popping when zooming over the 10k city | planned |
| UV-6 | M9 | Traffic benchmark | `--benchmark bench_city` | PERFORMANCE §3–5 | planned |
| UV-7 | M11 | Night look | `--screenshot-suite M11` on PC/M4 | night readable | planned |
| UV-8 | M13 | Listening pass | play 10 min with ambient/UI/sirens; note anything unpleasant | — | planned |
| UV-9 | M16 | Final performance | `--benchmark bench_city` warm (10 min) | PERFORMANCE §1 | planned |

## Known bugs

None open.

Fixed:
- 2026-10-17 — Container CPU budget over 4 ms p95 (open since M3: `bench_city` 3.67–4.81 ms,
  `view:terrain` p95 1.8–3.1 ms). The per-chunk distance/LOD pass now reruns only when the eye
  moves > 8 m (`view::TERRAIN_EYE_STEP_M`) and settles early; buildings iterate only the
  constructing set. Now `bench_city` 1.92 ms, `m4_city10k` 2.57 ms, `empty` 1.33 ms.
- 2026-10-17 — Construction scaffold sank with the reveal height (it shared the building's
  object slot); it now uses the neutral slot (found in the M4-05 review).
- 2026-10-16 — Junction asphalt shading fan (M2): large, slightly non-planar ear-clipped
  triangles under flat (derivative) normals. Now `junction::carriageway_triangles` (centre fan +
  zippered rings with mean-value heights), shared with the terrain stamp; this also removed a
  sliver hole from ear clipping (test `carriageway_triangles_tile_the_polygon`).
- 2026-10-16 — Crossing exactly on a sampled-centreline vertex counted twice (two nodes on top of
  each other, junction rejected as too acute): `crossing_on_a_polyline_vertex_found_once`.
- 2026-10-16 — Ground clipping through junctions (reported by the user on M2-04; also M2-03,
  M2-13): terrain under a junction came only from the segment corridors, up to 0.16 m above the
  junction surface. Fixed with junction surface stamps (ADR-0065 amendment); regression test
  `junction_surface_above_terrain` (worst now 0.02 m below).

Known gaps (planned):
- Settings `window_mode`, `frame_cap`, `render_scale`, `preset`, audio and accessibility values
  are stored and validated but not applied yet (vsync and size are): S-Render/M2 (ADR-0053),
  M13 (audio), M15 (accessibility).
- "Auto" language reads `LC_ALL`/`LC_MESSAGES`/`LANG` only; Windows and Finder-launched macOS
  apps fall back to English unless chosen in settings (OS API lookup in M15; ADR-0021 amendment).
- `--scene` is parsed but there is one scene (the M1 placeholder); the smoke scene with every
  material/specialization starts with S-Render/M2.
- `trace_tracy` nightly build is not wired (feature does not exist yet).

## Scope-cut ladder (apply in order when a milestone exceeds 1.5× its estimate; brief features are never cut without asking)

1. Generative music (brief: optional).
2. Ultra extras: real point lights for nearest lamps, TAA option.
3. Rain splashes on roads (puddle darkening stays).
4. Roundabout preset tool (manually drawn rings still work).
5. Articulated trams → single-body trams.
6. Landmark buildings (tiers 7–8 grant decorations instead).
7. Set-B signature generators reuse generic massing with signature details.
8. Anything beyond this → ask the user.

## Log

- 2026-10-15 — Phase 1 planning package written; Bevy 0.20.0 API surface verified from crate
  sources.
- 2026-10-15 — Skills (milestone-completion, write-adr, visual-verification); macOS on Apple
  Silicon made a required platform (ADR-0037, 0040; CI/input/DPI ADRs later superseded).
- 2026-10-15 — Plan approved. User decisions: diorama pace OK; PC = minimum spec (Ryzen 5
  7500F, GTX 1660 Ti, 16 GB single-channel DDR5-6000); Mac model to confirm (assumed MBA 13" M4);
  proprietary licence; Inter font (OFL) allowed; macOS CI on main/nightly only (+ one dispatch per
  milestone); lock time-of-day = lighting only. ADR-0059, ADR-0060.
- 2026-10-15 — Independent review by a separate agent (≈ 310 findings,
  `docs/review/2026-10-15-independent-review.md`); resolutions in `docs/PLAN_REVIEW.md`;
  ADRs 0042–0058 supersede/extend 15 ADRs; PERFORMANCE, ALGORITHMS, GAME_DESIGN, DATA_MODEL,
  ARCHITECTURE and this file rewritten; Phase S defined.
- 2026-10-15 — **M1 done** (estimate 4, actual 1). Workspace (10 crates, edition 2024, Rust
  1.97.1), cb_core, cb_l10n (en/de), cb_sim skeleton, cb_game bootstrap, smoke test, screenshot
  pipeline, dev tools, CI/nightly/release workflows, scripts, macOS bundle script.
  Evidence: fmt + clippy (default, dev) clean; `cargo test --workspace` and `--features dev`
  green (cb_core 24, cb_l10n 7, cb_sim 9, cb_game 24 incl. crash/logging integration tests);
  sim isolation ok; lavapipe smoke `frames=60 validation_errors=0` (debug and release);
  determinism probe hashes (Linux): seed 1 `b78b23a980070796`, seed 2 `6754ae5a029997cc`,
  seed 3 `2fcce984cb3a62f8`; headless bench (empty sim) p50 1.4 µs/tick.
  Decisions: ADR-0061 (sim schedules on the single-threaded executor: 70 µs → 1 µs per tick);
  ADR-0021 amendment (Fluent keys use dashes; locale from env), ADR-0022 amendment (subscriber
  installed before Bevy; `LogPlugin` not compiled in). Dev-only/CLI/log strings exempt from l10n
  (CLAUDE.md).
- 2026-10-15 — M1 CI confirmed. `ci.yml` run 2 on `9e558da` green (11 jobs, 32.6 min wall):
  https://github.com/ArvidsGertners/City-Builder/actions/runs/38776258475 — run 1 on `eeafc2a`
  failed only `crash_report_contains_all_sections` on Windows (test expected `src/lib.rs`, Windows
  reports `src\lib.rs`; fixed in the test). Linux/Windows determinism hashes equal.
  `nightly.yml` dispatch on `eeafc2a` green (macOS check × {default, dev}, macOS smoke, 3-OS
  determinism equal, soak, bench; 20.7 min): https://github.com/ArvidsGertners/City-Builder/actions/runs/38771849977
  `release.yml` dispatch (publish: false) green, three artifacts (51 min):
  https://github.com/ArvidsGertners/City-Builder/actions/runs/38771854627
  Known issue: CI wall time 32.6 min > 25 min target; the Windows debug test build dominates
  (~23 min). Plan: revisit caching/profile for Windows tests during S-Render (logged, not blocking).
- 2026-10-15 — User decisions: realistic number of visible cars (→ ADR-0062 amendment: trip scale
  6 kept and calibrated to 4–7 % of the population on the road at peak); Mac = MacBook Pro 14"
  M4 base, 16 GB (PERFORMANCE §1, Auto render scale 3/5).
- 2026-10-15 — S-J done (ADR-0063 supersedes ADR-0045): junction rules validated on 40 000 random
  + 12 canonical junctions; live spacing guide replaces the static table; p99 timing (1.4 ms vs
  0.4 ms) carried into M3.
- 2026-10-15 — S-P done (ADR-0064 supersedes ADR-0014): subdivision + fitting validated on
  10 000 random plots, a 24-plot showcase and 28 598 edits; all gates pass (p99 1.63 ms).
- 2026-10-15 — S-G done (ADR-0065 supersedes ADR-0046): road–terrain interface validated on 270
  roads, 30 sloped junctions, bridges and overpasses with an automated LOD0–3 sampler and 12
  lavapipe shots; constrained LOD decimation replaces min-downsampling.
- 2026-10-15 — S-T done (ADR-0066 supersedes ADR-0047 + the control section of ADR-0063;
  ADR-0067 supersedes ADR-0048): traffic at 5 000 agents meets every timing, safety and
  determinism gate without a fallback; despawn gate met at V/C ≤ 1 (0.67 per 1 000); cache hit
  rate reported only (harness-limited). **Phase S complete**; next M2.
- 2026-10-16 — M2 pure crates: `cb_terrain`, `cb_geometry` (incl. the ADR-0064 port) and
  `cb_network` landed with their tests; ADR-0064 amended (achievable fan bound, densest fallback,
  frontage detection from polylines). Next `cb_procgen`.
- 2026-10-16 — M2.4 `cb_procgen`: residential houses, yards, vegetation and road meshes; a
  software-rasterized preview example shows a curved street with 46 houses (avg 340 tris/house,
  yards ≤ 400). ADR-0049 amended with the packed attribute layout.
- 2026-10-16 — M2.5 `cb_sim` world state: commands, catalog, treasury, terrain resource, roads
  (crossing splits, junction geometry subset of ADR-0063 — connectors in M3), plots/lots with
  stable IDs and re-clipping, Debug growth, citygen, state hash; `cb_network::junction` and
  `cb_procgen::junction_mesh` added.
- 2026-10-16 — M2.6 checkpoint: tools (road/plot/bulldoze), picking, toolbar/HUD, zoning overlay,
  `EditPlot` + previews, scripted M2 features (`citygen::m2_features`: 8 % slope street with
  corner plot, 6.1 m cut, 6.7 m fill, shore street, 36 houses), M2 suite with per-shot state,
  `--benchmark` with per-set timings, lighting calibration (ADR-0050 amendment). Bug found and
  fixed: draped meshes wound CCW in (x, z) face down and were back-face culled (overlay, ghost).
  Main-thread p95 33.7 → 16.8 ms after the rebuild budget; still over 8 ms (Next step).
- 2026-10-16 — Off-thread terrain/water/scatter meshing (`AsyncComputeTaskPool`, `TerrainRes`
  holds `Arc<Terrain>`), far terrain merged into 4×4 super-chunks (1 800 → 1 027 meshes);
  benchmark `m2_slice` main p50 8.9 / p95 14.5 ms, view-sync 0.65 / 2.45 ms; new `empty`
  baseline scene p50 7.0 / p95 11.2 ms (engine pre/post-update dominate in the contended
  container). User-reported junction ground clipping fixed (Known bugs).
- 2026-10-16 — User decisions: (1) container CPU criterion = game sets ≤ 4 ms p95 + `empty`
  baseline, full frame on user hardware (ADR-0058 amendment); (2) freehand plot drawing
  (polygon mode, vertex/edge dragging) scheduled in M4; (3) lot lines stay automatic, the ±
  lot-count control is enough (no manual lot-line editing).
- 2026-10-16 — **M2 done** (CI pending). Evidence: fmt; clippy default + `dev` clean; `cargo
  test --workspace` and `--features dev` green; sim isolation ok; release smoke `frames=60
  validation_errors=0`; determinism probe (Linux): terrain seed 1 `ded815c48a85c936`, seed 2
  `906bb9654b3f2c3f`, seed 3 `46baef7eff10795c`, subdivision `4f1dee6563b7dec7` (pinned),
  m2_scene `0575619db05fa1db` (290 buildings), clock seed 1 `dc63c12f8a8a98b8`; benchmark game
  sets p95 3.90 ms (≤ 4); suite M2 reviewed (`docs/screenshots/M2/`). Fixed during review: corner
  plot lots not grown (`m2_scripted_lots_all_grow`), tonemapped magenta missed by the gate
  (hue-based test), overview/fill framing. Golden subdivision hash pinned.
- 2026-10-16 — M3.1–M3.3: lane kinds and variants; junction connectors with per-kind curvature
  limits, carriageway containment, re-sourced crossings (unresolved → growth) and conflict
  zones (`junction_random_10k_valid_or_typed_error`: 5 269 of 10 000 valid, rest typed errors;
  `junction_connectors_cover_all_allowed_movements`, `junction_surface_continuous_8pct`);
  `LaneGraph` with generational ids (`cb_core::collections::Slots`), U-turns at dead ends,
  lane-by-lane continuations, patched incrementally and equal to a full rebuild
  (`lane_graph_follows_network_edits`, `lane_graph_town_every_lane_enterable_and_leavable`).
  The m2_scene probe hash changes (one junction grows, 289 houses); the 2 rejected demo-town
  roads predate M3.
- 2026-10-16 — M2 CI confirmed on `f36cdef`: `ci.yml` run 30 green
  (https://github.com/ArvidsGertners/City-Builder/actions/runs/38884263345); `nightly.yml`
  dispatch run 3 green — macOS check default + dev, macOS smoke, determinism probe on
  Linux/Windows/macOS with matching hashes, soak, bench
  (https://github.com/ArvidsGertners/City-Builder/actions/runs/38884289448).
- 2026-10-16 — M3.7–M3.8: scripted M3 scene complete (9 features), 18-shot `M3` suite reviewed
  (`docs/screenshots/M3/`). Review fixes: control-aware markings (no line on priority
  approaches, dashed give-way line + shark teeth on yield, solid on stop/signals), roundabout
  nodes without ring markings and with entry give-way lines, set-back crosswalks and raised
  splitter islands; junction fans painted without per-face colour jitter (faint "sunburst");
  M3-05/M3-11 reframed. Gap found and closed: the M3 commands had no game UI — added the road
  tool roundabout preset (`BuildRoundabout`, atomic with rollback, removes road pieces inside the
  ring, one undo step) and the Edit road tool (U: upgrade, decorate, junction control, move node)
  with sim previews. Benchmark report gained per-`ViewStep` timings.
- 2026-10-16 — **M3 done**. Evidence: fmt; clippy default + `dev` clean; `cargo test
  --workspace` and `--features dev` green (36 binaries, 280 tests each); sim isolation ok; release
  smoke `frames=60 validation_errors=0`; determinism probe (Linux): terrain/subdivision/clock
  hashes unchanged from M2, m2_scene `e817d82b5c004d7b` (289 buildings; changes with junction
  growth, markings and control in the hash); benchmark game sets `bench_city` p95 median 4.49 ms
  (3.67–4.81), `m2_slice` 4.07–4.17 ms, `empty` 2.57 ms — over the 4 ms container budget, logged
  in Known bugs with a plan (terrain hand-over, M2 code). Suite M3 metrics: magenta 0, holes 0,
  black 0, clipped ≤ 0.01 %.
- 2026-10-16 — M3 CI confirmed on `02af6b5`: `ci.yml` run 56 green
  (https://github.com/ArvidsGertners/City-Builder/actions/runs/38955953609); `nightly.yml`
  dispatch run 4 green — macOS check default + dev, determinism probe on Linux/Windows/macOS
  with matching hashes (https://github.com/ArvidsGertners/City-Builder/actions/runs/38955970587).
- 2026-10-17 — M4.1–M4.7: zoned typologies R L4–L5, C L1–L3, I L1–L3 with budgets and 10k-lot
  property tests; civic generator set A + town hall (silhouette IoU ≤ 0.6 between kinds and vs
  zoned buildings); zone-aware growth with level caps, construction time and conditions
  (per-object shader slots: tint, condition, reveal height — ADR-0049 amendment); `PlaceBuilding`
  (snap to road edge, overlap/water/slope checks, town hall auto-placement) and `PlaceProp`
  (land-value bonus); freehand plot tool (polygon mode, vertex/edge drags, auto-clip); chunked
  building meshes with LOD0/LOD1 and visibility ranges; citygen 10k (`--scene city10k`,
  benchmark `m4_city10k`); procedural vehicles (`cb_procgen::vehicle`, 12 types) in the gallery.
  Suites `M4`/`M4gallery`/`M4city` reviewed (`docs/screenshots/M4/`); review fixes: scaffold slot,
  stronger condition look, gallery vehicle row and civic terrace fallback.
- 2026-10-17 — **M4 done** (estimate 10, actual ≈ 3). Evidence: fmt; clippy default + `dev` clean;
  `cargo test --workspace` and `--features dev` green; sim isolation ok (290 dependency lines);
  release smoke `frames=60 validation_errors=0`; gallery smoke `frames=120 validation_errors=0`;
  determinism probe (Linux): terrain/subdivision/clock hashes unchanged, m2_scene
  `722aa747f8e1f8c0` (287 buildings; changes with zone-aware growth and the town hall);
  benchmark game sets p95 `m4_city10k` 2.57, `bench_city` 1.92, `empty` 1.33 ms (≤ 4 ms);
  `headless_bench --city 10k`: citygen 14.3 s, 13 777 buildings, tick p95 0.035 ms; peak RSS
  city10k smoke 2.63 GB (lavapipe, CPU + GPU memory), headless 61 MB.
- 2026-10-17 — M4 CI confirmed: `ci.yml` run 66 on `5b280cc` (M4 + calendar tests; run 65 on
  `046357f` was cancelled by the newer push) green — Linux/Windows check default + dev, render
  smoke, sim isolation, determinism compare
  (https://github.com/ArvidsGertners/City-Builder/actions/runs/38987250857); `nightly.yml`
  dispatch run 5 on `046357f` green — macOS check default + dev, macOS smoke, determinism probe on
  Linux/Windows/macOS with matching hashes, soak, bench
  (https://github.com/ArvidsGertners/City-Builder/actions/runs/38987172290).
- 2026-10-17 — M5.2 citizens: `cb_sim::citizens` (population tables, capacities from
  `zones.ron`, immigration in 4 slices from tick 720, job matching in 200 slices from tick 500,
  eviction/lay-offs when homes or workplaces stop being habitable) and `cb_sim::demand` (§15 with
  stubs; immigration uses R without the housing-vacancy term — ALGORITHMS §16). Tests
  `households_move_in_and_find_jobs`, `citizens_deterministic`,
  `abandoned_home_empties_and_closed_workplace_lays_off`. 4×3 mixed town: 6 households/h,
  622 citizens after 44 game hours, unemployment 3–9 %. The state hash now covers population and
  demand (probe hashes change).
