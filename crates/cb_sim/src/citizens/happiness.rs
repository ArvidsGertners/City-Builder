//! Household happiness (GAME_DESIGN §10, ALGORITHMS §16), daily at hour 2: a weighted sum of
//! factor scores in 0–1 (weights sum to 100). Inputs of systems that do not exist yet use the
//! neutral stubs below; replacing a stub with the real input changes the input value only,
//! never the formula. Households below 35 for 3 days leave with 20 % probability per day.

use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use cb_core::ids::HouseholdId;
use cb_core::rng::{RngStream, SimRng};
use cb_core::time::{TICKS_PER_GAME_DAY, TICKS_PER_GAME_HOUR};

use super::{Capacities, Population};
use crate::catalog::{Catalog, ZoneKind};
use crate::config::WorldSeed;

/// Service coverage stub until services exist (M6).
pub const SERVICES_STUB: f32 = 0.5;
/// Crime stub until services exist (M6).
pub const CRIME_STUB: f32 = 0.2;
/// Neutral commute score of households without commuting workers.
pub const COMMUTE_STUB: f32 = 0.7;
/// Tick of the daily happiness pass in the game day (hour 2).
pub const OFFSET: u64 = 2 * TICKS_PER_GAME_HOUR;
/// Happiness below which a household counts an unhappy day.
pub const UNHAPPY_BELOW: f32 = 35.0;
/// Unhappy days before households start leaving.
pub const UNHAPPY_DAYS: u16 = 3;
/// Daily move-out probability once unhappy for long enough.
pub const MOVE_OUT_P: f32 = 0.2;

/// Factors in GAME_DESIGN order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Factor {
    /// Home quality.
    Housing,
    /// Employment of the household's adults.
    Employment,
    /// Power, water, sewage.
    Utilities,
    /// Garbage collection.
    Garbage,
    /// Service coverage.
    Services,
    /// Commute time.
    Commute,
    /// Ground pollution.
    Pollution,
    /// Noise.
    Noise,
    /// Crime.
    Crime,
    /// Taxes.
    Taxes,
    /// Land value and parks.
    LandValue,
}

impl Factor {
    /// Every factor.
    pub const ALL: [Self; 11] = [
        Self::Housing,
        Self::Employment,
        Self::Utilities,
        Self::Garbage,
        Self::Services,
        Self::Commute,
        Self::Pollution,
        Self::Noise,
        Self::Crime,
        Self::Taxes,
        Self::LandValue,
    ];

    /// Weight (GAME_DESIGN §10).
    #[must_use]
    pub const fn weight(self) -> f32 {
        match self {
            Self::Housing => 10.0,
            Self::Employment => 15.0,
            Self::Utilities => 15.0,
            Self::Garbage => 5.0,
            Self::Services => 15.0,
            Self::Commute => 12.0,
            Self::Pollution => 8.0,
            Self::Noise => 5.0,
            Self::Crime => 6.0,
            Self::Taxes => 5.0,
            Self::LandValue => 4.0,
        }
    }
}

/// Raw inputs of one household's happiness.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inputs {
    /// Home level 1–5.
    pub home_level: u8,
    /// Employed share of the household's labour force (1 without one).
    pub employment: f32,
    /// Power, water, sewage served; water polluted.
    pub power: bool,
    /// Water served.
    pub water: bool,
    /// Sewage served.
    pub sewage: bool,
    /// Water polluted.
    pub polluted_water: bool,
    /// Uncollected garbage problem.
    pub garbage_problem: bool,
    /// Service coverage 0–1.
    pub services: f32,
    /// Commute score 0–1.
    pub commute: f32,
    /// Pollution 0–1 (saturated).
    pub pollution: f32,
    /// Noise 0–1 (quiet → loud).
    pub noise: f32,
    /// Crime 0–1.
    pub crime: f32,
    /// Residential tax rate (fraction).
    pub tax: f32,
    /// Land value 0–1.
    pub land_value: f32,
}

impl Inputs {
    /// Inputs with every not-yet-simulated factor at its stub.
    #[must_use]
    pub fn with_stubs() -> Self {
        Self {
            home_level: 1,
            employment: 1.0,
            power: true,
            water: true,
            sewage: true,
            polluted_water: false,
            garbage_problem: false,
            services: SERVICES_STUB,
            commute: COMMUTE_STUB,
            pollution: 0.0,
            noise: 0.0,
            crime: CRIME_STUB,
            tax: 0.09,
            land_value: 0.5,
        }
    }
}

