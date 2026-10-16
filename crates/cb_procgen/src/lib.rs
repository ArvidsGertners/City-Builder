//! Procedural content (no Bevy): pure, deterministic generators
//! `generate(input, params, seed) -> MeshData` for buildings fitted to irregular lots, yards,
//! vehicles, vegetation, props, road/junction/bridge meshes and the SDF icon atlas (ADR-0029,
//! ADR-0049).
//!
//! All meshes use the 20 B world vertex format of `cb_geometry::MeshData` (position, sRGB colour
//! with AO in alpha, packed slot/surface attribute — [`surface`]). See `docs/ALGORITHMS.md` §11.

pub mod building;
pub mod junction_mesh;
pub mod paint;
pub mod palette;
pub mod road_mesh;
pub mod surface;
pub mod vegetation;
pub mod yard;
