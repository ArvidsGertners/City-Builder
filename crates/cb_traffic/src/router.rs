//! A* with ALT landmarks over links (ALGORITHMS §12, ADR-0067).
//!
//! A search goes from a node to a node: it starts on the links leaving the origin and ends when
//! a link entering the destination is settled. Link costs are the epoch's congested costs
//! (never below free flow) perturbed per route variant by +0…+20 % from
//! `hash(link, variant, epoch)`; landmark distances are computed on free-flow node distances,
//! so the heuristic stays admissible for every variant and epoch. Ties break by
//! `(f, −g, link)`. Searches are resumable: [`Search::advance`] settles at most a budget of
//! links and can be continued on a later tick.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::graph::Graph;

/// Landmarks per graph.
pub const LANDMARKS: usize = 16;
/// Landmarks used per query.
pub const ACTIVE: usize = 4;
/// Unreachable.
pub const INF: u64 = u64::MAX / 4;

/// A route: links in order and the total cost (ms).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    /// Links from the origin to the destination.
    pub links: Vec<u32>,
    /// Cost (ms).
    pub cost_ms: u64,
}

/// Multiplier (per mille, 1000–1200) of a link for a route variant in an epoch.
#[must_use]
pub fn variant_permille(link: u32, variant: u8, epoch: u64) -> u64 {
    let h = cb_core::hash::derive_seed(
        u64::from(link),
        cb_core::hash::Domain::Event,
        u64::from(variant),
        epoch,
    );
    1000 + h % 201
}

/// Landmark distances on the free-flow node graph.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Landmarks {
    /// `from[k][v]` = distance landmark k → v; `to[k][v]` = v → landmark k.
    from: Vec<Vec<u64>>,
    to: Vec<Vec<u64>>,
}

fn node_dijkstra(g: &Graph, start: u32, forward: bool) -> Vec<u64> {
    let mut dist = vec![INF; g.node_count()];
    let mut heap = BinaryHeap::new();
    dist[start as usize] = 0;
    heap.push(Reverse((0u64, start)));
    while let Some(Reverse((d, v))) = heap.pop() {
        if d > dist[v as usize] {
            continue;
        }
        let links = if forward {
            g.out_links(v)
        } else {
            g.in_links(v)
        };
        for &l in links {
            let link = g.link(l);
            let w = if forward { link.to } else { link.from };
            let nd = d + u64::from(g.free_ms(l));
            if nd < dist[w as usize] {
                dist[w as usize] = nd;
                heap.push(Reverse((nd, w)));
            }
        }
    }
    dist
}

impl Landmarks {
    /// Farthest-point landmark selection (deterministic: ties by node index).
    #[must_use]
    pub fn build(g: &Graph) -> Self {
        let n = g.node_count();
        let mut out = Self::default();
        if n == 0 {
            return out;
        }
        let mut best_min = vec![INF; n];
        let mut next: u32 = 0;
        for _ in 0..LANDMARKS.min(n) {
            let f = node_dijkstra(g, next, true);
            let t = node_dijkstra(g, next, false);
            for v in 0..n {
                let d = f[v].min(INF).saturating_add(t[v].min(INF));
                best_min[v] = best_min[v].min(if f[v] >= INF && t[v] >= INF { 0 } else { d });
            }
            out.from.push(f);
            out.to.push(t);
            next = (0..n as u32)
                .max_by(|a, b| {
                    best_min[*a as usize]
                        .cmp(&best_min[*b as usize])
                        .then(b.cmp(a))
                })
                .unwrap_or(0);
        }
        out
    }

    /// Lower bound of the distance `v → t` from landmark `k`.
    fn bound(&self, k: usize, v: u32, t: u32) -> u64 {
        let (fv, ft) = (self.from[k][v as usize], self.from[k][t as usize]);
        let (tv, tt) = (self.to[k][v as usize], self.to[k][t as usize]);
        let mut b = 0;
        if ft < INF {
            if fv < INF {
                b = b.max(ft.saturating_sub(fv));
            }
        } else if fv < INF {
            // L reaches v but not t, so v cannot reach t.
            return INF;
        }
        if tv < INF && tt < INF {
            b = b.max(tv.saturating_sub(tt));
        }
        b
    }

    /// The `ACTIVE` landmarks with the largest bound for `s → t` (ties by index).
    #[must_use]
    pub fn pick(&self, s: u32, t: u32) -> Vec<usize> {
        let mut ks: Vec<(u64, usize)> = (0..self.from.len())
            .map(|k| (self.bound(k, s, t), k))
            .collect();
        ks.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        ks.into_iter().take(ACTIVE).map(|x| x.1).collect()
    }

    /// Heuristic for node `v` to target `t` with the active landmarks.
    #[must_use]
    pub fn h(&self, active: &[usize], v: u32, t: u32) -> u64 {
        active
            .iter()
            .map(|k| self.bound(*k, v, t))
            .max()
            .unwrap_or(0)
    }
}

