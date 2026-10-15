//! Polylines with cumulative arc length: sampling, projection, offsetting, resampling and
//! Douglas–Peucker simplification.

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::arc_length::ArcLengthLut;
use crate::bezier::CubicBezier2;

/// A 2D polyline with cumulative lengths.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Polyline2 {
    points: Vec<Vec2>,
    cum: Vec<f32>,
}

impl Polyline2 {
    /// Builds from points (consecutive duplicates removed).
    #[must_use]
    pub fn new(points: impl IntoIterator<Item = Vec2>) -> Self {
        let mut pts: Vec<Vec2> = Vec::new();
        for p in points {
            if pts.last().is_none_or(|l| (*l - p).length_squared() > 1e-10) {
                pts.push(p);
            }
        }
        let mut cum = Vec::with_capacity(pts.len());
        let mut acc = 0.0;
        for k in 0..pts.len() {
            if k > 0 {
                acc += (pts[k] - pts[k - 1]).length();
            }
            cum.push(acc);
        }
        Self { points: pts, cum }
    }

    /// Samples a Bézier with chords no longer than `max_step` and a chord error ≤ `tol`.
    #[must_use]
    pub fn from_bezier(c: &CubicBezier2, max_step: f32, tol: f32) -> Self {
        let lut = ArcLengthLut::new(c);
        let len = lut.length();
        // Chord error ≈ κ L² / 8 → L ≤ √(8 tol / κ).
        let kmax = c.max_curvature(32).max(1e-6);
        let step = (8.0 * tol / kmax).sqrt().min(max_step).max(0.05);
        let count = ((len / step).ceil() as usize).max(1);
        Self::new((0..=count).map(|k| c.eval(lut.t_at(len * k as f32 / count as f32))))
    }

    /// Points.
    #[must_use]
    pub fn points(&self) -> &[Vec2] {
        &self.points
    }

    /// Total length.
    #[must_use]
    pub fn length(&self) -> f32 {
        self.cum.last().copied().unwrap_or(0.0)
    }

    /// Cumulative length at each point.
    #[must_use]
    pub fn cumulative(&self) -> &[f32] {
        &self.cum
    }

    fn locate(&self, s: f32) -> (usize, f32) {
        let n = self.points.len();
        if n < 2 {
            return (0, 0.0);
        }
        let s = s.clamp(0.0, self.length());
        let k = self.cum.partition_point(|v| *v <= s).clamp(1, n - 1) - 1;
        let seg = self.cum[k + 1] - self.cum[k];
        (
            k,
            if seg > 0.0 {
                (s - self.cum[k]) / seg
            } else {
                0.0
            },
        )
    }

    /// Point at arc length `s` (clamped).
    #[must_use]
    pub fn point_at(&self, s: f32) -> Vec2 {
        match self.points.len() {
            0 => Vec2::ZERO,
            1 => self.points[0],
            _ => {
                let (k, f) = self.locate(s);
                self.points[k].lerp(self.points[k + 1], f)
            }
        }
    }

    /// Unit tangent at arc length `s`.
    #[must_use]
    pub fn tangent_at(&self, s: f32) -> Vec2 {
        if self.points.len() < 2 {
            return Vec2::X;
        }
        let (k, _) = self.locate(s);
        (self.points[k + 1] - self.points[k]).normalize_or(Vec2::X)
    }

    /// Closest point: `(arc length, signed distance)`; positive = left of the direction.
    #[must_use]
    pub fn project(&self, p: Vec2) -> (f32, f32) {
        let mut best = (0.0, f32::INFINITY, 0.0);
        for k in 0..self.points.len().saturating_sub(1) {
            let (a, b) = (self.points[k], self.points[k + 1]);
            let ab = b - a;
            let l2 = ab.length_squared().max(1e-12);
            let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
            let q = a + ab * t;
            let d = (p - q).length();
            if d < best.1 {
                let side = ab.perp_dot(p - a).signum();
                best = (self.cum[k] + t * l2.sqrt(), d, side);
            }
        }
        (best.0, best.1 * if best.2 < 0.0 { -1.0 } else { 1.0 })
    }

