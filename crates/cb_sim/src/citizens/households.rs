//! Households moving in and out (ALGORITHMS §16). Immigration runs in 4 slices from tick 720 of
//! each game hour: `ceil(max(R_imm, 0) × base × (1 + pop / scale))` households per hour, capped
//! by free slots, each placed in a home drawn by free slots. Homes that stop being habitable
//! (removed, abandoned, ruined) send their households out of the city.

use bevy::ecs::world::World;
use cb_core::ids::{BuildingId, CitizenId, HouseholdId};
use cb_core::rng::{Pcg32, RngStream, SimRng};
use smallvec::SmallVec;

use super::{AgeGroup, Capacities, Citizen, Household, Population, Wealth};
use crate::catalog::{Catalog, CitizensSpec};
use crate::config::WorldSeed;

/// First tick of the immigration pass in the game hour (phase table).
pub const OFFSET: u64 = 720;
/// Immigration slices per hour.
pub const SLICES: u64 = 4;
/// Neutral happiness of a new household.
pub const NEUTRAL_HAPPINESS: f32 = 60.0;

/// `SimRng` purposes of this module.
mod purpose {
    pub const HOME: u16 = 1;
    pub const MAKEUP: u16 = 2;
}

/// Households whose home no longer offers slots (or holds more than it offers) move out,
/// newest first, so the home ends within capacity.
pub(crate) fn evict_uninhabitable(world: &mut World) {
    let Some(cap) = world.get_resource::<Capacities>() else {
        return;
    };
    let Some(pop) = world.get_resource::<Population>() else {
        return;
    };
    let mut leaving: Vec<HouseholdId> = Vec::new();
    for (home, hs) in &pop.homes {
        let slots = usize::from(cap.home_slots(*home));
        if hs.len() > slots {
            let mut sorted: Vec<HouseholdId> = hs.to_vec();
            sorted.sort_unstable();
            leaving.extend(sorted.into_iter().skip(slots));
        }
    }
    // Workplaces that closed lose their workers.
    let closed: Vec<BuildingId> = pop
        .filled
        .keys()
        .copied()
        .filter(|b| cap.jobs_of(*b) == [0; 3])
        .collect();
    let over: Vec<(BuildingId, [u16; 3])> = pop
        .filled
        .iter()
        .filter_map(|(b, f)| {
            let j = cap.jobs_of(*b);
            (j != [0; 3] && (0..3).any(|k| f[k] > j[k])).then_some((*b, j))
        })
        .collect();
    let Some(mut pop) = world.get_resource_mut::<Population>() else {
        return;
    };
    for h in leaving {
        pop.remove_household(h);
    }
    for b in closed {
        pop.close_workplace(b);
    }
    for (b, jobs) in over {
        shrink_workplace(&mut pop, b, jobs);
    }
}

/// Lays off the newest workers above the jobs a workplace now offers.
fn shrink_workplace(pop: &mut Population, b: BuildingId, jobs: [u16; 3]) {
    for edu in 0..3u8 {
        let mut held: Vec<CitizenId> = pop
            .citizens
            .iter()
            .filter(|(_, c)| c.job == Some((b, edu)))
            .map(|(id, _)| *id)
            .collect();
        let keep = usize::from(jobs[usize::from(edu)]);
        if held.len() <= keep {
            continue;
        }
        held.sort_unstable();
        for id in held.split_off(keep) {
            if let Some(c) = pop.citizens.get_mut(&id) {
                c.job = None;
            }
        }
    }
    pop.filled.insert(b, {
        let mut f = pop.filled.get(&b).copied().unwrap_or_default();
        for k in 0..3 {
            f[k] = f[k].min(jobs[k]);
        }
        f
    });
}

/// Households arriving this game hour.
#[must_use]
pub fn arrivals_per_hour(r_immigration: f32, population: usize, spec: &CitizensSpec) -> u32 {
    let r = r_immigration.max(0.0);
    if r <= 0.0 {
        return 0;
    }
    let scale = 1.0 + population as f32 / spec.immigration_pop_scale.max(1.0);
    (r * spec.immigration_base_per_game_h * scale).ceil() as u32
}