/// Factor scores 0–1.
#[must_use]
pub fn scores(x: &Inputs) -> [f32; 11] {
    let water = if x.water {
        if x.polluted_water { 0.5 } else { 1.0 }
    } else {
        0.0
    };
    let utilities = (f32::from(u8::from(x.power)) + water + f32::from(u8::from(x.sewage))) / 3.0;
    [
        (0.4 + 0.12 * f32::from(x.home_level.clamp(1, 5))).min(1.0),
        x.employment.clamp(0.0, 1.0),
        utilities,
        if x.garbage_problem { 0.0 } else { 1.0 },
        x.services.clamp(0.0, 1.0),
        x.commute.clamp(0.0, 1.0),
        1.0 - x.pollution.clamp(0.0, 1.0),
        1.0 - x.noise.clamp(0.0, 1.0),
        1.0 - x.crime.clamp(0.0, 1.0),
        (1.0 - x.tax / 0.25).clamp(0.0, 1.0),
        x.land_value.clamp(0.0, 1.0),
    ]
}

/// Happiness 0–100.
#[must_use]
pub fn happiness(x: &Inputs) -> f32 {
    scores(x)
        .iter()
        .zip(Factor::ALL)
        .map(|(s, f)| s * f.weight())
        .sum()
}

/// City-wide happiness (derived daily).
#[derive(
    Resource, Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize,
)]
pub struct HappinessStats {
    /// Mean happiness of households.
    pub mean: f32,
    /// Mean score per factor.
    pub factors: [f32; 11],
    /// Households that moved out today.
    pub moved_out_today: u32,
    /// Running sums of the day's slices (saved: the pass spans 600 ticks).
    #[serde(default)]
    pub(crate) acc_sum: f64,
    #[serde(default)]
    pub(crate) acc_factors: [f64; 11],
    #[serde(default)]
    pub(crate) acc_n: u32,
    #[serde(default)]
    pub(crate) acc_moved: u32,
}

/// Happiness slices per day (households `id % 600`), one per tick from [`OFFSET`].
pub const SLICES: u64 = 600;

/// One slice of the daily pass (households with `id % SLICES == k`): score them, count unhappy
/// days, move the long-unhappy out; the last slice publishes the city-wide means.
pub(crate) fn daily_slice(world: &mut World, now: u64, k: u64) {
    let Some(cat) = world.get_resource::<Catalog>() else {
        return;
    };
    let fspec = *cat.fields();
    let cspec = cat.citizens().commute;
    let seed = world.get_resource::<WorldSeed>().map_or(0, |s| s.0);
    let tax = world
        .get_resource::<crate::economy::Finances>()
        .map_or(0.09, |f| {
            f32::from(f.tax_bp(ZoneKind::Residential)) / 10_000.0
        });
    let (Some(pop), Some(cap)) = (
        world.get_resource::<Population>(),
        world.get_resource::<Capacities>(),
    ) else {
        return;
    };
    let utilities = world.get_resource::<crate::utilities::Utilities>();
    let fields = world.get_resource::<crate::fields::Fields>();
    let mut results: Vec<(HouseholdId, f32, [f32; 11])> = Vec::new();
    let mut slice_ids = Vec::new();
    let mut raw = k;
    while raw < u64::from(pop.household_ids.peek_next()) {
        if let Some(hid) = u32::try_from(raw)
            .ok()
            .and_then(|r| HouseholdId::try_new(r).ok())
            && let Some(h) = pop.household(hid)
        {
            slice_ids.push((hid, h));
        }
        raw += SLICES;
    }
    for (hid, h) in slice_ids {
        let st = utilities.map(|u| u.status(h.home())).unwrap_or_default();
        let pos = cap.pos(h.home()).unwrap_or_default();
        let commute = commute_score(pop, h.members(), &cspec);
        let (force, employed) = h.members().iter().fold((0u32, 0u32), |acc, c| {
            pop.citizen(*c).map_or(acc, |c| {
                if c.in_labour_force() {
                    (acc.0 + 1, acc.1 + u32::from(c.job().is_some()))
                } else {
                    acc
                }
            })
        });
        let x = Inputs {
            home_level: cap.level(h.home()).unwrap_or(1),
            employment: if force == 0 {
                1.0
            } else {
                employed as f32 / force as f32
            },
            power: st.power,
            water: st.water,
            sewage: st.sewage,
            polluted_water: st.polluted_water,
            garbage_problem: st.garbage_problem,
            services: SERVICES_STUB,
            commute,
            pollution: fields.map_or(0.0, |f| {
                f.pollution().sample(pos) / fspec.pollution.saturation
            }),
            noise: fields.map_or(0.0, |f| {
                (f.noise().sample(pos) - fspec.noise.quiet_db)
                    / (fspec.noise.loud_db - fspec.noise.quiet_db)
            }),
            crime: CRIME_STUB,
            tax,
            land_value: fields.map_or(0.5, |f| f.land_value().sample(pos)),
        };
        results.push((hid, happiness(&x), scores(&x)));
    }
    let mut stats = world
        .get_resource::<HappinessStats>()
        .copied()
        .unwrap_or_default();
    if k == 0 {
        stats.acc_sum = 0.0;
        stats.acc_factors = [0.0; 11];
        stats.acc_n = 0;
        stats.acc_moved = 0;
    }
    for (_, v, s) in &results {
        stats.acc_sum += f64::from(*v);
        for (acc, x) in stats.acc_factors.iter_mut().zip(s) {
            *acc += f64::from(*x);
        }
        stats.acc_n += 1;
    }
    let day = now / TICKS_PER_GAME_DAY;
    let Some(mut pop) = world.get_resource_mut::<Population>() else {
        return;
    };
    let mut leaving = Vec::new();
    for (hid, v, _) in results {
        let Some(h) = pop.households.get_mut(&hid) else {
            continue;
        };
        h.happiness = v;
        if v < UNHAPPY_BELOW {
            h.unhappy_days = h.unhappy_days.saturating_add(1);
        } else {
            h.unhappy_days = 0;
        }
        if h.unhappy_days >= UNHAPPY_DAYS
            && SimRng::for_event(seed, RngStream::Citizens, 3, day, u64::from(hid.get()))
                .chance(MOVE_OUT_P)
        {
            leaving.push(hid);
        }
    }
    stats.acc_moved += u32::try_from(leaving.len()).unwrap_or(u32::MAX);
    for hid in leaving {
        pop.remove_household(hid);
    }
    if k + 1 == SLICES {
        let n = f64::from(stats.acc_n.max(1));
        stats.mean = (stats.acc_sum / n) as f32;
        for (f, a) in stats.factors.iter_mut().zip(stats.acc_factors) {
            *f = (a / n) as f32;
        }
        stats.moved_out_today = stats.acc_moved;
    }
    world.insert_resource(stats);
}

