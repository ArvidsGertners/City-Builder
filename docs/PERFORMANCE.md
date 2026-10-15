# Performance Targets & Budgets

Status: **Accepted (Phase 1 plan, revised after the 2026-10-15 plan review)**. Budgets are
engineering estimates; spikes S-T, S-G and S-Render and the benchmark runs validate them.
Budgets are for the **worst frame**, not averages: players see hitches, not means.

## 1. Targets

| Target | Value |
|---|---|
| Map | 4 × 4 km (16 km²), terrain 2 m resolution |
| Buildings | ≥ 10 000 (zoned + service), unique procedural meshes |
| Citizens | ≥ 50 000 (≈ 20 000 households) |
| Simulated vehicles | ≥ 5 000 simultaneous agents (`SimConfig::agent_cap`, default 5 000, new-game option 2 000–12 000) |
| Frame rate | 60 FPS at 1× **and 4×** (day length 24 min), max city, p95 frame ≤ 16.6 ms, p99.9 ≤ 33 ms |
| Reference PC = **minimum spec** (user's machine) | AMD Ryzen 5 7500F (6C/12T Zen 4), GTX 1660 Ti (6 GB), 16 GB DDR5-6000 **single stick (single-channel, ≈ 48 GB/s)**, 1080p, "High" preset |
| Reference Mac | MacBook Air 13" M4 (assumed, to be confirmed; 4P + 6E cores, 10-core GPU, fanless), 16 GB unified, built-in 2560 × 1664 Retina at Auto render scale (2/3 → 1.89 MP rendered, ADR-0053), "Medium" preset, measured after 10 min warm |
| Memory | PC: ≤ 2 GB RAM, ≤ 1.5 GB VRAM; Mac: ≤ 3.5 GB unified total |
| Load | New map ≤ 6 s; load max save ≤ 10 s |
| Hitches | autosave snapshot ≤ 10 ms on the main thread; edit frames (large bulldoze/commit) ≤ 20 ms; no frame > 33 ms at p99.9 in the 60 s benchmark |
| Latency | input → preview mesh in the main world ≤ 1 frame (photon latency ≈ 2–3 frames with pipelined rendering); UI-issued commands apply next frame (ADR-0051) |

The PC above is both the user's machine and the minimum spec, so its measurements gate directly.
Single-channel DDR5 halves memory bandwidth: SoA hot loops (traffic, fields) and chunk merges are
profiled for bandwidth, not just instruction count. The Mac model is assumed (B12).

## 2. Frame model

| Thread | Work |
|---|---|
| Main | `First`/`PreUpdate` input → `Update` (GameSet::Input, SimDrive = SimEdit + k × SimTick, ViewSync, Presentation) → `PostUpdate` (egui pass, transforms, visibility, `VisibilityRange`) → **Extract** (sync point with the render thread) |
| Render | prepare, specialize/queue, sort/batch, GPU preprocessing dispatch, graph encoding, uploads, submit/present (pipelined: overlaps the next main-thread frame) |
| GPU | shadow, prepass, opaque, SSAO, transparent, post, egui |
| Workers | Bevy compute pool (intra-system parallel sim work, `SIM_PARTITIONS = 64`), async pool (procgen meshes, chunk merges, terrain re-mesh, env map), IO pool (saves, logs) |

Ticks per frame (`dt = 0.1 sim s`, ADR-0042), at 60 FPS:

| Day length | 1× | 2× | 4× | Worst frame at 4× (driver cap) |
|---|---|---|---|---|
| 48 min | 0.17 | 0.33 | 0.67 | 1 |
| 24 min (default) | 0.33 | 0.67 | 1.33 | 2 |
| 16 min | 0.50 | 1.00 | 2.00 | 3 (settings warn) |

## 3. Main thread budget (ms, worst frame, max city)

| Work | PC 1× | PC 4× | M4 1× | M4 4× | Notes |
|---|---|---|---|---|---|
| Input, action map, camera | 0.2 | 0.2 | 0.15 | 0.15 | |
| Picking + tool preview (steady) | 0.5 | 0.5 | 0.4 | 0.4 | drag frames: heavy preview work (junction/profile/subdivision) on async with ≤ 2.0 ms main-thread apply |
| SimEdit (steady / edit frame) | 0.1 / ≤ 3.0 | 0.1 / ≤ 3.0 | same | same | on edit frames ≥ 3 ms the driver defers one tick (ADR-0042) |
| SimTick (ticks × 3.0) | 3.0 | 6.0 | 3.0 | 6.0 | per-tick table §6 |
| ViewSync (≤ 3 000 visible agent poses, chunk swaps, job polling) | 1.0 | 1.0 | 0.9 | 0.9 | poses computed in parallel |
| Presentation (lighting director, weather FX, overlays, audio director, toasts) | 0.5 | 0.5 | 0.5 | 0.5 | |
| egui pass (PostUpdate) | 1.5 | 1.5 | 1.3 | 1.3 | large tables virtualized |
| Engine PostUpdate (transforms, visibility, VisibilityRange) | 1.2 | 1.2 | 1.0 | 1.0 | requires ≤ 15 000 renderables (§9) |
| Extract | 1.5 | 1.5 | 1.3 | 1.3 | |
| Schedule overhead | 0.5 | 0.5 | 0.4 | 0.4 | ~500 systems |
| **Sum** | **10.0** | **13.0** | **8.95** | **11.95** | |
| **Headroom to 16.6** | 6.6 | 3.6 | 7.65 | 4.65 | |

## 4. Render thread budget (ms)

| Work | PC | M4 (Metal) | Notes |
|---|---|---|---|
| Prepare (view/light uniforms, per-chunk param buffers, agent instance data) | 1.5 | 1.6 | |
| Specialize + queue (opaque, prepass, 3 shadow cascades) | 1.5 | 1.6 | cached specializations |
| Sort, batch, GPU-preprocess dispatch | 1.0 | 1.2 | |
| Graph encoding (≈ 3k main + 3k prepass + 3 × 1.5k shadow draws) | 4.0 | 5.0 | Metal emulates multi-draw; measured on M4 |
| egui render | 0.4 | 0.5 | |
| Uploads (≤ 8 MB/frame) | 1.5 | 1.0 | unified memory cheaper |
| Submit, present | 0.5 | 0.6 | |
| **Sum** | **10.4** | **11.5** | headroom 6.2 / 5.1 |

## 5. GPU budget (ms)

| Pass | PC High (1080p, 2.07 MP) | M4 Medium (Auto ≤ 2.2 MP) | Notes |
|---|---|---|---|
| Shadows (PC 3 × 2048², M4 2 × 2048², far cascade LOD1 casters) | 2.5 | 2.0 | |
| Depth + normal prepass | 1.5 | 1.7 | derivative normals (ADR-0049) |
| GPU culling / preprocessing | 0.3 | 0.3 | |
| Opaque forward | 4.5 | 5.0 | |
| SSAO (PC full res Medium, M4 half res) | 1.0 | 0.9 | |
| Water, sky, transparents, precipitation, light pools | 1.0 | 1.2 | light-pool overdraw capped |
| Bloom, tonemap, SMAA | 0.8 | 1.0 | M4: 4 bloom mips |
| Upscale composite + CAS (scale < 1 only) | 0 | 0.3 | |
| egui (native resolution) | 0.3 | 0.4 | |
| Env-map regeneration (≤ 1 per 2 s, amortized) | 0.1 | 0.1 | |
| **Sum** | **12.0** | **12.9** | headroom 4.6 / 3.7 (covers ~15–25 % thermal throttling) |

## 6. Per-tick budget (`SimTick`, wall clock, ≤ 3.0 ms)

| System | Budget | Rule |
|---|---|---|
| Clock, weather | 0.02 | |
| Time wheel (due activities) | 0.10 | ≤ 2 000 activities per tick; overflow carried |
| Trips, mode choice, admission | 0.25 | |
| Routing (searches + cache refresh + reroutes) | 0.65 hard cap | 50 000 cost units/tick (settled nodes; ≈ 0.65 ms nominal, ≤ 0.8 ms p99); FIFO overflow (ADR-0048) |
| Flow model | 0.10 | incremental per link event |
| Vehicle step: parallel / sequential pass | 0.30 / 0.40 | 64 fixed partitions; sequential = lane transfers, grants, lane changes |
| Junction control | 0.30 | active junctions only |
| Transit dwell/boarding, service vehicles | 0.10 | |
| Stuck counters + cycle-detection slice | 0.05 | |
| One sub-rate slot (phase table) | 0.60 hard cap | ARCHITECTURE §5.3 |
| ApplyDeferred / structural | 0.08 | |
| Stats, notifications | 0.05 | |
| **Total** | **3.0** | |

## 7. Async budgets

- At most one job per async thread in flight; jobs prioritized by camera distance; stale
  results dropped by generation counter.
- Building generation 1–3 ms each, chunk re-merge 3–8 ms, terrain chunk-LOD re-mesh 1–2 ms;
  a level-up wave of 200 buildings completes within ~1 s on two async threads.
- Main-thread cost of applying results is inside ViewSync (§3); uploads ≤ 8 MB/frame (§4).

## 8. Memory budget (max city)

| Category | CPU RAM | VRAM / unified GPU share | How |
|---|---|---|---|
| Terrain data (base u16, height f32, water, materials, min-max) | 60 MB | — | |
| Terrain meshes (LOD0 ~60 resident chunks, LOD1 ~200, LOD2/3 all) | — | 35 MB | residency rings |
| Buildings (LOD0 ~3k resident × ~22 KB, LOD1 all 10k × 3 KB, LOD2) | 256 MB LRU of quantized per-building meshes | 110 MB | 20 B indexed vertices, `RENDER_WORLD` usage |
| Props/trees (LOD0 ring, LOD1 ring, LOD2 cross-quads for all ~150k trees, grass near camera) | — | 90 MB | merged per chunk |
| Roads, junctions, bridges, decorations | 40 MB | 40 MB | |
| Sim (entities ~25k, population, trips, fields, economy, stats) | 60 MB | — | |
| Lane/link graphs, landmarks, route cache, traffic SoA | 45 MB | — | |
| Render targets (HDR, prepass, SSAO, bloom, shadows 3 × 2048²) at 1080p / M4 Auto | — | 200 MB | |
| Audio bank (lazy, 25–40 MB) | 40 MB | — | |
| Bevy/wgpu/egui overhead | 400 MB | 150 MB | |
| **Total** | **≈ 0.95 GB** (budget 2 GB) | **≈ 0.63 GB** (budget 1.5 GB) | Mac unified ≈ 1.6 GB (budget 3.5 GB) |

## 9. Entity budget (renderable, ≤ 15 000)

Terrain chunk-LODs (resident) ≤ 1 500 · building/props/road chunk-LODs (resident) ≤ 5 000 ·
visible vehicles ≤ 3 000 (High; Ultra 6 000 counts against headroom) · pedestrians ≤ 800 ·
FX emitters ≤ 200 · dynamic meshes (icons, headlight pools, previews) ≤ 20 · overlays ≤ 100.
Sim entities (nodes, segments, plots, lots, buildings, stops, lines ≈ 30k) carry no render
components and cost only sim work.

## 10. How the design meets the targets

| Risk area | Design response | ADR |
|---|---|---|
| 10k unique buildings | per-chunk merged meshes, 20 B indexed vertices with derivative normals, param buffer for state, LOD rings, LRU of per-building meshes | 0049 |
| 150k+ trees and props | merged per chunk per LOD, cross-quads at distance, residency rings | 0049 |
| Terrain 4.2 M samples | chunked LODs with skirts, min-downsampling in corridors, async re-mesh | 0046 |
| 5k+ agents | 1D lane state, SoA, fixed partitions; poses only for visible agents; pooled instanced entities | 0047 |
| Trip volume at 50k citizens | time wheel, routed estimates, cost-unit routing budget, cache keyed per commuter, sampling beyond the cap | 0042, 0047, 0048 |
| Sub-rate spikes | phase table with fixed offsets and slices, 0.6 ms slot cap | 0042 |
| Lighting with many lamps | emissive + merged light pools; one shadowed light | 0050 |
| Retina / 4K fill rate | Auto render scale by GPU class, Medium preset on Apple M-series | 0053 |
| Edit hitches | async preview work, tick deferral on edit frames | 0042 |

## 11. Scalability levers (graphics settings — never change sim results)

| Setting | Low | Medium | High | Ultra |
|---|---|---|---|---|
| Shadow map / cascades | 1024 / 2 | 2048 / 2 | 2048 / 3 | 4096 / 4 |
| SSAO | off (baked AO only) | half res | full res Medium | High (+ optional TAA) |
| LOD distance scale | 0.6 | 0.8 | 1.0 | 1.4 |
| Max visible vehicles | 1 000 | 2 000 | 3 000 | 6 000 |
| Weather particles | 25 % | 50 % | 100 % | 100 % + splashes |
| Anti-aliasing | FXAA | SMAA | SMAA | SMAA / TAA |
| Bloom mips | off | 4 | 6 | 6 |
| Render scale | Auto | Auto | Auto | Auto |

## 12. Apple Silicon specifics

- Tile-based GPU: few full-screen passes, half-res SSAO on Medium, fewer bloom mips; avoid
  render-target ping-pong.
- Heterogeneous cores: fixed 64 partitions keep efficiency cores from straggling.
- ProMotion: frame cap default 60 via the own limiter (ADR-0053).
- Verified by `--benchmark bench_city` on the user's M4 (☑ᵤ) at M2, M4, M9, M11, M16.

## 13. Measurement

- `cargo run -p cb_game --release -- --benchmark <scene> [--no-gpu-work]`: deterministic scene
  (seed + command script; benchmark save once M5 exists), 60 s scripted camera path (overview →
  dense center → street level → industry; night/rain variants from M11/M12). Report (logs
  folder + stdout, copy-paste friendly): per-thread p50/p95/p99/p99.9/max, per-set CPU timings,
  per-pass GPU timings where timestamp queries exist, ticks/frame, routing units and cache hit
  rate, agents (total/visible), renderable entities, triangles, memory by category, render
  scale, adapter. `--no-gpu-work` renders to a 64 × 64 target with shadows/SSAO off so CPU
  numbers in the container are not polluted by lavapipe.
- `cargo run -p cb_sim --release --example headless_bench -- --city <preset> [--no-traffic]`:
  citygen-built city (M4: 10k buildings; M5+: 50k citizens), per-system p50/p95/p99.9 tick
  times, routing stats, memory.
- Bench scenarios: `bench_autosave_max` (snapshot p99), `bench_road_commit` (100 random 200 m
  roads; command → visible mesh frames).
- `tracing` spans on every set and heavy system; Tracy via `--features trace_tracy` (nightly).
- CI runs a reduced headless bench (informational artifact); judgments use local container
  numbers (CPU) and user runs (GPU) recorded in PROGRESS.md.
