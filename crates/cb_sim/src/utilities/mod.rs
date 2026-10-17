//! Utility distribution (ADR-0026, ALGORITHMS §18), hourly at tick 120 of the game hour.
//!
//! Networks are the road graph's connected components; every building attaches through its
//! road access (a zoned lot's frontage segment, a civic parcel's nearest segment). Within a
//! component, power, water and sewage capacity are allocated deterministically: services and
//! utilities first, then by network distance from the nearest producer, ties by building id —
//! so outer areas fail first. Garbage accumulates per building and landfills collect it
//! analytically until trucks exist (M9). Sewage outlets pollute rivers; pumps drawing polluted
//! water deliver polluted water.

pub mod network;
pub mod pollution;

use std::collections::BTreeMap;

use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use cb_core::ids::{BuildingId, NodeId, SegmentId};
use cb_core::time::TICKS_PER_GAME_HOUR;
use cb_procgen::building::civic::CivicKind;
use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::buildings::{Building, BuildingKind, Condition};
use crate::catalog::{Catalog, UtilitySpec, ZoneKind};
use crate::citizens::Capacities;
use crate::ids::Ids;
use crate::roads::RoadSegment;
use crate::zoning::{Lot, Plot};
use network::Consumer;

/// Tick of the utility balance in the game hour (phase table).
pub const OFFSET: u64 = 120;
/// Civic buildings attach to a segment within this distance of their centre (m).
const CIVIC_ACCESS_M: f32 = 60.0;
/// Outlets inject into a river point within this distance (m); pumps draw within it too.
const RIVER_REACH_M: f32 = 150.0;

/// Utility state of one building.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Supply {
    /// Has road access to a network.
    pub connected: bool,
    /// Powered.
    pub power: bool,
    /// Supplied with water.
    pub water: bool,
    /// The water it gets is polluted.
    pub polluted_water: bool,
    /// Sewage taken away.
    pub sewage: bool,
    /// Uncollected garbage beyond the problem threshold.
    pub garbage_problem: bool,
}

/// Balance of one network (component) for one utility.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Balance {
    /// Supply (kW or m³/h).
    pub supply: f32,
    /// Demand.
    pub demand: f32,
}

/// Utility state (garbage stocks and landfill fill are saved; statuses are re-derived hourly).
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Utilities {
    pub(crate) status: BTreeMap<BuildingId, Supply>,
    /// Uncollected garbage per building (kg).
    pub(crate) garbage_kg: BTreeMap<BuildingId, f32>,
    /// Landfill fill (kg).
    pub(crate) landfill_kg: BTreeMap<BuildingId, f32>,
    /// Concentration per river point.
    pub(crate) river_pollution: Vec<Vec<f32>>,
    /// Power and water balance per network (component id = smallest node id).
    #[serde(skip)]
    pub(crate) power: BTreeMap<NodeId, Balance>,
    #[serde(skip)]
    pub(crate) water: BTreeMap<NodeId, Balance>,
}

impl Utilities {
    /// Status of a building (unconnected and unserved when unknown).
    #[must_use]
    pub fn status(&self, id: BuildingId) -> Supply {
        self.status.get(&id).copied().unwrap_or_default()
    }

    /// Uncollected garbage of a building (kg).
    #[must_use]
    pub fn garbage_kg(&self, id: BuildingId) -> f32 {
        self.garbage_kg.get(&id).copied().unwrap_or(0.0)
    }

    /// Fill of a landfill (kg).
    #[must_use]
    pub fn landfill_kg(&self, id: BuildingId) -> f32 {
        self.landfill_kg.get(&id).copied().unwrap_or(0.0)
    }

    /// Power balances per network.
    pub fn power_balances(&self) -> impl Iterator<Item = (NodeId, Balance)> + '_ {
        self.power.iter().map(|(k, v)| (*k, *v))
    }

    /// Water balances per network.
    pub fn water_balances(&self) -> impl Iterator<Item = (NodeId, Balance)> + '_ {
        self.water.iter().map(|(k, v)| (*k, *v))
    }

    /// Concentration per point of each river (terrain order).
    #[must_use]
    pub fn river_pollution(&self) -> &[Vec<f32>] {
        &self.river_pollution
    }
}

/// What a building produces or needs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Profile {
    power_kw: f32,
    water_m3h: f32,
    sewage_m3h: f32,
    garbage_kg_day: f32,
    /// Producer outputs.
    power_out: f32,
    water_out: f32,
    sewage_out: f32,
    landfill: bool,
    priority: bool,
}

