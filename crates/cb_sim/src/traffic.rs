//! Bridge to `cb_traffic` (ARCHITECTURE §3, ALGORITHMS §12–§16): the link graph synced from the
//! road network, building road access, and the M5 **macro commute model**.
//!
//! - **Graph**: one link per (segment, driving direction with car lanes); speed from the road
//!   type, capacity = car lanes × capacity per lane. Rebuilt in `EditSet::Connectivity` when the
//!   network changed (a pure function of the roads, so a loaded game rebuilds the same graph).
//!   Per-link accumulators survive rebuilds by link key (segment id, direction).
//! - **Access**: a zoned building reaches the network through its plot's first frontage segment;
//!   it leaves towards the ends its segment lets it drive to and arrives from the ends that drive
//!   past it, each with the partial segment time.
//! - **Daily assignment** (every [`FLOW_PERIOD`] ticks one slice; slice `k` = households with
//!   `id % SLICES == k`): one bounded tree per home reaches all its workers' workplaces under the
//!   day costs; each worker gets its commute estimate, its route adds the household's car share
//!   to the links, and its departure (shift start − commute − slack) enters the histogram. At the
//!   day boundary the link totals become the BPR **day costs** for the next day and the
//!   commute statistics are published.
//! - **Traps**: nodes outside the largest strongly connected component (one-way traps); a
//!   building whose access only touches trapped nodes has no route (`NoRoute`).

use std::collections::BTreeMap;

use bevy::ecs::message::MessageReader;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::ResMut;
use bevy::ecs::world::World;
use cb_core::ids::{BuildingId, CitizenId, HouseholdId, NodeId, SegmentId};
use cb_core::rng::{RngStream, SimRng};
use cb_core::time::TICKS_PER_GAME_DAY;
use cb_network::{Dir, LaneKind, RoadKind};
use cb_traffic::{Graph, LinkSpec, Tree};
use glam::Vec2;
use smallvec::SmallVec;

use crate::buildings::Building;
use crate::catalog::ZoneKind;
use crate::catalog::{Catalog, CommuteSpec, ShiftSpec};
use crate::citizens::Population;
use crate::config::WorldSeed;
use crate::ids::Ids;
use crate::roads::{NetworkChanged, RoadNode, RoadSegment};
use crate::zoning::{Lot, Plot, ZoningChanged};

/// Ticks between two assignment slices.
pub const FLOW_PERIOD: u64 = 2;
/// Assignment slices per game day.
pub const SLICES: u64 = TICKS_PER_GAME_DAY / FLOW_PERIOD;
/// Half-hour departure bins.
pub const DEPARTURE_BINS: usize = 48;
/// Perceived-minute histogram range (the last bin collects longer commutes).
const PERCEIVED_BINS: usize = 121;
/// Sim seconds per sim hour over sim seconds per game hour.
const GAME_HOURS_PER_SIM_H: f32 = 30.0;
/// A game day in sim hours.
const SIM_H_PER_GAME_DAY: f32 = 0.8;
/// Commute value of a worker without a route.
pub const NO_ROUTE: u16 = u16::MAX;

/// `SimRng` purposes of the commute model (citizens stream).
pub(crate) mod purpose {
    /// Shift start at hiring.
    pub const SHIFT: u16 = 4;
    /// Daily departure slack.
    pub const SLACK: u16 = 5;
}

/// Commute statistics of the last complete game day.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CommuteStats {
    /// Departures per half hour of the game day (bin 0 = 00:00–00:30).
    pub departures: [u32; DEPARTURE_BINS],
    /// Workers assigned.
    pub commuters: u32,
    /// Workers without a route to their workplace.
    pub no_route: u32,
    /// Median perceived commute (min).
    pub p50_perceived_min: f32,
    /// 95th percentile (min).
    pub p95_perceived_min: f32,
}

impl Default for CommuteStats {
    fn default() -> Self {
        Self {
            departures: [0; DEPARTURE_BINS],
            commuters: 0,
            no_route: 0,
            p50_perceived_min: 0.0,
            p95_perceived_min: 0.0,
        }
    }
}

