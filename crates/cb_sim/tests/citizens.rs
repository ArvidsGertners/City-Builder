//! Citizens (ALGORITHMS §16): households move into free homes, adults find jobs within the
//! workplaces' capacities, homes that stop being habitable empty, and the whole process is
//! deterministic (TESTING §3).

// Helpers outside `#[test]` fns assert by panicking, like the tests themselves.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use cb_core::time::TICKS_PER_GAME_HOUR;
use cb_sim::buildings::{Building, BuildingKind, Condition};
use cb_sim::catalog::ZoneKind;
use cb_sim::citizens::{Capacities, Population, PopulationStats};
use cb_sim::citygen::{self, GridSpec};
use cb_sim::commands::{DebugCommand, PlayerCommand};
use cb_sim::demand::Demand;
use cb_sim::ids::Ids;
use cb_sim::testing::Scenario;

/// A small mixed town (commercial core, industrial band, residential around it).
fn town(seed: u64) -> Scenario {
    let mut s = Scenario::new(seed);
    let rep = citygen::grid(
        s.world_mut(),
        &GridSpec {
            blocks: (4, 3),
            mixed: true,
            ..GridSpec::default()
        },
    );
    assert!(rep.buildings > 50, "{rep:?}");
    s
}

fn check_capacities(s: &Scenario) {
    let pop = s.world().resource::<Population>();
    let cap = s.world().resource::<Capacities>();
    let ids = s.world().resource::<Ids>();
    for (hid, h) in pop.households() {
        let e = ids.buildings.get(h.home()).expect("home exists");
        let b = s.world().get::<Building>(e).unwrap();
        assert_eq!(
            b.kind(),
            BuildingKind::Zoned(ZoneKind::Residential),
            "{hid} lives in a home"
        );
    }
    for (b, slots) in cap.homes() {
        assert!(
            pop.residents_of(b).len() <= usize::from(slots),
            "{b} over capacity"
        );
    }
    for (b, jobs) in cap.jobs() {
        let f = pop.workers_of(b);
        for k in 0..3 {
            assert!(f[k] <= jobs[k], "{b} level {k}: {} > {}", f[k], jobs[k]);
        }
    }
    for (_, c) in pop.citizens() {
        if let Some(w) = c.job() {
            assert!(c.in_labour_force());
            assert!(cap.jobs_of(w) != [0; 3], "works at an open workplace");
        }
    }
}

#[test]
fn households_move_in_and_find_jobs() {
    let mut s = town(3);
    s.run_ticks(12 * TICKS_PER_GAME_HOUR);
    let stats = *s.world().resource::<PopulationStats>();
    let d = *s.world().resource::<Demand>();
    assert!(stats.population > 50, "{stats:?} {d:?}");
    assert!(stats.households > 20, "{stats:?}");
    assert!(stats.jobs > 0 && stats.jobs_filled > 0, "{stats:?}");
    assert!(stats.unemployed < stats.labour_force, "{stats:?}");
    check_capacities(&s);
}

#[test]
fn citizens_deterministic() {
    let run = || {
        let mut s = town(5);
        s.run_ticks(6 * TICKS_PER_GAME_HOUR);
        let pop = s.world().resource::<Population>();
        (
            format!("{:?}", pop.households().collect::<Vec<_>>()),
            format!("{:?}", pop.citizens().collect::<Vec<_>>()),
            s.state_hash(),
        )
    };
    let a = run();
    let b = run();
    assert_eq!(a, b);
    assert!(a.0.len() > 10);
}

#[test]
fn abandoned_home_empties_and_closed_workplace_lays_off() {
    let mut s = town(7);
    s.run_ticks(8 * TICKS_PER_GAME_HOUR);
    let (home, work) = {
        let pop = s.world().resource::<Population>();
        let home = pop.households().next().map(|(_, h)| h.home()).unwrap();
        let work = pop.citizens().find_map(|(_, c)| c.job()).unwrap();
        (home, work)
    };
    for b in [home, work] {
        let r = s.command(PlayerCommand::Debug(DebugCommand::SetCondition {
            building: b,
            condition: Condition::Abandoned,
        }));
        assert!(r.outcome.is_ok(), "{:?}", r.outcome);
    }
    s.run_ticks(TICKS_PER_GAME_HOUR);
    let pop = s.world().resource::<Population>();
    assert!(pop.residents_of(home).is_empty());
    assert_eq!(pop.workers_of(work), [0; 3]);
    assert!(pop.citizens().all(|(_, c)| c.job() != Some(work)));
    check_capacities(&s);
}

/// Prints the hourly trajectory of a town (diagnostics, `--ignored --nocapture`).
#[test]
#[ignore = "diagnostics"]
fn print_town_trajectory() {
    let mut s = town(3);
    for hour in 0..48 {
        s.run_ticks(TICKS_PER_GAME_HOUR);
        let st = *s.world().resource::<PopulationStats>();
        let d = *s.world().resource::<Demand>();
        println!(
            "h{hour:02} pop {} hh {} homes {}/{} jobs {}/{} lf {} unemp {} R {:.2} Rimm {:.2} C {:.2} I {:.2}",
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
