# Architecture

Status: **Accepted (Phase 1 plan, revised after the 2026-10-15 plan review)** · Engine: Bevy 0.20 · Rust stable, edition 2024

This is the entry document. Read it, then `docs/PROGRESS.md`, at the start of every session.
Topic documents:

| Doc | Content |
|---|---|
| [DATA_MODEL.md](DATA_MODEL.md) | Every component, resource, message; IDs; how each world object is represented |
| [ALGORITHMS.md](ALGORITHMS.md) | Roads, lanes, junctions, bridges, terrain, plots, buildings, pathfinding, traffic, sim systems, picking |
| [GAME_DESIGN.md](GAME_DESIGN.md) | Rules and initial balance tables (road types, zones, services, unlocks, economy, weather) |
| [ART_DIRECTION.md](ART_DIRECTION.md) | Palettes, lighting moods, materials, UI style |
| [PERFORMANCE.md](PERFORMANCE.md) | Targets, frame budgets and how the design meets them |
| [TESTING.md](TESTING.md) | Unit, integration, headless, determinism, render smoke tests |
| [RISKS.md](RISKS.md) | Risks, mitigations, open questions |
| [CONSTANTS.md](CONSTANTS.md) | Stability/balance constants with owning tests and target metrics |
| [ASSUMPTIONS.md](ASSUMPTIONS.md) | Unconfirmed assumptions, defaults chosen, questions for the user |
| [PLAN_REVIEW.md](PLAN_REVIEW.md) | Independent plan review (2026-10-15) and how each finding was resolved |
| [adr/README.md](adr/README.md) | Index of architecture decision records |

---

## 1. Goals and constraints

- Fully 3D, continuous world (4 × 4 km), **no grid or tile placement**; all visuals generated
  at runtime (no image/model assets).
- Simulation separated from rendering, runnable and testable headless, deterministic for a
  given seed + command log across all three platforms (ADR-0043).
- 10k+ buildings, 50k+ citizens, 5k+ simultaneously simulated vehicles at 60 FPS on a
  ~2019 mid-range GPU (GTX 1660 Super / RX 5600 XT class) and a 6-core CPU, **and on an
  Apple M4** (base, 10-core GPU) at the Retina display's auto render scale.
- Required platforms: Windows 10/11 x86-64, Linux x86-64, macOS 14+ on Apple Silicon
  (ADR-0037), with native input conventions (ADR-0052), per-OS file locations (ADR-0040)
  and high-DPI handling (ADR-0053).
- Stylized low-poly look (Townscaper/Islanders spirit), readable at any time and weather.
- Playable with traffic agents disabled (aggregate fallback).
- Architecture must not block: metro, trains, ferries, airports, tunnels, parking sim,
  more disasters, modding, multiplayer.

