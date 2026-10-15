//! The two sim schedules and their ordered system sets (ARCHITECTURE §5.1).
//!
//! Both schedules use `ambiguity_detection = Error` and no automatic `ApplyDeferred`
//! insertion: sets are chained and sync points are explicit (ADR-0043).

use bevy::app::App;
use bevy::ecs::schedule::{
    ApplyDeferred, IntoScheduleConfigs, LogLevel, Schedule, ScheduleBuildSettings, ScheduleLabel,
    SingleThreadedExecutor, SystemSet,
};

/// Runs every frame, also while paused: player commands and structural derived updates.
#[derive(ScheduleLabel, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SimEdit;

/// One fixed simulation step of [`cb_core::time::TICK_DT_SIM_S`].
#[derive(ScheduleLabel, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SimTick;

/// Ordered sets of [`SimEdit`].
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum EditSet {
    /// Drain the command queue: validate → apply → record undo and `apply_tick`.
    Commands,
    /// Junction regeneration, segment geometry, bridges, rings.
    Network,
    /// Terrain deformation stamps for dirty regions.
    Terrain,
    /// Lane and link graph patches.
    Lanes,
    /// Plot clipping, subdivision, lot access.
    Zoning,
    /// Components, SCCs, outside reachability.
    Connectivity,
    /// Coverage reach and stop access.
    Coverage,
    /// Utility network structure.
    Utilities,
    /// Spatial index updates and dirty-region broadcast.
    Index,
}

impl EditSet {
    /// All sets in execution order.
    pub const ORDER: [EditSet; 9] = [
        EditSet::Commands,
        EditSet::Network,
        EditSet::Terrain,
        EditSet::Lanes,
        EditSet::Zoning,
        EditSet::Connectivity,
        EditSet::Coverage,
        EditSet::Utilities,
        EditSet::Index,
    ];
}

/// Ordered sets of [`SimTick`].
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum TickSet {
    /// Advance the tick; emit hour/day/season messages.
    Clock,
    /// Weather state machine.
    Weather,
    /// Time wheel → trip requests.
    Schedule,
    /// Mode choice, routing, flow model, agent admission.
    Trips,
    /// Junction control, car following, arrivals.
    Agents,
    /// Phase-table sub-rate systems.
    SubRate,
    /// Milestones and unlocks.
    Progression,
    /// Statistics and notifications.
    Stats,
}

impl TickSet {
    /// All sets in execution order.
    pub const ORDER: [TickSet; 8] = [
        TickSet::Clock,
        TickSet::Weather,
        TickSet::Schedule,
        TickSet::Trips,
        TickSet::Agents,
        TickSet::SubRate,
        TickSet::Progression,
        TickSet::Stats,
    ];
}

/// Build settings shared by both sim schedules.
#[must_use]
pub fn build_settings() -> ScheduleBuildSettings {
    ScheduleBuildSettings {
        ambiguity_detection: LogLevel::Error,
        auto_insert_apply_deferred: false,
        ..ScheduleBuildSettings::default()
    }
}

/// Creates both schedules with chained sets and an explicit `ApplyDeferred` after each set.
pub(crate) fn add_sim_schedules(app: &mut App) {
    let mut edit = Schedule::new(SimEdit);
    edit.set_build_settings(build_settings());
    chain_sets(&mut edit, &EditSet::ORDER);
    app.add_schedule(edit);

    let mut tick = Schedule::new(SimTick);
    tick.set_build_settings(build_settings());
    chain_sets(&mut tick, &TickSet::ORDER);
    app.add_schedule(tick);
}

fn chain_sets<S: SystemSet + Copy>(schedule: &mut Schedule, order: &[S]) {
    // Sets run strictly in order, with one explicit sync point between consecutive sets.
    for pair in order.windows(2) {
        schedule.configure_sets(pair[0].before(pair[1]));
        schedule.add_systems(ApplyDeferred.after(pair[0]).before(pair[1]));
    }
}

/// Test/debug helper: reconfigure both schedules for single-threaded execution and, with the
/// bevy `debug` feature, a shuffled topological order.
pub fn set_debug_order(app: &mut App, shuffle_seed: Option<u64>) {
    for label in [SimEdit.intern(), SimTick.intern()] {
        app.edit_schedule(label, |s| {
            s.set_executor(SingleThreadedExecutor::new());
            #[allow(unused_mut)]
            let mut settings = build_settings();
            #[cfg(feature = "debug_schedule")]
            {
                settings.shuffle_seed = shuffle_seed;
            }
            #[cfg(not(feature = "debug_schedule"))]
            let _ = shuffle_seed;
            s.set_build_settings(settings);
        });
    }
}
