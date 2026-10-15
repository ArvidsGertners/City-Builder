# Architecture Decision Records

Template: [0000-template.md](0000-template.md). Never edit the decision of an accepted ADR;
supersede it with a new one and update this index.

| ADR | Title | Status |
|---|---|---|
| [0001](0001-engine-bevy.md) | Engine — Bevy 0.20 | Accepted |
| [0002](0002-workspace-and-crates.md) | Cargo workspace and crate layout | Accepted |
| [0003](0003-rendering-approach.md) | Rendering approach — procedural meshes, chunk merging, instancing, LOD | Accepted |
| [0004](0004-lighting-pipeline.md) | Lighting pipeline | Accepted |
| [0005](0005-simulation-render-separation.md) | Simulation / rendering separation | Accepted |
| [0006](0006-fixed-timestep-simulation.md) | Fixed-timestep simulation and the time model | Accepted |
| [0007](0007-road-representation.md) | Road representation — cubic Bézier alignment + vertical profile | Accepted |
| [0008](0008-lane-model.md) | Lane model | Accepted |
| [0009](0009-intersection-handling.md) | Intersection handling and traffic control | Accepted |
| [0010](0010-traffic-simulation-model.md) | Traffic simulation model — agents, LOD, trip generation | Accepted |
| [0011](0011-pathfinding-and-caching.md) | Pathfinding algorithm and route caching | Accepted |
| [0012](0012-terrain-representation.md) | Terrain representation | Accepted |
| [0013](0013-spatial-index.md) | Spatial index | Accepted |
| [0014](0014-plot-subdivision.md) | Plot subdivision algorithm | Accepted |
| [0015](0015-command-pattern-undo-redo.md) | Command pattern and undo/redo | Accepted |
| [0016](0016-citizen-simulation-granularity.md) | Citizen simulation granularity | Accepted |
| [0017](0017-procgen-determinism.md) | Procedural generation determinism and seeds | Accepted |
| [0018](0018-weather-daynight-sim-effects.md) | Weather and day/night effects on the simulation | Accepted |
| [0019](0019-audio-approach.md) | Audio approach — procedural synthesis | Accepted |
| [0020](0020-save-format-and-migration.md) | Save format, settings persistence and migration | Accepted |
| [0021](0021-localization.md) | Localization approach | Accepted |
| [0022](0022-logging-and-crash-handling.md) | Logging and crash handling | Accepted |
| [0023](0023-crate-selection.md) | Crate selection and dependency policy | Accepted |
| [0024](0024-ui-framework.md) | UI framework — egui via bevy_egui | Accepted |
| [0025](0025-stable-object-ids.md) | Stable object IDs and entity mapping | Accepted |
| [0026](0026-utility-distribution.md) | Utility distribution through the road network | Accepted |
| [0027](0027-service-coverage.md) | Service coverage measured along the road network | Accepted |
| [0028](0028-scalar-fields.md) | Scalar fields as internal rasters | Accepted |
| [0029](0029-building-generation.md) | Procedural building generation approach | Accepted |
| [0030](0030-input-mapping.md) | Input and action mapping | Accepted |
| [0031](0031-debug-tooling-and-inspector.md) | Debug tooling and inspector | Accepted |
| [0032](0032-toolchain-and-ci.md) | Toolchain pinning and CI | Accepted |
| [0033](0033-coordinate-system-and-units.md) | Coordinate system and units | Accepted |
| [0034](0034-picking.md) | Cursor picking | Accepted |
| [0035](0035-public-transport-model.md) | Public transport model | Accepted |
| [0036](0036-gameplay-data-catalog.md) | Gameplay data catalog in embedded RON | Accepted |

Mapping to the brief's required topics: engine (0001), rendering (0003), lighting (0004),
sim/render separation (0005), fixed timestep (0006), road spline type (0007), lane model
(0008), intersections (0009), traffic model incl. agent count, LOD and trip generation
(0010), pathfinding & caching (0011), terrain (0012), spatial index (0013), plot
subdivision (0014), undo/redo (0015), citizen granularity (0016), procgen determinism
(0017), weather & day/night sim effects (0018), audio (0019), save format & migration
(0020), localization (0021), logging (0022), crate selection (0023).
