# Testing Strategy

Status: **Accepted (Phase 1 plan, revised after the 2026-10-15 plan review)**.

## 1. Principles

- The hard logic lives in pure crates → fast unit tests (whole pure-crate suite < 30 s).
- The simulation is headless → integration tests drive real `SimPlugins` in a `MinimalPlugins`
  app, no GPU, no window.
- Determinism is a testable property: same seed + same command log ⇒ same state hash — on every
  thread count, with shuffled schedules, across save/load, and **across Windows, Linux and macOS**.
- Randomized tests use `cb_core::rng` with fixed seeds and loop counts plus a tiny shrinker in
  `cb_core::test_util` (halves counts/ranges until the failure disappears); a failing seed is
  printed and pinned as a regression case. (No proptest dependency — ADR-0023.)
- Rendering is checked by shader validation, a smoke test, objective screenshot metrics and a
  semantic pass, plus the agent's own review of every screenshot (ADR-0058).
- Every test name in PROGRESS.md acceptance criteria exists in the code before the milestone is
  marked done.

## 2. Levels

| Level | Where | CI | Typical time |
|---|---|---|---|
| Unit | `#[cfg(test)]` in every crate | yes, Linux/Windows/macOS × {default, `dev`} | seconds |
| Golden/snapshot | procgen hashes, junction geometry snapshots (RON in `tests/snapshots/`) | yes | seconds |
| Platform | paths (ADR-0040) for all OS from injected env; `PRIMARY` mapping; scroll-unit/gesture policy; DPI/render-scale cursor ray (ADR-0053) | yes (pure functions, all OS) | seconds |
| Shader validation | compose every WESL shader and translate to SPIR-V/HLSL/MSL via naga, no GPU (if spike S-Render proves it feasible) | yes, all OS | seconds |
| Integration (headless sim) | `crates/cb_sim/tests/*.rs` via `cb_sim::testing::Scenario` | yes | < 2 min |
| Determinism | §5 | yes (cross-OS job blocking from M5) | < 3 min |
| Soak | `#[ignore]` tests (`traffic_multi_day_large`, 2-hour 4× soak) | nightly + before milestones | 5–30 min |
| Render smoke | `cb_game --smoke-test 60 --resolution 640x360 --scene smoke` (every material/specialization) | Linux blocking, macOS informational | ~3 min |
| Screenshot suites | `scripts/screenshot.sh <M>` → PNG + `metrics.json` + semantic pass | locally per visual milestone | 10–30 min |
| Audio QA | loudness, true peak, loop seams, DC/NaN, spectral centroid; spectrogram PNGs | yes (pure `cb_audio`) | seconds |
| Benchmarks | `headless_bench`, `--benchmark --no-gpu-work` (container), user GPU runs | informational artifact | minutes |
| Real hardware | smoke / screenshots / benchmark / input / listening on the user's PC and M4 | User verification queue (☑ᵤ) | — |

## 3. Scenario harness (cb_sim::testing)

```rust
let mut s = Scenario::new(seed).flat_terrain().money(1_000_000);
let a = s.road(RoadType::Street, [(0.0, 0.0), (300.0, 0.0)]);
let b = s.road_curved(RoadType::Avenue, (300.0, 0.0), (420.0, 60.0), (400.0, 200.0));
s.plot(Zone::Residential, a.side(Side::Left), 20.0..280.0, 30.0 /* depth */);
s.place(Utility::DieselGenerator, near(a, 50.0));
s.place(Utility::WaterPump, s.shore_near(a));
s.run_game_days(3);
assert!(s.population() > 50);
assert_eq!(s.problems(Problem::NoPower), 0);
let h = s.state_hash();   // stable hash over the canonical save snapshot
```
`Scenario` issues real `PlayerCommand`s (so validation is exercised), runs `SimEdit`/`SimTick`
explicitly, and exposes query helpers. `cb_sim::testing::citygen` builds large organic cities
(roads + plots + forced growth) for benchmarks and soak tests.

## 4. Required coverage

