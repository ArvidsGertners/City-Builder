# ADR-0013: Spatial index

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M2

## Context

Fast picking, overlap checks during placement, coverage and neighbor queries for roads,
plots, lots, buildings, stops, props and vehicles. Objects range from points (trees) to
long curves (roads) to large polygons (plots). Frequent inserts/removes; deterministic query
results.

## Options considered

1. **R*-tree (`rstar`)** — good for mixed sizes; bulk loading; updates moderately costly;
   extra dependency.
2. **Quadtree/BVH** — custom code, rebalancing complexity.
3. **Loose uniform spatial hash (chosen)** — O(1) insert/remove, trivially cache-friendly
   on a bounded 4 km map, simple to make deterministic; large objects split or multi-inserted.

## Decision

- `cb_core::spatial::SpatialHash<K: Ord + Copy>`: 32 m cells over the map bounds (dense
  128 × 128 `Vec<SmallVec<[Entry; 4]>>`), entries `(K, Aabb2)`, reverse map `K → cells`.
  Long road segments are inserted as ≤ 32 m sub-pieces; plots/buildings into all overlapped
  cells. Query results are de-duplicated and sorted by key.
- One instance per category (`SpatialIndices` resource; props inside `PropLayer`); vehicles
  use a per-tick rebuilt point grid inside `TrafficWorld`.
- Maintained by observers (`On<Add>`/`On<Remove>`) and dirty updates in `EditSet::Index`.
- It is an internal acceleration structure; gameplay never snaps to cells.

## Consequences

- No dependency; predictable performance; simple tests (brute-force comparison).
- Cell size is a tuning constant; extremely large objects (map-spanning) would be
  inefficient but don't exist in v1.
