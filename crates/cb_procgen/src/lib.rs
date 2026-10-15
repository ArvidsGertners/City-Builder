//! Procedural content (no Bevy): pure, deterministic generators
//! `generate(input, params, seed) -> MeshData` for buildings fitted to irregular lots, yards,
//! vehicles, vegetation, props, road/junction/bridge meshes and the SDF icon atlas (ADR-0029,
//! ADR-0049).
//!
//! Implemented from M2; see `docs/ALGORITHMS.md` §11, §24.
