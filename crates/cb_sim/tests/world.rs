//! Headless world-state tests for M2: roads, junctions, plots, lots, growth, citygen,
//! determinism (TESTING §3).

use cb_core::Money;
use cb_geometry::CubicBezier2;
use cb_network::RoadKind;
use cb_sim::buildings::Building;
use cb_sim::catalog::ZoneKind;
use cb_sim::commands::{BuildRoad, CreatePlot, Endpoint, FrontageSpan, PlayerCommand};
use cb_sim::economy::Treasury;
use cb_sim::ids::Ids;
use cb_sim::roads::{RoadNode, RoadSegment};
use cb_sim::testing::Scenario;
use cb_sim::zoning::{Lot, Plot};
use glam::Vec2;

fn road(kind: RoadKind, a: Vec2, b: Vec2, start: Endpoint, end: Endpoint) -> PlayerCommand {
    PlayerCommand::BuildRoad(BuildRoad {
        kind,
        curve: CubicBezier2::straight(a, b),
        start,
        end,
    })
}

fn counts(s: &Scenario) -> (usize, usize, usize, usize, usize) {
    let ids = s.world().resource::<Ids>();
    (
        ids.nodes.len(),
        ids.segments.len(),
        ids.plots.len(),
        ids.lots.len(),
        ids.buildings.len(),
    )
}

fn segments(s: &Scenario) -> Vec<RoadSegment> {
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

fn lots(s: &Scenario) -> Vec<Lot> {
    let ids = s.world().resource::<Ids>();
    ids.lots
        .iter()
        .filter_map(|(_, e)| s.world().get::<Lot>(e).cloned())
        .collect()
}

fn plots(s: &Scenario) -> Vec<Plot> {
    let ids = s.world().resource::<Ids>();
    ids.plots
        .iter()
        .filter_map(|(_, e)| s.world().get::<Plot>(e).cloned())
        .collect()
}

fn ok(s: &mut Scenario, cmd: PlayerCommand) -> Money {
    let r = s.command(cmd);
    match r.outcome {
        Ok(a) => a.cost,
        Err(e) => panic!("command rejected: {e:?}"),
    }
}

#[test]
fn build_straight_road_charges_and_creates_nodes() {
    let mut s = Scenario::new(1);
    let before = s.world().resource::<Treasury>().balance();
    let cost = ok(
        &mut s,
        road(
            RoadKind::Street,
            Vec2::ZERO,
            Vec2::new(100.0, 0.0),
            Endpoint::Free,
            Endpoint::Free,
        ),
    );
    assert_eq!(cost, Money(100 * 2200));
    assert_eq!(
        s.world().resource::<Treasury>().balance(),
        Money(before.0 - cost.0)
    );
    assert_eq!(counts(&s).0, 2);
    assert_eq!(counts(&s).1, 1);
}

#[test]
fn road_invalid_reasons_return_keys() {
    let mut s = Scenario::new(1);
    let short = s.command(road(
        RoadKind::Street,
        Vec2::ZERO,
        Vec2::new(3.0, 0.0),
        Endpoint::Free,
        Endpoint::Free,
    ));
    assert_eq!(short.outcome.unwrap_err().key, "road-invalid-too-short");
    let sharp = s.command(PlayerCommand::BuildRoad(BuildRoad {
        kind: RoadKind::Avenue,
        curve: CubicBezier2::new(
            Vec2::ZERO,
            Vec2::new(30.0, 0.0),
            Vec2::new(30.0, 30.0),
            Vec2::new(0.0, 30.0),
        ),
        start: Endpoint::Free,
        end: Endpoint::Free,
    }));
    assert_eq!(sharp.outcome.unwrap_err().key, "road-invalid-too-sharp");
    let oob = s.command(road(
        RoadKind::Street,
        Vec2::new(2000.0, 0.0),
        Vec2::new(2100.0, 0.0),
        Endpoint::Free,
        Endpoint::Free,
    ));
    assert_eq!(oob.outcome.unwrap_err().key, "road-invalid-out-of-bounds");
    assert_eq!(counts(&s).1, 0, "rejected commands leave no trace");
}

#[test]
fn crossing_roads_form_an_x_junction() {
    let mut s = Scenario::new(1);
    ok(
        &mut s,
        road(
            RoadKind::Street,
            Vec2::new(-100.0, 0.0),
            Vec2::new(100.0, 0.0),
            Endpoint::Free,
            Endpoint::Free,
        ),
    );
    ok(
        &mut s,
        road(
            RoadKind::Street,
            Vec2::new(0.0, -100.0),
            Vec2::new(0.0, 100.0),
            Endpoint::Free,
            Endpoint::Free,
        ),
    );
    let (n, sg, ..) = counts(&s);
    assert_eq!((n, sg), (5, 4));
    let center = nodes(&s)
        .into_iter()
        .find(|n| n.segments().len() == 4)
        .expect("4-way node");
    let j = center.junction().expect("junction geometry");
    assert_eq!(j.corners.len(), 4);
    for seg in segments(&s) {
        let (c0, c1) = seg.cuts();
        assert!(c0 > 5.0 || c1 > 5.0, "the centre end is cut back");
    }
}

#[test]
fn t_junction_by_snapping_onto_a_segment() {
    let mut s = Scenario::new(1);
    ok(
        &mut s,
        road(
            RoadKind::Street,
            Vec2::new(-100.0, 0.0),
            Vec2::new(100.0, 0.0),
            Endpoint::Free,
            Endpoint::Free,
        ),
    );
    let sid = segments(&s)[0].id();
    ok(
        &mut s,
        road(
            RoadKind::Street,
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 90.0),
            Endpoint::Segment(sid),
            Endpoint::Free,
        ),
    );
    let (n, sg, ..) = counts(&s);
    assert_eq!((n, sg), (4, 3));
    let t = nodes(&s)
        .into_iter()
        .find(|n| n.segments().len() == 3)
        .expect("T node");
    assert_eq!(
        t.junction()
            .expect("junction")
            .corners
            .iter()
            .filter(|c| c.fillet)
            .count(),
        2
    );
}

