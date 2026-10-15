//! Deterministic random number generation (ADR-0043).
//!
//! * [`SplitMix64`] — seeding and simple streams.
//! * [`Pcg32`] — the canonical PCG-XSH-RR 64/32 generator for content generation.
//! * [`SimRng::for_event`] — counter-based RNG for simulation events: a generator derived
//!   statelessly from `(world seed, stream, purpose, time, id)`, so nothing needs saving and
//!   systems are order-independent.

use crate::hash::{Domain, derive_seed, mix64};
use crate::math;

/// SplitMix64 generator (Vigna). Mainly used to seed other generators.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// A generator starting from `seed`.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Next 64-bit output.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        mix64(self.state)
    }
}

const PCG_MULT: u64 = 6_364_136_223_846_793_005;

/// PCG32 (XSH-RR, 64-bit state, 32-bit output), bit-compatible with `pcg32_srandom_r`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pcg32 {
    state: u64,
    inc: u64,
}

impl Pcg32 {
    /// Seeds like the reference `pcg32_srandom_r(initstate, initseq)`.
    #[must_use]
    pub fn new(initstate: u64, initseq: u64) -> Self {
        let mut rng = Self {
            state: 0,
            inc: (initseq << 1) | 1,
        };
        rng.next_u32();
        rng.state = rng.state.wrapping_add(initstate);
        rng.next_u32();
        rng
    }

    /// A generator from a single 64-bit seed (stream derived from the seed).
    #[must_use]
    pub fn from_seed(seed: u64) -> Self {
        Self::new(seed, mix64(seed ^ 0x5851_F42D_4C95_7F2D))
    }

    /// Next 32-bit output.
    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(PCG_MULT).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Next 64-bit output (two 32-bit draws).
    pub fn next_u64(&mut self) -> u64 {
        (u64::from(self.next_u32()) << 32) | u64::from(self.next_u32())
    }

    /// Uniform `f32` in `[0, 1)` (24 random bits).
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }

    /// Uniform `f64` in `[0, 1)` (53 random bits).
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }

    /// Uniform integer in `[0, bound)` without modulo bias (Lemire). Returns 0 for `bound == 0`.
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let m = u64::from(self.next_u32()) * u64::from(bound);
            if (m as u32) >= threshold {
                return (m >> 32) as u32;
            }
        }
    }

    /// Uniform `f32` in `[lo, hi)`.
    pub fn range_f32(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next_f32()
    }

    /// `true` with probability `p`.
    pub fn chance(&mut self, p: f32) -> bool {
        self.next_f32() < p
    }

    /// Normally distributed `f32` (Box–Muller with portable math).
    pub fn normal(&mut self, mean: f32, std_dev: f32) -> f32 {
        let u1 = 1.0 - self.next_f64(); // (0, 1]
        let u2 = self.next_f64();
        let z = (-2.0 * math::ln_f64(u1)).sqrt() * math::cos_f64(core::f64::consts::TAU * u2);
        mean + std_dev * z as f32
    }

    /// Index chosen with probability proportional to `weights` (non-negative). `None` if the
    /// weights are empty or sum to zero.
    pub fn weighted_index(&mut self, weights: &[f32]) -> Option<usize> {
        let total: f32 = weights.iter().copied().filter(|w| *w > 0.0).sum();
        if total <= 0.0 {
            return None;
        }
        let mut pick = self.next_f32() * total;
        let mut last = None;
        for (i, w) in weights.iter().copied().enumerate() {
            if w <= 0.0 {
                continue;
            }
            last = Some(i);
            if pick < w {
                return Some(i);
            }
            pick -= w;
        }
        last
    }
}

/// Simulation randomness streams. Values are part of the replay contract: append only.
#[repr(u64)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(missing_docs)]
pub enum RngStream {
    Citizens = 1,
    Traffic = 2,
    Weather = 3,
    Services = 4,
    Economy = 5,
    Growth = 6,
    Transit = 7,
    Test = 0xFFFF,
}

/// Counter-based RNG for simulation events.
pub struct SimRng;

impl SimRng {
    /// A generator for one decision: `purpose` distinguishes call sites sharing a stream,
    /// `t` is the tick or day, `id` the subject. Stateless — nothing to save.
    #[must_use]
    pub fn for_event(world_seed: u64, stream: RngStream, purpose: u16, t: u64, id: u64) -> Pcg32 {
        let s = derive_seed(world_seed, Domain::Event, stream as u64, u64::from(purpose));
        Pcg32::from_seed(derive_seed(s, Domain::Event, t, id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_streams_reproducible() {
        // Reference output of the PCG32 demo program (pcg32_srandom_r(42, 54)).
        let mut rng = Pcg32::new(42, 54);
        let expected = [
            0xa15c_02b7_u32,
            0x7b47_f409,
            0xba1d_3330,
            0x83d2_f293,
            0xbfa4_784b,
            0xcbed_606e,
        ];
        for e in expected {
            assert_eq!(rng.next_u32(), e);
        }
        // SplitMix64 reference vector (seed 1234567).
        let mut sm = SplitMix64::new(1_234_567);
        let expected = [
            6_457_827_717_110_365_317_u64,
            3_203_168_211_198_807_973,
            9_817_491_932_198_370_423,
            4_593_380_528_125_082_431,
            16_408_922_859_458_223_821,
        ];
        for e in expected {
            assert_eq!(sm.next_u64(), e);
        }
    }

    #[test]
    fn sim_rng_is_stateless_and_distinct() {
        let a = SimRng::for_event(1, RngStream::Citizens, 1, 100, 7).next_u64();
        let b = SimRng::for_event(1, RngStream::Citizens, 1, 100, 7).next_u64();
        assert_eq!(a, b);
        assert_ne!(
            a,
            SimRng::for_event(1, RngStream::Citizens, 2, 100, 7).next_u64()
        );
        assert_ne!(
            a,
            SimRng::for_event(1, RngStream::Traffic, 1, 100, 7).next_u64()
        );
        assert_ne!(
            a,
            SimRng::for_event(1, RngStream::Citizens, 1, 101, 7).next_u64()
        );
    }

    #[test]
    fn below_and_weighted_index_are_in_range() {
        let mut rng = Pcg32::from_seed(9);
        let mut counts = [0u32; 3];
        for _ in 0..30_000 {
            assert!(rng.below(7) < 7);
            if let Some(i) = rng.weighted_index(&[1.0, 0.0, 3.0]) {
                counts[i] += 1;
            }
        }
        assert_eq!(counts[1], 0);
        assert!(counts[2] > counts[0] * 2);
        assert_eq!(rng.weighted_index(&[0.0, -1.0]), None);
        let n = (0..10_000).map(|_| rng.normal(5.0, 2.0)).sum::<f32>() / 10_000.0;
        assert!((n - 5.0).abs() < 0.1);
    }
}
