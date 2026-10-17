//! Goods economy (ALGORITHMS §27, ADR-0057). Quantities are integer milli-units so the
//! conservation identity holds exactly:
//! `produced + imported = consumed + exported + stocked + in transit + lost`.
//!
//! Hourly at tick 0 of the game hour: industry produces 0.5 units per filled job into its stock
//! (capacity 40 × level) and exports above 80 % down to 50 %; commerce serves its share of the
//! retail demand while stock lasts (0.02 units per customer). At tick 90 commerce below 50 %
//! stock requests 10-unit deliveries from the nearest industry with stock (straight-line
//! distance until routing estimates travel times) or imports through the nearest outside
//! connection at +30 %. Deliveries travel at the macro truck speed and land on arrival.
//! Profit per hour = sales − wages − goods bought (+ exports) and feeds the business taxes.

use std::collections::BTreeMap;

use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use cb_core::ids::BuildingId;
use cb_core::time::TICKS_PER_GAME_HOUR;
use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::catalog::{Catalog, GoodsSpec, ZoneKind};
use crate::citizens::{Capacities, Population};

/// Milli-units per goods unit.
pub const MU: i64 = 1000;
/// Production per filled industrial job and game hour (milli-units).
pub const PRODUCTION_MU_PER_JOB_H: i64 = 500;
/// Consumption per customer unit (milli-units).
pub const CONSUMPTION_MU_PER_CUSTOMER: i64 = 20;
/// Tick of the delivery requests in the game hour (phase table).
pub const DELIVERY_OFFSET: u64 = 90;

/// One business.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Business {
    /// Commercial or industrial.
    pub zone: ZoneKind,
    /// Building level.
    pub level: u8,
    /// Stock (milli-units).
    pub stock_mu: i64,
    /// Capacity (milli-units).
    pub capacity_mu: i64,
    /// Goods on the way to this business (milli-units).
    pub incoming_mu: i64,
    /// Customers per game day (commerce).
    pub daily_customers: u32,
    /// Profit of the last game hour (cents).
    pub profit_hour_cents: i64,
    /// Goods bought since the last hourly profit (cents).
    pub(crate) bought_cents: i64,
    /// Goods sold to other businesses since the last hourly profit (cents).
    pub(crate) sold_cents: i64,
}

/// Where a delivery comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Supplier {
    /// A local industry.
    Industry(BuildingId),
    /// Imported through an outside connection.
    Import,
}

/// A delivery truck on its way (macro trip until traffic agents carry it, M9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delivery {
    /// Origin.
    pub from: Supplier,
    /// Destination business.
    pub to: BuildingId,
    /// Load (milli-units).
    pub units_mu: i64,
    /// Price paid by the destination (cents).
    pub cost_cents: i64,
}

/// Conservation counters (milli-units since the start).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoodsStats {
    /// Produced by industry.
    pub produced: i64,
    /// Imported.
    pub imported: i64,
    /// Consumed by customers.
    pub consumed: i64,
    /// Exported.
    pub exported: i64,
    /// Lost with removed businesses.
    pub lost: i64,
}

/// The goods economy (saved).
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Goods {
    pub(crate) businesses: BTreeMap<BuildingId, Business>,
    /// Deliveries by `(arrival tick, sequence)`.
    pub(crate) deliveries: BTreeMap<(u64, u32), Delivery>,
    pub(crate) next_seq: u32,
    pub(crate) stats: GoodsStats,
}

impl Goods {
    /// Businesses in ID order.
    pub fn businesses(&self) -> impl Iterator<Item = (BuildingId, &Business)> {
        self.businesses.iter().map(|(k, v)| (*k, v))
    }

    /// One business.
    #[must_use]
    pub fn business(&self, id: BuildingId) -> Option<&Business> {
        self.businesses.get(&id)
    }

    /// Deliveries on the way, by arrival.
    pub fn deliveries(&self) -> impl Iterator<Item = (u64, &Delivery)> {
        self.deliveries.iter().map(|((t, _), d)| (*t, d))
    }

    /// Conservation counters.
    #[must_use]
    pub fn stats(&self) -> GoodsStats {
        self.stats
    }

    /// Goods stocked by all businesses (milli-units).
    #[must_use]
    pub fn stocked_mu(&self) -> i64 {
        self.businesses.values().map(|b| b.stock_mu).sum()
    }

    /// Goods in transit (milli-units).
    #[must_use]
    pub fn in_transit_mu(&self) -> i64 {
        self.deliveries.values().map(|d| d.units_mu).sum()
    }

