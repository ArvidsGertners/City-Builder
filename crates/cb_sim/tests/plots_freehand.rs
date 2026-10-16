//! Freehand plots (ALGORITHMS §9, M4): polygon mode snaps road-edge edges within 1.5 m into
//! frontage, rejects outlines without frontage, self-intersections, wrong sizes and overlaps,
//! and vertex/edge drags keep lot IDs (TESTING §3).

// Helpers outside `#[test]` fns assert by panicking, like the tests themselves.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use cb_geometry::CubicBezier2;
use cb_network::RoadKind;
use cb_sim::catalog::ZoneKind;
use cb_sim::commands::{BuildRoad, Endpoint, PlayerCommand};
use cb_sim::ids::Ids;
use cb_sim::roads::RoadSegment;
use cb_sim::testing::Scenario;
use cb_sim::zoning::Plot;
use glam::Vec2;

fn ok(s: &mut Scenario, cmd: PlayerCommand) {
    let r = s.command(cmd);
    assert!(r.outcome.is_ok(), "{:?}", r.outcome);
}

fn err(s: &mut Scenario, cmd: PlayerCommand) -> &'static str {
    s.command(cmd).outcome.unwrap_err().key
}

/// A street along x; returns its half width.
fn street(s: &mut Scenario) -> f32 {
    ok(
        s,
        PlayerCommand::BuildRoad(BuildRoad {
            kind: RoadKind::Street,
            curve: CubicBezier2::straight(Vec2::new(-150.0, 0.0), Vec2::new(150.0, 0.0)),
            start: Endpoint::Free,
            end: Endpoint::Free,
        }),
    );
    let ids = s.world().resource::<Ids>();
    let (_, e) = ids.segments.iter().next().unwrap();
    s.world().get::<RoadSegment>(e).unwrap().geom().half_width_m
}

fn plots(s: &Scenario) -> Vec<Plot> {
    let ids = s.world().resource::<Ids>();
    ids.plots
        .iter()
        .filter_map(|(_, e)| s.world().get::<Plot>(e).cloned())
        .collect()
}

fn quad(hw: f32, x0: f32, x1: f32, front_off: (f32, f32), depth: f32) -> Vec<Vec2> {
    vec![
        Vec2::new(x0, hw + front_off.0),
        Vec2::new(x1, hw + front_off.1),
        Vec2::new(x1, hw + depth),
        Vec2::new(x0, hw + depth),
    ]
}

#[test]
fn plot_polygon_mode_snaps_frontage_1_5m() {
    let mut s = Scenario::new(1);
    let hw = street(&mut s);
    // Front vertices 1.2 m and 1.4 m off the road edge, clockwise order on purpose.
    let mut v = quad(hw, -40.0, 40.0, (1.2, 1.4), 32.0);
    v.reverse();
    ok(
        &mut s,
        PlayerCommand::CreatePlotPolygon {
            zone: ZoneKind::Residential,
            vertices: v,
            dn: 0,
        },
    );
    let p = plots(&s).pop().unwrap();
    let flen: f32 = p.frontage().windows(2).map(|w| w[0].distance(w[1])).sum();
    assert!((flen - 80.0).abs() < 0.1, "frontage {flen}");
    assert!(
        p.frontage().iter().all(|q| (q.y - hw).abs() < 0.02),
        "on the edge"
    );
    assert!(p.outline().iter().any(|q| (q.y - hw).abs() < 0.02));
    assert!(p.lots().len() >= 4, "{}", p.lots().len());
    // Vertices 1.6 m off do not snap: no frontage.
    let r = err(
        &mut s,
        PlayerCommand::CreatePlotPolygon {
            zone: ZoneKind::Residential,
            vertices: quad(-hw, 60.0, 120.0, (-1.6, -1.6), -32.0),
            dn: 0,
        },
    );
    assert_eq!(r, "plot-invalid-no-frontage");
}

#[test]
fn plot_polygon_mode_rejects_no_frontage() {
    let mut s = Scenario::new(1);
    let hw = street(&mut s);
    let r = err(
        &mut s,
        PlayerCommand::CreatePlotPolygon {
            zone: ZoneKind::Commercial,
            vertices: quad(hw, -20.0, 20.0, (5.0, 5.0), 40.0),
            dn: 0,
        },
    );
    assert_eq!(r, "plot-invalid-no-frontage");
    // Only one front vertex near the road: still no frontage edge.
    let r = err(
        &mut s,
        PlayerCommand::CreatePlotPolygon {
            zone: ZoneKind::Commercial,
            vertices: quad(hw, -20.0, 20.0, (0.5, 6.0), 40.0),
            dn: 0,
        },
    );
    assert_eq!(r, "plot-invalid-no-frontage");
    assert!(plots(&s).is_empty());
}

