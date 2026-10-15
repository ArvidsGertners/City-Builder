//! Headless scenario harness (TESTING §3). Grows with each milestone; M1 offers the app, tick
//! control and the state hash.

use bevy::MinimalPlugins;
use bevy::app::App;

use crate::driver::drive_frame;
use crate::schedule::{SimEdit, SimTick};
use crate::{SimClock, SimConfig, SimPlugins, state_hash};

/// A headless sim app driven explicitly by the test.
pub struct Scenario {
    /// The app (public for tests that need direct world access).
    pub app: App,
}

impl Scenario {
    /// A new scenario with `MinimalPlugins` and `SimPlugins` for `seed`.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SimPlugins { seed }));
        app.finish();
        app.cleanup();
        Self { app }
    }

    /// Replaces the sim config.
    #[must_use]
    pub fn config(mut self, config: SimConfig) -> Self {
        self.app.insert_resource(config);
        self
    }

    /// Runs `SimEdit` once, then `n` ticks, bypassing real time.
    pub fn run_ticks(&mut self, n: u64) {
        let world = self.app.world_mut();
        world.run_schedule(SimEdit);
        for _ in 0..n {
            world.run_schedule(SimTick);
        }
    }

    /// Runs one driver frame of `real_dt` seconds (real-time pacing path).
    pub fn frame(&mut self, real_dt: f64) -> crate::driver::FramePlan {
        drive_frame(self.app.world_mut(), real_dt)
    }

    /// Completed ticks.
    #[must_use]
    pub fn ticks(&self) -> u64 {
        self.app.world().resource::<SimClock>().tick().0
    }

    /// Canonical state hash.
    #[must_use]
    pub fn state_hash(&self) -> u64 {
        state_hash(self.app.world())
    }
}