#[derive(Clone, Debug)]
struct DayAcc {
    departures: [u32; DEPARTURE_BINS],
    perceived: [u32; PERCEIVED_BINS],
    commuters: u32,
    no_route: u32,
}

impl Default for DayAcc {
    fn default() -> Self {
        Self {
            departures: [0; DEPARTURE_BINS],
            perceived: [0; PERCEIVED_BINS],
            commuters: 0,
            no_route: 0,
        }
    }
}

impl DayAcc {
    fn stats(&self) -> CommuteStats {
        let q = |p: f32| {
            let n = self.perceived.iter().sum::<u32>();
            if n == 0 {
                return 0.0;
            }
            let want = (p * n as f32).ceil().max(1.0) as u32;
            let mut seen = 0;
            for (m, c) in self.perceived.iter().enumerate() {
                seen += c;
                if seen >= want {
                    return m as f32;
                }
            }
            (PERCEIVED_BINS - 1) as f32
        };
        CommuteStats {
            departures: self.departures,
            commuters: self.commuters,
            no_route: self.no_route,
            p50_perceived_min: q(0.5),
            p95_perceived_min: q(0.95),
        }
    }
}

/// Where a building meets the network: its access segment and the building's position along it
/// (0 = segment start, 1 = end).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Access {
    /// Access segment.
    pub segment: SegmentId,
    /// Position along the segment (0–1).
    pub at: f32,
}

/// Graph terminals: (graph node, extra cost ms).
pub(crate) type Terminals = SmallVec<[(u32, u64); 2]>;

/// The link graph and the macro commute model.
#[derive(Resource, Clone, Debug)]
pub struct Traffic {
    graph: Graph,
    nodes: BTreeMap<NodeId, u32>,
    /// Road node per graph node index (ascending ids).
    node_ids: Vec<NodeId>,
    seg_links: BTreeMap<SegmentId, [Option<u32>; 2]>,
    version: u64,
    dirty: bool,
    trapped: Vec<bool>,
    /// Day costs per link (ms).
    day_ms: Vec<u32>,
    /// Car commuters assigned to each link today and on the last complete day.
    today: Vec<f32>,
    last: Vec<f32>,
    acc: DayAcc,
    stats: CommuteStats,
    tree: Tree,
    path: Vec<u32>,
    /// Access per building, cleared on every network or zoning edit.
    access: BTreeMap<BuildingId, Option<Access>>,
    /// Link totals restored from a save, applied by the next rebuild (by link key).
    carry: BTreeMap<u64, (f32, f32)>,
}

/// The saved part of [`Traffic`] (ADR-0068): link totals by link key, the day accumulators and
/// the published statistics. Everything else is rebuilt from the roads.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TrafficSave {
    /// `(link key, today, last day)`.
    pub links: Vec<(u64, f32, f32)>,
    /// Today's departures, perceived-minute histogram, commuters and no-route count.
    pub acc: (Vec<u32>, Vec<u32>, u32, u32),
    /// Published departures and `(commuters, no route, p50, p95)`.
    pub stats: (Vec<u32>, u32, u32, f32, f32),
}

impl Default for Traffic {
    fn default() -> Self {
        Self {
            graph: Graph::default(),
            nodes: BTreeMap::new(),
            node_ids: Vec::new(),
            seg_links: BTreeMap::new(),
            version: 0,
            dirty: true,
            trapped: Vec::new(),
            day_ms: Vec::new(),
            today: Vec::new(),
            last: Vec::new(),
            acc: DayAcc::default(),
            stats: CommuteStats::default(),
            tree: Tree::new(),
            path: Vec::new(),
            access: BTreeMap::new(),
            carry: BTreeMap::new(),
        }
    }
}

impl Traffic {
    /// The link graph.
    #[must_use]
    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    /// Graph version (bumped on every rebuild).
    #[must_use]
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Commute statistics of the last complete day.
    #[must_use]
    pub fn stats(&self) -> &CommuteStats {
        &self.stats
    }

    /// Links of a segment: (forward, backward).
    #[must_use]
    pub fn links_of(&self, seg: SegmentId) -> [Option<u32>; 2] {
        self.seg_links.get(&seg).copied().unwrap_or_default()
    }

