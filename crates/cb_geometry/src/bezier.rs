//! Cubic Bézier curves in the XZ plane (ADR-0007): evaluation, derivatives, curvature, exact
//! splitting, tight bounds and nearest-point projection.

use glam::Vec2;
use serde::{Deserialize, Serialize};

/// A cubic Bézier `B(t) = Σ bᵢ(t) pᵢ`, `t ∈ [0, 1]`, in world XZ (`Vec2(x, z)`, ADR-0033).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CubicBezier2 {
    /// Control points `p0` (start) … `p3` (end).
    pub p: [Vec2; 4],
}

impl CubicBezier2 {
    /// Curve from four control points.
    #[must_use]
    pub const fn new(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2) -> Self {
        Self {
            p: [p0, p1, p2, p3],
        }
    }

    /// Straight segment with control points at thirds (uniform speed).
    #[must_use]
    pub fn straight(a: Vec2, b: Vec2) -> Self {
        let d = b - a;
        Self::new(a, a + d / 3.0, a + d * (2.0 / 3.0), b)
    }

    /// Quadratic `a, c, b` elevated to a cubic (the road tool's curved mode).
    #[must_use]
    pub fn from_quadratic(a: Vec2, c: Vec2, b: Vec2) -> Self {
        Self::new(a, a + (c - a) * (2.0 / 3.0), b + (c - b) * (2.0 / 3.0), b)
    }

    /// Start point.
    #[must_use]
    pub fn start(&self) -> Vec2 {
        self.p[0]
    }

    /// End point.
    #[must_use]
    pub fn end(&self) -> Vec2 {
        self.p[3]
    }

    /// Point at `t`.
    #[must_use]
    pub fn eval(&self, t: f32) -> Vec2 {
        let u = 1.0 - t;
        let [a, b, c, d] = self.p;
        a * (u * u * u) + b * (3.0 * u * u * t) + c * (3.0 * u * t * t) + d * (t * t * t)
    }

    /// First derivative `B'(t)`.
    #[must_use]
    pub fn deriv(&self, t: f32) -> Vec2 {
        let u = 1.0 - t;
        let [a, b, c, d] = self.p;
        (b - a) * (3.0 * u * u) + (c - b) * (6.0 * u * t) + (d - c) * (3.0 * t * t)
    }

    /// Second derivative `B''(t)`.
    #[must_use]
    pub fn deriv2(&self, t: f32) -> Vec2 {
        let [a, b, c, d] = self.p;
        (c - b * 2.0 + a) * (6.0 * (1.0 - t)) + (d - c * 2.0 + b) * (6.0 * t)
    }

    /// Unit tangent at `t` (falls back to the chord for degenerate derivatives).
    #[must_use]
    pub fn tangent(&self, t: f32) -> Vec2 {
        let d = self.deriv(t);
        if d.length_squared() > 1e-12 {
            d.normalize()
        } else {
            (self.p[3] - self.p[0]).normalize_or(Vec2::X)
        }
    }

    /// Signed curvature `κ = (B' × B'') / |B'|³` (positive = turning left, counter-clockwise).
    #[must_use]
    pub fn curvature(&self, t: f32) -> f32 {
        let d1 = self.deriv(t);
        let d2 = self.deriv2(t);
        let len = d1.length();
        if len < 1e-6 {
            0.0
        } else {
            d1.perp_dot(d2) / (len * len * len)
        }
    }

    /// Maximum |curvature| over `samples + 1` uniform parameters.
    #[must_use]
    pub fn max_curvature(&self, samples: usize) -> f32 {
        (0..=samples)
            .map(|k| self.curvature(k as f32 / samples as f32).abs())
            .fold(0.0, f32::max)
    }

    /// Exact split at `t` (de Casteljau).
    #[must_use]
    pub fn split(&self, t: f32) -> (Self, Self) {
        let [a, b, c, d] = self.p;
        let ab = a.lerp(b, t);
        let bc = b.lerp(c, t);
        let cd = c.lerp(d, t);
        let abc = ab.lerp(bc, t);
        let bcd = bc.lerp(cd, t);
        let m = abc.lerp(bcd, t);
        (Self::new(a, ab, abc, m), Self::new(m, bcd, cd, d))
    }

    /// Sub-curve on `[t0, t1]`.
    #[must_use]
    pub fn sub(&self, t0: f32, t1: f32) -> Self {
        if t1 <= t0 {
            let p = self.eval(t0);
            return Self::new(p, p, p, p);
        }
        let (_, right) = self.split(t0);
        let (left, _) = right.split((t1 - t0) / (1.0 - t0).max(1e-9));
        left
    }

    /// The same curve traversed backwards.
    #[must_use]
    pub fn reversed(&self) -> Self {
        Self::new(self.p[3], self.p[2], self.p[1], self.p[0])
    }

