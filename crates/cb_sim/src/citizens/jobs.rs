//! Job matching (ALGORITHMS §16, ADR-0068).
//!
//! **Job map** (ticks 497–499, one job level per tick): a reverse multi-source search from every
//! workplace with open jobs of that level labels each road node with its nearest such workplace
//! and the routed cost from there, bounded by the maximum commute (300 sim s including both
//! trip-end terminals). The map is keyed by road node id and saved, so it survives graph
//! rebuilds and loads.
//!
//! **Matching** (200 slices from tick 500): slice `k` handles the unemployed adults whose
//! `CitizenId % 200 == k`, in `(education desc, id)` order. Each looks up, for every job level
//! up to its education, the nearest workplace from its home's road access; the score is
//! `−estimate − 0.2 × level mismatch × max commute`, and the best candidate that still has an
//! open job of that level hires it. A seeker whose candidates filled up this hour waits for the
//! next map. A hire samples the shift start and stores the estimate.

use std::collections::BTreeMap;

use bevy::ecs::resource::Resource;
use bevy::ecs::world::{Mut, World};
use cb_core::ids::{BuildingId, CitizenId, NodeId};
use serde::{Deserialize, Serialize};

use super::{Capacities, Population};
use crate::catalog::Catalog;
use crate::config::WorldSeed;
use crate::traffic::{self, Traffic};

/// First tick of the job-matching pass in the game hour (phase table).
pub const OFFSET: u64 = 500;
/// Job-matching slices per hour.
pub const SLICES: u64 = 200;
/// First tick of the job map (one tick per job level).
pub const MAP_OFFSET: u64 = OFFSET - 3;

/// Weight of one level of over-qualification, as a fraction of the maximum commute.
const MISMATCH_WEIGHT: f32 = 0.2;

/// Nearest open workplace per road node and job level (saved).
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct JobMap {
    /// Per level: `(node, cost ms from the node to the workplace's door, workplace)`, by node.
    levels: [Vec<(NodeId, u64, BuildingId)>; 3],
}

impl JobMap {
    fn lookup(&self, level: usize, node: NodeId) -> Option<(u64, BuildingId)> {
        let v = self.levels.get(level)?;
        v.binary_search_by_key(&node, |e| e.0)
            .ok()
            .map(|i| (v[i].1, v[i].2))
    }
}

fn open_jobs(cap: &Capacities, pop: &Population) -> Vec<(BuildingId, [u16; 3])> {
    cap.jobs()
        .filter_map(|(b, jobs)| {
            let f = pop.workers_of(b);
            let o = [
                jobs[0].saturating_sub(f[0]),
                jobs[1].saturating_sub(f[1]),
                jobs[2].saturating_sub(f[2]),
            ];
            (o != [0; 3]).then_some((b, o))
        })
        .collect()
}

/// Builds the job map of one level.
pub(crate) fn build_map_level(world: &mut World, level: usize) {
    let Some(spec) = world
        .get_resource::<Catalog>()
        .map(|c| c.citizens().commute)
    else {
        return;
    };
    let (Some(cap), Some(pop)) = (
        world.get_resource::<Capacities>(),
        world.get_resource::<Population>(),
    ) else {
        return;
    };
    let workplaces: Vec<BuildingId> = open_jobs(cap, pop)
        .into_iter()
        .filter(|(_, o)| o[level] > 0)
        .map(|(b, _)| b)
        .collect();
    if world.get_resource::<Traffic>().is_none() {
        return;
    }
    let bound_ms = ((spec.max_sim_s - 2.0 * spec.terminal_sim_s).max(0.0) * 1000.0) as u64;
    let entries = world.resource_scope(|world, mut t: Mut<Traffic>| {
        let mut sources = Vec::new();
        for (label, b) in workplaces.iter().enumerate() {
            if let Some(a) = t.access_of(world, *b) {
                for (node, extra) in t.dests(a) {
                    sources.push((node, extra, label as u32));
                }
            }
        }
        let near = cb_traffic::tree::nearest_reverse(t.graph(), t.day_costs(), &sources, bound_ms);
        near.iter()
            .enumerate()
            .filter_map(|(i, n)| {
                let (c, label) = (*n)?;
                Some((t.node_id(i as u32)?, c, *workplaces.get(label as usize)?))
            })
            .collect::<Vec<_>>()
    });
    if let Some(mut m) = world.get_resource_mut::<JobMap>() {
        m.levels[level] = entries;
    }
}