fn consumption(
    spec: &UtilitySpec,
    per: crate::catalog::Consumption,
    units: f32,
    level: u8,
) -> Profile {
    let f = units * (1.0 + spec.level_factor * f32::from(level.saturating_sub(1)));
    Profile {
        power_kw: per.power_kw * f,
        water_m3h: per.water_m3h * f,
        sewage_m3h: per.water_m3h * f * spec.sewage_share,
        garbage_kg_day: per.garbage_kg_day * f,
        ..Profile::default()
    }
}

fn civic_profile(spec: &UtilitySpec, kind: CivicKind) -> Profile {
    let mut p = consumption(spec, spec.job, spec.civic_job_equivalent, 1);
    p.priority = true;
    match kind {
        CivicKind::DieselGenerator => p.power_out = spec.diesel_generator_kw,
        CivicKind::CoalPlant => p.power_out = spec.coal_plant_kw,
        CivicKind::WaterPump => p.water_out = spec.water_pump_m3h,
        CivicKind::SewageOutlet => p.sewage_out = spec.sewage_outlet_m3h,
        CivicKind::Landfill => p.landfill = true,
        _ => {}
    }
    p
}

/// One building in the hourly pass.
struct Node {
    id: BuildingId,
    pos: Vec2,
    /// Access segment's end nodes.
    ends: Option<(NodeId, NodeId)>,
    profile: Profile,
}

fn access_ends(world: &World, ids: &Ids, seg: SegmentId) -> Option<(NodeId, NodeId)> {
    let s = world.get::<RoadSegment>(ids.segments.get(seg)?)?;
    Some((s.start(), s.end()))
}

/// Collects buildings with their access and profile.
fn collect(world: &World, spec: &UtilitySpec) -> Vec<Node> {
    let (Some(ids), Some(cap)) = (
        world.get_resource::<Ids>(),
        world.get_resource::<Capacities>(),
    ) else {
        return Vec::new();
    };
    let segments: Vec<(SegmentId, &RoadSegment)> = ids
        .segments
        .iter()
        .filter_map(|(id, e)| Some((id, world.get::<RoadSegment>(e)?)))
        .collect();
    let mut out = Vec::new();
    for (id, e) in ids.buildings.iter() {
        let Some(b) = world.get::<Building>(e) else {
            continue;
        };
        if !matches!(b.condition(), Condition::Active | Condition::Declining) {
            continue;
        }
        let pos = b
            .outlines()
            .first()
            .map_or(Vec2::ZERO, |o| cb_geometry::polygon::centroid(o));
        let (ends, profile) = match b.kind() {
            BuildingKind::Zoned(zone) => {
                let ends = b
                    .lot()
                    .and_then(|l| world.get::<Lot>(ids.lots.get(l)?))
                    .and_then(|l| world.get::<Plot>(ids.plots.get(l.plot())?))
                    .and_then(|p| p.segments().first().copied())
                    .and_then(|s| access_ends(world, ids, s));
                let profile = if zone == ZoneKind::Residential {
                    consumption(
                        spec,
                        spec.household,
                        f32::from(cap.home_slots(id)),
                        b.level(),
                    )
                } else {
                    let jobs: u16 = cap.jobs_of(id).iter().sum();
                    consumption(spec, spec.job, f32::from(jobs), b.level())
                };
                (ends, profile)
            }
            BuildingKind::Civic(kind) => {
                let ends = segments
                    .iter()
                    .map(|(sid, s)| (s.geom().centerline.project(pos).1.abs(), *sid, *s))
                    .filter(|x| x.0 <= CIVIC_ACCESS_M)
                    .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
                    .map(|(_, _, s)| (s.start(), s.end()));
                (ends, civic_profile(spec, kind))
            }
        };
        out.push(Node {
            id,
            pos,
            ends,
            profile,
        });
    }
    out
}

