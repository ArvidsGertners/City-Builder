//! Fixed-timestep driver (ADR-0042): once per frame it runs `SimEdit`, then `k` × `SimTick`
//! where `k` follows from real time, speed and the day-length pacing.
//!
//! Real time is read only here. It decides *when* ticks run, never what they compute, so the
//! simulation is identical for every frame rate, speed history and day length.

use std::time::Instant;

use bevy::ecs::reflect::ReflectResource;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::SystemSet;
use bevy::ecs::world::World;
use bevy::reflect::Reflect;
use bevy::time::{Real, Time};

use crate::schedule::{SimEdit, SimTick};

/// Wall-clock budget for `SimEdit` + ticks in one frame before one tick is deferred.
pub const SIM_FRAME_BUDGET_MS: f64 = 7.0;
/// Longest real frame time the driver accumulates (longer hitches are not caught up).
pub const MAX_REAL_DT_S: f64 = 0.25;

/// Simulation speed requested by the player (driver API, independent of menus).
#[derive(Resource, Reflect, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[reflect(Resource)]
pub enum SimSpeed {
    /// No ticks (`SimEdit` still runs; single steps possible).
    Paused,
    /// Normal speed.
    #[default]
    X1,
    /// Double speed.
    X2,
    /// Quadruple speed.
    X4,
}

impl SimSpeed {
    /// Speed multiplier.
    #[must_use]
    pub const fn factor(self) -> u32 {
        match self {
            SimSpeed::Paused => 0,
            SimSpeed::X1 => 1,
            SimSpeed::X2 => 2,
            SimSpeed::X4 => 4,
        }
    }
}

/// Real minutes per game day at 1× (pacing only; lives in `Settings`, not in the save).
#[derive(Reflect, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum DayLength {
    /// 16 minutes.
    Min16,
    /// 24 minutes (default).
    #[default]
    Min24,
    /// 32 minutes.
    Min32,
    /// 48 minutes.
    Min48,
}

impl DayLength {
    /// All options in menu order.
    pub const ALL: [DayLength; 4] = [
        DayLength::Min16,
        DayLength::Min24,
        DayLength::Min32,
        DayLength::Min48,
    ];

    /// Real minutes per game day.
    #[must_use]
    pub const fn minutes(self) -> u32 {
        match self {
            DayLength::Min16 => 16,
            DayLength::Min24 => 24,
            DayLength::Min32 => 32,
            DayLength::Min48 => 48,
        }
    }

    /// Option for a minute value, if it is one of the four.
    #[must_use]
    pub fn from_minutes(minutes: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.minutes() == minutes)
    }
}

/// Presentation-side pacing inputs.
#[derive(Resource, Reflect, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[reflect(Resource)]
pub struct SimPacing {
    /// Day length.
    pub day_length: DayLength,
    /// Treat the speed as paused (e.g. an in-game menu is open).
    pub hold: bool,
}

/// Pending single-step requests; honoured only while paused, one per frame.
#[derive(Resource, Reflect, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[reflect(Resource)]
pub struct SimStepRequest(pub u32);

/// Ticks per real second at `speed` and `day_length`: `480 / L × speed`.
#[must_use]
pub fn ticks_per_real_second(speed: SimSpeed, day_length: DayLength) -> f64 {
    480.0 / f64::from(day_length.minutes()) * f64::from(speed.factor())
}

/// Result of planning one frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FramePlan {
    /// Ticks to run this frame.
    pub ticks: u32,
    /// Whole ticks dropped because they exceeded the cap.
    pub dropped: u32,
    /// The cap that applied.
    pub cap: u32,
}

/// The pure part of the driver: an accumulator in units of ticks.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct TickPlanner {
    acc_ticks: f64,
}

