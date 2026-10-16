//! Vertical profile solver (ALGORITHMS §2, ADR-0065): heights every 4 m of arc length that
//! follow the ground smoothly under grade limits, fixed endpoints, optional pinned interior
//! heights (junction crossings) and grade-feasible lower bounds (clearances). Evaluated with
//! monotone cubic Hermite (Fritsch–Carlson).

use serde::{Deserialize, Serialize};

/// Profile sample spacing (m).
pub const SAMPLE_M: f32 = 4.0;
/// Landing grade near junctions (ADR-0063).
pub const LANDING_GRADE: f32 = 0.04;
/// Smoothness weight λ.
const LAMBDA: f32 = 1.0;
/// Solver passes.
const MAX_PASSES: usize = 60;

/// Sample positions (arc length) for a segment of length `len`.
#[must_use]
pub fn sample_positions(len: f32) -> Vec<f32> {
    let n = (len / SAMPLE_M).ceil().max(1.0) as usize;
    (0..=n).map(|i| i as f32 * len / n as f32).collect()
}

/// Solver input.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileInput {
    /// Arc-length positions (from [`sample_positions`]).
    pub s: Vec<f32>,
    /// Ground height per sample (terrain without this road's own stamp).
    pub ground: Vec<f32>,
    /// Terrain attraction weight per sample (1 on ground, 0 on bridge spans).
    pub weight: Vec<f32>,
    /// Lower bounds per sample (clearances); `NEG_INFINITY` where none.
    pub lower: Vec<f32>,
    /// Start / end heights.
    pub h0: f32,
    /// End height.
    pub h1: f32,
    /// Pinned interior heights `(sample index, height)` (crossing junctions).
    pub pins: Vec<(usize, f32)>,
    /// Maximum grade (rise over run).
    pub max_grade: f32,
    /// Distances from start / end with the landing grade (junction approaches).
    pub landing0_m: f32,
    /// Landing distance from the end.
    pub landing1_m: f32,
}

impl ProfileInput {
    /// Ground-following input with endpoint heights and no bounds.
    #[must_use]
    pub fn ground(s: Vec<f32>, ground: Vec<f32>, h0: f32, h1: f32, max_grade: f32) -> Self {
        let n = s.len();
        Self {
            s,
            ground,
            weight: vec![1.0; n],
            lower: vec![f32::NEG_INFINITY; n],
            h0,
            h1,
            pins: Vec::new(),
            max_grade,
            landing0_m: 0.0,
            landing1_m: 0.0,
        }
    }
}

/// Why a profile cannot be built.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ProfileError {
    /// A step exceeds the grade limit at arc length `s_m`.
    TooSteep {
        /// Arc length of the first violating step.
        s_m: f32,
    },
}

/// A solved profile.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// Arc-length positions.
    pub s: Vec<f32>,
    /// Heights.
    pub h: Vec<f32>,
    /// Fritsch–Carlson tangents.
    pub m: Vec<f32>,
}

impl Profile {
    /// Builds the interpolant from samples.
    #[must_use]
    pub fn new(s: Vec<f32>, h: Vec<f32>) -> Self {
        let m = fc_tangents(&s, &h);
        Self { s, h, m }
    }

    /// Flat profile at height `h` over length `len`.
    #[must_use]
    pub fn flat(len: f32, h: f32) -> Self {
        let s = sample_positions(len);
        let hv = vec![h; s.len()];
        Self::new(s, hv)
    }

    /// Height at arc length `s` (clamped).
    #[must_use]
    pub fn height_at(&self, s: f32) -> f32 {
        let n = self.s.len();
        if n == 1 {
            return self.h[0];
        }
        let s = s.clamp(self.s[0], self.s[n - 1]);
        let k = self.s.partition_point(|v| *v <= s).clamp(1, n - 1) - 1;
        let dx = self.s[k + 1] - self.s[k];
        if dx <= 0.0 {
            return self.h[k];
        }
        let t = (s - self.s[k]) / dx;
        let (t2, t3) = (t * t, t * t * t);
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;
        h00 * self.h[k] + h10 * dx * self.m[k] + h01 * self.h[k + 1] + h11 * dx * self.m[k + 1]
    }

    /// Grade (dh/ds) at `s` by central difference.
    #[must_use]
    pub fn grade_at(&self, s: f32) -> f32 {
        let e = 0.5;
        (self.height_at(s + e) - self.height_at(s - e)) / (2.0 * e)
    }

