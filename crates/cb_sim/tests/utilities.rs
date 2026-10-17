//! Utilities (ADR-0026, ALGORITHMS §18): pumps need deep water close behind their parcel,
//! networks are road components (a disconnected street gets nothing), and sewage outlets pollute
//! the river downstream only (TESTING §3).

// Helpers outside `#[test]` fns assert by panicking, like the tests themselves.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use cb_core::time::TICKS_PER_GAME_HOUR;
use cb_geometry::CubicBezier2;
use cb_network::RoadKind;
use cb_procgen::building::civic::CivicKind;
use cb_sim::buildings::{Building, BuildingKind, Condition};
use cb_sim::catalog::ZoneKind;
use cb_sim::commands::{
    BuildRoad, CreatePlot, DebugCommand, Endpoint, FrontageSpan, PlayerCommand,
};
use cb_sim::ids::Ids;
use cb_sim::roads::RoadSegment;
use cb_sim::terrain::TerrainSetup;
use cb_sim::testing::Scenario;
use cb_sim::utilities::Utilities;
use glam::Vec2;

fn ok(s: &mut Scenario, cmd: PlayerCommand) {
    let r = s.command(cmd);
    assert!(r.outcome.is_ok(), "{:?}", r.outcome);
}

fn road(s: &mut Scenario, a: Vec2, b: Vec2) {
    ok(
        s,
        PlayerCommand::BuildRoad(BuildRoad {
            kind: RoadKind::Street,
            curve: CubicBezier2::straight(a, b),
            start: Endpoint::Free,
            end: Endpoint::Free,
        }),
    );
}

/// The segment nearest to `p`.
fn segment_near(s: &Scenario, p: Vec2) -> (cb_core::ids::SegmentId, f32) {
    let ids = s.world().resource::<Ids>();
    ids.segments
        .iter()
        .map(|(id, e)| {
            let g = s.world().get::<RoadSegment>(e).unwrap();
            (
                g.geom().centerline.project(p).1.abs(),
                id,
                g.geom().half_width_m,
            )
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id, hw)| (id, hw))
        .unwrap()
}

fn residential_plot(s: &mut Scenario, near: Vec2, left: bool) {
    let (sid, _) = segment_near(s, near);
    ok(
        s,
        PlayerCommand::CreatePlot(CreatePlot {
            zone: ZoneKind::Residential,
            frontage: vec![FrontageSpan {
                segment: sid,
                left,
                s0: 20.0,
                s1: 180.0,
            }],
            depth_m: 30.0,
            dn: 0,
        }),
    );
}

fn civic(s: &Scenario, kind: CivicKind) -> cb_core::ids::BuildingId {
    let ids = s.world().resource::<Ids>();
    ids.buildings
        .iter()
        .find(|(_, e)| {
            s.world()
                .get::<Building>(*e)
                .is_some_and(|b| b.kind() == BuildingKind::Civic(kind))
        })
        .map(|(id, _)| id)
        .unwrap()
}

fn activate(s: &mut Scenario, id: cb_core::ids::BuildingId) {
    ok(
        s,
        PlayerCommand::Debug(DebugCommand::SetCondition {
            building: id,
            condition: Condition::Active,
        }),
    );
}

fn homes(s: &Scenario) -> Vec<(cb_core::ids::BuildingId, Vec2)> {
    let ids = s.world().resource::<Ids>();
    ids.buildings
        .iter()
        .filter_map(|(id, e)| {
            let b = s.world().get::<Building>(e)?;
            (b.kind() == BuildingKind::Zoned(ZoneKind::Residential)
                && b.condition() == Condition::Active)
                .then(|| (id, cb_geometry::polygon::centroid(&b.outlines()[0])))
        })
        .collect()
}

/// Runs to just after the next utility balance (tick 120 of an hour).
fn next_balance(s: &mut Scenario) {
    let t = s.ticks() % TICKS_PER_GAME_HOUR;
    let to = if t < 121 {
        121 - t
    } else {
        TICKS_PER_GAME_HOUR - t + 121
    };
    s.run_ticks(to);
}

