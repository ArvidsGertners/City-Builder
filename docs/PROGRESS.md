# Progress

Read at the start of every session (after CLAUDE.md and ARCHITECTURE.md). Update after
every milestone and before ending a session.

## Current status

- **Phase:** Phase 1 (planning) — **complete, awaiting approval**.
- **Last updated:** 2026-10-15
- **Engine/toolchain verified available:** Bevy 0.20.0 (2026-10-08), bevy_egui 0.43.1
  (Bevy 0.20), rustc 1.97.0. `bevy-inspector-egui` latest (0.37) targets Bevy 0.19 → see ADR-0031.

## Next step

**Wait for approval of the Phase 1 plan.** After approval, start **M1 Project setup**:
1. Create `Cargo.toml` workspace with the 10 crates from ARCHITECTURE.md §3 (empty modules
   with `//!` docs), workspace lints (`unwrap_used`, `expect_used` deny outside tests),
   profiles, `rust-toolchain.toml` (1.97), `rustfmt.toml`, `clippy.toml`.
2. `cb_game::bootstrap`: paths (portable mode), logging (tracing + file appender + ring
   buffer layer via Bevy `LogPlugin::custom_layer`), crash hook + report writer + "last run
   crashed" detection.
3. `cb_l10n` with en/de bundles and the key-coverage test; `t!` usage in the first window title.
4. `cb_sim` skeleton: `SimPlugins`, `SimEdit`/`SimTick` schedules with ambiguity detection,
   driver with speeds and step-once, headless test that runs 1 000 ticks.
5. Debug tooling skeleton (feature `dev`): perf overlay (egui), sim controls (pause/step/
   speed), log console, in-house inspector stub.
6. CI workflows (ci.yml, release.yml) and `scripts/screenshot.sh` (Xvfb + lavapipe) with
   `--smoke-test`.
7. README (build/run/controls), update PROGRESS.md, commit `M1: …`.

## Milestones

Status legend: ☐ not started · ◐ in progress · ☑ done

| # | Milestone | Status |
|---|---|---|
| 0 | Phase 1 planning package | ☑ (awaiting approval) |
| 1 | Project setup | ☐ |
| 2 | Vertical slice | ☐ |
| 3 | Full road system | ☐ |
| 4 | All zone types, special buildings, leveling visuals | ☐ |
| 5 | Simulation core | ☐ |
| 6 | Services | ☐ |
| 7 | Progression | ☐ |
| 8 | UI, overlays, info views | ☐ |
| 9 | Traffic | ☐ |
| 10 | Public transport | ☐ |
| 11 | Day/night cycle | ☐ |
| 12 | Weather | ☐ |
| 13 | Audio | ☐ |
| 14 | Tutorial | ☐ |
| 15 | Menus, settings, accessibility, save/load, localization | ☐ |
| 16 | Polish, performance, bug fixing | ☐ |

Every milestone additionally requires: `cargo fmt --check`, `cargo clippy --workspace
--all-targets --all-features -- -D warnings`, `cargo test --workspace` green locally and in
CI; docs updated; commit `M<N>: …`.

### M0 — Phase 1 planning package ☑
- CLAUDE.md, ARCHITECTURE.md (structure, schedules, data flow, states, events, commands),
  DATA_MODEL.md, ALGORITHMS.md, GAME_DESIGN.md, ART_DIRECTION.md, PERFORMANCE.md,
  TESTING.md, RISKS.md, PROGRESS.md, ADRs 0001–0036, README.md.

### M1 — Project setup
Acceptance:
- [ ] Workspace with all crates compiles; `cb_sim` builds without any rendering crate
      (CI job `cargo build -p cb_sim` + `cargo tree -p cb_sim` check for forbidden crates).
- [ ] `cargo run -p cb_game` opens a window (localized title) with a clear-colored scene
      and an FPS readout in `dev`.
- [ ] Logs written to `<data_dir>/logs/`, level configurable via `--log`/`CB_LOG`; ring
      buffer keeps the last 500 lines.
- [ ] `--debug-panic` triggers a panic → crash report file with message, location,
      backtrace, version + git hash, OS, adapter, settings summary, last 200 log lines;
      next start shows "last session crashed" notice with the path.
- [ ] Headless test runs 1 000 `SimTick`s; speeds and step-once work in the driver.
- [ ] l10n: en + de bundles; key coverage test; `cb_l10n::format` tests for numbers/money/dates.
- [ ] CI: fmt, clippy, test on ubuntu-latest + windows-latest with rust-cache; render smoke
      job on Linux (lavapipe); release workflow builds zipped artifacts on `v*` tags.
- [ ] Dev tooling: perf overlay, sim controls panel, log console, inspector stub, feature `dev`.

