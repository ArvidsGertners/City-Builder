//! Junction control in the sim (M3, ALGORITHMS §6): automatic classes, custom and light
//! control via `SetApproachControl`, roundabout ring detection.

// Helpers outside `#[test]` fns assert by panicking, like the tests themselves.
#![allow(clippy::unwrap_used)]

use cb_core::ids::NodeId;
use cb_geometry::CubicBezier2;
use cb_network::RoadKind;
use cb_network::control::{ApproachControl, ControlMode};
use cb_network::markings::LineKind;
use cb_sim::commands::{BuildRoad, Endpoint, PlayerCommand};
use cb_sim::ids::Ids;
use cb_sim::roads::{RoadNode, RoadSegment};
use cb_sim::testing::Scenario;
use glam::Vec2;

fn road(s: &mut Scenario, kind: RoadKind, a: Vec2, b: Vec2) {
    let r = s.command(PlayerCommand::BuildRoad(BuildRoad {
        kind,
        curve: CubicBezier2::straight(a, b),
        start: Endpoint::Free,
        end: Endpoint::Free,
    }));
    assert!(r.outcome.is_ok(), "{:?}", r.outcome);
}

fn nodes(s: &Scenario) -> Vec<RoadNode> {
    let ids = s.world().resource::<Ids>();
    ids.nodes
        .iter()
        .filter_map(|(_, e)| s.world().get::<RoadNode>(e).cloned())
        .collect()
}

fn seg_kind(s: &Scenario, id: cb_core::ids::SegmentId) -> RoadKind {
    let ids = s.world().resource::<Ids>();
    s.world()
        .get::<RoadSegment>(ids.segments.get(id).unwrap())
        .unwrap()
        .kind()
}

fn center(s: &Scenario) -> RoadNode {
    nodes(s)
        .into_iter()
        .find(|n| n.segments().len() >= 3)
        .unwrap()
}

/// An avenue (east–west) crossed by a street.
fn avenue_x(s: &mut Scenario) -> NodeId {
    road(
        s,
        RoadKind::Avenue,
        Vec2::new(-150.0, 0.0),
        Vec2::new(150.0, 0.0),
    );
    road(
        s,
        RoadKind::Street,
        Vec2::new(0.0, -120.0),
        Vec2::new(0.0, 120.0),
    );
    center(s).id()
}

#[test]
fn auto_control_gives_the_avenue_priority() {
    let mut s = Scenario::new(1);
    avenue_x(&mut s);
    let c = center(&s);
    assert_eq!(c.controls().len(), 4);
    for (k, (sid, _)) in c.approaches().iter().enumerate() {
        let want = if seg_kind(&s, *sid) == RoadKind::Avenue {
            ApproachControl::Priority
        } else {
            ApproachControl::Yield
        };
        assert_eq!(c.controls()[k], want);
    }
    assert!(c.phases().is_empty());
    // Markings: crosswalks on all four arms; give-way lines only on the yielding street arms
    // (the priority avenue has none).
    assert_eq!(c.markings().crosswalks.len(), 4);
    assert_eq!(c.markings().stop_lines.len(), 2);
    for sl in &c.markings().stop_lines {
        assert_eq!(sl.kind, LineKind::GiveWay);
        assert_eq!(
            seg_kind(&s, c.approaches()[sl.approach].0),
            RoadKind::Street
        );
    }
}