/// One immigration slice: this slice's share of the hourly arrivals moves in.
pub(crate) fn immigrate_slice(world: &mut World, now: u64, slice: u64) {
    let Some(spec) = world.get_resource::<Catalog>().map(|c| *c.citizens()) else {
        return;
    };
    let seed = world.get_resource::<WorldSeed>().map_or(0, |s| s.0);
    let r = world
        .get_resource::<crate::demand::Demand>()
        .map_or(0.0, crate::demand::Demand::immigration);
    let Some(cap) = world.get_resource::<Capacities>() else {
        return;
    };
    let Some(pop) = world.get_resource::<Population>() else {
        return;
    };
    let total = u64::from(arrivals_per_hour(r, pop.population(), &spec));
    let n = total / SLICES + u64::from(slice < total % SLICES);
    if n == 0 {
        return;
    }
    // Free slots per home, in ID order; drawn by weight = free slots.
    let mut free: Vec<(BuildingId, u16, u8)> = cap
        .homes()
        .filter_map(|(b, slots)| {
            let used = u16::try_from(pop.residents_of(b).len()).unwrap_or(u16::MAX);
            let level = cap.level(b).unwrap_or(1);
            (slots > used).then_some((b, slots - used, level))
        })
        .collect();
    let mut arrivals: Vec<(BuildingId, u8)> = Vec::new();
    for k in 0..n {
        let weights: Vec<f32> = free.iter().map(|(_, f, _)| f32::from(*f)).collect();
        let mut rng = SimRng::for_event(seed, RngStream::Citizens, purpose::HOME, now, k);
        let Some(i) = rng.weighted_index(&weights) else {
            break;
        };
        arrivals.push((free[i].0, free[i].2));
        free[i].1 -= 1;
        if free[i].1 == 0 {
            free.remove(i);
        }
    }
    let Some(mut pop) = world.get_resource_mut::<Population>() else {
        return;
    };
    for (home, level) in arrivals {
        move_in(&mut pop, &spec, seed, now, home, level);
    }
}

/// Creates a household in `home` (makeup drawn from the citizens data).
pub(crate) fn move_in(
    pop: &mut Population,
    spec: &CitizensSpec,
    seed: u64,
    now: u64,
    home: BuildingId,
    level: u8,
) -> Option<HouseholdId> {
    let hid = pop.household_ids.allocate().ok()?;
    let mut rng = SimRng::for_event(
        seed,
        RngStream::Citizens,
        purpose::MAKEUP,
        now,
        u64::from(hid.get()),
    );
    let size = 1 + rng
        .weighted_index(&spec.household_size_weights)
        .unwrap_or(0);
    let senior = size <= 2 && rng.chance(spec.senior_household_p);
    let wealth = Wealth::ALL[rng
        .weighted_index(&spec.wealth_by_level[usize::from(level.clamp(1, 5)) - 1])
        .unwrap_or(1)];
    let cars = stochastic_round(spec.cars_by_wealth[wealth.index()], &mut rng);
    let mut members: SmallVec<[CitizenId; 5]> = SmallVec::new();
    for k in 0..size {
        let age = match k {
            _ if senior => AgeGroup::Senior,
            0 => AgeGroup::Adult,
            1 if rng.chance(spec.second_adult_p) => AgeGroup::Adult,
            _ if rng.chance(0.35) => AgeGroup::Student,
            _ => AgeGroup::Child,
        };
        let education = match age {
            AgeGroup::Adult | AgeGroup::Senior => u8::try_from(
                rng.weighted_index(&spec.adult_education_weights)
                    .unwrap_or(0),
            )
            .unwrap_or(0),
            AgeGroup::Student => 1,
            AgeGroup::Child => 0,
        };
        let Ok(cid) = pop.citizen_ids.allocate() else {
            break;
        };
        pop.citizens.insert(
            cid,
            Citizen {
                household: hid,
                age,
                education,
                job: None,
                shift_start_min: 0,
                commute_sim_s: 0,
            },
        );
        members.push(cid);
    }
    pop.households.insert(
        hid,
        Household {
            home,
            members,
            wealth,
            cars,
            happiness: NEUTRAL_HAPPINESS,
            moved_in_tick: now,
            unhappy_days: 0,
        },
    );
    pop.homes.entry(home).or_default().push(hid);
    Some(hid)
}

fn stochastic_round(x: f32, rng: &mut Pcg32) -> u8 {
    let base = x.floor();
    let extra = u8::from(rng.chance(x - base));
    (base as u8).saturating_add(extra)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrivals_scale_with_demand_and_population() {
        let spec = *Catalog::load().unwrap().citizens();
        assert_eq!(arrivals_per_hour(0.0, 0, &spec), 0);
        assert_eq!(arrivals_per_hour(-0.5, 100, &spec), 0);
        assert_eq!(arrivals_per_hour(1.0, 0, &spec), 6);
        assert_eq!(arrivals_per_hour(1.0, 1500, &spec), 12);
        assert_eq!(arrivals_per_hour(0.01, 0, &spec), 1);
    }
}
