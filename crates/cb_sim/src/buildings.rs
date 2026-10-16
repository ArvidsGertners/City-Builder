//! Buildings (DATA_MODEL §5): zoned buildings of every zone and level on their lots (Debug
//! growth until demand-driven growth arrives in M5) and civic buildings on fixed parcels, with
//! foundations and pad stamps, condition (construction, decline, abandonment, ruin) and removal
//! when a lot change leaves a building without room.
//!
//! A building keeps a snapshot of its site (lot shape or civic parcel frame) so its massing is
//! regenerated deterministically from `(kind, level, seed, site)` without the lot (undo, saves);
//! meshes are never stored (ADR-0029).

use bevy::ecs::component::Component;
use bevy::ecs::message::Message;
use bevy::ecs::world::World;
use cb_core::hash::{Domain, derive_seed};
use cb_core::ids::{BuildingId, LotId};
use cb_core::time::TICKS_PER_GAME_HOUR;
use cb_geometry::rect_fit::EdgeKind;
use cb_procgen::building::civic::{self, CivicKind, CivicLayout};
use cb_procgen::building::foundation::Foundation;
use cb_procgen::building::zoned::{self, LotShape, Zone};
use cb_procgen::paint::Frame;
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

/// What a building is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BuildingKind {
    /// Grown on a zoned lot.
    Zoned(ZoneKind),
    /// Placed civic building (services, utilities, town hall).
    Civic(CivicKind),
}

/// Building condition (DATA_MODEL §5). Shown through the per-object shader parameters, so a
/// change never rebuilds the mesh, except into and out of `Ruined` (a ruin generator).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Condition {
    /// Under construction since `start_tick` for `duration_ticks`.
    Constructing {
        /// Tick construction started.
        start_tick: u64,
        /// Construction time (ticks).
        duration_ticks: u64,
    },
    /// In use.
    Active,
    /// Run down (problems for a while).
    Declining,
    /// Empty and boarded up.
    Abandoned,
    /// Collapsed (after fire or long abandonment).
    Ruined,
}

impl Condition {
    /// Construction progress 0–1 at `tick` (1 when not constructing).
    #[must_use]
    pub fn progress(&self, tick: u64) -> f32 {
        match *self {
            Self::Constructing {
                start_tick,
                duration_ticks,
            } => {
                let done = tick.saturating_sub(start_tick);
                (done as f64 / duration_ticks.max(1) as f64).clamp(0.0, 1.0) as f32
            }
            _ => 1.0,
        }
    }
}

/// A zoned building's lot as it was when the building was planned.
#[derive(Clone, Debug, PartialEq)]
pub struct ZonedSite {
    /// Lot polygon (counter-clockwise).
    pub polygon: Vec<Vec2>,
    /// Setback inset.
    pub inset: Vec<Vec2>,
    /// Frontage end points.
    pub front: (Vec2, Vec2),
    /// Fitted rectangle (houses, halls).
    pub footprint: Option<[Vec2; 4]>,
    /// Lot edge classes (yards).
    pub edges: Vec<EdgeKind>,
}

impl ZonedSite {
    fn of(lot: &Lot) -> Self {
        Self {
            polygon: lot.polygon().to_vec(),
            inset: lot.inset().to_vec(),
            front: lot.front(),
            footprint: lot.footprint(),
            edges: lot.edges().to_vec(),
        }
    }

    /// As the generator's lot input.
    #[must_use]
    pub fn shape(&self) -> LotShape<'_> {
        LotShape {
            polygon: &self.polygon,
            inset: &self.inset,
            front: self.front,
            footprint: self.footprint,
        }
    }
}

/// Where a building stands.
#[derive(Clone, Debug, PartialEq)]
pub enum Site {
    /// On a zoned lot.
    Zoned(ZonedSite),
    /// On a civic parcel (frame: origin at the front-left corner, `ex` along the road).
    Civic(Frame),
}

/// Regenerated massing of a building.
#[derive(Clone, Debug, PartialEq)]
pub enum Massing {
    /// Zoned typology.
    Zoned(zoned::Massing),
    /// Civic building.
    Civic(CivicLayout),
}

impl Massing {
    /// World outlines of the ground volumes.
    #[must_use]
    pub fn outlines(&self) -> Vec<Vec<Vec2>> {
        match self {
            Self::Zoned(m) => m.outlines(),
            Self::Civic(c) => c.outlines(),
        }
    }

