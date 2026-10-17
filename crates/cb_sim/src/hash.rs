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
    hash_population(world, h);
}

fn hash_population(world: &World, h: &mut StableHasher) {
    let Some(pop) = world.get_resource::<crate::citizens::Population>() else {
        return;
    };
    for (id, hh) in pop.households() {
        (id, hh.home(), hh.members(), hh.wealth(), hh.cars()).hash(h);
        (hh.moved_in_tick(), hh.happiness().to_bits()).hash(h);
    }
    for (id, c) in pop.citizens() {
        (id, c.household(), c.age(), c.education(), c.job()).hash(h);
        (c.shift_start_min(), c.commute_sim_s(), c.no_route()).hash(h);
    }
    if let Some(t) = world.get_resource::<crate::traffic::Traffic>() {
        let s = t.to_save();
        for (k, today, last) in &s.links {
            (k, today.to_bits(), last.to_bits()).hash(h);
        }
        (&s.acc.0, &s.acc.1, s.acc.2, s.acc.3).hash(h);
    }
    // The ledger is a record of flows, not world state: an undone build leaves a charge and a
    // refund in it while the balance (hashed above) returns. Saves carry it in full.
    if let Some(f) = world.get_resource::<crate::economy::Finances>() {
        for z in [
            crate::catalog::ZoneKind::Residential,
            crate::catalog::ZoneKind::Commercial,
            crate::catalog::ZoneKind::Industrial,
        ] {
            f.tax_bp(z).hash(h);
        }
        (f.loans().len(), f.days_negative(), f.bankrupt()).hash(h);
    }
    if let Some(g) = world.get_resource::<crate::economy::goods::Goods>() {
        for (id, b) in g.businesses() {
            (
                id,
                b.stock_mu,
                b.incoming_mu,
                b.daily_customers,
                b.profit_hour_cents,
            )
                .hash(h);
        }
        for (t, d) in g.deliveries() {
            (t, d.to, d.units_mu, d.cost_cents).hash(h);
        }
    }
    if let Some(u) = world.get_resource::<crate::utilities::Utilities>() {
        for (id, st) in &u.status {
            (id, st).hash(h);
        }
        for (id, g) in &u.garbage_kg {
            (id, g.to_bits()).hash(h);
        }
        for (id, g) in &u.landfill_kg {
            (id, g.to_bits()).hash(h);
        }
    }
    if let Some(d) = world.get_resource::<crate::demand::Demand>() {
        f32s(
            h,
            &[
                d.zone(crate::catalog::ZoneKind::Residential),
                d.zone(crate::catalog::ZoneKind::Commercial),
                d.zone(crate::catalog::ZoneKind::Industrial),
                d.immigration(),
            ],
        );
    }
}