#[test]
fn disconnected_component_unserved() {
    let mut s = Scenario::new(31);
    road(&mut s, Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0));
    road(&mut s, Vec2::new(-100.0, 600.0), Vec2::new(100.0, 600.0));
    residential_plot(&mut s, Vec2::new(0.0, 0.0), true);
    residential_plot(&mut s, Vec2::new(0.0, 600.0), true);
    let (_, hw) = segment_near(&s, Vec2::ZERO);
    ok(
        &mut s,
        PlayerCommand::PlaceBuilding {
            kind: CivicKind::DieselGenerator,
            pos: Vec2::new(0.0, -hw - 0.5),
        },
    );
    let gen_id = civic(&s, CivicKind::DieselGenerator);
    activate(&mut s, gen_id);
    s.run_ticks(10 * TICKS_PER_GAME_HOUR);
    next_balance(&mut s);
    let h = homes(&s);
    assert!(h.iter().filter(|(_, p)| p.y < 300.0).count() >= 3, "{h:?}");
    assert!(h.iter().filter(|(_, p)| p.y > 300.0).count() >= 3, "{h:?}");
    let u = s.world().resource::<Utilities>();
    for (id, p) in h {
        let st = u.status(id);
        assert!(st.connected, "{id} has road access");
        assert_eq!(st.power, p.y < 300.0, "{id} at {p}: {st:?}");
        assert!(!st.water, "no pump anywhere");
    }
    assert!(u.status(gen_id).power, "the generator powers itself");
}

/// A narrow valley with a river 3 m deep along z: water ≥ 1.5 m deep only within 7.5 m of the
/// centreline, dry plain beyond 30 m.
fn river_valley() -> Scenario {
    Scenario::with_terrain(
        2,
        TerrainSetup::Valley {
            samples: 257,
            spacing_m: 4.0,
            h: 20.0,
            depth_m: 8.0,
            width_m: 60.0,
            water_m: Some(3.0),
        },
    )
}

#[test]
fn pump_requires_depth_1_5m_within_15m() {
    for (x, expect_ok) in [(40.0, true), (60.0, false)] {
        let mut s = river_valley();
        road(&mut s, Vec2::new(x, -150.0), Vec2::new(x, 150.0));
        let (_, hw) = segment_near(&s, Vec2::new(x, 0.0));
        let r = s.command(PlayerCommand::PlaceBuilding {
            kind: CivicKind::WaterPump,
            pos: Vec2::new(x - hw - 0.5, 0.0),
        });
        if expect_ok {
            assert!(r.outcome.is_ok(), "road at x={x}: {:?}", r.outcome);
        } else {
            assert_eq!(
                r.outcome.unwrap_err().key,
                "building-invalid-shore",
                "road at x={x}"
            );
        }
    }
}

#[test]
fn sewage_pollutes_the_river_downstream_only() {
    let mut s = river_valley();
    road(&mut s, Vec2::new(40.0, -300.0), Vec2::new(40.0, 300.0));
    residential_plot(&mut s, Vec2::new(40.0, 0.0), false);
    let (_, hw) = segment_near(&s, Vec2::new(40.0, 0.0));
    ok(
        &mut s,
        PlayerCommand::PlaceBuilding {
            kind: CivicKind::SewageOutlet,
            pos: Vec2::new(40.0 - hw - 0.5, -200.0),
        },
    );
    let outlet = civic(&s, CivicKind::SewageOutlet);
    activate(&mut s, outlet);
    s.run_ticks(10 * TICKS_PER_GAME_HOUR);
    next_balance(&mut s);
    let u = s.world().resource::<Utilities>();
    assert!(
        homes(&s).iter().all(|(id, _)| u.status(*id).sewage),
        "houses drain to the outlet"
    );
    let river = &u.river_pollution()[0];
    // River points every 16 m from z = −512: the outlet sits near z = −200.
    let at = |z: f32| river[((z + 512.0) / 1024.0 * (river.len() - 1) as f32).round() as usize];
    assert_eq!(at(-300.0), 0.0, "clean upstream");
    assert!(at(-150.0) > 0.0 && at(300.0) > 0.0, "polluted downstream");
    assert!(at(300.0) < at(-150.0), "decays downstream");
}
