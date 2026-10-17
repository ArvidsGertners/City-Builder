//! Traffic and routing (no Bevy): segment-direction link graph, A* with ALT landmarks and a
//! deterministic cost-unit budget (ADR-0067), route cache, 1D lane-based agents with IDM, lane
//! plans and junction admission with reservations (ADR-0066), macroscopic flow model.
//!
//! Implemented from spike S-T, M5 (routing, flow) and M9 (agents); see `docs/ALGORITHMS.md`
//! §6, §12–§13.

pub mod cache;
pub mod flow;
pub mod graph;
pub mod router;
pub mod scc;
pub mod slots;
pub mod tree;

pub use cache::{Key, RouteCache};
pub use graph::{Graph, LinkSpec};
pub use router::{Costs, Landmarks, Route};
pub use tree::Tree;
