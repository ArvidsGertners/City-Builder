//! Buildings (DATA_MODEL §5) for M2: residential L1–L3 houses on fitted lot footprints, grown by
//! the Debug growth mode (empty lots grow after a delay; demand-driven growth arrives in M5),
//! their foundations and pad stamps, and removal when a lot change leaves them without room.

use bevy::ecs::component::Component;
use bevy::ecs::message::Message;
use bevy::ecs::world::World;
use cb_core::hash::{Domain, derive_seed};
use cb_core::ids::{BuildingId, LotId};
use cb_procgen::building::foundation::Foundation;
use cb_procgen::building::house::{self, HouseLayout};
use cb_terrain::stamps::priority;
use cb_terrain::{Stamp, StampShape};
use glam::Vec2;

use crate::catalog::{Catalog, ZoneKind};
use crate::config::{GrowthMode, SimConfig, WorldSeed};
use crate::ids::Ids;
use crate::terrain;
use crate::zoning::{Lot, Plot};

/// Lots grown per growth pass at most (bounds the work per tick).
pub const GROWTH_PER_PASS: usize = 40;
/// Growth pass period (ticks).
pub const GROWTH_PERIOD_TICKS: u64 = 20;

/// A building.
#[derive(Component, Clone, Debug)]
pub struct Building {
    pub(crate) id: BuildingId,
    pub(crate) lot: LotId,
    pub(crate) level: u8,
    pub(crate) seed: u64,
    pub(crate) footprint: [Vec2; 4],
    pub(crate) foundation: Foundation,
    pub(crate) height_m: f32,
    pub(crate) stamp: Option<u32>,
    pub(crate) built_tick: u64,
}

impl Building {
    /// Stable id.
    #[must_use]
    pub fn id(&self) -> BuildingId {
        self.id
    }
    /// Lot it stands on.
    #[must_use]
    pub fn lot(&self) -> LotId {
        self.lot
    }
    /// Level (1–3 in M2).
    #[must_use]
    pub fn level(&self) -> u8 {
        self.level
    }
    /// Generation seed.
    #[must_use]
    pub fn seed(&self) -> u64 {
        self.seed
    }
    /// Fitted footprint `[front-left, front-right, back-right, back-left]`.
    #[must_use]
    pub fn footprint(&self) -> [Vec2; 4] {
        self.footprint
    }
    /// Foundation (floor level, kind).
    #[must_use]
    pub fn foundation(&self) -> &Foundation {
        &self.foundation
    }
    /// Height above the floor (m).
    #[must_use]
    pub fn height_m(&self) -> f32 {
        self.height_m
    }
    /// Tick it was built.
    #[must_use]
    pub fn built_tick(&self) -> u64 {
        self.built_tick
    }
    /// The house massing (deterministic from footprint, level, seed).
    #[must_use]
    pub fn layout(&self) -> HouseLayout {
        house::layout(self.footprint, self.level, self.seed)
    }
}

/// Building changes for view sync.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildingChanged {
    /// Built or changed.
    Built(BuildingId),
    /// Removed.
    Removed(BuildingId),
}

/// Removes a building with its pad stamp.
pub(crate) fn remove_building(world: &mut World, id: BuildingId) {
    let Some(e) = world.resource_mut::<Ids>().buildings.remove(id) else {
        return;
    };
    let b = world.get::<Building>(e).cloned();
    world.despawn(e);
    if let Some(b) = b {
        if let Some(s) = b.stamp {
            terrain::remove_stamp(world, s);
        }
        if let Some(le) = world.get_resource::<Ids>().and_then(|i| i.lots.get(b.lot))
            && let Some(mut lot) = world.get_mut::<Lot>(le)
            && lot.building == Some(id)
        {
            lot.building = None;
        }
    }
    world.write_message(BuildingChanged::Removed(id));
}

