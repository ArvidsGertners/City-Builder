//! Roundabout detection (ADR-0063, ALGORITHMS §6): a directed cycle of 3–8 one-way segments
//! whose nodes lie on a circle of radius 15–60 m is a ring. Ring approaches get Priority,
//! entries Yield, and entries are metered by ring occupancy (traffic runtime).

use std::collections::BTreeMap;

use cb_core::ids::{NodeId, SegmentId};
use glam::Vec2;

/// Ring size limits (nodes).
pub const RING_MIN_NODES: usize = 3;
/// Ring size limits (nodes).
pub const RING_MAX_NODES: usize = 8;
/// Ring radius limits (m).
pub const RING_MIN_RADIUS_M: f32 = 15.0;
/// Ring radius limits (m).
pub const RING_MAX_RADIUS_M: f32 = 60.0;
/// Node and arc-midpoint distances from the centre may deviate this much from the mean radius.
pub const RING_RADIUS_TOL: f32 = 0.1;

/// A one-way segment in travel direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OneWay {
    /// Segment.
    pub id: SegmentId,
    /// Node it leaves.
    pub from: NodeId,
    /// Node it enters.
    pub to: NodeId,
    /// Plan position of `from`.
    pub from_pos: Vec2,
    /// Plan midpoint of the curve.
    pub mid: Vec2,
}

/// A detected ring.
#[derive(Clone, Debug, PartialEq)]
pub struct Ring {
    /// Segments in travel order, starting with the smallest id.
    pub segments: Vec<SegmentId>,
    /// Nodes in travel order (`nodes[k]` is where `segments[k]` starts).
    pub nodes: Vec<NodeId>,
    /// Centre (plan).
    pub center: Vec2,
    /// Mean radius (m).
    pub radius_m: f32,
}

/// All rings among `one_ways` (deterministic: sorted by first segment id).
#[must_use]
pub fn detect_rings(one_ways: &[OneWay]) -> Vec<Ring> {
    let mut out_of: BTreeMap<NodeId, Vec<usize>> = BTreeMap::new();
    for (k, s) in one_ways.iter().enumerate() {
        out_of.entry(s.from).or_default().push(k);
    }
    for v in out_of.values_mut() {
        v.sort_by_key(|&k| one_ways[k].id);
    }
    let mut order: Vec<usize> = (0..one_ways.len()).collect();
    order.sort_by_key(|&k| one_ways[k].id);
    let mut rings: Vec<Ring> = Vec::new();
    let mut used: Vec<SegmentId> = Vec::new();
    for &start in &order {
        if used.contains(&one_ways[start].id) {
            continue;
        }
        // Depth-first over one-way successors, ≤ RING_MAX_NODES segments, simple cycles only.
        let mut stack: Vec<(Vec<usize>, usize)> = vec![(vec![start], 0)];
        let mut found: Option<Vec<usize>> = None;
        while let Some((path, next)) = stack.pop() {
            let Some(&last) = path.last() else { continue };
            let succ = out_of
                .get(&one_ways[last].to)
                .map_or(&[][..], Vec::as_slice);
            if next < succ.len() {
                stack.push((path.clone(), next + 1));
                let k = succ[next];
                if one_ways[k].to == one_ways[start].from && k != start {
                    let mut cyc = path.clone();
                    cyc.push(k);
                    if cyc.len() >= RING_MIN_NODES && is_ring(one_ways, &cyc) {
                        found = Some(cyc);
                        break;
                    }
                } else if k == start && path.len() >= RING_MIN_NODES {
                    if is_ring(one_ways, &path) {
                        found = Some(path.clone());
                        break;
                    }
                } else if path.len() < RING_MAX_NODES
                    && !path.iter().any(|p| one_ways[*p].from == one_ways[k].from)
                    && one_ways[k].from != one_ways[start].from
                {
                    let mut p = path.clone();
                    p.push(k);
                    stack.push((p, 0));
                }
            }
        }
        if let Some(cyc) = found {
            let ring = make_ring(one_ways, &cyc);
            used.extend(ring.segments.iter().copied());
            rings.push(ring);
        }
    }
    rings.sort_by_key(|r| r.segments[0]);
    rings
}

