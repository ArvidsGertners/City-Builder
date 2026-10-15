//! Driver and schedule integration tests (PROGRESS M1).

use std::time::Instant;

use bevy::ecs::message::Messages;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::ResMut;
use cb_core::time::{TICKS_PER_GAME_DAY, TICKS_PER_GAME_HOUR};
use cb_sim::schedule::set_debug_order;
use cb_sim::testing::Scenario;
use cb_sim::{
    DayLength, HourElapsed, SimClock, SimPacing, SimSpeed, SimStepRequest, SimTick, TickSet,
};

const FRAME: f64 = 1.0 / 60.0;

#[test]
fn driver_runs_1000_ticks_in_lt_100ms() {
    // Wall-clock bound on schedule overhead (ADR-0061: ≈ 1 µs per empty tick). Best of three
    // runs, so that CPU contention from tests running in parallel does not count.
    let mut best = std::time::Duration::MAX;
    for _ in 0..3 {
        let mut s = Scenario::new(7);
        s.run_ticks(1); // warm-up: schedule initialization
        let start = Instant::now();
        s.run_ticks(1000);
        best = best.min(start.elapsed());
        assert_eq!(s.ticks(), 1001);
    }
    println!("1000 ticks: {best:?}");
    assert!(best.as_millis() < 100, "1000 ticks took {best:?}");
}

#[test]
fn driver_step_once_advances_exactly_one_tick() {
    let mut s = Scenario::new(7);
    s.app.insert_resource(SimSpeed::Paused);
    for _ in 0..10 {
        s.frame(FRAME);
    }
    assert_eq!(s.ticks(), 0);
    s.app.insert_resource(SimStepRequest(1));
    s.frame(FRAME);
    assert_eq!(s.ticks(), 1);
    for _ in 0..10 {
        s.frame(FRAME);
    }
    assert_eq!(s.ticks(), 1);
    // Steps requested while running are discarded.
    s.app.insert_resource(SimSpeed::X1);
    s.app.insert_resource(SimStepRequest(5));
    s.frame(FRAME);
    assert_eq!(s.app.world().resource::<SimStepRequest>().0, 0);
}

#[test]
fn driver_caps_ticks_per_frame() {
    let mut s = Scenario::new(7);
    s.app.insert_resource(SimSpeed::X4);
    let mut total = 0;
    for _ in 0..600 {
        let plan = s.frame(FRAME);
        assert!(
            plan.ticks <= 2,
            "4× at L = 24 ran {} ticks in one frame",
            plan.ticks
        );
        total += u64::from(plan.ticks);
    }
    // 10 real seconds at 80 ticks/s; deferral may shift at most one tick.
    assert!((799..=800).contains(&total), "total {total}");
    assert_eq!(s.ticks(), total);

    let mut s = Scenario::new(7);
    s.app.insert_resource(SimSpeed::X1);
    for _ in 0..60 {
        assert!(s.frame(FRAME).ticks <= 1);
    }
    // Menu hold behaves like pause.
    s.app.insert_resource(SimPacing {
        hold: true,
        ..SimPacing::default()
    });
    let before = s.ticks();
    for _ in 0..60 {
        s.frame(FRAME);
    }
    assert_eq!(s.ticks(), before);
}

/// Runs the driver at 4× until `target` ticks, finishing exactly with single steps.
fn run_paced(day_length: DayLength, target: u64) -> u64 {
    let mut s = Scenario::new(42);
    s.app.insert_resource(SimPacing {
        day_length,
        hold: false,
    });
    s.app.insert_resource(SimSpeed::X4);
    while s.ticks() + 2 < target {
        s.frame(FRAME);
    }
    s.app.insert_resource(SimSpeed::Paused);
    while s.ticks() < target {
        s.app.insert_resource(SimStepRequest(1));
        s.frame(FRAME);
    }
    assert_eq!(s.ticks(), target);
    s.state_hash()
}

#[test]
fn driver_day_length_is_pacing_only() {
    let target = 2 * TICKS_PER_GAME_HOUR + 7;
    let hashes: Vec<u64> = [DayLength::Min16, DayLength::Min24, DayLength::Min48]
        .into_iter()
        .map(|l| run_paced(l, target))
        .collect();
    assert!(hashes.windows(2).all(|w| w[0] == w[1]), "{hashes:x?}");

    let mut direct = Scenario::new(42);
    direct.run_ticks(target);
    assert_eq!(direct.state_hash(), hashes[0]);
}

fn hour_messages(s: &Scenario) -> usize {
    s.app.world().resource::<Messages<HourElapsed>>().len()
}

#[test]
fn schedule_shuffle_same_hash() {
    let ticks = TICKS_PER_GAME_DAY + 3 * TICKS_PER_GAME_HOUR;
    let mut base = Scenario::new(9);
    set_debug_order(&mut base.app, None);
    base.run_ticks(ticks);
    let expected = (base.state_hash(), hour_messages(&base));
    assert!(expected.1 > 0);
    for seed in [1_u64, 2, 3] {
        let mut s = Scenario::new(9);
        set_debug_order(&mut s.app, Some(seed));
        s.run_ticks(ticks);
        assert_eq!(
            (s.state_hash(), hour_messages(&s)),
            expected,
            "shuffle seed {seed}"
        );
    }
}

#[test]
fn schedule_ambiguity_is_error() {
    fn bump_a(mut c: ResMut<SimClock>) {
        let t = c.tick().0;
        *c = SimClock::at(t);
    }
    fn bump_b(mut c: ResMut<SimClock>) {
        let t = c.tick().0;
        *c = SimClock::at(t);
    }
    let mut s = Scenario::new(1);
    s.app
        .add_systems(SimTick, (bump_a, bump_b).in_set(TickSet::Stats));
    let result = s
        .app
        .world_mut()
        .try_schedule_scope(SimTick, |world, sched| sched.initialize(world));
    assert!(
        matches!(result, Ok(Err(_))),
        "conflicting systems must fail the build"
    );
}
