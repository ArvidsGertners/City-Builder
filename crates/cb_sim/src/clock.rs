//! The simulation clock: tick counter and calendar messages (ADR-0042).

use bevy::ecs::message::{Message, MessageWriter};
use bevy::ecs::reflect::ReflectResource;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Res, ResMut};
use bevy::reflect::Reflect;
use cb_core::time::{Calendar, Season, TICKS_PER_GAME_DAY, TICKS_PER_GAME_HOUR, Tick};

use crate::config::SimConfig;

/// Completed simulation ticks since the start of the city.
#[derive(Resource, Reflect, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[reflect(Resource)]
pub struct SimClock {
    ticks: u64,
}

impl SimClock {
    /// A clock that has completed `ticks` ticks (used by loading and tests).
    #[must_use]
    pub const fn at(ticks: u64) -> Self {
        Self { ticks }
    }

    /// Completed ticks.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        Tick(self.ticks)
    }

    /// Calendar view for the given season length.
    #[must_use]
    pub fn calendar(&self, season_length_days: u32) -> Calendar {
        Calendar::from_tick(self.tick(), u64::from(season_length_days))
    }
}

/// A full game hour has elapsed (sent on the tick that completes it).
#[derive(Message, Clone, Copy, PartialEq, Eq, Debug)]
pub struct HourElapsed {
    /// Tick that completed the hour.
    pub tick: Tick,
    /// Hour of day that just started (0–23).
    pub hour: u32,
}

/// A full game day has elapsed.
#[derive(Message, Clone, Copy, PartialEq, Eq, Debug)]
pub struct DayElapsed {
    /// Tick that completed the day.
    pub tick: Tick,
    /// Zero-based index of the day that just started.
    pub day: u64,
}

/// A new season started.
#[derive(Message, Clone, Copy, PartialEq, Eq, Debug)]
pub struct SeasonChanged {
    /// Tick that started the season.
    pub tick: Tick,
    /// The new season.
    pub season: Season,
    /// Year (1-based).
    pub year: u64,
}

/// `TickSet::Clock`: advances the tick and emits calendar messages.
pub(crate) fn advance_clock(
    mut clock: ResMut<SimClock>,
    config: Res<SimConfig>,
    mut hours: MessageWriter<HourElapsed>,
    mut days: MessageWriter<DayElapsed>,
    mut seasons: MessageWriter<SeasonChanged>,
) {
    clock.ticks += 1;
    let tick = clock.tick();
    if !tick.0.is_multiple_of(TICKS_PER_GAME_HOUR) {
        return;
    }
    let cal = clock.calendar(config.season_length_days);
    hours.write(HourElapsed {
        tick,
        hour: cal.hour,
    });
    if tick.0.is_multiple_of(TICKS_PER_GAME_DAY) {
        days.write(DayElapsed {
            tick,
            day: tick.0 / TICKS_PER_GAME_DAY,
        });
        if cal.day_of_season == 1 {
            seasons.write(SeasonChanged {
                tick,
                season: cal.season,
                year: cal.year,
            });
        }
    }
}
