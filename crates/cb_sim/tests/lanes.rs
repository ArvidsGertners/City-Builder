//! Lane graph in the sim (M3, ADR-0008): patched incrementally from network edits, equal to a
//! full rebuild, every lane reachable and leavable.

// Helpers outside `#[test]` fns assert by panicking, like the tests themselves.
#![allow(clippy::unwrap_used)]

use cb_core::ids::SegmentId;
use cb_geometry::CubicBezier2;
use cb_network::RoadKind;
use cb_network::lane_graph::{LaneGraph, LaneId, LaneTurn};
use cb_sim::commands::{BuildRoad, Endpoint, PlayerCommand};
use cb_sim::ids::Ids;
use cb_sim::lanes::{LaneNetwork, rebuild_all};
use cb_sim::roads::{RoadNode, RoadSegment};
use cb_sim::testing::Scenario;
use glam::Vec2;

fn street(s: &mut Scenario, a: Vec2, b: Vec2) {
    let r = s.command(PlayerCommand::BuildRoad(BuildRoad {
        kind: RoadKind::Street,
        curve: CubicBezier2::straight(a, b),
        start: Endpoint::Free,
        end: Endpoint::Free,
    }));
    assert!(r.outcome.is_ok(), "{:?}", r.outcome);
}

fn graph(s: &Scenario) -> &LaneGraph {
    s.world().resource::<LaneNetwork>().graph()
}

fn segment_ids(s: &Scenario) -> Vec<SegmentId> {
    s.world()
        .resource::<Ids>()
        .segments
        .iter()
        .map(|(k, _)| k)
        .collect()
}

/// Connectors as (from segment, dir, index) → (to segment, dir, index), sorted.
fn movement_set(g: &LaneGraph) -> Vec<String> {
    let key = |l: LaneId| {
        let l = g.lane(l).unwrap();
        format!("{}:{:?}:{}", l.segment, l.spec.dir, l.spec.index)
    };
    let mut v: Vec<String> = g
        .connectors()
        .map(|(_, c)| format!("{}>{}@{}:{:?}", key(c.from), key(c.to), c.node, c.kind))
        .collect();
    v.sort();
    v
}

fn assert_matches_rebuild(s: &Scenario) {
    let g = graph(s);
    g.check().unwrap();
    let full = rebuild_all(s.world());
    assert_eq!(g.counts(), full.counts());
    assert_eq!(movement_set(g), movement_set(&full));
    for sid in segment_ids(s) {
        let a: Vec<_> = g
            .segment_lanes(sid)
            .iter()
            .map(|l| g.lane(*l).unwrap().path.clone())
            .collect();
        let b: Vec<_> = full
            .segment_lanes(sid)
            .iter()
            .map(|l| full.lane(*l).unwrap().path.clone())
            .collect();
        assert_eq!(a, b, "{sid}");
    }
}

#[test]
fn lane_graph_follows_network_edits() {
    let mut s = Scenario::new(1);
    // A remote street that no later edit touches.
    street(&mut s, Vec2::new(400.0, 400.0), Vec2::new(500.0, 400.0));
    let remote = segment_ids(&s)[0];
    let remote_lanes = graph(&s).segment_lanes(remote).to_vec();
    street(&mut s, Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0));
    street(&mut s, Vec2::new(0.0, -100.0), Vec2::new(0.0, 100.0));
    assert_matches_rebuild(&s);
    let g = graph(&s);
    let (lanes, conns, zones) = g.counts();
    assert_eq!(lanes, 5 * 2);
    // X: 12 movements; 4 + 2 dead ends turn around.
    assert_eq!(conns, 12 + 6);
    assert!(zones > 0);
    assert_eq!(
        g.connectors()
            .filter(|(_, c)| c.turn == LaneTurn::UTurn)
            .count(),
        6
    );
    // Incremental: the remote street kept its lanes.
    assert_eq!(g.segment_lanes(remote), remote_lanes.as_slice());

    // Bulldoze the north arm: the X becomes a T.
    let north = s
        .world()
        .resource::<Ids>()
        .segments
        .iter()
        .find(|(_, e)| {
            let g = s.world().get::<RoadSegment>(*e).unwrap().geom();
            let m = g.point_at(g.length() * 0.5);
            m.x.abs() < 1.0 && m.y > 20.0
        })
        .unwrap()
        .0;
    let v = graph(&s).version();
    let r = s.command(PlayerCommand::RemoveSegment { segment: north });
    assert!(r.outcome.is_ok());
    assert_matches_rebuild(&s);
    let g = graph(&s);
    assert!(g.version() > v);
    assert!(g.segment_lanes(north).is_empty());
    assert_eq!(g.counts().0, 4 * 2);
    assert_eq!(g.counts().1, 6 + 3 + 2);
    assert_eq!(g.segment_lanes(remote), remote_lanes.as_slice());
}

#[test]
fn lane_graph_town_every_lane_enterable_and_leavable() {
    let mut s = Scenario::new(3);
    let spec = cb_sim::citygen::GridSpec {
        blocks: (3, 2),
        ..Default::default()
    };
    let rep = cb_sim::citygen::grid(s.world_mut(), &spec);
    assert_eq!(rep.roads_rejected, 0);
    s.run_ticks(0);
    assert_matches_rebuild(&s);
    let g = graph(&s);
    for (id, l) in g.lanes() {
        assert!(
            !l.outgoing.is_empty() && !l.incoming.is_empty(),
            "{id:?} {l:?}"
        );
        assert!(l.speed_limit_mps > 5.0 && l.speed_limit_mps <= 50.0 / 3.6 + 1e-3);
    }
    // Every junction (3+ approaches) has conflict zones.
    let ids = s.world().resource::<Ids>();
    for (nid, e) in ids.nodes.iter() {
        let node = s.world().get::<RoadNode>(e).unwrap();
        if node.approaches().len() >= 3 {
            assert!(!g.node_zones(nid).is_empty(), "{nid}");
        }
    }
}
