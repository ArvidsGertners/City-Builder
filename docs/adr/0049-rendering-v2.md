# ADR-0049: Rendering approach v2 — memory and entity budgets

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M2 (formats, chunking, residency), M4 (10k scale); validated by spike S-Render
- **Supersedes:** ADR-0003 (keeps forward PBR with `ExtendedMaterial` + WESL, `MeshData`,
  128 m render chunks, buildings merged per chunk with a per-building param buffer,
  `VisibilityRange` LOD, runtime-generated textures only)

## Context

Review findings D9–D12, G-W14, A56: flat shading by duplicated vertices plus all LOD0 meshes
resident put buildings at ≈ 1.2 GB and merged trees at 1.2–1.9 GB — beyond the 2.5 GB VRAM /
3.5 GB unified budgets; renderable entity counts (road decorations, vehicles) were unbudgeted;
crossfade bands double draw cost.

## Decision

- **Vertex format** for all procedural world meshes: indexed, shared vertices; `position`
  f32x3 (12 B) + `color` unorm8x4 (4 B; alpha = baked AO) + `packed` u32 (slot u16 + surface
  flags u16) = **20 B/vertex**. No normal attribute: **flat shading from screen-space
  derivatives** (`normalize(cross(dpdx(p_view), dpdy(p_view)))`) in the forward and prepass
  fragment shaders (prepass normals feed SSAO). Terrain keeps smooth normals (vertex
  attribute) since it is smooth-shaded by art direction.
- **Residency rings**: LOD0 meshes exist only for chunks within 1.3× the LOD0 range (evicted
  beyond 1.6×); LOD1 within 1.3× its range; LOD2+ everywhere. Procgen meshes use
  `RenderAssetUsages::RENDER_WORLD` (no main-world copy). A 256 MB LRU of per-building
  quantized meshes avoids regeneration on chunk re-merge; misses regenerate asynchronously.
- **Props** (trees, rocks, grass, road decorations, lamps, light pools) are merged per chunk per
  LOD into the props mesh with the same format; no per-prop entities.
- **Renderable entity budget ≤ 15 000**: chunk-LOD entities for terrain/buildings/props/roads
  (resident only), visible vehicle pool (≤ `max_visible_vehicles`, default 3 000), pedestrians
  (≤ 800), one dynamic billboard mesh for problem icons, one dynamic headlight-pool mesh, FX
  emitters (≤ 200).
- **LOD bands** ≤ 10 % of the range; shadow casters switch hard at band centers.
- **Uploads** ≤ 8 MB per frame; async jobs prioritized by camera distance, at most one job per
  async thread in flight; stale results dropped by generation counter.
- Memory budgets per category are tracked in PERFORMANCE.md and reported by `--benchmark`.

## Consequences

- ≈ 5× less vertex memory than ADR-0003's approach; GPU budgets hold on both reference machines
  (to be measured). Spike S-Render proves derivative normals + custom vertex layout work with
  Bevy 0.20's prepass/shadow pipelines on Vulkan (lavapipe) and through MSL translation.

## Amendments

- 2026-10-15 (spike S-Render): validated on lavapipe. Locations: position 0, color 5
  (`Unorm8x4`, albedo linearized on the CPU, AO in alpha multiplied into rgb in the vertex
  stage), packed 8 (`Uint32`). `specialize` sets the vertex buffer layout and pushes
  `VERTEX_COLORS` so Bevy's `VertexOutput` carries the color; slot lookups happen in the vertex
  stage. 1 000 triangles ≈ 17.7 KiB.
- 2026-10-16 (M2 procgen): the packed `u32` is laid out as bits 0–15 slot, bits 16–19 surface
  kind (`cb_procgen::surface::Surface`: solid, roof, window, foliage, trunk, paved, marking,
  ground — drives night windows, snow on roofs, season tint, wetness), bits 20–31 a per-kind id
  (window id for the night lighting pattern). Procgen meshes carry sRGB colour bytes; the game
  linearizes them with a 256-entry table when building GPU meshes.
