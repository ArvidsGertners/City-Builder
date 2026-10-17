//! Citizens (DATA_MODEL §6, ALGORITHMS §16, ADR-0016): households and citizens live in resource
//! tables, never as entities. Homes and workplaces are buildings; their capacities come from
//! the zone data (`capacity`). Hourly work follows the phase table (ARCHITECTURE §5.3): job
//! matching in 200 slices from tick 500 of the hour, immigration in 4 slices from tick 720.
//!
//! Everything here iterates in stable-ID order and draws randomness from
//! `SimRng::for_event(Citizens, purpose, tick, id)`, so results never depend on ECS order.

pub mod capacity;
pub mod happiness;
pub mod households;
pub mod jobs;

use std::collections::BTreeMap;

use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use cb_core::ids::{BuildingId, CitizenId, HouseholdId, IdAllocator};
use cb_core::time::TICKS_PER_GAME_HOUR;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

pub use capacity::Capacities;

/// Age group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AgeGroup {
    /// Below school age or in elementary school.
    Child,
    /// In secondary or higher education.
    Student,
    /// Working age.
    Adult,
    /// Retired.
    Senior,
}

/// Household wealth.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Wealth {
    /// Low income.
    Low,
    /// Middle income.
    Mid,
    /// High income.
    High,
}

impl Wealth {
    /// All values, in index order.
    pub const ALL: [Self; 3] = [Self::Low, Self::Mid, Self::High];

    /// Index into per-wealth tables.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// One citizen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Citizen {
    pub(crate) household: HouseholdId,
    pub(crate) age: AgeGroup,
    /// 0 none, 1 secondary, 2 higher.
    pub(crate) education: u8,
    /// Workplace and the education level of the job held there.
    pub(crate) job: Option<(BuildingId, u8)>,
}

impl Citizen {
    /// Household.
    #[must_use]
    pub fn household(&self) -> HouseholdId {
        self.household
    }

    /// Age group.
    #[must_use]
    pub fn age(&self) -> AgeGroup {
        self.age
    }

    /// Education level (0–2).
    #[must_use]
    pub fn education(&self) -> u8 {
        self.education
    }

    /// Workplace, if employed.
    #[must_use]
    pub fn job(&self) -> Option<BuildingId> {
        self.job.map(|j| j.0)
    }

    /// Adults look for work; seniors, children and students do not.
    #[must_use]
    pub fn in_labour_force(&self) -> bool {
        self.age == AgeGroup::Adult
    }
}

/// One household.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Household {
    pub(crate) home: BuildingId,
    pub(crate) members: SmallVec<[CitizenId; 5]>,
    pub(crate) wealth: Wealth,
    pub(crate) cars: u8,
    /// 0–100 (neutral 60 until happiness lands, GAME_DESIGN §10).
    pub(crate) happiness: f32,
    pub(crate) moved_in_tick: u64,
    /// Consecutive days below the unhappiness threshold.
    #[serde(default)]
    pub(crate) unhappy_days: u16,
}

impl Household {
    /// Home building.
    #[must_use]
    pub fn home(&self) -> BuildingId {
        self.home
    }

    /// Members in creation order.
    #[must_use]
    pub fn members(&self) -> &[CitizenId] {
        &self.members
    }

    /// Wealth.
    #[must_use]
    pub fn wealth(&self) -> Wealth {
        self.wealth
    }

    /// Cars owned.
    #[must_use]
    pub fn cars(&self) -> u8 {
        self.cars
    }

    /// Happiness 0–100.
    #[must_use]
    pub fn happiness(&self) -> f32 {
        self.happiness
    }

    /// Tick of arrival.
    #[must_use]
    pub fn moved_in_tick(&self) -> u64 {
        self.moved_in_tick
    }
}

/// All households and citizens (saved). Indices by building are derived and rebuilt on load.
#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Population {
    pub(crate) households: BTreeMap<HouseholdId, Household>,
    pub(crate) citizens: BTreeMap<CitizenId, Citizen>,
    pub(crate) household_ids: IdAllocator<HouseholdId>,
    pub(crate) citizen_ids: IdAllocator<CitizenId>,
    /// Home building → households living there.
    #[serde(skip)]
    pub(crate) homes: BTreeMap<BuildingId, SmallVec<[HouseholdId; 4]>>,
    /// Workplace → filled jobs by education level.
    #[serde(skip)]
    pub(crate) filled: BTreeMap<BuildingId, [u16; 3]>,
}

impl Population {
    /// Number of citizens.
    #[must_use]
    pub fn population(&self) -> usize {
        self.citizens.len()
    }

    /// Households in ID order.
    pub fn households(&self) -> impl Iterator<Item = (HouseholdId, &Household)> {
        self.households.iter().map(|(k, v)| (*k, v))
    }

    /// Citizens in ID order.
    pub fn citizens(&self) -> impl Iterator<Item = (CitizenId, &Citizen)> {
        self.citizens.iter().map(|(k, v)| (*k, v))
    }

    /// One household.
    #[must_use]
    pub fn household(&self, id: HouseholdId) -> Option<&Household> {
        self.households.get(&id)
    }

    /// One citizen.
    #[must_use]
    pub fn citizen(&self, id: CitizenId) -> Option<&Citizen> {
        self.citizens.get(&id)
    }

    /// Households living in `building`.
    #[must_use]
    pub fn residents_of(&self, building: BuildingId) -> &[HouseholdId] {
        self.homes.get(&building).map_or(&[], |v| v.as_slice())
    }

