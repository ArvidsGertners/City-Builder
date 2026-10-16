//! Bridges and overpasses (M3, ALGORITHMS §3, ADR-0065) on a synthetic valley: a 20 m plain cut
//! by a straight valley along the z axis.

// Helpers outside `#[test]` fns assert by panicking, like the tests themselves.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use cb_core::Money;
use cb_core::ids::SegmentId;
use cb_geometry::CubicBezier2;
use cb_network::RoadKind;
use cb_network::bridge::{self, MIN_SPAN_M, UNDER_DECK_CLEARANCE_M, WATER_CLEARANCE_M};
use cb_sim::commands::{BuildRoad, Endpoint, PlayerCommand, ValidationError};
use cb_sim::ids::Ids;
use cb_sim::roads::{RoadSegment, preview_road, road_cost};
use cb_sim::terrain::{TerrainRes, TerrainSetup};
use cb_sim::testing::Scenario;
use glam::Vec2;

const PLAIN_M: f32 = 20.0;

fn valley(depth_m: f32, width_m: f32, water_m: Option<f32>) -> Scenario {
    Scenario::with_terrain(
        1,
        TerrainSetup::Valley {
            samples: 257,
            spacing_m: 4.0,
            h: PLAIN_M,
            depth_m,
            width_m,
            water_m,
        },
    )
}

fn cmd(kind: RoadKind, a: Vec2, b: Vec2) -> PlayerCommand {
    PlayerCommand::BuildRoad(BuildRoad {
        kind,
        curve: CubicBezier2::straight(a, b),
        start: Endpoint::Free,
        end: Endpoint::Free,
    })
}

fn build(s: &mut Scenario, kind: RoadKind, a: Vec2, b: Vec2) -> Result<Money, ValidationError> {
    s.command(cmd(kind, a, b)).outcome.map(|o| o.cost)
}

fn segs(s: &Scenario) -> Vec<RoadSegment> {
    let ids = s.world().resource::<Ids>();
    ids.segments
        .iter()
        .filter_map(|(_, e)| s.world().get::<RoadSegment>(e).cloned())
        .collect()
}

fn bridge_seg(s: &Scenario) -> RoadSegment {
    segs(s)
        .into_iter()
        .find(|g| !g.spans().is_empty())
        .expect("a bridge")
}

fn terrain_h(s: &Scenario, p: Vec2) -> f32 {
    s.world().resource::<TerrainRes>().terrain().height_at(p)
}

#[test]
fn river_bridge_spans_water_with_clearance_and_piers() {
    let mut s = valley(12.0, 160.0, Some(3.0));
    let cost = build(
        &mut s,
        RoadKind::Street,
        Vec2::new(-150.0, 0.0),
        Vec2::new(150.0, 0.0),
    )
    .unwrap();
    let b = bridge_seg(&s);
    assert_eq!(b.spans().len(), 1, "{:?}", b.spans());
    let (s0, s1) = b.spans()[0];
    assert!(s1 - s0 >= MIN_SPAN_M);
    // The span covers the river and the deck clears the water by 4 m under its underside.
    let deck = bridge::params(RoadKind::Street).deck_m;
    let water = PLAIN_M - 12.0 + 3.0;
    let mid = b.geom().project(Vec2::ZERO).0;
    assert!(s0 < mid && mid < s1);
    assert!(b.profile().height_at(mid) - deck >= water + WATER_CLEARANCE_M - 0.05);
    assert!(!b.piers().is_empty());
    for p in b.piers() {
        assert!(p.height_m() > 0.0 && p.s_m > s0 && p.s_m < s1);
    }
    // Decks and piers cost extra.
    let cat = s.world().resource::<cb_sim::catalog::Catalog>();
    assert!(cost.0 > road_cost(cat, RoadKind::Street, b.geom().length()).0 * 2);
}

#[test]
fn bridge_abutment_step_le_2cm() {
    let mut s = valley(14.0, 140.0, None);
    build(
        &mut s,
        RoadKind::Street,
        Vec2::new(-150.0, 0.0),
        Vec2::new(150.0, 0.0),
    )
    .unwrap();
    s.run_ticks(0);
    let b = bridge_seg(&s);
    let deck = bridge::params(RoadKind::Street).deck_m;
    for &(s0, s1) in b.spans() {
        for se in [s0, s1] {
            // One profile for road and deck: no step at the span end.
            let step = (b.profile().height_at(se - 0.05) - b.profile().height_at(se + 0.05)).abs();
            assert!(step <= 0.02, "step {step} at {se}");
        }
        // No terrain through the deck: under the span the ground stays below the underside
        // (beyond the abutment wall, which the terrain grid smears over one 4 m cell).
        let mut s_ = s0 + 4.5;
        while s_ <= s1 - 4.5 {
            let p = b.geom().point_at(s_);
            let under = b.profile().height_at(s_) - deck;
            assert!(
                terrain_h(&s, p) <= under + 0.02,
                "terrain {} above the deck underside {under} at {s_} (span {s0}..{s1})",
                terrain_h(&s, p)
            );
            s_ += 2.0;
        }
    }
}