/// Commute score of a household: the mean over its workers of `1` at or below the short and `0`
/// at or above the long perceived commute, `0` without a route; neutral without workers or
/// estimates.
pub(crate) fn commute_score(
    pop: &Population,
    members: &[cb_core::ids::CitizenId],
    spec: &crate::catalog::CommuteSpec,
) -> f32 {
    let (lo, hi) = spec.score_min;
    let mut sum = 0.0;
    let mut n = 0u32;
    for c in members.iter().filter_map(|c| pop.citizen(*c)) {
        if c.no_route() {
            n += 1;
        } else if let (Some(_), Some(s)) = (c.job(), c.commute_sim_s()) {
            let perceived = f32::from(s) * 0.1;
            sum += (1.0 - (perceived - lo) / (hi - lo).max(1.0)).clamp(0.0, 1.0);
            n += 1;
        }
    }
    if n == 0 { COMMUTE_STUB } else { sum / n as f32 }
}

/// `TickSet::SubRate`: the daily happiness pass.
pub(crate) fn tick(world: &mut World) {
    let now = world
        .get_resource::<crate::SimClock>()
        .map_or(0, |c| c.tick().0);
    let t = now % TICKS_PER_GAME_DAY;
    if (OFFSET..OFFSET + SLICES).contains(&t) {
        daily_slice(world, now, t - OFFSET);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happiness_factors_weights_sum_100() {
        let sum: f32 = Factor::ALL.iter().map(|f| f.weight()).sum();
        assert!((sum - 100.0).abs() < 1e-4);
        // All scores 1 → 100; all 0 → 0.
        let best = Inputs {
            home_level: 5,
            pollution: 0.0,
            noise: 0.0,
            crime: 0.0,
            tax: 0.0,
            land_value: 1.0,
            services: 1.0,
            commute: 1.0,
            ..Inputs::with_stubs()
        };
        assert!((happiness(&best) - 100.0).abs() < 1e-3);
    }

    #[test]
    fn happiness_stub_replacement_keeps_formula() {
        // A real input equal to the stub value gives the same happiness: stubs are inputs.
        let stub = Inputs::with_stubs();
        let real = Inputs {
            services: 0.5,
            crime: 0.2,
            commute: 0.7,
            ..Inputs::with_stubs()
        };
        assert_eq!(happiness(&stub), happiness(&real));
        // And a different real value moves only its own weighted term.
        let better = Inputs {
            services: 1.0,
            ..Inputs::with_stubs()
        };
        let d = happiness(&better) - happiness(&stub);
        assert!((d - 0.5 * Factor::Services.weight()).abs() < 1e-4);
        // Neutral stubs leave a plain served household in the GAME_DESIGN target band.
        assert!(
            (55.0..=80.0).contains(&happiness(&stub)),
            "{}",
            happiness(&stub)
        );
    }
}
