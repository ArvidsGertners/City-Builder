# Architecture Decision Records

Template: [0000-template.md](0000-template.md). Never edit the decision of an accepted ADR;
supersede it with a new one and update this index. References *inside* older ADRs to an ADR that
has since been superseded should be read as references to its successor (follow the
"Superseded by" link); successor ADRs state what they keep and what they change.

| ADR | Title | Status |
|---|---|---|
| [0001](0001-engine-bevy.md) | Engine — Bevy 0.20 | Accepted |
| [0002](0002-workspace-and-crates.md) | Cargo workspace and crate layout | Accepted |
| [0003](0003-rendering-approach.md) | Rendering approach — procedural meshes, chunk merging, instancing, LOD | Superseded by 0049 |
| [0004](0004-lighting-pipeline.md) | Lighting pipeline | Superseded by 0050 |
| [0005](0005-simulation-render-separation.md) | Simulation / rendering separation | Accepted |
| [0006](0006-fixed-timestep-simulation.md) | Fixed-timestep simulation and the time model | Superseded by 0042 |
| [0007](0007-road-representation.md) | Road representation — cubic Bézier alignment + vertical profile | Accepted |
| [0008](0008-lane-model.md) | Lane model | Accepted |
| [0009](0009-intersection-handling.md) | Intersection handling and traffic control | Superseded by 0045 |
| [0010](0010-traffic-simulation-model.md) | Traffic simulation model — agents, LOD, trip generation | Superseded by 0047 |
| [0011](0011-pathfinding-and-caching.md) | Pathfinding algorithm and route caching | Superseded by 0048 |
| [0012](0012-terrain-representation.md) | Terrain representation | Superseded by 0046 |
| [0013](0013-spatial-index.md) | Spatial index | Accepted |
| [0014](0014-plot-subdivision.md) | Plot subdivision algorithm | Accepted |
| [0015](0015-command-pattern-undo-redo.md) | Command pattern and undo/redo | Superseded by 0055 |
| [0016](0016-citizen-simulation-granularity.md) | Citizen simulation granularity | Accepted |
| [0017](0017-procgen-determinism.md) | Procedural generation determinism and seeds | Superseded by 0043 |
| [0018](0018-weather-daynight-sim-effects.md) | Weather and day/night effects on the simulation | Accepted |
| [0019](0019-audio-approach.md) | Audio approach — procedural synthesis | Accepted |
| [0020](0020-save-format-and-migration.md) | Save format, settings persistence and migration | Superseded by 0044 |
| [0021](0021-localization.md) | Localization approach | Accepted |
| [0022](0022-logging-and-crash-handling.md) | Logging and crash handling | Accepted |
| [0023](0023-crate-selection.md) | Crate selection and dependency policy | Accepted |
| [0024](0024-ui-framework.md) | UI framework — egui via bevy_egui | Superseded by 0051 |
| [0025](0025-stable-object-ids.md) | Stable object IDs and entity mapping | Accepted |
| [0026](0026-utility-distribution.md) | Utility distribution through the road network | Accepted |
| [0027](0027-service-coverage.md) | Service coverage measured along the road network | Superseded by 0056 |
| [0028](0028-scalar-fields.md) | Scalar fields as internal rasters | Accepted |
| [0029](0029-building-generation.md) | Procedural building generation approach | Accepted |
| [0030](0030-input-mapping.md) | Input and action mapping | Superseded by 0039 |
| [0031](0031-debug-tooling-and-inspector.md) | Debug tooling and inspector | Accepted |
| [0032](0032-toolchain-and-ci.md) | Toolchain pinning and CI | Superseded by 0038 |
| [0033](0033-coordinate-system-and-units.md) | Coordinate system and units | Accepted |
| [0034](0034-picking.md) | Cursor picking | Accepted |
| [0035](0035-public-transport-model.md) | Public transport model | Accepted |
| [0036](0036-gameplay-data-catalog.md) | Gameplay data catalog in embedded RON | Accepted |
| [0037](0037-platform-support.md) | Platform support — Windows, Linux, macOS on Apple Silicon | Accepted (determinism paragraph superseded by 0043) |
| [0038](0038-ci-and-release-three-platforms.md) | CI and release on Windows, Linux and macOS | Superseded by 0054 |
| [0039](0039-input-mapping-v2.md) | Input and action mapping v2 — platform conventions and trackpads | Superseded by 0052 |
| [0040](0040-per-os-file-locations.md) | Per-OS file locations | Accepted |
| [0041](0041-high-dpi-and-render-scale.md) | High-DPI displays and render scale | Superseded by 0053 |
| [0042](0042-time-model-v2.md) | Time model v2 — fixed sim/game ratio, day length as pacing | Accepted (supersedes 0006) |
| [0043](0043-determinism-v2.md) | Determinism v2 — seeds, partitions, portable math, maps | Accepted (supersedes 0017) |
| [0044](0044-save-format-v2.md) | Save format v2 — completeness rule, saved base terrain, validation | Accepted (supersedes 0020) |
| [0045](0045-junctions-v2.md) | Junction geometry and traffic control v2 | Accepted (supersedes 0009) |
| [0046](0046-terrain-v2.md) | Terrain representation v2 — continuous compositing, carved corridors | Accepted (supersedes 0012) |
| [0047](0047-traffic-v2.md) | Traffic simulation model v2 | Accepted (supersedes 0010) |
| [0048](0048-routing-v2.md) | Pathfinding and route caching v2 | Accepted (supersedes 0011) |
| [0049](0049-rendering-v2.md) | Rendering approach v2 — memory and entity budgets | Accepted (supersedes 0003) |
| [0050](0050-lighting-v2.md) | Lighting pipeline v2 | Accepted (supersedes 0004) |
| [0051](0051-ui-framework-v2.md) | UI framework v2 — egui scheduling, glyphs, structural tooltips | Accepted (supersedes 0024) |
| [0052](0052-input-v3.md) | Input and action mapping v3 | Accepted (supersedes 0039) |
| [0053](0053-high-dpi-v2.md) | High-DPI displays and render scale v2 | Accepted (supersedes 0041) |
| [0054](0054-ci-v3.md) | CI and release v3 — feature matrix, fast smoke, cross-OS determinism | Accepted (supersedes 0038) |
| [0055](0055-undo-redo-v2.md) | Command pattern and undo/redo v2 | Accepted (supersedes 0015) |
| [0056](0056-service-coverage-v2.md) | Service coverage v2 — incremental reach, global allocation | Accepted (supersedes 0027) |
| [0057](0057-goods-economy.md) | Goods economy and delivery trips | Accepted |
| [0058](0058-verification-process.md) | Verification process for a single GPU-less agent | Accepted |

Current decision per required topic (superseded ADRs remain for history): engine (0001),
rendering (0049), lighting (0050), sim/render separation (0005), fixed timestep & time model
(0042), road spline type (0007), lane model (0008), intersections (0045), traffic model incl.
agent count, LOD and trip generation (0047), pathfinding & caching (0048), terrain (0046),
spatial index (0013), plot subdivision (0014), undo/redo (0055), citizen granularity (0016),
procgen determinism (0043), weather & day/night sim effects (0018), audio (0019), save format &
migration (0044), localization (0021), logging (0022), crate selection (0023). Platforms:
support (0037), CI/release (0054), input (0052), file locations (0040), high-DPI (0053). Also:
UI (0051), coverage (0056), goods economy (0057), verification process (0058).

ADRs carrying an **Amendments** section (clarifications from the 2026-10-15 plan review that
do not change the decision): 0001, 0007, 0014, 0016, 0019, 0022, 0023, 0025, 0028, 0029, 0031,
0034, 0035, 0036, 0037, 0040.
