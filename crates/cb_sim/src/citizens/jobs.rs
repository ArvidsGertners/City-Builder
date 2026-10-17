//! Job matching (ALGORITHMS §16), sliced over 200 ticks from tick 500 of each game hour. Slice
//! `k` handles the unemployed adults whose `CitizenId % 200 == k`, in `(education desc, id)`
//! order. Each takes the workplace with an open job it qualifies for that has the best score
//! `−routed estimate − 0.2 × level mismatch × max commute`, preferring jobs at its own education
//! level. Estimates come from one bounded tree per seeker home under the day costs
//! (`traffic::Traffic::estimates`); workplaces beyond the maximum commute (300 sim s) or without a
//! route are never offered. A hire samples the shift start and stores the estimate.

use std::collections::BTreeMap;

use bevy::ecs::world::{Mut, World};
use cb_core::ids::{BuildingId, CitizenId};

use super::{Capacities, Population};
use crate::catalog::Catalog;
use crate::config::WorldSeed;
use crate::traffic::{self, Traffic};

/// First tick of the job-matching pass in the game hour (phase table).
pub const OFFSET: u64 = 500;
/// Job-matching slices per hour.
pub const SLICES: u64 = 200;

/// Weight of one level of over-qualification, as a fraction of the maximum commute.
const MISMATCH_WEIGHT: f32 = 0.2;

/// One job-matching slice.
pub(crate) fn match_slice(world: &mut World, now: u64, slice: u64) {
    let Some(spec) = world
        .get_resource::<Catalog>()
        .map(|c| c.citizens().commute)
    else {
        return;
    };
    let seed = world.get_resource::<WorldSeed>().map_or(0, |s| s.0);
    let (Some(cap), Some(pop)) = (
        world.get_resource::<Capacities>(),
        world.get_resource::<Population>(),
    ) else {
        return;
    };
    let mut seekers: Vec<(u8, CitizenId, BuildingId)> = pop
        .citizens
        .iter()
        .filter(|(id, c)| {
            c.in_labour_force() && c.job.is_none() && u64::from(id.get()) % SLICES == slice
        })
        .filter_map(|(id, c)| {
            let home = pop.households.get(&c.household)?.home;
            Some((c.education, *id, home))
        })
        .collect();
    if seekers.is_empty() {
        return;
    }
    seekers.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    // Open jobs per workplace and level, in ID order.
    let mut open: Vec<(BuildingId, [u16; 3])> = cap
        .jobs()
        .filter_map(|(b, jobs)| {
            let f = pop.workers_of(b);
            let o = [
                jobs[0].saturating_sub(f[0]),
                jobs[1].saturating_sub(f[1]),
                jobs[2].saturating_sub(f[2]),
            ];
            (o != [0; 3]).then_some((b, o))
        })
        .collect();
    if open.is_empty() {
        return;
    }
    let zones: Vec<_> = open.iter().map(|(b, _)| cap.zone(*b)).collect();
    let work: Vec<_> = open
        .iter()
        .map(|(b, _)| traffic::access(world, *b))
        .collect();
    let homes: BTreeMap<BuildingId, Option<traffic::Access>> = seekers
        .iter()
        .map(|s| (s.2, traffic::access(world, s.2)))
        .collect();
    // Estimates per seeker home (one bounded tree each).
    let mut est: BTreeMap<BuildingId, Vec<Option<f32>>> = BTreeMap::new();
    if world.get_resource::<Traffic>().is_none() {
        return;
    }
    world.resource_scope(|_, mut t: Mut<Traffic>| {
        for (home, acc) in &homes {
            let e = acc.map_or_else(|| vec![None; work.len()], |a| t.estimates(a, &work, &spec));
            est.insert(*home, e);
        }
    });
    let mut hires: Vec<(CitizenId, BuildingId, u8, u16, u16)> = Vec::new();
    for (edu, id, home) in seekers {
        let Some(e) = est.get(&home) else {
            continue;
        };
        let mut best: Option<(f32, usize, u8)> = None;
        for (i, (_, o)) in open.iter().enumerate() {
            let Some(sim_s) = e[i] else {
                continue;
            };
            for lvl in (0..=edu).rev() {
                if o[usize::from(lvl)] == 0 {
                    continue;
                }
                let score = -sim_s - MISMATCH_WEIGHT * f32::from(edu - lvl) * spec.max_sim_s;
                if best.is_none_or(|b| score > b.0) {
                    best = Some((score, i, lvl));
                }
                break;
            }
        }
        if let Some((_, i, lvl)) = best {
            open[i].1[usize::from(lvl)] -= 1;
            let shift = traffic::sample_shift(traffic::shift_spec(&spec, zones[i]), seed, now, id);
            let commute = e[i].map_or(0, |s| (s.round() as u16).max(1));
            hires.push((id, open[i].0, lvl, shift, commute));
        }
    }
    let Some(mut pop) = world.get_resource_mut::<Population>() else {
        return;
    };
    for (id, b, lvl, shift, commute) in hires {
        if let Some(c) = pop.citizens.get_mut(&id) {
            c.job = Some((b, lvl));
            c.shift_start_min = shift;
            c.commute_sim_s = commute;
        }
        pop.filled.entry(b).or_default()[usize::from(lvl)] += 1;
    }
}