#[test]
fn junction_too_close_is_rejected() {
    let mut s = Scenario::new(1);
    ok(
        &mut s,
        road(
            RoadKind::Street,
            Vec2::new(-100.0, 0.0),
            Vec2::new(100.0, 0.0),
            Endpoint::Free,
            Endpoint::Free,
        ),
    );
    ok(
        &mut s,
        road(
            RoadKind::Street,
            Vec2::new(0.0, -50.0),
            Vec2::new(0.0, 50.0),
            Endpoint::Free,
            Endpoint::Free,
        ),
    );
    let r = s.command(road(
        RoadKind::Street,
        Vec2::new(8.0, -50.0),
        Vec2::new(8.0, 50.0),
        Endpoint::Free,
        Endpoint::Free,
    ));
    let key = r.outcome.unwrap_err().key;
    assert!(
        key.starts_with("road-invalid-junction") || key == "road-invalid-overlap",
        "{key}"
    );
}

fn street_with_plot(s: &mut Scenario) -> cb_core::ids::SegmentId {
    ok(
        s,
        road(
            RoadKind::Street,
            Vec2::new(-150.0, 0.0),
            Vec2::new(150.0, 0.0),
            Endpoint::Free,
            Endpoint::Free,
        ),
    );
    let sid = segments(s)[0].id();
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
    assert!(r.outcome.is_ok(), "{:?}", r.outcome);
    sid
}

