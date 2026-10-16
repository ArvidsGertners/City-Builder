//! Road network (DATA_MODEL §4, ALGORITHMS §1–§5): nodes and segments as entities, building with
//! crossing resolution and segment splitting, junction geometry, corridor stamps and bulldozing.
//!
//! A `BuildRoad` command is **planned** first without touching the world (endpoints, profile
//! fixed point, shape validation, splits of existing segments, junction validation of every
//! affected node) and only then charged and applied, so a rejected command never leaves a
//! half-built network.

use std::collections::BTreeMap;

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::message::Message;
use bevy::ecs::world::World;
use cb_core::Money;
use cb_core::ids::{BuildingId, NodeId, SegmentId};
use cb_geometry::{CubicBezier2, Polyline2};
use cb_network::control::{ApproachControl, ControlMode, SignalPhase};
use cb_network::junction::{self, Approach, Junction};
use cb_network::markings::Markings;
use cb_network::validation::{self, CrossingKind, Existing, Ground, RoadLimits};
use cb_network::{CrossSection, Profile, ProfileInput, RoadKind, SegmentGeom, Variant};
use cb_terrain::stamps::priority;
use cb_terrain::{Stamp, StampShape};
use glam::{Vec2, Vec3};
use smallvec::SmallVec;

use crate::catalog::Catalog;
use crate::commands::{BuildRoad, Endpoint, ValidationError};
use crate::ids::Ids;
use crate::terrain;

/// Corridor shoulder beyond the road edge (ADR-0065).
pub const SHOULDER_M: f32 = 3.5;
/// Corridor carve below the surface (ADR-0065).
pub const CARVE_M: f32 = 0.15;
/// Endpoint snapping tolerance (m).
pub const SNAP_TOL_M: f32 = 0.5;
/// A crossing this close to the end of an existing segment goes through its node (m).
const NODE_CROSSING_M: f32 = 1.0;
/// Two segments meeting within this of straight form a plain continuation (no junction patch).
pub const CONTINUATION_DEG: f32 = 10.0;

/// A road node.
#[derive(Component, Clone, Debug)]
pub struct RoadNode {
    pub(crate) id: NodeId,
    pub(crate) pos: Vec3,
    pub(crate) segments: SmallVec<[SegmentId; 4]>,
    pub(crate) junction: Option<Junction>,
    /// Approaches in junction input order: (segment, node is its curve start).
    pub(crate) approaches: SmallVec<[(SegmentId, bool); 4]>,
    /// Terrain stamp of the junction surface (`priority::JUNCTION`).
    pub(crate) junction_stamp: Option<u32>,
    /// Player-chosen control mode.
    pub(crate) control: ControlMode,
    /// Player-set classes for `ControlMode::Custom`: (segment, node is its start, class).
    pub(crate) custom: SmallVec<[(SegmentId, bool, ApproachControl); 4]>,
    /// Effective class per approach (approach order); derived.
    pub(crate) controls: SmallVec<[ApproachControl; 4]>,
    /// Signal phases (connector indices of the junction) under `ControlMode::Lights`; derived.
    pub(crate) phases: Vec<SignalPhase>,
    /// Crosswalks, stop lines and lane arrows; derived.
    pub(crate) markings: Markings,
    /// Member of a detected roundabout ring; derived.
    pub(crate) ring: bool,
}

impl RoadNode {
    /// Stable id.
    #[must_use]
    pub fn id(&self) -> NodeId {
        self.id
    }
    /// Position (y = road surface).
    #[must_use]
    pub fn pos(&self) -> Vec3 {
        self.pos
    }
    /// Incident segments.
    #[must_use]
    pub fn segments(&self) -> &[SegmentId] {
        &self.segments
    }
    /// Junction geometry (nodes with ≥ 2 segments that are not plain continuations).
    #[must_use]
    pub fn junction(&self) -> Option<&Junction> {
        self.junction.as_ref()
    }
    /// Approaches in junction input order: (segment, node is its curve start); junction
    /// approach and connector indices refer to this order.
    #[must_use]
    pub fn approaches(&self) -> &[(SegmentId, bool)] {
        &self.approaches
    }
    /// Player-chosen control mode.
    #[must_use]
    pub fn control(&self) -> ControlMode {
        self.control
    }
    /// Player-set classes (`ControlMode::Custom`).
    #[must_use]
    pub fn custom(&self) -> &[(SegmentId, bool, ApproachControl)] {
        &self.custom
    }
    /// Effective class per approach (approach order).
    #[must_use]
    pub fn controls(&self) -> &[ApproachControl] {
        &self.controls
    }
    /// Signal phases (empty unless lights).
    #[must_use]
    pub fn phases(&self) -> &[SignalPhase] {
        &self.phases
    }
    /// Junction markings.
    #[must_use]
    pub fn markings(&self) -> &Markings {
        &self.markings
    }
    /// Part of a roundabout ring.
    #[must_use]
    pub fn ring(&self) -> bool {
        self.ring
    }
}

/// A road segment.
#[derive(Component, Clone, Debug)]
pub struct RoadSegment {
    pub(crate) id: SegmentId,
    pub(crate) start: NodeId,
    pub(crate) end: NodeId,
    pub(crate) kind: RoadKind,
    /// Bus lanes / tram tracks.
    pub(crate) variant: Variant,
    pub(crate) curve: CubicBezier2,
    pub(crate) profile: Profile,
    pub(crate) built_tick: u64,
    pub(crate) geom: SegmentGeom,
    pub(crate) cut: (f32, f32),
    pub(crate) stamp: u32,
}

