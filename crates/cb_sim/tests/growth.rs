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