fn fit(one_ways: &[OneWay], cyc: &[usize]) -> (Vec2, f32, f32) {
    let pts: Vec<Vec2> = cyc
        .iter()
        .flat_map(|&k| [one_ways[k].from_pos, one_ways[k].mid])
        .collect();
    let c = pts.iter().copied().fold(Vec2::ZERO, |a, p| a + p) / pts.len() as f32;
    let r = pts.iter().map(|p| p.distance(c)).sum::<f32>() / pts.len() as f32;
    let dev = pts
        .iter()
        .map(|p| (p.distance(c) - r).abs())
        .fold(0.0f32, f32::max);
    (c, r, dev)
}

fn is_ring(one_ways: &[OneWay], cyc: &[usize]) -> bool {
    if cyc.len() < RING_MIN_NODES || cyc.len() > RING_MAX_NODES {
        return false;
    }
    let (_, r, dev) = fit(one_ways, cyc);
    (RING_MIN_RADIUS_M..=RING_MAX_RADIUS_M).contains(&r) && dev <= RING_RADIUS_TOL * r
}

fn make_ring(one_ways: &[OneWay], cyc: &[usize]) -> Ring {
    let (center, radius_m, _) = fit(one_ways, cyc);
    let first = (0..cyc.len())
        .min_by_key(|&i| one_ways[cyc[i]].id)
        .unwrap_or(0);
    let rot: Vec<usize> = cyc[first..].iter().chain(&cyc[..first]).copied().collect();
    Ring {
        segments: rot.iter().map(|&k| one_ways[k].id).collect(),
        nodes: rot.iter().map(|&k| one_ways[k].from).collect(),
        center,
        radius_m,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sid(n: u32) -> SegmentId {
        SegmentId::try_new(n).unwrap()
    }
    fn nid(n: u32) -> NodeId {
        NodeId::try_new(n).unwrap()
    }

    fn circle(n: usize, r: f32, first_seg: u32, first_node: u32, ccw: bool) -> Vec<OneWay> {
        let at = |k: usize, half: f32| {
            let a =
                (k as f32 + half) / n as f32 * std::f32::consts::TAU * if ccw { 1.0 } else { -1.0 };
            Vec2::new(cb_core::math::cos(a), cb_core::math::sin(a)) * r
        };
        (0..n)
            .map(|k| OneWay {
                id: sid(first_seg + k as u32),
                from: nid(first_node + k as u32),
                to: nid(first_node + ((k + 1) % n) as u32),
                from_pos: at(k, 0.0),
                mid: at(k, 0.5),
            })
            .collect()
    }

    #[test]
    fn ring_detection_cases() {
        // A 4-arc ring of radius 30 m plus an unrelated one-way.
        let mut ws = circle(4, 30.0, 10, 1, true);
        ws.push(OneWay {
            id: sid(1),
            from: nid(50),
            to: nid(51),
            from_pos: Vec2::new(200.0, 0.0),
            mid: Vec2::new(250.0, 0.0),
        });
        let rings = detect_rings(&ws);
        assert_eq!(rings.len(), 1);
        assert_eq!(rings[0].segments, vec![sid(10), sid(11), sid(12), sid(13)]);
        assert!((rings[0].radius_m - 30.0).abs() < 1.0);
        // Clockwise rings and 8 arcs too.
        assert_eq!(detect_rings(&circle(8, 55.0, 1, 1, false)).len(), 1);
        // Too small, too large, too many arcs.
        assert!(detect_rings(&circle(4, 10.0, 1, 1, true)).is_empty());
        assert!(detect_rings(&circle(4, 80.0, 1, 1, true)).is_empty());
        assert!(detect_rings(&circle(9, 50.0, 1, 1, true)).is_empty());
        // A one-way block loop (square 60 m, straight sides) is not round enough.
        let sq = [
            Vec2::new(0.0, 0.0),
            Vec2::new(60.0, 0.0),
            Vec2::new(60.0, 60.0),
            Vec2::new(0.0, 60.0),
        ];
        let block: Vec<OneWay> = (0..4)
            .map(|k| OneWay {
                id: sid(k as u32 + 1),
                from: nid(k as u32 + 1),
                to: nid(((k + 1) % 4) as u32 + 1),
                from_pos: sq[k],
                mid: (sq[k] + sq[(k + 1) % 4]) * 0.5,
            })
            .collect();
        assert!(detect_rings(&block).is_empty());
    }
}
