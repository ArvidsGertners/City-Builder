//! `SetSimConfig` (ARCHITECTURE §14): sim options change only through the command, out-of-range
//! values are rejected with a localization key, and the change is not part of undo history.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use cb_sim::commands::PlayerCommand;
use cb_sim::testing::Scenario;
use cb_sim::{GrowthMode, SimConfig};

#[test]
fn set_sim_config_validates_and_applies() {
    let mut s = Scenario::new(1);
    let bad = SimConfig {
        season_length_days: 0,
        ..SimConfig::default()
    };
    let r = s.command(PlayerCommand::SetSimConfig(bad));
    assert_eq!(
        r.outcome.err().map(|e| e.key),
        Some("config-invalid".into())
    );
    assert_eq!(*s.world().resource::<SimConfig>(), SimConfig::default());
    let good = SimConfig {
        growth: GrowthMode::Normal,
        season_length_days: 7,
        sandbox: true,
        ..SimConfig::default()
    };
    let r = s.command(PlayerCommand::SetSimConfig(good.clone()));
    assert!(r.outcome.is_ok(), "{:?}", r.outcome);
    assert_eq!(*s.world().resource::<SimConfig>(), good);
    // Not undoable: undo has nothing to revert.
    let _ = s.command(PlayerCommand::Undo);
    assert_eq!(*s.world().resource::<SimConfig>(), good);
}
