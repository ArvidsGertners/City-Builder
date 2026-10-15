//! Canonical state hash for determinism tests. Grows with every milestone; from M3 it hashes
//! the canonical save snapshot (TESTING §3).

use core::hash::{Hash, Hasher};

use bevy::ecs::world::World;
use cb_core::hash::StableHasher;

use crate::clock::SimClock;
use crate::config::{SimConfig, WorldSeed};

/// Stable hash over all sim state that exists so far.
#[must_use]
pub fn state_hash(world: &World) -> u64 {
    let mut h = StableHasher::default();
    world.get_resource::<WorldSeed>().map(|s| s.0).hash(&mut h);
    world
        .get_resource::<SimClock>()
        .map(|c| c.tick().0)
        .hash(&mut h);
    world.get_resource::<SimConfig>().hash(&mut h);
    h.finish()
}
