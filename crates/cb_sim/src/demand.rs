//! Zone demand (ALGORITHMS §15): hourly at tick 60 of the game hour, R/C/I in −1…1 smoothed
//! with an EMA of half-life 6 game hours; the breakdown of every term is kept for tooltips.
//!
//! Terms whose systems do not exist yet use the neutral stubs of GAME_DESIGN §10 (happiness 60,
//! coverage 0.5, land value 0.5, commute 15 perceived min, goods available, exports reachable,
//! taxes at the 9 % default). Immigration uses R without the housing-vacancy term: vacant
//! homes attract movers, they only hold back new residential construction.

use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;

use crate::catalog::ZoneKind;
use crate::citizens::{Capacities, Population};

/// Tick of the demand update in the game hour (phase table).
pub const OFFSET: u64 = 60;
/// EMA weight of one hourly sample (half-life 6 game hours): `1 − 0.5^(1/6)`.
pub const EMA_ALPHA: f32 = 0.109_101_01;
/// Default tax rate until the economy sets rates (M5.4).
pub const DEFAULT_TAX: f32 = 0.09;

/// Every term of one demand sample (raw, before smoothing).
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Breakdown {
    /// `(jobs − filled) / jobs`.
    pub job_vacancy: f32,
    /// Free household slots / slots.
    pub housing_vacancy: f32,
    /// Attractiveness (signed).
    pub attractiveness: f32,
    /// Base immigration (0.3 until 300 citizens, to 0 at 1 500).
    pub base_immigration: f32,
    /// Weighted terms of R: job vacancy, attractiveness, housing vacancy, tax, base.
    pub r_terms: [f32; 5],
    /// Weighted terms of C: retail gap, goods availability, tax.
    pub c_terms: [f32; 3],
    /// Weighted terms of I: goods gap, workforce, export access, tax.
    pub i_terms: [f32; 4],
}

impl Breakdown {
    /// Raw R (sum of its terms, clamped).
    #[must_use]
    pub fn r(&self) -> f32 {
        self.r_terms.iter().sum::<f32>().clamp(-1.0, 1.0)
    }

    /// Raw C.
    #[must_use]
    pub fn c(&self) -> f32 {
        self.c_terms.iter().sum::<f32>().clamp(-1.0, 1.0)
    }

    /// Raw I.
    #[must_use]
    pub fn i(&self) -> f32 {
        self.i_terms.iter().sum::<f32>().clamp(-1.0, 1.0)
    }

    /// Raw R for immigration (without the housing-vacancy term).
    #[must_use]
    pub fn r_immigration(&self) -> f32 {
        (self.r_terms.iter().sum::<f32>() - self.r_terms[2]).clamp(-1.0, 1.0)
    }
}

/// Smoothed demand per zone and the last breakdown.
#[derive(
    Resource, Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize,
)]
pub struct Demand {
    pub(crate) r: f32,
    pub(crate) c: f32,
    pub(crate) i: f32,
    pub(crate) r_immigration: f32,
    pub(crate) last: Breakdown,
    pub(crate) samples: u32,
}

impl Demand {
    /// Smoothed demand of a zone.
    #[must_use]
    pub fn zone(&self, z: ZoneKind) -> f32 {
        match z {
            ZoneKind::Residential => self.r,
            ZoneKind::Commercial => self.c,
            ZoneKind::Industrial => self.i,
        }
    }

    /// Smoothed R used by immigration.
    #[must_use]
    pub fn immigration(&self) -> f32 {
        self.r_immigration
    }

    /// The last raw sample's breakdown.
    #[must_use]
    pub fn breakdown(&self) -> &Breakdown {
        &self.last
    }
}

/// Inputs of one demand sample (plain data, so the formula is testable without a world).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Inputs {
    /// Citizens.
    pub population: f32,
    /// Jobs offered, filled.
    pub jobs: f32,
    /// Jobs filled.
    pub jobs_filled: f32,
    /// Household slots.
    pub home_slots: f32,
    /// Free household slots.
    pub free_homes: f32,
    /// Unemployment rate of the labour force.
    pub unemployment: f32,
    /// Mean happiness / 100.
    pub happiness: f32,
    /// Mean service coverage 0–1.
    pub coverage: f32,
    /// Mean land value 0–1.
    pub land_value: f32,
    /// Perceived commute p50 (min).
    pub commute_p50_min: f32,
    /// Retail demand (customer units per day).
    pub retail_demand: f32,
    /// Retail capacity (commercial jobs × 25).
    pub retail_capacity: f32,
    /// Goods availability 0–1.
    pub goods_availability: f32,
    /// Goods demand (units per day).
    pub goods_demand: f32,
    /// Production (units per day).
    pub production: f32,
    /// Unemployed adults qualified for open industrial jobs.
    pub unemployed_matching: f32,
    /// Open industrial jobs.
    pub open_industrial_jobs: f32,
    /// 1 when an outside connection is reachable within 300 sim s, else 0.3.
    pub export_access: f32,
    /// Tax rates R, C, I.
    pub tax: [f32; 3],
}

