//! Loose uniform spatial hash over the map (ADR-0013).
//!
//! An internal acceleration structure only — nothing in gameplay snaps to its cells. Entries are
//! axis-aligned boxes in the ground plane, inserted into every cell they overlap; query results
//! are de-duplicated and sorted by key so consumers are deterministic.

use crate::collections::LookupMap;
use core::hash::Hash;
use glam::Vec2;
use smallvec::SmallVec;

/// Axis-aligned box in the ground plane (`Vec2(x, z)`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb2 {
    /// Minimum corner.
    pub min: Vec2,
    /// Maximum corner.
    pub max: Vec2,
}

impl Aabb2 {
    /// Box from two corners in any order.
    #[must_use]
    pub fn from_corners(a: Vec2, b: Vec2) -> Self {
        Self {
            min: a.min(b),
            max: a.max(b),
        }
    }

    /// Box around a point with half extent `r`.
    #[must_use]
    pub fn around(p: Vec2, r: f32) -> Self {
        Self {
            min: p - Vec2::splat(r),
            max: p + Vec2::splat(r),
        }
    }

    /// Smallest box containing all points; `None` for an empty slice.
    #[must_use]
    pub fn from_points(points: &[Vec2]) -> Option<Self> {
        let first = *points.first()?;
        Some(points.iter().fold(
            Self {
                min: first,
                max: first,
            },
            |b, p| Self {
                min: b.min.min(*p),
                max: b.max.max(*p),
            },
        ))
    }

    /// Whether the boxes overlap (touching counts).
    #[must_use]
    pub fn intersects(&self, other: &Aabb2) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
    }

    /// Whether the box contains `p` (inclusive).
    #[must_use]
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    /// Grows the box by `r` on every side.
    #[must_use]
    pub fn expanded(&self, r: f32) -> Self {
        Self {
            min: self.min - Vec2::splat(r),
            max: self.max + Vec2::splat(r),
        }
    }
}

/// Loose uniform grid of AABB entries keyed by `K`.
#[derive(Clone, Debug)]
pub struct SpatialHash<K> {
    origin: Vec2,
    cell_size: f32,
    nx: u32,
    ny: u32,
    cells: Vec<SmallVec<[(K, Aabb2); 4]>>,
    entries: LookupMap<K, (Aabb2, SmallVec<[u32; 4]>)>,
}

impl<K: Copy + Ord + Hash> SpatialHash<K> {
    /// A grid covering `bounds` with square cells of `cell_size` meters (positions outside the
    /// bounds are clamped into the border cells).
    #[must_use]
    pub fn new(bounds: Aabb2, cell_size: f32) -> Self {
        let cell_size = cell_size.max(1.0);
        let size = bounds.max - bounds.min;
        let nx = ((size.x / cell_size).ceil() as u32).max(1);
        let ny = ((size.y / cell_size).ceil() as u32).max(1);
        Self {
            origin: bounds.min,
            cell_size,
            nx,
            ny,
            cells: vec![SmallVec::new(); (nx * ny) as usize],
            entries: LookupMap::new(),
        }
    }

    fn cell_range(&self, b: &Aabb2) -> (u32, u32, u32, u32) {
        let to_cell = |v: f32, n: u32| -> u32 {
            let c = (v / self.cell_size).floor();
            if c < 0.0 { 0 } else { (c as u32).min(n - 1) }
        };
        let lo = b.min - self.origin;
        let hi = b.max - self.origin;
        (
            to_cell(lo.x, self.nx),
            to_cell(lo.y, self.ny),
            to_cell(hi.x, self.nx),
            to_cell(hi.y, self.ny),
        )
    }

