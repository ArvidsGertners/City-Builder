//! The lane graph (ADR-0008, DATA_MODEL §4.2): lanes per segment, connectors through nodes and
//! conflict zones, with generational ids, patched per dirty segment and node.
//!
//! Patching protocol: [`LaneGraph::set_segment`] replaces a segment's lanes and clears the
//! connectors at both its nodes (they referenced the old lanes); the caller then calls
//! [`LaneGraph::set_node`] for every dirty node — including both ends of every patched segment —
//! in ascending node order. Ids are only stable until the next patch of their segment or node;
//! vehicles re-project by (segment, direction, index, s/length).

use std::collections::BTreeMap;

use cb_core::collections::{SlotKey, Slots};
use cb_core::ids::{NodeId, SegmentId};
use glam::{Vec2, Vec3};
use smallvec::SmallVec;

use crate::connectors::{CONFLICT_WIDTH_M, ConnectorKind, Turn};
use crate::cross_section::{Dir, LaneKind, LaneSpec};
use crate::junction::Junction;
use crate::lanes::LanePath;

/// Samples of a dead-end turnaround.
const UTURN_SAMPLES: usize = 12;

macro_rules! slot_id {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub SlotKey);
    };
}

slot_id!(
    /// A lane.
    LaneId
);
slot_id!(
    /// A connector through a node.
    ConnectorId
);
slot_id!(
    /// A conflict zone.
    ZoneId
);

/// One lane of a segment.
#[derive(Clone, Debug, PartialEq)]
pub struct Lane {
    /// Owning segment.
    pub segment: SegmentId,
    /// Cross-section lane (direction, index from the right, kind, offset, width).
    pub spec: LaneSpec,
    /// Centreline in travel direction.
    pub path: LanePath,
    /// Road speed capped by the curve speed of the tightest radius (m/s).
    pub speed_limit_mps: f32,
    /// Connectors leaving the lane's end.
    pub outgoing: SmallVec<[ConnectorId; 4]>,
    /// Connectors arriving at the lane's start.
    pub incoming: SmallVec<[ConnectorId; 4]>,
    /// Same-direction neighbour to the left (index + 1).
    pub left: Option<LaneId>,
    /// Same-direction neighbour to the right (index − 1).
    pub right: Option<LaneId>,
}

/// A lane-to-lane path through a node.
#[derive(Clone, Debug, PartialEq)]
pub struct LaneConnector {
    /// Node it crosses.
    pub node: NodeId,
    /// Incoming lane.
    pub from: LaneId,
    /// Outgoing lane.
    pub to: LaneId,
    /// Users.
    pub kind: ConnectorKind,
    /// Movement class.
    pub turn: LaneTurn,
    /// Path (x, height, z).
    pub points: Vec<Vec3>,
    /// Plan length (m).
    pub length_m: f32,
    /// Conflict zones on the connector with the arc length where each lies.
    pub conflicts: SmallVec<[(ZoneId, f32); 6]>,
}

/// Movement class of a lane connector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LaneTurn {
    /// Through or continuation.
    Straight,
    /// Left turn.
    Left,
    /// Right turn.
    Right,
    /// Dead-end turnaround.
    UTurn,
}

impl From<Turn> for LaneTurn {
    fn from(t: Turn) -> Self {
        match t {
            Turn::Straight => Self::Straight,
            Turn::Left => Self::Left,
            Turn::Right => Self::Right,
        }
    }
}

/// Where connectors of one node conflict.
#[derive(Clone, Debug, PartialEq)]
pub struct LaneZone {
    /// Node.
    pub node: NodeId,
    /// Member connectors.
    pub connectors: SmallVec<[ConnectorId; 4]>,
    /// Centre.
    pub center: Vec3,
    /// Radius (m).
    pub radius_m: f32,
}

/// Per-node bookkeeping.
#[derive(Clone, Debug, Default, PartialEq)]
struct NodeEntry {
    connectors: Vec<ConnectorId>,
    zones: Vec<ZoneId>,
}

/// Lanes of one segment for [`LaneGraph::set_segment`].
#[derive(Clone, Debug)]
pub struct SegmentLanes {
    /// Segment.
    pub id: SegmentId,
    /// Start node.
    pub start: NodeId,
    /// End node.
    pub end: NodeId,
    /// Lanes from [`crate::lanes::segment_lanes`].
    pub lanes: Vec<LanePath>,
    /// Road speed (m/s).
    pub speed_mps: f32,
}

