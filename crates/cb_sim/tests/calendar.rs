//! Calendar owning tests (CONSTANTS: `SIM_SECONDS_PER_GAME_MINUTE`, season length): the clock's
//! hour/day/season messages land on the right ticks in a headless run.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use bevy::ecs::message::{Message, MessageCursor, Messages};
use cb_core::time::{Season, TICKS_PER_GAME_DAY, TICKS_PER_GAME_HOUR};
use cb_sim::clock::{DayElapsed, HourElapsed, SeasonChanged};
use cb_sim::testing::Scenario;

/// Runs `ticks` ticks one at a time and collects every message of type `M` with its tick.
fn collect<M: Message + Clone>(s: &mut Scenario, ticks: u64) -> Vec<(u64, M)> {
    let mut cursor = MessageCursor::<M>::default();
    let mut out = Vec::new();
    for _ in 0..ticks {
        s.run_ticks(1);
        let t = s.ticks();
        let msgs = s.world().resource::<Messages<M>>();
        out.extend(cursor.read(msgs).cloned().map(|m| (t, m)));
    }
    out
}

#[test]
fn calendar_day_is_28800_ticks() {
    assert_eq!(TICKS_PER_GAME_DAY, 28_800);
    let mut s = Scenario::new(1);
    let days = collect::<DayElapsed>(&mut s, 2 * TICKS_PER_GAME_DAY);
    assert_eq!(
        days.iter().map(|(t, d)| (*t, d.day)).collect::<Vec<_>>(),
        vec![(28_800, 1), (57_600, 2)]
    );
    let mut s = Scenario::new(1);
    let hours = collect::<HourElapsed>(&mut s, TICKS_PER_GAME_DAY);
    assert_eq!(hours.len(), 24);
    for (k, (t, h)) in hours.iter().enumerate() {
        assert_eq!(*t, (k as u64 + 1) * TICKS_PER_GAME_HOUR);
        assert_eq!(h.hour, (k as u32 + 1) % 24);
    }
}

#[test]
fn season_changes_every_4_days() {
    let mut s = Scenario::new(2);
    let seasons = collect::<SeasonChanged>(&mut s, 16 * TICKS_PER_GAME_DAY);
    let got: Vec<(u64, Season, u64)> = seasons
        .iter()
        .map(|(t, m)| (*t / TICKS_PER_GAME_DAY, m.season, m.year))
        .collect();
    assert_eq!(
        got,
        vec![
            (4, Season::Summer, 1),
            (8, Season::Autumn, 1),
            (12, Season::Winter, 1),
            (16, Season::Spring, 2),
        ]
    );
}
