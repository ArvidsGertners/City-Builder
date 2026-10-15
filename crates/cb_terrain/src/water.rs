//! Water surfaces: a per-sample water level (`NaN` = dry) and the body each wet sample belongs
//! to. Water is immutable after generation (ADR-0065); depth is `level − terrain height`.

use glam::Vec2;

use crate::heightfield::{Grid, HeightGrid};

/// Kind of a water body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaterBodyKind {
    /// A river channel (sloped surface, monotonic towards the mouth).
    River,
    /// A lake (flat surface).
    Lake,
}

/// Water level and body id per height sample.
#[derive(Clone, Debug, PartialEq)]
pub struct WaterField {
    level: Grid<f32>,
    /// 0 = dry, otherwise index + 1 into `bodies`.
    body: Grid<u8>,
    bodies: Vec<WaterBodyKind>,
}

impl WaterField {
    /// An all-dry field matching `heights`.
    #[must_use]
    pub fn dry_like(heights: &HeightGrid) -> Self {
        let (n, s) = (heights.n(), heights.spacing_m());
        Self {
            level: Grid::filled(n, s, f32::NAN),
            body: Grid::filled(n, s, 0),
            bodies: Vec::new(),
        }
    }

    /// Registers a body; returns its id (≥ 1).
    pub fn add_body(&mut self, kind: WaterBodyKind) -> u8 {
        self.bodies.push(kind);
        self.bodies.len() as u8
    }

    /// Marks sample `(i, j)` wet with `level` for body `id`.
    pub fn set(&mut self, i: usize, j: usize, level: f32, id: u8) {
        self.level.set(i, j, level);
        self.body.set(i, j, id);
    }

    /// Clears sample `(i, j)`.
    pub fn clear(&mut self, i: usize, j: usize) {
        self.level.set(i, j, f32::NAN);
        self.body.set(i, j, 0);
    }

    /// Water level at sample `(i, j)` (`None` if dry).
    #[must_use]
    pub fn level(&self, i: usize, j: usize) -> Option<f32> {
        let l = self.level.get(i, j);
        (!l.is_nan()).then_some(l)
    }

    /// Body id at sample `(i, j)` (0 = dry).
    #[must_use]
    pub fn body(&self, i: usize, j: usize) -> u8 {
        self.body.get(i, j)
    }

    /// Body kinds by id − 1.
    #[must_use]
    pub fn bodies(&self) -> &[WaterBodyKind] {
        &self.bodies
    }

    /// Per-sample levels (`NaN` = dry).
    #[must_use]
    pub fn levels(&self) -> &Grid<f32> {
        &self.level
    }

    /// Water level at the nearest sample of a world position.
    #[must_use]
    pub fn level_at(&self, p: Vec2) -> Option<f32> {
        let (i, j) = self.level.nearest(p);
        self.level(i, j)
    }

    /// Water depth at a world position (0 when dry).
    #[must_use]
    pub fn depth_at(&self, p: Vec2, heights: &HeightGrid) -> f32 {
        self.level_at(p)
            .map_or(0.0, |l| (l - heights.height_at(p)).max(0.0))
    }

    /// Whether a world position is in water.
    #[must_use]
    pub fn is_water(&self, p: Vec2) -> bool {
        self.level_at(p).is_some()
    }

    /// Number of wet samples of body `id`.
    #[must_use]
    pub fn body_samples(&self, id: u8) -> usize {
        self.body.data().iter().filter(|b| **b == id).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_and_bodies() {
        let h = HeightGrid::filled(9, 2.0, 1.0);
        let mut w = WaterField::dry_like(&h);
        let id = w.add_body(WaterBodyKind::Lake);
        assert_eq!(id, 1);
        w.set(4, 4, 3.5, id);
        assert!(w.is_water(Vec2::ZERO));
        assert!((w.depth_at(Vec2::ZERO, &h) - 2.5).abs() < 1e-6);
        assert_eq!(w.depth_at(Vec2::new(6.0, 6.0), &h), 0.0);
        assert_eq!(w.body_samples(id), 1);
        w.clear(4, 4);
        assert!(!w.is_water(Vec2::ZERO));
    }
}
