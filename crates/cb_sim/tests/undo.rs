//! Undo/redo (M3, ADR-0055): every undoable command restores the state hash on undo and
//! reproduces the applied state (same ids) on redo; depth, window, refunds, blocked reasons and
//! command-log replay.

// Helpers outside `#[test]` fns assert by panicking, like the tests themselves.
#![allow(clippy::unwrap_used)]

use cb_core::Money;
use cb_core::ids::{NodeId, PlotId, SegmentId};
use cb_core::time::{TICKS_PER_GAME_DAY, TICKS_PER_GAME_HOUR};
use cb_geometry::CubicBezier2;
use cb_network::control::ControlMode;
use cb_network::style::SegmentStyle;
use cb_network::{RoadKind, Variant};
use cb_sim::catalog::ZoneKind;
use cb_sim::commands::{BuildRoad, CommandLog, CreatePlot, Endpoint, FrontageSpan, PlayerCommand};
use cb_sim::economy::Treasury;
use cb_sim::history::History;
use cb_sim::ids::Ids;
use cb_sim::roads::{RoadNode, RoadSegment};
use cb_sim::testing::Scenario;
use cb_sim::zoning::Plot;
use glam::Vec2;

fn road(a: Vec2, b: Vec2) -> PlayerCommand {
    PlayerCommand::BuildRoad(BuildRoad {
        kind: RoadKind::Street,
        curve: CubicBezier2::straight(a, b),
        start: Endpoint::Free,
        end: Endpoint::Free,
    })
}

fn ok(s: &mut Scenario, cmd: PlayerCommand) -> Money {
    let r = s.command(cmd.clone());
    match r.outcome {
        Ok(a) => a.cost,
        Err(e) => panic!("{cmd:?} rejected: {e:?}"),
    }
}

fn segs(s: &Scenario) -> Vec<RoadSegment> {
    let ids = s.world().resource::<Ids>();
    ids.segments
        .iter()
        .filter_map(|(_, e)| s.world().get::<RoadSegment>(e).cloned())
        .collect()
}

fn first_plot(s: &Scenario) -> PlotId {
    s.world().resource::<Ids>().plots.iter().next().unwrap().0
}

fn junction(s: &Scenario) -> NodeId {
    let ids = s.world().resource::<Ids>();
    ids.nodes
        .iter()
        .find(|(_, e)| {
            s.world()
                .get::<RoadNode>(*e)
                .is_some_and(|n| n.segments().len() >= 3)
        })
        .unwrap()
        .0
}

/// A street with a side street and a plot along the main street (west of the junction).
fn base() -> Scenario {
    let mut s = Scenario::new(5);
    ok(&mut s, road(Vec2::new(-150.0, 0.0), Vec2::new(150.0, 0.0)));
    ok(&mut s, road(Vec2::new(0.0, 0.0), Vec2::new(0.0, 120.0)));
    let west = segs(&s)
        .into_iter()
        .find(|g| g.geom().point_at(0.5 * g.geom().length()).x < -10.0)
        .unwrap();
    let len = west.geom().length();
    ok(
        &mut s,
        PlayerCommand::CreatePlot(CreatePlot {
            zone: ZoneKind::Residential,
            frontage: vec![FrontageSpan {
                segment: west.id(),
                left: false,
                s0: 10.0,
                s1: len - 20.0,
            }],
            depth_m: 30.0,
            dn: 0,
        }),
    );
    s
}

fn west_id(s: &Scenario) -> SegmentId {
    segs(s)
        .into_iter()
        .find(|g| g.geom().point_at(0.5 * g.geom().length()).x < -10.0)
        .unwrap()
        .id()
}

/// Applies `make(s)`, undoes and redoes it, checking hashes and money.
fn roundtrip(name: &str, make: impl Fn(&Scenario) -> PlayerCommand) {
    let mut s = base();
    let h0 = s.state_hash();
    let m0 = s.world().resource::<Treasury>().balance();
    let cmd = make(&s);
    ok(&mut s, cmd);
    let h1 = s.state_hash();
    assert_ne!(h0, h1, "{name}: the command changed nothing");
    ok(&mut s, PlayerCommand::Undo);
    assert_eq!(s.state_hash(), h0, "undo_{name}_restores_state_hash");
    assert_eq!(
        s.world().resource::<Treasury>().balance(),
        m0,
        "{name}: full refund"
    );
    ok(&mut s, PlayerCommand::Redo);
    assert_eq!(s.state_hash(), h1, "redo_{name}_equals_apply");
    // Lane graph and junctions are rebuilt consistently after the round trip.
    let g = s.world().resource::<cb_sim::lanes::LaneNetwork>().graph();
    g.check().unwrap();
    assert_eq!(
        g.counts(),
        cb_sim::lanes::rebuild_all(s.world()).counts(),
        "{name}: lanes"
    );
}