    /// Day cost of a link (ms).
    #[must_use]
    pub fn day_ms(&self, link: u32) -> Option<u32> {
        self.day_ms.get(link as usize).copied()
    }

    /// Mean daily traffic on a segment, both directions (vehicle entities per sim hour, like
    /// agents count; commute trips both ways only).
    #[must_use]
    pub fn segment_volume_per_sim_h(&self, seg: SegmentId, trip_scale: f32) -> f32 {
        let total: f32 = self
            .links_of(seg)
            .iter()
            .flatten()
            .map(|l| self.last.get(*l as usize).copied().unwrap_or(0.0))
            .sum();
        // Morning trips on the assigned links, evening trips mirrored on the way back.
        2.0 * total / trip_scale.max(1.0) / SIM_H_PER_GAME_DAY
    }

    /// Peak-hour volume of a link (vehicle entities per sim hour).
    #[must_use]
    pub fn peak_volume_per_sim_h(&self, link: u32, spec: &CommuteSpec) -> f32 {
        self.last.get(link as usize).copied().unwrap_or(0.0) / spec.trip_scale.max(1.0)
            * spec.peak_hour_share
            * GAME_HOURS_PER_SIM_H
    }

    /// Cached [`access`] of a building.
    pub(crate) fn access_of(&mut self, world: &World, b: BuildingId) -> Option<Access> {
        *self.access.entry(b).or_insert_with(|| access(world, b))
    }

    /// Road node of a graph node index.
    #[must_use]
    pub fn node_id(&self, idx: u32) -> Option<NodeId> {
        self.node_ids.get(idx as usize).copied()
    }

    /// Day costs of all links (ms).
    #[must_use]
    pub fn day_costs(&self) -> &[u32] {
        &self.day_ms
    }

    /// Whether a graph node lies in a one-way trap.
    #[must_use]
    pub fn trapped(&self, node: u32) -> bool {
        self.trapped.get(node as usize).copied().unwrap_or(false)
    }

    pub(crate) fn origins(&self, a: Access) -> Terminals {
        let [f, b] = self.links_of(a.segment);
        let mut out = Terminals::new();
        let part = |l: u32, frac: f32| {
            (f64::from(self.day_ms[l as usize]) * f64::from(frac.clamp(0.0, 1.0))).round() as u64
        };
        if let Some(l) = f {
            out.push((self.graph.link(l).to, part(l, 1.0 - a.at)));
        }
        if let Some(l) = b {
            out.push((self.graph.link(l).to, part(l, a.at)));
        }
        out
    }

    pub(crate) fn dests(&self, a: Access) -> Terminals {
        let [f, b] = self.links_of(a.segment);
        let mut out = Terminals::new();
        let part = |l: u32, frac: f32| {
            (f64::from(self.day_ms[l as usize]) * f64::from(frac.clamp(0.0, 1.0))).round() as u64
        };
        if let Some(l) = f {
            out.push((self.graph.link(l).from, part(l, a.at)));
        }
        if let Some(l) = b {
            out.push((self.graph.link(l).from, part(l, 1.0 - a.at)));
        }
        out
    }

    /// Whether a building's access touches only trapped nodes (or none at all).
    #[must_use]
    pub fn no_route(&self, a: Option<Access>) -> bool {
        a.is_none_or(|a| {
            let o = self.origins(a);
            o.is_empty() || o.iter().all(|(n, _)| self.trapped(*n))
        })
    }

    /// Grows the tree from `home` until every destination is reached (or `bound_ms`).
    fn grow_from(&mut self, home: Access, targets: &[Terminals], bound_ms: u64) {
        let sources = self.origins(home);
        let mut nodes: SmallVec<[u32; 16]> = targets.iter().flatten().map(|t| t.0).collect();
        nodes.sort_unstable();
        nodes.dedup();
        let Self {
            tree,
            graph,
            day_ms,
            ..
        } = self;
        tree.grow(graph, day_ms, &sources, bound_ms, &nodes);
    }

