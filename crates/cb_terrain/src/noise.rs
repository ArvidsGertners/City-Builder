//! Deterministic 2D gradient noise, fBm, ridged noise and domain warping.
//!
//! Only `floor`, `+`, `*` and table lookups are used, so results are bit-identical on every
//! platform (ADR-0043). Gradients come from a 16-direction table indexed by a stable hash of the
//! lattice point and the seed.

use cb_core::hash::mix64;
use core::f32::consts::{FRAC_1_SQRT_2 as R2, SQRT_2};

use glam::Vec2;

/// 16 unit gradient directions (precomputed: no trigonometry at runtime).
const GRADIENTS: [(f32, f32); 16] = [
    (1.0, 0.0),
    (0.923_879_5, 0.382_683_4),
    (R2, R2),
    (0.382_683_4, 0.923_879_5),
    (0.0, 1.0),
    (-0.382_683_4, 0.923_879_5),
    (-R2, R2),
    (-0.923_879_5, 0.382_683_4),
    (-1.0, 0.0),
    (-0.923_879_5, -0.382_683_4),
    (-R2, -R2),
    (-0.382_683_4, -0.923_879_5),
    (0.0, -1.0),
    (0.382_683_4, -0.923_879_5),
    (R2, -R2),
    (0.923_879_5, -0.382_683_4),
];

#[inline]
fn grad(ix: i32, iy: i32, seed: u64) -> (f32, f32) {
    let h = mix64(
        seed ^ (u64::from(ix as u32) << 32 | u64::from(iy as u32))
            .wrapping_mul(0x9E37_79B9_7F4A_7C15),
    );
    GRADIENTS[(h >> 60) as usize]
}

#[inline]
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Gradient (Perlin-style) noise at `p` (lattice units), roughly in `[-1, 1]`.
#[must_use]
pub fn gradient(p: Vec2, seed: u64) -> f32 {
    let (fx, fy) = (p.x.floor(), p.y.floor());
    let (ix, iy) = (fx as i32, fy as i32);
    let (dx, dy) = (p.x - fx, p.y - fy);
    let dot = |gx: i32, gy: i32, ox: f32, oy: f32| {
        let g = grad(gx, gy, seed);
        g.0 * ox + g.1 * oy
    };
    let n00 = dot(ix, iy, dx, dy);
    let n10 = dot(ix + 1, iy, dx - 1.0, dy);
    let n01 = dot(ix, iy + 1, dx, dy - 1.0);
    let n11 = dot(ix + 1, iy + 1, dx - 1.0, dy - 1.0);
    let (u, v) = (fade(dx), fade(dy));
    let a = n00 + (n10 - n00) * u;
    let b = n01 + (n11 - n01) * u;
    // 2D gradient noise peaks at ±√0.5; scale to about ±1.
    (a + (b - a) * v) * SQRT_2
}

/// Fractal Brownian motion: `octaves` layers of [`gradient`] noise with lacunarity 2 and gain
/// 0.5; `wavelength_m` is the first octave's wavelength. Result normalized to about `[-1, 1]`.
#[must_use]
pub fn fbm(p: Vec2, wavelength_m: f32, octaves: u32, seed: u64) -> f32 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut norm = 0.0;
    let mut q = p / wavelength_m;
    for o in 0..octaves {
        sum += amp
            * gradient(
                q,
                seed.wrapping_add(u64::from(o).wrapping_mul(0x632B_E59B_D9B4_E019)),
            );
        norm += amp;
        amp *= 0.5;
        // Rotate slightly between octaves to hide lattice alignment (exact rational rotation).
        q = Vec2::new(q.x * 1.6 - q.y * 1.2, q.x * 1.2 + q.y * 1.6);
    }
    sum / norm
}

/// Ridged multifractal in `[0, 1]`: sharp crests where the base noise crosses zero; each octave
/// is weighted by the previous one so ridges carry the detail.
#[must_use]
pub fn ridged(p: Vec2, wavelength_m: f32, octaves: u32, seed: u64) -> f32 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut norm = 0.0;
    let mut weight = 1.0;
    let mut q = p / wavelength_m;
    for o in 0..octaves {
        let r = 1.0
            - gradient(
                q,
                seed.wrapping_add(u64::from(o).wrapping_mul(0x632B_E59B_D9B4_E019)),
            )
            .abs();
        let r = (r * r * weight).clamp(0.0, 1.0);
        sum += amp * r;
        norm += amp;
        weight = r;
        amp *= 0.5;
        q = Vec2::new(q.x * 1.6 - q.y * 1.2, q.x * 1.2 + q.y * 1.6);
    }
    sum / norm
}

/// Domain warp: `p` displaced by two independent fBm fields of amplitude `amp_m`.
#[must_use]
pub fn warp(p: Vec2, wavelength_m: f32, amp_m: f32, seed: u64) -> Vec2 {
    let wx = fbm(p, wavelength_m, 2, seed ^ 0xA5A5_0001);
    let wy = fbm(p, wavelength_m, 2, seed ^ 0x5A5A_0002);
    p + Vec2::new(wx, wy) * amp_m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_deterministic_and_bounded() {
        let mut max = 0.0f32;
        for k in 0..10_000 {
            let p = Vec2::new(
                (k % 100) as f32 * 0.37 - 20.0,
                (k / 100) as f32 * 0.41 - 20.0,
            );
            let a = gradient(p, 7);
            assert_eq!(a.to_bits(), gradient(p, 7).to_bits());
            max = max.max(a.abs());
        }
        assert!(max <= 1.05, "max {max}");
        assert!(max > 0.5, "noise too flat: {max}");
    }

    #[test]
    fn noise_zero_on_lattice_and_seed_dependent() {
        assert_eq!(gradient(Vec2::new(3.0, -2.0), 1), 0.0);
        let p = Vec2::new(0.3, 0.6);
        assert_ne!(gradient(p, 1).to_bits(), gradient(p, 2).to_bits());
    }

    #[test]
    fn fbm_and_ridged_ranges() {
        for k in 0..2_000 {
            let p = Vec2::new(k as f32 * 13.7, k as f32 * -7.3);
            let f = fbm(p, 500.0, 4, 3);
            assert!((-1.05..=1.05).contains(&f));
            let r = ridged(p, 600.0, 3, 3);
            assert!((0.0..=1.0).contains(&r));
        }
    }

    #[test]
    fn noise_is_continuous() {
        let p = Vec2::new(10.25, 4.75);
        let e = Vec2::new(1e-3, 0.0);
        assert!((gradient(p, 9) - gradient(p + e, 9)).abs() < 0.01);
        let lattice = Vec2::new(11.0, 4.5);
        assert!((gradient(lattice - e, 9) - gradient(lattice + e, 9)).abs() < 0.01);
    }
}
