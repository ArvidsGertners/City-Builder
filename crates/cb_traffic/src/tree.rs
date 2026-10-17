//! Bounded shortest-path trees over links (ALGORITHMS §12, §16): the macro commute model grows
//! one tree per home (all of its workers' workplaces at once) and job matching one per seeker
//! home bounded by the longest acceptable commute. Searches run over links so turn penalties
//! apply exactly as in [`crate::router`]; costs are plain per-link milliseconds (no route
//! variants: the aggregate model assigns every commuter to the shortest path).
//!
//! The scratch buffers are reused between searches and reset through touched lists, so a
//! search allocates nothing in steady state.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::graph::Graph;

const NONE: u32 = u32::MAX;
const UNSET: u64 = u64::MAX;

/// A reusable shortest-path tree.
#[derive(Clone, Debug, Default)]
pub struct Tree {
    /// Cost to the end of each link (ms).
    g: Vec<u64>,
    parent: Vec<u32>,
    closed: Vec<bool>,
    /// Earliest arrival per node and the link that achieved it.
    arrival: Vec<u64>,
    best_in: Vec<u32>,
    touched_links: Vec<u32>,
    touched_nodes: Vec<u32>,
    wanted: Vec<bool>,
    heap: BinaryHeap<Reverse<(u64, u32)>>,
    /// Links settled by the last search.
    pub settled: u64,
}

impl Tree {
    /// An empty tree.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn reset(&mut self, g: &Graph) {
        let (nl, nn) = (g.link_count(), g.node_count());
        if self.g.len() != nl {
            self.g = vec![UNSET; nl];
            self.parent = vec![NONE; nl];
            self.closed = vec![false; nl];
            self.touched_links.clear();
        }
        if self.arrival.len() != nn {
            self.arrival = vec![UNSET; nn];
            self.best_in = vec![NONE; nn];
            self.wanted = vec![false; nn];
            self.touched_nodes.clear();
        }
        for &l in &self.touched_links {
            self.g[l as usize] = UNSET;
            self.parent[l as usize] = NONE;
            self.closed[l as usize] = false;
        }
        for &v in &self.touched_nodes {
            self.arrival[v as usize] = UNSET;
            self.best_in[v as usize] = NONE;
            self.wanted[v as usize] = false;
        }
        self.touched_links.clear();
        self.touched_nodes.clear();
        self.heap.clear();
        self.settled = 0;
    }

    fn touch_node(&mut self, v: u32) {
        if self.arrival[v as usize] == UNSET && !self.wanted[v as usize] {
            self.touched_nodes.push(v);
        }
    }

    fn push(&mut self, l: u32, cost: u64, parent: u32) {
        if cost < self.g[l as usize] {
            if self.g[l as usize] == UNSET {
                self.touched_links.push(l);
            }
            self.g[l as usize] = cost;
            self.parent[l as usize] = parent;
            self.heap.push(Reverse((cost, l)));
        }
    }

    /// Grows the tree from `sources` (node, initial cost ms) with per-link costs `link_ms`
    /// until every node in `targets` is reached or the next cost exceeds `bound_ms`. Empty
    /// `targets` grows to the bound. Ties settle by link index, so the tree is deterministic.
    pub fn grow(
        &mut self,
        g: &Graph,
        link_ms: &[u32],
        sources: &[(u32, u64)],
        bound_ms: u64,
        targets: &[u32],
    ) {
        self.reset(g);
        let mut remaining = 0usize;
        for &t in targets {
            if (t as usize) < self.wanted.len() && !self.wanted[t as usize] {
                self.touch_node(t);
                self.wanted[t as usize] = true;
                remaining += 1;
            }
        }
        for &(v, c0) in sources {
            if (v as usize) >= g.node_count() {
                continue;
            }
            // Arriving at a source costs its initial cost.
            if c0 < self.arrival[v as usize] {
                self.touch_node(v);
                if self.wanted[v as usize] && self.arrival[v as usize] == UNSET {
                    remaining = remaining.saturating_sub(1);
                }
                self.arrival[v as usize] = c0;
            }
            for &l in g.out_links(v) {
                let c = c0 + u64::from(link_ms.get(l as usize).copied().unwrap_or(u32::MAX));
                self.push(l, c, NONE);
            }
        }
        if !targets.is_empty() && remaining == 0 {
            return;
        }
        while let Some(Reverse((c, l))) = self.heap.pop() {
            if c > bound_ms {
                break;
            }
            if self.closed[l as usize] || c > self.g[l as usize] {
                continue;
            }
            self.closed[l as usize] = true;
            self.settled += 1;
            let to = g.link(l).to;
            if c < self.arrival[to as usize] {
                self.touch_node(to);
                if self.wanted[to as usize] && self.arrival[to as usize] == UNSET {
                    remaining = remaining.saturating_sub(1);
                }
                self.arrival[to as usize] = c;
                self.best_in[to as usize] = l;
                if !targets.is_empty() && remaining == 0 {
                    break;
                }
            }
            for &(m, turn) in g.succ(l) {
                if self.closed[m as usize] {
                    continue;
                }
                let mc = u64::from(link_ms.get(m as usize).copied().unwrap_or(u32::MAX));
                self.push(m, c + u64::from(turn) + mc, l);
            }
        }
    }

    /// Cost of reaching `node` (ms), if it was reached within the bound.
    #[must_use]
    pub fn cost(&self, node: u32) -> Option<u64> {
        self.arrival
            .get(node as usize)
            .copied()
            .filter(|c| *c != UNSET)
    }

    /// Links of the tree path to `node`, from the source (cleared first; empty when the node is
    /// a source or unreached).
    pub fn path(&self, node: u32, out: &mut Vec<u32>) {
        out.clear();
        let mut l = self.best_in.get(node as usize).copied().unwrap_or(NONE);
        while l != NONE {
            out.push(l);
            l = self.parent[l as usize];
        }
        out.reverse();
    }
}