fn tax_penalty(rate: f32) -> f32 {
    ((rate - 0.09) / 0.11).max(0.0)
}

/// One raw sample (ALGORITHMS §15).
#[must_use]
pub fn sample(x: &Inputs) -> Breakdown {
    let job_vacancy = ((x.jobs - x.jobs_filled) / x.jobs.max(1.0)).clamp(0.0, 1.0);
    let housing_vacancy = (x.free_homes / x.home_slots.max(1.0)).clamp(0.0, 1.0);
    let attractiveness =
        0.30 * (2.0 * x.happiness - 1.0) + 0.20 * x.coverage + 0.20 * (2.0 * x.land_value - 1.0)
            - 0.20 * (5.0 * x.unemployment).min(1.0)
            - 0.10 * ((x.commute_p50_min - 20.0) / 20.0).clamp(0.0, 1.0);
    let base_immigration = if x.population <= 300.0 {
        0.3
    } else {
        0.3 * (1.0 - (x.population - 300.0) / 1200.0).clamp(0.0, 1.0)
    };
    let r_terms = [
        0.45 * job_vacancy,
        0.25 * attractiveness,
        -0.35 * housing_vacancy,
        -0.2 * tax_penalty(x.tax[0]),
        base_immigration,
    ];
    let c_terms = [
        0.5 * (x.retail_demand - x.retail_capacity) / x.retail_demand.max(1.0),
        0.2 * x.goods_availability,
        -0.2 * tax_penalty(x.tax[1]),
    ];
    let workforce =
        (x.unemployed_matching / (x.open_industrial_jobs + 50.0).max(1.0)).clamp(0.0, 1.0);
    let i_terms = [
        0.4 * (x.goods_demand - x.production) / x.goods_demand.max(1.0),
        0.35 * workforce,
        0.15 * x.export_access,
        -0.2 * tax_penalty(x.tax[2]),
    ];
    Breakdown {
        job_vacancy,
        housing_vacancy,
        attractiveness,
        base_immigration,
        r_terms,
        c_terms,
        i_terms,
    }
}

/// Customer units per household and day by wealth (ALGORITHMS §15).
pub const SPEND_BY_WEALTH: [f32; 3] = [15.0, 25.0, 40.0];
/// Customer units served per commercial job and day.
pub const RETAIL_PER_JOB: f32 = 25.0;
/// Goods units consumed per customer unit (ALGORITHMS §27).
pub const GOODS_PER_CUSTOMER: f32 = 0.02;
/// Goods units produced per filled industrial job and game hour (ALGORITHMS §27).
pub const GOODS_PER_JOB_H: f32 = 0.5;

/// Gathers the inputs from the world.
#[must_use]
pub fn inputs(world: &World) -> Inputs {
    let mut x = Inputs {
        happiness: 0.6,
        coverage: 0.5,
        land_value: 0.5,
        commute_p50_min: 15.0,
        goods_availability: 1.0,
        export_access: 1.0,
        tax: [DEFAULT_TAX; 3],
        ..Inputs::default()
    };
    let (Some(pop), Some(cap)) = (
        world.get_resource::<Population>(),
        world.get_resource::<Capacities>(),
    ) else {
        return x;
    };
    x.population = pop.population() as f32;
    let mut happiness = 0.0;
    for (_, h) in pop.households() {
        x.retail_demand += SPEND_BY_WEALTH[h.wealth().index()];
        happiness += h.happiness();
    }
    if !pop.households.is_empty() {
        x.happiness = happiness / pop.households.len() as f32 / 100.0;
    }
    for (b, slots) in cap.homes() {
        x.home_slots += f32::from(slots);
        x.free_homes += f32::from(
            slots.saturating_sub(u16::try_from(pop.residents_of(b).len()).unwrap_or(u16::MAX)),
        );
    }
    let mut open_i = 0.0;
    let mut open_i_edu = [0.0f32; 3];
    for (b, jobs) in cap.jobs() {
        let f = pop.workers_of(b);
        let total: u16 = jobs.iter().sum();
        let filled: u16 = (0..3).map(|k| f[k].min(jobs[k])).sum();
        x.jobs += f32::from(total);
        x.jobs_filled += f32::from(filled);
        match cap.zone(b) {
            Some(ZoneKind::Commercial) => x.retail_capacity += f32::from(total) * RETAIL_PER_JOB,
            Some(ZoneKind::Industrial) => {
                x.production += f32::from(filled) * GOODS_PER_JOB_H * 24.0;
                for k in 0..3 {
                    let o = f32::from(jobs[k].saturating_sub(f[k]));
                    open_i += o;
                    open_i_edu[k] += o;
                }
            }
            _ => {}
        }
    }
    x.goods_demand = x.retail_demand * GOODS_PER_CUSTOMER * 24.0;
    // One pass over the citizens: unemployment, and the unemployed adults who qualify for an
    // open industrial job (any level ≤ their education).
    let (mut force, mut unemployed, mut matching) = (0u32, 0u32, 0.0);
    for (_, c) in pop.citizens() {
        if !c.in_labour_force() {
            continue;
        }
        force += 1;
        if c.job().is_none() {
            unemployed += 1;
            let e = usize::from(c.education().min(2));
            if open_i_edu[..=e].iter().any(|o| *o > 0.0) {
                matching += 1.0;
            }
        }
    }
    x.unemployment = if force == 0 {
        0.0
    } else {
        unemployed as f32 / force as f32
    };
    x.unemployed_matching = matching;
    x.open_industrial_jobs = open_i;
    x
}

