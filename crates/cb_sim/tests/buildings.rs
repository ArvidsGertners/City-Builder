//! Buildings in M4 (GAME_DESIGN §4, DATA_MODEL §5): Debug growth on every zone with the lot-area
//! level caps, construction that finishes after its time, and the debug commands that change a
//! building's level and condition (TESTING §3).

use cb_geometry::CubicBezier2;
use cb_network::RoadKind;
use cb_sim::buildings::{Building, BuildingChanged, BuildingKind, Condition};
use cb_sim::catalog::{Catalog, ZoneKind};
use cb_sim::commands::{
    BuildRoad, CreatePlot, DebugCommand, Endpoint, FrontageSpan, PlayerCommand,
};
use cb_sim::ids::Ids;
use cb_sim::roads::RoadSegment;
use cb_sim::testing::Scenario;
use cb_sim::zoning::Lot;
use glam::Vec2;

fn ok(s: &mut Scenario, cmd: PlayerCommand) {
    let r = s.command(cmd);
    assert!(r.outcome.is_ok(), "{:?}", r.outcome);
}

/// A long avenue with a plot of `zone` on each side (depth by zone).
fn zoned_street(s: &mut Scenario, zones: [ZoneKind; 2]) {
    ok(
        s,
        PlayerCommand::BuildRoad(BuildRoad {
            kind: RoadKind::Avenue,
            curve: CubicBezier2::straight(Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)),
            start: Endpoint::Free,
            end: Endpoint::Free,
        }),
    );
    let ids = s.world().resource::<Ids>();
    let sid = ids.segments.iter().next().map(|(id, _)| id).unwrap();
    let _ = s.world().get::<RoadSegment>(ids.segments.get(sid).unwrap());
    for (zone, left) in [(zones[0], true), (zones[1], false)] {
        let depth_m = match zone {
            ZoneKind::Residential => 40.0,
            ZoneKind::Commercial => 40.0,
            ZoneKind::Industrial => 70.0,
        };
        ok(
            s,
            PlayerCommand::CreatePlot(CreatePlot {
                zone,
                frontage: vec![FrontageSpan {
                    segment: sid,
                    left,
                    s0: 20.0,
                    s1: 380.0,
                }],
                depth_m,
                dn: 0,
            }),
        );
    }
}

fn buildings(s: &Scenario) -> Vec<Building> {
    let ids = s.world().resource::<Ids>();
    ids.buildings
        .iter()
        .filter_map(|(_, e)| s.world().get::<Building>(e).cloned())
        .collect()
}

fn lot(s: &Scenario, b: &Building) -> Lot {
    let ids = s.world().resource::<Ids>();
    let e = ids.lots.get(b.lot().unwrap()).unwrap();
    s.world().get::<Lot>(e).cloned().unwrap()
}

#[test]
fn debug_growth_fills_every_zone_within_level_caps() {
    let mut s = Scenario::new(3);
    zoned_street(&mut s, [ZoneKind::Residential, ZoneKind::Commercial]);
    let mut t = Scenario::new(3);
    zoned_street(&mut t, [ZoneKind::Industrial, ZoneKind::Residential]);
    for sc in [&mut s, &mut t] {
        sc.run_ticks(800);
    }
    let cat = Catalog::load().unwrap();
    let mut by_zone = std::collections::BTreeMap::new();
    for sc in [&s, &t] {
        for b in buildings(sc) {
            let BuildingKind::Zoned(zone) = b.kind() else {
                panic!("only zoned buildings grow")
            };
            let l = lot(sc, &b);
            let cap = cat.zone(zone).level_cap(l.area_m2());
            assert!(
                b.level() >= 1 && b.level() <= cap,
                "{zone:?} L{} on {} m² (cap {cap})",
                b.level(),
                l.area_m2()
            );
            for c in b.outlines().iter().flatten() {
                assert!(
                    cb_geometry::polygon::contains(l.polygon(), *c)
                        || cb_geometry::polygon::boundary_distance(l.polygon(), *c) < 0.02,
                    "{zone:?} outline leaves its lot"
                );
            }
            *by_zone.entry(zone).or_insert(0usize) += 1;
        }
    }
    for z in [
        ZoneKind::Residential,
        ZoneKind::Commercial,
        ZoneKind::Industrial,
    ] {
        assert!(
            by_zone.get(&z).copied().unwrap_or(0) >= 3,
            "{z:?}: {by_zone:?}"
        );
    }
}

