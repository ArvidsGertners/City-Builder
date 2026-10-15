//! Headless simulation for City Builder (ARCHITECTURE §4–5, §10).
//!
//! Depends on `bevy_ecs`/`bevy_app` only — never on rendering, windowing, audio or UI crates
//! (checked by `scripts/check_sim_isolation.sh`). Runs in the game and in plain
//! `MinimalPlugins` apps (tests, benchmarks).

pub mod clock;
pub mod config;
pub mod driver;
pub mod hash;
pub mod schedule;
pub mod testing;

use bevy::app::{App, Plugin, PluginGroup, PluginGroupBuilder, Update};
use bevy::ecs::schedule::IntoScheduleConfigs;

pub use clock::{DayElapsed, HourElapsed, SeasonChanged, SimClock};
pub use config::{Difficulty, SimConfig, WeatherMode, WorldSeed};
pub use driver::{DayLength, SimDriveSystems, SimDriver, SimPacing, SimSpeed, SimStepRequest};
pub use hash::state_hash;
pub use schedule::{EditSet, SimEdit, SimTick, TickSet};

/// All simulation plugins.
pub struct SimPlugins {
    /// World seed for a new city.
    pub seed: u64,
}

impl Default for SimPlugins {
    fn default() -> Self {
        Self { seed: 1 }
    }
}

impl PluginGroup for SimPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>().add(SimCorePlugin { seed: self.seed })
    }
}

/// Schedules, driver, clock and configuration.
pub struct SimCorePlugin {
    /// World seed for a new city.
    pub seed: u64,
}

impl Plugin for SimCorePlugin {
    fn build(&self, app: &mut App) {
        schedule::add_sim_schedules(app);
        app.insert_resource(WorldSeed(self.seed))
            .init_resource::<SimConfig>()
            .init_resource::<SimClock>()
            .init_resource::<SimSpeed>()
            .init_resource::<SimPacing>()
            .init_resource::<SimStepRequest>()
            .init_resource::<SimDriver>()
            .add_message::<HourElapsed>()
            .add_message::<DayElapsed>()
            .add_message::<SeasonChanged>()
            .register_type::<SimClock>()
            .register_type::<SimConfig>()
            .register_type::<WorldSeed>()
            .register_type::<SimSpeed>()
            .register_type::<SimPacing>()
            .add_systems(SimTick, clock::advance_clock.in_set(TickSet::Clock))
            .add_systems(Update, driver::drive_sim.in_set(SimDriveSystems));
    }
}
