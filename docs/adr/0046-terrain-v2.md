# ADR-0046: Terrain representation v2 — continuous compositing, carved corridors, saved base

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M2; validated by spike S-G
- **Supersedes:** ADR-0012 (keeps the 2 m heightfield, stamps, chunks and LODs)

## Context

Review findings G-G2–G-G4, G-S2, C-5: "highest-weight-wins" compositing jumps between
overlapping stamps (cliffs between embankments); a 1.5 m flat shoulder is smaller than the
2.83 m cell diagonal so terrain pokes through road edges, worse at LOD1+; overpass
embankments buried lower roads; base terrain regenerated from the seed made saves fragile.

## Decision

- Base heights, water surface and forest mask are **saved** (ADR-0044) and generated with the
  portable math facade (ADR-0043).
- Each stamp has a **core** (corridor carriageway + shoulder, pad polygon, abutment) with an
  exact target height, and a **blend zone** with weight `w ∈ (0, 1)` falling off with a
  smoothstep along the side slope (max 1:1.5 fill, 1:1 cut).
- **Compositing** (order-independent, C0-continuous):
  - inside any core: `H = target` of the highest-priority core (road > abutment > pad);
    overlapping road cores of different roads only occur at junctions (equal node height) —
    ties by stamp ID;
  - elsewhere: `H = base + max_k(max(0, w_k (t_k − base))) + min_k(min(0, w_k (t_k − base)))`
    (fills raise via max, cuts lower via min).
- Road cores are **carved 0.15 m below** the road surface; the flat shoulder is ≥ 3.5 m
  (≥ cell diagonal + 0.5 m); road meshes carry 0.6 m skirts.
- Terrain LOD1+ chunks downsample with **min** inside corridor-core cells (never above the
  road), regular decimation elsewhere.
- Any span where a new road's corridor overlaps another corridor with Δh ≥ 4 m becomes a bridge
  span (min length = crossed width + 2 × 6 m), so the upper road never stamps over the lower.
- Water bodies stay immutable in v1.

## Consequences

- No cliffs, no terrain through roads at any LOD (verified by the spike's automated sampler),
  exact undo (stamp removal), portable saves.