impl TickPlanner {
    /// Plans a frame of `real_dt` seconds at `rate` ticks per real second. At most
    /// `max(1, ceil(1.5 × nominal))` ticks run; whole ticks beyond that are dropped.
    pub fn plan(&mut self, real_dt: f64, rate: f64) -> FramePlan {
        let dt = if real_dt.is_finite() {
            real_dt.clamp(0.0, MAX_REAL_DT_S)
        } else {
            0.0
        };
        let nominal = dt * rate.max(0.0);
        self.acc_ticks += nominal;
        let cap = (1.5 * nominal).ceil().max(1.0) as u32;
        // The epsilon absorbs rounding so that e.g. 3 × (1/3) yields a tick.
        let available = (self.acc_ticks + 1e-9).floor();
        let ticks = (available as u32).min(cap);
        self.acc_ticks = (self.acc_ticks - f64::from(ticks)).max(0.0);
        let mut dropped = 0;
        if self.acc_ticks + 1e-9 >= 1.0 {
            let whole = (self.acc_ticks + 1e-9).floor();
            dropped = whole as u32;
            self.acc_ticks = (self.acc_ticks - whole).max(0.0);
        }
        FramePlan {
            ticks,
            dropped,
            cap,
        }
    }

    /// Returns `n` planned ticks to the accumulator (deferral: they run next frame).
    pub fn give_back(&mut self, n: u32) {
        self.acc_ticks += f64::from(n);
    }

    /// Fraction of the next tick already accumulated, for pose interpolation.
    #[must_use]
    pub fn alpha(&self) -> f32 {
        self.acc_ticks.clamp(0.0, 1.0) as f32
    }
}

/// Driver counters for the perf overlay and benchmarks.
#[derive(Reflect, Clone, Copy, PartialEq, Debug, Default)]
pub struct DriverStats {
    /// Ticks run in the last frame.
    pub ticks_last_frame: u32,
    /// Ticks run in total.
    pub ticks_total: u64,
    /// Ticks dropped by the cap in total.
    pub dropped_total: u64,
    /// Ticks deferred to a later frame in total.
    pub deferred_total: u64,
    /// `SimEdit` wall time in the last frame (ms).
    pub edit_ms_last: f64,
    /// Mean wall time per tick in the last frame (ms; 0 if none ran).
    pub tick_ms_last: f64,
    /// Exponential moving average of the wall time per tick (ms).
    pub tick_ms_ema: f64,
}

/// Driver state.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct SimDriver {
    planner: TickPlanner,
    deferred_last_frame: bool,
    /// Counters.
    pub stats: DriverStats,
}

impl SimDriver {
    /// Interpolation factor between the last two ticks.
    #[must_use]
    pub fn alpha(&self) -> f32 {
        self.planner.alpha()
    }
}

/// The set containing the driver system in `Update`; the game orders it inside its
/// `GameSet::SimDrive`.
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SimDriveSystems;

/// Exclusive system: one driver frame using `Time<Real>`.
pub fn drive_sim(world: &mut World) {
    let real_dt = world
        .get_resource::<Time<Real>>()
        .map_or(0.0, Time::delta_secs_f64);
    drive_frame(world, real_dt);
}