    /// Cost (ms) to a destination from the last grown tree: the best end and its node.
    fn cost_to(&self, dests: &Terminals) -> Option<(u64, u32)> {
        dests
            .iter()
            .filter_map(|(n, extra)| self.tree.cost(*n).map(|c| (c + extra, *n)))
            .min()
    }

    /// The saved state.
    #[must_use]
    pub fn to_save(&self) -> TrafficSave {
        let mut links: Vec<(u64, f32, f32)> = (0..self.graph.link_count())
            .map(|l| {
                (
                    self.graph.link(l as u32).key,
                    self.today.get(l).copied().unwrap_or(0.0),
                    self.last.get(l).copied().unwrap_or(0.0),
                )
            })
            .chain(self.carry.iter().map(|(k, (t, y))| (*k, *t, *y)))
            .collect();
        links.sort_by_key(|x| x.0);
        TrafficSave {
            links,
            acc: (
                self.acc.departures.to_vec(),
                self.acc.perceived.to_vec(),
                self.acc.commuters,
                self.acc.no_route,
            ),
            stats: (
                self.stats.departures.to_vec(),
                self.stats.commuters,
                self.stats.no_route,
                self.stats.p50_perceived_min,
                self.stats.p95_perceived_min,
            ),
        }
    }

    /// Traffic from a save; the graph is rebuilt (and the totals re-attached) on the next tick
    /// or edit.
    #[must_use]
    pub fn from_save(s: &TrafficSave) -> Self {
        let mut t = Self {
            carry: s.links.iter().map(|(k, a, b)| (*k, (*a, *b))).collect(),
            ..Self::default()
        };
        for (dst, src) in t.acc.departures.iter_mut().zip(&s.acc.0) {
            *dst = *src;
        }
        for (dst, src) in t.acc.perceived.iter_mut().zip(&s.acc.1) {
            *dst = *src;
        }
        t.acc.commuters = s.acc.2;
        t.acc.no_route = s.acc.3;
        for (dst, src) in t.stats.departures.iter_mut().zip(&s.stats.0) {
            *dst = *src;
        }
        t.stats.commuters = s.stats.1;
        t.stats.no_route = s.stats.2;
        t.stats.p50_perceived_min = s.stats.3;
        t.stats.p95_perceived_min = s.stats.4;
        t
    }

    fn rebuild(&mut self, world: &World) {
        let mut old: BTreeMap<u64, (f32, f32)> = std::mem::take(&mut self.carry);
        old.extend((0..self.graph.link_count() as u32).map(|l| {
            (
                self.graph.link(l).key,
                (
                    self.today.get(l as usize).copied().unwrap_or(0.0),
                    self.last.get(l as usize).copied().unwrap_or(0.0),
                ),
            )
        }));
        let (graph, nodes, seg_links) = build_graph(world);
        let n = graph.link_count();
        self.today = vec![0.0; n];
        self.last = vec![0.0; n];
        for l in 0..n {
            if let Some((t, y)) = old.get(&graph.link(l as u32).key) {
                self.today[l] = *t;
                self.last[l] = *y;
            }
        }
        let mut trapped = vec![false; graph.node_count()];
        for v in cb_traffic::scc::trapped(&graph) {
            trapped[v as usize] = true;
        }
        self.trapped = trapped;
        self.graph = graph;
        self.node_ids = nodes.keys().copied().collect();
        self.nodes = nodes;
        self.seg_links = seg_links;
        self.version += 1;
        self.dirty = false;
        let spec = world
            .get_resource::<Catalog>()
            .map(|c| c.citizens().commute);
        self.update_day_costs(spec.as_ref());
    }

    fn update_day_costs(&mut self, spec: Option<&CommuteSpec>) {
        self.day_ms = (0..self.graph.link_count() as u32)
            .map(|l| {
                let free = self.graph.free_ms(l);
                let Some(spec) = spec else {
                    return free;
                };
                let v = self.peak_volume_per_sim_h(l, spec);
                let c = self.graph.link(l).capacity_per_sim_h;
                let f = 1.0 / cb_traffic::flow::speed_factor(v, c);
                (f64::from(free) * f64::from(f))
                    .round()
                    .min(f64::from(u32::MAX)) as u32
            })
            .collect();
    }

