//! Economy (ALGORITHMS §21, §27): the ledger accounts for every change of the balance, an
//! emergency loan follows 7 days below zero, and goods are conserved (TESTING §3).

// Helpers outside `#[test]` fns assert by panicking, like the tests themselves.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use cb_core::time::{TICKS_PER_GAME_DAY, TICKS_PER_GAME_HOUR};
use cb_geometry::CubicBezier2;
use cb_network::RoadKind;
use cb_sim::catalog::ZoneKind;
use cb_sim::citygen::{self, GridSpec};
use cb_sim::commands::{BuildRoad, Endpoint, PlayerCommand};
use cb_sim::economy::goods::Goods;
use cb_sim::economy::{Account, Finances, Ledger, Treasury};
use cb_sim::testing::Scenario;
use glam::Vec2;

fn town(seed: u64) -> Scenario {
    let mut s = Scenario::new(seed);
    citygen::grid(
        s.world_mut(),
        &GridSpec {
            blocks: (4, 3),
            mixed: true,
            ..GridSpec::default()
        },
    );
    s
}

fn balance(s: &Scenario) -> i64 {
    s.world().resource::<Treasury>().balance().0
}

#[test]
fn ledger_sum_equals_money_delta() {
    let mut s = town(21);
    let start = balance(&s);
    let ledger_start = s.world().resource::<Ledger>().total_net();
    s.run_ticks(TICKS_PER_GAME_DAY + 5 * TICKS_PER_GAME_HOUR);
    let r = s.command(PlayerCommand::BuildRoad(BuildRoad {
        kind: RoadKind::Street,
        curve: CubicBezier2::straight(Vec2::new(900.0, 900.0), Vec2::new(1100.0, 900.0)),
        start: Endpoint::Free,
        end: Endpoint::Free,
    }));
    assert!(r.outcome.is_ok(), "{:?}", r.outcome);
    let r = s.command(PlayerCommand::Undo);
    assert!(r.outcome.is_ok(), "{:?}", r.outcome);
    s.run_ticks(TICKS_PER_GAME_DAY + 7 * TICKS_PER_GAME_HOUR);
    let l = s.world().resource::<Ledger>();
    assert_eq!(balance(&s) - start, l.total_net() - ledger_start);
    assert!(
        l.total(Account::TaxResidential) > 0,
        "household taxes accrue"
    );
    assert!(l.total(Account::RoadUpkeep) < 0, "roads cost upkeep");
    assert!(l.total(Account::Construction) < 0 && l.total(Account::Refunds) > 0);
    assert_eq!(l.history().count(), 2, "one statement per closed day");
    let st = l.history().last().unwrap();
    assert_eq!(st.day, 1);
    assert!(st.by_account.iter().any(|x| *x != 0));
}

#[test]
fn bankruptcy_after_7_days_triggers_loan() {
    let mut s = Scenario::new(4);
    // A road network with upkeep and no income.
    citygen::grid(
        s.world_mut(),
        &GridSpec {
            blocks: (3, 3),
            plot_depth_m: 0.0,
            ..GridSpec::default()
        },
    );
    s.set_balance(-100_000);
    for day in 1..=6 {
        s.run_ticks(TICKS_PER_GAME_DAY);
        let f = s.world().resource::<Finances>();
        assert_eq!(f.days_negative(), day, "day {day}");
        assert!(f.loans().is_empty());
    }
    let before = balance(&s);
    s.run_ticks(TICKS_PER_GAME_DAY);
    let f = s.world().resource::<Finances>();
    assert_eq!(f.loans().len(), 1, "emergency loan on day 7");
    assert_eq!(f.loans()[0].tier, 0);
    assert!(!f.bankrupt());
    assert!(balance(&s) > before + 2_000_000);
    assert!(s.world().resource::<Ledger>().total(Account::LoanProceeds) == 2_500_000);
    // Interest is charged from then on.
    s.run_ticks(TICKS_PER_GAME_DAY);
    assert!(s.world().resource::<Ledger>().total(Account::LoanInterest) < 0);
}

#[test]
fn bankrupt_without_loans_freezes_construction() {
    let mut s = Scenario::new(5);
    citygen::grid(
        s.world_mut(),
        &GridSpec {
            blocks: (2, 2),
            plot_depth_m: 0.0,
            ..GridSpec::default()
        },
    );
    for tier in 0..3 {
        let r = s.command(PlayerCommand::TakeLoan { tier });
        assert!(r.outcome.is_ok(), "{:?}", r.outcome);
    }
    let r = s.command(PlayerCommand::TakeLoan { tier: 1 });
    assert!(r.outcome.is_err());
    s.set_balance(-50_000_000);
    s.run_ticks(7 * TICKS_PER_GAME_DAY);
    assert!(s.world().resource::<Finances>().bankrupt());
    let r = s.command(PlayerCommand::BuildRoad(BuildRoad {
        kind: RoadKind::Street,
        curve: CubicBezier2::straight(Vec2::new(900.0, 900.0), Vec2::new(1000.0, 900.0)),
        start: Endpoint::Free,
        end: Endpoint::Free,
    }));
    assert!(r.outcome.is_err(), "construction frozen");
    s.set_balance(1_000_000);
    s.run_ticks(TICKS_PER_GAME_DAY);
    assert!(
        !s.world().resource::<Finances>().bankrupt(),
        "recovers after a day ≥ 0"
    );
}

#[test]
fn tax_rate_clamped_and_applied() {
    let mut s = town(8);
    let r = s.command(PlayerCommand::SetTaxRate {
        zone: ZoneKind::Residential,
        bp: 9000,
    });
    assert!(r.outcome.is_ok());
    assert_eq!(
        s.world()
            .resource::<Finances>()
            .tax_bp(ZoneKind::Residential),
        2500
    );
}

#[test]
fn goods_units_conserved() {
    let mut s = town(9);
    for _ in 0..3 {
        s.run_ticks(TICKS_PER_GAME_DAY);
        let g = s.world().resource::<Goods>();
        let st = g.stats();
        assert!(st.produced > 0 && st.consumed > 0, "{st:?}");
        assert_eq!(
            st.produced + st.imported,
            st.consumed + st.exported + st.lost + g.stocked_mu() + g.in_transit_mu(),
            "{st:?} stocked {} transit {}",
            g.stocked_mu(),
            g.in_transit_mu()
        );
    }
}