/// Nearest source per node in reverse (ALGORITHMS §16 job matching): for every node, the
/// cheapest cost of driving from it to one of `sources` `(node, extra ms on arrival, label)`
/// with its label, within `bound_ms`; `None` beyond. Link-based with turn penalties; ties go to
/// the smaller label, then the smaller link, so the result is deterministic.
#[must_use]
pub fn nearest_reverse(
    g: &Graph,
    link_ms: &[u32],
    sources: &[(u32, u64, u32)],
    bound_ms: u64,
) -> Vec<Option<(u64, u32)>> {
    let nl = g.link_count();
    let mut best: Vec<Option<(u64, u32)>> = vec![None; g.node_count()];
    let mut cost = vec![UNSET; nl];
    let mut label = vec![u32::MAX; nl];
    let mut closed = vec![false; nl];
    let mut heap: BinaryHeap<Reverse<(u64, u32, u32)>> = BinaryHeap::new();
    let cost_of = |l: u32| u64::from(link_ms.get(l as usize).copied().unwrap_or(u32::MAX));
    for &(v, extra, lab) in sources {
        if (v as usize) >= best.len() {
            continue;
        }
        if best[v as usize].is_none_or(|b| (extra, lab) < b) {
            best[v as usize] = Some((extra, lab));
        }
        for &l in g.in_links(v) {
            let c = extra + cost_of(l);
            if (c, lab) < (cost[l as usize], label[l as usize]) {
                cost[l as usize] = c;
                label[l as usize] = lab;
                heap.push(Reverse((c, lab, l)));
            }
        }
    }
    while let Some(Reverse((c, lab, l))) = heap.pop() {
        if c > bound_ms {
            break;
        }
        if closed[l as usize] || (c, lab) > (cost[l as usize], label[l as usize]) {
            continue;
        }
        closed[l as usize] = true;
        let from = g.link(l).from as usize;
        if best[from].is_none_or(|b| (c, lab) < b) {
            best[from] = Some((c, lab));
        }
        for &(p, turn) in g.pred(l) {
            if closed[p as usize] {
                continue;
            }
            let pc = c + u64::from(turn) + cost_of(p);
            if (pc, lab) < (cost[p as usize], label[p as usize]) {
                cost[p as usize] = pc;
                label[p as usize] = lab;
                heap.push(Reverse((pc, lab, p)));
            }
        }
    }
    for b in &mut best {
        if b.is_some_and(|(c, _)| c > bound_ms) {
            *b = None;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use glam::Vec2;

    use super::*;
    use crate::graph::LinkSpec;

    fn line(n: u32) -> Graph {
        let pos: Vec<Vec2> = (0..n).map(|i| Vec2::new(i as f32 * 100.0, 0.0)).collect();
        let mut links = Vec::new();
        for i in 0..n - 1 {
            for (a, b) in [(i, i + 1), (i + 1, i)] {
                let d = (pos[b as usize] - pos[a as usize]).normalize();
                links.push(LinkSpec {
                    from: a,
                    to: b,
                    length_m: 100.0,
                    speed_mps: 10.0,
                    dirt: false,
                    capacity_per_sim_h: 1600.0,
                    dir_out: d,
                    dir_in: d,
                    key: 0,
                });
            }
        }
        Graph::build(pos, links)
    }

    #[test]
    fn tree_costs_paths_and_bounds() {
        let g = line(6);
        let mut t = Tree::new();
        t.grow(&g, g.free_costs(), &[(0, 500)], u64::MAX, &[]);
        assert_eq!(t.cost(0), Some(500));
        assert_eq!(t.cost(5), Some(500 + 5 * 10_000));
        let mut p = Vec::new();
        t.path(5, &mut p);
        assert_eq!(p.len(), 5);
        assert_eq!(g.link(p[0]).from, 0);
        assert_eq!(g.link(p[4]).to, 5);
        // Bounded: nodes beyond the bound stay unreached; reuse resets the scratch.
        t.grow(&g, g.free_costs(), &[(5, 0)], 25_000, &[]);
        assert_eq!(t.cost(3), Some(20_000));
        assert_eq!(t.cost(1), None);
        assert_eq!(t.cost(0), None);
        // Targets stop the search early.
        t.grow(&g, g.free_costs(), &[(0, 0)], u64::MAX, &[2]);
        assert_eq!(t.cost(2), Some(20_000));
        assert!(t.settled <= 4, "settled {}", t.settled);
    }

    #[test]
    fn nearest_reverse_matches_forward_trees() {
        let g = line(8);
        // Sources at nodes 1 (label 7) and 6 (label 3, arriving costs 2 s extra).
        let near = nearest_reverse(&g, g.free_costs(), &[(1, 0, 7), (6, 2_000, 3)], u64::MAX);
        let mut t = Tree::new();
        for v in 0..8u32 {
            t.grow(&g, g.free_costs(), &[(v, 0)], u64::MAX, &[]);
            let a = t.cost(1).unwrap();
            let b = t.cost(6).unwrap() + 2_000;
            let want = if (a, 7) <= (b, 3) { (a, 7) } else { (b, 3) };
            assert_eq!(near[v as usize], Some(want), "node {v}");
        }
        let bounded = nearest_reverse(&g, g.free_costs(), &[(0, 0, 1)], 25_000);
        assert_eq!(bounded[2], Some((20_000, 1)));
        assert_eq!(bounded[3], None);
    }
}
