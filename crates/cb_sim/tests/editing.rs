//! Road geometry editing (M3, ALGORITHMS §1, ADR-0055): moving nodes and reshaping segments
//! rebuilds junctions and lanes, re-attaches plots, and undoes exactly.

// Helpers outside `#[test]` fns assert by panicking, like the tests themselves.
#![allow(clippy::unwrap_used)]

use cb_geometry::CubicBezier2;
use cb_network::RoadKind;
use cb_sim::catalog::ZoneKind;
use cb_sim::commands::{
    BuildRoad, CreatePlot, Endpoint, FrontageSpan, GeometryEdit, PlayerCommand,
};
use cb_sim::ids::Ids;
use cb_sim::lanes::{LaneNetwork, rebuild_all};
use cb_sim::roads::{RoadNode, RoadSegment};
use cb_sim::testing::Scenario;
use cb_sim::zoning::{Lot, Plot};
use glam::Vec2;

fn road(s: &mut Scenario, a: Vec2, b: Vec2) {
    let r = s.command(PlayerCommand::BuildRoad(BuildRoad {
        kind: RoadKind::Street,
        curve: CubicBezier2::straight(a, b),
        start: Endpoint::Free,
        end: Endpoint::Free,
    }));
    assert!(r.outcome.is_ok(), "{:?}", r.outcome);
}

fn segs(s: &Scenario) -> Vec<RoadSegment> {
    let ids = s.world().resource::<Ids>();
    ids.segments
        .iter()
        .filter_map(|(_, e)| s.world().get::<RoadSegment>(e).cloned())
        .collect()
}

fn nodes(s: &Scenario) -> Vec<RoadNode> {
    let ids = s.world().resource::<Ids>();
    ids.nodes
        .iter()
        .filter_map(|(_, e)| s.world().get::<RoadNode>(e).cloned())
        .collect()
}

#[test]
fn edit_road_geometry_regenerates_junctions() {
    let mut s = Scenario::new(1);
    road(&mut s, Vec2::new(-150.0, 0.0), Vec2::new(150.0, 0.0));
    road(&mut s, Vec2::new(0.0, 0.0), Vec2::new(0.0, 120.0));
    let t = nodes(&s)
        .into_iter()
        .find(|n| n.segments().len() == 3)
        .unwrap();
    let cuts0 = t.junction().unwrap().cuts.clone();
    let h0 = s.state_hash();
    // Slide the T node 12 m east along the main road and 4 m north.
    let r = s.command(PlayerCommand::EditRoadGeometry(GeometryEdit::MoveNode {
        node: t.id(),
        to: Vec2::new(12.0, 4.0),
    }));
    assert!(r.outcome.is_ok(), "{:?}", r.outcome);
    let t2 = nodes(&s).into_iter().find(|n| n.id() == t.id()).unwrap();
    assert!((t2.pos().x - 12.0).abs() < 1e-3 && (t2.pos().z - 4.0).abs() < 1e-3);
    let j = t2.junction().expect("junction rebuilt");
    assert_ne!(j.cuts, cuts0);
    // Every incident segment ends at the moved node.
    for g in segs(&s).iter().filter(|g| t2.segments().contains(&g.id())) {
        let end = if g.start() == t.id() {
            g.curve().start()
        } else {
            g.curve().end()
        };
        assert!(end.distance(Vec2::new(12.0, 4.0)) < 1e-3);
    }
    let g = s.world().resource::<LaneNetwork>().graph();
    g.check().unwrap();
    assert_eq!(g.counts(), rebuild_all(s.world()).counts());
    // Undo puts everything back.
    assert!(s.command(PlayerCommand::Undo).outcome.is_ok());
    assert_eq!(s.state_hash(), h0);
    // A move that would cross another road is rejected.
    road(&mut s, Vec2::new(60.0, 40.0), Vec2::new(60.0, 160.0));
    let r = s.command(PlayerCommand::EditRoadGeometry(GeometryEdit::MoveNode {
        node: t.id(),
        to: Vec2::new(0.0, 0.0),
    }));
    assert!(r.outcome.is_ok());
    let top = nodes(&s)
        .into_iter()
        .find(|n| (n.pos().z - 120.0).abs() < 1.0 && n.pos().x.abs() < 1.0)
        .unwrap();
    let r = s.command(PlayerCommand::EditRoadGeometry(GeometryEdit::MoveNode {
        node: top.id(),
        to: Vec2::new(110.0, 110.0),
    }));
    assert_eq!(r.outcome.unwrap_err().key, "road-invalid-edit-crossing");
}