/// The lane graph.
#[derive(Clone, Debug, Default)]
pub struct LaneGraph {
    lanes: Slots<Lane>,
    connectors: Slots<LaneConnector>,
    zones: Slots<LaneZone>,
    by_segment: BTreeMap<SegmentId, (NodeId, NodeId, SmallVec<[LaneId; 8]>)>,
    by_node: BTreeMap<NodeId, NodeEntry>,
    version: u64,
}

impl LaneGraph {
    /// Bumped on every patch.
    #[must_use]
    pub fn version(&self) -> u64 {
        self.version
    }

    /// A lane.
    #[must_use]
    pub fn lane(&self, id: LaneId) -> Option<&Lane> {
        self.lanes.get(id.0)
    }

    /// A connector.
    #[must_use]
    pub fn connector(&self, id: ConnectorId) -> Option<&LaneConnector> {
        self.connectors.get(id.0)
    }

    /// A conflict zone.
    #[must_use]
    pub fn zone(&self, id: ZoneId) -> Option<&LaneZone> {
        self.zones.get(id.0)
    }

    /// All lanes in slot order.
    pub fn lanes(&self) -> impl Iterator<Item = (LaneId, &Lane)> + '_ {
        self.lanes.iter().map(|(k, l)| (LaneId(k), l))
    }

    /// All connectors in slot order.
    pub fn connectors(&self) -> impl Iterator<Item = (ConnectorId, &LaneConnector)> + '_ {
        self.connectors.iter().map(|(k, c)| (ConnectorId(k), c))
    }

    /// All zones in slot order.
    pub fn zones(&self) -> impl Iterator<Item = (ZoneId, &LaneZone)> + '_ {
        self.zones.iter().map(|(k, z)| (ZoneId(k), z))
    }

    /// Counts (lanes, connectors, zones).
    #[must_use]
    pub fn counts(&self) -> (usize, usize, usize) {
        (self.lanes.len(), self.connectors.len(), self.zones.len())
    }

    /// Lanes of a segment (forward then backward, each from the right).
    #[must_use]
    pub fn segment_lanes(&self, id: SegmentId) -> &[LaneId] {
        self.by_segment.get(&id).map_or(&[], |e| e.2.as_slice())
    }

    /// Connectors of a node.
    #[must_use]
    pub fn node_connectors(&self, id: NodeId) -> &[ConnectorId] {
        self.by_node
            .get(&id)
            .map_or(&[], |e| e.connectors.as_slice())
    }

    /// Conflict zones of a node.
    #[must_use]
    pub fn node_zones(&self, id: NodeId) -> &[ZoneId] {
        self.by_node.get(&id).map_or(&[], |e| e.zones.as_slice())
    }

    /// The lane of `segment` in direction `dir` with index `index`.
    #[must_use]
    pub fn find_lane(&self, segment: SegmentId, dir: Dir, index: u8) -> Option<LaneId> {
        self.segment_lanes(segment).iter().copied().find(|l| {
            self.lane(*l)
                .is_some_and(|l| l.spec.dir == dir && l.spec.index == index)
        })
    }

    /// Replaces the lanes of a segment and clears the connectors of both its nodes (call
    /// [`Self::set_node`] for them afterwards).
    pub fn set_segment(&mut self, input: SegmentLanes) {
        let _ = self.remove_segment(input.id);
        let mut ids: SmallVec<[LaneId; 8]> = SmallVec::new();
        for path in input.lanes {
            let speed_limit_mps = input.speed_mps.min(path.curve_speed_cap_mps());
            let id = LaneId(self.lanes.insert(Lane {
                segment: input.id,
                spec: path.spec,
                path,
                speed_limit_mps,
                outgoing: SmallVec::new(),
                incoming: SmallVec::new(),
                left: None,
                right: None,
            }));
            ids.push(id);
        }
        // Same-direction neighbours.
        for &a in &ids {
            let Some(la) = self.lanes.get(a.0).map(|l| l.spec) else {
                continue;
            };
            let find = |idx: Option<u8>| {
                idx.and_then(|i| {
                    ids.iter().copied().find(|b| {
                        self.lanes
                            .get(b.0)
                            .is_some_and(|l| l.spec.dir == la.dir && l.spec.index == i)
                    })
                })
            };
            let left = find(la.index.checked_add(1));
            let right = find(la.index.checked_sub(1));
            if let Some(l) = self.lanes.get_mut(a.0) {
                l.left = left;
                l.right = right;
            }
        }
        for n in [input.start, input.end] {
            self.clear_node(n);
        }
        self.by_segment
            .insert(input.id, (input.start, input.end, ids));
        self.version += 1;
    }

    /// Removes a segment's lanes and the connectors of both its nodes; returns those nodes
    /// (call [`Self::set_node`] or [`Self::remove_node`] for them afterwards).
    pub fn remove_segment(&mut self, id: SegmentId) -> Option<(NodeId, NodeId)> {
        let (start, end, ids) = self.by_segment.remove(&id)?;
        for n in [start, end] {
            self.clear_node(n);
        }
        for l in ids {
            self.lanes.remove(l.0);
        }
        self.version += 1;
        Some((start, end))
    }

    /// Removes a node's connectors and zones.
    pub fn remove_node(&mut self, id: NodeId) {
        self.clear_node(id);
        self.by_node.remove(&id);
        self.version += 1;
    }

    fn clear_node(&mut self, id: NodeId) {
        let Some(entry) = self.by_node.get_mut(&id) else {
            return;
        };
        let entry = std::mem::take(entry);
        for z in entry.zones {
            self.zones.remove(z.0);
        }
        for c in entry.connectors {
            if let Some(conn) = self.connectors.remove(c.0) {
                if let Some(l) = self.lanes.get_mut(conn.from.0) {
                    l.outgoing.retain(|x| *x != c);
                }
                if let Some(l) = self.lanes.get_mut(conn.to.0) {
                    l.incoming.retain(|x| *x != c);
                }
            }
        }
    }

    /// Rebuilds the connectors and zones of a node. `approaches` are the node's approaches in
    /// junction input order (`at_start`: the node is the segment's curve start). Without a
    /// junction, two approaches continue lane by lane and a single approach gets a turnaround.
    pub fn set_node(
        &mut self,
        id: NodeId,
        approaches: &[(SegmentId, bool)],
        junction: Option<&Junction>,
    ) {
        self.clear_node(id);
        let mut entry = NodeEntry::default();
        // Incoming lanes arrive at the node, outgoing ones leave it.
        let lane_at = |g: &Self, k: usize, incoming: bool, index: u8| -> Option<LaneId> {
            let (seg, at_start) = *approaches.get(k)?;
            let dir = if at_start == incoming {
                Dir::Backward
            } else {
                Dir::Forward
            };
            g.find_lane(seg, dir, index)
        };
        if let Some(j) = junction {
            let mut map: Vec<Option<ConnectorId>> = Vec::with_capacity(j.connectors.len());
            for c in &j.connectors {
                let (Some(from), Some(to)) = (
                    lane_at(self, c.from, true, c.from_lane),
                    lane_at(self, c.to, false, c.to_lane),
                ) else {
                    map.push(None);
                    continue;
                };
                let cid = self.add_connector(LaneConnector {
                    node: id,
                    from,
                    to,
                    kind: c.kind,
                    turn: c.turn.into(),
                    points: c.points.clone(),
                    length_m: c.length_m,
                    conflicts: SmallVec::new(),
                });
                entry.connectors.push(cid);
                map.push(Some(cid));
            }
            for z in &j.zones {
                let members: SmallVec<[(ConnectorId, f32); 4]> = z
                    .members
                    .iter()
                    .filter_map(|(ci, s)| map.get(*ci).copied().flatten().map(|c| (c, *s)))
                    .collect();
                if members.len() < 2 {
                    continue;
                }
                let zid = ZoneId(self.zones.insert(LaneZone {
                    node: id,
                    connectors: members.iter().map(|m| m.0).collect(),
                    center: z.center,
                    radius_m: CONFLICT_WIDTH_M,
                }));
                for (c, s) in members {
                    if let Some(conn) = self.connectors.get_mut(c.0) {
                        conn.conflicts.push((zid, s));
                    }
                }
                entry.zones.push(zid);
            }
        } else if approaches.len() == 2 {
            for (a, b) in [(0usize, 1usize), (1, 0)] {
                self.continue_lanes(id, approaches, a, b, &mut entry);
            }
        } else if approaches.len() == 1 {
            self.turnaround(id, approaches[0], &mut entry);
        }
        self.by_node.insert(id, entry);
        self.version += 1;
    }

    fn add_connector(&mut self, c: LaneConnector) -> ConnectorId {
        let (from, to) = (c.from, c.to);
        let cid = ConnectorId(self.connectors.insert(c));
        if let Some(l) = self.lanes.get_mut(from.0) {
            l.outgoing.push(cid);
        }
        if let Some(l) = self.lanes.get_mut(to.0) {
            l.incoming.push(cid);
        }
        cid
    }

    /// Lanes of an approach arriving at (`incoming`) or leaving the node, sorted from the right.
    fn approach_lanes(
        &self,
        (seg, at_start): (SegmentId, bool),
        incoming: bool,
        kind: ConnectorKind,
    ) -> Vec<LaneId> {
        let dir = if at_start == incoming {
            Dir::Backward
        } else {
            Dir::Forward
        };
        let mut v: Vec<(u8, LaneId)> = self
            .segment_lanes(seg)
            .iter()
            .filter_map(|l| {
                let lane = self.lane(*l)?;
                (lane.spec.dir == dir && carries(kind, lane.spec.kind))
                    .then_some((lane.spec.index, *l))
            })
            .collect();
        v.sort_unstable();
        v.into_iter().map(|x| x.1).collect()
    }

    fn continue_lanes(
        &mut self,
        id: NodeId,
        approaches: &[(SegmentId, bool)],
        a: usize,
        b: usize,
        entry: &mut NodeEntry,
    ) {
        for kind in [ConnectorKind::Road, ConnectorKind::Tram] {
            let ins = self.approach_lanes(approaches[a], true, kind);
            let outs = self.approach_lanes(approaches[b], false, kind);
            if outs.is_empty() {
                continue;
            }
            for (k, from) in ins.iter().enumerate() {
                let to = outs[k.min(outs.len() - 1)];
                let (Some(p0), Some(p1)) = (
                    self.lane(*from).and_then(|l| l.path.points.last().copied()),
                    self.lane(to).and_then(|l| l.path.points.first().copied()),
                ) else {
                    continue;
                };
                let cid = self.add_connector(LaneConnector {
                    node: id,
                    from: *from,
                    to,
                    kind,
                    turn: LaneTurn::Straight,
                    points: vec![p0, p1],
                    length_m: Vec2::new(p1.x - p0.x, p1.z - p0.z).length(),
                    conflicts: SmallVec::new(),
                });
                entry.connectors.push(cid);
            }
        }
    }

    /// Dead end: the leftmost arriving lane turns into the leftmost leaving lane on a half loop
    /// beyond the road end.
    fn turnaround(&mut self, id: NodeId, app: (SegmentId, bool), entry: &mut NodeEntry) {
        for kind in [ConnectorKind::Road, ConnectorKind::Tram] {
            let (Some(&from), Some(&to)) = (
                self.approach_lanes(app, true, kind).last(),
                self.approach_lanes(app, false, kind).last(),
            ) else {
                continue;
            };
            let (Some(lf), Some(lt)) = (self.lane(from), self.lane(to)) else {
                continue;
            };
            let (Some(p0), Some(p3)) = (
                lf.path.points.last().copied(),
                lt.path.points.first().copied(),
            ) else {
                continue;
            };
            let d = lf.path.end_dir();
            let reach = 0.5 * Vec2::new(p3.x - p0.x, p3.z - p0.z).length() * 4.0 / 3.0;
            let ctrl = [
                Vec2::new(p0.x, p0.z),
                Vec2::new(p0.x, p0.z) + d * reach,
                Vec2::new(p3.x, p3.z) + d * reach,
                Vec2::new(p3.x, p3.z),
            ];
            let points: Vec<Vec3> = (0..=UTURN_SAMPLES)
                .map(|k| {
                    let t = k as f32 / UTURN_SAMPLES as f32;
                    let u = 1.0 - t;
                    let q = ctrl[0] * (u * u * u)
                        + ctrl[1] * (3.0 * u * u * t)
                        + ctrl[2] * (3.0 * u * t * t)
                        + ctrl[3] * (t * t * t);
                    Vec3::new(q.x, p0.y + (p3.y - p0.y) * t, q.y)
                })
                .collect();
            let length_m = points
                .windows(2)
                .map(|w| Vec2::new(w[1].x - w[0].x, w[1].z - w[0].z).length())
                .sum();
            let cid = self.add_connector(LaneConnector {
                node: id,
                from,
                to,
                kind,
                turn: LaneTurn::UTurn,
                points,
                length_m,
                conflicts: SmallVec::new(),
            });
            entry.connectors.push(cid);
        }
    }

    /// Internal consistency (tests, debug builds): every reference is live and mirrored.
    ///
    /// # Errors
    /// A description of the first inconsistency.
    pub fn check(&self) -> Result<(), String> {
        for (cid, c) in self.connectors() {
            let from = self
                .lane(c.from)
                .ok_or(format!("{cid:?}: dead from lane"))?;
            let to = self.lane(c.to).ok_or(format!("{cid:?}: dead to lane"))?;
            if !from.outgoing.contains(&cid) || !to.incoming.contains(&cid) {
                return Err(format!("{cid:?}: not mirrored in its lanes"));
            }
            if !self.node_connectors(c.node).contains(&cid) {
                return Err(format!("{cid:?}: not listed at its node"));
            }
            for (z, _) in &c.conflicts {
                let zone = self.zone(*z).ok_or(format!("{cid:?}: dead zone"))?;
                if !zone.connectors.contains(&cid) {
                    return Err(format!("{cid:?}: zone does not list it"));
                }
            }
        }
        for (lid, l) in self.lanes() {
            if !self.segment_lanes(l.segment).contains(&lid) {
                return Err(format!("{lid:?}: not listed at its segment"));
            }
            for c in l.outgoing.iter().chain(&l.incoming) {
                if self.connector(*c).is_none() {
                    return Err(format!("{lid:?}: dead connector {c:?}"));
                }
            }
            for n in [l.left, l.right].into_iter().flatten() {
                if self.lane(n).is_none_or(|x| x.segment != l.segment) {
                    return Err(format!("{lid:?}: bad neighbour"));
                }
            }
        }
        for (zid, z) in self.zones() {
            if z.connectors.iter().any(|c| self.connector(*c).is_none()) {
                return Err(format!("{zid:?}: dead member"));
            }
        }
        Ok(())
    }
}

