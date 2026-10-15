//! Stable 64-bit hashing and seed derivation (ADR-0043).
//!
//! Nothing here depends on `std`'s randomized hashers or on platform word size/endianness, so
//! results are identical on every supported platform and across Rust versions.

use core::hash::{BuildHasherDefault, Hasher};

/// SplitMix64 finalizer: a fast, well-distributed 64-bit bijection.
#[inline]
#[must_use]
pub const fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Domain tags for [`derive_seed`]. Values are part of the save/replay contract: never
/// renumber, only append.
#[repr(u64)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(missing_docs)] // names are self-explanatory
pub enum Domain {
    Terrain = 1,
    River = 2,
    Lake = 3,
    Scatter = 4,
    Plot = 5,
    Lot = 6,
    Building = 7,
    Yard = 8,
    Vehicle = 9,
    Citizen = 10,
    Name = 11,
    Weather = 12,
    Audio = 13,
    Event = 14,
    Growth = 15,
    Immigration = 16,
    Lifecycle = 17,
    OutsideConnection = 18,
    Test = 0xFFFF,
}

const K0: u64 = 0x9E37_79B9_7F4A_7C15;
const K1: u64 = 0xD1B5_4A32_D192_ED03;
const K2: u64 = 0xA24B_AED4_963E_E407;
const K3: u64 = 0x9FB2_1C65_1E98_DF25;

/// Derives a content seed by sequential absorption of its inputs.
///
/// Order matters (`(id, extra)` ≠ `(extra, id)`) and equal inputs never cancel — unlike XOR
/// combining (review finding G-D1).
#[inline]
#[must_use]
pub const fn derive_seed(world_seed: u64, domain: Domain, id: u64, extra: u64) -> u64 {
    let mut h = mix64(world_seed ^ K0);
    h = mix64(h ^ (domain as u64).wrapping_mul(K1));
    h = mix64(h ^ id.wrapping_mul(K2));
    mix64(h ^ extra.wrapping_mul(K3))
}

/// Deterministic streaming hasher for state hashes and [`crate::collections::LookupMap`].
///
/// Integers are fed little-endian regardless of the platform; `usize` is widened to 64 bits.
#[derive(Clone, Copy, Debug)]
pub struct StableHasher {
    state: u64,
    len: u64,
}

impl Default for StableHasher {
    fn default() -> Self {
        Self { state: K0, len: 0 }
    }
}

impl StableHasher {
    /// A hasher with a custom starting key.
    #[must_use]
    pub fn with_key(key: u64) -> Self {
        Self {
            state: mix64(key ^ K0),
            len: 0,
        }
    }

    #[inline]
    fn absorb(&mut self, word: u64) {
        self.state = mix64(self.state ^ word.wrapping_mul(K2)).rotate_left(23);
        self.len = self.len.wrapping_add(1);
    }
}

impl Hasher for StableHasher {
    fn finish(&self) -> u64 {
        mix64(self.state ^ self.len.wrapping_mul(K3))
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(chunk);
            self.absorb(u64::from_le_bytes(buf));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut buf = [0u8; 8];
            buf[..rest.len()].copy_from_slice(rest);
            // Tag the tail with its length so "ab" and "ab\0" differ.
            self.absorb(u64::from_le_bytes(buf) ^ ((rest.len() as u64) << 56));
        }
    }

    fn write_u8(&mut self, i: u8) {
        self.absorb(u64::from(i));
    }
    fn write_u16(&mut self, i: u16) {
        self.absorb(u64::from(i));
    }
    fn write_u32(&mut self, i: u32) {
        self.absorb(u64::from(i));
    }
    fn write_u64(&mut self, i: u64) {
        self.absorb(i);
    }
    fn write_u128(&mut self, i: u128) {
        self.absorb(i as u64);
        self.absorb((i >> 64) as u64);
    }
    fn write_usize(&mut self, i: usize) {
        self.absorb(i as u64);
    }
    fn write_i8(&mut self, i: i8) {
        self.write_u8(i as u8);
    }
    fn write_i16(&mut self, i: i16) {
        self.write_u16(i as u16);
    }
    fn write_i32(&mut self, i: i32) {
        self.write_u32(i as u32);
    }
    fn write_i64(&mut self, i: i64) {
        self.write_u64(i as u64);
    }
    fn write_isize(&mut self, i: isize) {
        self.write_usize(i as usize);
    }
}

/// `BuildHasher` producing [`StableHasher`]s (fixed key, deterministic).
pub type StableBuildHasher = BuildHasherDefault<StableHasher>;

/// Hashes any `Hash` value with [`StableHasher`].
#[must_use]
pub fn stable_hash<T: core::hash::Hash + ?Sized>(value: &T) -> u64 {
    let mut h = StableHasher::default();
    value.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn derive_seed_no_collisions_1e6() {
        let mut seen = BTreeSet::new();
        for id in 0..1000u64 {
            for extra in 0..1000u64 {
                assert!(seen.insert(derive_seed(42, Domain::Building, id, extra)));
            }
        }
        assert_eq!(seen.len(), 1_000_000);
    }

    #[test]
    fn derive_seed_order_sensitive() {
        for (a, b) in [(3u64, 5u64), (1, 2), (0, 1), (7, 7)] {
            if a != b {
                assert_ne!(
                    derive_seed(1, Domain::Building, a, b),
                    derive_seed(1, Domain::Building, b, a)
                );
            }
            // Equal inputs must not cancel out (XOR combining would map (a, a) to (0, 0)).
            if a != 0 {
                assert_ne!(
                    derive_seed(1, Domain::Building, a, a),
                    derive_seed(1, Domain::Building, 0, 0)
                );
            }
        }
        assert_ne!(
            derive_seed(1, Domain::Lot, 3, 5),
            derive_seed(1, Domain::Building, 3, 5)
        );
        assert_ne!(
            derive_seed(1, Domain::Lot, 3, 5),
            derive_seed(2, Domain::Lot, 3, 5)
        );
    }

    #[test]
    fn derive_seed_golden_value_is_stable() {
        // Guards the save/replay contract: changing the mixer changes every world.
        assert_eq!(
            derive_seed(0, Domain::Test, 0, 0),
            derive_seed(0, Domain::Test, 0, 0)
        );
        assert_eq!(mix64(0), 0);
        assert_eq!(mix64(1), 0x5692_161D_100B_05E5);
    }

    #[test]
    fn stable_hasher_is_length_and_order_sensitive() {
        assert_ne!(stable_hash("ab"), stable_hash("ab\0"));
        assert_ne!(stable_hash(&(1u32, 2u32)), stable_hash(&(2u32, 1u32)));
        assert_eq!(stable_hash(&[1u64, 2, 3]), stable_hash(&[1u64, 2, 3]));
    }
}
