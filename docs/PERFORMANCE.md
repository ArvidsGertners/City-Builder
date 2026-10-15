# Performance Targets & Budgets

Status: **Accepted (Phase 1 plan)**.

## 1. Targets

| Target | Value |
|---|---|
| Map | 4 × 4 km (16 km²), terrain 2 m resolution |
| Buildings | ≥ 10 000 (zoned + service), unique procedural meshes |
| Citizens | ≥ 50 000 (≈ 20 000 households) |
| Simulated vehicles | ≥ 5 000 simultaneous agents (default cap 5 000; High 8 000, Ultra 12 000) |
| Frame rate | 60 FPS at 1080p, "High" preset, 1× speed; ≥ 50 FPS at 4× |
| Reference HW | GTX 1660 Super / RX 5600 XT, 6-core CPU (i5-9600K / Ryzen 5 3600), 16 GB RAM, SSD |
| Memory | ≤ 3 GB RAM, ≤ 2.5 GB VRAM at max city |
| Load | New map ≤ 6 s; load max-size save ≤ 10 s; autosave hitch ≤ 30 ms |
| Input latency | Tool preview update ≤ 1 frame; road commit visible ≤ 100 ms |

Low preset target: 60 FPS on GTX 1050 Ti-class at 1080p with reduced shadows/AO/LOD
distances (best effort).

## 2. Frame budget (16.6 ms, reference HW, max city, High, 1×)

### CPU (main + workers; wall-clock on the main schedule)

| Work | Budget | Notes |
|---|---|---|
| Input, camera, picking, tool previews | 0.7 ms | ray march < 20 µs; preview validation incremental |
| egui UI | 1.5 ms | panels only when open; tables virtualized |
| SimEdit (steady state, no edits) | 0.2 ms | dirty-driven |
| SimTick share per frame | 4.0 ms at 1× (≈ 1.2 ticks/frame), 9 ms at 4× | parallel inside systems |
| View sync (agents interpolation, chunk updates) | 1.5 ms | ≤ 3 000 visible agents |
| Lighting/weather/overlay/audio | 0.6 ms | env-map regen async |
| Bevy extract + prepare (render world) | 4.0 ms | pipelined, overlaps next frame |
| Headroom | ~4 ms | |

### SimTick (per tick, 50 ms sim time) at max city

| System | Budget/tick | How |
|---|---|---|
| Vehicle step (5 000 agents) | 1.2 ms | 1D lane state, SoA, ~150 ns/agent, 6-way parallel |
| Junction control | 0.3 ms | only junctions with requests |
| Trip generation + mode choice | 0.3 ms | time wheel → only due activities |
| Routing batch | 0.8 ms avg (cap 300 searches) | cache hit rate > 90 %, ALT A* ~0.2–0.6 ms/search, parallel |
| Sliced systems (fields, leveling, happiness, coverage refresh) | 0.6 ms | fixed slices per tick |
| Economy/utilities/demand | < 0.1 ms amortized | hourly |

### GPU (High, 1080p)

| Pass | Budget |
|---|---|
| Shadow cascades (3 × 2048², far cascade uses LOD1) | 2.5 ms |
| Depth + normal prepass | 1.5 ms |
| Opaque forward (terrain, buildings, roads, props, vehicles) | 4.5 ms |
| SSAO (Medium) | 1.0 ms |
| Water, sky, transparent FX, precipitation | 1.0 ms |
| Post (bloom, tonemapping, SMAA) | 0.8 ms |
| egui | 0.3 ms |
| Total | ≈ 11.6 ms |

Triangle budget visible: ≤ 4 M (LOD0 near ~2 M, LOD1/LOD2 rest); draw calls ≤ 3 000
(batched by material via Bevy's GPU preprocessing / multi-draw indirect).

## 3. How the design meets the targets

| Risk area | Design response | ADR |
|---|---|---|
| 10k unique building meshes | per-chunk merged meshes per material, building params in a storage buffer, LOD1 massing, LOD2 merged per 4 chunks, `VisibilityRange` crossfade; state changes don't re-mesh | 0003 |
| 100k+ trees/props | merged per chunk, 3 LODs incl. cross-quad billboards, vertex-shader wind | 0003 |
| Terrain 4.2 M samples | 1 024 chunks × 4 LODs with skirts; edits re-mesh only touched chunks; async | 0012 |
| 5k+ vehicles | 1D lane model for all agents; world pose computed only for visible agents (≤ max visible, default 3 000); instanced meshes with `MeshTag` colors | 0010 |
| Trips at 50k citizens | event-driven time wheel; route cache with variants; trip sampling beyond agent cap with macro background flows | 0010, 0011, 0016 |
| Pathfinding spikes | per-tick search cap, stale-while-revalidate cache, background landmark recompute | 0011 |
| Coverage recompute | bounded Dijkstra per emitter, only dirty emitters | 0027 |
| Fields | 512² rasters, sliced updates (64 rows/s) | 0028 |
| Lighting with many lamps | no real point lights for lamps (emissive + fake light pools, bloom); optional ≤ 32 nearest real point lights on Ultra | 0004 |
| Shadows | single shadowed directional light, 3 cascades, far cascade bias, LOD1 casters beyond 400 m | 0004 |
| Allocation churn | buffers reused (`Local<Vec<_>>`), SoA stores, no per-tick `String`/`Box` | CLAUDE.md |
| Save/load | snapshot in memory, serialize/compress on IO pool, derived data rebuilt in parallel | 0020 |

## 4. Scalability levers (settings)

| Setting | Low | Medium | High | Ultra |
|---|---|---|---|---|
| Shadow map / cascades | 1024 / 2 | 2048 / 3 | 2048 / 3 | 4096 / 4 |
| SSAO | off (vertex AO only) | Low | Medium | High + TAA option |
| LOD distance scale | 0.6 | 0.8 | 1.0 | 1.4 |
| Max visible vehicles | 1 000 | 2 000 | 3 000 | 6 000 |
| Agent cap (sim) | 2 000 | 4 000 | 5 000 | 8 000–12 000 |
| Weather particles | 25 % | 50 % | 100 % | 100 % + splashes |
| Anti-aliasing | FXAA | SMAA | SMAA | SMAA / TAA |
| Bloom | off | on | on | on |

## 5. Measurement

- `tracing` spans on every sim set and heavy system; `--features trace_tracy` for Tracy.
- In-game perf overlay (feature `dev`): frame time graph, CPU set timings, sim tick time,
  routing searches/cache hit rate, agents (total/visible), entity counts, triangles drawn,
  GPU timings (wgpu timestamp queries where supported).
- `cargo run -p cb_sim --release --example headless_bench`: builds a scripted large city
  (grid-free organic road generator + plots + forced growth) to 10k buildings / 50k
  citizens / 5k agents and reports per-system p50/p95/p99 tick times. Run before and
  after perf-sensitive changes; results logged in PROGRESS.md for each milestone from M9.
- CI runs a reduced headless bench (1k buildings) and uploads the timing report as an
  artifact. It is informational only (shared runners are too noisy to gate on); regressions
  are judged against the local reference runs recorded in PROGRESS.md.
