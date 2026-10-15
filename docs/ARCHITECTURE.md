# Architecture

Status: **Accepted (Phase 1 plan)** · Engine: Bevy 0.20 · Rust stable, edition 2024

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
| [adr/README.md](adr/README.md) | Index of architecture decision records |

---

## 1. Goals and constraints

- Fully 3D, continuous world (4 × 4 km), **no grid or tile placement**; all visuals generated
  at runtime (no image/model assets).
- Simulation separated from rendering, runnable and testable headless, deterministic for a
  given seed + command stream on the same platform.
- 10k+ buildings, 50k+ citizens, 5k+ simultaneously simulated vehicles at 60 FPS on a
  ~2019 mid-range GPU (GTX 1660 Super / RX 5600 XT class) and a 6-core CPU.
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
│  Schedules: SimEdit (every frame, also when paused) · SimTick (fixed 20 Hz)  │
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
  sim state directly; it sends `PlayerCommand`s (ADR-0015).

## 3. Project structure

```
City-Builder/
├── Cargo.toml                  Workspace: members, [workspace.dependencies], [workspace.lints], profiles
├── rust-toolchain.toml         Pinned stable toolchain + rustfmt/clippy (ADR-0032)
├── rustfmt.toml                Formatting rules
├── clippy.toml                 Clippy config (msrv, thresholds)
├── .cargo/config.toml          Aliases (`cargo bench-sim`, `cargo gallery`), optional fast linker
├── .github/workflows/
│   ├── ci.yml                  fmt, clippy -D warnings, test (Linux + Windows), sim-only build, render smoke
│   └── release.yml             Tagged builds → zipped Windows/Linux artifacts on GitHub Releases
├── scripts/
│   └── screenshot.sh           Xvfb + lavapipe: run N frames, capture screenshots for art review
├── CLAUDE.md                   Agent/dev conventions (read first)
├── README.md                   Build, run, controls
├── docs/                       This documentation set, ADRs in docs/adr/
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
    │       ├── collections.rs  SortedVecMap, DenseSlotMap (generational), BitSet
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
    │   │   ├── economy/        ledger.rs, taxes.rs, costs.rs, upkeep.rs, loans.rs
    │   │   ├── utilities/      power.rs, water.rs, sewage.rs, garbage.rs
    │   │   ├── services/       coverage.rs, dispatch.rs, police.rs, fire.rs, health.rs, education.rs
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
    │   │   └── testing.rs      Scenario builder & assertions for tests (cfg(test) + feature "testing")
    │   ├── tests/              Headless integration tests (growth, determinism, save roundtrip,
    │   │                       migration, utilities, services, traffic multi-day, transit)
    │   │   └── fixtures/       Golden saves per format version
    │   └── examples/headless_bench.rs   Scripted benchmark city, prints timings
    └── cb_game/                Game binary (full Bevy + bevy_egui)
        └── src/
            ├── main.rs         CLI parsing, bootstrap (paths → logging → crash hook → App)
            ├── app.rs          Plugin assembly; headless/sim vs presentation plugins
            ├── states.rs       AppState, InGameMenu, transitions
            ├── bootstrap/      paths.rs, logging.rs (file + ring buffer), crash.rs (panic hook, reports), cli.rs
            ├── settings/       mod.rs (Settings resource), graphics.rs (presets → camera/render),
            │                   persist.rs (RON, versioned), apply.rs
            ├── input/          actions.rs (Action enum), bindings.rs (InputMap), contexts.rs
            ├── camera/         controller.rs (orbit/pan/zoom/snap), follow.rs, bounds.rs
            ├── picking.rs      Cursor ray → terrain hit → object queries (ADR-0034)
            ├── render/
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
                                subdivision), procgen_gallery.rs, screenshot.rs, console.rs (log tail)
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
schedules that the sim driver runs explicitly from `Update`. Rendering uses Bevy's
pipelined render sub-app as usual (extract → prepare → queue → render overlaps the next
main-world frame).

```
Frame N (main world)                                         Render world (frame N-1)
 First       input, time, message buffer swap                  extract/prepare/render
 PreUpdate   egui input, picking ray
 Update
   GameSet::Input       camera, action mapping, tool input
   GameSet::Ui          egui panels (EguiPrimaryContextPass) → PlayerCommand
   GameSet::SimDrive    run SimEdit once; run SimTick k times (k = accumulator / dt)
   GameSet::ViewSync    spawn/update visuals from sim changes, interpolate agents
   GameSet::Presentation lighting director, weather FX, overlays, audio, notifications
 PostUpdate  transform propagation, visibility, LOD (VisibilityRange), camera
