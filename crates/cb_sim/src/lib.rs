//! Headless simulation for City Builder (ARCHITECTURE §4–5, §10).
//!
//! Depends on `bevy_ecs`/`bevy_app` only — never on rendering, windowing, audio or UI crates
//! (checked by `scripts/check_sim_isolation.sh`). Runs in the game and in plain
//! `MinimalPlugins` apps (tests, benchmarks).

pub mod buildings;
pub mod catalog;
pub mod citizens;
pub mod citygen;
pub mod clock;
pub mod commands;
pub mod config;
pub mod demand;
pub mod driver;
pub mod economy;
pub mod fields;
pub mod hash;
pub mod history;
pub mod ids;
pub mod lanes;
pub mod props;
pub mod roads;
pub mod save;
pub mod schedule;
pub mod terrain;
pub mod testing;
pub mod traffic;
pub mod utilities;
pub mod zoning;

use bevy::app::{App, Plugin, PluginGroup, PluginGroupBuilder, Update};
use bevy::ecs::schedule::IntoScheduleConfigs;

pub use clock::{DayElapsed, HourElapsed, SeasonChanged, SimClock};
pub use commands::{CommandQueue, CommandResult, PlayerCommand, ValidationError};
pub use config::{Difficulty, GrowthMode, SimConfig, WeatherMode, WorldSeed};
pub use driver::{DayLength, SimDriveSystems, SimDriver, SimPacing, SimSpeed, SimStepRequest};
pub use hash::state_hash;
pub use schedule::{EditSet, SimEdit, SimTick, TickSet};
pub use terrain::TerrainSetup;

/// All simulation plugins.
pub struct SimPlugins {
    /// World seed for a new city.
    pub seed: u64,
    /// Terrain source.
    pub terrain: TerrainSetup,
}

impl Default for SimPlugins {
    fn default() -> Self {
        Self {
            seed: 1,
            terrain: TerrainSetup::None,
        }
    }
}

impl PluginGroup for SimPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(SimCorePlugin { seed: self.seed })
            .add(WorldStatePlugin {
                seed: self.seed,
                terrain: self.terrain,
            })
    }
}

/// World state for M2: catalog, ids, treasury, terrain, commands, roads, zoning, buildings.
pub struct WorldStatePlugin {
    /// World seed.
    pub seed: u64,
    /// Terrain source.
    pub terrain: TerrainSetup,
}

impl Plugin for WorldStatePlugin {
    fn build(&self, app: &mut App) {
        match catalog::Catalog::load() {
            Ok(cat) => {
                let money = cat.start_money(Difficulty::Normal);
                app.insert_resource(cat)
                    .insert_resource(economy::Treasury::new(money));
            }
            Err(e) => tracing::error!("catalog failed to load: {e}"),
        }
        match terrain::build(&self.terrain, self.seed) {
            Ok(Some(t)) => {
                app.insert_resource(t);
            }
            Ok(None) => {}
            Err(e) => tracing::error!("terrain generation failed: {e}"),
        }
        app.init_resource::<ids::Ids>()
            .init_resource::<CommandQueue>()
            .init_resource::<commands::CommandLog>()
            .init_resource::<history::History>()
            .init_resource::<zoning::DirtyPlots>()
            .init_resource::<lanes::LaneNetwork>()
            .add_message::<CommandResult>()
            .add_message::<roads::NetworkChanged>()
            .add_message::<terrain::TerrainChanged>()
            .add_message::<zoning::ZoningChanged>()
            .add_message::<buildings::BuildingChanged>()
            .add_message::<props::PropChanged>()
            .add_systems(SimEdit, commands::apply_commands.in_set(EditSet::Commands))
            .add_systems(SimEdit, lanes::patch_lanes.in_set(EditSet::Lanes))
            .add_systems(SimEdit, zoning::update_dirty_plots.in_set(EditSet::Zoning))
            .add_systems(
                SimEdit,
                (traffic::mark_dirty, traffic::rebuild_if_dirty)
                    .chain()
                    .in_set(EditSet::Connectivity),
            )
            .init_resource::<traffic::Traffic>()
            .init_resource::<citizens::Population>()
            .init_resource::<citizens::Capacities>()
            .init_resource::<citizens::PopulationStats>()
            .init_resource::<citizens::jobs::JobMap>()
            .init_resource::<demand::Demand>()
            .init_resource::<economy::Ledger>()
            .init_resource::<economy::Finances>()
            .init_resource::<economy::goods::Goods>()
            .init_resource::<utilities::Utilities>()
            .init_resource::<fields::Fields>()
            .init_resource::<citizens::happiness::HappinessStats>()
            .add_systems(
                SimTick,
                (
                    buildings::growth,
                    economy::tick,
                    economy::goods::tick,
                    demand::tick,
                    utilities::tick,
                    traffic::tick,
                    fields::tick,
                    citizens::happiness::tick,
                    buildings::develop,
                    citizens::tick,
                )
                    .chain()
                    .in_set(TickSet::SubRate),
            );
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