#[test]
fn construction_finishes_after_its_time() {
    let mut s = Scenario::new(5);
    zoned_street(&mut s, [ZoneKind::Residential, ZoneKind::Residential]);
    s.run_ticks(800);
    let bs = buildings(&s);
    assert!(!bs.is_empty());
    let mut longest = 0;
    for b in &bs {
        let Condition::Constructing {
            start_tick,
            duration_ticks,
        } = b.condition()
        else {
            panic!(
                "a fresh building is under construction: {:?}",
                b.condition()
            );
        };
        assert!(duration_ticks > 0);
        longest = longest.max(start_tick + duration_ticks);
        assert!(b.condition().progress(start_tick) < 0.01);
        assert!((b.condition().progress(start_tick + duration_ticks) - 1.0).abs() < 1e-6);
    }
    let now = s.ticks();
    s.run_ticks(longest - now + 40);
    for b in buildings(&s)
        .iter()
        .filter(|b| bs.iter().any(|o| o.id() == b.id()))
    {
        assert_eq!(b.condition(), Condition::Active, "{:?}", b.id());
    }
}

#[test]
fn debug_commands_change_level_and_condition() {
    let mut s = Scenario::new(7);
    zoned_street(&mut s, [ZoneKind::Residential, ZoneKind::Commercial]);
    s.run_ticks(800);
    let cat = Catalog::load().unwrap();
    // A residential building on a lot that allows L4+.
    let b = buildings(&s)
        .into_iter()
        .find(|b| {
            b.kind() == BuildingKind::Zoned(ZoneKind::Residential)
                && cat
                    .zone(ZoneKind::Residential)
                    .level_cap(lot(&s, b).area_m2())
                    >= 4
        })
        .expect("a lot of 400 m² or more");
    ok(
        &mut s,
        PlayerCommand::Debug(DebugCommand::SetBuildingLevel {
            building: b.id(),
            level: 4,
        }),
    );
    let after = buildings(&s)
        .into_iter()
        .find(|x| x.id() == b.id())
        .unwrap();
    assert_eq!(after.level(), 4, "same id, new level");
    assert_eq!(after.condition(), Condition::Active);
    assert!(after.height_m() > b.height_m());
    // Condition changes report `Condition` (no remesh), except into and out of ruins.
    let read = |s: &mut Scenario| -> Vec<BuildingChanged> {
        let msgs = s
            .world()
            .resource::<bevy::ecs::message::Messages<BuildingChanged>>();
        let mut c = msgs.get_cursor();
        c.read(msgs).copied().collect()
    };
    for (cond, built) in [
        (Condition::Declining, false),
        (Condition::Abandoned, false),
        (Condition::Ruined, true),
        (Condition::Active, true),
    ] {
        ok(
            &mut s,
            PlayerCommand::Debug(DebugCommand::SetCondition {
                building: b.id(),
                condition: cond,
            }),
        );
        let msgs = read(&mut s);
        let want = if built {
            BuildingChanged::Built(b.id())
        } else {
            BuildingChanged::Condition(b.id())
        };
        assert_eq!(msgs.last(), Some(&want), "{cond:?}: {msgs:?}");
        let now = buildings(&s)
            .into_iter()
            .find(|x| x.id() == b.id())
            .unwrap();
        assert_eq!(now.condition(), cond);
    }
    // Commercial buildings cap at their lot's level.
    let c = buildings(&s)
        .into_iter()
        .find(|b| b.kind() == BuildingKind::Zoned(ZoneKind::Commercial))
        .unwrap();
    let cap = cat
        .zone(ZoneKind::Commercial)
        .level_cap(lot(&s, &c).area_m2());
    ok(
        &mut s,
        PlayerCommand::Debug(DebugCommand::SetBuildingLevel {
            building: c.id(),
            level: 9,
        }),
    );
    let c2 = buildings(&s)
        .into_iter()
        .find(|x| x.id() == c.id())
        .unwrap();
    assert!(c2.level() <= cap);
}

