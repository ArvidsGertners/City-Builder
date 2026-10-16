//! Road hierarchy (M3, ALGORITHMS §1, GAME_DESIGN §3): upgrade/downgrade with `SetRoadType`.

// Helpers outside `#[test]` fns assert by panicking, like the tests themselves.
#![allow(clippy::unwrap_used)]

use cb_core::Money;
use cb_core::ids::SegmentId;
use cb_geometry::CubicBezier2;
use cb_network::{RoadKind, Variant};
use cb_sim::buildings::Building;
use cb_sim::catalog::{Catalog, ZoneKind};
use cb_sim::commands::{BuildRoad, CreatePlot, Endpoint, FrontageSpan, PlayerCommand};
use cb_sim::economy::Treasury;
use cb_sim::ids::Ids;
use cb_sim::lanes::{LaneNetwork, rebuild_all};
use cb_sim::roads::{RoadSegment, preview_set_road_type, road_cost_with};
use cb_sim::testing::Scenario;
use cb_sim::zoning::Plot;
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

fn segs(s: &Scenario) -> Vec<RoadSegment> {
    let ids = s.world().resource::<Ids>();
    ids.segments
        .iter()
        .filter_map(|(_, e)| s.world().get::<RoadSegment>(e).cloned())
        .collect()
}

fn balance(s: &Scenario) -> Money {
    s.world().resource::<Treasury>().balance()
}

fn set_type(s: &mut Scenario, segment: SegmentId, kind: RoadKind, variant: Variant) -> Money {
    let r = s.command(PlayerCommand::SetRoadType {
        segment,
        kind,
        variant,
    });
    r.outcome.unwrap().cost
}

/// The west half of an east–west street with a side street to the north at x = 0.
fn west(s: &Scenario) -> SegmentRemote {
    let seg = segs(s)
        .into_iter()
        .find(|g| {
            let m = g.geom().point_at(0.5 * g.geom().length());
            m.y.abs() < 1.0 && m.x < 0.0
        })
        .unwrap();
    SegmentRemote {
        id: seg.id(),
        len: seg.geom().length(),
    }
}

struct SegmentRemote {
    id: SegmentId,
    len: f32,
}

#[test]
fn upgrade_street_to_avenue_rebuilds_lanes() {
    let mut s = Scenario::new(1);
    road(
        &mut s,
        RoadKind::Street,
        Vec2::new(-150.0, 0.0),
        Vec2::new(150.0, 0.0),
    );
    road(
        &mut s,
        RoadKind::Street,
        Vec2::new(0.0, 0.0),
        Vec2::new(0.0, 120.0),
    );
    let w = west(&s);
    let lanes_before = s
        .world()
        .resource::<LaneNetwork>()
        .graph()
        .segment_lanes(w.id)
        .len();
    assert_eq!(lanes_before, 2);
    let before = balance(&s);
    let cat = s.world().resource::<Catalog>().clone();
    let expect = Money(
        road_cost_with(&cat, RoadKind::Avenue, Variant::default(), w.len).0
            - road_cost_with(&cat, RoadKind::Street, Variant::default(), w.len).0 / 2,
    );
    let north_cut = |s: &Scenario| {
        segs(s)
            .into_iter()
            .find(|g| g.geom().point_at(0.5 * g.geom().length()).y > 20.0)
            .unwrap()
            .cuts()
            .0
    };
    let cut_before = north_cut(&s);
    let cost = set_type(&mut s, w.id, RoadKind::Avenue, Variant::default());
    assert_eq!(cost, expect);
    assert_eq!(balance(&s), Money(before.0 - expect.0));
    let seg = segs(&s).into_iter().find(|g| g.id() == w.id).unwrap();
    assert_eq!(seg.kind(), RoadKind::Avenue);
    assert!((seg.geom().half_width_m - 11.0).abs() < 1e-3);
    // The junction was rebuilt: the side street now stops at the wider avenue.
    assert!(
        north_cut(&s) > cut_before + 3.0,
        "{} vs {cut_before}",
        north_cut(&s)
    );
    let g = s.world().resource::<LaneNetwork>().graph();
    assert_eq!(g.segment_lanes(w.id).len(), 4);
    g.check().unwrap();
    assert_eq!(g.counts(), rebuild_all(s.world()).counts());

    // Bus lanes on the avenue swap lane kinds and cost the variant extra.
    let bus = Variant {
        bus_lanes: true,
        tram_tracks: false,
    };
    let p = preview_set_road_type(s.world(), w.id, RoadKind::Avenue, bus).unwrap();
    assert!(p.cost.0 > 0);
    set_type(&mut s, w.id, RoadKind::Avenue, bus);
    let g = s.world().resource::<LaneNetwork>().graph();
    let kinds: Vec<_> = g
        .segment_lanes(w.id)
        .iter()
        .map(|l| g.lane(*l).unwrap().spec.kind)
        .collect();
    assert_eq!(
        kinds
            .iter()
            .filter(|k| **k == cb_network::LaneKind::Bus)
            .count(),
        2
    );
    // Streets have no room for bus lanes.
    let r = s.command(PlayerCommand::SetRoadType {
        segment: segs(&s)
            .iter()
            .find(|g| g.kind() == RoadKind::Street)
            .unwrap()
            .id(),
        kind: RoadKind::Street,
        variant: bus,
    });
    assert_eq!(r.outcome.unwrap_err().key, "road-invalid-variant");
}