fn lot_ids(s: &Scenario) -> Vec<cb_core::ids::LotId> {
    let mut v = plots(s).pop().unwrap().lots().to_vec();
    v.sort();
    v
}

#[test]
fn plot_vertex_drag_keeps_lot_ids() {
    let mut s = Scenario::new(2);
    let hw = street(&mut s);
    ok(
        &mut s,
        PlayerCommand::CreatePlotPolygon {
            zone: ZoneKind::Residential,
            vertices: quad(hw, -40.0, 40.0, (0.0, 0.0), 30.0),
            dn: 0,
        },
    );
    let before = lot_ids(&s);
    let p = plots(&s).pop().unwrap();
    // Drag the back-right vertex 6 m further back.
    let mut v = p.outline().to_vec();
    let k = v
        .iter()
        .enumerate()
        .max_by(|a, b| (a.1.x + a.1.y).total_cmp(&(b.1.x + b.1.y)))
        .map(|(k, _)| k)
        .unwrap();
    v[k].y += 6.0;
    ok(
        &mut s,
        PlayerCommand::EditPlotPolygon {
            plot: p.id(),
            vertices: v,
        },
    );
    assert_eq!(lot_ids(&s), before);
    let after = plots(&s).pop().unwrap();
    assert!(cb_geometry::polygon::area(after.polygon()) > cb_geometry::polygon::area(p.polygon()));
}

#[test]
fn plot_edge_drag_keeps_lot_ids() {
    let mut s = Scenario::new(3);
    let hw = street(&mut s);
    ok(
        &mut s,
        PlayerCommand::CreatePlotPolygon {
            zone: ZoneKind::Residential,
            vertices: quad(hw, -40.0, 40.0, (0.0, 0.0), 30.0),
            dn: 0,
        },
    );
    let before = lot_ids(&s);
    let p = plots(&s).pop().unwrap();
    // Drag the back edge (both back vertices) 8 m back.
    let v: Vec<Vec2> = p
        .outline()
        .iter()
        .map(|q| {
            if q.y > hw + 1.0 {
                *q + Vec2::Y * 8.0
            } else {
                *q
            }
        })
        .collect();
    ok(
        &mut s,
        PlayerCommand::EditPlotPolygon {
            plot: p.id(),
            vertices: v,
        },
    );
    assert_eq!(lot_ids(&s), before);
    // Undo restores the old outline in one step.
    ok(&mut s, PlayerCommand::Undo);
    assert_eq!(plots(&s).pop().unwrap().outline(), p.outline());
}

#[test]
fn plot_freehand_validation_keys() {
    let mut s = Scenario::new(4);
    let hw = street(&mut s);
    let mk = |vertices: Vec<Vec2>| PlayerCommand::CreatePlotPolygon {
        zone: ZoneKind::Residential,
        vertices,
        dn: 0,
    };
    // Bow tie: the back edges cross.
    let bow = vec![
        Vec2::new(-30.0, hw),
        Vec2::new(30.0, hw),
        Vec2::new(-30.0, hw + 30.0),
        Vec2::new(30.0, hw + 30.0),
    ];
    assert_eq!(err(&mut s, mk(bow)), "plot-invalid-self-intersection");
    // Too small (10 m × 4 m) and too few vertices.
    assert_eq!(
        err(&mut s, mk(quad(hw, -5.0, 5.0, (0.0, 0.0), 4.0))),
        "plot-invalid-size"
    );
    assert_eq!(
        err(&mut s, mk(vec![Vec2::new(0.0, hw), Vec2::new(10.0, hw)])),
        "plot-invalid-size"
    );
    // A plot, then an outline whose frontage lies inside it: nothing is left after the clip.
    ok(&mut s, mk(quad(hw, -40.0, 40.0, (0.0, 0.0), 30.0)));
    assert_eq!(
        err(&mut s, mk(quad(hw, -20.0, 20.0, (0.0, 0.0), 20.0))),
        "plot-invalid-overlap"
    );
    // A neighbour that overlaps partly is clipped (auto-clip), not rejected.
    ok(&mut s, mk(quad(hw, 30.0, 90.0, (0.0, 0.0), 30.0)));
    let ps = plots(&s);
    let a = cb_geometry::polygon::area(ps[1].polygon());
    assert!(a < 60.0 * 30.0 - 200.0, "clipped: {a}");
}