fn street(s: &mut Scenario) {
    ok(
        s,
        PlayerCommand::BuildRoad(BuildRoad {
            kind: RoadKind::Street,
            curve: CubicBezier2::straight(Vec2::new(-150.0, 0.0), Vec2::new(150.0, 0.0)),
            start: Endpoint::Free,
            end: Endpoint::Free,
        }),
    );
}

fn road_half_width(s: &Scenario) -> f32 {
    let ids = s.world().resource::<Ids>();
    let (_, e) = ids.segments.iter().next().unwrap();
    s.world().get::<RoadSegment>(e).unwrap().geom().half_width_m
}

#[test]
fn place_building_snaps_to_road_edge_within_2m() {
    use cb_procgen::building::civic::CivicKind;
    let mut s = Scenario::new(9);
    street(&mut s);
    let hw = road_half_width(&s);
    let before = s.world().resource::<cb_sim::economy::Treasury>().balance();
    // 1.6 m off the left road edge (y > 0), slightly rotated intent: the front lands on the edge.
    ok(
        &mut s,
        PlayerCommand::PlaceBuilding {
            kind: CivicKind::PoliceStation,
            pos: Vec2::new(-40.0, hw + 1.6),
        },
    );
    let b = buildings(&s).pop().unwrap();
    assert_eq!(b.kind(), BuildingKind::Civic(CivicKind::PoliceStation));
    let cb_sim::buildings::Site::Civic(fr) = b.site().clone() else {
        panic!("civic site")
    };
    let (w, _) = CivicKind::PoliceStation.parcel_m();
    for p in [fr.at(0.0, 0.0), fr.at(w, 0.0)] {
        assert!(
            (p.y - hw).abs() < 0.05,
            "front corner {p} on the road edge y = {hw}"
        );
    }
    assert!(fr.at(0.0, 5.0).y > hw, "parcel lies away from the road");
    let spent = before.0
        - s.world()
            .resource::<cb_sim::economy::Treasury>()
            .balance()
            .0;
    assert_eq!(spent, 900_000);
    assert!(matches!(b.condition(), Condition::Constructing { .. }));
    // The right side works too; beyond 2 m it is rejected.
    ok(
        &mut s,
        PlayerCommand::PlaceBuilding {
            kind: CivicKind::Clinic,
            pos: Vec2::new(40.0, -hw - 0.5),
        },
    );
    let r = s.command(PlayerCommand::PlaceBuilding {
        kind: CivicKind::Clinic,
        pos: Vec2::new(100.0, hw + 2.6),
    });
    assert_eq!(r.outcome.unwrap_err().key, "building-invalid-no-road");
    // Undo removes it in one step and refunds.
    ok(&mut s, PlayerCommand::Undo);
    assert_eq!(buildings(&s).len(), 1);
}

#[test]
fn place_building_rejects_overlap() {
    use cb_procgen::building::civic::CivicKind;
    let mut s = Scenario::new(9);
    street(&mut s);
    let hw = road_half_width(&s);
    ok(
        &mut s,
        PlayerCommand::PlaceBuilding {
            kind: CivicKind::FireStation,
            pos: Vec2::new(0.0, hw),
        },
    );
    // Overlapping parcel (30 m wide fire station, 26 m police: 10 m apart).
    let r = s.command(PlayerCommand::PlaceBuilding {
        kind: CivicKind::PoliceStation,
        pos: Vec2::new(10.0, hw),
    });
    assert_eq!(r.outcome.unwrap_err().key, "building-invalid-overlap");
    // Adjacent is fine.
    ok(
        &mut s,
        PlayerCommand::PlaceBuilding {
            kind: CivicKind::PoliceStation,
            pos: Vec2::new(28.5, hw),
        },
    );
    // A plot on the same frontage blocks it as well.
    let sid = s
        .world()
        .resource::<Ids>()
        .segments
        .iter()
        .next()
        .map(|(id, _)| id)
        .unwrap();
    ok(
        &mut s,
        PlayerCommand::CreatePlot(CreatePlot {
            zone: ZoneKind::Residential,
            frontage: vec![FrontageSpan {
                segment: sid,
                left: false,
                s0: 10.0,
                s1: 90.0,
            }],
            depth_m: 30.0,
            dn: 0,
        }),
    );
    let r = s.command(PlayerCommand::PlaceBuilding {
        kind: CivicKind::Clinic,
        pos: Vec2::new(-100.0, -hw),
    });
    assert_eq!(r.outcome.unwrap_err().key, "building-invalid-overlap");
    // Shoreline kinds need water (none on flat test ground).
    let r = s.command(PlayerCommand::PlaceBuilding {
        kind: CivicKind::WaterPump,
        pos: Vec2::new(120.0, hw),
    });
    assert_eq!(r.outcome.unwrap_err().key, "building-invalid-shore");
}