    /// Inserts or replaces the entry for `key`.
    pub fn insert(&mut self, key: K, aabb: Aabb2) {
        self.remove(key);
        let (x0, y0, x1, y1) = self.cell_range(&aabb);
        let mut cells = SmallVec::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                let idx = y * self.nx + x;
                self.cells[idx as usize].push((key, aabb));
                cells.push(idx);
            }
        }
        self.entries.insert(key, (aabb, cells));
    }

    /// Removes the entry for `key`; returns whether it existed.
    pub fn remove(&mut self, key: K) -> bool {
        let Some((_, cells)) = self.entries.remove(&key) else {
            return false;
        };
        for idx in cells {
            self.cells[idx as usize].retain(|(k, _)| *k != key);
        }
        true
    }

    /// The stored box for `key`.
    #[must_use]
    pub fn get(&self, key: K) -> Option<Aabb2> {
        self.entries.get(&key).map(|(b, _)| *b)
    }

    /// Number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the index is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Keys whose box intersects `query`, sorted ascending, written into `out` (cleared first).
    pub fn query_aabb(&self, query: Aabb2, out: &mut Vec<K>) {
        out.clear();
        let (x0, y0, x1, y1) = self.cell_range(&query);
        for y in y0..=y1 {
            for x in x0..=x1 {
                for (k, b) in &self.cells[(y * self.nx + x) as usize] {
                    if b.intersects(&query) {
                        out.push(*k);
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
    }

    /// Keys whose box intersects the circle (box test against the circle's distance).
    pub fn query_circle(&self, center: Vec2, radius: f32, out: &mut Vec<K>) {
        self.query_aabb(Aabb2::around(center, radius), out);
        let r2 = radius * radius;
        out.retain(|k| {
            self.get(*k).is_some_and(|b| {
                let nearest = center.clamp(b.min, b.max);
                nearest.distance_squared(center) <= r2
            })
        });
    }

    /// Keys whose box contains `p`.
    pub fn query_point(&self, p: Vec2, out: &mut Vec<K>) {
        self.query_aabb(Aabb2 { min: p, max: p }, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::cases;

    #[test]
    fn spatial_hash_matches_brute_force() {
        let bounds = Aabb2::from_corners(Vec2::splat(-2048.0), Vec2::splat(2048.0));
        for (_, mut rng) in cases(11, 20) {
            let mut grid = SpatialHash::new(bounds, 32.0);
            let mut boxes = Vec::new();
            for k in 0..300u32 {
                let c = Vec2::new(
                    rng.range_f32(-2100.0, 2100.0),
                    rng.range_f32(-2100.0, 2100.0),
                );
                let half = Vec2::new(rng.range_f32(0.0, 150.0), rng.range_f32(0.0, 20.0));
                let b = Aabb2 {
                    min: c - half,
                    max: c + half,
                };
                grid.insert(k, b);
                boxes.push((k, b));
            }
            // Remove a few and re-insert others to exercise bookkeeping.
            for k in (0..300u32).step_by(7) {
                assert!(grid.remove(k));
            }
            boxes.retain(|(k, _)| k % 7 != 0);
            let mut out = Vec::new();
            for _ in 0..100 {
                let c = Vec2::new(
                    rng.range_f32(-2000.0, 2000.0),
                    rng.range_f32(-2000.0, 2000.0),
                );
                let q = Aabb2::around(c, rng.range_f32(1.0, 200.0));
                grid.query_aabb(q, &mut out);
                let mut expected: Vec<u32> = boxes
                    .iter()
                    .filter(|(_, b)| b.intersects(&q))
                    .map(|(k, _)| *k)
                    .collect();
                expected.sort_unstable();
                assert_eq!(out, expected);
            }
        }
    }

    #[test]
    fn circle_query_filters_corners() {
        let mut grid = SpatialHash::new(Aabb2::from_corners(Vec2::ZERO, Vec2::splat(100.0)), 10.0);
        grid.insert(
            1u32,
            Aabb2::from_corners(Vec2::new(8.0, 8.0), Vec2::new(9.0, 9.0)),
        );
        let mut out = Vec::new();
        grid.query_circle(Vec2::ZERO, 10.0, &mut out);
        assert!(
            out.is_empty(),
            "corner box at distance ~11.3 must be excluded"
        );
        grid.query_circle(Vec2::ZERO, 12.0, &mut out);
        assert_eq!(out, vec![1]);
    }
}