    /// Positive profit of the last hour by zone (commercial, industrial), cents.
    #[must_use]
    pub fn taxable_profit_cents(&self) -> (i64, i64) {
        let mut c = 0;
        let mut i = 0;
        for b in self.businesses.values() {
            let p = b.profit_hour_cents.max(0);
            match b.zone {
                ZoneKind::Commercial => c += p,
                ZoneKind::Industrial => i += p,
                ZoneKind::Residential => {}
            }
        }
        (c, i)
    }

    fn push_delivery(&mut self, arrive: u64, d: Delivery) {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        self.deliveries.insert((arrive, seq), d);
    }
}

/// Picks the supplier for a delivery to `to`: the nearest industry holding at least one load,
/// ties by building ID, else an import.
#[must_use]
pub fn pick_supplier(to: Vec2, industries: &[(BuildingId, Vec2, i64)], load_mu: i64) -> Supplier {
    industries
        .iter()
        .filter(|(_, _, stock)| *stock >= load_mu)
        .min_by(|a, b| {
            a.1.distance_squared(to)
                .total_cmp(&b.1.distance_squared(to))
                .then(a.0.cmp(&b.0))
        })
        .map_or(Supplier::Import, |(id, _, _)| Supplier::Industry(*id))
}

/// Travel time of a macro delivery (ticks).
#[must_use]
pub fn travel_ticks(distance_m: f32, spec: &GoodsSpec) -> u64 {
    let s = distance_m / spec.macro_speed_mps.max(0.1);
    ((s * 10.0).ceil() as u64).max(1)
}

fn wage_cents_per_day(cat: &Catalog, zone: ZoneKind, edu_level: usize) -> i64 {
    let w = cat.economy().wages;
    match zone {
        ZoneKind::Commercial => w.commercial[edu_level.min(2)],
        ZoneKind::Industrial => w.industrial[edu_level.min(2)],
        ZoneKind::Residential => 0,
    }
}

/// Brings the business table up to date with the open workplaces; stock of businesses that
/// closed is lost, deliveries to them are lost on arrival.
fn refresh(world: &mut World) {
    let Some(cat) = world.get_resource::<Catalog>() else {
        return;
    };
    let per_level = i64::from(cat.economy().goods.stock_units_per_level) * MU;
    let Some(cap) = world.get_resource::<Capacities>() else {
        return;
    };
    let open: BTreeMap<BuildingId, (ZoneKind, u8)> = cap
        .jobs()
        .filter_map(|(b, _)| Some((b, (cap.zone(b)?, cap.level(b)?))))
        .collect();
    let Some(mut g) = world.get_resource_mut::<Goods>() else {
        return;
    };
    let closed: Vec<BuildingId> = g
        .businesses
        .keys()
        .copied()
        .filter(|b| !open.contains_key(b))
        .collect();
    for b in closed {
        if let Some(x) = g.businesses.remove(&b) {
            g.stats.lost += x.stock_mu;
        }
    }
    for (b, (zone, level)) in open {
        let capacity_mu = per_level * i64::from(level);
        let e = g.businesses.entry(b).or_insert(Business {
            zone,
            level,
            stock_mu: 0,
            capacity_mu,
            incoming_mu: 0,
            daily_customers: 0,
            profit_hour_cents: 0,
            bought_cents: 0,
            sold_cents: 0,
        });
        e.level = level;
        e.capacity_mu = capacity_mu;
    }
}

/// Deliveries due by `now` land (or are lost when the business is gone).
pub(crate) fn arrivals(world: &mut World, now: u64) {
    let Some(mut g) = world.get_resource_mut::<Goods>() else {
        return;
    };
    let due: Vec<(u64, u32)> = g
        .deliveries
        .range(..=(now, u32::MAX))
        .map(|(k, _)| *k)
        .collect();
    for k in due {
        let Some(d) = g.deliveries.remove(&k) else {
            continue;
        };
        if let Some(b) = g.businesses.get_mut(&d.to) {
            b.incoming_mu -= d.units_mu;
            b.stock_mu += d.units_mu;
            b.bought_cents += d.cost_cents;
        } else {
            g.stats.lost += d.units_mu;
        }
    }
}