/// `TickSet::SubRate`: the hourly demand update.
pub(crate) fn tick(world: &mut World) {
    let now = world
        .get_resource::<crate::SimClock>()
        .map_or(0, |c| c.tick().0);
    let t = crate::citizens::tick_in_hour(now);
    if t == OFFSET - 2 {
        crate::citizens::capacity::refresh(world);
    }
    if t != OFFSET {
        return;
    }
    let s = sample(&inputs(world));
    let mut d = world.get_resource::<Demand>().copied().unwrap_or_default();
    // The first sample seeds the average so a new city does not wait a day for demand.
    let a = if d.samples == 0 { 1.0 } else { EMA_ALPHA };
    d.r += a * (s.r() - d.r);
    d.c += a * (s.c() - d.c);
    d.i += a * (s.i() - d.i);
    d.r_immigration += a * (s.r_immigration() - d.r_immigration);
    d.last = s;
    d.samples = d.samples.saturating_add(1);
    world.insert_resource(d);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Inputs {
        Inputs {
            population: 500.0,
            jobs: 200.0,
            jobs_filled: 150.0,
            home_slots: 300.0,
            free_homes: 30.0,
            unemployment: 0.05,
            happiness: 0.6,
            coverage: 0.5,
            land_value: 0.5,
            commute_p50_min: 15.0,
            retail_demand: 5000.0,
            retail_capacity: 4000.0,
            goods_availability: 1.0,
            goods_demand: 2400.0,
            production: 1800.0,
            unemployed_matching: 10.0,
            open_industrial_jobs: 20.0,
            export_access: 1.0,
            tax: [DEFAULT_TAX; 3],
        }
    }

    #[test]
    fn demand_breakdown_sums_to_value() {
        let b = sample(&base());
        assert!((b.r() - b.r_terms.iter().sum::<f32>().clamp(-1.0, 1.0)).abs() < 1e-6);
        assert!((b.c() - b.c_terms.iter().sum::<f32>().clamp(-1.0, 1.0)).abs() < 1e-6);
        assert!((b.i() - b.i_terms.iter().sum::<f32>().clamp(-1.0, 1.0)).abs() < 1e-6);
        assert!((b.r_immigration() - (b.r() - b.r_terms[2])).abs() < 1e-6);
    }

    #[test]
    fn demand_monotonic_responses() {
        let b0 = sample(&base());
        let more_jobs = sample(&Inputs {
            jobs: 400.0,
            ..base()
        });
        assert!(more_jobs.r() > b0.r(), "open jobs raise R");
        let more_homes = sample(&Inputs {
            free_homes: 200.0,
            ..base()
        });
        assert!(more_homes.r() < b0.r(), "vacant homes lower R");
        assert!((more_homes.r_immigration() - b0.r_immigration()).abs() < 1e-6);
        let unhappy = sample(&Inputs {
            happiness: 0.3,
            ..base()
        });
        assert!(unhappy.r() < b0.r());
        let taxed = sample(&Inputs {
            tax: [0.2, 0.2, 0.2],
            ..base()
        });
        assert!(taxed.r() < b0.r() && taxed.c() < b0.c() && taxed.i() < b0.i());
        let shops = sample(&Inputs {
            retail_capacity: 6000.0,
            ..base()
        });
        assert!(shops.c() < b0.c(), "more shops lower C");
        let factories = sample(&Inputs {
            production: 3000.0,
            ..base()
        });
        assert!(factories.i() < b0.i(), "more production lowers I");
        let big = sample(&Inputs {
            population: 2000.0,
            ..base()
        });
        assert!(big.base_immigration.abs() < 1e-6 && big.r() < b0.r());
    }
}