    /// Offset by `d` to the left (negative = right) with mitred joints, miter capped at 4·|d|.
    #[must_use]
    pub fn offset(&self, d: f32) -> Self {
        let n = self.points.len();
        if n < 2 {
            return self.clone();
        }
        let normal = |k: usize| {
            (self.points[k + 1] - self.points[k])
                .normalize_or(Vec2::X)
                .perp()
        };
        let mut out = Vec::with_capacity(n);
        for k in 0..n {
            let nrm = if k == 0 {
                normal(0)
            } else if k == n - 1 {
                normal(n - 2)
            } else {
                let (a, b) = (normal(k - 1), normal(k));
                let m = (a + b).normalize_or(b);
                let cos = m.dot(b).max(0.25);
                m / cos
            };
            out.push(self.points[k] + nrm * d);
        }
        Self::new(out)
    }

    /// Uniform resampling every `step` (end kept).
    #[must_use]
    pub fn resample(&self, step: f32) -> Self {
        let len = self.length();
        let count = ((len / step.max(1e-3)).ceil() as usize).max(1);
        Self::new((0..=count).map(|k| self.point_at(len * k as f32 / count as f32)))
    }

    /// Douglas–Peucker simplification with tolerance `tol`.
    #[must_use]
    pub fn simplify(&self, tol: f32) -> Self {
        Self::new(douglas_peucker(&self.points, tol))
    }
}

/// Douglas–Peucker on an open point list (endpoints kept).
#[must_use]
pub fn douglas_peucker(pts: &[Vec2], tol: f32) -> Vec<Vec2> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut stack = vec![(0, pts.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (pa, pb) = (pts[a], pts[b]);
        let ab = pb - pa;
        let l = ab.length().max(1e-9);
        let mut best = (0, 0.0f32);
        for (k, p) in pts.iter().enumerate().take(b).skip(a + 1) {
            let d = (ab.perp_dot(*p - pa) / l).abs();
            if d > best.1 {
                best = (k, d);
            }
        }
        if best.1 > tol {
            keep[best.0] = true;
            stack.push((a, best.0));
            stack.push((best.0, b));
        }
    }
    pts.iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(*p))
        .collect()
}

/// A 3D polyline (x, height, z) with cumulative horizontal arc length.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Polyline3 {
    /// Points.
    pub points: Vec<Vec3>,
}

impl Polyline3 {
    /// Horizontal projection.
    #[must_use]
    pub fn xz(&self) -> Polyline2 {
        Polyline2::new(self.points.iter().map(|p| Vec2::new(p.x, p.z)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampling_projection_and_offset() {
        let pl = Polyline2::new([Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0)]);
        assert_eq!(pl.length(), 20.0);
        assert_eq!(pl.point_at(15.0), Vec2::new(10.0, 5.0));
        let (s, d) = pl.project(Vec2::new(5.0, 2.0));
        assert!(
            (s - 5.0).abs() < 1e-5 && (d - 2.0).abs() < 1e-5,
            "left is positive"
        );
        let (_, d2) = pl.project(Vec2::new(5.0, -2.0));
        assert!((d2 + 2.0).abs() < 1e-5);
        let off = pl.offset(1.0);
        // Left offset of an L turning left: inner corner at (9, 1).
        assert!(
            (off.points()[1] - Vec2::new(9.0, 1.0)).length() < 1e-4,
            "{:?}",
            off.points()
        );
    }

    #[test]
    fn bezier_sampling_respects_tolerance() {
        let c = CubicBezier2::new(
            Vec2::ZERO,
            Vec2::new(0.0, 60.0),
            Vec2::new(60.0, 60.0),
            Vec2::new(60.0, 0.0),
        );
        let pl = Polyline2::from_bezier(&c, 4.0, 0.05);
        for k in 0..=200 {
            let p = c.eval(k as f32 / 200.0);
            assert!(pl.project(p).1.abs() < 0.06);
        }
        assert!(
            pl.points()
                .windows(2)
                .all(|w| (w[1] - w[0]).length() <= 4.0 + 1e-3)
        );
    }

    #[test]
    fn simplify_keeps_corners() {
        let pts: Vec<Vec2> = (0..=10)
            .map(|k| Vec2::new(k as f32, 0.0))
            .chain((1..=10).map(|k| Vec2::new(10.0, k as f32)))
            .collect();
        let s = douglas_peucker(&pts, 0.01);
        assert_eq!(
            s,
            vec![Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0)]
        );
    }
}