```

Headless (tests, bench, future server): `App::new().add_plugins((MinimalPlugins, SimPlugins))`
and the test drives `SimEdit`/`SimTick` directly via `cb_sim::testing::Scenario`.

## 5. ECS schedules and system ordering

### 5.1 Sim schedules (cb_sim)

`SimEdit` runs **every frame, also while paused**. It applies player commands and all
*structural* derived updates, so construction, overlays and previews stay live when paused.

```
SimEdit:
  EditSet::Commands     drain CommandQueue → validate → apply → record undo → CommandResult
  EditSet::Network      road graph edits finalize: junction regeneration (dirty nodes),
                        segment geometry, vertical profiles, bridge spans
  EditSet::Terrain      apply/remove deformation stamps for dirty regions → TerrainChanged
  EditSet::Lanes        patch LaneGraph + LinkGraph for dirty segments/nodes; bump graph version
  EditSet::Zoning       plot validation, subdivision of dirty plots → lots; lot access
  EditSet::Connectivity network components, outside-connection reachability, road access flags
  EditSet::Coverage     recompute service coverage & stop access for dirty emitters/segments
  EditSet::Utilities    recompute utility network components (structure only)
  EditSet::Index        spatial index updates (also via observers), dirty-region broadcast
```

`SimTick` runs at a **fixed 20 Hz of sim time** (ADR-0006), 0..N times per frame depending
on speed. Systems that run at lower rates use run conditions (`every_n_ticks(n, phase)`),
with staggered phases so heavy work never lands on the same tick.

```
SimTick:
  TickSet::Clock        advance GameTime; emit HourElapsed/DayElapsed/SeasonChanged
  TickSet::Weather      weather state machine step, snow/wetness accumulation
  TickSet::Schedule     citizen time-wheel: due activities → TripRequest messages      (1 Hz)
  TickSet::Trips        mode choice, routing batch (parallel, deterministic), flow update,
                        agent admission (sampling), transit boarding queues
  TickSet::Agents       vehicle step: junction control → IDM → lane change → advance (20 Hz, all
                        agents, 1D lane state), transit & service vehicles, despawn on arrival
  TickSet::Dispatch     service/garbage dispatch, fire/crime/sickness incidents         (1 Hz)
  TickSet::Utilities    supply/demand balance and allocation per network component     (1/game-hour)
  TickSet::Fields       pollution/noise/land value incremental update (sliced)         (sliced, 1 Hz)
  TickSet::City         demand, lot development, building leveling/decline (sliced),
                        happiness (sliced), migration                                  (sliced)
  TickSet::Economy      hourly accrual of taxes/upkeep/fares; daily statement, loans
  TickSet::Progression  milestone checks, unlocks
  TickSet::Stats        statistics sampling, SimNotification emission
```

Rules:
- Every set is `.chain()`ed; systems within a set may run in parallel only if they have no
  conflicting access. `ScheduleBuildSettings { ambiguity_detection: LogLevel::Error }` on
  both sim schedules; a CI test builds the schedules with Bevy's `shuffle_seed` and the
  single-threaded executor and asserts identical state hashes (TESTING.md).
- Heavy per-tick work (routing batches, vehicle steps, coverage Dijkstras) is parallelized
  *inside* a system with `ComputeTaskPool::scope` over deterministic partitions; results are
  merged in stable order.
- Sim systems never use `Commands` for anything order-sensitive within a tick; structural
  spawns/despawns are applied at set boundaries (explicit `ApplyDeferred` points between
  sets) so later sets observe them.

### 5.2 Game schedule usage (cb_game)

| Set (in `Update`) | Contents | Order |
|---|---|---|
| `GameSet::Input` | action mapping, camera controller, cursor ray & picking, tool input state machines | 1 |
| `GameSet::Ui` | egui systems run in `EguiPrimaryContextPass`; they read sim state and push `PlayerCommand`s and `UiIntent`s; ordered before `SimDrive` by frame order | 2 |
| `GameSet::SimDrive` | `run_sim_edit`, `run_sim_ticks` (exclusive systems) | 3 |
| `GameSet::ViewSync` | `sync_terrain_chunks`, `sync_roads`, `sync_buildings`, `sync_props`, `sync_vehicles` (interpolation α), `sync_transit`, `sync_icons`, mesh job polling | 4 |
| `GameSet::Presentation` | lighting director, env map refresh, weather FX, overlays, audio director, notifications/toasts, tutorial triggers | 5 |
| `GameSet::Debug` | perf overlay, gizmo overlays (feature `dev`) | 6 |

Tool previews (ghost meshes, validation) run in `GameSet::Input` after picking and use the
same pure validation functions as the sim commands, so preview and execution agree.

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
     │                   View sync systems ─► async mesh jobs (cb_procgen, pure) ─► chunk meshes
     │                                     │
     └── notifications/audio cues ◄── SimNotification messages
```

