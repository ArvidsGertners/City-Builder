//! Demand-driven growth (ALGORITHMS §15–16): in `GrowthMode::Normal` empty zoned lots develop at
//! the rate demand allows, households move in and adults find jobs (TESTING §3).

// Helpers outside `#[test]` fns assert by panicking, like the tests themselves.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use cb_core::time::{TICKS_PER_GAME_DAY, TICKS_PER_GAME_HOUR};
use cb_sim::buildings::{Building, Condition};
use cb_sim::catalog::ZoneKind;
use cb_sim::citizens::PopulationStats;
use cb_sim::citygen::{self, GridSpec};
use cb_sim::demand::Demand;
use cb_sim::ids::Ids;
use cb_sim::testing::Scenario;
use cb_sim::{GrowthMode, SimConfig};

/// An empty mixed town (plots, no buildings) growing on demand.
fn empty_town(seed: u64) -> Scenario {
    let mut s = Scenario::new(seed).config(SimConfig {
        growth: GrowthMode::Normal,
        ..SimConfig::default()
    });
    let rep = citygen::grid(
        s.world_mut(),
        &GridSpec {
            blocks: (4, 3),
            mixed: true,
            grow: false,
            ..GridSpec::default()
        },
    );
    assert!(rep.plots > 20 && rep.buildings == 0, "{rep:?}");
    s
}

fn built(s: &Scenario) -> usize {
    let ids = s.world().resource::<Ids>();
    ids.buildings
        .iter()
        .filter(|(_, e)| {
            s.world()
                .get::<Building>(*e)
                .is_some_and(|b| b.condition() == Condition::Active && b.lot().is_some())
        })
        .count()
}

#[test]
fn normal_growth_develops_lots_and_fills_them() {
    let mut s = empty_town(11);
    s.run_ticks(3 * TICKS_PER_GAME_DAY);
    let st = *s.world().resource::<PopulationStats>();
    let n = built(&s);
    assert!(n > 60, "built {n}, {st:?}");
    assert!(st.population >= 300, "{st:?}");
    let unemployment = st.unemployed as f32 / st.labour_force.max(1) as f32;
    assert!(unemployment <= 0.15, "unemployment {unemployment} {st:?}");
}

/// Prints the daily trajectory (diagnostics, `--ignored --nocapture`).
#[test]
#[ignore = "diagnostics"]
fn print_growth_trajectory() {
    let mut s = empty_town(11);
    for hour in 0..96 {
        s.run_ticks(TICKS_PER_GAME_HOUR);
        if hour % 6 != 5 {
            continue;
        }
        let st = *s.world().resource::<PopulationStats>();
        let d = *s.world().resource::<Demand>();
        println!(
            "h{hour:02} built {} pop {} hh {} homes {}/{} jobs {}/{} lf {} unemp {} R {:.2} Rimm {:.2} C {:.2} I {:.2}",
            built(&s),
            st.population,
            st.households,
            st.home_slots - st.free_homes,
            st.home_slots,
            st.jobs_filled,
            st.jobs,
            st.labour_force,
            st.unemployed,
            d.zone(ZoneKind::Residential),
            d.immigration(),
            d.zone(ZoneKind::Commercial),
            d.zone(ZoneKind::Industrial),
        );
    }
}