/// Costs of one epoch.
#[derive(Clone, Debug, PartialEq)]
pub struct Costs {
    /// Per link (ms), never below free flow.
    pub link_ms: Vec<u32>,
    /// Epoch number (variants change per epoch).
    pub epoch: u64,
}

impl Costs {
    /// Free-flow costs.
    #[must_use]
    pub fn free(g: &Graph) -> Self {
        Self {
            link_ms: g.free_costs().to_vec(),
            epoch: 0,
        }
    }

    /// Cost of entering link `l` on route variant `variant`.
    #[must_use]
    pub fn enter(&self, l: u32, variant: u8) -> u64 {
        u64::from(self.link_ms[l as usize]) * variant_permille(l, variant, self.epoch) / 1000
    }
}

/// A resumable search.
#[derive(Clone, Debug)]
pub struct Search {
    target: u32,
    variant: u8,
    active: Vec<usize>,
    g: Vec<u64>,
    parent: Vec<u32>,
    closed: Vec<bool>,
    heap: BinaryHeap<Reverse<(u64, Reverse<u64>, u32)>>,
    /// Links settled so far.
    pub settled: u64,
    result: Option<Option<Route>>,
}

impl Search {
    /// Starts a search `origin → target` (node indices).
    #[must_use]
    pub fn new(
        g: &Graph,
        lm: &Landmarks,
        costs: &Costs,
        origin: u32,
        target: u32,
        variant: u8,
        use_alt: bool,
    ) -> Self {
        let n = g.link_count();
        let active = if use_alt {
            lm.pick(origin, target)
        } else {
            Vec::new()
        };
        let mut s = Self {
            target,
            variant,
            active,
            g: vec![INF; n],
            parent: vec![u32::MAX; n],
            closed: vec![false; n],
            heap: BinaryHeap::new(),
            settled: 0,
            result: None,
        };
        if origin == target {
            s.result = Some(Some(Route {
                links: Vec::new(),
                cost_ms: 0,
            }));
            return s;
        }
        for &l in g.out_links(origin) {
            let c = costs.enter(l, variant);
            if c < s.g[l as usize] {
                s.g[l as usize] = c;
                let h = lm.h(&s.active, g.link(l).to, target);
                if h < INF {
                    s.heap.push(Reverse((c + h, Reverse(c), l)));
                }
            }
        }
        s
    }

    /// Settles up to `budget` links. Returns the result once finished (`None` inside: no route).
    pub fn advance(
        &mut self,
        g: &Graph,
        lm: &Landmarks,
        costs: &Costs,
        budget: u64,
    ) -> Option<Option<Route>> {
        if let Some(r) = &self.result {
            return Some(r.clone());
        }
        let mut done = 0;
        while done < budget {
            let Some(Reverse((_, Reverse(gl), l))) = self.heap.pop() else {
                self.result = Some(None);
                return Some(None);
            };
            if self.closed[l as usize] || gl > self.g[l as usize] {
                continue;
            }
            self.closed[l as usize] = true;
            self.settled += 1;
            done += 1;
            if g.link(l).to == self.target {
                let mut links = vec![l];
                let mut cur = l;
                while self.parent[cur as usize] != u32::MAX {
                    cur = self.parent[cur as usize];
                    links.push(cur);
                }
                links.reverse();
                let r = Route { links, cost_ms: gl };
                self.result = Some(Some(r.clone()));
                return Some(Some(r));
            }
            for &(m, turn) in g.succ(l) {
                if self.closed[m as usize] {
                    continue;
                }
                let ng = gl + u64::from(turn) + costs.enter(m, self.variant);
                if ng < self.g[m as usize] {
                    self.g[m as usize] = ng;
                    self.parent[m as usize] = l;
                    let h = lm.h(&self.active, g.link(m).to, self.target);
                    if h < INF {
                        self.heap.push(Reverse((ng + h, Reverse(ng), m)));
                    }
                }
            }
        }
        None
    }
}

/// Runs a search to completion.
#[must_use]
pub fn route(
    g: &Graph,
    lm: &Landmarks,
    costs: &Costs,
    origin: u32,
    target: u32,
    variant: u8,
) -> Option<Route> {
    let mut s = Search::new(g, lm, costs, origin, target, variant, true);
    loop {
        if let Some(r) = s.advance(g, lm, costs, u64::MAX) {
            return r;
        }
    }
}

/// Plain Dijkstra (reference; ALT disabled).
#[must_use]
pub fn dijkstra(g: &Graph, costs: &Costs, origin: u32, target: u32, variant: u8) -> Option<Route> {
    let lm = Landmarks::default();
    let mut s = Search::new(g, &lm, costs, origin, target, variant, false);
    loop {
        if let Some(r) = s.advance(g, &lm, costs, u64::MAX) {
            return r;
        }
    }
}