#[test]
fn plot_subdivides_into_lots_covering_it() {
    let mut s = Scenario::new(1);
    street_with_plot(&mut s);
    let p = &plots(&s)[0];
    let area = cb_geometry::polygon::area(p.polygon());
    assert!((area - 260.0 * 30.0).abs() < 30.0, "{area}");
    let ls = lots(&s);
    assert_eq!(ls.len(), p.lots().len());
    assert!((15..=26).contains(&ls.len()), "{}", ls.len());
    let sum: f32 = ls.iter().map(Lot::area_m2).sum();
    assert!((sum - area).abs() / area < 0.005);
    // Lots lie on the plot side of the road (z > 6 m road half width).
    for l in &ls {
        assert!(
            l.polygon().iter().all(|v| v.y > 5.99),
            "lot inside the road"
        );
        assert!(l.footprint().is_some());
    }
}

#[test]
fn plot_clipped_by_new_corridor() {
    let mut s = Scenario::new(1);
    let _ = street_with_plot(&mut s);
    let before = plots(&s)[0].polygon().to_vec();
    let lots_before: Vec<_> = lots(&s).iter().map(Lot::id).collect();
    // A side street from the main street into the plot.
    let main = segments(&s)[0].id();
    ok(
        &mut s,
        road(
            RoadKind::Street,
            Vec2::new(30.0, 0.0),
            Vec2::new(30.0, 120.0),
            Endpoint::Segment(main),
            Endpoint::Free,
        ),
    );
    let p = &plots(&s)[0];
    let (a0, a1) = (
        cb_geometry::polygon::area(&before),
        cb_geometry::polygon::area(p.polygon()),
    );
    assert!(a1 < a0 - 200.0, "{a0} → {a1}");
    // No lot overlaps any corridor.
    for seg in segments(&s) {
        let corridor: Vec<glam::DVec2> = seg.geom().corridor.iter().map(|v| v.as_dvec2()).collect();
        for l in lots(&s) {
            let lp: Vec<glam::DVec2> = l.polygon().iter().map(|v| v.as_dvec2()).collect();
            let inter: f64 = cb_geometry::boolean::f64ops::intersect(&lp, &corridor)
                .iter()
                .map(|q| cb_geometry::d2::area(q))
                .sum();
            assert!(
                inter < 0.5,
                "lot {:?} overlaps a corridor by {inter} m²",
                l.id()
            );
        }
    }
    // Lots away from the new street keep their IDs.
    let after: Vec<_> = lots(&s).iter().map(Lot::id).collect();
    let kept = lots_before.iter().filter(|id| after.contains(id)).count();
    assert!(
        kept * 2 >= lots_before.len(),
        "kept {kept} of {}",
        lots_before.len()
    );
}

#[test]
fn corner_plot_around_a_junction() {
    let mut s = Scenario::new(1);
    ok(
        &mut s,
        road(
            RoadKind::Street,
            Vec2::new(-100.0, 0.0),
            Vec2::new(100.0, 0.0),
            Endpoint::Free,
            Endpoint::Free,
        ),
    );
    ok(
        &mut s,
        road(
            RoadKind::Street,
            Vec2::new(0.0, -100.0),
            Vec2::new(0.0, 100.0),
            Endpoint::Free,
            Endpoint::Free,
        ),
    );
    // NE quadrant: down the north arm's east side, around the corner, along the east arm's
    // north side.
    let segs = segments(&s);
    let north = segs
        .iter()
        .find(|g| g.geom().point_at(g.geom().length() * 0.5).y > 20.0)
        .expect("north arm");
    let east = segs
        .iter()
        .find(|g| g.geom().point_at(g.geom().length() * 0.5).x > 20.0)
        .expect("east arm");
    let span = |g: &RoadSegment, toward_node: bool| {
        let len = g.geom().length();
        // Which side faces the NE quadrant and where the node is.
        let mid = g.geom().point_at(0.5 * len);
        let probe = mid + g.geom().dir_at(0.5 * len).perp() * 10.0;
        let left = probe.x > 0.0 && probe.y > 0.0;
        let node_at_start = g.geom().point_at(0.0).length() < 1.0;
        let (near, far) = if node_at_start {
            (0.0, len)
        } else {
            (len, 0.0)
        };
        let (s0, s1) = if toward_node {
            (far, near)
        } else {
            (near, far)
        };
        FrontageSpan {
            segment: g.id(),
            left,
            s0,
            s1,
        }
    };
    let r = s.command(PlayerCommand::CreatePlot(CreatePlot {
        zone: ZoneKind::Residential,
        frontage: vec![span(north, true), span(east, false)],
        depth_m: 30.0,
        dn: 0,
    }));
    assert!(r.outcome.is_ok(), "{:?}", r.outcome);
    let ls = lots(&s);
    assert!(ls.iter().any(Lot::corner), "a corner lot spans the fillet");
}