#[test]
fn lights_and_custom_control_via_command() {
    let mut s = Scenario::new(1);
    let n = avenue_x(&mut s);
    let r = s.command(PlayerCommand::SetApproachControl {
        node: n,
        mode: ControlMode::Lights,
        custom: Vec::new(),
    });
    assert!(r.outcome.is_ok());
    let c = center(&s);
    assert_eq!(c.control(), ControlMode::Lights);
    let conns = c.junction().unwrap().connectors.len();
    assert!(c.phases().len() >= 2);
    // Signalised: solid stop lines (carrying the signal poles) on every arm.
    assert_eq!(c.markings().stop_lines.len(), 4);
    assert!(
        c.markings()
            .stop_lines
            .iter()
            .all(|l| l.kind == LineKind::Solid)
    );
    for k in 0..conns {
        assert!(c.phases().iter().any(|p| p.connectors.contains(&k)));
    }
    // Custom: the first street approach gets a stop sign; it survives a rebuild of the node.
    let (sid, at_start) = *c
        .approaches()
        .iter()
        .find(|(sid, _)| seg_kind(&s, *sid) == RoadKind::Street)
        .unwrap();
    let r = s.command(PlayerCommand::SetApproachControl {
        node: n,
        mode: ControlMode::Custom,
        custom: vec![(sid, at_start, ApproachControl::Stop)],
    });
    assert!(r.outcome.is_ok());
    let k = center(&s)
        .approaches()
        .iter()
        .position(|a| *a == (sid, at_start))
        .unwrap();
    assert_eq!(center(&s).controls()[k], ApproachControl::Stop);
    // A road elsewhere does not touch it; all-way stop sets every approach to Stop.
    road(
        &mut s,
        RoadKind::Street,
        Vec2::new(400.0, 0.0),
        Vec2::new(500.0, 0.0),
    );
    assert_eq!(center(&s).controls()[k], ApproachControl::Stop);
    let r = s.command(PlayerCommand::SetApproachControl {
        node: n,
        mode: ControlMode::AllWayStop,
        custom: Vec::new(),
    });
    assert!(r.outcome.is_ok());
    assert!(
        center(&s)
            .controls()
            .iter()
            .all(|c| *c == ApproachControl::Stop)
    );
    // Dead ends have no junction to control.
    let dead = nodes(&s)
        .into_iter()
        .find(|n| n.segments().len() == 1)
        .unwrap()
        .id();
    let r = s.command(PlayerCommand::SetApproachControl {
        node: dead,
        mode: ControlMode::Lights,
        custom: Vec::new(),
    });
    assert!(r.outcome.is_err());
}

#[test]
fn ring_detected_as_roundabout() {
    let mut s = Scenario::new(1);
    assert!(cb_sim::citygen::roundabout(
        s.world_mut(),
        Vec2::ZERO,
        30.0,
        80.0
    ));
    s.run_ticks(0);
    let ring: Vec<RoadNode> = nodes(&s).into_iter().filter(RoadNode::ring).collect();
    assert_eq!(ring.len(), 4, "ring nodes");
    for n in &ring {
        assert!(n.junction().is_some());
        for (k, (sid, _)) in n.approaches().iter().enumerate() {
            let want = if seg_kind(&s, *sid) == RoadKind::OneWayStreet {
                ApproachControl::Priority
            } else {
                ApproachControl::Yield
            };
            assert_eq!(n.controls()[k], want, "{}", n.id());
        }
        // Markings only on the entry: give-way line at the ring, crosswalk set back, a
        // splitter island in two pieces around the pedestrian refuge; nothing on the ring.
        let entry = n
            .approaches()
            .iter()
            .position(|(sid, _)| seg_kind(&s, *sid) == RoadKind::Street)
            .unwrap();
        let m = n.markings();
        assert!(m.crosswalks.iter().all(|c| c.approach == entry));
        assert_eq!(m.crosswalks.len(), 1);
        assert_eq!(m.stop_lines.len(), 1);
        assert_eq!(m.stop_lines[0].kind, LineKind::GiveWay);
        assert!(m.arrows.is_empty());
        assert_eq!(m.islands.len(), 2, "{}", n.id());
        assert!(m.islands.iter().all(|i| i.approach == entry));
    }
    // Lanes: every lane can be entered and left (the ring is navigable).
    let g = s.world().resource::<cb_sim::lanes::LaneNetwork>().graph();
    g.check().unwrap();
    for (_, l) in g.lanes() {
        assert!(!l.outgoing.is_empty() && !l.incoming.is_empty());
    }
    // Breaking the ring clears the flags.
    let arc = s
        .world()
        .resource::<Ids>()
        .segments
        .iter()
        .find(|(id, _)| seg_kind(&s, *id) == RoadKind::OneWayStreet)
        .unwrap()
        .0;
    let r = s.command(PlayerCommand::RemoveSegment {
        segment: arc,
        remove_plots: false,
    });
    assert!(r.outcome.is_ok());
    assert!(nodes(&s).iter().all(|n| !n.ring()));
}