- **Visual entities are separate from sim entities** (ADR-0005). A view plugin keeps a
  `VisualRegistry<K>` mapping stable IDs to visual entities (or to a render chunk), and
  reacts to `Added`/`Changed` sim components, removal observers and dirty messages.
- Buildings, props and terrain are rendered as **merged per-chunk meshes** with LOD; a
  building is a *slot* inside a chunk mesh with a per-building parameter record in a GPU
  storage buffer (condition, construction progress, highlight, overlay value, window seed).
  ADR-0003.
- Agents (vehicles, trams, pedestrians) live in SoA stores in `cb_traffic`. The view pool
  spawns at most `max_visible_vehicles` visual entities for agents inside the view
  frustum + margin, writes interpolated transforms (`prev_pose.lerp(pose, α)`), and recycles.
- Overlays read sim fields/stats and upload compact textures/storage buffers
  (field rasters, per-segment values, per-building values).

## 7. State management

```rust
#[derive(States)]           enum AppState    { Boot, MainMenu, Loading, InGame, ProcgenGallery }
#[derive(SubStates)]        enum InGameMenu  { #[default] None, Pause, Settings, SaveLoad, Dialog } // source: AppState::InGame
#[derive(Resource)]         struct SimSpeed  { Paused | X1 | X2 | X4 }            // independent of menus
#[derive(Resource)]         struct ActiveTool(ToolKind + per-tool state)        // presentation only
#[derive(Resource)]         struct ActiveOverlay(Option<OverlayKind>)
#[derive(Resource)]         struct TutorialState { step, completed, hints_enabled }
```

- `Boot`: paths, settings, l10n, icon atlas, audio bank synthesis (async), shader warmup.
- `MainMenu`: menu over an animated procedurally generated backdrop city (seeded, sim paused).
- `Loading`: world generation or save restore runs as a staged async task with progress UI;
  sim plugins are idle until `InGame`.
- `InGame`: sim schedules active; `InGameMenu != None` forces the sim driver to treat speed
  as paused (without changing the stored `SimSpeed`).
- `ProcgenGallery` (feature `dev` or `--procgen-gallery`): debug scene, no sim.
- Entities are scoped with `DespawnOnExit(AppState::InGame)` so leaving to the menu tears
  down the world cleanly; sim resources are removed by `SimPlugins::teardown`.

## 8. Event flow

Bevy 0.20 has two mechanisms; we use both deliberately:

| Mechanism | Used for | Examples |
|---|---|---|
| `Message` (buffered, `MessageWriter`/`MessageReader`) | Cross-system/cross-schedule data flow, many-to-many, consumed in a later set or frame | `PlayerCommand`, `CommandResult`, `TripRequest`, `SegmentChanged`, `TerrainChanged`, `LotsChanged`, `BuildingChanged`, `SimNotification`, `HourElapsed`, `DayElapsed` |
| Observers (`Event`/`EntityEvent`, `On<…>`) | Immediate, local reactions to lifecycle or UI triggers | `On<Remove, Building>` → spatial index removal; `On<Add, RoadSegment>` → index insert; UI button triggers in menus |

Rules:
- Sim → presentation communication is **only** via messages, change detection and
  read-only queries. Presentation → sim is **only** via `PlayerCommand` (plus the
  `SimSpeed`/debug-override resources, which are part of the driver API).
- Messages written in `SimTick` are read in `GameSet::ViewSync`/`Presentation` of the same
  frame. Because `SimTick` may run 0 or several times per frame, readers always drain
  everything (`MessageReader::read()`), never assume one message per frame.
