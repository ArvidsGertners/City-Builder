//! Road network geometry (no Bevy): road cross-sections, segment alignment, the vertical profile
//! solver, placement validation with crossing resolution and junction geometry (ADR-0063);
//! connectors, bridge spans and the lane graph (ADR-0008) follow in M3.
//!
//! See `docs/ALGORITHMS.md` §1–§6. World plane coordinates are `Vec2(x, z)` (ADR-0033).

pub mod bridge;
pub mod connectors;
pub mod control;
pub mod cross_section;
pub mod junction;
pub mod lane_graph;
pub mod lanes;
pub mod markings;
pub mod profile;
pub mod rings;
pub mod segment_geom;
pub mod validation;

pub use cross_section::{CrossSection, Dir, LaneKind, LaneSpec, RoadKind, Variant};
pub use profile::{Profile, ProfileInput};
pub use segment_geom::SegmentGeom;
pub use validation::{Invalid, RoadLimits};