/// The hourly utility pass.
pub(crate) fn balance(world: &mut World) {
    let Some(spec) = world.get_resource::<Catalog>().map(|c| *c.utilities()) else {
        return;
    };
    let nodes = collect(world, &spec);
    // Road graph.
    let edges: Vec<(NodeId, NodeId, f32)> = world.get_resource::<Ids>().map_or(Vec::new(), |ids| {
        ids.segments
            .iter()
            .filter_map(|(_, e)| {
                let s = world.get::<RoadSegment>(e)?;
                Some((s.start(), s.end(), s.geom().length()))
            })
            .collect()
    });
    let adj = network::adjacency(&edges);
    let comp = network::components(&adj);
    let comp_of = |n: &Node| n.ends.and_then(|(a, _)| comp.get(&a).copied());
    // Rivers and pollution from sewage outlets.
    let rivers: Vec<Vec<Vec2>> = world
        .get_resource::<crate::terrain::TerrainRes>()
        .map(|t| t.rivers().to_vec())
        .unwrap_or_default();
    let prev = world
        .get_resource::<Utilities>()
        .cloned()
        .unwrap_or_default();
    let mut status: BTreeMap<BuildingId, Supply> = BTreeMap::new();
    let mut power_bal = BTreeMap::new();
    let mut water_bal = BTreeMap::new();
    let mut networks: BTreeMap<NodeId, Vec<&Node>> = BTreeMap::new();
    for n in &nodes {
        let connected = comp_of(n).is_some();
        status.insert(
            n.id,
            Supply {
                connected,
                ..Supply::default()
            },
        );
        if let Some(c) = comp_of(n) {
            networks.entry(c).or_default().push(n);
        }
    }
    let mut river_injections: Vec<Vec<(usize, f32)>> = vec![Vec::new(); rivers.len()];
    let mut pumps: Vec<(NodeId, Vec2, f32)> = Vec::new();
    for (c, members) in &networks {
        // Distances from each producer kind.
        let from = |pick: fn(&Profile) -> f32| {
            let src: Vec<NodeId> = members
                .iter()
                .filter(|n| pick(&n.profile) > 0.0)
                .filter_map(|n| n.ends.map(|e| e.0))
                .collect();
            network::distances(&adj, &src)
        };
        let consumers = |pick: fn(&Profile) -> f32,
                         dist: &BTreeMap<NodeId, f32>,
                         served: fn(&Supply) -> bool| {
            members
                .iter()
                .filter(|n| pick(&n.profile) > 0.0)
                .map(|n| Consumer {
                    id: n.id,
                    priority: n.profile.priority,
                    distance_m: n.ends.map_or(f32::INFINITY, |(a, b)| {
                        let da = dist.get(&a).copied().unwrap_or(f32::INFINITY);
                        let db = dist.get(&b).copied().unwrap_or(f32::INFINITY);
                        da.min(db)
                    }),
                    need: pick(&n.profile),
                    was_served: served(&prev.status(n.id)),
                })
                .collect::<Vec<_>>()
        };
        // Power.
        let supply: f32 = members.iter().map(|n| n.profile.power_out).sum();
        let d = from(|p| p.power_out);
        let cs = consumers(|p| p.power_kw, &d, |s| s.power);
        let need: f32 = cs.iter().map(|c| c.need).sum();
        power_bal.insert(
            *c,
            Balance {
                supply,
                demand: need,
            },
        );
        for id in network::allocate(supply, &cs, spec.hysteresis) {
            if let Some(s) = status.get_mut(&id) {
                s.power = true;
            }
        }
        // Water.
        let supply: f32 = members.iter().map(|n| n.profile.water_out).sum();
        let d = from(|p| p.water_out);
        let cs = consumers(|p| p.water_m3h, &d, |s| s.water);
        let need: f32 = cs.iter().map(|c| c.need).sum();
        water_bal.insert(
            *c,
            Balance {
                supply,
                demand: need,
            },
        );
        for id in network::allocate(supply, &cs, spec.hysteresis) {
            if let Some(s) = status.get_mut(&id) {
                s.water = true;
            }
        }
        for n in members.iter().filter(|n| n.profile.water_out > 0.0) {
            pumps.push((*c, n.pos, n.profile.water_out));
        }
        // Sewage.
        let supply: f32 = members.iter().map(|n| n.profile.sewage_out).sum();
        let d = from(|p| p.sewage_out);
        let cs = consumers(|p| p.sewage_m3h, &d, |s| s.sewage);
        let mut taken = 0.0f32;
        for id in network::allocate(supply, &cs, spec.hysteresis) {
            if let Some(s) = status.get_mut(&id) {
                s.sewage = true;
            }
            taken += cs.iter().find(|x| x.id == id).map_or(0.0, |x| x.need);
        }
        // Each outlet discharges its share of the sewage taken into the nearest river.
        let outlets: Vec<&&Node> = members
            .iter()
            .filter(|n| n.profile.sewage_out > 0.0)
            .collect();
        for o in &outlets {
            let share = taken.min(supply) * o.profile.sewage_out / supply.max(1e-6);
            if let Some((ri, (pi, _))) = rivers
                .iter()
                .enumerate()
                .filter_map(|(ri, r)| Some((ri, pollution::nearest_point(r, o.pos)?)))
                .filter(|(_, (_, d))| *d <= RIVER_REACH_M)
                .min_by(|a, b| a.1.1.total_cmp(&b.1.1).then(a.0.cmp(&b.0)))
            {
                river_injections[ri].push((pi, share));
            }
        }
    }
    let river_pollution: Vec<Vec<f32>> = rivers
        .iter()
        .zip(&river_injections)
        .map(|(r, inj)| pollution::advect(r, inj, spec.pollution_decay_per_km, spec.river_flow_m3h))
        .collect();
    // Polluted water: a network whose pumps mostly draw polluted water.
    let mut polluted_out: BTreeMap<NodeId, (f32, f32)> = BTreeMap::new();
    for (c, pos, out) in pumps {
        let conc = rivers
            .iter()
            .zip(&river_pollution)
            .filter_map(|(r, cp)| {
                let (pi, d) = pollution::nearest_point(r, pos)?;
                (d <= RIVER_REACH_M).then(|| cp[pi])
            })
            .fold(0.0f32, f32::max);
        let e = polluted_out.entry(c).or_default();
        e.1 += out;
        if conc > spec.polluted_threshold {
            e.0 += out;
        }
    }
    for (c, members) in &networks {
        let polluted = polluted_out.get(c).is_some_and(|(p, t)| *p * 2.0 > *t);
        if polluted {
            for n in members {
                if let Some(s) = status.get_mut(&n.id)
                    && s.water
                {
                    s.polluted_water = true;
                }
            }
        }
    }
    // Garbage: accumulate, then landfills collect within their network.
    let mut garbage = prev.garbage_kg.clone();
    let mut landfill = prev.landfill_kg.clone();
    garbage.retain(|id, _| status.contains_key(id));
    landfill.retain(|id, _| nodes.iter().any(|n| n.id == *id && n.profile.landfill));
    for n in &nodes {
        if n.profile.garbage_kg_day > 0.0 && !n.profile.landfill {
            *garbage.entry(n.id).or_default() += n.profile.garbage_kg_day / 24.0;
        }
    }
    for members in networks.values() {
        let fills: Vec<BuildingId> = members
            .iter()
            .filter(|n| n.profile.landfill)
            .map(|n| n.id)
            .collect();
        let room: f32 = fills
            .iter()
            .map(|id| {
                (spec.landfill_storage_kg - landfill.get(id).copied().unwrap_or(0.0)).max(0.0)
            })
            .sum();
        let capacity = (spec.landfill_collection_kg_day / 24.0 * fills.len() as f32).min(room);
        let stored: f32 = members
            .iter()
            .map(|n| garbage.get(&n.id).copied().unwrap_or(0.0))
            .sum();
        if capacity <= 0.0 || stored <= 0.0 {
            continue;
        }
        let ratio = (capacity / stored).min(1.0);
        let mut collected = 0.0;
        for n in members {
            if let Some(g) = garbage.get_mut(&n.id) {
                let take = *g * ratio;
                *g -= take;
                collected += take;
            }
        }
        // Fill landfills in id order.
        for id in fills {
            let f = landfill.entry(id).or_default();
            let add = collected.min((spec.landfill_storage_kg - *f).max(0.0));
            *f += add;
            collected -= add;
        }
    }
    for n in &nodes {
        let limit = n.profile.garbage_kg_day * spec.garbage_problem_days;
        if limit > 0.0
            && garbage.get(&n.id).copied().unwrap_or(0.0) > limit
            && let Some(s) = status.get_mut(&n.id)
        {
            s.garbage_problem = true;
        }
    }
    world.insert_resource(Utilities {
        status,
        garbage_kg: garbage,
        landfill_kg: landfill,
        river_pollution,
        power: power_bal,
        water: water_bal,
    });
}

/// `TickSet::SubRate`: the hourly utility balance.
pub(crate) fn tick(world: &mut World) {
    let now = world
        .get_resource::<crate::SimClock>()
        .map_or(0, |c| c.tick().0);
    if now % TICKS_PER_GAME_HOUR == OFFSET {
        balance(world);
    }
}