#[test]
fn undo_build_road_restores_state_hash() {
    // Crosses the plot's street: splits segments and clips the plot.
    roundtrip("build_road", |_| {
        road(Vec2::new(-80.0, -100.0), Vec2::new(-80.0, 100.0))
    });
}

#[test]
fn undo_remove_segment_restores_state_hash() {
    roundtrip("remove_segment", |s| PlayerCommand::RemoveSegment {
        segment: west_id(s),
        remove_plots: false,
    });
}

#[test]
fn undo_set_road_type_restores_state_hash() {
    roundtrip("set_road_type", |s| PlayerCommand::SetRoadType {
        segment: west_id(s),
        kind: RoadKind::Avenue,
        variant: Variant::default(),
    });
}

#[test]
fn undo_set_segment_style_restores_state_hash() {
    roundtrip("set_segment_style", |s| PlayerCommand::SetSegmentStyle {
        segment: west_id(s),
        style: SegmentStyle {
            tree_spacing_m: Some(12.0),
            ..SegmentStyle::default()
        },
    });
}

#[test]
fn undo_set_approach_control_restores_state_hash() {
    roundtrip("set_approach_control", |s| {
        PlayerCommand::SetApproachControl {
            node: junction(s),
            mode: ControlMode::AllWayStop,
            custom: Vec::new(),
        }
    });
}

#[test]
fn undo_create_plot_restores_state_hash() {
    roundtrip("create_plot", |s| {
        let east = segs(s)
            .into_iter()
            .find(|g| g.geom().point_at(0.5 * g.geom().length()).x > 10.0)
            .unwrap();
        PlayerCommand::CreatePlot(CreatePlot {
            zone: ZoneKind::Residential,
            frontage: vec![FrontageSpan {
                segment: east.id(),
                left: true,
                s0: 20.0,
                s1: 120.0,
            }],
            depth_m: 25.0,
            dn: 0,
        })
    });
}

#[test]
fn undo_edit_plot_restores_state_hash() {
    roundtrip("edit_plot", |s| {
        let p = first_plot(s);
        let plot = s
            .world()
            .get::<Plot>(s.world().resource::<Ids>().plots.get(p).unwrap())
            .unwrap()
            .clone();
        PlayerCommand::EditPlot {
            plot: p,
            frontage: plot.spans().to_vec(),
            depth_m: 40.0,
        }
    });
}

#[test]
fn undo_delete_plot_restores_state_hash() {
    roundtrip("delete_plot", |s| PlayerCommand::DeletePlot {
        plot: first_plot(s),
    });
}

#[test]
fn undo_set_subdivision_restores_state_hash() {
    roundtrip("set_subdivision", |s| PlayerCommand::SetSubdivision {
        plot: first_plot(s),
        dn: -2,
    });
}

#[test]
fn undo_restores_grown_lots_and_removes_later_houses() {
    // Houses grow on the plot after it was drawn; undoing the plot removes them too.
    let mut s = Scenario::new(5);
    ok(&mut s, road(Vec2::new(-150.0, 0.0), Vec2::new(150.0, 0.0)));
    let h0 = s.state_hash();
    let sid = segs(&s)[0].id();
    ok(
        &mut s,
        PlayerCommand::CreatePlot(CreatePlot {
            zone: ZoneKind::Residential,
            frontage: vec![FrontageSpan {
                segment: sid,
                left: true,
                s0: 20.0,
                s1: 280.0,
            }],
            depth_m: 30.0,
            dn: 0,
        }),
    );
    s.run_ticks(800);
    assert!(!s.world().resource::<Ids>().buildings.is_empty());
    ok(&mut s, PlayerCommand::Undo);
    let ids = s.world().resource::<Ids>();
    assert!(ids.buildings.is_empty() && ids.lots.is_empty() && ids.plots.is_empty());
    let _ = h0; // the clock advanced, so the hash differs by design
}

