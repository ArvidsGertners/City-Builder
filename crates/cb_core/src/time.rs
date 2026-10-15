//! Time model (ADR-0042).
//!
//! The tick count is the master clock. One tick is `0.1` sim seconds; one game minute is
//! `2` sim seconds (20 ticks). The real-time pace (day length, speed) is decided by the driver
//! and never affects simulation results.

use serde::{Deserialize, Serialize};

/// Sim seconds per tick.
pub const TICK_DT_SIM_S: f32 = 0.1;
/// Ticks per sim second (exact integer form of [`TICK_DT_SIM_S`]).
pub const TICKS_PER_SIM_SECOND: u64 = 10;
/// Sim seconds per game minute (fixed).
pub const SIM_SECONDS_PER_GAME_MINUTE: f32 = 2.0;
/// Ticks per game minute.
pub const TICKS_PER_GAME_MINUTE: u64 = 20;
/// Ticks per game hour.
pub const TICKS_PER_GAME_HOUR: u64 = TICKS_PER_GAME_MINUTE * 60;
/// Ticks per game day.
pub const TICKS_PER_GAME_DAY: u64 = TICKS_PER_GAME_HOUR * 24;
/// Game milliseconds per tick (3 game seconds).
pub const GAME_MS_PER_TICK: i64 = 3_000;
/// Perceived minutes per sim second (UI, happiness, demand).
pub const PERCEIVED_MIN_PER_SIM_S: f32 = 0.1;

/// Simulation tick index.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Tick(pub u64);

impl Tick {
    /// Sim seconds elapsed since tick 0.
    #[must_use]
    pub fn sim_seconds(self) -> f64 {
        self.0 as f64 / TICKS_PER_SIM_SECOND as f64
    }

    /// Game time since the start of the game.
    #[must_use]
    pub fn game_time(self) -> GameTime {
        GameTime(
            i64::try_from(self.0)
                .unwrap_or(i64::MAX / GAME_MS_PER_TICK)
                .saturating_mul(GAME_MS_PER_TICK),
        )
    }
}

/// Game time in game milliseconds since the start of the game.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct GameTime(pub i64);

impl GameTime {
    /// Whole game minutes.
    #[must_use]
    pub fn minutes(self) -> i64 {
        self.0 / 60_000
    }
}

/// Season of the year.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[allow(missing_docs)]
pub enum Season {
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl Season {
    /// Season for a zero-based index modulo 4.
    #[must_use]
    pub fn from_index(i: u64) -> Self {
        match i % 4 {
            0 => Season::Spring,
            1 => Season::Summer,
            2 => Season::Autumn,
            _ => Season::Winter,
        }
    }
}

/// Calendar view of a tick (year and day are 1-based for display).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Calendar {
    /// Year, starting at 1.
    pub year: u64,
    /// Season.
    pub season: Season,
    /// Day within the season, starting at 1.
    pub day_of_season: u64,
    /// Hour 0–23.
    pub hour: u32,
    /// Minute 0–59.
    pub minute: u32,
}

impl Calendar {
    /// Calendar for `tick` with seasons of `season_length_days` game days (min 1).
    #[must_use]
    pub fn from_tick(tick: Tick, season_length_days: u64) -> Self {
        let season_len = season_length_days.max(1);
        let day = tick.0 / TICKS_PER_GAME_DAY;
        let in_day = tick.0 % TICKS_PER_GAME_DAY;
        let season_index = day / season_len;
        Self {
            year: season_index / 4 + 1,
            season: Season::from_index(season_index),
            day_of_season: day % season_len + 1,
            hour: (in_day / TICKS_PER_GAME_HOUR) as u32,
            minute: ((in_day % TICKS_PER_GAME_HOUR) / TICKS_PER_GAME_MINUTE) as u32,
        }
    }
}

/// Perceived minutes for a duration in sim seconds.
#[inline]
#[must_use]
pub fn perceived_minutes(sim_seconds: f32) -> f32 {
    sim_seconds * PERCEIVED_MIN_PER_SIM_S
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gametime_from_tick() {
        assert_eq!(TICKS_PER_GAME_DAY, 28_800);
        assert_eq!(Tick(20).game_time().minutes(), 1);
        assert_eq!(Tick(TICKS_PER_GAME_DAY).game_time().minutes(), 24 * 60);
        let c = Calendar::from_tick(Tick(TICKS_PER_GAME_HOUR), 4);
        assert_eq!(
            (c.year, c.season, c.day_of_season, c.hour, c.minute),
            (1, Season::Spring, 1, 1, 0)
        );
        let c = Calendar::from_tick(
            Tick(TICKS_PER_GAME_DAY * 4 + TICKS_PER_GAME_HOUR * 13 + 20 * 7),
            4,
        );
        assert_eq!(
            (c.season, c.day_of_season, c.hour, c.minute),
            (Season::Summer, 1, 13, 7)
        );
        let c = Calendar::from_tick(Tick(TICKS_PER_GAME_DAY * 16), 4);
        assert_eq!((c.year, c.season), (2, Season::Spring));
    }

    #[test]
    fn sim_seconds_and_perception() {
        assert!((Tick(1200).sim_seconds() - 120.0).abs() < 1e-9);
        assert!((perceived_minutes(200.0) - 20.0).abs() < 1e-6);
    }
}
