# ADR-0003: Rendering approach — procedural meshes, chunk merging, instancing, LOD

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M2 (terrain, roads, houses), M4 (chunk merging at scale)

## Context

All visuals are generated at runtime; no textures or models. Targets: 10k+ unique buildings,
100k+ trees/props, 5k+ vehicles at 60 FPS on a 2019 mid-range GPU. The look is low-poly,
flat-shaded, vertex-colored. Per-building state must be visible (construction, condition,
lit windows, overlays, highlight) without re-meshing on every change.

## Options considered

1. **One entity + mesh per building/prop** — simplest, uses Bevy batching; but 100k+
   entities cost extraction time every frame, LOD per object multiplies entities, unique
   meshes defeat instancing.
2. **Bevy meshlets (virtual geometry)** — requires offline preprocessing and is
   experimental; not suited to constantly regenerated procedural meshes.
3. **Merged meshes per render chunk + per-object parameter buffer; instancing for repeated
   meshes** — few entities, good batching, LOD per chunk, state via storage buffer.
4. Deferred renderer — more dynamic lights, but we don't need many real lights (ADR-0004)
   and water/glass need forward anyway; forward keeps memory lower.

## Decision

- Bevy forward (clustered) PBR with **`ExtendedMaterial<StandardMaterial, Ext>`** materials
  and small WESL shaders: `WorldMaterial` (buildings/props/roads: vertex colors, flat
  normals, per-object params, window emissive, snow/wetness/season, condition, construction
  reveal with matching prepass discard), `TerrainMaterial` (material weights, slope bands,
  snow, wetness, cloud shadows, overlay field sampling), `WaterMaterial`, `SkyMaterial`,
  `PrecipitationMaterial`, `LightPoolMaterial` (additive), `BillboardMaterial` (icons),
  `OverlayRibbonMaterial`.
- `cb_geometry::MeshData` (positions, normals, colors, custom attributes, indices) is
  converted to `bevy::Mesh` in `cb_game`. Custom vertex attributes: `ATTRIBUTE_OBJECT_SLOT`
  (u16→u32), `ATTRIBUTE_SURFACE_FLAGS` (window id/roof/snow-capable/foliage/sway weight).
- **Render chunks** of 128 m (internal partition, not gameplay). Buildings and their yards,
  trees and props are merged per chunk per material class per LOD. Per-building parameters
  (`condition, construction_progress, highlight, overlay_value, window_seed, flags`) live in
  a per-chunk `ShaderBuffer` indexed by `ATTRIBUTE_OBJECT_SLOT`. Re-merge is async and
  debounced (250 ms), only for dirty chunks.
- **Instancing**: vehicles, street lamps, road trees, transit stops, bridge piers share
  meshes/materials so Bevy's automatic batching instances them; per-instance color via
  `MeshTag` → palette lookup.
- **LOD**: `VisibilityRange` with dithered crossfade per chunk-LOD entity; buildings LOD0
  (full) / LOD1 (massing + painted window bands) / LOD2 (merged massing per 4 chunks);
  trees full / simplified / cross-quad; terrain 4 LODs with skirts. Far shadow cascade
  uses LOD1 casters.
- **Runtime-generated textures only**: SDF icon atlas, overlay field rasters, small noise
  LUTs if a shader needs them. Everything else is geometry + vertex colors + shader math.

## Consequences

- Very few draw calls and entities; per-object state changes are a buffer write.
- Chunk re-merge latency (≤ ~300 ms) for structural changes; mitigated by showing the
  building immediately as a temporary standalone entity until the chunk merge lands.
- Bevy mesh picking is not usable (merged meshes) → own picking (ADR-0034).
- Custom shaders must stay compatible with Bevy's prepass/shadow pipelines; covered by the
  render smoke test.
