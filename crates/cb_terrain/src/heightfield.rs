//! Square sample grids over the map (2 m spacing, origin at the map centre, ADR-0033) and the
//! height query API.
//!
//! Heights between samples are interpolated on the **render triangulation**: every cell is split
//! along its `(i, j)`–`(i + 1, j + 1)` diagonal, exactly like the terrain chunk meshes, so
//! vehicles and buildings sit on the rendered LOD0 surface (DATA_MODEL §2).

use glam::{Vec2, Vec3};

/// A square grid of `n × n` samples with uniform spacing, row-major in Z (`index = j * n + i`).
#[derive(Clone, Debug, PartialEq)]
pub struct Grid<T> {
    n: usize,
    spacing_m: f32,
    half_extent_m: f32,
    data: Vec<T>,
}

/// Height samples in metres.
pub type HeightGrid = Grid<f32>;

impl<T: Copy> Grid<T> {
    /// A grid of `n × n` samples covering `[-half, half]²` with `half = (n - 1) · spacing / 2`.
    #[must_use]
    pub fn filled(n: usize, spacing_m: f32, value: T) -> Self {
        Self {
            n,
            spacing_m,
            half_extent_m: (n.saturating_sub(1)) as f32 * spacing_m * 0.5,
            data: vec![value; n * n],
        }
    }

    /// Wraps existing row-major data; `None` if the length is not `n²`.
    #[must_use]
    pub fn from_vec(n: usize, spacing_m: f32, data: Vec<T>) -> Option<Self> {
        (data.len() == n * n).then(|| Self {
            n,
            spacing_m,
            half_extent_m: (n.saturating_sub(1)) as f32 * spacing_m * 0.5,
            data,
        })
    }

    /// Samples per side.
    #[must_use]
    pub fn n(&self) -> usize {
        self.n
    }

    /// Sample spacing in metres.
    #[must_use]
    pub fn spacing_m(&self) -> f32 {
        self.spacing_m
    }

    /// Half the side length in metres (the grid covers `[-h, h]²`).
    #[must_use]
    pub fn half_extent_m(&self) -> f32 {
        self.half_extent_m
    }

    /// Row-major index of sample `(i, j)`.
    #[inline]
    #[must_use]
    pub fn index(&self, i: usize, j: usize) -> usize {
        j * self.n + i
    }

    /// Sample value (panics in debug if out of range; callers iterate `0..n`).
    #[inline]
    #[must_use]
    pub fn get(&self, i: usize, j: usize) -> T {
        self.data[j * self.n + i]
    }

    /// Sets a sample value.
    #[inline]
    pub fn set(&mut self, i: usize, j: usize, value: T) {
        let n = self.n;
        self.data[j * n + i] = value;
    }

    /// Sample value with indices clamped to the grid.
    #[inline]
    #[must_use]
    pub fn get_clamped(&self, i: isize, j: isize) -> T {
        let m = self.n as isize - 1;
        self.get(i.clamp(0, m) as usize, j.clamp(0, m) as usize)
    }

    /// World position (x, z) of sample `(i, j)`.
    #[inline]
    #[must_use]
    pub fn pos(&self, i: usize, j: usize) -> Vec2 {
        Vec2::new(
            i as f32 * self.spacing_m - self.half_extent_m,
            j as f32 * self.spacing_m - self.half_extent_m,
        )
    }

    /// Continuous grid coordinates of a world position (not clamped).
    #[inline]
    #[must_use]
    pub fn to_grid(&self, p: Vec2) -> Vec2 {
        (p + Vec2::splat(self.half_extent_m)) / self.spacing_m
    }

    /// Nearest sample to a world position (clamped).
    #[must_use]
    pub fn nearest(&self, p: Vec2) -> (usize, usize) {
        let g = self.to_grid(p);
        let m = (self.n - 1) as f32;
        (
            g.x.round().clamp(0.0, m) as usize,
            g.y.round().clamp(0.0, m) as usize,
        )
    }

    /// Whether a world position lies inside the grid.
    #[must_use]
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= -self.half_extent_m
            && p.x <= self.half_extent_m
            && p.y >= -self.half_extent_m
            && p.y <= self.half_extent_m
    }

    /// Raw row-major data.
    #[must_use]
    pub fn data(&self) -> &[T] {
        &self.data
    }

    /// Mutable raw data.
    pub fn data_mut(&mut self) -> &mut [T] {
        &mut self.data
    }
}

impl HeightGrid {
    /// Height at a world position on the render triangulation (clamped to the grid).
    #[must_use]
    pub fn height_at(&self, p: Vec2) -> f32 {
        let (i, j, fx, fz) = self.cell(p);
        let h00 = self.get(i, j);
        let h11 = self.get(i + 1, j + 1);
        if fx >= fz {
            // Triangle (00, 10, 11).
            let h10 = self.get(i + 1, j);
            h00 + (h10 - h00) * fx + (h11 - h10) * fz
        } else {
            // Triangle (00, 01, 11).
            let h01 = self.get(i, j + 1);
            h00 + (h11 - h01) * fx + (h01 - h00) * fz
        }
    }

