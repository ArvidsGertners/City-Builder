//! Pure utility network helpers (ADR-0026, ALGORITHMS §14, §18): connected components of the
//! road graph, network distances from producers and the deterministic shortage allocation.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use cb_core::ids::{BuildingId, NodeId};

/// Undirected road graph: node → (neighbour, length m), neighbours sorted.
pub type Adjacency = BTreeMap<NodeId, Vec<(NodeId, f32)>>;

/// Builds the adjacency from `(a, b, length)` edges.
#[must_use]
pub fn adjacency(edges: &[(NodeId, NodeId, f32)]) -> Adjacency {
    let mut adj: Adjacency = BTreeMap::new();
    for &(a, b, l) in edges {
        adj.entry(a).or_default().push((b, l));
        adj.entry(b).or_default().push((a, l));
    }
    for v in adj.values_mut() {
        v.sort_by(|x, y| x.0.cmp(&y.0).then(x.1.total_cmp(&y.1)));
    }
    adj
}

/// Component id per node: the smallest node id of its component (stable under edits elsewhere).
#[must_use]
pub fn components(adj: &Adjacency) -> BTreeMap<NodeId, NodeId> {
    let mut comp: BTreeMap<NodeId, NodeId> = BTreeMap::new();
    for &start in adj.keys() {
        if comp.contains_key(&start) {
            continue;
        }
        // `start` is the smallest unvisited id, hence the smallest of its component.
        let mut stack = vec![start];
        comp.insert(start, start);
        while let Some(n) = stack.pop() {
            for &(m, _) in adj.get(&n).map_or(&[][..], |v| v.as_slice()) {
                if let std::collections::btree_map::Entry::Vacant(v) = comp.entry(m) {
                    v.insert(start);
                    stack.push(m);
                }
            }
        }
    }
    comp
}

/// Network distance from the nearest source node (Dijkstra; ties by node id).
#[must_use]
pub fn distances(adj: &Adjacency, sources: &[NodeId]) -> BTreeMap<NodeId, f32> {
    let mut dist: BTreeMap<NodeId, f32> = BTreeMap::new();
    let mut heap: BinaryHeap<Reverse<(u64, NodeId)>> = BinaryHeap::new();
    for &s in sources {
        dist.insert(s, 0.0);
        heap.push(Reverse((0, s)));
    }
    while let Some(Reverse((dmm, n))) = heap.pop() {
        let d = dmm as f32 / 1000.0;
        if dist.get(&n).is_some_and(|x| *x < d) {
            continue;
        }
        for &(m, l) in adj.get(&n).map_or(&[][..], |v| v.as_slice()) {
            let nd = d + l;
            if dist.get(&m).is_none_or(|x| nd < *x) {
                dist.insert(m, nd);
                // Millimetre keys keep the heap ordering total and deterministic.
                heap.push(Reverse(((nd * 1000.0).round() as u64, m)));
            }
        }
    }
    dist
}

/// One consumer in a shortage allocation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Consumer {
    /// Building.
    pub id: BuildingId,
    /// Services and utilities are served first.
    pub priority: bool,
    /// Network distance from the nearest producer (m).
    pub distance_m: f32,
    /// Need (kW or m³/h).
    pub need: f32,
    /// Served in the previous allocation (hysteresis).
    pub was_served: bool,
}

/// Allocates `supply` among consumers: priority first, then by distance ascending, ties by
/// building id. A consumer already served keeps service while the remaining supply covers at
/// least `1 − hysteresis` of its need, so status does not flicker at the margin; the
/// borrowed share is not available to later consumers. Returns the served buildings.
#[must_use]
pub fn allocate(supply: f32, consumers: &[Consumer], hysteresis: f32) -> Vec<BuildingId> {
    let mut order: Vec<&Consumer> = consumers.iter().collect();
    order.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then(a.distance_m.total_cmp(&b.distance_m))
            .then(a.id.cmp(&b.id))
    });
    let mut left = supply;
    let mut served = Vec::new();
    for c in order {
        let need = c.need.max(0.0);
        let ok = left >= need || (c.was_served && left >= need * (1.0 - hysteresis));
        if ok {
            left = (left - need).max(0.0);
            served.push(c.id);
        } else {
            // Outer consumers fail first: once one fails, everyone farther fails too.
            break;
        }
    }
    served.sort_unstable();
    served
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(i: u32) -> NodeId {
        NodeId::try_new(i).unwrap()
    }
    fn b(i: u32) -> BuildingId {
        BuildingId::try_new(i).unwrap()
    }

    #[test]
    fn components_and_distances() {
        let adj = adjacency(&[(n(1), n(2), 100.0), (n(2), n(3), 50.0), (n(5), n(4), 10.0)]);
        let c = components(&adj);
        assert_eq!(c[&n(3)], n(1));
        assert_eq!(c[&n(5)], n(4));
        let d = distances(&adj, &[n(1)]);
        assert!((d[&n(3)] - 150.0).abs() < 1e-3);
        assert!(!d.contains_key(&n(4)));
    }

    #[test]
    fn power_brownout_outer_first() {
        let consumers: Vec<Consumer> = (1..=6)
            .map(|i| Consumer {
                id: b(i),
                priority: i == 6,
                distance_m: 100.0 * i as f32,
                need: 10.0,
                was_served: false,
            })
            .collect();
        // Enough for 4 of 6: the priority building and the three nearest.
        let s = allocate(40.0, &consumers, 0.1);
        assert_eq!(s, vec![b(1), b(2), b(3), b(6)]);
        // Full supply serves everyone.
        assert_eq!(allocate(60.0, &consumers, 0.1).len(), 6);
        // Hysteresis: a served building keeps power with a 10 % shortfall.
        let mut c2 = consumers.clone();
        for c in &mut c2 {
            c.was_served = true;
        }
        assert_eq!(allocate(59.0, &c2, 0.1).len(), 6);
        assert_eq!(allocate(59.0, &consumers, 0.1).len(), 5);
    }
}
