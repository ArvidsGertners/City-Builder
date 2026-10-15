//! Sim-affecting options, saved with the city and changed via `SetSimConfig` (ARCHITECTURE §14).
//! Presentation options (day length, lock time of day) live in the game's `Settings` instead.

use bevy::ecs::reflect::ReflectResource;
use bevy::ecs::resource::Resource;
use bevy::reflect::Reflect;
use serde::{Deserialize, Serialize};

/// The world seed every deterministic stream derives from (ADR-0043).
#[derive(Resource, Reflect, Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct WorldSeed(pub u64);

/// Weather simulation mode.
#[derive(Reflect, Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
pub enum WeatherMode {
    /// Always clear; seasons still tint visuals.
    Off,
    /// Markov weather (M12).
    #[default]
    Dynamic,
}

/// Difficulty preset (GAME_DESIGN §1).
#[derive(Reflect, Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
#[allow(missing_docs)]
pub enum Difficulty {
    Relaxed,
    #[default]
    Normal,
    Hard,
}

/// Options that change simulation results. Field defaults apply to missing fields in saves.
#[derive(Resource, Reflect, Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[reflect(Resource)]
#[serde(default)]
pub struct SimConfig {
    /// Simulate individual vehicles (otherwise trips are resolved by the flow model only).
    pub traffic_agents: bool,
    /// Maximum simultaneous traffic agents (ADR-0047).
    pub agent_cap: u32,
    /// Weather mode.
    pub weather: WeatherMode,
    /// Game days per season (GAME_DESIGN §2).
    pub season_length_days: u32,
    /// Unlimited money, everything unlocked.
    pub sandbox: bool,
    /// Difficulty preset.
    pub difficulty: Difficulty,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            traffic_agents: true,
            agent_cap: 5_000,
            weather: WeatherMode::Dynamic,
            season_length_days: 4,
            sandbox: false,
            difficulty: Difficulty::Normal,
        }
    }
}