#[test]
fn debug_growth_builds_houses_inside_lots() {
    let mut s = Scenario::new(1);
    street_with_plot(&mut s);
    s.run_ticks(800);
    let ids = s.world().resource::<Ids>();
    assert!(ids.buildings.len() >= 10, "{}", ids.buildings.len());
    for (_, e) in ids.buildings.iter() {
        let b = s.world().get::<Building>(e).unwrap();
        let lot = lots(&s).into_iter().find(|l| l.id() == b.lot()).unwrap();
        for c in b.layout().outlines().into_iter().flatten() {
            assert!(
                cb_geometry::polygon::contains(lot.polygon(), c)
                    || cb_geometry::polygon::boundary_distance(lot.polygon(), c) < 0.01
            );
        }
    }
}

#[test]
fn removing_a_segment_rebuilds_the_junction() {
    let mut s = Scenario::new(1);
    ok(
        &mut s,
        road(
            RoadKind::Street,
            Vec2::new(-100.0, 0.0),
            Vec2::new(100.0, 0.0),
            Endpoint::Free,
            Endpoint::Free,
        ),
    );
    ok(
        &mut s,
        road(
            RoadKind::Street,
            Vec2::new(0.0, -100.0),
            Vec2::new(0.0, 100.0),
            Endpoint::Free,
            Endpoint::Free,
        ),
    );
    let north = segments(&s)
        .into_iter()
        .find(|g| g.geom().point_at(g.geom().length() * 0.5).y > 20.0)
        .unwrap()
        .id();
    ok(&mut s, PlayerCommand::RemoveSegment { segment: north });
    let (n, sg, ..) = counts(&s);
    assert_eq!((n, sg), (4, 3));
    let t = nodes(&s)
        .into_iter()
        .find(|n| n.segments().len() == 3)
        .unwrap();
    assert_eq!(t.junction().unwrap().corners.len(), 3);
}

#[test]
fn citygen_grid_with_houses() {
    let mut s = Scenario::new(1);
    let spec = cb_sim::citygen::GridSpec {
        blocks: (3, 2),
        ..Default::default()
    };
    let rep = cb_sim::citygen::grid(s.world_mut(), &spec);
    assert_eq!(rep.roads_rejected, 0, "{rep:?}");
    assert_eq!(rep.segments, 3 * 3 + 4 * 2);
    assert!(rep.buildings >= 100, "{rep:?}");
}

#[test]
fn same_commands_same_state_hash() {
    let run = || {
        let mut s = Scenario::new(7);
        street_with_plot(&mut s);
        ok(
            &mut s,
            road(
                RoadKind::Street,
                Vec2::new(0.0, -80.0),
                Vec2::new(0.0, 0.0),
                Endpoint::Free,
                Endpoint::Free,
            ),
        );
        s.run_ticks(700);
        s.state_hash()
    };
    assert_eq!(run(), run());
}