    /// Height above the floor (m).
    #[must_use]
    pub fn height_m(&self) -> f32 {
        match self {
            Self::Zoned(m) => m.height_m(),
            Self::Civic(c) => c.height_m(),
        }
    }

    /// Foundation from the ground.
    #[must_use]
    pub fn plan_foundation(&self, ground: &dyn Fn(Vec2) -> f32) -> Option<Foundation> {
        match self {
            Self::Zoned(m) => m.plan_foundation(ground),
            Self::Civic(c) => c.plan_foundation(ground),
        }
    }
}

/// The generator zone of a sim zone.
#[must_use]
pub const fn generator_zone(z: ZoneKind) -> Zone {
    match z {
        ZoneKind::Residential => Zone::Residential,
        ZoneKind::Commercial => Zone::Commercial,
        ZoneKind::Industrial => Zone::Industrial,
    }
}

/// Plans the massing of `kind` at `level` on `site` (`None` if it does not fit).
#[must_use]
pub fn plan_massing(kind: BuildingKind, level: u8, site: &Site, seed: u64) -> Option<Massing> {
    match (kind, site) {
        (BuildingKind::Zoned(z), Site::Zoned(s)) => {
            zoned::layout(generator_zone(z), level, &s.shape(), seed).map(Massing::Zoned)
        }
        (BuildingKind::Civic(c), Site::Civic(fr)) => {
            Some(Massing::Civic(civic::layout(c, *fr, seed)))
        }
        _ => None,
    }
}

/// A building.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Building {
    pub(crate) id: BuildingId,
    pub(crate) kind: BuildingKind,
    pub(crate) lot: Option<LotId>,
    pub(crate) level: u8,
    pub(crate) seed: u64,
    pub(crate) site: Site,
    pub(crate) foundation: Foundation,
    pub(crate) height_m: f32,
    pub(crate) outlines: Vec<Vec<Vec2>>,
    pub(crate) stamp: Option<u32>,
    pub(crate) built_tick: u64,
    pub(crate) condition: Condition,
}

impl Building {
    /// Stable id.
    #[must_use]
    pub fn id(&self) -> BuildingId {
        self.id
    }
    /// Kind.
    #[must_use]
    pub fn kind(&self) -> BuildingKind {
        self.kind
    }
    /// Lot it stands on (zoned buildings).
    #[must_use]
    pub fn lot(&self) -> Option<LotId> {
        self.lot
    }
    /// Level (zoned 1–5 by zone; civic 1).
    #[must_use]
    pub fn level(&self) -> u8 {
        self.level
    }
    /// Generation seed.
    #[must_use]
    pub fn seed(&self) -> u64 {
        self.seed
    }
    /// Site snapshot.
    #[must_use]
    pub fn site(&self) -> &Site {
        &self.site
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
    /// World outlines of the ground volumes (picking, overlap, corridor checks).
    #[must_use]
    pub fn outlines(&self) -> &[Vec<Vec2>] {
        &self.outlines
    }
    /// Tick it was built.
    #[must_use]
    pub fn built_tick(&self) -> u64 {
        self.built_tick
    }
    /// Condition.
    #[must_use]
    pub fn condition(&self) -> Condition {
        self.condition
    }
    /// Civic parcel quad (counter-clockwise); `None` for zoned buildings (their lot is the
    /// parcel).
    #[must_use]
    pub fn parcel(&self) -> Option<Vec<Vec2>> {
        match (&self.site, self.kind) {
            (Site::Civic(fr), BuildingKind::Civic(k)) => Some(civic_parcel(k, fr)),
            _ => None,
        }
    }
    /// The massing (deterministic from kind, level, seed and site).
    #[must_use]
    pub fn massing(&self) -> Option<Massing> {
        plan_massing(self.kind, self.level, &self.site, self.seed)
    }
}

/// Building changes for view sync.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildingChanged {
    /// Built or rebuilt (new mesh).
    Built(BuildingId),
    /// Removed.
    Removed(BuildingId),
    /// Condition changed (shader parameters only, no new mesh).
    Condition(BuildingId),
}

fn entity_of(world: &World, id: BuildingId) -> Option<bevy::ecs::entity::Entity> {
    world.get_resource::<Ids>()?.buildings.get(id)
}