    fn close_day(&mut self, spec: &CommuteSpec) {
        self.last = std::mem::replace(&mut self.today, vec![0.0; self.graph.link_count()]);
        self.stats = self.acc.stats();
        self.acc = DayAcc::default();
        self.update_day_costs(Some(spec));
    }
}

/// Car lanes `(forward, backward)` of a segment.
fn car_lanes(seg: &RoadSegment) -> (usize, usize) {
    let lanes = seg.cross_section().lanes();
    let count = |d: Dir| {
        lanes
            .iter()
            .filter(|l| l.dir == d && l.kind == LaneKind::Car)
            .count()
    };
    (count(Dir::Forward), count(Dir::Backward))
}

type Built = (
    Graph,
    BTreeMap<NodeId, u32>,
    BTreeMap<SegmentId, [Option<u32>; 2]>,
);

/// Builds the link graph from the road network (nodes and segments in ID order).
fn build_graph(world: &World) -> Built {
    let (Some(ids), Some(cat)) = (world.get_resource::<Ids>(), world.get_resource::<Catalog>())
    else {
        return (Graph::default(), BTreeMap::new(), BTreeMap::new());
    };
    let mut nodes = BTreeMap::new();
    let mut pos = Vec::new();
    for (id, e) in ids.nodes.iter() {
        if let Some(n) = world.get::<RoadNode>(e) {
            nodes.insert(id, pos.len() as u32);
            pos.push(Vec2::new(n.pos().x, n.pos().z));
        }
    }
    let mut links = Vec::new();
    let mut seg_links = BTreeMap::new();
    for (sid, e) in ids.segments.iter() {
        let Some(s) = world.get::<RoadSegment>(e) else {
            continue;
        };
        let (Some(&a), Some(&b)) = (nodes.get(&s.start()), nodes.get(&s.end())) else {
            continue;
        };
        let (fwd, bwd) = car_lanes(s);
        let line = &s.geom().centerline;
        let len = line.length().max(0.1);
        let t0 = line.tangent_at(0.0);
        let t1 = line.tangent_at(len);
        let rt = cat.road(s.kind());
        let speed = rt.speed_kmh / 3.6;
        let dirt = s.kind() == RoadKind::Dirt;
        let mut pair = [None, None];
        for (k, lanes, from, to, d_out, d_in) in
            [(0u64, fwd, a, b, t0, t1), (1u64, bwd, b, a, -t1, -t0)]
        {
            if lanes == 0 {
                continue;
            }
            pair[k as usize] = Some(links.len() as u32);
            links.push(LinkSpec {
                from,
                to,
                length_m: len,
                speed_mps: speed,
                dirt,
                capacity_per_sim_h: lanes as f32 * rt.capacity_per_lane_per_sim_h,
                dir_out: d_out,
                dir_in: d_in,
                key: (u64::from(sid.get()) << 1) | k,
            });
        }
        if pair != [None, None] {
            seg_links.insert(sid, pair);
        }
    }
    (Graph::build(pos, links), nodes, seg_links)
}