### M2 — Vertical slice (must look good before moving on)
Acceptance:
- [ ] Seeded terrain: hills, valleys, 1–2 rivers, 1–3 lakes; materials grass/dirt/rock/sand/
      forest; chunked LOD rendering with skirts; water with depth color, shoreline foam,
      fresnel/sky reflection.
- [ ] Trees, rocks, grass clumps scattered deterministically, removed under roads/plots/
      buildings and restored on undo of those actions (undo arrives in M3; here: on removal).
- [ ] Camera: WASD/arrows/left-drag pan (rotation-relative), wheel zoom (clamped), MMB
      free rotate, Q/E 90° snaps, terrain-following focus, smoothing, bounds; bindings from
      the `InputMap`.
- [ ] Road tool: straight/curved/freehand street segments with snapping to nodes/segments,
      elevation (PgUp/PgDn), vertical profile + grading (cut/fill embankments), simple
      node joins (full junctions in M3), validity feedback (slope, curvature, overlap) and
      live cost.
- [ ] Residential plot tool (frontage drag + depth, vertex editing, sub-plot count);
      subdivision into irregular lots; generated houses (L1–L3 styles) that fit lots, face
      the road, adapt to slopes (pads/plinths/stilts) and fill yards.
- [ ] Lighting & art style in place: procedural sky, sun with CSM soft shadows, SSAO + vertex
      AO, fog, tonemapping, bloom; time-of-day presets selectable (morning/noon/sunset/night
      preview) via debug panel.
- [ ] Screenshot set (`scripts/screenshot.sh`) reviewed against ART_DIRECTION.md; 60 FPS with
      500 houses on the dev machine profile.
- [ ] Tests: splines, profile solver, grading stamps, subdivision suite, building procgen
      determinism & containment, terrain determinism, raycast.

### M3 — Full road system
Acceptance:
- [ ] Road hierarchy: dirt path, street, one-way street, avenue, one-way avenue;
      upgrade/downgrade in place.
- [ ] Lanes generated per cross-section, stored in `LaneGraph`; debug overlay shows lanes,
      connectors, conflict zones.
- [ ] Automatic junctions (cut-backs, fillets, crosswalks, stop lines, lane arrows,
      connectors) for crossings and endpoint joins; grade separation by elevation.
- [ ] Intersection tool: priority (auto), all-way stop, traffic lights (auto plan,
      timing override).
- [ ] Bridges over water/valleys with supports, railings, abutments, length-scaled cost.
- [ ] Segment customization: surface, sidewalks, curbs, grass strips, tree rows, street
      lamps with spacing; decorations follow curves/elevation.
- [ ] Undo/redo (Ctrl+Z/Ctrl+Y) for all road and plot actions with refunds; history 100.
- [ ] Bulldozing of segments/nodes/plots with dependency warning dialog (remove plots /
      keep plots (lose access) / cancel).
- [ ] Tests: lanes, junctions (randomized), connectivity, bridges, undo/redo roundtrips.

### M4 — All zone types, special buildings, building leveling
Acceptance:
- [ ] Commercial and industrial plots/lots/buildings with their generators and palettes.
- [ ] Service, utility, transit and special building generators (signature silhouettes),
      free placement with rotation and road-edge snapping, footprint validation.
- [ ] Level variants R1–R5, C1–C3, I1–I3; construction animation; run-down/abandoned and
      ruined visuals (shader + generator).
- [ ] Procgen debug scene (`--procgen-gallery`): random lots × types × levels, vehicles per
      type, regenerate/seed controls.
- [ ] Building LOD1/LOD2 and chunk merging with per-building param buffer; 10k-building
      synthetic scene renders within GPU budget.

### M5 — Simulation core
Acceptance:
- [ ] Clock/calendar/seasons; speeds pause/1×/2×/4×; fixed timestep driver with interpolation α.
- [ ] Outside connections, immigration/emigration; households & citizens tables; jobs
      matching; schedules (time wheel); trips (macro only, agents in M9).
- [ ] RCI demand with breakdown; lot development by demand; leveling/decline/abandonment
      logic driving M4 visuals.
- [ ] Economy: taxes per zone, upkeep, construction costs, ledger, loans, bankruptcy warnings.
- [ ] Utilities: power (plants incl. weather/day dependence hooks), water (shoreline pumps,
      towers), sewage (outlets, treatment), garbage (analytic collection until M9).
- [ ] Fields: pollution, noise, land value; happiness.
- [ ] Save/load core (snapshot, MessagePack + LZ4, header, migration framework, fixture v1).
- [ ] Tests: headless growth (3 days), determinism (incl. save/load continuation, shuffled
      schedules), utilities, demand, leveling, economy.

### M6 — Services
Acceptance:
- [ ] Police, fire, health, education buildings with network-distance coverage, capacity and
      budgets; incidents (crime, fire with spread/burning/ruins, sickness) and education
      progression; service vehicles in ghost mode until M9.
- [ ] Coverage incremental == full recompute (test); coverage feeds happiness/LV/leveling.