    /// Length covered.
    #[must_use]
    pub fn length(&self) -> f32 {
        self.s.last().copied().unwrap_or(0.0)
    }

    /// Maximum absolute step grade between samples.
    #[must_use]
    pub fn max_step_grade(&self) -> f32 {
        self.s
            .windows(2)
            .zip(self.h.windows(2))
            .map(|(s, h)| ((h[1] - h[0]) / (s[1] - s[0]).max(1e-6)).abs())
            .fold(0.0, f32::max)
    }
}

/// Fritsch–Carlson monotone tangents.
#[must_use]
pub fn fc_tangents(x: &[f32], y: &[f32]) -> Vec<f32> {
    let n = x.len();
    if n < 2 {
        return vec![0.0; n];
    }
    let d: Vec<f32> = (0..n - 1)
        .map(|i| (y[i + 1] - y[i]) / (x[i + 1] - x[i]).max(1e-6))
        .collect();
    let mut m = vec![0.0; n];
    m[0] = d[0];
    m[n - 1] = d[n - 2];
    for i in 1..n - 1 {
        m[i] = if d[i - 1] * d[i] <= 0.0 {
            0.0
        } else {
            0.5 * (d[i - 1] + d[i])
        };
    }
    for i in 0..n - 1 {
        if d[i] == 0.0 {
            m[i] = 0.0;
            m[i + 1] = 0.0;
            continue;
        }
        let a = m[i] / d[i];
        let b = m[i + 1] / d[i];
        let r = a * a + b * b;
        if r > 9.0 {
            let t = 3.0 / r.sqrt();
            m[i] = t * a * d[i];
            m[i + 1] = t * b * d[i];
        }
    }
    m
}