fn tick(world: &World) -> u64 {
    world
        .get_resource::<crate::SimClock>()
        .map_or(0, |c| c.tick().0)
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
        if let Some(lot) = b.lot
            && let Some(le) = world.get_resource::<Ids>().and_then(|i| i.lots.get(lot))
            && let Some(mut l) = world.get_mut::<Lot>(le)
            && l.building == Some(id)
        {
            l.building = None;
        }
    }
    world.write_message(BuildingChanged::Removed(id));
}

/// Undo/redo: puts a building back with its original id and a fresh pad stamp.
pub(crate) fn restore_put_building(world: &mut World, mut b: Building) {
    let id = b.id;
    let existing = entity_of(world, id);
    if let Some(e) = existing
        && let Some(st) = world.get::<Building>(e).and_then(|x| x.stamp)
    {
        terrain::remove_stamp(world, st);
    }
    b.stamp = add_pad_stamp(world, &b.foundation);
    match existing {
        Some(e) => {
            if let Some(mut x) = world.get_mut::<Building>(e) {
                *x = b;
            }
        }
        None => {
            let e = world.spawn(b).id();
            world.resource_mut::<Ids>().buildings.insert(id, e);
        }
    }
    world.write_message(BuildingChanged::Built(id));
}

/// After a lot changed: removes its building if the massing no longer fits inside the lot.
pub(crate) fn check_fit(world: &mut World, lot: LotId) {
    let Some(le) = world.get_resource::<Ids>().and_then(|i| i.lots.get(lot)) else {
        return;
    };
    let Some(l) = world.get::<Lot>(le) else {
        return;
    };
    let Some(bid) = l.building else { return };
    let poly = l.polygon.clone();
    let Some(b) = entity_of(world, bid).and_then(|e| world.get::<Building>(e)) else {
        return;
    };
    let fits = b.outlines.iter().flatten().all(|c| {
        cb_geometry::polygon::contains(&poly, *c)
            || cb_geometry::polygon::boundary_distance(&poly, *c) < 0.02
    });
    if !fits {
        remove_building(world, bid);
    }
}

/// Seed of a zoned building on `lot` at `level`.
fn zoned_seed(world: &World, lot: LotId, level: u8) -> u64 {
    let world_seed = world.get_resource::<WorldSeed>().map_or(0, |s| s.0);
    derive_seed(
        world_seed,
        Domain::Building,
        u64::from(lot.get()),
        u64::from(level),
    )
}

/// Construction state for a new building of `level`.
fn constructing(world: &World, level: u8) -> Condition {
    let game_h = world
        .get_resource::<Catalog>()
        .map_or(3.0, |c| c.construction().duration_game_h(level));
    Condition::Constructing {
        start_tick: tick(world),
        duration_ticks: (f64::from(game_h) * TICKS_PER_GAME_HOUR as f64).round() as u64,
    }
}

/// A planned building before it gets an id.
struct Planned {
    kind: BuildingKind,
    lot: Option<LotId>,
    level: u8,
    seed: u64,
    site: Site,
    foundation: Foundation,
    height_m: f32,
    outlines: Vec<Vec<Vec2>>,
    condition: Condition,
}

impl Planned {
    fn into_building(self, id: BuildingId, built_tick: u64) -> Building {
        Building {
            id,
            kind: self.kind,
            lot: self.lot,
            level: self.level,
            seed: self.seed,
            site: self.site,
            foundation: self.foundation,
            height_m: self.height_m,
            outlines: self.outlines,
            stamp: None,
            built_tick,
            condition: self.condition,
        }
    }
}

/// Plans a zoned building on `lot` at `level` or the highest lower level that fits.
fn plan_zoned(world: &World, lot: LotId, level: u8) -> Option<Planned> {
    let le = world.get_resource::<Ids>()?.lots.get(lot)?;
    let l = world.get::<Lot>(le)?;
    let zone = world
        .get_resource::<Ids>()?
        .plots
        .get(l.plot)
        .and_then(|pe| world.get::<Plot>(pe))?
        .zone;
    let site = Site::Zoned(ZonedSite::of(l));
    let kind = BuildingKind::Zoned(zone);
    let g = terrain::ground(world);
    let ground = |p: Vec2| cb_network::validation::Ground::height(&g, p);
    (1..=level).rev().find_map(|lv| {
        let seed = zoned_seed(world, lot, lv);
        let m = plan_massing(kind, lv, &site, seed)?;
        let foundation = m.plan_foundation(&ground)?;
        Some(Planned {
            kind,
            lot: Some(lot),
            level: lv,
            seed,
            site: site.clone(),
            height_m: m.height_m(),
            outlines: m.outlines(),
            foundation,
            condition: Condition::Active,
        })
    })
}