/// One job-matching slice.
pub(crate) fn match_slice(world: &mut World, now: u64, slice: u64) {
    let Some(spec) = world
        .get_resource::<Catalog>()
        .map(|c| c.citizens().commute)
    else {
        return;
    };
    let seed = world.get_resource::<WorldSeed>().map_or(0, |s| s.0);
    let Some(pop) = world.get_resource::<Population>() else {
        return;
    };
    // Citizens of the slice by direct id lookup (ids `slice`, `slice + 200`, …).
    let mut seekers: Vec<(u8, CitizenId, BuildingId)> = Vec::new();
    let mut raw = slice;
    while raw < u64::from(pop.citizen_ids.peek_next()) {
        if let Some((id, c)) = u32::try_from(raw)
            .ok()
            .and_then(|r| CitizenId::try_new(r).ok())
            .and_then(|id| Some((id, pop.citizens.get(&id)?)))
            && c.in_labour_force()
            && c.job.is_none()
            && let Some(h) = pop.households.get(&c.household)
        {
            seekers.push((c.education, id, h.home));
        }
        raw += SLICES;
    }
    if seekers.is_empty() {
        return;
    }
    seekers.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    if world.get_resource::<Traffic>().is_none() {
        return;
    }
    let map = world.get_resource::<JobMap>().cloned().unwrap_or_default();
    let penalty_ms = f64::from(MISMATCH_WEIGHT * spec.max_sim_s) * 1000.0;
    let max_ms = (f64::from(spec.max_sim_s - 2.0 * spec.terminal_sim_s) * 1000.0) as u64;
    let mut hires: Vec<(CitizenId, BuildingId, u8, u16, u16)> = Vec::new();
    world.resource_scope(|world, mut t: Mut<Traffic>| {
        let (Some(cap), Some(pop)) = (
            world.get_resource::<Capacities>(),
            world.get_resource::<Population>(),
        ) else {
            return;
        };
        // Open jobs are checked lazily per candidate: offered − filled − hired in this slice.
        let mut taken: BTreeMap<(BuildingId, u8), u16> = BTreeMap::new();
        let open_of = |w: BuildingId, lvl: u8, taken: &BTreeMap<(BuildingId, u8), u16>| {
            let l = usize::from(lvl);
            cap.jobs_of(w)[l]
                .saturating_sub(pop.workers_of(w)[l])
                .saturating_sub(taken.get(&(w, lvl)).copied().unwrap_or(0))
        };
        for (edu, id, home) in seekers {
            let Some(a) = t.access_of(world, home) else {
                continue;
            };
            let origins = t.origins(a);
            // Candidates: the nearest workplace of every level up to the seeker's education.
            let mut cands: Vec<(f64, u8, u64, BuildingId)> = Vec::new();
            for lvl in 0..=edu.min(2) {
                let best = origins
                    .iter()
                    .filter_map(|(node, extra)| {
                        let (c, w) = map.lookup(usize::from(lvl), t.node_id(*node)?)?;
                        Some((c + extra, w))
                    })
                    .min();
                if let Some((c, w)) = best.filter(|(c, _)| *c <= max_ms) {
                    let score = c as f64 + penalty_ms * f64::from(edu - lvl);
                    cands.push((score, lvl, c, w));
                }
            }
            cands.sort_by(|x, y| x.0.total_cmp(&y.0).then(y.1.cmp(&x.1)).then(x.3.cmp(&y.3)));
            for (_, lvl, c, w) in cands {
                if open_of(w, lvl, &taken) == 0 {
                    continue;
                }
                *taken.entry((w, lvl)).or_default() += 1;
                let zone = cap.zone(w);
                let shift = traffic::sample_shift(traffic::shift_spec(&spec, zone), seed, now, id);
                let sim_s = c as f32 / 1000.0 + 2.0 * spec.terminal_sim_s;
                hires.push((id, w, lvl, shift, (sim_s.round() as u16).max(1)));
                break;
            }
        }
    });
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