/// One driver frame of `real_dt` real seconds: `SimEdit` once, then the planned ticks.
pub fn drive_frame(world: &mut World, real_dt: f64) -> FramePlan {
    let edit_start = Instant::now();
    world.run_schedule(SimEdit);
    let edit_ms = edit_start.elapsed().as_secs_f64() * 1000.0;

    let speed = world
        .get_resource::<SimSpeed>()
        .copied()
        .unwrap_or_default();
    let pacing = world
        .get_resource::<SimPacing>()
        .copied()
        .unwrap_or_default();
    let effective = if pacing.hold { SimSpeed::Paused } else { speed };
    let rate = ticks_per_real_second(effective, pacing.day_length);

    let step = match world.get_resource_mut::<SimStepRequest>() {
        Some(mut req) if req.0 > 0 => {
            // Steps only apply while paused; otherwise they are discarded.
            let take = effective == SimSpeed::Paused;
            req.0 = if take { req.0 - 1 } else { 0 };
            take
        }
        _ => false,
    };

    let mut driver = world
        .get_resource::<SimDriver>()
        .copied()
        .unwrap_or_default();
    let mut plan = driver.planner.plan(real_dt, rate);
    if step {
        plan.ticks += 1;
    }

    let projected = edit_ms + f64::from(plan.ticks) * driver.stats.tick_ms_ema;
    let defer =
        plan.ticks > 0 && !step && !driver.deferred_last_frame && projected > SIM_FRAME_BUDGET_MS;
    if defer {
        plan.ticks -= 1;
        driver.planner.give_back(1);
        driver.stats.deferred_total += 1;
    }
    driver.deferred_last_frame = defer;

    let ticks_start = Instant::now();
    for _ in 0..plan.ticks {
        world.run_schedule(SimTick);
    }
    let ticks_ms = ticks_start.elapsed().as_secs_f64() * 1000.0;

    let stats = &mut driver.stats;
    stats.ticks_last_frame = plan.ticks;
    stats.ticks_total += u64::from(plan.ticks);
    stats.dropped_total += u64::from(plan.dropped);
    stats.edit_ms_last = edit_ms;
    if plan.ticks > 0 {
        stats.tick_ms_last = ticks_ms / f64::from(plan.ticks);
        stats.tick_ms_ema = if stats.tick_ms_ema == 0.0 {
            stats.tick_ms_last
        } else {
            0.9 * stats.tick_ms_ema + 0.1 * stats.tick_ms_last
        };
    } else {
        stats.tick_ms_last = 0.0;
    }
    world.insert_resource(driver);
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: f64 = 1.0 / 60.0;

    #[test]
    fn driver_caps_ticks_per_frame() {
        // 1× at L = 24: 1/3 tick per frame, cap 1.
        let mut p = TickPlanner::default();
        let rate = ticks_per_real_second(SimSpeed::X1, DayLength::Min24);
        assert!((rate - 20.0).abs() < 1e-12);
        let plans: Vec<_> = (0..6).map(|_| p.plan(FRAME, rate)).collect();
        assert!(plans.iter().all(|f| f.cap == 1));
        assert_eq!(plans.iter().map(|f| f.ticks).sum::<u32>(), 2);

        // 4× at L = 24: 4/3 ticks per frame, cap 2.
        let mut p = TickPlanner::default();
        let rate = ticks_per_real_second(SimSpeed::X4, DayLength::Min24);
        let plans: Vec<_> = (0..60).map(|_| p.plan(FRAME, rate)).collect();
        assert!(
            plans
                .iter()
                .all(|f| f.cap == 2 && f.ticks <= 2 && f.dropped == 0)
        );
        assert_eq!(plans.iter().map(|f| f.ticks).sum::<u32>(), 80);

        // A hitch beyond MAX_REAL_DT_S is clamped; the excess over the cap is dropped.
        let mut p = TickPlanner::default();
        let f = p.plan(10.0, rate);
        assert_eq!(f.cap, 30); // ceil(1.5 × 0.25 s × 80/s)
        assert_eq!(f.ticks, 20);
        assert_eq!(f.dropped, 0);
        let mut p = TickPlanner::default();
        p.give_back(50);
        let f = p.plan(FRAME, rate);
        assert_eq!((f.ticks, f.dropped), (2, 49));
        assert!(p.alpha() < 1.0);
    }

    #[test]
    fn paused_plans_no_ticks() {
        let mut p = TickPlanner::default();
        for _ in 0..100 {
            assert_eq!(p.plan(FRAME, 0.0).ticks, 0);
        }
    }

    #[test]
    fn ticks_per_real_second_matches_adr() {
        assert!((ticks_per_real_second(SimSpeed::X1, DayLength::Min16) - 30.0).abs() < 1e-12);
        assert!((ticks_per_real_second(SimSpeed::X4, DayLength::Min16) - 120.0).abs() < 1e-12);
        assert!((ticks_per_real_second(SimSpeed::X2, DayLength::Min48) - 20.0).abs() < 1e-12);
        assert_eq!(DayLength::from_minutes(32), Some(DayLength::Min32));
        assert_eq!(DayLength::from_minutes(20), None);
    }
}