/// Spawns a planned building with a fresh id and pad stamp.
fn spawn_building(world: &mut World, p: Planned) -> Option<BuildingId> {
    let id = world.resource_mut::<Ids>().buildings.allocate().ok()?;
    let mut b = p.into_building(id, tick(world));
    b.stamp = add_pad_stamp(world, &b.foundation);
    let lot = b.lot;
    let e = world.spawn(b).id();
    world.resource_mut::<Ids>().buildings.insert(id, e);
    if let Some(lot) = lot
        && let Some(le) = world.get_resource::<Ids>().and_then(|i| i.lots.get(lot))
        && let Some(mut lm) = world.get_mut::<Lot>(le)
    {
        lm.building = Some(id);
    }
    world.write_message(BuildingChanged::Built(id));
    Some(id)
}

/// Highest level the lot's zone and area allow.
fn level_cap(world: &World, lot: LotId) -> Option<u8> {
    let le = world.get_resource::<Ids>()?.lots.get(lot)?;
    let l = world.get::<Lot>(le)?;
    let zone = world
        .get_resource::<Ids>()?
        .plots
        .get(l.plot)
        .and_then(|pe| world.get::<Plot>(pe))?
        .zone;
    let cat = world.get_resource::<Catalog>()?;
    Some(cat.zone(zone).level_cap(l.area_m2))
}

/// Grows a building on an empty lot (Debug growth and citygen): a seeded level up to the lot's
/// cap (falling back to lower levels that fit), under construction unless `instant`.
pub(crate) fn grow_lot_with(world: &mut World, lot: LotId, instant: bool) -> Option<BuildingId> {
    let le = world.get_resource::<Ids>()?.lots.get(lot)?;
    if world.get::<Lot>(le)?.building.is_some() {
        return None;
    }
    let cap = level_cap(world, lot)?;
    let world_seed = world.get_resource::<WorldSeed>().map_or(0, |s| s.0);
    let level_seed = derive_seed(world_seed, Domain::Growth, u64::from(lot.get()), 0);
    let level = 1 + (level_seed % u64::from(cap)) as u8;
    let Some(mut b) = plan_zoned(world, lot, level) else {
        // Nothing fits (too steep or too small): the lot stays green.
        if let Some(mut lm) = world.get_mut::<Lot>(le) {
            lm.footprint = None;
            lm.inset.clear();
        }
        return None;
    };
    if !instant {
        b.condition = constructing(world, b.level);
    }
    spawn_building(world, b)
}

/// Grows a building on an empty lot, under construction.
pub(crate) fn grow_lot(world: &mut World, lot: LotId) -> Option<BuildingId> {
    grow_lot_with(world, lot, false)
}

/// Debug: rebuilds a zoned building at `level` (clamped to the lot's cap; the nearest lower
/// level that fits is used). Keeps the building id; the condition becomes `Active`.
pub(crate) fn set_level(world: &mut World, id: BuildingId, level: u8) -> bool {
    let Some(e) = entity_of(world, id) else {
        return false;
    };
    let Some(old) = world.get::<Building>(e).cloned() else {
        return false;
    };
    let Some(lot) = old.lot else { return false };
    let cap = level_cap(world, lot).unwrap_or(1);
    let Some(p) = plan_zoned(world, lot, level.clamp(1, cap)) else {
        return false;
    };
    if let Some(s) = old.stamp {
        terrain::remove_stamp(world, s);
    }
    let mut b = p.into_building(id, old.built_tick);
    b.stamp = add_pad_stamp(world, &b.foundation);
    if let Some(mut x) = world.get_mut::<Building>(e) {
        *x = b;
    }
    world.write_message(BuildingChanged::Built(id));
    true
}

/// Debug: sets a building's condition. Only changes into or out of `Ruined` rebuild the mesh.
pub(crate) fn set_condition(world: &mut World, id: BuildingId, condition: Condition) -> bool {
    let Some(e) = entity_of(world, id) else {
        return false;
    };
    let Some(mut b) = world.get_mut::<Building>(e) else {
        return false;
    };
    let was_ruined = b.condition == Condition::Ruined;
    b.condition = condition;
    let msg = if was_ruined != (condition == Condition::Ruined) {
        BuildingChanged::Built(id)
    } else {
        BuildingChanged::Condition(id)
    };
    world.write_message(msg);
    true
}