/// Road access of a zoned building: the projection of its lot's frontage midpoint onto the
/// nearest of the plot's frontage segments (ALGORITHMS §14). Depends only on lots, plots and
/// roads, which change in `SimEdit`, so [`Traffic`] can cache it between edits.
#[must_use]
pub fn access(world: &World, b: BuildingId) -> Option<Access> {
    let ids = world.get_resource::<Ids>()?;
    let bld = world.get::<Building>(ids.buildings.get(b)?)?;
    let lot = world.get::<Lot>(ids.lots.get(bld.lot()?)?)?;
    let plot = world.get::<Plot>(ids.plots.get(lot.plot())?)?;
    let (f0, f1) = lot.front();
    let mid = 0.5 * (f0 + f1);
    plot.segments()
        .iter()
        .filter_map(|sid| {
            let seg = world.get::<RoadSegment>(ids.segments.get(*sid)?)?;
            let line = &seg.geom().centerline;
            let (s, d) = line.project(mid);
            Some((d.abs(), *sid, s / line.length().max(0.1)))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|(_, segment, at)| Access {
            segment,
            at: at.clamp(0.0, 1.0),
        })
}

/// Whether a building cannot reach the city (no access, or a one-way trap).
#[must_use]
pub fn building_no_route(world: &World, b: BuildingId) -> bool {
    world
        .get_resource::<Traffic>()
        .is_none_or(|t| t.no_route(access(world, b)))
}

/// Samples a shift start (game minutes after midnight, ADR-0062).
pub(crate) fn sample_shift(spec: &ShiftSpec, seed: u64, now: u64, citizen: CitizenId) -> u16 {
    let mut rng = SimRng::for_event(
        seed,
        RngStream::Citizens,
        purpose::SHIFT,
        now,
        u64::from(citizen.get()),
    );
    let (lo, hi) = spec.range_min;
    let mut x = rng.normal(spec.mean_min, spec.sd_min);
    for _ in 0..8 {
        if (lo..=hi).contains(&x) {
            break;
        }
        x = rng.normal(spec.mean_min, spec.sd_min);
    }
    x = x.clamp(lo, hi);
    if rng.chance(spec.alt_share) {
        x += spec.alt_offset_min;
    }
    x.rem_euclid(1440.0) as u16
}

/// Shift spec of a workplace zone.
pub(crate) fn shift_spec(spec: &CommuteSpec, zone: Option<ZoneKind>) -> &ShiftSpec {
    if zone == Some(ZoneKind::Industrial) {
        &spec.industrial
    } else {
        &spec.commercial
    }
}

/// `EditSet::Connectivity`: marks the graph dirty when the network changed and drops cached
/// access after network or zoning edits.
pub(crate) fn mark_dirty(
    mut network: MessageReader<NetworkChanged>,
    mut zoning: MessageReader<ZoningChanged>,
    mut t: ResMut<Traffic>,
) {
    let net = network.read().count() > 0;
    let zon = zoning.read().count() > 0;
    if net {
        t.dirty = true;
    }
    if net || zon {
        t.access.clear();
    }
}

/// Rebuilds the graph if needed (after edits in `EditSet::Connectivity`, after load in the tick).
pub(crate) fn rebuild_if_dirty(world: &mut World) {
    if !world.get_resource::<Traffic>().is_some_and(|t| t.dirty) {
        return;
    }
    let _span = tracing::info_span!("traffic_graph_rebuild").entered();
    world.resource_scope(|world, mut t: bevy::ecs::world::Mut<Traffic>| t.rebuild(world));
}

/// One worker's assignment input.
struct Worker {
    id: CitizenId,
    work: BuildingId,
    shift: u16,
    car_share: f32,
}

/// One assignment slice: the households of slice `k`.
fn assign_slice(world: &mut World, now: u64, k: u64) {
    let Some(spec) = world
        .get_resource::<Catalog>()
        .map(|c| c.citizens().commute)
    else {
        return;
    };
    let seed = world.get_resource::<WorldSeed>().map_or(0, |s| s.0);
    let day = now / TICKS_PER_GAME_DAY;
    // Households of the slice, grouped by home.
    let mut homes: BTreeMap<BuildingId, Vec<Worker>> = BTreeMap::new();
    {
        let Some(pop) = world.get_resource::<Population>() else {
            return;
        };
        let max = u64::from(pop.household_ids.peek_next());
        let mut raw = k;
        while raw < max {
            if let Some(h) = u32::try_from(raw)
                .ok()
                .and_then(|r| HouseholdId::try_new(r).ok())
                .and_then(|hid| pop.households.get(&hid))
            {
                let workers: Vec<(CitizenId, BuildingId)> = h
                    .members
                    .iter()
                    .filter_map(|c| Some((*c, pop.citizens.get(c)?.job()?)))
                    .collect();
                if !workers.is_empty() {
                    let car_share = (f32::from(h.cars) / workers.len() as f32).min(1.0);
                    let list = homes.entry(h.home).or_default();
                    for (c, b) in workers {
                        list.push(Worker {
                            id: c,
                            work: b,
                            shift: pop.citizens.get(&c).map_or(0, |c| c.shift_start_min),
                            car_share,
                        });
                    }
                }
            }
            raw += SLICES;
        }
    }
    if homes.is_empty() {
        return;
    }
    let mut results: Vec<(CitizenId, u16)> = Vec::new();
    world.resource_scope(|world, mut t: bevy::ecs::world::Mut<Traffic>| {
        for (home, workers) in &homes {
            let acc = t.access_of(world, *home);
            let work: Vec<Option<Access>> =
                workers.iter().map(|w| t.access_of(world, w.work)).collect();
            let dests: Vec<Terminals> = work
                .iter()
                .map(|w| w.map(|a| t.dests(a)).unwrap_or_default())
                .collect();
            let reachable = acc.is_some_and(|a| !t.origins(a).is_empty());
            if let (Some(a), true) = (acc, reachable) {
                t.grow_from(a, &dests, u64::MAX);
            }
            for ((w, d), wa) in workers.iter().zip(&dests).zip(&work) {
                let best = if reachable { t.cost_to(d) } else { None };
                t.acc.commuters += 1;
                let Some((ms, node)) = best else {
                    t.acc.no_route += 1;
                    results.push((w.id, NO_ROUTE));
                    continue;
                };
                let sim_s = ms as f32 / 1000.0 + 2.0 * spec.terminal_sim_s;
                let commute = (sim_s.round() as u32).min(u32::from(NO_ROUTE - 1)) as u16;
                results.push((w.id, commute));
                // Link volumes: the tree path plus the partial access links at both ends.
                let mut path = std::mem::take(&mut t.path);
                t.tree.path(node, &mut path);
                if let Some(a) = acc {
                    let first_node = path.first().map(|l| t.graph.link(*l).from).unwrap_or(node);
                    let [f, b] = t.links_of(a.segment);
                    for l in [f, b].into_iter().flatten() {
                        if t.graph.link(l).to == first_node {
                            path.push(l);
                            break;
                        }
                    }
                }
                if let Some(wa) = wa {
                    let [f, b] = t.links_of(wa.segment);
                    for l in [f, b].into_iter().flatten() {
                        if t.graph.link(l).from == node {
                            path.push(l);
                            break;
                        }
                    }
                }
                for l in &path {
                    if let Some(v) = t.today.get_mut(*l as usize) {
                        *v += w.car_share;
                    }
                }
                t.path = path;
                let perceived = (sim_s * 0.1).round() as usize;
                t.acc.perceived[perceived.min(PERCEIVED_BINS - 1)] += 1;
                let slack = SimRng::for_event(
                    seed,
                    RngStream::Citizens,
                    purpose::SLACK,
                    day,
                    u64::from(w.id.get()),
                )
                .normal(spec.slack_min.0, spec.slack_min.1)
                .max(0.0);
                let depart = (f32::from(w.shift) - sim_s / 2.0 - slack).rem_euclid(1440.0);
                let bin = ((depart / 30.0) as usize).min(DEPARTURE_BINS - 1);
                t.acc.departures[bin] += 1;
            }
        }
    });
    let Some(mut pop) = world.get_resource_mut::<Population>() else {
        return;
    };
    for (c, s) in results {
        if let Some(c) = pop.citizens.get_mut(&c) {
            c.commute_sim_s = s;
        }
    }
}

/// `TickSet::SubRate` (every tick): graph rebuild after load, day close and assignment slices.
pub(crate) fn tick(world: &mut World) {
    rebuild_if_dirty(world);
    let now = world
        .get_resource::<crate::SimClock>()
        .map_or(0, |c| c.tick().0);
    let t = now % TICKS_PER_GAME_DAY;
    if !t.is_multiple_of(FLOW_PERIOD) {
        return;
    }
    let k = t / FLOW_PERIOD;
    if k == 0
        && now > 0
        && let Some(spec) = world
            .get_resource::<Catalog>()
            .map(|c| c.citizens().commute)
        && let Some(mut tr) = world.get_resource_mut::<Traffic>()
    {
        tr.close_day(&spec);
    }
    let _span = tracing::info_span!("commute_assign").entered();
    assign_slice(world, now, k);
}