/// After a lot changed: removes its building if the house no longer fits inside the lot.
pub(crate) fn check_fit(world: &mut World, lot: LotId) {
    let Some(le) = world.get_resource::<Ids>().and_then(|i| i.lots.get(lot)) else {
        return;
    };
    let Some(l) = world.get::<Lot>(le) else {
        return;
    };
    let Some(bid) = l.building else { return };
    let poly = l.polygon.clone();
    let Some(b) = world
        .get_resource::<Ids>()
        .and_then(|i| i.buildings.get(bid))
        .and_then(|e| world.get::<Building>(e))
    else {
        return;
    };
    let fits = b.layout().outlines().iter().flatten().all(|c| {
        cb_geometry::polygon::contains(&poly, *c)
            || cb_geometry::polygon::boundary_distance(&poly, *c) < 0.01
    });
    if !fits {
        remove_building(world, bid);
    }
}

/// Builds a house on a lot (Debug growth and citygen). Returns `None` if nothing fits.
pub(crate) fn grow_lot(world: &mut World, lot: LotId) -> Option<BuildingId> {
    let le = world.get_resource::<Ids>()?.lots.get(lot)?;
    let l = world.get::<Lot>(le)?.clone();
    if l.building.is_some() {
        return None;
    }
    let fp = l.footprint?;
    let world_seed = world.get_resource::<WorldSeed>().map_or(0, |s| s.0);
    let level_seed = derive_seed(world_seed, Domain::Growth, u64::from(lot.get()), 0);
    let level = 1 + (level_seed % 3) as u8;
    let seed = derive_seed(
        world_seed,
        Domain::Building,
        u64::from(lot.get()),
        u64::from(level),
    );
    let layout = house::layout(fp, level, seed);
    let foundation = {
        let g = terrain::ground(world);
        let ground = |p: Vec2| cb_network::validation::Ground::height(&g, p);
        layout.plan_foundation(&ground)
    };
    let Some(foundation) = foundation else {
        // Too steep: the lot stays green.
        if let Some(mut lm) = world.get_mut::<Lot>(le) {
            lm.footprint = None;
        }
        return None;
    };
    let id = world.resource_mut::<Ids>().buildings.allocate().ok()?;
    let stamp = foundation.pad.as_ref().map(|pad| {
        let sid = world.resource_mut::<Ids>().stamp();
        terrain::add_stamp(
            world,
            Stamp {
                id: sid,
                priority: priority::PAD,
                shape: StampShape::Pad {
                    polygon: pad.polygon.clone(),
                    target_h_m: pad.target_h_m,
                    falloff_m: pad.falloff_m,
                },
            },
        );
        sid
    });
    let tick = world
        .get_resource::<crate::SimClock>()
        .map_or(0, |c| c.tick().0);
    let b = Building {
        id,
        lot,
        level,
        seed,
        footprint: fp,
        height_m: layout.height_m(),
        foundation,
        stamp,
        built_tick: tick,
    };
    let e = world.spawn(b).id();
    world.resource_mut::<Ids>().buildings.insert(id, e);
    if let Some(mut lm) = world.get_mut::<Lot>(le) {
        lm.building = Some(id);
    }
    world.write_message(BuildingChanged::Built(id));
    Some(id)
}

/// `TickSet::SubRate` (every [`GROWTH_PERIOD_TICKS`]): Debug growth of empty residential lots.
pub(crate) fn growth(world: &mut World) {
    let tick = world
        .get_resource::<crate::SimClock>()
        .map_or(0, |c| c.tick().0);
    if !tick.is_multiple_of(GROWTH_PERIOD_TICKS) {
        return;
    }
    if world
        .get_resource::<SimConfig>()
        .is_none_or(|c| c.growth != GrowthMode::Debug)
    {
        return;
    }
    let delay = world
        .get_resource::<Catalog>()
        .map_or(600, Catalog::growth_debug_delay_ticks);
    let Some(ids) = world.get_resource::<Ids>() else {
        return;
    };
    let candidates: Vec<LotId> = ids
        .lots
        .iter()
        .filter_map(|(id, e)| {
            let l = world.get::<Lot>(e)?;
            let residential = world
                .get_resource::<Ids>()?
                .plots
                .get(l.plot)
                .and_then(|pe| world.get::<Plot>(pe))
                .is_some_and(|p| p.zone == ZoneKind::Residential);
            (residential
                && l.building.is_none()
                && l.footprint.is_some()
                && tick >= l.created_tick + delay)
                .then_some(id)
        })
        .take(GROWTH_PER_PASS)
        .collect();
    for lot in candidates {
        grow_lot(world, lot);
    }
}
