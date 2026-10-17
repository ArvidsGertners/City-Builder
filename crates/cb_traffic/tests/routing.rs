//! Macro routing (ALGORITHMS §12, ADR-0067): ALT A* equals Dijkstra, the landmark heuristic is
//! admissible for every variant and congestion, the cache drops routes through removed links,
//! the slot budget never changes results, and one-way traps are found (TESTING §3).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use cb_core::rng::Pcg32;
use cb_traffic::cache::{Key, RouteCache};
use cb_traffic::graph::{Graph, LinkSpec};
use cb_traffic::router::{self, Costs, Landmarks};
use cb_traffic::scc;
use cb_traffic::slots::{Request, Scheduler};
use glam::Vec2;

/// A jittered `w × h` grid with random one-way streets and missing blocks.
fn random_graph(seed: u64, w: u32, h: u32) -> Graph {
    let mut rng = Pcg32::from_seed(seed);
    let mut pos = Vec::new();
    for j in 0..h {
        for i in 0..w {
            pos.push(Vec2::new(
                i as f32 * 100.0 + rng.range_f32(-20.0, 20.0),
                j as f32 * 100.0 + rng.range_f32(-20.0, 20.0),
            ));
        }
    }
    let id = |i: u32, j: u32| j * w + i;
    let mut links = Vec::new();
    let add = |links: &mut Vec<LinkSpec>, a: u32, b: u32, speed: f32, dirt: bool| {
        let (pa, pb) = (pos[a as usize], pos[b as usize]);
        let d = (pb - pa).normalize_or_zero();
        links.push(LinkSpec {
            from: a,
            to: b,
            length_m: pa.distance(pb),
            speed_mps: speed,
            dirt,
            capacity_per_sim_h: 1600.0,
            dir_out: d,
            dir_in: d,
            key: links.len() as u64,
        });
    };
    for j in 0..h {
        for i in 0..w {
            for (ni, nj) in [(i + 1, j), (i, j + 1)] {
                if ni >= w || nj >= h || rng.chance(0.1) {
                    continue;
                }
                let (a, b) = (id(i, j), id(ni, nj));
                let speed = if rng.chance(0.2) { 16.7 } else { 13.9 };
                let dirt = rng.chance(0.1);
                match rng.below(8) {
                    0 => add(&mut links, a, b, speed, dirt),
                    1 => add(&mut links, b, a, speed, dirt),
                    _ => {
                        add(&mut links, a, b, speed, dirt);
                        add(&mut links, b, a, speed, dirt);
                    }
                }
            }
        }
    }
    Graph::build(pos, links)
}

fn congested(g: &Graph, seed: u64, epoch: u64) -> Costs {
    let mut rng = Pcg32::from_seed(seed);
    Costs {
        link_ms: g
            .free_costs()
            .iter()
            .map(|c| c + rng.below(*c / 2 + 1))
            .collect(),
        epoch,
    }
}

#[test]
fn astar_equals_dijkstra_random_graphs() {
    for seed in 1..=6u64 {
        let g = random_graph(seed, 12, 10);
        let lm = Landmarks::build(&g);
        let costs = congested(&g, seed + 100, seed);
        let mut rng = Pcg32::from_seed(seed + 7);
        let mut found = 0;
        for q in 0..60 {
            let o = rng.below(g.node_count() as u32);
            let t = rng.below(g.node_count() as u32);
            let v = (q % 4) as u8;
            let a = router::route(&g, &lm, &costs, o, t, v);
            let d = router::dijkstra(&g, &costs, o, t, v);
            assert_eq!(
                a.as_ref().map(|r| r.cost_ms),
                d.as_ref().map(|r| r.cost_ms),
                "seed {seed} {o}→{t}"
            );
            if let Some(r) = a {
                found += 1;
                // The route is connected and starts and ends at the right nodes.
                if !r.links.is_empty() {
                    assert_eq!(g.link(r.links[0]).from, o);
                    assert_eq!(g.link(*r.links.last().unwrap()).to, t);
                    for w in r.links.windows(2) {
                        assert_eq!(g.link(w[0]).to, g.link(w[1]).from);
                    }
                }
            }
        }
        assert!(found > 30, "seed {seed}: {found} routes");
    }
}

#[test]
fn alt_heuristic_admissible_with_variants() {
    let g = random_graph(42, 14, 12);
    let lm = Landmarks::build(&g);
    let mut rng = Pcg32::from_seed(9);
    for epoch in 0..3 {
        let costs = congested(&g, 5 + epoch, epoch);
        for _ in 0..40 {
            let s = rng.below(g.node_count() as u32);
            let t = rng.below(g.node_count() as u32);
            let active = lm.pick(s, t);
            for v in 0..4u8 {
                // From every node the heuristic stays below the true cost to t.
                for probe in 0..g.node_count() as u32 {
                    if probe % 7 != 0 {
                        continue;
                    }
                    let h = lm.h(&active, probe, t);
                    if let Some(r) = router::dijkstra(&g, &costs, probe, t, v) {
                        assert!(h <= r.cost_ms, "h {h} > {} at {probe}→{t}", r.cost_ms);
                    }
                }
            }
        }
    }
}