impl RoadSegment {
    /// Stable id.
    #[must_use]
    pub fn id(&self) -> SegmentId {
        self.id
    }
    /// Start node.
    #[must_use]
    pub fn start(&self) -> NodeId {
        self.start
    }
    /// End node.
    #[must_use]
    pub fn end(&self) -> NodeId {
        self.end
    }
    /// Road type.
    #[must_use]
    pub fn kind(&self) -> RoadKind {
        self.kind
    }
    /// Horizontal alignment.
    #[must_use]
    pub fn curve(&self) -> &CubicBezier2 {
        &self.curve
    }
    /// Vertical profile (surface heights).
    #[must_use]
    pub fn profile(&self) -> &Profile {
        &self.profile
    }
    /// Derived geometry.
    #[must_use]
    pub fn geom(&self) -> &SegmentGeom {
        &self.geom
    }
    /// Junction cut-backs at the start and end (m).
    #[must_use]
    pub fn cuts(&self) -> (f32, f32) {
        self.cut
    }
    /// Tick it was built.
    #[must_use]
    pub fn built_tick(&self) -> u64 {
        self.built_tick
    }
    /// Bus lanes / tram tracks.
    #[must_use]
    pub fn variant(&self) -> Variant {
        self.variant
    }
    /// Cross-section (with its variant's lane kinds).
    #[must_use]
    pub fn cross_section(&self) -> CrossSection {
        CrossSection::with(self.kind, self.variant)
    }
}

/// Network changes for view sync and dependent systems.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkChanged {
    /// A segment was added or its geometry/cuts changed.
    Segment(SegmentId),
    /// A segment was removed.
    SegmentRemoved(SegmentId),
    /// A node was added or its junction changed.
    Node(NodeId),
    /// A node was removed.
    NodeRemoved(NodeId),
}

/// Half width of the carriageway (curb line) of a cross-section.
#[must_use]
pub fn carriageway_half_m(cs: &CrossSection) -> f32 {
    cs.carriageway_half_m()
}

fn limits(cat: &Catalog, kind: RoadKind) -> RoadLimits {
    let rt = cat.road(kind);
    RoadLimits {
        half_width_m: CrossSection::of(kind).half_width_m(),
        max_grade: rt.max_grade,
        min_radius_m: rt.min_radius_m,
    }
}

/// Construction cost of a road of `len_m` (integer cents, deterministic rounding).
#[must_use]
pub fn road_cost(cat: &Catalog, kind: RoadKind, len_m: f32) -> Money {
    road_cost_with(cat, kind, Variant::default(), len_m)
}

/// Construction cost of a road of `len_m` with bus lanes / tram tracks.
#[must_use]
pub fn road_cost_with(cat: &Catalog, kind: RoadKind, variant: Variant, len_m: f32) -> Money {
    let mm = (len_m * 1000.0).round() as i64;
    let per_m = cat.road(kind).cost_cents_per_m + cat.variant_cost_cents_per_m(variant);
    Money(mm * per_m / 1000)
}

/// Sub-profile of `p` over `[s0, s1]` (arc length), resampled at the standard spacing.
fn profile_piece(p: &Profile, s0: f32, s1: f32) -> Profile {
    let s = cb_network::profile::sample_positions((s1 - s0).max(0.01));
    let h = s.iter().map(|x| p.height_at(s0 + x)).collect();
    Profile::new(s, h)
}

/// Re-solves a piece of an existing road so its ends meet new node heights.
fn resolve_piece(p: &Profile, s0: f32, s1: f32, h0: f32, h1: f32, max_grade: f32) -> Profile {
    let s = cb_network::profile::sample_positions((s1 - s0).max(0.01));
    let h: Vec<f32> = s.iter().map(|x| p.height_at(s0 + x)).collect();
    let inp = ProfileInput::ground(s.clone(), h.clone(), h0, h1, max_grade);
    cb_network::profile::solve(&inp).unwrap_or_else(|_| Profile::new(s, h))
}