#[test]
fn edit_plot_keeps_lot_ids_and_previews_agree() {
    let mut s = Scenario::new(1);
    let sid = street_with_plot(&mut s);
    let p = plots(&s)[0].clone();
    let before: Vec<_> = p.lots().to_vec();
    let spans = vec![FrontageSpan {
        segment: sid,
        left: true,
        s0: 20.0,
        s1: 290.0,
    }];
    let preview =
        cb_sim::zoning::preview_edit_plot(s.world(), p.id(), &spans, 30.0).expect("edit preview");
    // A new plot over the same frontage would overlap the existing one.
    let fresh = cb_sim::zoning::preview_plot(
        s.world(),
        &CreatePlot {
            zone: ZoneKind::Residential,
            frontage: spans.clone(),
            depth_m: 30.0,
            dn: 0,
        },
    );
    assert!(fresh.is_err(), "{fresh:?}");
    let hash_before = s.state_hash();
    ok(
        &mut s,
        PlayerCommand::EditPlot {
            plot: p.id(),
            frontage: spans,
            depth_m: 30.0,
        },
    );
    assert_ne!(s.state_hash(), hash_before);
    let q = plots(&s)[0].clone();
    let a_prev = cb_geometry::polygon::area(&preview);
    let a_new = cb_geometry::polygon::area(q.polygon());
    assert!((a_prev - a_new).abs() < 1.0, "{a_prev} vs {a_new}");
    assert!((a_new - 270.0 * 30.0).abs() < 30.0, "{a_new}");
    let kept = q.lots().iter().filter(|l| before.contains(l)).count();
    assert!(
        kept * 10 >= before.len() * 7,
        "only {kept} of {} lot IDs kept",
        before.len()
    );
    // Depth edit: deeper plot, same frontage.
    let spans = q.spans().to_vec();
    ok(
        &mut s,
        PlayerCommand::EditPlot {
            plot: q.id(),
            frontage: spans,
            depth_m: 40.0,
        },
    );
    let r = plots(&s)[0].clone();
    assert!((r.depth_m() - 40.0).abs() < 1e-3);
    assert!((cb_geometry::polygon::area(r.polygon()) - 270.0 * 40.0).abs() < 40.0);
}

#[test]
fn road_preview_matches_build() {
    let mut s = Scenario::new(1);
    let cmd = BuildRoad {
        kind: RoadKind::Street,
        curve: CubicBezier2::straight(Vec2::ZERO, Vec2::new(120.0, 0.0)),
        start: Endpoint::Free,
        end: Endpoint::Free,
    };
    let pv = cb_sim::roads::preview_road(s.world(), &cmd).expect("preview");
    let hash = s.state_hash();
    let _ = cb_sim::roads::preview_road(s.world(), &cmd);
    assert_eq!(s.state_hash(), hash, "preview must not mutate");
    let cost = ok(&mut s, PlayerCommand::BuildRoad(cmd));
    assert_eq!(pv.cost, cost);
}

#[test]
fn m2_scripted_scene_has_its_features() {
    let mut s = Scenario::with_terrain(1234, cb_sim::terrain::TerrainSetup::Generate);
    let f = cb_sim::citygen::m2_features(s.world_mut(), Vec2::ZERO, Vec2::new(220.0, 120.0));
    eprintln!("{f:#?}");
    assert!(f.slope.is_some(), "no slope street");
    assert!(f.corner.is_some(), "no corner plot");
    assert!(f.cut.is_some_and(|c| c.depth_m >= 2.0), "cut {:?}", f.cut);
    assert!(
        f.fill.is_some_and(|c| c.depth_m >= 1.0),
        "fill {:?}",
        f.fill
    );
    assert!(f.steep.is_some(), "no steep ground");
    // Houses only along the slope and side streets here (no town nodes, so no shore street;
    // the crossings stay bare for the cut and fill shots).
    assert!(f.report.buildings >= 20, "{:?}", f.report);
}

/// Highest terrain sample above the junction carriageway (m), over a grid of points inside
/// every junction of `s`.
fn junction_terrain_poke(s: &Scenario) -> (f32, Vec2) {
    let t = s.world().resource::<cb_sim::terrain::TerrainRes>();
    let mut worst = (f32::NEG_INFINITY, Vec2::ZERO);
    for n in nodes(s) {
        let Some(j) = n.junction() else { continue };
        // The surface the junction mesh draws.
        for v in cb_network::junction::carriageway_triangles(&j.inner) {
            for a in 1..10 {
                for b in 1..(10 - a) {
                    let (u, w) = (a as f32 / 10.0, b as f32 / 10.0);
                    let p = v[0] * (1.0 - u - w) + v[1] * u + v[2] * w;
                    let q = Vec2::new(p.x, p.z);
                    let d = t.terrain().height_at(q) - p.y;
                    if d > worst.0 {
                        worst = (d, q);
                    }
                }
            }
        }
    }
    worst
}

