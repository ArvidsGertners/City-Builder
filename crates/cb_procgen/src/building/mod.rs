//! Buildings (ALGORITHMS §11, ADR-0029): typology generators on fitted footprints, roofs,
//! foundations. `zoned` picks the typology per zone and level: houses (R L1–L3), polygon
//! blocks (R L4–L5, C L1–L3) and industrial halls (I L1–L3).

pub mod block;
pub mod foundation;
pub mod house;
pub mod industrial;
pub mod roofs;
pub mod zoned;
