# Testing Strategy

Status: **Accepted (Phase 1 plan)**.

## 1. Principles

- The hard logic lives in pure crates → fast unit tests (whole pure-crate suite < 30 s).
- The simulation is headless → integration tests drive real `SimPlugins` in a `MinimalPlugins`
  app, no GPU, no window.
- Determinism is a testable property: same seed + same command stream ⇒ same state hash.
- Randomized tests use our own seeded RNG (`cb_core::rng`) with fixed seeds and loop counts;
  a failing seed is printed so it can be pinned as a regression case. (No proptest
  dependency — ADR-0023.)
- Rendering is checked by a smoke test (shader compile + N frames) and screenshot review,
  not pixel-exact assertions.

## 2. Levels

| Level | Where | Runs in CI | Typical time |
|---|---|---|---|
| Unit | `#[cfg(test)]` modules in every crate | yes (Linux + Windows) | seconds |
| Golden/snapshot | pure crates: procgen hashes, junction geometry snapshots (RON in `tests/snapshots/`) | yes | seconds |
| Integration (headless sim) | `crates/cb_sim/tests/*.rs` using `cb_sim::testing::Scenario` | yes | < 2 min |
| Soak (multi-day, large) | `#[ignore]` tests in `cb_sim/tests/` | nightly workflow + before milestones | 5–15 min |
| Schedule property | sim schedules built with shuffled order (`shuffle_seed`) + single-threaded executor, state hash compared | yes (3 seeds) | < 1 min |
| Render smoke | `cb_game --smoke-test 300` under Xvfb + lavapipe (Linux job) | yes (Linux) | ~3 min |
| Screenshot review | `scripts/screenshot.sh` → PNGs for manual/agent review | on demand, per art-relevant milestone | ~5 min |
| Bench | `headless_bench` | on demand + CI informational | minutes |

## 3. Scenario harness (cb_sim::testing)

```rust
let mut s = Scenario::new(seed).flat_terrain().money(1_000_000);
let a = s.road(RoadType::Street, [(0.0, 0.0), (300.0, 0.0)]);
let b = s.road_curved(RoadType::Avenue, (300.0, 0.0), (420.0, 60.0), (400.0, 200.0));
s.plot(Zone::Residential, a.side(Side::Left), 20.0..280.0, depth 30.0);
s.place(Service::PowerDiesel, near(a, 50.0)); s.place(Utility::WaterPump, shore());
s.run_days(3);
assert!(s.population() > 50);
assert_eq!(s.problems(Problem::NoPower), 0);
let h = s.state_hash();   // stable hash over sorted saved state
```
`Scenario` issues real `PlayerCommand`s (so command validation is exercised), runs
`SimEdit`/`SimTick` explicitly, and exposes query helpers. `state_hash()` hashes the save
snapshot (the canonical sorted representation), so it is independent of ECS iteration order.

## 4. Required test coverage (from the project brief)