    /// Tight axis-aligned bounds (extrema from the derivative's roots).
    #[must_use]
    pub fn bounds(&self) -> (Vec2, Vec2) {
        let mut lo = self.p[0].min(self.p[3]);
        let mut hi = self.p[0].max(self.p[3]);
        let [a, b, c, d] = self.p;
        for axis in 0..2 {
            let (pa, pb, pc, pd) = (a[axis], b[axis], c[axis], d[axis]);
            // B'(t)/3 = α t² + β t + γ
            let qa = -pa + 3.0 * pb - 3.0 * pc + pd;
            let qb = 2.0 * (pa - 2.0 * pb + pc);
            let qc = pb - pa;
            let mut roots = [f32::NAN; 2];
            if qa.abs() < 1e-9 {
                if qb.abs() > 1e-9 {
                    roots[0] = -qc / qb;
                }
            } else {
                let disc = qb * qb - 4.0 * qa * qc;
                if disc >= 0.0 {
                    let s = disc.sqrt();
                    roots = [(-qb + s) / (2.0 * qa), (-qb - s) / (2.0 * qa)];
                }
            }
            for t in roots {
                if t > 0.0 && t < 1.0 {
                    let v = self.eval(t)[axis];
                    lo[axis] = lo[axis].min(v);
                    hi[axis] = hi[axis].max(v);
                }
            }
        }
        (lo, hi)
    }

    /// Nearest point: `(t, distance)`; 16 seeds refined by Newton steps on `(B − p)·B'`.
    #[must_use]
    pub fn nearest(&self, p: Vec2) -> (f32, f32) {
        let mut best = (0.0f32, f32::INFINITY);
        for k in 0..=16 {
            let t = k as f32 / 16.0;
            let d = (self.eval(t) - p).length_squared();
            if d < best.1 {
                best = (t, d);
            }
        }
        let mut t = best.0;
        for _ in 0..8 {
            let r = self.eval(t) - p;
            let d1 = self.deriv(t);
            let f = r.dot(d1);
            let df = d1.length_squared() + r.dot(self.deriv2(t));
            if df.abs() < 1e-12 {
                break;
            }
            let nt = (t - f / df).clamp(0.0, 1.0);
            if (nt - t).abs() < 1e-7 {
                t = nt;
                break;
            }
            t = nt;
        }
        let d = (self.eval(t) - p).length();
        if d * d <= best.1 {
            (t, d)
        } else {
            (best.0, best.1.sqrt())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s_curve() -> CubicBezier2 {
        CubicBezier2::new(
            Vec2::new(0.0, 0.0),
            Vec2::new(40.0, 60.0),
            Vec2::new(80.0, -60.0),
            Vec2::new(120.0, 0.0),
        )
    }

    #[test]
    fn straight_has_uniform_speed_and_zero_curvature() {
        let c = CubicBezier2::straight(Vec2::ZERO, Vec2::new(30.0, 40.0));
        assert!((c.deriv(0.2).length() - 50.0).abs() < 1e-4);
        assert!((c.deriv(0.9).length() - 50.0).abs() < 1e-4);
        assert!(c.max_curvature(16) < 1e-6);
        assert!((c.eval(0.5) - Vec2::new(15.0, 20.0)).length() < 1e-5);
    }

    #[test]
    fn quadratic_elevation_is_exact() {
        let (a, ctl, b) = (Vec2::ZERO, Vec2::new(10.0, 20.0), Vec2::new(20.0, 0.0));
        let c = CubicBezier2::from_quadratic(a, ctl, b);
        for k in 0..=10 {
            let t = k as f32 / 10.0;
            let q = a * (1.0 - t) * (1.0 - t) + ctl * 2.0 * t * (1.0 - t) + b * t * t;
            assert!((c.eval(t) - q).length() < 1e-4);
        }
    }

    #[test]
    fn circle_arc_curvature() {
        // Quarter circle r = 50 (standard κ approximation constant).
        let k = 0.552_284_8 * 50.0;
        let c = CubicBezier2::new(
            Vec2::new(50.0, 0.0),
            Vec2::new(50.0, k),
            Vec2::new(k, 50.0),
            Vec2::new(0.0, 50.0),
        );
        for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
            assert!(
                (c.curvature(t) - 1.0 / 50.0).abs() < 0.0006,
                "t {t}: {}",
                c.curvature(t)
            );
        }
    }

    #[test]
    fn split_and_sub_are_exact() {
        let c = s_curve();
        let (l, r) = c.split(0.3);
        assert!((l.eval(1.0) - c.eval(0.3)).length() < 1e-4);
        assert!((l.eval(0.5) - c.eval(0.15)).length() < 1e-4);
        assert!((r.eval(0.5) - c.eval(0.65)).length() < 1e-4);
        let s = c.sub(0.2, 0.7);
        assert!((s.eval(0.0) - c.eval(0.2)).length() < 1e-4);
        assert!((s.eval(1.0) - c.eval(0.7)).length() < 1e-3);
        assert_eq!(c.reversed().eval(0.25), c.eval(0.75));
    }

    #[test]
    fn bounds_are_tight_and_nearest_projects() {
        let c = s_curve();
        let (lo, hi) = c.bounds();
        let mut slo = Vec2::splat(f32::INFINITY);
        let mut shi = Vec2::splat(f32::NEG_INFINITY);
        for k in 0..=1000 {
            let p = c.eval(k as f32 / 1000.0);
            slo = slo.min(p);
            shi = shi.max(p);
        }
        assert!(
            (lo - slo).length() < 0.01 && (hi - shi).length() < 0.01,
            "{lo} {hi} vs {slo} {shi}"
        );
        let target = c.eval(0.42) + c.tangent(0.42).perp() * 3.0;
        let (t, d) = c.nearest(target);
        assert!((t - 0.42).abs() < 2e-3, "t {t}");
        assert!((d - 3.0).abs() < 0.01, "d {d}");
    }
}