| Area | Tests (names in PROGRESS.md acceptance criteria) |
|---|---|
| Geometry edge cases | Bézier split/eval/nearest point vs dense sampling; arc-length accuracy; degenerate curves; offsets at r_min; polygon orientation/area/containment with collinear & duplicate points; self-intersecting input rejected; ear clipping on random polygons; booleans on touching/overlapping/slivers; rect fitting on L/U/angled shapes |
| Plot subdivision | `subdivide_random_10k_invariants`, `lot_ids_stable_under_small_drags`, corner plots, concave/S-curve frontage, tiny plots, N too large, slivers, area conservation ±0.5 % |
| Building fitting | `building_footprint_within_lot_10k`, triangle budgets, façade edges, slope classes, golden hashes |
| Lanes | counts/offsets per cross-section incl. bus/tram variants; parallelism; reversal |
| Junctions | `junction_random_10k_valid_or_typed_error`, connectors cover movements, same-approach no crossing, conflict zone symmetry, `junction_surface_continuous_8pct`, ring detection |
| Junction control | `no_vehicle_stops_inside_conflict_zone`, `junction_no_conflicting_occupancy`, `equal_priority_4way_all_occupied_no_idle_gt_2s`, light plans without conflicting greens (T/X/Y/5/6-way), all-way greedy release, anti-starvation |
| Traffic | `idm_no_negative_speed_no_overlap_1e6_steps`, `idm_saturation_flow_matches_capacity`, lane plans on short links, spawn queues, gridlock cycle detection, stuck despawn logging |
| Pathfinding | A* = Dijkstra on random graphs; ALT admissible with variants; cache invalidation; budget determinism; negative cache; SCC traps; bounded Dijkstra radius |
| Bridges | span detection + hysteresis, piers outside corridors, abutment steps, overpass leaves lower corridor unchanged, cost monotonic |
| Terrain | determinism (incl. cross-OS hash), compositing order-independence and continuity, bitwise restore on stamp removal, raycast vs brute force, worldgen constraints |
| Utilities | deterministic allocation, brownout order, disconnected networks, shoreline rules, water pollution advection |
| Coverage | network-distance (river without bridge uncovered), incremental reach == full, proportional capacity split |
| Demand / leveling / economy | monotonic demand, breakdown sums, leveling thresholds + hysteresis, ledger = money delta, rounding residuals, loans, bankruptcy, undo refund decay, goods conservation |
| Undo/redo | per command: undo restores hash, redo equals apply; depth; window; blocked reasons; replay log reproduces hash |
| Procgen determinism | identical hashes for same input; different IDs differ; never panics on 10⁴ random lots; footprints within lots |
| Save/load | roundtrip hash; continuation equals continuous (with traffic from M9); corrupt → error; fuzz → no panic; struct-map missing-field default; migration fixtures (from v1.0) |
| Localization | keys complete in en + de; placeholders match; formatting per locale; no string literals in UI code; German overflow test |
| UI | structural tooltips lint; anchors; CVD palette separation (Machado 2009) |
| Audio | loudness targets, seams, clipping/DC/NaN, spectral centroid, deterministic ambient mixer |
| Headless multi-day with traffic | `traffic_multi_day` (medium, CI) and `traffic_multi_day_large` (soak): thresholds in PROGRESS M9 |
| Transit | stop access, transfers, overflow, ridership = boardings, tram geometry, car-trip reduction A/B |

## 5. Determinism tests

1. Same seed + commands, two runs → equal `state_hash` every game day.
2. `results_identical_1_4_8_threads` (fixed 64 partitions).
3. Shuffled schedules (`shuffle_seed`, 3 seeds, single-threaded executor; `bevy/debug` dev
   feature) → equal hashes.
4. Save at day 3 → load → run to day 6 vs continuous to day 6 → equal hashes (with traffic on
   from M9).
5. Presentation independence: vary window size, frame rate (tick distribution per frame),
   graphics preset, `max_visible_vehicles`, camera path and day-length pacing → equal hashes.
6. **Cross-OS**: `determinism_probe` (5 worldgen seeds + 2-game-day scenario with traffic) on
   Linux, Windows and macOS; a CI job compares hashes (blocking from M5, ADR-0054).
7. `replay_command_log_reproduces_hash` (commands applied at recorded `apply_tick`s).

## 6. Rendering checks

- **Smoke**: `--smoke-test 60 --resolution 640x360 --scene smoke` loads a scene containing one
  instance of every material and specialization (extended each milestone), exits non-zero on
  wgpu validation errors or panics.
- **Screenshot suites**: `scripts/screenshot.sh <M>` renders the suite into
  `docs/screenshots/<M>/` with `metrics.json` (luminance histogram/median, clipped %, pure-black %,
  magenta/NaN count) and a semantic class pass with per-shot assertions; gates in PROGRESS.md and
  ADR-0050 calibration metrics; the agent reviews every image (skill `visual-verification`).
- **lavapipe vs GPU**: compared once at M2 with the user's suite; metric deltas recorded.

## 7. Conventions

- Test names: `<unit>_<condition>_<expectation>` (e.g. `subdivide_concave_frontage_rays_do_not_cross`).
- Shared fixtures in `cb_*/src/test_util.rs` behind `#[cfg(any(test, feature = "testing"))]`.
- No sleeping or wall-clock timing in tests; time is driven by ticks.
- Tests may `unwrap()`/`expect()`.