    /// Height gradient `(dh/dx, dh/dz)` of the triangle under `p`.
    #[must_use]
    pub fn gradient_at(&self, p: Vec2) -> Vec2 {
        let (i, j, fx, fz) = self.cell(p);
        let s = self.spacing_m;
        let h00 = self.get(i, j);
        let h11 = self.get(i + 1, j + 1);
        if fx >= fz {
            let h10 = self.get(i + 1, j);
            Vec2::new((h10 - h00) / s, (h11 - h10) / s)
        } else {
            let h01 = self.get(i, j + 1);
            Vec2::new((h11 - h01) / s, (h01 - h00) / s)
        }
    }

    /// Unit surface normal (Y up) of the triangle under `p`.
    #[must_use]
    pub fn normal_at(&self, p: Vec2) -> Vec3 {
        let g = self.gradient_at(p);
        Vec3::new(-g.x, 1.0, -g.y).normalize()
    }

    /// Slope (rise over run) of the triangle under `p`.
    #[must_use]
    pub fn slope_at(&self, p: Vec2) -> f32 {
        self.gradient_at(p).length()
    }

    /// Central-difference slope at sample `(i, j)` (rise over run).
    #[must_use]
    pub fn sample_slope(&self, i: usize, j: usize) -> f32 {
        let (ii, jj) = (i as isize, j as isize);
        let dx =
            (self.get_clamped(ii + 1, jj) - self.get_clamped(ii - 1, jj)) / (2.0 * self.spacing_m);
        let dz =
            (self.get_clamped(ii, jj + 1) - self.get_clamped(ii, jj - 1)) / (2.0 * self.spacing_m);
        (dx * dx + dz * dz).sqrt()
    }

    /// Cell `(i, j)` and the fractional position inside it, clamped to the grid.
    fn cell(&self, p: Vec2) -> (usize, usize, f32, f32) {
        let g = self.to_grid(p);
        let m = (self.n - 2) as f32;
        let (gx, gz) = (g.x.clamp(0.0, m + 1.0), g.y.clamp(0.0, m + 1.0));
        let (ci, cj) = (gx.floor().min(m), gz.floor().min(m));
        (ci as usize, cj as usize, gx - ci, gz - cj)
    }

    /// Minimum and maximum height.
    #[must_use]
    pub fn min_max(&self) -> (f32, f32) {
        self.data
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), h| {
                (a.min(*h), b.max(*h))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp() -> HeightGrid {
        let mut g = HeightGrid::filled(5, 2.0, 0.0);
        for j in 0..5 {
            for i in 0..5 {
                g.set(i, j, i as f32 * 1.0 + j as f32 * 0.5);
            }
        }
        g
    }

    #[test]
    fn grid_covers_centered_extent() {
        let g = HeightGrid::filled(2049, 2.0, 0.0);
        assert_eq!(g.half_extent_m(), 2048.0);
        assert_eq!(g.pos(0, 0), Vec2::new(-2048.0, -2048.0));
        assert_eq!(g.pos(2048, 2048), Vec2::new(2048.0, 2048.0));
        assert_eq!(g.nearest(Vec2::new(0.9, -0.9)), (1024, 1024));
    }

    #[test]
    fn height_exact_on_samples_and_linear_on_planes() {
        let g = ramp();
        for j in 0..5 {
            for i in 0..5 {
                assert!((g.height_at(g.pos(i, j)) - g.get(i, j)).abs() < 1e-5);
            }
        }
        // A plane is reproduced exactly anywhere.
        let p = Vec2::new(-1.3, 0.7);
        let gp = g.to_grid(p);
        assert!((g.height_at(p) - (gp.x + 0.5 * gp.y)).abs() < 1e-5);
        assert!((g.gradient_at(p) - Vec2::new(0.5, 0.25)).length() < 1e-5);
    }

    #[test]
    fn height_follows_triangle_split() {
        let mut g = HeightGrid::filled(2, 1.0, 0.0);
        g.set(1, 0, 1.0); // only corner (1, 0) raised
        let half = g.half_extent_m();
        // Below the diagonal (fx ≥ fz) the raised corner contributes, above it does not.
        assert!((g.height_at(Vec2::new(0.75 - half, 0.25 - half)) - 0.5).abs() < 1e-6);
        assert!(g.height_at(Vec2::new(0.25 - half, 0.75 - half)).abs() < 1e-6);
    }

    #[test]
    fn clamped_outside_and_normals_up() {
        let g = ramp();
        let far = g.height_at(Vec2::new(1e4, 1e4));
        assert!((far - g.get(4, 4)).abs() < 1e-5);
        assert!(g.normal_at(Vec2::ZERO).y > 0.0);
        assert!(g.sample_slope(2, 2) > 0.0);
    }
}