/// The hourly production, sales, exports and profit.
pub(crate) fn hour(world: &mut World, now: u64) {
    refresh(world);
    arrivals(world, now);
    let Some(cat) = world.get_resource::<Catalog>().cloned() else {
        return;
    };
    let spec = cat.economy().goods;
    // Retail demand (customer units per day) shared by commercial jobs.
    let (retail, workers) = {
        let Some(pop) = world.get_resource::<Population>() else {
            return;
        };
        let retail: f32 = pop
            .households()
            .map(|(_, h)| crate::demand::SPEND_BY_WEALTH[h.wealth().index()])
            .sum();
        let Some(g) = world.get_resource::<Goods>() else {
            return;
        };
        let workers: BTreeMap<BuildingId, [u16; 3]> = g
            .businesses
            .keys()
            .map(|b| (*b, pop.workers_of(*b)))
            .collect();
        (retail, workers)
    };
    let commercial_jobs: f32 = world.get_resource::<Capacities>().map_or(0.0, |cap| {
        cap.jobs()
            .filter(|(b, _)| cap.zone(*b) == Some(ZoneKind::Commercial))
            .map(|(_, j)| f32::from(j.iter().sum::<u16>()))
            .sum()
    });
    let jobs_of = |b: BuildingId| {
        world
            .get_resource::<Capacities>()
            .map_or([0; 3], |c| c.jobs_of(b))
    };
    let job_table: BTreeMap<BuildingId, [u16; 3]> =
        workers.keys().map(|b| (*b, jobs_of(*b))).collect();
    let Some(mut g) = world.get_resource_mut::<Goods>() else {
        return;
    };
    let mut stats = g.stats;
    for (id, b) in &mut g.businesses {
        let filled = workers.get(id).copied().unwrap_or_default();
        let wages: i64 = (0..3)
            .map(|k| i64::from(filled[k]) * wage_cents_per_day(&cat, b.zone, k))
            .sum::<i64>()
            / 24;
        let mut revenue = b.sold_cents;
        match b.zone {
            ZoneKind::Industrial => {
                let n: i64 = filled.iter().map(|x| i64::from(*x)).sum();
                let made = (n * PRODUCTION_MU_PER_JOB_H)
                    .min(b.capacity_mu - b.stock_mu)
                    .max(0);
                b.stock_mu += made;
                stats.produced += made;
                if b.stock_mu * 10 > b.capacity_mu * 8 {
                    let out = b.stock_mu - b.capacity_mu / 2;
                    b.stock_mu -= out;
                    stats.exported += out;
                    revenue += (out as f32 / MU as f32
                        * spec.unit_price_cents as f32
                        * spec.export_factor)
                        .round() as i64;
                }
            }
            ZoneKind::Commercial => {
                let jobs: f32 = f32::from(job_table.get(id).map_or(0, |j| j.iter().sum::<u16>()));
                let share = if commercial_jobs > 0.0 {
                    jobs / commercial_jobs
                } else {
                    0.0
                };
                let cap_customers = jobs * crate::demand::RETAIL_PER_JOB;
                let staffed = f32::from(filled.iter().sum::<u16>()) / jobs.max(1.0);
                b.daily_customers = ((retail * share).min(cap_customers) * staffed) as u32;
                let hour_customers = i64::from(b.daily_customers) / 24;
                let want = hour_customers * CONSUMPTION_MU_PER_CUSTOMER;
                let used = want.min(b.stock_mu).max(0);
                let served = if want > 0 {
                    hour_customers * used / want
                } else {
                    0
                };
                b.stock_mu -= used;
                stats.consumed += used;
                revenue += served * spec.revenue_per_customer_cents;
            }
            ZoneKind::Residential => {}
        }
        b.profit_hour_cents = revenue - wages - b.bought_cents;
        b.bought_cents = 0;
        b.sold_cents = 0;
    }
    g.stats = stats;
}