/// Terrain pad stamp of a building foundation (if it has a pad).
pub(crate) fn add_pad_stamp(world: &mut World, foundation: &Foundation) -> Option<u32> {
    foundation.pad.as_ref().map(|pad| {
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
    })
}

/// Finishes construction of buildings whose time is up.
fn finish_construction(world: &mut World, now: u64) {
    let Some(ids) = world.get_resource::<Ids>() else {
        return;
    };
    let done: Vec<BuildingId> = ids
        .buildings
        .iter()
        .filter_map(|(id, e)| {
            let b = world.get::<Building>(e)?;
            (matches!(b.condition, Condition::Constructing { .. })
                && b.condition.progress(now) >= 1.0)
                .then_some(id)
        })
        .collect();
    for id in done {
        set_condition(world, id, Condition::Active);
    }
}

/// `TickSet::SubRate` (every [`GROWTH_PERIOD_TICKS`]): finishes construction, then Debug
/// growth of empty lots of every zone.
pub(crate) fn growth(world: &mut World) {
    let now = tick(world);
    if !now.is_multiple_of(GROWTH_PERIOD_TICKS) {
        return;
    }
    finish_construction(world, now);
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
            let can_build = l.footprint.is_some() || !l.inset.is_empty();
            (l.building.is_none() && can_build && now >= l.created_tick + delay).then_some(id)
        })
        .take(GROWTH_PER_PASS)
        .collect();
    for lot in candidates {
        grow_lot(world, lot);
    }
}

/// How far from a road edge a civic building's front snaps onto it (m).
pub const PLACE_SNAP_M: f32 = 2.0;
/// Overlap tolerated between a parcel and a road corridor (the front edge touches it), m².
const PLACE_OVERLAP_TOL_M2: f32 = 0.5;
/// Shoreline kinds need water at least this deep within [`SHORE_REACH_M`] behind the parcel.
pub const SHORE_MIN_DEPTH_M: f32 = 1.5;
/// Shoreline search distance behind the parcel (m).
pub const SHORE_REACH_M: f32 = 15.0;

/// A validated civic placement.
#[derive(Clone, Debug, PartialEq)]
pub struct CivicPlacement {
    /// Parcel frame (front edge on the road edge).
    pub frame: Frame,
    /// Price.
    pub cost: cb_core::Money,
    /// World outlines of the planned volumes.
    pub outlines: Vec<Vec<Vec2>>,
    /// Parcel quad.
    pub parcel: Vec<Vec2>,
}

/// The road edge point nearest to `pos` within [`PLACE_SNAP_M`]: `(point, direction along the
/// road with the parcel side on its left)`.
#[must_use]
pub fn snap_to_road_edge(world: &World, pos: Vec2) -> Option<(Vec2, Vec2)> {
    let ids = world.get_resource::<Ids>()?;
    let mut best: Option<(f32, Vec2, Vec2)> = None;
    for (_, e) in ids.segments.iter() {
        let Some(seg) = world.get::<crate::roads::RoadSegment>(e) else {
            continue;
        };
        let g = seg.geom();
        for (edge, left) in [(&g.left, true), (&g.right, false)] {
            let (s, d) = edge.project(pos);
            if d.abs() > PLACE_SNAP_M || best.is_some_and(|b| b.0 <= d.abs()) {
                continue;
            }
            let t = edge.tangent_at(s);
            // The left edge has the road on its right: parcels go left of the tangent.
            let along = if left { t } else { -t };
            best = Some((d.abs(), edge.point_at(s), along));
        }
    }
    best.map(|b| (b.1, b.2))
}

fn overlap_area(a: &[Vec2], b: &[Vec2]) -> f32 {
    let (la, ha) = cb_geometry::polygon::bounds(a);
    let (lb, hb) = cb_geometry::polygon::bounds(b);
    if la.cmpgt(hb).any() || lb.cmpgt(ha).any() {
        return 0.0;
    }
    cb_geometry::boolean::intersect(a, b)
        .outers
        .iter()
        .map(|o| cb_geometry::polygon::area(o))
        .sum()
}