#[test]
fn approach_control_persists_in_snapshot() {
    let mut s = Scenario::new(1);
    let n = avenue_x(&mut s);
    let r = s.command(PlayerCommand::SetApproachControl {
        node: n,
        mode: ControlMode::Lights,
        custom: Vec::new(),
    });
    assert!(r.outcome.is_ok());
    let snap = cb_sim::save::snapshot::capture(s.world());
    assert_eq!(snap.nodes[&n].control(), ControlMode::Lights);
    // A later edit at the junction and its undo keep the lights.
    let side = center(&s)
        .approaches()
        .iter()
        .find(|(sid, _)| seg_kind(&s, *sid) == RoadKind::Street)
        .unwrap()
        .0;
    let r = s.command(PlayerCommand::SetRoadType {
        segment: side,
        kind: RoadKind::Avenue,
        variant: cb_network::Variant::default(),
    });
    assert!(r.outcome.is_ok());
    assert_eq!(center(&s).control(), ControlMode::Lights);
    assert!(s.command(PlayerCommand::Undo).outcome.is_ok());
    assert_eq!(center(&s).control(), ControlMode::Lights);
    assert!(!center(&s).phases().is_empty());
}

#[test]
fn build_roundabout_command_makes_entries_and_undoes_in_one_step() {
    let mut s = Scenario::new(1);
    // Out of the 15–60 m preset range.
    let r = s.command(PlayerCommand::BuildRoundabout {
        center: Vec2::ZERO,
        radius_m: 10.0,
    });
    assert_eq!(r.outcome.unwrap_err().key, "road-invalid-roundabout-radius");
    // A street through the centre becomes two entries of the ring.
    road(
        &mut s,
        RoadKind::Street,
        Vec2::new(-120.0, 0.0),
        Vec2::new(120.0, 0.0),
    );
    let r = s.command(PlayerCommand::BuildRoundabout {
        center: Vec2::new(0.0, 0.0),
        radius_m: 30.0,
    });
    assert!(r.outcome.is_ok(), "{:?}", r.outcome);
    let ring: Vec<RoadNode> = nodes(&s).into_iter().filter(RoadNode::ring).collect();
    assert!(ring.len() >= 4, "ring nodes {}", ring.len());
    let entries = ring
        .iter()
        .flat_map(|n| n.controls().iter())
        .filter(|c| **c == ApproachControl::Yield)
        .count();
    assert_eq!(entries, 2, "both street halves enter the ring");
    // One undo removes the whole ring and restores the street.
    let r = s.command(PlayerCommand::Undo);
    assert!(r.outcome.is_ok(), "{:?}", r.outcome);
    let ids = s.world().resource::<Ids>();
    assert_eq!(ids.segments.iter().count(), 1);
    assert!(nodes(&s).iter().all(|n| !n.ring()));
}

#[test]
fn build_roundabout_rolls_back_when_an_arc_fails() {
    use cb_core::Money;
    use cb_sim::economy::Treasury;
    let mut s = Scenario::new(1);
    // Enough money for about one arc: a later arc fails on funds, everything is rolled back.
    let arc = cb_sim::roads::preview_road(
        s.world(),
        &cb_sim::roads::roundabout_arcs(Vec2::ZERO, 40.0)[0],
    )
    .unwrap()
    .cost;
    s.world_mut()
        .insert_resource(Treasury::new(Money(arc.0 * 3 / 2)));
    let before = s.world().resource::<Treasury>().balance();
    let r = s.command(PlayerCommand::BuildRoundabout {
        center: Vec2::ZERO,
        radius_m: 40.0,
    });
    assert_eq!(r.outcome.unwrap_err().key, "money-insufficient");
    assert_eq!(s.world().resource::<Treasury>().balance(), before);
    assert_eq!(s.world().resource::<Ids>().segments.iter().count(), 0);
    assert!(nodes(&s).is_empty());
    assert!(
        s.world()
            .resource::<cb_sim::history::History>()
            .undo_entries()
            .is_empty()
    );
}