#[test]
fn undo_depth_limit_100() {
    let mut s = Scenario::new(1);
    for k in 0..105 {
        let x = k as f32 * 30.0 - 1500.0;
        ok(&mut s, road(Vec2::new(x, 500.0), Vec2::new(x, 520.0)));
    }
    assert_eq!(s.world().resource::<History>().undo_entries().len(), 100);
    for _ in 0..100 {
        ok(&mut s, PlayerCommand::Undo);
    }
    // The five oldest roads stay.
    assert_eq!(s.world().resource::<Ids>().segments.len(), 5);
    assert_eq!(
        s.command(PlayerCommand::Undo).outcome.unwrap_err().key,
        "undo-empty"
    );
}

#[test]
fn undo_refund_decays() {
    let mut s = Scenario::new(1);
    let cost = ok(&mut s, road(Vec2::new(0.0, 0.0), Vec2::new(200.0, 0.0)));
    let m1 = s.world().resource::<Treasury>().balance();
    // Halfway through the window after the first hour: 62.5 %.
    s.run_ticks(TICKS_PER_GAME_HOUR + (TICKS_PER_GAME_DAY - TICKS_PER_GAME_HOUR) / 2);
    let upkeep = s
        .world()
        .resource::<cb_sim::economy::Ledger>()
        .total(cb_sim::economy::Account::RoadUpkeep);
    let refund = -ok(&mut s, PlayerCommand::Undo).0;
    assert_eq!(refund, cost.0 * 625 / 1000);
    // The road cost upkeep while it existed (M5 economy).
    assert!(upkeep < 0);
    assert_eq!(
        s.world().resource::<Treasury>().balance(),
        Money(m1.0 + refund + upkeep)
    );
}

#[test]
fn undo_after_window_rejected() {
    let mut s = Scenario::new(1);
    ok(&mut s, road(Vec2::new(0.0, 0.0), Vec2::new(200.0, 0.0)));
    s.run_ticks(TICKS_PER_GAME_DAY + 1);
    let r = s.command(PlayerCommand::Undo);
    assert_eq!(r.outcome.unwrap_err().key, "undo-expired");
    assert_eq!(s.world().resource::<Ids>().segments.len(), 1);
    assert!(s.world().resource::<History>().undo_entries().is_empty());
}

#[test]
fn undo_blocked_reports_reason() {
    let mut s = Scenario::new(1);
    assert_eq!(
        s.command(PlayerCommand::Undo).outcome.unwrap_err().key,
        "undo-empty"
    );
    assert_eq!(
        s.command(PlayerCommand::Redo).outcome.unwrap_err().key,
        "redo-empty"
    );
    // Redo needs the money again: spend it elsewhere, then redo is blocked.
    let cost = ok(&mut s, road(Vec2::new(0.0, 0.0), Vec2::new(200.0, 0.0)));
    ok(&mut s, PlayerCommand::Undo);
    // Spend down to less than the road costs.
    s.world_mut()
        .insert_resource(Treasury::new(Money(cost.0 / 2)));
    let r = s.command(PlayerCommand::Redo);
    assert_eq!(r.outcome.unwrap_err().key, "money-insufficient");
    assert_eq!(s.world().resource::<History>().redo_entries().len(), 1);
}

#[test]
fn replay_command_log_reproduces_hash() {
    let mut s = base();
    ok(
        &mut s,
        road(Vec2::new(-105.0, -100.0), Vec2::new(-105.0, 100.0)),
    );
    s.run_ticks(50);
    ok(&mut s, PlayerCommand::Undo);
    s.run_ticks(30);
    ok(&mut s, PlayerCommand::Redo);
    let p = first_plot(&s);
    ok(&mut s, PlayerCommand::SetSubdivision { plot: p, dn: -1 });
    s.run_ticks(20);
    let want = s.state_hash();
    let log = s.world().resource::<CommandLog>().entries().to_vec();
    // Replay on a fresh scenario at the recorded ticks.
    let mut r = Scenario::new(5);
    for (tick, cmd) in log {
        while r.ticks() < tick {
            r.run_ticks(1);
        }
        ok(&mut r, cmd);
    }
    while r.ticks() < s.ticks() {
        r.run_ticks(1);
    }
    assert_eq!(r.state_hash(), want);
}

#[test]
fn undo_edit_road_geometry_restores_state_hash() {
    roundtrip("edit_road_geometry", |s| {
        PlayerCommand::EditRoadGeometry(cb_sim::commands::GeometryEdit::MoveNode {
            node: junction(s),
            to: Vec2::new(8.0, 3.0),
        })
    });
}