/// A riverside town: a flat plain beside a river valley, an empty mixed grid whose west street
/// runs along the river, a water pump on the bank and a diesel generator on the east edge.
fn riverside_town(seed: u64) -> (Scenario, usize) {
    use cb_procgen::building::civic::CivicKind;
    use cb_sim::buildings::BuildingKind;
    use cb_sim::commands::{DebugCommand, PlayerCommand};
    use cb_sim::roads::RoadSegment;
    use cb_sim::zoning::Lot;
    let mut s = Scenario::with_terrain(
        seed,
        cb_sim::TerrainSetup::Valley {
            samples: 513,
            spacing_m: 4.0,
            h: 20.0,
            depth_m: 8.0,
            width_m: 60.0,
            water_m: Some(3.0),
        },
    )
    .config(SimConfig {
        growth: GrowthMode::Normal,
        ..SimConfig::default()
    });
    let spec = GridSpec {
        center: glam::Vec2::new(315.0, 0.0),
        blocks: (5, 4),
        mixed: true,
        grow: false,
        ..GridSpec::default()
    };
    let rep = citygen::grid(s.world_mut(), &spec);
    assert!(rep.buildings == 0 && rep.plots > 20, "{rep:?}");
    let hw = {
        let ids = s.world().resource::<Ids>();
        let (_, e) = ids.segments.iter().next().unwrap();
        s.world().get::<RoadSegment>(e).unwrap().geom().half_width_m
    };
    for (kind, pos) in [
        (CivicKind::WaterPump, glam::Vec2::new(40.0 - hw - 0.5, 0.0)),
        (
            CivicKind::DieselGenerator,
            glam::Vec2::new(590.0 + hw + 0.5, 0.0),
        ),
    ] {
        let r = s.command(PlayerCommand::PlaceBuilding { kind, pos });
        assert!(r.outcome.is_ok(), "{kind:?}: {:?}", r.outcome);
        let id = {
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
        };
        let r = s.command(PlayerCommand::Debug(DebugCommand::SetCondition {
            building: id,
            condition: Condition::Active,
        }));
        assert!(r.outcome.is_ok(), "{:?}", r.outcome);
    }
    let r_lots = {
        let ids = s.world().resource::<Ids>();
        ids.lots
            .iter()
            .filter(|(_, e)| {
                s.world().get::<Lot>(*e).is_some_and(|l| {
                    ids.plots.get(l.plot()).is_some_and(|p| {
                        s.world()
                            .get::<cb_sim::zoning::Plot>(p)
                            .is_some_and(|p| p.zone() == ZoneKind::Residential)
                    })
                })
            })
            .count()
    };
    (s, r_lots)
}

#[test]
fn growth_scenario_flat_3days() {
    let (mut s, r_lots) = riverside_town(17);
    assert!(r_lots >= 120, "{r_lots} residential lots");
    s.run_ticks(3 * TICKS_PER_GAME_DAY);
    let st = *s.world().resource::<PopulationStats>();
    assert!(st.population >= 400, "{st:?}");
    let unemployment = st.unemployed as f32 / st.labour_force.max(1) as f32;
    assert!(unemployment <= 0.15, "unemployment {unemployment} {st:?}");
    // Homes are powered and watered.
    let u = s.world().resource::<cb_sim::utilities::Utilities>();
    let pop = s.world().resource::<cb_sim::citizens::Population>();
    let (mut served, mut homes) = (0, 0);
    for (_, h) in pop.households() {
        homes += 1;
        let st = u.status(h.home());
        served += usize::from(st.power && st.water);
    }
    assert!(
        served * 10 >= homes * 9,
        "{served} of {homes} households served"
    );
    // A morning rush: the peak lies between 05:30 and 08:00 (bins 11–15) and most commuters
    // leave between 05:00 and 09:30. (Commerce starts at 08:45, ADR-0062; in a town this small
    // commutes take minutes, so the peak sits later than the 05:30–07:30 of city-scale commutes.)
    let c = *s.world().resource::<cb_sim::traffic::Traffic>().stats();
    let peak = (0..c.departures.len())
        .max_by_key(|b| (c.departures[*b], std::cmp::Reverse(*b)))
        .unwrap();
    assert!(
        (11..=15).contains(&peak),
        "peak bin {peak}: {:?}",
        c.departures
    );
    let morning: u32 = c.departures[10..19].iter().sum();
    assert!(
        morning * 10 >= c.commuters * 7,
        "{morning} of {} leave 05:00–09:30",
        c.commuters
    );
}
