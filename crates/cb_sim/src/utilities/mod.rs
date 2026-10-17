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

/// First tick of the utility pass in the game hour (phase table).
pub const OFFSET: u64 = 120;
/// Building collection slices (`BuildingId % 16`); the allocation runs on the tick after.
pub const COLLECT_SLICES: u64 = 16;
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
    #[serde(default)]
    pub(crate) power: BTreeMap<NodeId, Balance>,
    #[serde(default)]
    pub(crate) water: BTreeMap<NodeId, Balance>,
    /// Buildings collected so far this hour (saved: the pass spans several ticks).
    #[serde(default)]
    pub(crate) pending: Vec<Node>,
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Profile {
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Node {
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
fn collect(world: &World, spec: &UtilitySpec, slice: u64) -> Vec<Node> {
    let (Some(ids), Some(cap)) = (
        world.get_resource::<Ids>(),
        world.get_resource::<Capacities>(),
    ) else {
        return Vec::new();
    };
    // Segments with their centreline bounds, so civic access projects only onto nearby ones.
    let segments: Vec<(SegmentId, &RoadSegment, Vec2, Vec2)> = ids
        .segments
        .iter()
        .filter_map(|(id, e)| {
            let s = world.get::<RoadSegment>(e)?;
            let (lo, hi) = cb_geometry::polygon::bounds(s.geom().centerline.points());
            Some((id, s, lo, hi))
        })
        .collect();
    let mut out = Vec::new();
    for (id, e) in ids
        .buildings
        .iter()
        .filter(|(id, _)| u64::from(id.get()) % COLLECT_SLICES == slice)
    {
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
                    .filter(|(_, _, lo, hi)| {
                        pos.x >= lo.x - CIVIC_ACCESS_M
                            && pos.x <= hi.x + CIVIC_ACCESS_M
                            && pos.y >= lo.y - CIVIC_ACCESS_M
                            && pos.y <= hi.y + CIVIC_ACCESS_M
                    })
                    .map(|(sid, s, _, _)| (s.geom().centerline.project(pos).1.abs(), *sid, *s))
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

/// A per-building map aligned to `nodes` (both sorted by id), by a merge walk.
fn aligned<V: Copy>(nodes: &[Node], map: &BTreeMap<BuildingId, V>) -> Vec<Option<V>> {
    let mut it = map.iter().peekable();
    nodes
        .iter()
        .map(|n| {
            while it.peek().is_some_and(|(k, _)| **k < n.id) {
                it.next();
            }
            match it.peek() {
                Some((k, v)) if **k == n.id => Some(**v),
                _ => None,
            }
        })
        .collect()
}

/// One collection slice: buildings with `id % COLLECT_SLICES == k` join the pending list.
fn collect_slice(world: &mut World, k: u64) {
    let Some(spec) = world.get_resource::<Catalog>().map(|c| *c.utilities()) else {
        return;
    };
    let mut part = collect(world, &spec, k);
    if let Some(mut u) = world.get_resource_mut::<Utilities>() {
        if k == 0 {
            u.pending.clear();
        }
        u.pending.append(&mut part);
    }
}

/// The hourly allocation over the collected buildings (dense graph, arrays aligned to the
/// buildings in id order).
#[allow(clippy::too_many_lines)]
fn finish(world: &mut World) {
    let Some(spec) = world.get_resource::<Catalog>().map(|c| *c.utilities()) else {
        return;
    };
    let mut prev = world.remove_resource::<Utilities>().unwrap_or_default();
    let mut nodes = std::mem::take(&mut prev.pending);
    nodes.sort_by_key(|n| n.id);
    nodes.dedup_by_key(|n| n.id);
    let edges: Vec<(NodeId, NodeId, f32)> = world.get_resource::<Ids>().map_or(Vec::new(), |ids| {
        ids.segments
            .iter()
            .filter_map(|(_, e)| {
                let s = world.get::<RoadSegment>(e)?;
                Some((s.start(), s.end(), s.geom().length()))
            })
            .collect()
    });
    let g = network::Dense::build(&edges);
    let comp = g.components();
    let ends_idx: Vec<Option<(usize, usize)>> = nodes
        .iter()
        .map(|n| n.ends.and_then(|(a, b)| Some((g.index(a)?, g.index(b)?))))
        .collect();
    let rivers: Vec<Vec<Vec2>> = world
        .get_resource::<crate::terrain::TerrainRes>()
        .map(|t| t.rivers().to_vec())
        .unwrap_or_default();
    let prev_status = aligned(&nodes, &prev.status);
    let idx_of = |id: BuildingId| nodes.binary_search_by_key(&id, |n| n.id).ok();
    let mut status: Vec<Supply> = ends_idx
        .iter()
        .map(|e| Supply {
            connected: e.is_some(),
            ..Supply::default()
        })
        .collect();
    let mut networks: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (i, e) in ends_idx.iter().enumerate() {
        if let Some((a, _)) = e {
            networks.entry(comp[*a]).or_default().push(i);
        }
    }
    let mut power_bal = BTreeMap::new();
    let mut water_bal = BTreeMap::new();
    let mut river_injections: Vec<Vec<(usize, f32)>> = vec![Vec::new(); rivers.len()];
    let mut pumps: Vec<(u32, Vec2, f32)> = Vec::new();
    for (c, members) in &networks {
        let from = |pick: fn(&Profile) -> f32| {
            let src: Vec<usize> = members
                .iter()
                .filter(|i| pick(&nodes[**i].profile) > 0.0)
                .filter_map(|i| ends_idx[*i].map(|e| e.0))
                .collect();
            g.distances(&src)
        };
        let consumers = |pick: fn(&Profile) -> f32, dist: &[f32], served: fn(&Supply) -> bool| {
            members
                .iter()
                .filter(|i| pick(&nodes[**i].profile) > 0.0)
                .map(|i| Consumer {
                    id: nodes[*i].id,
                    priority: nodes[*i].profile.priority,
                    distance_m: ends_idx[*i].map_or(f32::INFINITY, |(a, b)| dist[a].min(dist[b])),
                    need: pick(&nodes[*i].profile),
                    was_served: prev_status[*i].as_ref().is_some_and(served),
                })
                .collect::<Vec<_>>()
        };
        let key = g.ids[*c as usize];
        // Power.
        let supply: f32 = members.iter().map(|i| nodes[*i].profile.power_out).sum();
        let d = from(|p| p.power_out);
        let cs = consumers(|p| p.power_kw, &d, |s| s.power);
        let need: f32 = cs.iter().map(|c| c.need).sum();
        power_bal.insert(
            key,
            Balance {
                supply,
                demand: need,
            },
        );
        for id in network::allocate(supply, &cs, spec.hysteresis) {
            if let Some(i) = idx_of(id) {
                status[i].power = true;
            }
        }
        // Water.
        let supply: f32 = members.iter().map(|i| nodes[*i].profile.water_out).sum();
        let d = from(|p| p.water_out);
        let cs = consumers(|p| p.water_m3h, &d, |s| s.water);
        let need: f32 = cs.iter().map(|c| c.need).sum();
        water_bal.insert(
            key,
            Balance {
                supply,
                demand: need,
            },
        );
        for id in network::allocate(supply, &cs, spec.hysteresis) {
            if let Some(i) = idx_of(id) {
                status[i].water = true;
            }
        }
        for i in members
            .iter()
            .filter(|i| nodes[**i].profile.water_out > 0.0)
        {
            pumps.push((*c, nodes[*i].pos, nodes[*i].profile.water_out));
        }
        // Sewage.
        let supply: f32 = members.iter().map(|i| nodes[*i].profile.sewage_out).sum();
        let d = from(|p| p.sewage_out);
        let cs = consumers(|p| p.sewage_m3h, &d, |s| s.sewage);
        let served = network::allocate(supply, &cs, spec.hysteresis);
        for id in &served {
            if let Some(i) = idx_of(*id) {
                status[i].sewage = true;
            }
        }
        let taken: f32 = cs
            .iter()
            .filter(|c| served.binary_search(&c.id).is_ok())
            .map(|c| c.need)
            .sum();
        // Each outlet discharges its share of the sewage taken into the nearest river.
        for i in members
            .iter()
            .filter(|i| nodes[**i].profile.sewage_out > 0.0)
        {
            let o = &nodes[*i];
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
    let mut polluted_out: BTreeMap<u32, (f32, f32)> = BTreeMap::new();
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
        if polluted_out.get(c).is_some_and(|(p, t)| *p * 2.0 > *t) {
            for i in members {
                if status[*i].water {
                    status[*i].polluted_water = true;
                }
            }
        }
    }
    // Garbage: accumulate, then landfills collect within their network.
    let mut garbage: Vec<Option<f32>> = nodes
        .iter()
        .zip(aligned(&nodes, &prev.garbage_kg))
        .map(|(n, g)| {
            if n.profile.garbage_kg_day > 0.0 && !n.profile.landfill {
                Some(g.unwrap_or(0.0) + n.profile.garbage_kg_day / 24.0)
            } else {
                g
            }
        })
        .collect();
    let mut landfill = prev.landfill_kg;
    landfill.retain(|id, _| idx_of(*id).is_some_and(|i| nodes[i].profile.landfill));
    for members in networks.values() {
        let fills: Vec<BuildingId> = members
            .iter()
            .filter(|i| nodes[**i].profile.landfill)
            .map(|i| nodes[*i].id)
            .collect();
        let room: f32 = fills
            .iter()
            .map(|id| {
                (spec.landfill_storage_kg - landfill.get(id).copied().unwrap_or(0.0)).max(0.0)
            })
            .sum();
        let capacity = (spec.landfill_collection_kg_day / 24.0 * fills.len() as f32).min(room);
        let stored: f32 = members.iter().map(|i| garbage[*i].unwrap_or(0.0)).sum();
        if capacity <= 0.0 || stored <= 0.0 {
            continue;
        }
        let ratio = (capacity / stored).min(1.0);
        let mut collected = 0.0;
        for i in members {
            if let Some(g) = garbage[*i].as_mut() {
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
    for (i, n) in nodes.iter().enumerate() {
        let limit = n.profile.garbage_kg_day * spec.garbage_problem_days;
        if limit > 0.0 && garbage[i].unwrap_or(0.0) > limit {
            status[i].garbage_problem = true;
        }
    }
    world.insert_resource(Utilities {
        status: nodes.iter().zip(status).map(|(n, s)| (n.id, s)).collect(),
        garbage_kg: nodes
            .iter()
            .zip(garbage)
            .filter_map(|(n, g)| Some((n.id, g?)))
            .collect(),
        landfill_kg: landfill,
        river_pollution,
        power: power_bal,
        water: water_bal,
        pending: Vec::new(),
    });
}

/// `TickSet::SubRate`: the hourly utility pass — building collection in [`COLLECT_SLICES`]
/// slices from tick [`OFFSET`], then the allocation.
pub(crate) fn tick(world: &mut World) {
    let now = world
        .get_resource::<crate::SimClock>()
        .map_or(0, |c| c.tick().0);
    let t = now % TICKS_PER_GAME_HOUR;
    if (OFFSET..OFFSET + COLLECT_SLICES).contains(&t) {
        collect_slice(world, t - OFFSET);
    } else if t == OFFSET + COLLECT_SLICES {
        finish(world);
    }
}