### M7 — Progression
Acceptance:
- [ ] Population tiers and unlocks from `progression.ron`; locked tools shown with unlock
      condition; milestone notification with summary; sandbox mode (all unlocked, unlimited money).

### M8 — UI, overlays, info views
Acceptance:
- [ ] Top bar, RCI bars, tool bar with flyouts, road customization panel, budget & tax
      panel, info panels (building, road, vehicle placeholder, line placeholder),
      notifications with jump-to, tooltips everywhere, all strings localized.
- [ ] Overlays: traffic (placeholder data until M9), power, water, sewage, garbage, each
      service coverage, land value, pollution, noise, happiness, demand, terrain slope —
      with legends, CVD-safe palettes and redundant patterns.
- [ ] Problem icons above buildings (billboards, clustering, fading).

### M9 — Traffic
Acceptance:
- [ ] Vehicle agents (commuter cars, delivery trucks C↔I and outside, service vehicles,
      garbage trucks) spawned from trips; despawn into buildings.
- [ ] Routing (ALT A*, cache, variants, batching), lane following, lane changes, IDM,
      slope/curve/weather speed, junction control (priority, stops, lights), no overlaps.
- [ ] Congestion per segment feeding costs, commute times, demand, happiness, response
      times; agent cap + sampling + macro background; traffic-disabled fallback verified.
- [ ] Gridlock protection with logs; pedestrians (visual) near camera; follow-camera on vehicles.
- [ ] Bench: 5 000 agents within budget; 10-day soak test green.

### M10 — Public transport
Acceptance:
- [ ] Buses: depot, stops, lines (create/edit/delete, color, vehicle count), bus lanes,
      dwell/boarding, ridership stats, costs/income.
- [ ] Trams: tracks in road (avenue/street option) and standalone tracks, stops, depot,
      lines, articulated trams.
- [ ] Mode choice uses transit; transit reduces car trips; land value near stops; line
      management panel and transit overlay; info panel for lines.

### M11 — Day/night cycle
Acceptance:
- [ ] Continuous sun/moon movement, sky transitions, sunrise/sunset tones per ART_DIRECTION
      keyframes; configurable cycle length; lock time-of-day option.
- [ ] Lit windows (deterministic per building), street lamps with light pools, headlights/
      tail lights, lit transit vehicles and stops; night readable.
- [ ] Commute peaks and commercial activity visibly follow the clock.

### M12 — Weather
Acceptance:
- [ ] Clear, cloudy, rain, thunderstorm, fog, snow; seasons with probabilities; forecast UI;
      disable/lock options.
- [ ] Visuals: rain/snow particles, wet roads and puddle darkening, snow on roofs/terrain/
      trees, fog density, cloud shadows, lightning (respects reduce-flashes), seasonal
      vegetation colors.
- [ ] Sim effects: speeds, maintenance cost, lightning fires, happiness, solar/wind output.

### M13 — Audio
Acceptance:
- [ ] Synthesized bank: ambient beds (nature, traffic hum, industry, rain, wind, night
      insects), thunder, UI sounds, construction/demolition, vehicles near camera, sirens.
- [ ] Ambient director mixes by zoom, time, weather, on-screen content; channel volumes
      (master, music, effects, ambient).

### M14 — Tutorial
Acceptance:
- [ ] Guided steps: camera, draw road, draw plot, power & water, read demand; later
      chapters when traffic/transit unlock; contextual first-use hints with toggle;
      replay from main menu.

### M15 — Menus, settings, accessibility, save/load, autosave, localization
Acceptance:
- [ ] Main menu (new game with seed & sandbox, load, tutorial, settings, quit), pause menu.
- [ ] Settings: graphics quality items, resolution, fullscreen, UI scale, key remapping,
      audio volumes, language, accessibility (CVD modes, high contrast, reduce weather
      effects/flashes, reduced motion), day/night & weather toggles, traffic toggle,
      autosave interval.
- [ ] Save/load UI with slots, metadata, autosave with rotating slots; migration fixtures.
- [ ] German localization complete; locale formatting everywhere.

### M16 — Polish, performance pass, bug fixing
Acceptance:
- [ ] Performance targets met on reference profile (bench + in-game); results recorded.
- [ ] Bug backlog cleared to zero known crashers and no known gameplay blockers.
- [ ] Final art pass via screenshot review; README/controls complete; docs current.

## Known bugs

None yet.

## Log

- 2026-10-15 — Phase 1 planning package written (this commit). Verified Bevy 0.20.0 API
  surface from crate sources: `Message`/observers split, WESL shaders (naga_oil removed),
  `ShaderBuffer`, `MeshTag`, `VisibilityRange`, `ScreenSpaceAmbientOcclusion`,
  `ContactShadows`, `GeneratedEnvironmentMapLight`, `DistanceFog`, `EguiPrimaryContextPass`.