    /// Filled jobs at `building` by education level.
    #[must_use]
    pub fn workers_of(&self, building: BuildingId) -> [u16; 3] {
        self.filled.get(&building).copied().unwrap_or_default()
    }

    /// Labour force (adults) and how many of them are employed.
    #[must_use]
    pub fn labour(&self) -> (usize, usize) {
        let mut force = 0;
        let mut employed = 0;
        for c in self.citizens.values().filter(|c| c.in_labour_force()) {
            force += 1;
            employed += usize::from(c.job.is_some());
        }
        (force, employed)
    }

    /// Unemployment rate of the labour force (0 when there is none).
    #[must_use]
    pub fn unemployment(&self) -> f32 {
        let (force, employed) = self.labour();
        if force == 0 {
            0.0
        } else {
            (force - employed) as f32 / force as f32
        }
    }

    /// Rebuilds the per-building indices from the tables (after load).
    pub fn rebuild_indices(&mut self) {
        self.homes.clear();
        self.filled.clear();
        for (id, h) in &self.households {
            self.homes.entry(h.home).or_default().push(*id);
        }
        for c in self.citizens.values() {
            if let Some((b, edu)) = c.job {
                self.filled.entry(b).or_default()[usize::from(edu.min(2))] += 1;
            }
        }
    }

    /// Removes a household and its members (moving out of the city).
    pub(crate) fn remove_household(&mut self, id: HouseholdId) {
        let Some(h) = self.households.remove(&id) else {
            return;
        };
        if let Some(v) = self.homes.get_mut(&h.home) {
            v.retain(|x| *x != id);
            if v.is_empty() {
                self.homes.remove(&h.home);
            }
        }
        for c in &h.members {
            if let Some(cit) = self.citizens.remove(c) {
                self.release_job(&cit);
            }
        }
    }

    /// Frees the job a citizen held (index only).
    fn release_job(&mut self, c: &Citizen) {
        if let Some((b, edu)) = c.job
            && let Some(f) = self.filled.get_mut(&b)
        {
            let slot = &mut f[usize::from(edu.min(2))];
            *slot = slot.saturating_sub(1);
            if f.iter().all(|x| *x == 0) {
                self.filled.remove(&b);
            }
        }
    }

    /// Fires every worker of `building` (it closed or was removed).
    pub(crate) fn close_workplace(&mut self, building: BuildingId) {
        if self.filled.remove(&building).is_none() {
            return;
        }
        for c in self.citizens.values_mut() {
            if c.job.is_some_and(|j| j.0 == building) {
                c.job = None;
            }
        }
    }
}

/// Population statistics for UI and demand (derived each hour).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct PopulationStats {
    /// Citizens.
    pub population: u32,
    /// Households.
    pub households: u32,
    /// Free household slots in habitable homes.
    pub free_homes: u32,
    /// Household slots in habitable homes.
    pub home_slots: u32,
    /// Jobs offered by open workplaces.
    pub jobs: u32,
    /// Jobs filled.
    pub jobs_filled: u32,
    /// Labour force (adults).
    pub labour_force: u32,
    /// Unemployed adults.
    pub unemployed: u32,
}

/// Tick within the game hour.
#[must_use]
pub(crate) fn tick_in_hour(tick: u64) -> u64 {
    tick % TICKS_PER_GAME_HOUR
}

/// `TickSet::SubRate`: the hourly citizen work, dispatched by the phase table.
pub(crate) fn tick(world: &mut World) {
    let now = world
        .get_resource::<crate::SimClock>()
        .map_or(0, |c| c.tick().0);
    let t = tick_in_hour(now);
    if (jobs::OFFSET..jobs::OFFSET + jobs::SLICES).contains(&t) {
        if t == jobs::OFFSET {
            capacity::refresh(world);
            households::evict_uninhabitable(world);
        }
        jobs::match_slice(world, now, t - jobs::OFFSET);
    }
    if (households::OFFSET..households::OFFSET + households::SLICES).contains(&t) {
        if t == households::OFFSET {
            capacity::refresh(world);
            households::evict_uninhabitable(world);
        }
        households::immigrate_slice(world, now, t - households::OFFSET);
        if t + 1 == households::OFFSET + households::SLICES {
            update_stats(world);
        }
    }
}

/// Recomputes [`PopulationStats`].
pub(crate) fn update_stats(world: &mut World) {
    let (Some(pop), Some(cap)) = (
        world.get_resource::<Population>(),
        world.get_resource::<Capacities>(),
    ) else {
        return;
    };
    let mut s = PopulationStats {
        population: u32::try_from(pop.population()).unwrap_or(u32::MAX),
        households: u32::try_from(pop.households.len()).unwrap_or(u32::MAX),
        ..PopulationStats::default()
    };
    for (b, slots) in cap.homes() {
        let used = pop.residents_of(b).len();
        s.home_slots += u32::from(slots);
        s.free_homes += u32::from(slots).saturating_sub(u32::try_from(used).unwrap_or(u32::MAX));
    }
    for (b, jobs) in cap.jobs() {
        let f = pop.workers_of(b);
        for k in 0..3 {
            s.jobs += u32::from(jobs[k]);
            s.jobs_filled += u32::from(f[k].min(jobs[k]));
        }
    }
    let (force, employed) = pop.labour();
    s.labour_force = u32::try_from(force).unwrap_or(u32::MAX);
    s.unemployed = u32::try_from(force - employed).unwrap_or(u32::MAX);
    world.insert_resource(s);
}