- Facts are past tense (`FireStarted`), requests are imperative (`RequestRoute`).
- `SimNotification { kind, severity, subject: Option<StableRef>, args: SmallVec<L10nArg> }`
  carries localization keys and arguments, never strings.

## 9. Command pattern and undo/redo

Full design in ADR-0015; summary:

```rust
pub enum PlayerCommand {               // serializable, carries stable IDs only
    BuildRoad(BuildRoad), UpgradeRoad(UpgradeRoad), SetSegmentStyle(SetSegmentStyle),
    DemolishSegments(DemolishSegments /* cascade policy */), SetIntersectionControl(..),
    CreatePlot(..), EditPlot(..), SetSubdivision(..), DeletePlot(..),
    PlaceBuilding(..), DemolishBuilding(..),
    PlaceStop(..), RemoveStop(..), CreateLine(..), EditLine(..), DeleteLine(..), SetLineVehicles(..),
    SetTaxRate(..), SetServiceBudget(..), TakeLoan(..), RepayLoan(..),
    Undo, Redo,
}
pub trait Command {                    // implemented per undoable action
    fn validate(&self, world: &World) -> Result<Validated, ValidationError>;
    fn apply(&mut self, world: &mut World) -> Result<Applied, CommandError>;   // records what it did
    fn undo(&mut self, world: &mut World) -> Result<(), CommandError>;
    fn redo(&mut self, world: &mut World) -> Result<(), CommandError>;          // default: apply with recorded IDs
    fn label(&self) -> L10nKey;
}
```

- Commands are queued as messages and applied in `EditSet::Commands` in arrival order with
  a monotonically increasing `CommandSeq`, tagged with the current tick (future multiplayer
  lockstep/replay can reuse the stream).
- Applying records an inverse: created IDs, removed-object snapshots (serialized via the
  same snapshot types as the save system), money delta. Undo restores with **the original
  stable IDs** (IDs are never reused). Refund = recorded cost.
- `UndoHistory { undo: VecDeque<Box<dyn Command>>, redo: Vec<…>, depth: 100 }`; cleared on
  load and on new game. Policy changes (taxes, budgets, loans) are commands but not undoable.
- Undo checks preconditions (e.g. the road to remove has no newer dependents not covered by
  the snapshot); on failure it reports `undo.blocked` and leaves the stack intact.

## 10. Plugin breakdown

### cb_sim (all headless)

| Plugin | Owns |
|---|---|
| `SimCorePlugin` | schedules, driver, clock, catalog, ID allocators, RNG, command queue + history |
| `TerrainSimPlugin` | `Terrain` resource, stamps, `TerrainChanged` |
| `RoadNetworkPlugin` | node/segment entities, editor ops, junctions, profiles, bridges, lane/link graph sync, connectivity, outside connections |
| `ZoningPlugin` | plots, lots, subdivision, lot development |
| `BuildingsPlugin` | building entities, construction, leveling, condition, access, massing |
| `CitizensPlugin` | population tables, households, jobs, schedules, trips, migration, happiness, education |
| `DemandPlugin` | RCI demand |
| `EconomyPlugin` | ledger, taxes, upkeep, costs, loans |
| `UtilitiesPlugin` | power, water, sewage, garbage |
| `ServicesPlugin` | coverage, dispatch, police, fire, health, education |
| `FieldsPlugin` | land value, pollution, noise rasters |
| `TrafficSimPlugin` | cb_traffic integration (agents, flow model, stats); honors `traffic_enabled` |
| `TransitPlugin` | stops, lines, transit graph, ridership |
| `WeatherSimPlugin` | weather state, seasons, forecast, sim effect factors |
| `ProgressionPlugin` | milestones, unlocks, sandbox |
| `StatsPlugin` | statistics history, notifications |
| `SavePlugin` | snapshot/restore, migration (file IO is in cb_game) |

### cb_game

`BootstrapPlugin`, `SettingsPlugin`, `L10nPlugin`, `InputPlugin`, `CameraPlugin`,
`PickingPlugin`, `RenderSetupPlugin` (camera, post FX, quality presets), `MaterialsPlugin`,
`TerrainViewPlugin`, `WaterViewPlugin`, `RoadViewPlugin`, `BuildingViewPlugin`,
`PropViewPlugin`, `AgentViewPlugin`, `FxViewPlugin`, `LightingPlugin`, `WeatherFxPlugin`,
`ToolsPlugin` (+ one sub-plugin per tool), `UiPlugin` (+ HUD/panels/menus sub-plugins),
`OverlayPlugin`, `AudioPlugin`, `SaveIoPlugin`, `TutorialPlugin`, `DebugPlugin` (feature `dev`).