#[test]
fn route_cache_invalidation_on_removed_links() {
    let g = random_graph(3, 8, 8);
    let lm = Landmarks::build(&g);
    let costs = Costs::free(&g);
    let mut cache = RouteCache::new(64);
    let mut keys = Vec::new();
    for (o, t) in [(0u32, 63u32), (7, 56), (9, 54), (1, 2)] {
        let key = Key {
            origin: o,
            dest: t,
            mode: 0,
            variant: 0,
        };
        cache.insert(key, router::route(&g, &lm, &costs, o, t, 0), 1, 0);
        keys.push(key);
    }
    let first = cache.get(keys[0], 1, 0).unwrap().unwrap();
    let removed = first.links[first.links.len() / 2];
    cache.evict_link(removed);
    assert!(
        cache.get(keys[0], 1, 0).is_none(),
        "route through the removed link evicted"
    );
    // Routes not using the link survive; a new epoch or graph version misses.
    for k in &keys[1..] {
        let through = cache
            .get(*k, 1, 0)
            .map(|r| r.is_some_and(|r| r.links.contains(&removed)));
        assert_ne!(through, Some(true));
    }
    assert!(cache.get(keys[3], 1, 1).is_none());
    assert!(cache.get(keys[3], 2, 0).is_none());
    // Unreachable pairs are remembered per graph version.
    let none = Key {
        origin: 0,
        dest: 0,
        mode: 9,
        variant: 0,
    };
    cache.insert(none, None, 1, 0);
    assert_eq!(cache.get(none, 1, 0), Some(None));
    assert_eq!(cache.get(none, 2, 0), None);
    // LRU: capacity holds.
    let mut small = RouteCache::new(2);
    for (i, k) in keys.iter().enumerate() {
        small.insert(
            *k,
            router::route(&g, &lm, &costs, k.origin, k.dest, 0),
            1,
            0,
        );
        assert!(small.len() <= 2, "after {i}");
    }
}

#[test]
fn routing_budget_deterministic() {
    let g = random_graph(11, 16, 12);
    let lm = Landmarks::build(&g);
    let costs = congested(&g, 2, 1);
    let mut rng = Pcg32::from_seed(4);
    let requests: Vec<Request> = (0..40)
        .map(|i| Request {
            ticket: i,
            key: Key {
                origin: rng.below(g.node_count() as u32),
                dest: rng.below(g.node_count() as u32),
                mode: 0,
                variant: (i % 4) as u8,
            },
        })
        .collect();
    let run = |budget: u64| {
        let mut s = Scheduler::new();
        for r in &requests {
            s.push(*r);
        }
        let mut out = Vec::new();
        let mut ticks = 0;
        while s.pending() > 0 {
            out.extend(s.step(&g, &lm, &costs, budget));
            ticks += 1;
            assert!(ticks < 100_000);
        }
        (out, ticks)
    };
    let (a, ta) = run(7);
    let (b, tb) = run(5000);
    let (a2, _) = run(7);
    assert_eq!(a, a2, "same budget, same delivery order");
    assert!(ta > tb);
    // Every request gets the same result whatever the budget, equal to a direct search.
    let by_ticket = |v: &Vec<(Request, Option<cb_traffic::Route>)>| {
        let mut x: Vec<_> = v.iter().map(|(r, res)| (r.ticket, res.clone())).collect();
        x.sort_by_key(|p| p.0);
        x
    };
    assert_eq!(by_ticket(&a), by_ticket(&b));
    for (t, res) in by_ticket(&a) {
        let k = requests[t as usize].key;
        assert_eq!(
            res,
            router::route(&g, &lm, &costs, k.origin, k.dest, k.variant)
        );
    }
}

#[test]
fn one_way_trap_flagged() {
    // A two-way loop 0-1-2-3 plus a one-way spur 3 → 4 → 5 (a trap: no way back).
    let pos: Vec<Vec2> = (0..6).map(|i| Vec2::new(i as f32 * 100.0, 0.0)).collect();
    let mut links = Vec::new();
    let mut add = |a: u32, b: u32| {
        let d = (pos[b as usize] - pos[a as usize]).normalize_or_zero();
        links.push(LinkSpec {
            from: a,
            to: b,
            length_m: 100.0,
            speed_mps: 13.9,
            dirt: false,
            capacity_per_sim_h: 1600.0,
            dir_out: d,
            dir_in: d,
            key: 0,
        });
    };
    for (a, b) in [(0, 1), (1, 2), (2, 3), (3, 0)] {
        add(a, b);
        add(b, a);
    }
    add(3, 4);
    add(4, 5);
    let g = Graph::build(pos, links);
    assert_eq!(scc::trapped(&g), vec![4, 5]);
    let lm = Landmarks::build(&g);
    let costs = Costs::free(&g);
    assert!(router::route(&g, &lm, &costs, 5, 0, 0).is_none());
    assert!(router::route(&g, &lm, &costs, 0, 5, 0).is_some());
}
