//! Traffic and routing (no Bevy): segment-direction link graph, A* with ALT landmarks and a
//! deterministic cost-unit budget (ADR-0048), route cache, 1D lane-based agents with IDM, lane
//! plans and junction admission with reservations (ADR-0045, ADR-0047), macroscopic flow model.
//!
//! Implemented from spike S-T, M5 (routing, flow) and M9 (agents); see `docs/ALGORITHMS.md`
//! §6, §12–§13.
