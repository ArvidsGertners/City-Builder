//! Terrain (no Bevy): 2 m heightfield over the 4 × 4 km map, deterministic world generation
//! (hills, valleys, rivers, lakes), order-independent deformation stamps with carved corridors
//! (ADR-0046), water surfaces, material weights, scatter and min-max-quadtree ray marching.
//!
//! Implemented from spike S-G and M2; see `docs/ALGORITHMS.md` §7–§8, §23.
