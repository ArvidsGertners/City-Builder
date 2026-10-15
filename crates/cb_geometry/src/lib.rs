//! Geometry kernel (no Bevy): cubic Béziers with arc-length tables, polylines, simple polygons,
//! robust predicates, polygon booleans (i_overlay), ear clipping, inscribed rectangles,
//! frontage-strip lot subdivision (ADR-0014), cross-section sweeps and engine-agnostic
//! `MeshData`.
//!
//! Implemented from Phase S (spikes S-J, S-P) and M2 onwards; see `docs/ALGORITHMS.md` §1, §5,
//! §9–§11.
