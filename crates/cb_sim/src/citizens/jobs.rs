//! Job matching (ALGORITHMS §16), sliced over 200 ticks from tick 500 of each game hour. Slice
//! `k` handles the unemployed adults whose `CitizenId % 200 == k`, in `(education desc, id)`
//! order. Each takes the nearest workplace with an open job it qualifies for, preferring jobs
//! at its own education level (score = −distance − 0.2 × level mismatch × search radius), within
//! the straight-line search radius until macro routing estimates commutes (M5.7).

use bevy::ecs::world::World;
use cb_core::ids::{BuildingId, CitizenId};
use glam::Vec2;

use super::{Capacities, Population};
use crate::catalog::Catalog;

/// First tick of the job-matching pass in the game hour (phase table).
pub const OFFSET: u64 = 500;
/// Job-matching slices per hour.
pub const SLICES: u64 = 200;

/// Weight of one level of over-qualification, as a fraction of the search radius.
const MISMATCH_WEIGHT: f32 = 0.2;

/// One job-matching slice.
pub(crate) fn match_slice(world: &mut World, _now: u64, slice: u64) {
    let radius = world
        .get_resource::<Catalog>()
        .map_or(4000.0, |c| c.citizens().job_search_radius_m);
    let (Some(cap), Some(pop)) = (
        world.get_resource::<Capacities>(),
        world.get_resource::<Population>(),
    ) else {
        return;
    };
    let mut seekers: Vec<(u8, CitizenId, Vec2)> = pop
        .citizens
        .iter()
        .filter(|(id, c)| {
            c.in_labour_force() && c.job.is_none() && u64::from(id.get()) % SLICES == slice
        })
        .filter_map(|(id, c)| {
            let home = pop.households.get(&c.household)?.home;
            Some((c.education, *id, cap.pos(home)?))
        })
        .collect();
    if seekers.is_empty() {
        return;
    }
    seekers.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    // Open jobs per workplace and level, in ID order.
    let mut open: Vec<(BuildingId, Vec2, [u16; 3])> = cap
        .jobs()
        .filter_map(|(b, jobs)| {
            let f = pop.workers_of(b);
            let o = [
                jobs[0].saturating_sub(f[0]),
                jobs[1].saturating_sub(f[1]),
                jobs[2].saturating_sub(f[2]),
            ];
            (o != [0; 3]).then_some((b, cap.pos(b)?, o))
        })
        .collect();
    let mut hires: Vec<(CitizenId, BuildingId, u8)> = Vec::new();
    for (edu, id, home) in seekers {
        let mut best: Option<(f32, usize, u8)> = None;
        for (i, (_, pos, o)) in open.iter().enumerate() {
            let d = pos.distance(home);
            if d > radius {
                continue;
            }
            for lvl in (0..=edu).rev() {
                if o[usize::from(lvl)] == 0 {
                    continue;
                }
                let score = -d - MISMATCH_WEIGHT * f32::from(edu - lvl) * radius;
                if best.is_none_or(|b| score > b.0) {
                    best = Some((score, i, lvl));
                }
                break;
            }
        }
        if let Some((_, i, lvl)) = best {
            open[i].2[usize::from(lvl)] -= 1;
            hires.push((id, open[i].0, lvl));
        }
    }
    let Some(mut pop) = world.get_resource_mut::<Population>() else {
        return;
    };
    for (id, b, lvl) in hires {
        if let Some(c) = pop.citizens.get_mut(&id) {
            c.job = Some((b, lvl));
        }
        pop.filled.entry(b).or_default()[usize::from(lvl)] += 1;
    }
}
