//! Arc-length tables for Béziers: `s(t)` sampled at uniform `t`, inverted by binary search and
//! linear interpolation (ALGORITHMS §1: 32–128 samples by length).

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::bezier::CubicBezier2;

/// Cumulative arc length at uniform parameter steps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArcLengthLut {
    cum: Vec<f32>,
}

impl ArcLengthLut {
    /// Builds the table: samples = clamp(length / 2 m, 32, 128), each step integrated with
    /// 3-point Gauss–Legendre on the speed.
    #[must_use]
    pub fn new(c: &CubicBezier2) -> Self {
        let chord: f32 = (0..8)
            .map(|k| (c.eval((k + 1) as f32 / 8.0) - c.eval(k as f32 / 8.0)).length())
            .sum();
        let samples = ((chord / 2.0).ceil() as usize).clamp(32, 128);
        let (x, w) = (
            [-0.774_596_7f32, 0.0, 0.774_596_7],
            [5.0f32 / 9.0, 8.0 / 9.0, 5.0 / 9.0],
        );
        let mut cum = Vec::with_capacity(samples + 1);
        cum.push(0.0);
        let mut acc = 0.0;
        let h = 1.0 / samples as f32;
        for k in 0..samples {
            let (a, b) = (k as f32 * h, (k + 1) as f32 * h);
            let (mid, half) = (0.5 * (a + b), 0.5 * (b - a));
            let seg: f32 = (0..3)
                .map(|q| w[q] * c.deriv(mid + half * x[q]).length())
                .sum::<f32>()
                * half;
            acc += seg;
            cum.push(acc);
        }
        Self { cum }
    }

    /// Total length (m).
    #[must_use]
    pub fn length(&self) -> f32 {
        self.cum.last().copied().unwrap_or(0.0)
    }

    /// Arc length at parameter `t`.
    #[must_use]
    pub fn s_at(&self, t: f32) -> f32 {
        let n = self.cum.len() - 1;
        let x = t.clamp(0.0, 1.0) * n as f32;
        let k = (x.floor() as usize).min(n.saturating_sub(1));
        let f = x - k as f32;
        self.cum[k] + (self.cum[(k + 1).min(n)] - self.cum[k]) * f
    }

    /// Parameter at arc length `s` (clamped).
    #[must_use]
    pub fn t_at(&self, s: f32) -> f32 {
        let n = self.cum.len() - 1;
        let s = s.clamp(0.0, self.length());
        let k = self.cum.partition_point(|v| *v < s).clamp(1, n);
        let (a, b) = (self.cum[k - 1], self.cum[k]);
        let f = if b > a { (s - a) / (b - a) } else { 0.0 };
        ((k - 1) as f32 + f) / n as f32
    }

    /// Points every `step` metres along the curve (end included).
    #[must_use]
    pub fn sample(&self, c: &CubicBezier2, step: f32) -> Vec<(f32, Vec2)> {
        let len = self.length();
        let count = ((len / step).ceil() as usize).max(1);
        (0..=count)
            .map(|k| {
                let s = len * k as f32 / count as f32;
                (s, c.eval(self.t_at(s)))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_length_and_inverse() {
        let c = CubicBezier2::straight(Vec2::ZERO, Vec2::new(300.0, 400.0));
        let lut = ArcLengthLut::new(&c);
        assert!((lut.length() - 500.0).abs() < 1e-2);
        assert!((lut.t_at(250.0) - 0.5).abs() < 1e-4);
        assert!((lut.s_at(lut.t_at(123.0)) - 123.0).abs() < 1e-2);
    }

    #[test]
    fn quarter_circle_length() {
        let k = 0.552_284_8 * 100.0;
        let c = CubicBezier2::new(
            Vec2::new(100.0, 0.0),
            Vec2::new(100.0, k),
            Vec2::new(k, 100.0),
            Vec2::new(0.0, 100.0),
        );
        let lut = ArcLengthLut::new(&c);
        let exact = std::f32::consts::FRAC_PI_2 * 100.0;
        assert!((lut.length() - exact).abs() < 0.05, "{}", lut.length());
        let pts = lut.sample(&c, 10.0);
        let gaps: Vec<f32> = pts.windows(2).map(|w| (w[1].1 - w[0].1).length()).collect();
        assert!(
            gaps.iter().all(|g| (g - gaps[0]).abs() < 0.05),
            "uniform spacing {gaps:?}"
        );
    }
}