#[test]
fn bridge_cost_monotonic_in_length() {
    let mut last = Money(0);
    for width in [80.0, 110.0, 140.0, 170.0] {
        let mut s = valley(20.0, width, None);
        let c = build(
            &mut s,
            RoadKind::Street,
            Vec2::new(-200.0, 0.0),
            Vec2::new(200.0, 0.0),
        )
        .unwrap();
        let b = bridge_seg(&s);
        let span: f32 = b.spans().iter().map(|(a, b)| b - a).sum();
        eprintln!("valley {width} m: span {span:.0} m, cost {}", c.0);
        assert!(c.0 > last.0, "cost not increasing at width {width}");
        last = c;
    }
}

/// A street on the valley floor along z and a street across the valley at the plain height.
fn floor_and_cross(
    s: &mut Scenario,
    floor_first: bool,
) -> (SegmentId, Result<Money, ValidationError>) {
    let floor = |s: &mut Scenario| {
        build(
            s,
            RoadKind::Street,
            Vec2::new(0.0, -150.0),
            Vec2::new(0.0, 150.0),
        )
    };
    let cross = |s: &mut Scenario| {
        build(
            s,
            RoadKind::Street,
            Vec2::new(-150.0, 0.0),
            Vec2::new(150.0, 0.0),
        )
    };
    let r = if floor_first {
        floor(s).unwrap();
        cross(s)
    } else {
        cross(s).unwrap();
        floor(s)
    };
    let fl = segs(s)
        .into_iter()
        .find(|g| g.spans().is_empty() && g.curve().start().x.abs() < 1.0)
        .unwrap()
        .id();
    (fl, r)
}

#[test]
fn bridge_piers_outside_corridors() {
    for floor_first in [true, false] {
        let mut s = valley(14.0, 160.0, None);
        let (floor, r) = floor_and_cross(&mut s, floor_first);
        assert!(r.is_ok(), "{r:?}");
        let fl = segs(&s).into_iter().find(|g| g.id() == floor).unwrap();
        let b = bridge_seg(&s);
        assert!(!b.piers().is_empty());
        for p in b.piers() {
            let (_, d) = fl.geom().project(p.at);
            assert!(
                d.abs() > fl.geom().half_width_m + 1.0,
                "pier {d:.1} m from the floor street (floor first: {floor_first})"
            );
        }
        // The deck clears the street below.
        let deck = bridge::params(RoadKind::Street).deck_m;
        let at = b.geom().project(Vec2::ZERO).0;
        let under = b.profile().height_at(at) - deck;
        assert!(
            under - fl.profile().height_at(fl.geom().project(Vec2::ZERO).0)
                >= UNDER_DECK_CLEARANCE_M
        );
    }
}

#[test]
fn overpass_does_not_modify_lower_corridor() {
    let mut s = valley(14.0, 160.0, None);
    build(
        &mut s,
        RoadKind::Street,
        Vec2::new(0.0, -150.0),
        Vec2::new(0.0, 150.0),
    )
    .unwrap();
    s.run_ticks(0);
    let probe: Vec<Vec2> = (-40..=40)
        .flat_map(|z| [-7.0f32, -3.0, 0.0, 3.0, 7.0].map(|x| Vec2::new(x, z as f32)))
        .collect();
    let before: Vec<f32> = probe.iter().map(|p| terrain_h(&s, *p)).collect();
    build(
        &mut s,
        RoadKind::Street,
        Vec2::new(-150.0, 0.0),
        Vec2::new(150.0, 0.0),
    )
    .unwrap();
    s.run_ticks(0);
    let after: Vec<f32> = probe.iter().map(|p| terrain_h(&s, *p)).collect();
    assert_eq!(
        before, after,
        "the overpass changed terrain under the lower road"
    );
}

#[test]
fn no_junction_on_a_bridge() {
    let mut s = valley(14.0, 160.0, None);
    build(
        &mut s,
        RoadKind::Street,
        Vec2::new(-150.0, 0.0),
        Vec2::new(150.0, 0.0),
    )
    .unwrap();
    let b = bridge_seg(&s);
    let mid = b.geom().project(Vec2::new(-10.0, 0.0)).0;
    let p = b.geom().point_at(mid);
    let r = s.command(PlayerCommand::BuildRoad(BuildRoad {
        kind: RoadKind::Street,
        curve: CubicBezier2::straight(p, p + Vec2::new(0.0, 80.0)),
        start: Endpoint::Segment(b.id()),
        end: Endpoint::Free,
    }));
    assert_eq!(
        r.outcome.unwrap_err().key,
        "road-invalid-junction-on-bridge"
    );
    // The preview reports the same.
    let pv = preview_road(
        s.world(),
        &BuildRoad {
            kind: RoadKind::Street,
            curve: CubicBezier2::straight(p, p + Vec2::new(0.0, 80.0)),
            start: Endpoint::Segment(b.id()),
            end: Endpoint::Free,
        },
    );
    assert_eq!(pv.unwrap_err().key, "road-invalid-junction-on-bridge");
}