/// Splits a curve at arc lengths (ascending, exclusive of the ends) into pieces.
fn split_curve(curve: &CubicBezier2, cuts_s: &[f32]) -> Vec<CubicBezier2> {
    let lut = cb_geometry::ArcLengthLut::new(curve);
    let mut ts: Vec<f32> = vec![0.0];
    ts.extend(cuts_s.iter().map(|s| lut.t_at(*s)));
    ts.push(1.0);
    ts.windows(2).map(|w| curve.sub(w[0], w[1])).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum NodeRef {
    Old(NodeId),
    New(usize),
}

#[derive(Clone, Debug)]
struct PlannedSeg {
    kind: RoadKind,
    variant: Variant,
    curve: CubicBezier2,
    profile: Profile,
    start: NodeRef,
    end: NodeRef,
    built_tick: u64,
}

#[derive(Default)]
struct Plan {
    new_nodes: Vec<Vec3>,
    remove: Vec<SegmentId>,
    add: Vec<PlannedSeg>,
}

/// Snapshot of an existing segment for planning.
#[derive(Clone)]
struct Snap {
    id: SegmentId,
    kind: RoadKind,
    variant: Variant,
    start: NodeId,
    end: NodeId,
    curve: CubicBezier2,
    profile: Profile,
    geom: SegmentGeom,
    built_tick: u64,
}

fn snapshot(world: &World) -> (Vec<Snap>, BTreeMap<NodeId, (Vec3, Vec<SegmentId>)>) {
    let Some(ids) = world.get_resource::<Ids>() else {
        return (Vec::new(), BTreeMap::new());
    };
    let segs = ids
        .segments
        .iter()
        .filter_map(|(_, e)| world.get::<RoadSegment>(e))
        .map(|s| Snap {
            id: s.id,
            kind: s.kind,
            variant: s.variant,
            start: s.start,
            end: s.end,
            curve: s.curve,
            profile: s.profile.clone(),
            geom: s.geom.clone(),
            built_tick: s.built_tick,
        })
        .collect();
    let nodes = ids
        .nodes
        .iter()
        .filter_map(|(id, e)| {
            world
                .get::<RoadNode>(e)
                .map(|n| (id, (n.pos, n.segments.to_vec())))
        })
        .collect();
    (segs, nodes)
}

/// Approach of a (planned or existing) segment at one of its nodes.
fn approach(
    cat: &Catalog,
    (kind, variant): (RoadKind, Variant),
    geom: &SegmentGeom,
    profile: &Profile,
    at_start: bool,
) -> Approach {
    let cs = CrossSection::with(kind, variant);
    let centerline = if at_start {
        geom.centerline.clone()
    } else {
        Polyline2::new(geom.centerline.points().iter().rev().copied())
    };
    Approach {
        centerline,
        half_width_m: cs.half_width_m(),
        carriageway_half_m: carriageway_half_m(&cs),
        corner_radius_m: cat.road(kind).corner_radius_m,
        profile: profile.clone(),
        reversed: !at_start,
        lanes: cb_network::connectors::approach_lanes(&cs, at_start),
    }
}

/// Junction for a node with the given approaches (`None` for dead ends and continuations).
fn node_junction(
    apps: &[(RoadKind, Approach)],
) -> Option<Result<Junction, junction::JunctionError>> {
    match apps.len() {
        0 | 1 => None,
        2 => {
            let (a, b) = (&apps[0], &apps[1]);
            let (ta, tb) = (
                a.1.centerline.tangent_at(0.0),
                b.1.centerline.tangent_at(0.0),
            );
            let straight =
                ta.dot(tb) < -cb_core::math::cos(cb_core::math::to_radians(CONTINUATION_DEG));
            if straight && a.0 == b.0 {
                None
            } else {
                Some(junction::build(&[a.1.clone(), b.1.clone()]))
            }
        }
        _ => Some(junction::build(
            &apps.iter().map(|a| a.1.clone()).collect::<Vec<_>>(),
        )),
    }
}

/// Plans a new road.
fn plan_build(
    world: &World,
    cmd: &BuildRoad,
    tick: u64,
) -> Result<(Plan, Money, Profile), ValidationError> {
    let Some(cat) = world.get_resource::<Catalog>() else {
        return Err(ValidationError::new("internal-no-catalog"));
    };
    let lim = limits(cat, cmd.kind);
    let geom = SegmentGeom::new(cmd.curve, lim.half_width_m);
    let len = geom.length();
    let ground = terrain::ground(world);
    let (snaps, nodes) = snapshot(world);
    let index_of = |id: SegmentId| snaps.iter().position(|s| s.id == id);

    // Endpoints: height and which existing segments they touch.
    let resolve = |ep: Endpoint, p: Vec2| -> Result<(f32, Option<(usize, f32)>), ValidationError> {
        match ep {
            Endpoint::Free => Ok((ground.height(p), None)),
            Endpoint::Node(n) => {
                let (pos, _) = nodes
                    .get(&n)
                    .ok_or(ValidationError::new("road-invalid-target-missing"))?;
                if Vec2::new(pos.x, pos.z).distance(p) > SNAP_TOL_M {
                    return Err(ValidationError::at("road-invalid-target-missing", p));
                }
                Ok((pos.y, None))
            }
            Endpoint::Segment(sid) => {
                let k = index_of(sid).ok_or(ValidationError::new("road-invalid-target-missing"))?;
                let (s, d) = snaps[k].geom.project(p);
                if d.abs() > SNAP_TOL_M {
                    return Err(ValidationError::at("road-invalid-target-missing", p));
                }
                Ok((snaps[k].profile.height_at(s), Some((k, s))))
            }
        }
    };
    let (h0, split0) = resolve(cmd.start, cmd.curve.start())?;
    let (h1, split1) = resolve(cmd.end, cmd.curve.end())?;

    // Existing segments near the new corridor.
    let (lo, hi) = cmd.curve.bounds();
    let pad = lim.half_width_m + 30.0;
    let near: Vec<usize> = (0..snaps.len())
        .filter(|&k| {
            let (a, b) = snaps[k].curve.bounds();
            let r = snaps[k].geom.half_width_m + pad;
            a.x - r <= hi.x && lo.x <= b.x + r && a.y - r <= hi.y && lo.y <= b.y + r
        })
        .collect();
    let shares = |k: usize| -> Option<Vec2> {
        let s = &snaps[k];
        let touch = |ep: Endpoint, split: Option<(usize, f32)>, p: Vec2| match ep {
            Endpoint::Node(n) if s.start == n || s.end == n => Some(p),
            Endpoint::Segment(_) if split.is_some_and(|(kk, _)| kk == k) => Some(p),
            _ => None,
        };
        touch(cmd.start, split0, cmd.curve.start())
            .or_else(|| touch(cmd.end, split1, cmd.curve.end()))
    };
    let existing: Vec<Existing<'_>> = near
        .iter()
        .map(|&k| Existing {
            geom: &snaps[k].geom,
            profile: &snaps[k].profile,
            shares_node_at: shares(k),
        })
        .collect();
    let base = validation::ground_input(&geom, &ground, h0, h1, &lim);
    let (profile, crossings) = validation::resolve_crossings(&geom, &base, &existing)?;
    let plain: Vec<_> = crossings.iter().map(|c| c.0).collect();
    if let Some(e) = validation::validate_shape(&geom, &lim, &ground, &existing, &plain).first() {
        return Err((*e).into());
    }
    if let Some((c, _)) = crossings.iter().find(|(_, k)| *k == CrossingKind::Overpass) {
        return Err(ValidationError::at(
            "road-invalid-overpass-unavailable",
            c.at,
        ));
    }

    // Topology.
    let mut plan = Plan::default();
    let mut new_node = |plan: &mut Plan, p: Vec3| {
        plan.new_nodes.push(p);
        NodeRef::New(plan.new_nodes.len() - 1)
    };
    let endpoint_ref =
        |plan: &mut Plan,
         ep: Endpoint,
         p: Vec2,
         h: f32,
         new_node: &mut dyn FnMut(&mut Plan, Vec3) -> NodeRef| match ep {
            Endpoint::Node(n) => NodeRef::Old(n),
            Endpoint::Free | Endpoint::Segment(_) => new_node(plan, Vec3::new(p.x, h, p.y)),
        };
    let start_ref = endpoint_ref(&mut plan, cmd.start, cmd.curve.start(), h0, &mut new_node);
    let end_ref = endpoint_ref(&mut plan, cmd.end, cmd.curve.end(), h1, &mut new_node);
    // Splits of existing segments: snapshot index → [(s, node, height)].
    let mut splits: BTreeMap<usize, Vec<(f32, NodeRef, f32)>> = BTreeMap::new();
    if let Some((k, s)) = split0 {
        splits.entry(k).or_default().push((s, start_ref, h0));
    }
    if let Some((k, s)) = split1 {
        splits.entry(k).or_default().push((s, end_ref, h1));
    }
    // Junction crossings split both roads. A crossing through an existing node (reported once
    // per segment meeting there) joins that node instead of splitting at a segment end.
    let mut new_cuts: Vec<(f32, NodeRef)> = Vec::new();
    for (c, _) in crossings
        .iter()
        .filter(|(_, k)| *k == CrossingKind::Junction)
    {
        let other = &snaps[near[c.other]];
        let olen = other.geom.length();
        let at_node = if c.s_other < NODE_CROSSING_M {
            Some(other.start)
        } else if c.s_other > olen - NODE_CROSSING_M {
            Some(other.end)
        } else {
            None
        };
        if let Some(n) = at_node {
            if !new_cuts.iter().any(|(_, r)| *r == NodeRef::Old(n)) {
                new_cuts.push((c.s_new, NodeRef::Old(n)));
            }
            continue;
        }
        let h = profile.height_at(c.s_new);
        let r = new_node(&mut plan, Vec3::new(c.at.x, h, c.at.y));
        new_cuts.push((c.s_new, r));
        splits
            .entry(near[c.other])
            .or_default()
            .push((c.s_other, r, h));
    }
    new_cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
    // New road pieces.
    let pieces = split_curve(
        &cmd.curve,
        &new_cuts.iter().map(|c| c.0).collect::<Vec<_>>(),
    );
    let mut bounds: Vec<(f32, NodeRef)> = vec![(0.0, start_ref)];
    bounds.extend(new_cuts.iter().copied());
    bounds.push((len, end_ref));
    for (k, piece) in pieces.into_iter().enumerate() {
        let (s0, a) = bounds[k];
        let (s1, b) = bounds[k + 1];
        plan.add.push(PlannedSeg {
            kind: cmd.kind,
            variant: Variant::default(),
            curve: piece,
            profile: profile_piece(&profile, s0, s1),
            start: a,
            end: b,
            built_tick: tick,
        });
    }
    // Split existing segments (profiles re-solved to the new node heights).
    for (k, mut pts) in splits {
        let s = &snaps[k];
        let slen = s.geom.length();
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        if pts.iter().any(|p| p.0 < 1.0 || p.0 > slen - 1.0) {
            return Err(ValidationError::at(
                "road-invalid-junction-too-close",
                s.geom.point_at(pts[0].0),
            ));
        }
        let max_grade = cat.road(s.kind).max_grade;
        let pieces = split_curve(&s.curve, &pts.iter().map(|p| p.0).collect::<Vec<_>>());
        let start_h = s.profile.height_at(0.0);
        let mut b: Vec<(f32, NodeRef, f32)> = vec![(0.0, NodeRef::Old(s.start), start_h)];
        b.extend(pts.iter().copied());
        b.push((slen, NodeRef::Old(s.end), s.profile.height_at(slen)));
        for (q, piece) in pieces.into_iter().enumerate() {
            let (s0, a, ha) = b[q];
            let (s1, e, he) = b[q + 1];
            plan.add.push(PlannedSeg {
                kind: s.kind,
                variant: s.variant,
                curve: piece,
                profile: resolve_piece(&s.profile, s0, s1, ha, he, max_grade),
                start: a,
                end: e,
                built_tick: s.built_tick,
            });
        }
        plan.remove.push(s.id);
    }

    // Junction validation of every affected node on the planned topology.
    let mut affected: Vec<NodeRef> = plan.add.iter().flat_map(|p| [p.start, p.end]).collect();
    affected.sort();
    affected.dedup();
    for node in affected {
        let mut apps: Vec<(RoadKind, Approach)> = Vec::new();
        if let NodeRef::Old(n) = node
            && let Some((_, incident)) = nodes.get(&n)
        {
            for sid in incident {
                if plan.remove.contains(sid) {
                    continue;
                }
                if let Some(k) = index_of(*sid) {
                    let s = &snaps[k];
                    apps.push((
                        s.kind,
                        approach(cat, (s.kind, s.variant), &s.geom, &s.profile, s.start == n),
                    ));
                }
            }
        }
        for p in plan.add.iter().filter(|p| p.start == node || p.end == node) {
            let g = SegmentGeom::new(p.curve, CrossSection::of(p.kind).half_width_m());
            if p.start == node {
                apps.push((
                    p.kind,
                    approach(cat, (p.kind, p.variant), &g, &p.profile, true),
                ));
            }
            if p.end == node {
                apps.push((
                    p.kind,
                    approach(cat, (p.kind, p.variant), &g, &p.profile, false),
                ));
            }
        }
        if let Some(Err(e)) = node_junction(&apps) {
            let at = match node {
                NodeRef::Old(n) => nodes.get(&n).map(|v| Vec2::new(v.0.x, v.0.z)),
                NodeRef::New(i) => plan.new_nodes.get(i).map(|v| Vec2::new(v.x, v.z)),
            };
            return Err(ValidationError { key: e.key(), at });
        }
    }
    Ok((plan, road_cost(cat, cmd.kind, len), profile))
}

fn corridor_stamp(id: u32, seg: &RoadSegment) -> Stamp {
    let centerline = seg.geom.centerline3(&seg.profile, 2.0);
    Stamp {
        id,
        priority: priority::ROAD,
        shape: StampShape::Corridor {
            centerline,
            core_half_width_m: seg.geom.half_width_m + SHOULDER_M,
            carve_m: CARVE_M,
        },
    }
}

/// Terrain stamp keeping the ground below a junction's surface (sidewalks included): the
/// corridors under it follow their own profiles, which a junction joining several grades does
/// not, so without it terrain pokes through the junction.
fn junction_stamp(id: u32, j: &Junction) -> Option<Stamp> {
    let plan: Vec<Vec2> = j.outer.iter().map(|v| Vec2::new(v.x, v.z)).collect();
    // The carriageway first, with exactly the triangles the junction mesh draws (lookups take
    // the first triangle containing a sample); the outer polygon covers the sidewalk ring.
    let mut triangles = junction::carriageway_triangles(&j.inner);
    triangles.extend(
        cb_geometry::triangulate::triangulate(&plan)
            .into_iter()
            .map(|t| t.map(|i| j.outer[i as usize])),
    );
    (!triangles.is_empty()).then_some(Stamp {
        id,
        priority: priority::JUNCTION,
        shape: StampShape::Surface {
            triangles,
            carve_m: CARVE_M,
        },
    })
}

fn spawn_node(world: &mut World, pos: Vec3) -> Result<NodeId, ValidationError> {
    let id = world
        .resource_mut::<Ids>()
        .nodes
        .allocate()
        .map_err(|_| ValidationError::new("internal-id-space"))?;
    let e = world
        .spawn(RoadNode {
            id,
            pos,
            segments: SmallVec::new(),
            junction: None,
            approaches: SmallVec::new(),
            junction_stamp: None,
            control: ControlMode::Auto,
            custom: SmallVec::new(),
            controls: SmallVec::new(),
            phases: Vec::new(),
            markings: Markings::default(),
            ring: false,
        })
        .id();
    world.resource_mut::<Ids>().nodes.insert(id, e);
    world.write_message(NetworkChanged::Node(id));
    Ok(id)
}

fn node_entity(world: &World, id: NodeId) -> Option<Entity> {
    world.get_resource::<Ids>().and_then(|i| i.nodes.get(id))
}

fn segment_entity(world: &World, id: SegmentId) -> Option<Entity> {
    world.get_resource::<Ids>().and_then(|i| i.segments.get(id))
}

/// Read access to a segment by id.
#[must_use]
pub fn segment(world: &World, id: SegmentId) -> Option<&RoadSegment> {
    segment_entity(world, id).and_then(|e| world.get::<RoadSegment>(e))
}

/// Read access to a node by id.
#[must_use]
pub fn node(world: &World, id: NodeId) -> Option<&RoadNode> {
    node_entity(world, id).and_then(|e| world.get::<RoadNode>(e))
}

fn spawn_segment(
    world: &mut World,
    p: &PlannedSeg,
    start: NodeId,
    end: NodeId,
) -> Result<SegmentId, ValidationError> {
    let id = world
        .resource_mut::<Ids>()
        .segments
        .allocate()
        .map_err(|_| ValidationError::new("internal-id-space"))?;
    let stamp = world.resource_mut::<Ids>().stamp();
    let geom = SegmentGeom::new(p.curve, CrossSection::of(p.kind).half_width_m());
    let seg = RoadSegment {
        id,
        start,
        end,
        kind: p.kind,
        variant: p.variant,
        curve: p.curve,
        profile: p.profile.clone(),
        built_tick: p.built_tick,
        geom,
        cut: (0.0, 0.0),
        stamp,
    };
    terrain::add_stamp(world, corridor_stamp(stamp, &seg));
    let (lo, hi) = cb_geometry::polygon::bounds(&seg.geom.corridor);
    crate::zoning::mark_near(world, lo, hi);
    let e = world.spawn(seg).id();
    world.resource_mut::<Ids>().segments.insert(id, e);
    for n in [start, end] {
        if let Some(ne) = node_entity(world, n)
            && let Some(mut node) = world.get_mut::<RoadNode>(ne)
            && !node.segments.contains(&id)
        {
            node.segments.push(id);
        }
    }
    world.write_message(NetworkChanged::Segment(id));
    Ok(id)
}

fn despawn_segment(world: &mut World, id: SegmentId) -> Option<RoadSegment> {
    let e = world.resource_mut::<Ids>().segments.remove(id)?;
    let seg = world.get::<RoadSegment>(e).cloned()?;
    world.despawn(e);
    terrain::remove_stamp(world, seg.stamp);
    for n in [seg.start, seg.end] {
        if let Some(ne) = node_entity(world, n)
            && let Some(mut node) = world.get_mut::<RoadNode>(ne)
        {
            node.segments.retain(|s| *s != id);
        }
    }
    world.write_message(NetworkChanged::SegmentRemoved(id));
    Some(seg)
}

/// Recomputes a node's junction and its segments' cut-backs; removes isolated nodes.
fn refresh_node(world: &mut World, id: NodeId) {
    let Some(ne) = node_entity(world, id) else {
        return;
    };
    let Some(node) = world.get::<RoadNode>(ne).cloned() else {
        return;
    };
    if node.segments.is_empty() {
        if let Some(st) = node.junction_stamp {
            terrain::remove_stamp(world, st);
        }
        world.resource_mut::<Ids>().nodes.remove(id);
        world.despawn(ne);
        world.write_message(NetworkChanged::NodeRemoved(id));
        return;
    }
    let Some(cat) = world.get_resource::<Catalog>().cloned() else {
        return;
    };
    let mut apps: Vec<(RoadKind, Approach)> = Vec::new();
    let mut order: Vec<(SegmentId, bool)> = Vec::new();
    let mut sorted = node.segments.to_vec();
    sorted.sort();
    for sid in &sorted {
        if let Some(s) = segment(world, *sid) {
            // A loop segment (both ends here) contributes two approaches.
            if s.start == id {
                apps.push((
                    s.kind,
                    approach(&cat, (s.kind, s.variant), &s.geom, &s.profile, true),
                ));
                order.push((*sid, true));
            }
            if s.end == id {
                apps.push((
                    s.kind,
                    approach(&cat, (s.kind, s.variant), &s.geom, &s.profile, false),
                ));
                order.push((*sid, false));
            }
        }
    }
    let j = match node_junction(&apps) {
        Some(Ok(j)) => Some(j),
        Some(Err(e)) => {
            tracing::warn!(?id, ?e, "junction rebuild failed after a validated edit");
            None
        }
        None => None,
    };
    for (k, (sid, at_start)) in order.iter().enumerate() {
        let cut = j
            .as_ref()
            .and_then(|j| j.cuts.get(k).copied())
            .unwrap_or(0.0);
        if let Some(se) = segment_entity(world, *sid)
            && let Some(mut s) = world.get_mut::<RoadSegment>(se)
        {
            if *at_start {
                s.cut.0 = cut;
            } else {
                s.cut.1 = cut;
            }
        }
        world.write_message(NetworkChanged::Segment(*sid));
    }
    if let Some(jj) = &j {
        let outline: Vec<Vec2> = jj.outer.iter().map(|v| Vec2::new(v.x, v.z)).collect();
        let (lo, hi) = cb_geometry::polygon::bounds(&outline);
        crate::zoning::mark_near(world, lo, hi);
    }
    if let Some(st) = node.junction_stamp {
        terrain::remove_stamp(world, st);
    }
    let stamp = j.as_ref().and_then(|jj| {
        let sid = world.resource_mut::<Ids>().stamp();
        let st = junction_stamp(sid, jj)?;
        terrain::add_stamp(world, st);
        Some(sid)
    });
    let derived = j
        .as_ref()
        .map(|jj| derive_control(&node, &apps, &order, jj));
    if let Some(mut n) = world.get_mut::<RoadNode>(ne) {
        n.junction = j;
        n.approaches = order.into_iter().collect();
        n.junction_stamp = stamp;
        n.ring = false;
        let (controls, phases, markings) = derived.unwrap_or_default();
        n.controls = controls;
        n.phases = phases;
        n.markings = markings;
    }
    world.write_message(NetworkChanged::Node(id));
}

/// Effective approach classes, signal phases and markings of a junction (ALGORITHMS §5–§6).
fn derive_control(
    node: &RoadNode,
    apps: &[(RoadKind, Approach)],
    order: &[(SegmentId, bool)],
    j: &Junction,
) -> (SmallVec<[ApproachControl; 4]>, Vec<SignalPhase>, Markings) {
    use cb_network::control;
    let dirs: Vec<Vec2> = apps
        .iter()
        .map(|a| a.1.centerline.tangent_at(0.0))
        .collect();
    let ranks: Vec<u8> = apps.iter().map(|a| control::rank(a.0)).collect();
    let auto = control::auto_controls(&ranks, &dirs);
    let controls: SmallVec<[ApproachControl; 4]> = match node.control {
        ControlMode::Auto | ControlMode::Lights => auto.into_iter().collect(),
        ControlMode::AllWayStop => order.iter().map(|_| ApproachControl::Stop).collect(),
        ControlMode::Custom => order
            .iter()
            .zip(auto)
            .map(|(a, au)| {
                node.custom
                    .iter()
                    .find(|c| (c.0, c.1) == *a)
                    .map_or(au, |c| c.2)
            })
            .collect(),
    };
    let phases = if node.control == ControlMode::Lights {
        control::phase_plan(&j.connectors, &j.zones, &dirs)
    } else {
        Vec::new()
    };
    let raw: Vec<Approach> = apps.iter().map(|a| a.1.clone()).collect();
    (controls, phases, cb_network::markings::markings(&raw, j))
}

/// Detects roundabout rings among the one-way segments near `around` (ADR-0063) and gives
/// their nodes ring control: ring approaches Priority, entries Yield.
pub(crate) fn update_rings(world: &mut World, around: &[NodeId]) {
    let Some(ids) = world.get_resource::<Ids>() else {
        return;
    };
    let pts: Vec<Vec2> = around
        .iter()
        .filter_map(|n| node(world, *n).map(|n| Vec2::new(n.pos.x, n.pos.z)))
        .collect();
    if pts.is_empty() {
        return;
    }
    let reach = 2.0 * cb_network::rings::RING_MAX_RADIUS_M + 10.0;
    let mut one_ways = Vec::new();
    for (_, e) in ids.segments.iter() {
        let Some(sg) = world.get::<RoadSegment>(e) else {
            continue;
        };
        if !matches!(sg.kind, RoadKind::OneWayStreet | RoadKind::OneWayAvenue) {
            continue;
        }
        let a = sg.curve.start();
        if pts.iter().all(|p| p.distance(a) > reach) {
            continue;
        }
        one_ways.push(cb_network::rings::OneWay {
            id: sg.id,
            from: sg.start,
            to: sg.end,
            from_pos: a,
            mid: sg.geom.point_at(0.5 * sg.geom.length()),
        });
    }
    let was_ring: Vec<NodeId> = ids
        .nodes
        .iter()
        .filter_map(|(id, e)| {
            let n = world.get::<RoadNode>(e)?;
            let p = Vec2::new(n.pos.x, n.pos.z);
            (n.ring && pts.iter().any(|q| q.distance(p) <= reach)).then_some(id)
        })
        .collect();
    let rings = cb_network::rings::detect_rings(&one_ways);
    // Nodes of rings that no longer exist go back to their own control.
    for n in was_ring {
        if !rings.iter().any(|r| r.nodes.contains(&n)) {
            refresh_node(world, n);
        }
    }
    for ring in rings {
        for n in &ring.nodes {
            let Some(ne) = node_entity(world, *n) else {
                continue;
            };
            if let Some(mut nd) = world.get_mut::<RoadNode>(ne) {
                if nd.ring {
                    continue;
                }
                nd.ring = true;
                if nd.junction.is_some() && nd.control == ControlMode::Auto {
                    nd.controls = nd
                        .approaches
                        .iter()
                        .map(|(sid, _)| {
                            if ring.segments.contains(sid) {
                                ApproachControl::Priority
                            } else {
                                ApproachControl::Yield
                            }
                        })
                        .collect();
                }
            }
            world.write_message(NetworkChanged::Node(*n));
        }
    }
}

/// `SetApproachControl`: control mode of a junction, with per-approach classes for
/// `ControlMode::Custom` (approaches not listed keep their automatic class).
///
/// # Errors
/// `road-invalid-target-missing` if the node does not exist or is not a junction.
pub(crate) fn set_node_control(
    world: &mut World,
    id: NodeId,
    mode: ControlMode,
    custom: &[(SegmentId, bool, ApproachControl)],
) -> Result<(), ValidationError> {
    let ne = node_entity(world, id).ok_or(ValidationError::new("road-invalid-target-missing"))?;
    let has_junction = world
        .get::<RoadNode>(ne)
        .is_some_and(|n| n.junction.is_some());
    if !has_junction {
        return Err(ValidationError::new("road-invalid-target-missing"));
    }
    if let Some(mut n) = world.get_mut::<RoadNode>(ne) {
        n.control = mode;
        n.custom = custom.iter().copied().collect();
    }
    refresh_node(world, id);
    update_rings(world, &[id]);
    Ok(())
}

/// What building a road would do (the tool preview): cost, geometry and profile.
#[derive(Clone, Debug)]
pub struct RoadPreview {
    /// Construction cost.
    pub cost: Money,
    /// Geometry of the new road.
    pub geom: SegmentGeom,
    /// Solved surface profile.
    pub profile: Profile,
    /// Endpoints after snapping.
    pub start: Endpoint,
    /// End after snapping.
    pub end: Endpoint,
}

/// Read-only preview of `BuildRoad` with the same planning as the command (validation reasons,
/// cost, profile). Affordability is reported separately by the tool.
///
/// # Errors
/// The first validation failure, with its location.
pub fn preview_road(world: &World, cmd: &BuildRoad) -> Result<RoadPreview, ValidationError> {
    let start = snap_endpoint(world, cmd.start, cmd.curve.start());
    let end = snap_endpoint(world, cmd.end, cmd.curve.end());
    let snapped = BuildRoad {
        start,
        end,
        ..cmd.clone()
    };
    let (_, cost, profile) = plan_build(world, &snapped, 0)?;
    let geom = SegmentGeom::new(cmd.curve, CrossSection::of(cmd.kind).half_width_m());
    Ok(RoadPreview {
        cost,
        geom,
        profile,
        start,
        end,
    })
}

/// Free endpoints lying on an existing node or segment centreline connect to it.
fn snap_endpoint(world: &World, ep: Endpoint, p: Vec2) -> Endpoint {
    if ep != Endpoint::Free {
        return ep;
    }
    let Some(ids) = world.get_resource::<Ids>() else {
        return ep;
    };
    for (id, e) in ids.nodes.iter() {
        if let Some(n) = world.get::<RoadNode>(e)
            && Vec2::new(n.pos.x, n.pos.z).distance(p) < SNAP_TOL_M
        {
            return Endpoint::Node(id);
        }
    }
    for (id, e) in ids.segments.iter() {
        if let Some(s) = world.get::<RoadSegment>(e)
            && s.geom.project(p).1.abs() < SNAP_TOL_M
        {
            let (sp, _) = s.geom.project(p);
            if sp > 0.0 && sp < s.geom.length() {
                return Endpoint::Segment(id);
            }
        }
    }
    ep
}

/// `BuildRoad`: plan, charge, apply. Returns the cost.
///
/// # Errors
/// The first validation failure.
pub(crate) fn build_road(
    world: &mut World,
    cmd: &BuildRoad,
    tick: u64,
) -> Result<Money, ValidationError> {
    let cmd = &BuildRoad {
        start: snap_endpoint(world, cmd.start, cmd.curve.start()),
        end: snap_endpoint(world, cmd.end, cmd.curve.end()),
        ..cmd.clone()
    };
    let (plan, cost, _) = plan_build(world, cmd, tick)?;
    crate::economy::charge(world, cost)?;
    let mut new_ids: Vec<NodeId> = Vec::with_capacity(plan.new_nodes.len());
    for p in &plan.new_nodes {
        new_ids.push(spawn_node(world, *p)?);
    }
    let resolve = |r: NodeRef| match r {
        NodeRef::Old(n) => n,
        NodeRef::New(i) => new_ids[i],
    };
    let mut touched: Vec<NodeId> = new_ids.clone();
    for sid in &plan.remove {
        if let Some(seg) = despawn_segment(world, *sid) {
            touched.extend([seg.start, seg.end]);
        }
    }
    let mut added = Vec::with_capacity(plan.add.len());
    for p in &plan.add {
        let (a, b) = (resolve(p.start), resolve(p.end));
        added.push(spawn_segment(world, p, a, b)?);
        touched.extend([a, b]);
    }
    touched.sort();
    touched.dedup();
    for n in &touched {
        refresh_node(world, *n);
    }
    update_rings(world, &touched);
    let _ = added;
    Ok(cost)
}

/// `RemoveSegment` (bulldoze): removes the segment, its stamp and orphaned nodes.
///
/// # Errors
/// `road-invalid-target-missing` if the segment does not exist.
pub(crate) fn remove_segment(world: &mut World, id: SegmentId) -> Result<(), ValidationError> {
    let seg =
        despawn_segment(world, id).ok_or(ValidationError::new("road-invalid-target-missing"))?;
    let mut nodes = vec![seg.start, seg.end];
    nodes.dedup();
    for n in &nodes {
        refresh_node(world, *n);
    }
    update_rings(world, &nodes);
    Ok(())
}

/// What changing a segment's type would do (the upgrade tool preview).
#[derive(Clone, Debug, PartialEq)]
pub struct RoadTypePreview {
    /// Charge: new − 50 % of old for upgrades, nothing for downgrades (no refund).
    pub cost: Money,
    /// Buildings standing in the new corridor; they are removed on apply.
    pub demolished: Vec<BuildingId>,
}

/// Plans `SetRoadType` without touching the world.
fn plan_set_type(
    world: &World,
    id: SegmentId,
    kind: RoadKind,
    variant: Variant,
) -> Result<(RoadTypePreview, SegmentGeom), ValidationError> {
    let Some(cat) = world.get_resource::<Catalog>() else {
        return Err(ValidationError::new("internal-no-catalog"));
    };
    let seg = segment(world, id).ok_or(ValidationError::new("road-invalid-target-missing"))?;
    if !kind.allows(variant) {
        return Err(ValidationError::new("road-invalid-variant"));
    }
    let lim = limits(cat, kind);
    let geom = SegmentGeom::new(seg.curve, lim.half_width_m);
    let len = geom.length();
    let mid = geom.point_at(0.5 * len);
    if seg.profile.max_step_grade() > lim.max_grade + 1e-3 {
        return Err(ValidationError::at("road-invalid-too-steep", mid));
    }
    let ground = terrain::ground(world);
    let (snaps, nodes) = snapshot(world);
    let node_pos = |n: NodeId| nodes.get(&n).map(|(p, _)| Vec2::new(p.x, p.z));
    let (lo, hi) = seg.curve.bounds();
    let pad = lim.half_width_m + 30.0;
    let existing: Vec<Existing<'_>> = snaps
        .iter()
        .filter(|o| o.id != id)
        .filter(|o| {
            let (a, b) = o.curve.bounds();
            let r = o.geom.half_width_m + pad;
            a.x - r <= hi.x && lo.x <= b.x + r && a.y - r <= hi.y && lo.y <= b.y + r
        })
        .map(|o| {
            let shared = [seg.start, seg.end]
                .into_iter()
                .find(|n| o.start == *n || o.end == *n);
            Existing {
                geom: &o.geom,
                profile: &o.profile,
                shares_node_at: shared.and_then(node_pos),
            }
        })
        .collect();
    if let Some(e) = validation::validate_shape(&geom, &lim, &ground, &existing, &[])
        .into_iter()
        .find(|e| !matches!(e, cb_network::Invalid::TooShort))
    {
        return Err(e.into());
    }
    // Both junctions with the new cross-section.
    let mut ends = vec![seg.start, seg.end];
    ends.dedup();
    for n in ends {
        let Some((pos, incident)) = nodes.get(&n) else {
            continue;
        };
        let mut sorted = incident.clone();
        sorted.sort();
        let mut apps: Vec<(RoadKind, Approach)> = Vec::new();
        for sid in sorted {
            let Some(o) = snaps.iter().find(|o| o.id == sid) else {
                continue;
            };
            let (k, v, g) = if sid == id {
                (kind, variant, &geom)
            } else {
                (o.kind, o.variant, &o.geom)
            };
            if o.start == n {
                apps.push((k, approach(cat, (k, v), g, &o.profile, true)));
            }
            if o.end == n {
                apps.push((k, approach(cat, (k, v), g, &o.profile, false)));
            }
        }
        if let Some(Err(e)) = node_junction(&apps) {
            return Err(ValidationError::at(e.key(), Vec2::new(pos.x, pos.z)));
        }
    }
    let new_cost = road_cost_with(cat, kind, variant, len);
    let old_cost = road_cost_with(cat, seg.kind, seg.variant, len);
    let cost = if new_cost > old_cost {
        Money(new_cost.0 - old_cost.0 / 2)
    } else {
        Money::ZERO
    };
    // Buildings in the new corridor (only a wider road can reach them).
    let mut demolished = Vec::new();
    if lim.half_width_m > seg.geom.half_width_m + 1e-3
        && let Some(ids) = world.get_resource::<Ids>()
    {
        for (bid, e) in ids.buildings.iter() {
            let Some(b) = world.get::<crate::buildings::Building>(e) else {
                continue;
            };
            let f = b.footprint();
            let c = (f[0] + f[1] + f[2] + f[3]) * 0.25;
            let probes = [
                f[0],
                f[1],
                f[2],
                f[3],
                (f[0] + f[1]) * 0.5,
                (f[1] + f[2]) * 0.5,
                (f[2] + f[3]) * 0.5,
                (f[3] + f[0]) * 0.5,
                c,
            ];
            if probes
                .iter()
                .any(|q| cb_geometry::polygon::contains(&geom.corridor, *q))
            {
                demolished.push(bid);
            }
        }
    }
    Ok((RoadTypePreview { cost, demolished }, geom))
}

/// Read-only preview of `SetRoadType`.
///
/// # Errors
/// The first validation failure.
pub fn preview_set_road_type(
    world: &World,
    id: SegmentId,
    kind: RoadKind,
    variant: Variant,
) -> Result<RoadTypePreview, ValidationError> {
    plan_set_type(world, id, kind, variant).map(|(p, _)| p)
}

/// `SetRoadType` (upgrade/downgrade, ALGORITHMS §1): replaces the cross-section, re-stamps the
/// corridor, rebuilds both junctions and lanes; plots re-clip against the new corridor and
/// buildings in it are removed. Returns the charge.
///
/// # Errors
/// The first validation failure or `money-insufficient`.
pub(crate) fn set_road_type(
    world: &mut World,
    id: SegmentId,
    kind: RoadKind,
    variant: Variant,
) -> Result<Money, ValidationError> {
    let (preview, geom) = plan_set_type(world, id, kind, variant)?;
    crate::economy::charge(world, preview.cost)?;
    for b in &preview.demolished {
        crate::buildings::remove_building(world, *b);
    }
    let Some(e) = segment_entity(world, id) else {
        return Err(ValidationError::new("road-invalid-target-missing"));
    };
    let Some(old) = world.get::<RoadSegment>(e).cloned() else {
        return Err(ValidationError::new("road-invalid-target-missing"));
    };
    let (lo0, hi0) = cb_geometry::polygon::bounds(&old.geom.corridor);
    terrain::remove_stamp(world, old.stamp);
    let stamp = world.resource_mut::<Ids>().stamp();
    let mut seg = old.clone();
    seg.kind = kind;
    seg.variant = variant;
    seg.geom = geom;
    seg.stamp = stamp;
    terrain::add_stamp(world, corridor_stamp(stamp, &seg));
    let (lo1, hi1) = cb_geometry::polygon::bounds(&seg.geom.corridor);
    crate::zoning::mark_near(world, lo0.min(lo1), hi0.max(hi1));
    let (a, b) = (seg.start, seg.end);
    if let Some(mut s) = world.get_mut::<RoadSegment>(e) {
        *s = seg;
    }
    world.write_message(NetworkChanged::Segment(id));
    refresh_node(world, a);
    if b != a {
        refresh_node(world, b);
    }
    update_rings(world, &[a, b]);
    Ok(preview.cost)
}

/// Outer road edge of a segment side between arc lengths `s0` and `s1` (either order), walked
/// from `s0` to `s1`. Uses the same offset polyline as the corridor polygon, so plots clipped by
/// the corridor keep exact contact with their frontage.
#[must_use]
pub fn edge_polyline(seg: &RoadSegment, left: bool, s0: f32, s1: f32) -> Vec<Vec2> {
    let edge = if left {
        &seg.geom.left
    } else {
        &seg.geom.right
    };
    let pts = edge.points();
    let cum = seg.geom.centerline.cumulative();
    let len = seg.geom.length();
    let (a, b) = (s0.min(s1).clamp(0.0, len), s0.max(s1).clamp(0.0, len));
    // Edge point at centreline arc length `s` (vertex correspondence of the offset polyline).
    let at = |s: f32| -> Vec2 {
        let k = cum
            .partition_point(|v| *v <= s)
            .clamp(1, cum.len().max(2) - 1)
            - 1;
        let seg_len = (cum[k + 1] - cum[k]).max(1e-9);
        pts[k].lerp(pts[k + 1], ((s - cum[k]) / seg_len).clamp(0.0, 1.0))
    };
    let mut out = vec![at(a)];
    for (k, c) in cum.iter().enumerate() {
        if *c > a + 1e-3 && *c < b - 1e-3 {
            out.push(pts[k]);
        }
    }
    out.push(at(b));
    if s0 > s1 {
        out.reverse();
    }
    out
}
