//! Canonical state hash for determinism tests. Grows with every milestone; from M3 it hashes
//! the canonical save snapshot (TESTING §3). Objects are hashed in stable-ID order.

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
    hash_objects(world, &mut h);
    h.finish()
}

fn f32s<'a>(h: &mut StableHasher, v: impl IntoIterator<Item = &'a f32>) {
    for x in v {
        h.write_u32(x.to_bits());
    }
}

fn hash_objects(world: &World, h: &mut StableHasher) {
    use crate::buildings::Building;
    use crate::roads::{RoadNode, RoadSegment};
    use crate::zoning::{Lot, Plot};
    let Some(ids) = world.get_resource::<crate::ids::Ids>() else {
        return;
    };
    world
        .get_resource::<crate::economy::Treasury>()
        .map(|t| t.balance().0)
        .hash(h);
    for (id, e) in ids.nodes.iter() {
        id.hash(h);
        if let Some(n) = world.get::<RoadNode>(e) {
            f32s(h, &n.pos().to_array());
            n.segments().hash(h);
            (n.control(), n.custom()).hash(h);
            if let Some(j) = n.junction() {
                f32s(h, &j.cuts);
            }
        }
    }
    for (id, e) in ids.segments.iter() {
        id.hash(h);
        if let Some(s) = world.get::<RoadSegment>(e) {
            (s.start(), s.end(), s.kind(), s.variant(), s.style()).hash(h);
            f32s(h, s.curve().p.iter().flat_map(|p| [&p.x, &p.y]));
            f32s(h, &s.profile().h);
        }
    }
    for (id, e) in ids.plots.iter() {
        id.hash(h);
        if let Some(p) = world.get::<Plot>(e) {
            f32s(h, p.polygon().iter().flat_map(|v| [&v.x, &v.y]));
            (p.lots(), p.dn()).hash(h);
        }
    }
    for (id, e) in ids.lots.iter() {
        id.hash(h);
        if let Some(l) = world.get::<Lot>(e) {
            f32s(h, l.polygon().iter().flat_map(|v| [&v.x, &v.y]));
            l.building().hash(h);
        }
    }
    for (id, e) in ids.buildings.iter() {
        id.hash(h);
        if let Some(b) = world.get::<Building>(e) {
            (b.kind(), b.lot(), b.level(), b.seed(), b.condition()).hash(h);
            h.write_u32(b.foundation().floor_y.to_bits());
        }
    }
    for (id, e) in ids.props.iter() {
        id.hash(h);
        if let Some(p) = world.get::<crate::props::PlacedProp>(e) {
            (p.kind(), p.seed()).hash(h);
            f32s(h, &[p.pos().x, p.pos().y, p.pos().z, p.yaw()]);
        }
    }
}