| Area | Tests |
|---|---|
| Geometry edge cases | Bézier split/eval/nearest-point vs. dense sampling; arc-length accuracy; curvature on degenerate curves (coincident control points); offset of sharp curves (no inversion at r_min); polygon orientation/area/containment with collinear & duplicate points; self-intersecting input rejected; ear clipping on random simple polygons (area preserved, no inverted triangles); boolean ops on touching/overlapping/slivers; rect fitting on L/U/angled shapes |
| Plot subdivision | tiny lots (N clamps), sharp concave frontage (no crossing rays), corner plots on two roads, deep plots, N too large, slivers merged, stable lot IDs across small edits, all lots touch frontage, union of lots ⊆ plot and area conserved ±0.5 % |
| Lane generation | lane count/offset per cross-section; one-way; lane polylines parallel within tolerance; lengths consistent; bus/tram lanes; lanes reversed correctly |
| Junctions | cut-backs for T/X/Y/5-way/acute angles; polygon simple; connectors exist for all allowed movements; same-approach connectors never cross; conflict zones symmetric; continuation nodes produce no junction |
| Road graph connectivity | components after add/remove/split; outside reachability; one-way dead ends detected by routing tests |
| Pathfinding | A* = Dijkstra on random graphs (optimality); ALT admissibility; cache invalidation on removed links; variants deterministic; unreachable handled; bounded Dijkstra radius |
| Intersection rules | no deadlock on standard junctions under random arrivals (10⁵ ticks, all control types); no two vehicles in conflicting zones simultaneously; stop-sign full stop; light phases never give conflicting greens; anti-starvation |
| Traffic | IDM never produces negative speed or overlap; queue discharge rate within 10 % of expected capacity; gridlock detector despawns stuck agents and logs |
| Bridges | span detection over water/valleys; piers inside span and outside other corridors; min clearance; cost scales with length |
| Terrain | determinism of generation; stamps order-independent (shuffle insertion → identical heights); remove stamp restores base exactly; raycast vs. brute force |
| Utilities | supply/demand allocation deterministic; brownout order; disconnected networks; shoreline requirement |
| Service coverage | network distance (not Euclidean): a building across a river without bridge is uncovered; incremental recompute equals full recompute |
| Demand | monotonic responses (more jobs → R up; higher tax → down) on synthetic inputs |
| Leveling | thresholds and hysteresis; decline/abandon/recover sequences |
| Economy | ledger sums equal money delta; rounding residuals carried; loans; refunds on undo |
| Undo/redo | every command: apply → undo restores state hash; apply → undo → redo equals apply; history depth limit; cleared on load |
| Procgen determinism | building/vehicle/tree/yard meshes: same seed → identical hash; different IDs → different; generation never panics on random lots (10⁴ random polygons) and stays within lot polygon (footprint ⊆ lot) |
| Save/load | roundtrip: snapshot → bytes → load → snapshot equal; run N days, save, load, run M days vs. continuous run → equal hashes; corrupted file → error, no panic |
| Migration | golden saves for every format version in `tests/fixtures/` load and migrate to latest |
| Localization | every key present in en and de; no unused keys; placeholders consistent; number/date formatting per locale |
| Headless multi-day with traffic | `traffic_multi_day`: medium city, traffic on, 10 game days, asserts no panics, gridlock despawns < threshold, commute EMA reasonable, population grows, tick p95 within budget (soak variant with large city) |
| Transit routing | stop access, transfers, mode choice shifts with fares/headway, ridership counts consistent with boardings |

## 5. Determinism tests

1. Same seed, same commands, two runs → equal `state_hash` every game day.
2. Save at day 3 → load → run to day 6 vs. continuous to day 6 → equal hashes.
3. Shuffled schedule order (3 seeds) + single-threaded executor → equal hashes (catches
   hidden ordering dependencies).
4. Traffic enabled with different presentation settings (max visible vehicles) → equal
   hashes (proves presentation never affects sim).
Cross-platform equality is **not** asserted (ADR-0017); CI runs each test per platform.

## 6. Render smoke & screenshots

- `cb_game --smoke-test <frames> [--seed S] [--scene vertical-slice]` loads a fixed scene,
  runs N frames, exercises every material/shader path, exits non-zero on wgpu validation
  errors or panics. CI Linux job installs `mesa-vulkan-drivers` + `xvfb`.
- `scripts/screenshot.sh` renders fixed camera views × times of day × weathers to
  `target/screenshots/` for art review at each visual milestone (M2, M3, M4, M8, M11, M12, M16).

## 7. Conventions

- Test names: `<unit>_<condition>_<expectation>` (e.g. `subdivide_concave_frontage_rays_do_not_cross`).
- Shared fixtures in `cb_*/src/test_util.rs` behind `#[cfg(any(test, feature = "testing"))]`.
- No sleeping or wall-clock timing in tests; time is driven by ticks.
- Tests may `unwrap()`/`expect()`.