## 2. Layered architecture

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ cb_game (binary)                                                             │
│  Presentation plugins: render views, lighting director, weather FX, camera,  │
│  picking, tools, UI (egui), overlays, audio playback, menus, settings,       │
│  save UI, tutorial, debug tooling                                            │
│        │ reads sim state (Query/Res, read-only)      ▲ PlayerCommand messages │
│        ▼                                             │                       │
├──────────────────────────────────────────────────────────────────────────────┤
│ cb_sim (headless library: bevy_app + bevy_ecs + bevy_state, no rendering)    │
│  Sim plugins: clock, catalog, commands/undo, roads, terrain, zoning,         │
│  buildings, citizens, demand, economy, utilities, services, fields, traffic, │
│  transit, weather(sim), progression, notifications, stats, save/load         │
│  Schedules: SimEdit (every frame, also paused) · SimTick (dt 0.1 sim s)      │
├──────────────────────────────────────────────────────────────────────────────┤
│ Pure crates (no Bevy): cb_core · cb_geometry · cb_terrain · cb_network ·     │
│ cb_traffic · cb_procgen · cb_l10n · cb_audio                                 │
│  Algorithms over plain data, exhaustively unit-tested                        │
└──────────────────────────────────────────────────────────────────────────────┘
```

- The **sim layer** owns the canonical game state as ECS entities/components (discrete
  world objects: nodes, segments, plots, lots, buildings, stops, lines) plus resources for
  table- or graph-shaped data (terrain, lane graph, traffic agents, population, fields,
  economy). See ADR-0005 and DATA_MODEL.md.
- The **pure crates** hold the hard algorithms. They are engine-independent so they can be
  tested in milliseconds and reused (e.g. by a future dedicated server or mod tools).
- The **presentation layer** derives everything visual from sim state and never mutates
  sim state directly; it sends `PlayerCommand`s (ADR-0055).

## 3. Project structure

```
City-Builder/
├── Cargo.toml                  Workspace: members, [workspace.dependencies], [workspace.lints], profiles
├── rust-toolchain.toml         Pinned stable toolchain + rustfmt/clippy (ADR-0060)
├── rustfmt.toml                Formatting rules
├── clippy.toml                 Clippy config (msrv, thresholds)
├── .cargo/config.toml          Aliases (`cargo bench-sim`, `cargo gallery`), optional fast linker
├── .github/workflows/
│   ├── ci.yml                  push/PR: fmt, clippy -D warnings, test on Linux + Windows; macOS only on main;
│   │                           sim-isolation, render smoke (Linux), cross-OS determinism (Linux + Windows)
│   ├── nightly.yml             schedule + manual dispatch: macOS check/smoke, 3-OS determinism, soak, benches
│   └── release.yml             Tagged builds → Linux tar.gz, Windows zip, macOS arm64 .app zip (ADR-0060)
├── LICENSE                     Proprietary, all rights reserved (ADR-0059)
├── THIRD_PARTY_NOTICES.md      Font licence (Inter, SIL OFL) and later crate licences
├── .claude/skills/             Claude Code skills: milestone-completion, write-adr, visual-verification, …
├── packaging/macos/Info.plist  App bundle metadata (bundle id, NSHighResolutionCapable, min macOS 14)
├── scripts/
│   ├── screenshot.sh           Xvfb + lavapipe: render a milestone screenshot suite into docs/screenshots/<M>/
│   ├── check_sim_isolation.sh  Fails if cb_sim (transitively) depends on rendering/window/audio crates
│   └── macos_bundle.sh         Build CityBuilder.app (+ ad-hoc codesign) from the dist profile
├── spikes/                     Phase S throwaway spikes: traffic, junctions, plots, timemodel, grading, render
│                               (not workspace default members; removed after use, kept on tags spike/<name>)
├── CLAUDE.md                   Agent/dev conventions (read first)
├── README.md                   Build, run, controls
├── docs/                       This documentation set, ADRs in docs/adr/, screenshots in docs/screenshots/<M>/,
│                               independent plan review in docs/review/
└── crates/
    ├── cb_core/                Foundation types (no Bevy)
    │   └── src/
    │       ├── lib.rs
    │       ├── ids.rs          Typed stable IDs (NodeId, SegmentId, PlotId, LotId, BuildingId, …), IdAllocator
    │       ├── hash.rs         Stable 64-bit hashing (wyhash-style mix), derive_seed(), Domain tags
    │       ├── rng.rs          SplitMix64 / Pcg32 streams, counter-based event RNG, distributions
    │       ├── spatial.rs      Loose spatial hash grid (generic over key), AABB/segment/point queries
    │       ├── units.rs        Meters, speeds, angles helpers; consts
    │       ├── time.rs         GameTime (game-ms), Calendar (year/season/day/hh:mm), durations
    │       ├── money.rs        Money(i64 cents) with checked arithmetic
    │       ├── collections.rs  LookupMap (fixed hasher, no iteration), SortedVecMap, DenseSlotMap, BitSet
    │       ├── math.rs         Portable math facade over libm (sin/cos/exp/powf/atan2/…) — ADR-0043
    │       ├── test_util.rs    Seeded random-case generators + tiny shrinker (cfg(test)/feature "testing")
    │       └── error.rs        Shared error helpers
    ├── cb_geometry/            Geometry kernel (no Bevy)
    │   └── src/
    │       ├── bezier.rs       Cubic Bézier 2D/3D: eval, derivatives, curvature, split, bounds, nearest point
    │       ├── arc_length.rs   Arc-length LUT, uniform resampling
    │       ├── polyline.rs     Polyline with cumulative length, offset, projection, simplification
    │       ├── curve_fit.rs    Schneider Bézier fitting for freehand strokes
    │       ├── polygon.rs      Simple polygon: area, orientation, contains, convexity, inset, repair
    │       ├── boolean.rs      Wrapper over i_overlay (union/intersection/difference/offset) with snapping
    │       ├── triangulate.rs  Ear clipping (with hole bridging) for meshing polygons
    │       ├── rect_fit.rs     Largest inscribed oriented rectangle in polygon
    │       ├── subdivide.rs    Frontage-strip lot subdivision (ADR-0014)
    │       ├── predicates.rs   Robust orientation/intersection tests (f64, epsilon policy)
    │       ├── sweep.rs        Sweep a 2D cross-section along a 3D curve (road/bridge/railing meshes)
    │       ├── mesh.rs         MeshData (positions, normals, colors, custom attributes, indices) + builders
    │       └── ray.rs          Ray/AABB/triangle/plane intersection
    ├── cb_terrain/             Terrain (no Bevy)
    │   └── src/
    │       ├── heightfield.rs  2 m grid storage, bilinear/triangle-consistent sampling, normals, slope
    │       ├── noise.rs        Gradient noise, fBm, ridged, domain warp (deterministic, f32)
    │       ├── generate.rs     World-gen pipeline from seed: macro shape → hills → valleys → erosion-lite
    │       ├── rivers.rs       River tracing (steepest descent + meander) and channel carving
    │       ├── lakes.rs        Basin detection, lake level fill
    │       ├── water.rs        Water surface field, depth queries, shoreline mask
    │       ├── materials.rs    Material weights (grass/dirt/rock/sand/forest) from slope/height/moisture
    │       ├── stamps.rs       Deformation stamps (road corridors, pads) and order-independent compositing
    │       ├── raycast.rs      Min-max quadtree ray march for cursor picking
    │       ├── chunk_mesh.rs   Chunk mesh data with LOD + skirts
    │       └── scatter.rs      Deterministic tree/rock/grass scatter with exclusion masks
    ├── cb_network/             Road network geometry (no Bevy)
    │   └── src/
    │       ├── cross_section.rs Road type cross-sections (lanes, sidewalks, medians, tracks)
    │       ├── segment_geom.rs  Horizontal alignment (cubic Bézier) + trimmed ranges + edges
    │       ├── profile.rs       Vertical profile solver (terrain-following, max grade, smoothing)
    │       ├── grading.rs       Cut/fill stamp generation, embankments
    │       ├── bridge.rs        Bridge span detection, support placement, deck params
    │       ├── junction.rs      Junction geometry: cut-backs, corner fillets, polygon, crosswalks
    │       ├── lanes.rs         Lane polylines from cross-section + curve
    │       ├── lane_graph.rs    LaneGraph (lanes, connectors, conflict zones), incremental patching
    │       ├── turn_rules.rs    Lane arrows / allowed movements per approach
    │       ├── conflicts.rs     Connector-vs-connector conflict zone extraction
    │       └── validation.rs    Slope, curvature, overlap, clearance checks with reasons
    ├── cb_traffic/             Traffic & routing (no Bevy)
    │   └── src/
    │       ├── link_graph.rs   Routing graph (directed segment links + turn edges), per-mode views
    │       ├── router.rs       A* with ALT landmarks; bounded Dijkstra; batch API
    │       ├── route_cache.rs  LRU (origin, dest, mode, variant) → Arc<[LinkId]>, epoch invalidation
    │       ├── costs.rs        Link cost functions (free-flow, congestion EMA, turn penalties, variants)
    │       ├── vehicles.rs     Vehicle SoA store (DenseSlotMap), kinds, routes, lane positions
    │       ├── idm.rs          Intelligent Driver Model, slope/curve/weather speed caps
    │       ├── lane_change.rs  Route-driven + MOBIL-lite discretionary lane changes
    │       ├── junction_ctl.rs Priority/yield, all-way stop, traffic light phase plans
    │       ├── reservations.rs Conflict-zone locking + exit-space check (deadlock avoidance)
    │       ├── admission.rs    Agent cap, adaptive sampling, ghost mode when traffic is disabled
    │       ├── flow_model.rs   Macro flows (EMA), BPR travel times, background volume
    │       ├── gridlock.rs     Stuck detection, despawn policy, diagnostics
    │       ├── transit.rs      Transit vehicle runs, dwell, boarding/alighting, articulated trams
    │       ├── pedestrians.rs  Visual pedestrian agents on sidewalk polylines
    │       └── stats.rs        Per-segment speed/volume/congestion stats
    ├── cb_procgen/             Procedural content (no Bevy)
    │   └── src/
    │       ├── palette.rs      Palettes per zone/type/season (from ART_DIRECTION.md)
    │       ├── building/       massing.rs, roofs.rs, facade.rs, windows.rs, details.rs, foundation.rs,
    │       │                   residential.rs, commercial.rs, industrial.rs, service.rs, utility.rs,
    │       │                   transit.rs, special.rs, condition.rs, lod.rs, mod.rs (generate())
    │       ├── yard.rs         Gardens, sheds, fences, driveways, parking, loading yards
    │       ├── vehicle.rs      Cars, trucks, service vehicles, buses, trams
    │       ├── vegetation.rs   Trees (conifer/deciduous variants, seasonal masks), bushes, grass clumps
    │       ├── props.rs        Street lamps, benches, rocks, fences, bollards, stop shelters, signals
    │       ├── road_mesh.rs    Road surface/markings/curbs/sidewalks sweep, decoration placement
    │       ├── junction_mesh.rs Junction surface, crosswalks, stop lines
    │       ├── bridge_mesh.rs  Decks, supports, railings
    │       ├── icons.rs        SDF icon set rasterized into an RGBA atlas at runtime
    │       └── gallery.rs      Random lot/building/vehicle sets for the procgen debug scene
    ├── cb_l10n/                Localization (no Bevy)
    │   ├── locales/en/*.ftl, locales/de/*.ftl   Embedded with include_str!
    │   └── src/ bundle.rs (Fluent), format.rs (numbers/money/dates/times per locale), keys.rs
    ├── cb_audio/               Procedural audio DSP (no Bevy)
    │   └── src/ osc.rs, noise.rs, filter.rs, envelope.rs, reverb.rs, wav.rs, bank.rs,
    │            sounds/{ambient,ui,construction,vehicles,weather,sirens}.rs
    ├── cb_sim/                 Headless simulation (bevy_app/bevy_ecs/bevy_state)
    │   ├── data/               Catalog RON files embedded at compile time (roads, zones, buildings,
    │   │                       services, utilities, transit, progression, economy, weather)
    │   ├── src/
    │   │   ├── lib.rs          SimPlugins plugin group, public API surface
    │   │   ├── schedule.rs     SimEdit / SimTick schedules, SimSet ordering, ambiguity settings
    │   │   ├── driver.rs       Fixed-timestep driver (accumulator, speeds, step-once, catch-up cap)
    │   │   ├── clock.rs        SimClock, GameTime/Calendar advance, season
    │   │   ├── catalog.rs      Catalog resource + validation of RON definitions
    │   │   ├── ids.rs          IdAllocators, IdMap<K> (stable ID ↔ Entity), ObjectId<K> component
    │   │   ├── rng.rs          SimRng streams (counter-based)
    │   │   ├── commands/       mod.rs (PlayerCommand, CommandQueue), history.rs (undo/redo),
    │   │   │                   road.rs, plot.rs, building.rs, transit.rs, policy.rs
    │   │   ├── roads/          editor.rs (split/merge/connect), junctions.rs, lanes_sync.rs,
    │   │   │                   connectivity.rs, outside.rs (outside connections)
    │   │   ├── terrain.rs      Terrain resource, stamp application, dirty regions
    │   │   ├── zoning/         plots.rs, lots.rs, growth.rs (lot development)
    │   │   ├── buildings/      construction.rs, leveling.rs, condition.rs, access.rs, massing.rs
    │   │   ├── citizens/       population.rs, households.rs, schedule.rs, jobs.rs, migration.rs,
    │   │   │                   happiness.rs, education.rs, trips.rs, mode_choice.rs
    │   │   ├── demand.rs       RCI demand
    │   │   ├── economy/        ledger.rs, taxes.rs, costs.rs, upkeep.rs, loans.rs, goods.rs (ADR-0057)
    │   │   ├── utilities/      power.rs, water.rs, sewage.rs, water_pollution.rs, garbage.rs
    │   │   ├── services/       reach.rs, allocation.rs, dispatch.rs, incidents.rs, police.rs, fire.rs,
    │   │   │                   health.rs, education.rs
    │   │   ├── fields/         raster.rs, land_value.rs, pollution.rs, noise.rs
    │   │   ├── traffic.rs      Bridge to cb_traffic: lane graph sync, trip → agent, stats feedback
    │   │   ├── transit/        stops.rs, lines.rs, network.rs, ridership.rs
    │   │   ├── weather.rs      Weather state machine, seasons, sim effects
    │   │   ├── progression.rs  Milestones, unlocks, sandbox
    │   │   ├── notifications.rs SimNotification messages
    │   │   ├── stats.rs        City statistics history (for graphs)
    │   │   ├── spatial.rs      Spatial index maintenance (observers + dirty)
    │   │   ├── save/           format.rs (header/body), snapshot.rs (collect/restore), migrate.rs
    │   │   ├── worldgen.rs     New game: terrain, outside connections, initial state
    │   │   └── testing/        scenario.rs (Scenario builder & assertions), citygen.rs (organic road +
    │   │                       plot generator + forced growth: benchmarks, soak tests, menu backdrop)
    │   ├── tests/              Headless integration tests (growth, determinism, save roundtrip,
    │   │                       migration, utilities, services, traffic multi-day, transit)
    │   │   └── fixtures/       Golden saves per format version
    │   └── examples/           headless_bench.rs (citygen city, per-system timings, --balance),
    │                           determinism_probe.rs (worldgen + scenario hashes for the cross-OS CI job)
    └── cb_game/                Game binary (full Bevy + bevy_egui)
        ├── assets/fonts/     Inter-Regular.ttf, Inter-SemiBold.ttf, OFL.txt (the only asset files, ADR-0059)
        └── src/
            ├── main.rs         CLI parsing, bootstrap (paths → logging → crash hook → App)
            ├── app.rs          Plugin assembly; headless/sim vs presentation plugins
            ├── states.rs       AppState, InGameMenu, transitions
            ├── platform/       paths.rs (per-OS AppPaths, ADR-0040), modifiers.rs (PRIMARY = Cmd/Ctrl, glyphs),
            │                   gestures.rs (pinch/rotate/pan, scroll-unit policy), window.rs (fullscreen,
            │                   DPI, frame cap), open.rs (open folder/URL) — the only cfg(target_os) code
            ├── bootstrap/      logging.rs (file + ring buffer), crash.rs (panic hook, reports), cli.rs
            ├── settings/       mod.rs (Settings resource), graphics.rs (presets → camera/render),
            │                   persist.rs (RON, versioned), apply.rs
            ├── input/          actions.rs (Action enum), bindings.rs (InputMap), contexts.rs
            ├── camera/         controller.rs (orbit/pan/zoom/snap), follow.rs, bounds.rs
            ├── picking.rs      Cursor ray (logical px → NDC → render-scaled target, ADR-0053) → terrain hit → objects (ADR-0034)
            ├── render/
            │   ├── setup.rs    World camera, offscreen render-scale target + composite pass, post FX presets
            │   ├── materials/  world.rs, terrain.rs, water.rs, road.rs, sky.rs, precipitation.rs,
            │   │               light_pool.rs, overlay.rs, billboard.rs (ExtendedMaterial + WESL)
            │   ├── shaders/    *.wesl (embedded via embedded_asset!)
            │   ├── chunks.rs   Render chunk keys, chunk entity registry, dirty chunk queue
            │   ├── mesh_jobs.rs Async procgen/merge jobs on AsyncComputeTaskPool
            │   ├── terrain_view.rs, water_view.rs, road_view.rs, building_view.rs, prop_view.rs,
            │   │   vehicle_view.rs, transit_view.rs, pedestrian_view.rs, fx_view.rs (smoke/fire/dust)
            │   ├── world_icons.rs Problem icon billboards
            │   └── lod.rs      VisibilityRange policy per quality preset
            ├── lighting/       director.rs (time+season+weather → light state), keyframes.rs,
            │                   env_map.rs (procedural cubemap), sky.rs
            ├── weather_fx/     precipitation.rs, lightning.rs, wetness.rs, snow.rs, clouds.rs
            ├── tools/          mod.rs (ActiveTool), road.rs, plot.rs, building.rs, intersection.rs,
            │                   transit.rs, bulldoze.rs, preview.rs (ghosts), snapping.rs, feedback.rs
            ├── ui/             theme.rs, icons.rs, widgets.rs, hud/ (top_bar, rci, toolbar, speed),
            │                   panels/ (road, budget, transit_lines, info, milestones, city_stats),
            │                   notifications.rs, tooltips.rs, menus/ (main, pause, new_game, load, settings),
            │                   tutorial.rs, hints.rs, dialogs.rs
            ├── overlays/       mod.rs (OverlayKind), fields.rs, network.rs, buildings.rs, transit.rs, palettes.rs
            ├── audio/          bank.rs (synthesize at startup), ambient_director.rs, sfx.rs, spatial.rs, mixer.rs
            ├── l10n.rs         Bevy resource around cb_l10n, language switching
            ├── save_io.rs      Save slots, autosave rotation, async file IO
            └── debug/          perf_overlay.rs, inspector.rs, sim_controls.rs (step/time scale/time+weather
                                override), gizmo_overlays.rs (graph, lanes, connectors, paths, spatial,
                                subdivision), procgen_gallery.rs, console.rs (log tail), debug_view.rs
                                (DebugView trait for pure-crate resources), screenshot.rs + screenshot_suites.ron
                                (suites, settle rule), screenshot_metrics.rs (luminance/clipping/NaN metrics,
                                semantic class pass), benchmark.rs (--benchmark, --no-gpu-work), smoke.rs
```

### Crate dependency graph

```
cb_core ◄── cb_geometry ◄── cb_terrain
   ▲            ▲   ▲
   │            │   └────── cb_network ◄── cb_traffic
   │            └────────── cb_procgen ◄─(cb_network for road meshes)
   ├── cb_l10n
   └── cb_audio
cb_sim  ──► cb_core, cb_geometry, cb_terrain, cb_network, cb_traffic, cb_procgen (massing only), bevy (ecs/app/state)
cb_game ──► everything + bevy (full) + bevy_egui
```

Rationale for the workspace split: ADR-0002.

## 4. Runtime topology

One Bevy `App`, one main `World`. The simulation runs in the main world inside two custom
schedules that the sim driver runs explicitly from `Update`. Rendering uses Bevy's pipelined
render sub-app: **extract runs on the main thread** at the end of each frame (a sync point that
waits for the render thread to finish the previous frame); prepare/queue/encode/submit then run
on the render thread while the main thread starts the next frame.

```
Main thread, frame N                                          Render thread
 First       input events, time, message buffer swap            (frame N−1: prepare, queue,
 PreUpdate   egui input capture, cursor ray                      sort, encode, upload, submit)
 Update
   GameSet::Input        action mapping (gated by egui wants from frame N−1), camera, picking,
                         tool state machines + previews → PlayerCommand
   GameSet::SimDrive     SimEdit once (structural work completes fully); then k × SimTick
   GameSet::ViewSync     visuals from sim changes, agent poses (α interpolation), mesh-job polling
   GameSet::Presentation lighting director, weather FX, overlays, audio director, toasts
 PostUpdate
   EguiPrimaryContextPass  all egui UI (reads sim, emits PlayerCommand/UiIntent → applied by
                           SimEdit in frame N+1: one-frame UI latency, ADR-0051)
   transforms, visibility, VisibilityRange
 Extract (main thread, sync point) ───────────────────────────► render thread frame N
```

Headless (tests, bench, future server): `App::new().add_plugins((MinimalPlugins, SimPlugins))`;
tests drive `SimEdit`/`SimTick` via `cb_sim::testing::Scenario`.

## 5. ECS schedules and system ordering

### 5.1 Sim schedules (cb_sim)

`SimEdit` runs **every frame, also while paused**. It applies player commands and all
*structural* derived updates, so construction, overlays and previews stay live when paused.
Structural work always **completes within the frame** before any `SimTick`; when it is
expensive, the driver defers ticks instead of splitting work (determinism, ADR-0042). Each
applied command records `apply_tick`.

```
SimEdit:
  EditSet::Commands     drain CommandQueue → validate → apply → record undo + apply_tick → CommandResult
  EditSet::Network      junction regeneration (dirty nodes), segment geometry, profiles, bridge spans,
                        ring (roundabout) detection
  EditSet::Terrain      apply/remove deformation stamps for dirty regions → TerrainChanged
  EditSet::Lanes        patch LaneGraph + LinkGraph + upstream horizons; bump graph version
  EditSet::Zoning       plot clipping/validation, subdivision of dirty plots → lots; lot access
  EditSet::Connectivity undirected components, directed SCCs, outside reachability, access flags
  EditSet::Coverage     coverage *reach* for dirty facilities; stop access for dirty stops/segments
  EditSet::Utilities    utility network components (structure only)
  EditSet::Index        spatial index updates (also via observers), dirty-region broadcast
```

`SimTick` has a fixed `dt = 0.1 sim s` (ADR-0042); it runs 0..N times per frame depending on
speed and day length (PERFORMANCE §2). Lower-rate work follows the phase table (§5.3).

```
SimTick:
  TickSet::Clock        advance tick; emit HourElapsed/DayElapsed/SeasonChanged
  TickSet::Weather      weather state machine, snow/wetness accumulation
  TickSet::Schedule     time wheel: due activities → TripRequest (≤ 2 000 per tick, overflow carried)
  TickSet::Trips        mode choice, routing within the cost-unit budget (parallel, request order),
                        flow model update, agent admission (sampling), transit boarding queues
  TickSet::Agents       spawn queues → junction control (grants, reservations) → IDM step
                        (64 partitions) → sequential pass (transfers, lane changes) → arrivals;
                        transit and service vehicles; stuck counters
  TickSet::SubRate      exactly the systems whose phase-table slot matches this tick
  TickSet::Progression  milestone checks, unlocks
  TickSet::Stats        statistics sampling, SimNotification emission
```

Rules:
- Every set is `.chain()`ed; both sim schedules use the single-threaded executor (ADR-0061),
  so systems run in one fixed order. `ambiguity_detection: LogLevel::Error` on both sim schedules;
  `auto_insert_apply_deferred` is disabled — explicit `ApplyDeferred` between sets.
- A test builds both schedules with `shuffle_seed` (cb_sim feature `debug_schedule` = bevy
  `debug`, enabled by `cb_game/dev`, so it runs in the `dev` CI leg) and asserts identical
  state hashes.
- Intra-system parallelism uses `ComputeTaskPool::scope` over `SIM_PARTITIONS = 64` fixed
  partitions; results merged in partition order (ADR-0043).

### 5.2 Game schedule usage (cb_game)

| Schedule / set | Contents | Order |
|---|---|---|
| `Update` / `GameSet::Input` | action mapping (egui-gated with previous frame's wants), camera controller, cursor ray & picking, tool state machines, previews (heavy preview work on async, ≤ 2 ms apply) | 1 |
| `Update` / `GameSet::SimDrive` | `run_sim_edit`, `run_sim_ticks` (exclusive) | 2 |
| `Update` / `GameSet::ViewSync` | terrain/road/building/prop chunk sync, agent pose pool (α), transit, icons, mesh-job polling, uploads within budget | 3 |
| `Update` / `GameSet::Presentation` | lighting director, env map (throttled), weather FX, overlays, audio director, toasts, tutorial triggers | 4 |
| `Update` / `GameSet::Debug` | perf overlay data, gizmo overlays (`dev`) | 5 |
| `PostUpdate` / `EguiPrimaryContextPass` | all egui UI (HUD, panels, menus, dialogs, tutorial, debug windows) | — |

Tool previews use the same pure validation functions as the sim commands.

### 5.3 Sub-rate phase table (1 200 ticks per game hour)

| System | Period | Offset (tick in hour) | Slices | Nominal per slice |
|---|---|---|---|---|
| Economy accrual (incl. goods profit, taxes, upkeep) | 1 game h | 0 | 4 | ≤ 0.15 ms |
| Demand update | 1 game h | 60 | 1 | ≤ 0.1 ms |
| Goods matching / delivery requests | 1 game h | 90 | 8 | ≤ 0.1 ms |
| Utilities balance + allocation | 1 game h | 120 | 4 | ≤ 0.6 ms |
| Coverage capacity allocation | 1 game h | 180 | 10 | ≤ 0.3 ms |
| Ground/water pollution diffusion (double-buffered) | 1 game h | 200 | 64 | ≤ 0.2 ms |
| Noise recompute | 1 game h | 300 | 64 | ≤ 0.3 ms |
| Land value | 1 game h | 400 | 64 | ≤ 0.2 ms |
| Job matching | 1 game h | 500 | 200 | ≤ 0.2 ms |
| Immigration / lot development | 1 game h | 720 | 4 | ≤ 0.2 ms |
| Dispatch (incidents → vehicles) | 20 ticks | 7 | 1 | ≤ 0.2 ms |
| Cycle detection slice | 100 ticks | 13 | 1 | ≤ 0.05 ms |
| Happiness | 1 game day | hour 2, 0 | 600 | ≤ 0.2 ms |
| Leveling / decline | 1 game day | hour 3, 0 | 600 | ≤ 0.2 ms |
| Weather forecast pre-roll | 1 game day | hour 0, 30 | 1 | ≤ 0.1 ms |
| Lifecycle / education | 1 season | day 0 hour 4 | 1 200 | ≤ 0.2 ms |

No tick runs more than one slot's work; a unit test asserts every tick's nominal sum ≤ 0.6 ms.

## 6. Simulation ↔ rendering data flow

```
Player input ─► Tool (preview: pure validation + cost estimate) ─► PlayerCommand
     ▲                                                                  │
     │                                                       CommandQueue (Messages)
     │                                                                  ▼
 egui UI ◄── read-only queries ◄── Sim state (entities, resources) ◄── SimEdit / SimTick
     ▲                                     │
     │                    change detection + dirty messages (SegmentChanged, LotsChanged,
     │                    TerrainChanged{rect}, BuildingChanged, …)
     │                                     ▼
     │                   View sync ─► async mesh jobs (cb_procgen, pure) ─► chunk meshes
     │                                     │
     └── notifications/audio cues ◄── SimNotification messages
```

- **Visual entities are separate from sim entities** (ADR-0005); view plugins keep
  `VisualRegistry<K>` (stable ID → visual entity or chunk slot).
- Buildings, yards, props, trees, road surfaces and decorations render as **merged per-chunk
  meshes** with LOD residency rings; a building is a slot with a parameter record in a per-chunk
  storage buffer (ADR-0049).
- Agents live in SoA stores in `cb_traffic`; the view pool keeps ≤ `max_visible_vehicles`
  pooled entities for agents inside the frustum + margin, writing interpolated poses.
- Overlays read sim fields/stats and upload compact textures/buffers.

## 7. State management

```rust
#[derive(States)]    enum AppState   { Boot, MainMenu, Loading, InGame, ProcgenGallery }
#[derive(SubStates)] enum InGameMenu { #[default] None, Pause, Settings, SaveLoad, Dialog } // source: InGame
#[derive(Resource)]  struct SimSpeed(Speed /* Paused | X1 | X2 | X4 */)   // driver API, independent of menus
#[derive(Resource)]  struct ActiveTool(ToolKind + per-tool state)         // presentation only
#[derive(Resource)]  struct ActiveOverlay(Option<OverlayKind>)
#[derive(Resource)]  struct TutorialState { step, completed, hints_enabled }
```

- `Boot`: paths, settings, l10n, icon atlas, crash-notice check; audio synthesized lazily.
- `MainMenu` (M15; until then the game boots straight into a new game or `--load`): backdrop city
  from `cb_sim::testing::citygen` (300 buildings, sim paused).
- `Loading`: world generation or save restore as a staged task with progress UI; **shader
  warm-up**: render one hidden instance per material × specialization (prepass, shadow, LOD
  dither on/off, overlay on/off, transparent) with an offscreen camera until the pipeline cache
  has nothing queued (cap 10 s); count logged and reported by `--benchmark`.
- `InGame`: sim schedules active; `InGameMenu != None` makes the driver treat speed as paused.
- `ProcgenGallery` (`dev` or `--procgen-gallery`): debug scene, no sim.
- Entities scoped with `DespawnOnExit(AppState::InGame)`; sim resources removed by
  `SimPlugins::teardown`.

## 8. Event flow

| Mechanism | Used for | Examples |
|---|---|---|
| `Message` (buffered) | cross-system/cross-schedule data flow | `PlayerCommand`, `CommandResult`, `TripRequest`, `SegmentChanged`, `TerrainChanged`, `LotsChanged`, `BuildingChanged`, `SimNotification`, `HourElapsed`, `DayElapsed` |
| Observers (`On<…>`) | immediate lifecycle/UI reactions | `On<Remove, Building>` → index removal; `On<Add, RoadSegment>` → index insert; menu widgets |

Rules:
- Sim → presentation only via messages, change detection and read-only queries;
  presentation → sim only via `PlayerCommand` and the driver API (`SimSpeed`, step requests,
  `DebugCommand`s in `dev`).
- `SimTick` runs 0..N times per frame → readers always drain all messages.
- Facts past tense (`FireStarted`), requests imperative (`RequestRoute`).
- `SimNotification { kind, severity, subject, args }` carries l10n keys + typed args only.

## 9. Command pattern and undo/redo

Design: ADR-0055 (framework in M2, history in M3).

```rust
pub enum PlayerCommand {               // serializable, stable IDs only, versioned with the save format
    BuildRoad(..), EditRoadGeometry(..), UpgradeRoad(..), SetSegmentStyle(..),
    DemolishSegments(.. /* cascade policy */), SetNodeControl(..), SetApproachControl(..),
    CreatePlot(..), EditPlot(..), SetSubdivision(..), DeletePlot(..),
    PlaceBuilding(..), MoveTownHall(..), DemolishBuilding(..), PlaceProp(..), RemoveProp(..),
    PlaceStop(..), RemoveStop(..), CreateLine(..), EditLine(..), DeleteLine(..), SetLineVehicles(..),
    SetTaxRate(..), SetServiceBudget(..), TakeLoan(..), RepayLoan(..), EmptyLandfill(..),
    SetSimConfig(..),                  // non-undoable (season length, weather toggles, traffic toggle)
    Undo, Redo,
}
pub trait Command {
    fn validate(&self, world: &World) -> Result<Validated, ValidationError>;
    fn apply(&mut self, world: &mut World) -> Result<Applied, CommandError>;   // records the effect log
    fn undo(&mut self, world: &mut World) -> Result<(), CommandError>;
    fn redo(&mut self, world: &mut World) -> Result<(), CommandError>;
    fn label(&self) -> L10nKey;
}
```

- Applied in `EditSet::Commands` in `CommandSeq` order; each records `apply_tick`.
- Effect logs hold created IDs, removed-object snapshots (`cb_sim::save::snapshot` types, M3),
  old values, money delta; undo restores **original IDs**.
- Undo window: 1 game day; refund 100 % in the first game hour, linearly to 25 % at expiry;
  evicted households go to `homeless_queue`. Depth 100; cleared on load/new game; policy
  commands not undoable. Failed preconditions → `undo.blocked` with reason.

## 10. Plugin breakdown

### cb_sim (all headless)

| Plugin | Owns |
|---|---|
| `SimCorePlugin` | schedules, driver, phase table, clock, catalog, ID allocators, RNG, command queue + history |
| `TerrainSimPlugin` | `Terrain`, stamps, `TerrainChanged`, tree-density field |
| `RoadNetworkPlugin` | nodes/segments, editor ops, junctions, profiles, bridges, rings, lane/link graph sync, connectivity + SCC, outside connections |
| `ZoningPlugin` | plots, lots, subdivision, clipping, lot development |
| `BuildingsPlugin` | buildings, construction, leveling, condition, access, massing, placed props |
| `CitizensPlugin` | population, households, jobs, schedules, trips, migration, happiness, education, lifecycle |
| `DemandPlugin` | RCI demand |
| `EconomyPlugin` | ledger, taxes, upkeep, costs, loans, goods market (ADR-0057) |
| `UtilitiesPlugin` | power, water, sewage, water pollution graph, garbage |
| `ServicesPlugin` | coverage reach + allocation, dispatch, incidents |
| `FieldsPlugin` | land value, pollution, noise rasters |
| `TrafficSimPlugin` | cb_traffic integration (router, cache, agents, flow model, admission); honors `traffic_enabled` |
| `TransitPlugin` | stops, lines, transit graph, ridership |
| `WeatherSimPlugin` | weather state, seasons, forecast, effect factors |
| `ProgressionPlugin` | milestones, unlocks, sandbox |
| `StatsPlugin` | statistics history, notifications |
| `SavePlugin` | snapshot/restore, validation, migration (file IO in cb_game) |

### cb_game

`BootstrapPlugin`, `PlatformPlugin`, `SettingsPlugin`, `L10nPlugin`, `InputPlugin`,
`CameraPlugin`, `PickingPlugin`, `RenderSetupPlugin` (world camera, offscreen render scale,
composite, post FX, presets, frame limiter), `MaterialsPlugin`, `TerrainViewPlugin`,
`WaterViewPlugin`, `RoadViewPlugin`, `BuildingViewPlugin`, `PropViewPlugin`, `AgentViewPlugin`,
`FxViewPlugin`, `LightingPlugin`, `WeatherFxPlugin`, `ToolsPlugin` (+ one sub-plugin per tool),
`UiPlugin` (theme, widgets, anchors, HUD/panels/menus), `OverlayPlugin`, `AudioPlugin` (absent
with `--no-audio` or no device), `SaveIoPlugin`, `TutorialPlugin`, `DebugPlugin` (`dev`).

## 11. Incremental update pipeline

| Change | Dirty propagation | Rebuilt |
|---|---|---|
| Segment added/removed/edited | `DirtyNodes`, `DirtySegments`, AABB → `DirtyRegion` | junction geometry; lanes/connectors/horizons; link graph patch; stamps; road chunks; plot clipping and lots referencing the segment |
| Terrain stamps changed | `TerrainChanged{rect}` | heights in rect; terrain chunk-LODs; scatter exclusion; tree density; pads |
| Plot edited | `DirtyPlots` | subdivision → lot diff (stable IDs), regenerate changed lots' buildings |
| Building level/condition changed | `BuildingChanged` | mesh regenerate (async) + chunk re-merge (debounced 250 ms) or param-buffer update only |
| Network topology changed | graph version++ | components, SCCs, route-cache eviction, landmark recompute (tick-sliced), coverage reach for touched facilities, utility components |
| Facility added/removed | `DirtyFacilities` | reach for that facility; next hourly allocation |

## 12. Threading model

- Game schedules: Bevy multi-threaded executor. Sim schedules: single-threaded executor
  (ADR-0061); sim parallelism is intra-system only.
- Intra-system parallelism over 64 fixed partitions (routing batches, vehicle step, reach
  Dijkstras, field slices); merges in partition order.
- Render-side procgen, chunk merges, terrain re-mesh, env map on `AsyncComputeTaskPool`
  (≤ 1 job per async thread, distance-prioritized, generation counters).
- IO on `IoTaskPool`; saving snapshots synchronously (≤ 10 ms) then serializes/compresses/
  writes async.

## 13. Save/load flow (ADR-0044)

1. `SavePlugin::snapshot(world) -> SaveData` — everything that influences future results (incl.
   base terrain, traffic and service state), sorted by stable ID (canonical; used for
   `state_hash`).
2. `cb_game::save_io` on the IO pool: header (magic, version, metadata, catalog hash,
   optional presentation state) + struct-map MessagePack body, LZ4, checksum; `*.tmp` + rename.
3. Load: header → body → migrate (`rmpv::Value` chain) → `validate_snapshot` → restore terrain
   (no regeneration) → spawn entities with original IDs → rebuild derived structures → restore
   traffic/service state onto rebuilt lanes → resume.

## 14. Settings and input

- `Settings` (graphics, display, render scale, frame cap, audio, bindings, language,
  accessibility, **day length** (pacing), lock time-of-day visuals, autosave) persisted as
  versioned RON at `AppPaths::settings_file` from M1 (ADR-0040); invalid fields fall back to
  defaults with a warning.
- Sim-affecting options live in `SimConfig` (saved with the city, changed via
  `SetSimConfig`): traffic agents on/off, `agent_cap`, weather on/off/lock, season length,
  sandbox, difficulty.
- Input: `InputMap` with contexts, `PRIMARY` modifier, logical-key mnemonics, trackpad gestures,
  drag precedence (ADR-0052).

## 15. Coding patterns (quick reference)

- Pure function core, thin ECS shell: systems gather inputs → call pure function →
  write outputs.
- Stable IDs in all persisted/command data; `Entity` only inside a frame.
- `SystemParam` bundles for recurring access patterns (`RoadQuery`, `BuildingQuery`).
- Errors: `thiserror` enums per crate; systems return `Result` and use Bevy's error
  handler (configured to log + count; release builds never abort on a system error).
- Feature flags: `dev` (debug tooling), `inspector` (bevy-inspector-egui when available,
  ADR-0031), `trace_tracy` (profiling, nightly only). CI never uses `--all-features` (ADR-0060).
- Math in sim-affecting code via `cb_core::math` (libm); no glam SIMD types; maps via
  `LookupMap`/`BTreeMap` (ADR-0043).
