//! Terrain (no Bevy): 2 m heightfield over the 4 × 4 km map, deterministic world generation
//! (hills, valleys, rivers, lakes, outside connections), water surfaces, material weights and
//! the forest mask; deformation stamps, chunk meshes, raycast and scatter follow (ADR-0065).
//!
//! See `docs/ALGORITHMS.md` §7–§8. Every function here is a pure function of its inputs and the
//! world seed; transcendental math goes through `cb_core::math` (ADR-0043).

pub mod generate;
pub mod heightfield;
pub mod lakes;
pub mod materials;
pub mod noise;
mod par;
pub mod rivers;
pub mod water;

pub use generate::{OutsideConnection, World, WorldGenParams, WorldReport, generate};
pub use heightfield::{Grid, HeightGrid};
pub use materials::{MaterialWeights, TerrainMaterial};
pub use water::{WaterBodyKind, WaterField};

use thiserror::Error;

/// Errors from terrain generation and editing.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum TerrainError {
    /// Generation parameters are out of range.
    #[error("invalid world generation parameters: {0}")]
    InvalidParams(&'static str),
}