#[test]
fn upgrade_clips_plots_and_prices_demolition() {
    let mut s = Scenario::new(1);
    road(
        &mut s,
        RoadKind::Street,
        Vec2::new(-150.0, 0.0),
        Vec2::new(150.0, 0.0),
    );
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
    let houses_before = s.world().resource::<Ids>().buildings.len();
    assert!(houses_before >= 10);
    let p = preview_set_road_type(s.world(), sid, RoadKind::Avenue, Variant::default()).unwrap();
    // The avenue is 5 m wider per side: houses near the street are in the new corridor.
    let before = balance(&s);
    let cost = set_type(&mut s, sid, RoadKind::Avenue, Variant::default());
    assert_eq!(cost, p.cost);
    assert_eq!(balance(&s), Money(before.0 - p.cost.0));
    s.run_ticks(0);
    let ids = s.world().resource::<Ids>();
    for b in &p.demolished {
        assert!(ids.buildings.get(*b).is_none(), "{b} still stands");
    }
    // The plot re-attached to the new road edge and stays out of the corridor.
    let seg = segs(&s)[0].clone();
    let plot = ids
        .plots
        .iter()
        .find_map(|(_, e)| s.world().get::<Plot>(e).cloned())
        .expect("plot survives the upgrade");
    for v in plot.polygon() {
        assert!(
            v.y > seg.geom().half_width_m - 0.05,
            "plot vertex {v} in the road"
        );
    }
    let near_edge = plot
        .polygon()
        .iter()
        .filter(|v| (v.y - 11.0).abs() < 0.1)
        .count();
    assert!(near_edge >= 2, "frontage not on the new edge");
    // No remaining building pokes into the corridor.
    for (_, e) in ids.buildings.iter() {
        let b = s.world().get::<Building>(e).unwrap();
        assert!(b.footprint().iter().all(|c| c.y > 11.0 - 0.05));
    }
    eprintln!(
        "houses {houses_before} → {}, demolished {}",
        ids.buildings.len(),
        p.demolished.len()
    );
}

#[test]
fn downgrade_refund_zero() {
    let mut s = Scenario::new(1);
    road(
        &mut s,
        RoadKind::Avenue,
        Vec2::new(-100.0, 0.0),
        Vec2::new(100.0, 0.0),
    );
    let sid = segs(&s)[0].id();
    let before = balance(&s);
    let p = preview_set_road_type(s.world(), sid, RoadKind::Street, Variant::default()).unwrap();
    assert_eq!(p.cost, Money::ZERO);
    assert!(p.demolished.is_empty());
    assert_eq!(
        set_type(&mut s, sid, RoadKind::Street, Variant::default()),
        Money::ZERO
    );
    assert_eq!(balance(&s), before);
    assert_eq!(segs(&s)[0].kind(), RoadKind::Street);
    assert_eq!(
        s.world()
            .resource::<LaneNetwork>()
            .graph()
            .segment_lanes(sid)
            .len(),
        2
    );
}
