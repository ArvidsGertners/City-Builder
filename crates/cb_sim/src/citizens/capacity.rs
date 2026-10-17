//! Home and job capacities of buildings (GAME_DESIGN §4): units = floor area / m² per unit,
//! clamped per level; jobs split by required education. Only active or declining buildings
//! offer capacity — construction sites, abandoned buildings and ruins offer none.
//!
//! The table is derived (not saved) and refreshed at the start of each hourly pass; a building
//! is only re-measured when its level or condition changed since the last refresh.

use std::collections::BTreeMap;

use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use cb_core::ids::BuildingId;
use glam::Vec2;

use crate::buildings::{Building, BuildingKind, Condition};
use crate::catalog::{Catalog, ZoneKind};

/// Storey height used to count floors (m).
const STOREY_M: f32 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Entry {
    zone: ZoneKind,
    level: u8,
    habitable: bool,
    /// Households (R) or jobs (C/I).
    units: u16,
    jobs: [u16; 3],
    pos: Vec2,
}

/// Capacities of every zoned building (derived).
#[derive(Resource, Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Capacities {
    entries: BTreeMap<BuildingId, Entry>,
}

impl Capacities {
    /// Habitable homes with their household slots, in ID order.
    pub fn homes(&self) -> impl Iterator<Item = (BuildingId, u16)> + '_ {
        self.entries
            .iter()
            .filter(|(_, e)| e.habitable && e.zone == ZoneKind::Residential)
            .map(|(id, e)| (*id, e.units))
    }

    /// Open workplaces with their jobs by education, in ID order.
    pub fn jobs(&self) -> impl Iterator<Item = (BuildingId, [u16; 3])> + '_ {
        self.entries
            .iter()
            .filter(|(_, e)| e.habitable && e.zone != ZoneKind::Residential)
            .map(|(id, e)| (*id, e.jobs))
    }

    /// Household slots of a home (0 if not a habitable home).
    #[must_use]
    pub fn home_slots(&self, id: BuildingId) -> u16 {
        self.entries
            .get(&id)
            .filter(|e| e.habitable && e.zone == ZoneKind::Residential)
            .map_or(0, |e| e.units)
    }

    /// Jobs of a workplace by education (zero if closed).
    #[must_use]
    pub fn jobs_of(&self, id: BuildingId) -> [u16; 3] {
        self.entries
            .get(&id)
            .filter(|e| e.habitable && e.zone != ZoneKind::Residential)
            .map_or([0; 3], |e| e.jobs)
    }

    /// Level of a building in the table.
    #[must_use]
    pub fn level(&self, id: BuildingId) -> Option<u8> {
        self.entries.get(&id).map(|e| e.level)
    }

    /// Ground position (outline centroid) of a building in the table.
    #[must_use]
    pub fn pos(&self, id: BuildingId) -> Option<Vec2> {
        self.entries.get(&id).map(|e| e.pos)
    }

    /// Zone of a building in the table.
    #[must_use]
    pub fn zone(&self, id: BuildingId) -> Option<ZoneKind> {
        self.entries.get(&id).map(|e| e.zone)
    }
}

/// Floor area of a building: footprint area × floors.
#[must_use]
pub fn floor_area_m2(b: &Building) -> f32 {
    let footprint: f32 = b
        .outlines()
        .iter()
        .map(|o| cb_geometry::polygon::area(o))
        .sum();
    let floors = (b.height_m() / STOREY_M).round().max(1.0);
    footprint * floors
}

fn habitable(c: Condition) -> bool {
    matches!(c, Condition::Active | Condition::Declining)
}

fn measure(cat: &Catalog, zone: ZoneKind, b: &Building) -> Entry {
    let spec = &cat.zone(zone).capacity;
    let units = spec.units(b.level(), floor_area_m2(b));
    let jobs = if zone == ZoneKind::Residential {
        [0; 3]
    } else {
        spec.split_jobs(b.level(), units)
    };
    let pos = b
        .outlines()
        .first()
        .map_or(Vec2::ZERO, |o| cb_geometry::polygon::centroid(o));
    Entry {
        zone,
        level: b.level(),
        habitable: habitable(b.condition()),
        units,
        jobs,
        pos,
    }
}

/// Brings the table up to date with the buildings (new, removed, re-levelled, condition).
pub(crate) fn refresh(world: &mut World) {
    let mut table = world.remove_resource::<Capacities>().unwrap_or_default();
    if let Some(cat) = world.get_resource::<Catalog>().cloned() {
        // Dense component iteration, then sorted by id: the table never depends on ECS order.
        let mut next: Vec<(BuildingId, Entry)> = world
            .query::<&Building>()
            .iter(world)
            .filter_map(|b| {
                let BuildingKind::Zoned(zone) = b.kind() else {
                    return None;
                };
                let entry = match table.entries.get(&b.id()) {
                    Some(old) if old.level == b.level() && old.zone == zone => Entry {
                        habitable: habitable(b.condition()),
                        ..*old
                    },
                    _ => measure(&cat, zone, b),
                };
                Some((b.id(), entry))
            })
            .collect();
        next.sort_unstable_by_key(|e| e.0);
        table.entries = next.into_iter().collect::<BTreeMap<_, _>>();
    }
    world.insert_resource(table);
}
