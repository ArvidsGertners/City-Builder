# ADR-0012: Terrain representation

- **Status:** Superseded by ADR-0046
- **Date:** 2026-10-15
- **Milestone:** M2

## Context

Seeded heightmap terrain (hills, valleys, lakes, rivers), materials, deformation by roads
(cut/fill embankments) and building pads, height queries anywhere, efficient rendering with
LOD and incremental updates, exact undo of deformation, saves that store seed +
modifications.

## Options considered

1. **Regular heightfield + deformation stamps (chosen)** — simple, fast queries, trivial LOD
   chunking; stamps make edits reversible and saveable.
2. **Adaptive TIN / mesh-based terrain** — fewer triangles, but complex edits and queries.
3. **Voxel / SDF terrain** — enables tunnels/overhangs (out of scope), heavy.
4. **Direct destructive height edits** — simple, but undo needs height snapshots and saves
   need full height arrays.

## Decision

- Heightfield 4096 × 4096 m at **2 m spacing** (2049² f32 ≈ 16 MB) for `base` and `height`;
  water surface field; material weights (u8 × 5); forest mask; min-max quadtree.
- `base` is regenerated from the seed on load (never saved). Deformations are **stamps**
  (Corridor, Pad, Clear, Paint) composited order-independently (highest priority/weight
  wins, ties by stamp ID). Stamps are saved; removing a stamp restores exactly.
- Height queries use the render-mesh triangulation (consistent with LOD0 surface).
- Water bodies are static (no fluid simulation); stamps never modify water in v1.
- Rendering: 128 m chunks × 4 LODs with skirts; chunks intersecting a `TerrainChanged`
  rect regenerate asynchronously.

## Consequences

- Exact undo and small saves; incremental remeshing; deterministic regeneration.
- 2 m resolution limits fine detail (fine for the stylized look); roads/plots never rely on
  sub-2 m terrain features because stamps define exact heights where it matters.
- Tunnels would need a different (or additional) representation — documented as future work.