/// Parcel quad of a civic building.
fn civic_parcel(kind: CivicKind, fr: &Frame) -> Vec<Vec2> {
    let (w, d) = kind.parcel_m();
    cb_geometry::polygon::to_ccw(&fr.rect([0.0, 0.0, w, d]))
}

/// Validates placing `kind` with its front at `pos` (snapped to the nearest road edge).
///
/// # Errors
/// `building-invalid-no-road`, `-overlap`, `-water`, `-shore`, `-steep` or `money-insufficient`.
pub fn preview_place_civic(
    world: &World,
    kind: CivicKind,
    pos: Vec2,
) -> Result<CivicPlacement, crate::commands::ValidationError> {
    use crate::commands::ValidationError as E;
    let (front, along) =
        snap_to_road_edge(world, pos).ok_or(E::at("building-invalid-no-road", pos))?;
    let frame = civic::frame_at(kind, front, along);
    let parcel = civic_parcel(kind, &frame);
    let centre = cb_geometry::polygon::centroid(&parcel);
    let ids = world
        .get_resource::<Ids>()
        .ok_or(E::new("internal-no-catalog"))?;
    // Roads: only the touching front edge may overlap.
    for (_, e) in ids.segments.iter() {
        if let Some(seg) = world.get::<crate::roads::RoadSegment>(e)
            && overlap_area(&parcel, &seg.geom().corridor) > PLACE_OVERLAP_TOL_M2
        {
            return Err(E::at("building-invalid-overlap", centre));
        }
    }
    for (_, e) in ids.plots.iter() {
        if let Some(p) = world.get::<Plot>(e)
            && overlap_area(&parcel, p.polygon()) > PLACE_OVERLAP_TOL_M2
        {
            return Err(E::at("building-invalid-overlap", centre));
        }
    }
    for (_, e) in ids.buildings.iter() {
        let Some(b) = world.get::<Building>(e) else {
            continue;
        };
        let theirs: Vec<Vec<Vec2>> = match (&b.site, b.kind) {
            (Site::Civic(fr), BuildingKind::Civic(k)) => vec![civic_parcel(k, fr)],
            _ => b.outlines.clone(),
        };
        if theirs
            .iter()
            .any(|o| overlap_area(&parcel, o) > PLACE_OVERLAP_TOL_M2)
        {
            return Err(E::at("building-invalid-overlap", centre));
        }
    }
    // Water: the parcel stays dry; shoreline kinds need deep water behind it.
    let (w, d) = kind.parcel_m();
    let terrain = world.get_resource::<terrain::TerrainRes>();
    if let Some(t) = terrain.map(terrain::TerrainRes::terrain) {
        let wet = (0..=4)
            .flat_map(|i| (0..=4).map(move |j| (i, j)))
            .any(|(i, j)| {
                let p = frame.at(w * i as f32 / 4.0, d * j as f32 / 4.0);
                t.water_depth_at(p) > 0.05
            });
        if wet {
            return Err(E::at("building-invalid-water", centre));
        }
        if kind.pipe_reach_m() > 0.0 {
            let shore = (1..=15).any(|k| {
                let p = frame.at(0.5 * w, d + SHORE_REACH_M * k as f32 / 15.0);
                t.water_depth_at(p) >= SHORE_MIN_DEPTH_M
            });
            if !shore {
                return Err(E::at("building-invalid-shore", centre));
            }
        }
    } else if kind.pipe_reach_m() > 0.0 {
        return Err(E::at("building-invalid-shore", centre));
    }
    let layout = civic::layout(kind, frame, 0);
    let g = terrain::ground(world);
    let ground = |p: Vec2| cb_network::validation::Ground::height(&g, p);
    if layout.plan_foundation(&ground).is_none() {
        return Err(E::at("building-invalid-steep", centre));
    }
    let cost = world
        .get_resource::<Catalog>()
        .map_or(cb_core::Money::ZERO, |c| {
            cb_core::Money(c.civic(kind).cost_cents)
        });
    if !crate::economy::can_afford(world, cost) {
        return Err(E::new("money-insufficient"));
    }
    Ok(CivicPlacement {
        frame,
        cost,
        outlines: layout.outlines(),
        parcel,
    })
}