## 11. Incremental update pipeline

Every derived structure has an explicit dirty set; nothing is rebuilt globally in steady state.

| Change | Dirty propagation | Rebuilt |
|---|---|---|
| Segment added/removed/edited | `DirtyNodes` (both ends), `DirtySegments`, AABB → `DirtyRegion` | junction geometry for dirty nodes; lanes/connectors of touching segments; link graph patch; terrain stamps; road render chunks; lots whose frontage references the segment |
| Terrain stamps changed | `TerrainChanged{rect}` | heights in rect; terrain chunks intersecting rect; props in rect (re-filter); building pads (foundations) in rect |
| Plot edited | `DirtyPlots` | subdivision → lots diff (stable lot IDs kept where geometry matches), building regeneration for changed lots |
| Building level/condition changed | `BuildingChanged` | building mesh regenerate (async) → chunk re-merge (debounced 250 ms) or param-buffer update only (condition, progress) |
| Network topology changed | graph version++ | connectivity components (O(V+E)), route cache entries touching removed links, coverage for emitters whose reach intersects dirty segments, utility components |
| Service building added/removed | `DirtyEmitters` | bounded Dijkstra for that emitter only; building coverage samples in its reach |

## 12. Threading model

- Bevy multi-threaded executor for system-level parallelism.
- Intra-system data parallelism via `ComputeTaskPool::scope` with fixed, deterministic
  partitioning (by stable ID ranges or lane ranges) for: routing batches, vehicle micro
  steps (lanes partitioned so leaders are local), coverage Dijkstras, field updates.
- Render-side procgen and chunk merging on `AsyncComputeTaskPool`; results are polled in
  `ViewSync`. Non-deterministic completion order is fine there (visual only, keyed by ID +
  generation counter to drop stale results).
- File IO (save/load/autosave, logs) on `IoTaskPool` / non-blocking appender. Saving
  snapshots the sim synchronously (fast, in-memory) then serializes/compresses/writes async.

## 13. Save/load flow

1. `SavePlugin::snapshot(world) -> SaveData` (pure data: seeds, terrain stamps, entities by
   stable ID, resources, RNG/counter state, weather, clock, undo NOT included).
2. `cb_game::save_io` serializes on the IO pool: header (magic, format version, metadata
   for the load menu) + MessagePack body, LZ4-compressed, written to `*.tmp` then renamed.
3. Load: read header → migrate body (`rmpv::Value` chain v_n → v_latest) → deserialize →
   new world gen from seed (terrain base) → apply stamps → spawn entities with original IDs
   → rebuild all derived structures in one pass (lane graph, indices, coverage, fields
   recomputed or loaded) → vehicles regenerated from in-flight trips.
Details: ADR-0020.

## 14. Settings and input

- `Settings` resource (graphics, display, audio, input bindings, language, accessibility,
  gameplay toggles, autosave) persisted as versioned RON in `<data_dir>/settings.ron`;
  invalid/missing fields fall back to defaults with a logged warning (never a crash).
- Gameplay toggles that change sim behavior (traffic agents on/off, weather on/off,
  day length, season length) are copied into the sim's `SimConfig` resource and saved with
  the city; purely visual toggles (lock time-of-day visuals, reduce flashes) stay
  presentation-side.
- Input: one `InputMap` (`Action → Vec<Binding>`), contexts (Global, Camera, Tool, Ui),
  remappable in settings, conflict detection, defaults in `input/bindings.rs`. ADR-0030.

## 15. Coding patterns (quick reference)

- Pure function core, thin ECS shell: systems gather inputs → call pure function →
  write outputs.
- Stable IDs in all persisted/command data; `Entity` only inside a frame.
- `SystemParam` bundles for recurring access patterns (`RoadQuery`, `BuildingQuery`).
- Errors: `thiserror` enums per crate; systems return `Result` and use Bevy's error
  handler (configured to log + count; release builds never abort on a system error).
- Feature flags: `dev` (debug tooling), `inspector` (bevy-inspector-egui when available,
  ADR-0031), `trace_tracy` (profiling).