/// Delivery requests (tick 90): commerce below half stock orders loads from the nearest
/// industry with stock, else imports through the nearest outside connection.
pub(crate) fn request_deliveries(world: &mut World, now: u64) {
    let Some(spec) = world.get_resource::<Catalog>().map(|c| c.economy().goods) else {
        return;
    };
    let load = i64::from(spec.units_per_delivery) * MU;
    let outside: Vec<Vec2> = world
        .get_resource::<crate::terrain::TerrainRes>()
        .map(|t| t.outside_ends().to_vec())
        .unwrap_or_default();
    let fallback_outside = Vec2::new(
        cb_network::validation::Ground::half_extent(&crate::terrain::ground(world)),
        0.0,
    );
    let Some(cap) = world.get_resource::<Capacities>() else {
        return;
    };
    let pos = |b: BuildingId| cap.pos(b).unwrap_or(Vec2::ZERO);
    let Some(g) = world.get_resource::<Goods>() else {
        return;
    };
    let mut industries: Vec<(BuildingId, Vec2, i64)> = g
        .businesses
        .iter()
        .filter(|(_, b)| b.zone == ZoneKind::Industrial)
        .map(|(id, b)| (*id, pos(*id), b.stock_mu))
        .collect();
    let needs: Vec<(BuildingId, Vec2, i64)> = g
        .businesses
        .iter()
        .filter(|(_, b)| b.zone == ZoneKind::Commercial)
        .filter_map(|(id, b)| {
            let have = b.stock_mu + b.incoming_mu;
            (have * 2 < b.capacity_mu).then(|| (*id, pos(*id), b.capacity_mu - have))
        })
        .collect();
    let mut planned: Vec<(u64, Delivery, Option<usize>)> = Vec::new();
    for (to, at, deficit) in needs {
        let loads = (deficit + load - 1) / load;
        for _ in 0..loads {
            let (from, dist, idx) = match pick_supplier(at, &industries, load) {
                Supplier::Industry(id) => {
                    let i = industries.iter().position(|x| x.0 == id).unwrap_or(0);
                    industries[i].2 -= load;
                    (
                        Supplier::Industry(id),
                        industries[i].1.distance(at),
                        Some(i),
                    )
                }
                Supplier::Import => {
                    let o = outside
                        .iter()
                        .copied()
                        .min_by(|a, b| a.distance_squared(at).total_cmp(&b.distance_squared(at)))
                        .unwrap_or(fallback_outside);
                    (Supplier::Import, o.distance(at), None)
                }
            };
            let factor = match from {
                Supplier::Industry(_) => 1.0,
                Supplier::Import => spec.import_factor,
            };
            let cost = (spec.units_per_delivery as f32 * spec.unit_price_cents as f32 * factor)
                .round() as i64;
            planned.push((
                now + travel_ticks(dist, &spec),
                Delivery {
                    from,
                    to,
                    units_mu: load,
                    cost_cents: cost,
                },
                idx,
            ));
        }
    }
    let unit_price = i64::from(spec.units_per_delivery) * spec.unit_price_cents;
    let Some(mut g) = world.get_resource_mut::<Goods>() else {
        return;
    };
    for (arrive, d, _) in planned {
        match d.from {
            Supplier::Industry(src) => {
                if let Some(s) = g.businesses.get_mut(&src) {
                    s.stock_mu -= d.units_mu;
                    s.sold_cents += unit_price;
                }
            }
            Supplier::Import => g.stats.imported += d.units_mu,
        }
        if let Some(b) = g.businesses.get_mut(&d.to) {
            b.incoming_mu += d.units_mu;
        }
        g.push_delivery(arrive, d);
    }
}

/// `TickSet::SubRate`: delivery requests at tick 90 and arrivals every 20 ticks.
pub(crate) fn tick(world: &mut World) {
    let now = world
        .get_resource::<crate::SimClock>()
        .map_or(0, |c| c.tick().0);
    if now % TICKS_PER_GAME_HOUR == DELIVERY_OFFSET {
        request_deliveries(world, now);
    }
    if now.is_multiple_of(20) {
        arrivals(world, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u32) -> BuildingId {
        BuildingId::try_new(n).unwrap()
    }

    #[test]
    fn deliveries_prefer_nearest_supplier() {
        let to = Vec2::ZERO;
        let industries = [
            (id(1), Vec2::new(900.0, 0.0), 50 * MU),
            (id(2), Vec2::new(200.0, 0.0), 50 * MU),
            (id(3), Vec2::new(100.0, 0.0), 5 * MU),
        ];
        // The nearest has too little stock for a load; the next nearest supplies.
        assert_eq!(
            pick_supplier(to, &industries, 10 * MU),
            Supplier::Industry(id(2))
        );
        // Ties by ID.
        let tie = [
            (id(7), Vec2::new(0.0, 300.0), 50 * MU),
            (id(4), Vec2::new(300.0, 0.0), 50 * MU),
        ];
        assert_eq!(pick_supplier(to, &tie, 10 * MU), Supplier::Industry(id(4)));
    }

    #[test]
    fn import_when_no_supplier() {
        assert_eq!(pick_supplier(Vec2::ZERO, &[], 10 * MU), Supplier::Import);
        let empty = [(id(1), Vec2::new(10.0, 0.0), 9 * MU)];
        assert_eq!(pick_supplier(Vec2::ZERO, &empty, 10 * MU), Supplier::Import);
    }
}