/// `PlaceBuilding`: validate, charge, build (under construction). Returns the cost.
pub(crate) fn place_civic(
    world: &mut World,
    kind: CivicKind,
    pos: Vec2,
) -> Result<cb_core::Money, crate::commands::ValidationError> {
    let pl = preview_place_civic(world, kind, pos)?;
    crate::economy::charge(world, pl.cost)?;
    let id = world
        .resource_mut::<Ids>()
        .buildings
        .allocate()
        .map_err(|_| crate::commands::ValidationError::new("internal-id-space"))?;
    let seed = derive_seed(
        world.get_resource::<WorldSeed>().map_or(0, |s| s.0),
        Domain::Building,
        u64::from(id.get()),
        0,
    );
    let site = Site::Civic(pl.frame);
    let kind_b = BuildingKind::Civic(kind);
    let m = plan_massing(kind_b, 1, &site, seed).ok_or(crate::commands::ValidationError::new(
        "building-invalid-steep",
    ))?;
    let g = terrain::ground(world);
    let ground = |p: Vec2| cb_network::validation::Ground::height(&g, p);
    let foundation = m
        .plan_foundation(&ground)
        .ok_or(crate::commands::ValidationError::new(
            "building-invalid-steep",
        ))?;
    drop(g);
    let condition = constructing(world, 1);
    let mut b = Planned {
        kind: kind_b,
        lot: None,
        level: 1,
        seed,
        site,
        height_m: m.height_m(),
        outlines: m.outlines(),
        foundation,
        condition,
    }
    .into_building(id, tick(world));
    b.stamp = add_pad_stamp(world, &b.foundation);
    let e = world.spawn(b).id();
    world.resource_mut::<Ids>().buildings.insert(id, e);
    world.write_message(BuildingChanged::Built(id));
    Ok(pl.cost)
}

/// Spacing of town hall candidate positions along road edges (m).
const TOWN_HALL_STEP_M: f32 = 6.0;

/// Auto-places the town hall at the start (GAME_DESIGN §5): free and already built, on the
/// first valid road-edge position nearest to the first outside connection's end. Until outside
/// connections exist (M5) the anchor is the first dead-end road node, else the first node.
/// Does nothing if a town hall exists.
pub fn place_town_hall(world: &mut World) -> Option<BuildingId> {
    let ids = world.get_resource::<Ids>()?;
    let exists = ids.buildings.iter().any(|(_, e)| {
        world
            .get::<Building>(e)
            .is_some_and(|b| b.kind == BuildingKind::Civic(CivicKind::TownHall))
    });
    if exists {
        return None;
    }
    let nodes: Vec<&crate::roads::RoadNode> = ids
        .nodes
        .iter()
        .filter_map(|(_, e)| world.get::<crate::roads::RoadNode>(e))
        .collect();
    let anchor = nodes
        .iter()
        .find(|n| n.segments().len() == 1)
        .or_else(|| nodes.first())
        .map(|n| Vec2::new(n.pos().x, n.pos().z))?;
    // Candidate points along both edges of every segment, nearest to the anchor first.
    let mut cands: Vec<(f32, Vec2)> = Vec::new();
    for (_, e) in ids.segments.iter() {
        let Some(seg) = world.get::<crate::roads::RoadSegment>(e) else {
            continue;
        };
        let g = seg.geom();
        for edge in [&g.left, &g.right] {
            let len = edge.length();
            let n = ((len - 20.0) / TOWN_HALL_STEP_M).floor().max(0.0) as usize;
            for k in 0..=n {
                let p = edge.point_at(10.0 + TOWN_HALL_STEP_M * k as f32);
                // Nudge off the edge so the snap finds this side.
                let c = g.centerline.point_at(g.centerline.project(p).0);
                let q = p + (p - c).normalize_or_zero() * 0.5;
                cands.push((q.distance(anchor), q));
            }
        }
    }
    cands.sort_by(|a, b| a.0.total_cmp(&b.0));
    let pos = cands
        .into_iter()
        .map(|c| c.1)
        .find(|p| preview_place_civic(world, CivicKind::TownHall, *p).is_ok())?;
    place_civic(world, CivicKind::TownHall, pos).ok()?;
    let id = world
        .get_resource::<Ids>()?
        .buildings
        .iter()
        .filter(|(_, e)| {
            world
                .get::<Building>(*e)
                .is_some_and(|b| b.kind == BuildingKind::Civic(CivicKind::TownHall))
        })
        .map(|(id, _)| id)
        .next()?;
    set_condition(world, id, Condition::Active);
    Some(id)
}