#[test]
fn junction_surface_above_terrain() {
    let mut s = Scenario::with_terrain(1234, cb_sim::terrain::TerrainSetup::Generate);
    let site = Vec2::new(19.2, -1228.8);
    cb_sim::citygen::grid(
        s.world_mut(),
        &cb_sim::citygen::GridSpec {
            center: site,
            blocks: (4, 3),
            grow: false,
            plot_depth_m: 0.0,
            ..Default::default()
        },
    );
    // Plus the T junction on the 8 % slope and the other scripted roads.
    cb_sim::citygen::m2_features(s.world_mut(), site, Vec2::new(220.0, 120.0));
    let (d, at) = junction_terrain_poke(&s);
    eprintln!("worst terrain above junction surface: {d:.3} m at {at}");
    assert!(d < 0.0, "terrain {d:.3} m above a junction surface at {at}");
}

#[test]
fn m2_scripted_lots_all_grow() {
    let mut s = Scenario::with_terrain(1234, cb_sim::terrain::TerrainSetup::Generate);
    let site = Vec2::new(19.2, -1228.8);
    cb_sim::citygen::grid(
        s.world_mut(),
        &cb_sim::citygen::GridSpec {
            center: site,
            blocks: (4, 3),
            ..Default::default()
        },
    );
    let f = cb_sim::citygen::m2_features(s.world_mut(), site, Vec2::new(220.0, 120.0));
    let corner = f.corner.expect("corner plot");
    // Regression: the corner plot's lots were skipped by growth.
    let empty: Vec<_> = lots(&s)
        .into_iter()
        .filter(|l| l.footprint().is_some() && l.building().is_none())
        .map(|l| l.id())
        .collect();
    assert!(
        empty.is_empty(),
        "lots with a footprint but no house: {empty:?}"
    );
    let near_corner = lots(&s)
        .iter()
        .filter(|l| l.building().is_some() && l.polygon().iter().any(|p| p.distance(corner) < 40.0))
        .count();
    assert!(near_corner >= 2, "{near_corner}");
}

#[test]
fn road_through_existing_node_joins_it() {
    // A diagonal road passing exactly through the T node of an east–west street joins that node
    // (the crossing is reported by both segments meeting there).
    let mut s = Scenario::new(1);
    ok(
        &mut s,
        road(
            RoadKind::Street,
            Vec2::new(-150.0, 0.0),
            Vec2::new(150.0, 0.0),
            Endpoint::Free,
            Endpoint::Free,
        ),
    );
    ok(
        &mut s,
        road(
            RoadKind::Street,
            Vec2::new(0.0, 0.0),
            Vec2::new(0.0, 100.0),
            Endpoint::Free,
            Endpoint::Free,
        ),
    );
    let before = counts(&s);
    let r = s.command(road(
        RoadKind::Street,
        Vec2::new(-70.0, -70.0),
        Vec2::new(70.0, 70.0),
        Endpoint::Free,
        Endpoint::Free,
    ));
    // Either joined (one 5-way node) or rejected for the junction shape — never two nodes on
    // top of each other.
    if r.outcome.is_ok() {
        let (n, sg, ..) = counts(&s);
        assert_eq!((n, sg), (before.0 + 2, before.1 + 2));
        let five = nodes(&s).into_iter().find(|n| n.segments().len() == 5);
        assert!(five.is_some(), "the diagonal joined the existing node");
    }
    let ns = nodes(&s);
    for (i, a) in ns.iter().enumerate() {
        for b in &ns[i + 1..] {
            assert!(a.pos().distance(b.pos()) > 1.0, "duplicate nodes");
        }
    }
}
