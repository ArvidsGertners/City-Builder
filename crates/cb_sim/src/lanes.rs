//! The lane graph of the road network (ADR-0008, DATA_MODEL §4.2), patched in `EditSet::Lanes`
//! from the frame's `NetworkChanged` messages: dirty segments get new lanes over their trimmed
//! range, dirty nodes (and both ends of every dirty segment) get new connectors and conflict
//! zones. Work is in ascending id order so the graph's slot layout is deterministic.

use std::collections::BTreeSet;

use bevy::ecs::message::MessageReader;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, Res, ResMut};
use cb_core::ids::{NodeId, SegmentId};
use cb_network::lane_graph::{LaneGraph, SegmentLanes};
use cb_network::lanes::segment_lanes;

use crate::catalog::Catalog;
use crate::ids::Ids;
use crate::roads::{NetworkChanged, RoadNode, RoadSegment};

/// Kilometres per hour → metres per second.
const KMH_TO_MPS: f32 = 1.0 / 3.6;

/// Lane speed of a segment: the road type's speed, 5 km/h less on cobblestone.
fn speed_mps(cat: Option<&Catalog>, seg: &RoadSegment) -> f32 {
    let kmh = cat.map_or(50.0, |c| c.road(seg.kind()).speed_kmh);
    let cobble = seg.style().surface == cb_network::style::RoadSurface::Cobblestone;
    (kmh - if cobble { COBBLE_SLOWDOWN_KMH } else { 0.0 }) * KMH_TO_MPS
}

/// Speed reduction on cobblestone (GAME_DESIGN §3).
const COBBLE_SLOWDOWN_KMH: f32 = 5.0;

/// The lane graph (derived state; rebuilt from the network, never saved).
#[derive(Resource, Clone, Debug, Default)]
pub struct LaneNetwork {
    pub(crate) graph: LaneGraph,
}

impl LaneNetwork {
    /// The graph.
    #[must_use]
    pub fn graph(&self) -> &LaneGraph {
        &self.graph
    }
}

/// `EditSet::Lanes`: patches the lane graph for this frame's network changes.
pub fn patch_lanes(
    mut changes: MessageReader<NetworkChanged>,
    ids: Res<Ids>,
    cat: Option<Res<Catalog>>,
    segments: Query<&RoadSegment>,
    nodes: Query<&RoadNode>,
    mut lanes: ResMut<LaneNetwork>,
) {
    let mut dirty_segs: BTreeSet<SegmentId> = BTreeSet::new();
    let mut dirty_nodes: BTreeSet<NodeId> = BTreeSet::new();
    for c in changes.read() {
        match *c {
            NetworkChanged::Segment(s) | NetworkChanged::SegmentRemoved(s) => {
                dirty_segs.insert(s);
            }
            NetworkChanged::Node(n) | NetworkChanged::NodeRemoved(n) => {
                dirty_nodes.insert(n);
            }
        }
    }
    if dirty_segs.is_empty() && dirty_nodes.is_empty() {
        return;
    }
    let _span = tracing::info_span!("patch_lanes", segments = dirty_segs.len()).entered();
    let graph = &mut lanes.graph;
    for sid in dirty_segs {
        let seg = ids.segments.get(sid).and_then(|e| segments.get(e).ok());
        let Some(seg) = seg else {
            if let Some((a, b)) = graph.remove_segment(sid) {
                dirty_nodes.insert(a);
                dirty_nodes.insert(b);
            }
            continue;
        };
        let cs = seg.cross_section();
        let speed_mps = speed_mps(cat.as_deref(), seg);
        graph.set_segment(SegmentLanes {
            id: sid,
            start: seg.start(),
            end: seg.end(),
            lanes: segment_lanes(seg.geom(), seg.profile(), &cs, seg.cuts()),
            speed_mps,
        });
        dirty_nodes.insert(seg.start());
        dirty_nodes.insert(seg.end());
    }
    for nid in dirty_nodes {
        match ids.nodes.get(nid).and_then(|e| nodes.get(e).ok()) {
            Some(node) => graph.set_node(nid, node.approaches(), node.junction()),
            None => graph.remove_node(nid),
        }
    }
}

/// The lane graph built from scratch from the current network (tests and debug checks compare
/// it with the incrementally patched one).
#[must_use]
pub fn rebuild_all(world: &bevy::ecs::world::World) -> LaneGraph {
    let mut graph = LaneGraph::default();
    let Some(ids) = world.get_resource::<Ids>() else {
        return graph;
    };
    let cat = world.get_resource::<Catalog>();
    for (sid, e) in ids.segments.iter() {
        if let Some(seg) = world.get::<RoadSegment>(e) {
            let cs = seg.cross_section();
            let speed_mps = speed_mps(cat, seg);
            graph.set_segment(SegmentLanes {
                id: sid,
                start: seg.start(),
                end: seg.end(),
                lanes: segment_lanes(seg.geom(), seg.profile(), &cs, seg.cuts()),
                speed_mps,
            });
        }
    }
    for (nid, e) in ids.nodes.iter() {
        if let Some(node) = world.get::<RoadNode>(e) {
            graph.set_node(nid, node.approaches(), node.junction());
        }
    }
    graph
}