/// Solves the profile: Gauss–Seidel smoothing toward the ground, then forward/backward grade
/// clamping from fixed points, repeated until `max |Δh| < 0.01 m` (≤ 60 passes).
///
/// # Errors
/// [`ProfileError::TooSteep`] if a step still exceeds the grade (incl. infeasible endpoints).
pub fn solve(inp: &ProfileInput) -> Result<Profile, ProfileError> {
    let n = inp.s.len();
    if n < 2 {
        return Ok(Profile::new(inp.s.clone(), vec![inp.h0; n]));
    }
    let len = inp.s[n - 1];
    let max_step = |i: usize| -> f32 {
        let ds = inp.s[i + 1] - inp.s[i];
        let mid = 0.5 * (inp.s[i] + inp.s[i + 1]);
        let g = if mid < inp.landing0_m || len - mid < inp.landing1_m {
            LANDING_GRADE.min(inp.max_grade)
        } else {
            inp.max_grade
        };
        g * ds
    };
    let mut fixed = vec![false; n];
    fixed[0] = true;
    fixed[n - 1] = true;
    for (k, _) in &inp.pins {
        if *k < n {
            fixed[*k] = true;
        }
    }
    // Grade-feasible envelope of the lower bounds.
    let mut lo = inp.lower.clone();
    for i in 1..n {
        lo[i] = lo[i].max(lo[i - 1] - max_step(i - 1));
    }
    for i in (0..n - 1).rev() {
        lo[i] = lo[i].max(lo[i + 1] - max_step(i));
    }
    let mut h: Vec<f32> = inp.ground.iter().zip(&lo).map(|(g, l)| g.max(*l)).collect();
    h[0] = inp.h0;
    h[n - 1] = inp.h1;
    for (k, v) in &inp.pins {
        if *k < n {
            h[*k] = *v;
        }
    }
    for _ in 0..MAX_PASSES {
        let mut maxd: f32 = 0.0;
        for i in 1..n - 1 {
            if fixed[i] {
                continue;
            }
            let (hm2, hm1, hp1, hp2) = (
                h[i.saturating_sub(2)],
                h[i - 1],
                h[i + 1],
                h[(i + 2).min(n - 1)],
            );
            let w = inp.weight[i];
            let nv = ((w * inp.ground[i] + LAMBDA * (4.0 * hm1 + 4.0 * hp1 - hm2 - hp2))
                / (w + 6.0 * LAMBDA))
                .max(lo[i]);
            maxd = maxd.max((nv - h[i]).abs());
            h[i] = nv;
        }
        for i in 1..n - 1 {
            if !fixed[i] {
                let m = max_step(i - 1);
                h[i] = h[i].clamp(h[i - 1] - m, h[i - 1] + m);
            }
        }
        for i in (1..n - 1).rev() {
            if !fixed[i] {
                let m = max_step(i);
                h[i] = h[i].clamp(h[i + 1] - m, h[i + 1] + m).max(lo[i]);
            }
        }
        if maxd < 0.01 {
            break;
        }
    }
    for i in 0..n - 1 {
        if (h[i + 1] - h[i]).abs() > max_step(i) + 1e-3 {
            return Err(ProfileError::TooSteep { s_m: inp.s[i] });
        }
    }
    Ok(Profile::new(inp.s.clone(), h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bumpy(len: f32) -> (Vec<f32>, Vec<f32>) {
        let s = sample_positions(len);
        // Gentle hills (max slope 8 %) a 10 % road can follow closely.
        let g = s
            .iter()
            .map(|x| 10.0 + 4.0 * cb_core::math::sin(x * 0.02))
            .collect();
        (s, g)
    }

    #[test]
    fn profile_solver_follows_ground_within_grade() {
        let (s, g) = bumpy(400.0);
        let (h0, h1) = (g[0], g[g.len() - 1]);
        let p = solve(&ProfileInput::ground(s, g.clone(), h0, h1, 0.10)).expect("feasible");
        assert!(p.max_step_grade() <= 0.10 + 1e-3);
        let err: f32 =
            p.h.iter()
                .zip(&g)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f32::max);
        assert!(err < 0.5, "follows ground: {err}");
        assert_eq!(p.h[0], h0);
    }

    #[test]
    fn profile_solver_clamps_steep_ground() {
        let s = sample_positions(100.0);
        let g: Vec<f32> = s.iter().map(|x| x * 0.3).collect(); // 30 % hill
        let p = solve(&ProfileInput::ground(s.clone(), g, 0.0, 8.0, 0.10))
            .expect("feasible: 8 m over 100 m");
        assert!(p.max_step_grade() <= 0.10 + 1e-3);
        assert!((p.height_at(100.0) - 8.0).abs() < 1e-4);
    }

    #[test]
    fn profile_solver_infeasible_endpoints() {
        let s = sample_positions(40.0);
        let g = vec![0.0; s.len()];
        let r = solve(&ProfileInput::ground(s, g, 0.0, 10.0, 0.10));
        assert!(
            matches!(r, Err(ProfileError::TooSteep { .. })),
            "10 m over 40 m at 10 %"
        );
    }

    #[test]
    fn profile_solver_pins_and_landings() {
        let s = sample_positions(160.0);
        let g = vec![0.0; s.len()];
        let mut inp = ProfileInput::ground(s.clone(), g, 0.0, 0.0, 0.10);
        inp.pins = vec![(20, 3.0)];
        inp.landing0_m = 20.0;
        let p = solve(&inp).expect("feasible");
        assert!((p.h[20] - 3.0).abs() < 1e-6);
        assert!(
            p.h[0..5]
                .windows(2)
                .all(|w| (w[1] - w[0]).abs() <= 0.04 * SAMPLE_M + 1e-3),
            "landing grade"
        );
    }

    #[test]
    fn profile_solver_respects_clearance_envelope() {
        let s = sample_positions(200.0);
        let g = vec![0.0; s.len()];
        let mut inp = ProfileInput::ground(s.clone(), g, 0.0, 0.0, 0.08);
        let mid = s.len() / 2;
        inp.lower[mid] = 5.5;
        inp.weight[mid] = 0.0;
        let p = solve(&inp).expect("ramps are grade feasible");
        assert!(p.h[mid] >= 5.5 - 1e-4);
        assert!(p.max_step_grade() <= 0.08 + 1e-3);
    }

    #[test]
    fn hermite_is_monotone_between_samples() {
        let p = Profile::new(vec![0.0, 4.0, 8.0, 12.0], vec![0.0, 0.0, 3.0, 3.0]);
        let mut last = -1.0;
        for k in 0..=120 {
            let h = p.height_at(k as f32 * 0.1);
            assert!(h >= last - 1e-6 && (-1e-6..=3.0 + 1e-6).contains(&h));
            last = h;
        }
    }
}
