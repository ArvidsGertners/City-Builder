//! Zoned buildings (ALGORITHMS §11, GAME_DESIGN §4): picks the typology for a zone and level and
//! dispatches to the house (R L1–L3, fitted rectangle), block (R L4–L5, C L1–L3, inset polygon)
//! or industrial (I L1–L3, fitted hall rectangle) generator.

use cb_geometry::MeshData;
use glam::Vec2;

use super::block::{self, BlockLayout};
use super::foundation::Foundation;
use super::house::{self, HouseLayout};
use super::industrial::{self, IndustrialLayout};

/// Zone of a lot (mirrors the sim's zone kinds; pure crates do not depend on the sim).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Zone {
    /// Residential (levels 1–5).
    Residential,
    /// Commercial (levels 1–3).
    Commercial,
    /// Industrial (levels 1–3).
    Industrial,
}

impl Zone {
    /// Highest level of the zone.
    #[must_use]
    pub const fn max_level(self) -> u8 {
        match self {
            Self::Residential => 5,
            Self::Commercial | Self::Industrial => 3,
        }
    }
}

/// The lot a building stands on.
#[derive(Clone, Copy, Debug)]
pub struct LotShape<'a> {
    /// Lot polygon (counter-clockwise).
    pub polygon: &'a [Vec2],
    /// Lot shrunk by the zone's setbacks.
    pub inset: &'a [Vec2],
    /// Frontage end points (along the road).
    pub front: (Vec2, Vec2),
    /// Fitted rectangle `[front-left, front-right, back-right, back-left]` (houses and halls).
    pub footprint: Option<[Vec2; 4]>,
}

/// Massing of a zoned building.
#[derive(Clone, Debug, PartialEq)]
pub enum Massing {
    /// R L1–L3.
    House(HouseLayout),
    /// R L4–L5, C L1–L3.
    Block(BlockLayout),
    /// I L1–L3.
    Industrial(IndustrialLayout),
}

/// Plans the building; `None` when the lot cannot hold this zone and level (the caller tries a
/// lower level or leaves the lot green).
#[must_use]
pub fn layout(zone: Zone, level: u8, lot: &LotShape, seed: u64) -> Option<Massing> {
    let level = level.clamp(1, zone.max_level());
    match zone {
        Zone::Residential if level <= 3 => lot
            .footprint
            .map(|fp| Massing::House(house::layout(fp, level, seed))),
        Zone::Residential | Zone::Commercial => {
            block::layout(zone, level, lot, seed).map(Massing::Block)
        }
        Zone::Industrial => lot
            .footprint
            .and_then(|fp| industrial::layout(fp, lot.inset, level, seed))
            .map(Massing::Industrial),
    }
}

impl Massing {
    /// Plan outlines of the main volumes (picking, overlap tests).
    #[must_use]
    pub fn outlines(&self) -> Vec<Vec<Vec2>> {
        match self {
            Self::House(h) => h.outlines().into_iter().map(|q| q.to_vec()).collect(),
            Self::Block(b) => vec![b.outline().to_vec()],
            Self::Industrial(i) => i.outlines(),
        }
    }

    /// Height above the floor level (m).
    #[must_use]
    pub fn height_m(&self) -> f32 {
        match self {
            Self::House(h) => h.height_m(),
            Self::Block(b) => b.height_m(),
            Self::Industrial(i) => i.height_m(),
        }
    }

    /// Foundation from the ground (`None`: too steep for this typology).
    #[must_use]
    pub fn plan_foundation(&self, ground: &dyn Fn(Vec2) -> f32) -> Option<Foundation> {
        match self {
            Self::House(h) => h.plan_foundation(ground),
            Self::Block(b) => b.plan_foundation(ground),
            Self::Industrial(i) => i.plan_foundation(ground),
        }
    }

    /// LOD0 and LOD1 meshes (yards are added by the caller for houses).
    #[must_use]
    pub fn mesh(
        &self,
        f: &Foundation,
        ground: &dyn Fn(Vec2) -> f32,
        slot: u16,
    ) -> (MeshData, MeshData) {
        match self {
            Self::House(h) => house::mesh(h, f, ground, slot),
            Self::Block(b) => block::mesh(b, f, slot),
            Self::Industrial(i) => industrial::mesh(i, f, slot),
        }
    }

    /// LOD0 triangle budget of this typology.
    #[must_use]
    pub fn tri_budget(&self) -> usize {
        match self {
            Self::House(_) => house::TRI_BUDGET,
            Self::Block(_) => block::TRI_BUDGET,
            Self::Industrial(_) => industrial::TRI_BUDGET,
        }
    }
}