#[test]
fn place_prop_lv_effect() {
    use cb_sim::props::{DecorKind, land_value_bonus_at, spec};
    let mut s = Scenario::new(4);
    street(&mut s);
    let p = Vec2::new(0.0, 30.0);
    assert_eq!(land_value_bonus_at(s.world(), p), 0.0);
    ok(
        &mut s,
        PlayerCommand::PlaceProp {
            kind: DecorKind::Fountain,
            pos: p,
            yaw: 0.0,
        },
    );
    let f = spec(DecorKind::Fountain);
    assert!((land_value_bonus_at(s.world(), p) - f.lv_bonus).abs() < 1e-6);
    let half = land_value_bonus_at(s.world(), p + Vec2::new(0.5 * f.radius_m, 0.0));
    assert!((half - 0.5 * f.lv_bonus).abs() < 1e-4, "{half}");
    assert_eq!(
        land_value_bonus_at(s.world(), p + Vec2::new(f.radius_m + 1.0, 0.0)),
        0.0
    );
    // Two trees add up; props on the road are rejected; undo removes the last one.
    for x in [-2.0, 2.0] {
        ok(
            &mut s,
            PlayerCommand::PlaceProp {
                kind: DecorKind::Tree,
                pos: Vec2::new(x, 80.0),
                yaw: 0.3,
            },
        );
    }
    let t = land_value_bonus_at(s.world(), Vec2::new(0.0, 80.0));
    assert!(t > spec(DecorKind::Tree).lv_bonus * 1.8, "{t}");
    let r = s.command(PlayerCommand::PlaceProp {
        kind: DecorKind::Bench,
        pos: Vec2::new(0.0, 0.0),
        yaw: 0.0,
    });
    assert_eq!(r.outcome.unwrap_err().key, "prop-invalid-blocked");
    ok(&mut s, PlayerCommand::Undo);
    let t2 = land_value_bonus_at(s.world(), Vec2::new(0.0, 80.0));
    assert!(t2 < t && t2 > 0.0);
}

#[test]
fn town_hall_auto_placed_at_start() {
    use cb_procgen::building::civic::CivicKind;
    let mut s = Scenario::new(2);
    let spec = cb_sim::citygen::GridSpec {
        blocks: (2, 2),
        grow: false,
        ..Default::default()
    };
    let rep = cb_sim::citygen::grid(s.world_mut(), &spec);
    assert_eq!(rep.civic, 1, "{rep:?}");
    let halls: Vec<Building> = buildings(&s)
        .into_iter()
        .filter(|b| b.kind() == BuildingKind::Civic(CivicKind::TownHall))
        .collect();
    assert_eq!(halls.len(), 1);
    assert_eq!(halls[0].condition(), Condition::Active, "built at start");
    let spent_on_hall = cb_sim::catalog::Catalog::load()
        .unwrap()
        .civic(CivicKind::TownHall)
        .cost_cents;
    assert_eq!(spent_on_hall, 0, "the town hall is free");
    // Plots were clipped around its parcel.
    let parcel = halls[0].parcel().unwrap();
    let ids = s.world().resource::<Ids>();
    for (_, e) in ids.plots.iter() {
        let p = s.world().get::<cb_sim::zoning::Plot>(e).unwrap();
        let ov: f32 = cb_geometry::boolean::intersect(&parcel, p.polygon())
            .outers
            .iter()
            .map(|o| cb_geometry::polygon::area(o))
            .sum();
        assert!(ov < 1.0, "plot overlaps the town hall parcel: {ov} m²");
    }
    // Only one, ever.
    assert!(cb_sim::buildings::place_town_hall(s.world_mut()).is_none());
}
