//! Geometry kernel (no Bevy): cubic Béziers with arc-length tables, polylines, simple polygons,
//! robust predicates, polygon booleans (i_overlay), ear clipping, freehand curve fitting and
//! engine-agnostic `MeshData`; frontage-strip lot subdivision (ADR-0064) follows with the plot
//! tool.
//!
//! See `docs/ALGORITHMS.md` §1, §5, §9–§11. World plane coordinates are `Vec2(x, z)` (ADR-0033).

pub mod arc_length;
pub mod bezier;
pub mod boolean;
pub mod curve_fit;
pub mod mesh;
pub mod polygon;
pub mod polyline;
pub mod predicates;
pub mod triangulate;

pub use arc_length::ArcLengthLut;
pub use bezier::CubicBezier2;
pub use mesh::MeshData;
pub use polyline::{Polyline2, Polyline3};