#[test]
fn edit_road_geometry_repoints_dependents() {
    let mut s = Scenario::new(1);
    road(&mut s, Vec2::new(-150.0, 0.0), Vec2::new(150.0, 0.0));
    let sid = segs(&s)[0].id();
    let r = s.command(PlayerCommand::CreatePlot(CreatePlot {
        zone: ZoneKind::Residential,
        frontage: vec![FrontageSpan {
            segment: sid,
            left: true,
            s0: 40.0,
            s1: 260.0,
        }],
        depth_m: 30.0,
        dn: 0,
    }));
    assert!(r.outcome.is_ok());
    // Bend the street gently north (control points 25 m up).
    let r = s.command(PlayerCommand::EditRoadGeometry(GeometryEdit::Reshape {
        segment: sid,
        p1: Vec2::new(-50.0, 25.0),
        p2: Vec2::new(50.0, 25.0),
    }));
    assert!(r.outcome.is_ok(), "{:?}", r.outcome);
    s.run_ticks(0);
    let seg = segs(&s)[0].clone();
    let ids = s.world().resource::<Ids>();
    let plot = ids
        .plots
        .iter()
        .find_map(|(_, e)| s.world().get::<Plot>(e).cloned())
        .expect("the plot follows the road");
    // The frontage lies on the bent road's edge, the plot stays outside the corridor.
    let hw = seg.geom().half_width_m;
    for v in plot.frontage() {
        let (_, d) = seg.geom().project(*v);
        assert!((d.abs() - hw).abs() < 0.1, "frontage {d} vs edge {hw}");
    }
    for v in plot.polygon() {
        assert!(seg.geom().project(*v).1.abs() > hw - 0.1);
    }
    let lots = ids.lots.len();
    assert!(lots >= 8, "{lots}");
    let lot = ids
        .lots
        .iter()
        .find_map(|(_, e)| s.world().get::<Lot>(e).cloned())
        .unwrap();
    assert_eq!(lot.plot(), plot.id());
}

#[test]
fn demolish_validate_lists_dependents() {
    let build = || {
        let mut s = Scenario::new(1);
        road(&mut s, Vec2::new(-150.0, 0.0), Vec2::new(150.0, 0.0));
        road(&mut s, Vec2::new(-150.0, 200.0), Vec2::new(150.0, 200.0));
        let sid = segs(&s)[0].id();
        let r = s.command(PlayerCommand::CreatePlot(CreatePlot {
            zone: ZoneKind::Residential,
            frontage: vec![FrontageSpan {
                segment: sid,
                left: true,
                s0: 20.0,
                s1: 280.0,
            }],
            depth_m: 30.0,
            dn: 0,
        }));
        assert!(r.outcome.is_ok());
        s.run_ticks(800);
        (s, sid)
    };
    let (mut s, sid) = build();
    let deps = cb_sim::roads::demolish_dependents(s.world(), sid);
    let ids = s.world().resource::<Ids>();
    assert_eq!(deps.plots.len(), 1);
    assert_eq!(deps.buildings.len(), ids.buildings.len());
    assert!(deps.buildings.len() >= 10);
    // The other street has none.
    let other = segs(&s)[1].id();
    assert_eq!(
        cb_sim::roads::demolish_dependents(s.world(), other),
        Default::default()
    );
    // "Remove road and plots" takes the houses too.
    let r = s.command(PlayerCommand::RemoveSegment {
        segment: sid,
        remove_plots: true,
    });
    assert!(r.outcome.is_ok());
    let ids = s.world().resource::<Ids>();
    assert!(ids.plots.is_empty() && ids.lots.is_empty() && ids.buildings.is_empty());
    // "Road only" leaves the plot and its houses standing.
    let (mut s, sid) = build();
    let houses = s.world().resource::<Ids>().buildings.len();
    let r = s.command(PlayerCommand::RemoveSegment {
        segment: sid,
        remove_plots: false,
    });
    assert!(r.outcome.is_ok());
    let ids = s.world().resource::<Ids>();
    assert_eq!(ids.plots.len(), 1);
    assert_eq!(ids.buildings.len(), houses);
}