fn carries(kind: ConnectorKind, lane: LaneKind) -> bool {
    match kind {
        ConnectorKind::Road => !matches!(lane, LaneKind::Tram),
        ConnectorKind::Tram => lane.trams(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connectors::approach_lanes;
    use crate::junction::{Approach, build};
    use crate::lanes::segment_lanes;
    use crate::{CrossSection, Profile, RoadKind, SegmentGeom};
    use cb_geometry::{CubicBezier2, Polyline2};

    fn sid(n: u32) -> SegmentId {
        SegmentId::try_new(n).unwrap()
    }
    fn nid(n: u32) -> NodeId {
        NodeId::try_new(n).unwrap()
    }

    /// A T: three streets from node 1 at the origin to nodes 2..4.
    fn t_graph() -> (LaneGraph, Vec<(SegmentId, bool)>, Junction) {
        let cs = CrossSection::of(RoadKind::Street);
        let dirs = [Vec2::X, Vec2::Y, -Vec2::X];
        let mut apps = Vec::new();
        let mut g = LaneGraph::default();
        let mut geoms = Vec::new();
        for (k, d) in dirs.iter().enumerate() {
            let curve = CubicBezier2::straight(Vec2::ZERO, *d * 80.0);
            let geom = SegmentGeom::new(curve, cs.half_width_m());
            let profile = Profile::flat(geom.length(), 0.0);
            apps.push(Approach {
                centerline: Polyline2::new(geom.centerline.points().to_vec()),
                half_width_m: cs.half_width_m(),
                carriageway_half_m: cs.carriageway_half_m(),
                corner_radius_m: 6.0,
                profile: profile.clone(),
                reversed: false,
                lanes: approach_lanes(&cs, true),
            });
            geoms.push((geom, profile, sid(k as u32 + 1)));
        }
        let j = build(&apps).unwrap();
        for (k, (geom, profile, s)) in geoms.iter().enumerate() {
            g.set_segment(SegmentLanes {
                id: *s,
                start: nid(1),
                end: nid(k as u32 + 2),
                lanes: segment_lanes(geom, profile, &cs, (j.cuts[k], 0.0)),
                speed_mps: 13.9,
            });
        }
        let order: Vec<(SegmentId, bool)> = (1..=3).map(|k| (sid(k), true)).collect();
        g.set_node(nid(1), &order, Some(&j));
        for k in 0..3u32 {
            g.set_node(nid(k + 2), &[(sid(k + 1), false)], None);
        }
        (g, order, j)
    }

    #[test]
    fn t_junction_graph_is_consistent_and_connected() {
        let (g, _, j) = t_graph();
        g.check().unwrap();
        let (lanes, conns, zones) = g.counts();
        assert_eq!(lanes, 6);
        // 6 junction movements + 3 dead-end turnarounds.
        assert_eq!(conns, j.connectors.len() + 3);
        assert_eq!(zones, j.zones.len());
        // Junction connectors start at the end of their incoming lane.
        for cid in g.node_connectors(nid(1)) {
            let c = g.connector(*cid).unwrap();
            let end = *g.lane(c.from).unwrap().path.points.last().unwrap();
            assert!((c.points[0] - end).length() < 1e-3);
            let start = g.lane(c.to).unwrap().path.points[0];
            assert!((*c.points.last().unwrap() - start).length() < 1e-3);
        }
        // Every lane can be left and entered (dead ends turn around).
        for (_, l) in g.lanes() {
            assert!(!l.outgoing.is_empty() && !l.incoming.is_empty());
        }
    }

    #[test]
    fn patching_a_segment_clears_and_restores_node_connectors() {
        let (mut g, order, j) = t_graph();
        let before = g.counts();
        let v = g.version();
        let lanes: Vec<LanePath> = g
            .segment_lanes(sid(2))
            .iter()
            .map(|l| g.lane(*l).unwrap().path.clone())
            .collect();
        g.set_segment(SegmentLanes {
            id: sid(2),
            start: nid(1),
            end: nid(3),
            lanes,
            speed_mps: 13.9,
        });
        g.check().unwrap();
        assert!(g.node_connectors(nid(1)).is_empty() && g.node_connectors(nid(3)).is_empty());
        g.set_node(nid(1), &order, Some(&j));
        g.set_node(nid(3), &[(sid(2), false)], None);
        g.check().unwrap();
        assert_eq!(g.counts(), before);
        assert!(g.version() > v);
        assert_eq!(g.remove_segment(sid(3)), Some((nid(1), nid(4))));
        g.remove_node(nid(4));
        g.check().unwrap();
        assert_eq!(g.counts().0, 4);
    }

    #[test]
    fn continuation_links_lanes_in_order() {
        let cs = CrossSection::of(RoadKind::Avenue);
        let mut g = LaneGraph::default();
        for (k, (a, b)) in [
            (Vec2::ZERO, Vec2::new(60.0, 0.0)),
            (Vec2::new(60.0, 0.0), Vec2::new(120.0, 0.0)),
        ]
        .into_iter()
        .enumerate()
        {
            let geom = SegmentGeom::new(CubicBezier2::straight(a, b), cs.half_width_m());
            let p = Profile::flat(geom.length(), 0.0);
            g.set_segment(SegmentLanes {
                id: sid(k as u32 + 1),
                start: nid(k as u32 + 1),
                end: nid(k as u32 + 2),
                lanes: segment_lanes(&geom, &p, &cs, (0.0, 0.0)),
                speed_mps: 16.7,
            });
        }
        g.set_node(nid(2), &[(sid(1), false), (sid(2), true)], None);
        g.check().unwrap();
        // 2 + 2 lanes each way continue: 4 connectors, each joining index k to index k.
        assert_eq!(g.node_connectors(nid(2)).len(), 4);
        for c in g.node_connectors(nid(2)) {
            let c = g.connector(*c).unwrap();
            let (a, b) = (g.lane(c.from).unwrap(), g.lane(c.to).unwrap());
            assert_eq!(a.spec.index, b.spec.index);
            assert_eq!(a.spec.dir, b.spec.dir);
            assert!(c.length_m < 1e-3);
        }
        let f0 = g.find_lane(sid(1), Dir::Forward, 0).unwrap();
        let f1 = g.find_lane(sid(1), Dir::Forward, 1).unwrap();
        assert_eq!(g.lane(f0).unwrap().left, Some(f1));
        assert_eq!(g.lane(f1).unwrap().right, Some(f0));
    }
}
