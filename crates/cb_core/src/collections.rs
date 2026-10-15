//! Deterministic collections (ADR-0043).
//!
//! [`LookupMap`] wraps a hash map with a fixed hasher and **no iteration API**: lookups are
//! fine, but iteration order of a hash map depends on insertion history, which differs after
//! save/load, so results must never depend on it. Iterate via [`LookupMap::sorted_keys`] or use
//! `BTreeMap`.
//!
//! ```compile_fail
//! use cb_core::collections::LookupMap;
//! let map: LookupMap<u32, u32> = LookupMap::new();
//! for _ in map.iter() {} // no iteration API by design
//! ```

#![allow(clippy::disallowed_types)] // the one sanctioned wrapper around std's HashMap

use crate::hash::StableBuildHasher;
use core::hash::Hash;
use std::collections::HashMap;

/// Hash map for lookups only (no iteration). See the module docs.
#[derive(Clone, Debug)]
pub struct LookupMap<K, V> {
    inner: HashMap<K, V, StableBuildHasher>,
}

impl<K: Eq + Hash, V> Default for LookupMap<K, V> {
    fn default() -> Self {
        Self {
            inner: HashMap::default(),
        }
    }
}

impl<K: Eq + Hash, V> LookupMap<K, V> {
    /// An empty map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty map with room for `capacity` entries.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: HashMap::with_capacity_and_hasher(capacity, StableBuildHasher::default()),
        }
    }

    /// Inserts, returning the previous value.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.inner.insert(key, value)
    }

    /// Looks up a value.
    #[must_use]
    pub fn get(&self, key: &K) -> Option<&V> {
        self.inner.get(key)
    }

    /// Looks up a value mutably.
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.inner.get_mut(key)
    }

    /// Value for `key`, inserting `default()` first if absent.
    pub fn get_or_insert_with(&mut self, key: K, default: impl FnOnce() -> V) -> &mut V {
        self.inner.entry(key).or_insert_with(default)
    }

    /// Removes and returns a value.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.inner.remove(key)
    }

    /// Whether `key` is present.
    #[must_use]
    pub fn contains_key(&self, key: &K) -> bool {
        self.inner.contains_key(key)
    }

    /// Number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether the map is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Removes all entries.
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// All keys in ascending order — the only sanctioned way to visit every entry.
    #[must_use]
    pub fn sorted_keys(&self) -> Vec<K>
    where
        K: Ord + Copy,
    {
        let mut keys: Vec<K> = self.inner.keys().copied().collect();
        keys.sort_unstable();
        keys
    }
}

/// Fixed-size bit set over `0..len`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BitSet {
    words: Vec<u64>,
    len: usize,
}

impl BitSet {
    /// A cleared set able to hold `len` bits.
    #[must_use]
    pub fn new(len: usize) -> Self {
        Self {
            words: vec![0; len.div_ceil(64)],
            len,
        }
    }

    /// Capacity in bits.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the capacity is zero.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Sets bit `i` (ignored if out of range).
    pub fn set(&mut self, i: usize, value: bool) {
        if i >= self.len {
            return;
        }
        let (w, b) = (i / 64, i % 64);
        if value {
            self.words[w] |= 1 << b;
        } else {
            self.words[w] &= !(1 << b);
        }
    }

    /// Reads bit `i` (`false` if out of range).
    #[must_use]
    pub fn get(&self, i: usize) -> bool {
        i < self.len && self.words[i / 64] & (1 << (i % 64)) != 0
    }

    /// Number of set bits.
    #[must_use]
    pub fn count_ones(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// Indices of set bits in ascending order.
    pub fn ones(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.len).filter(|i| self.get(*i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_map_basic_and_sorted_keys() {
        let mut m = LookupMap::new();
        for k in [5u32, 1, 9, 3] {
            m.insert(k, k * 10);
        }
        assert_eq!(m.get(&9), Some(&90));
        assert_eq!(m.sorted_keys(), vec![1, 3, 5, 9]);
        *m.get_or_insert_with(2, || 0) += 7;
        assert_eq!(m.remove(&2), Some(7));
        assert_eq!(m.len(), 4);
    }

    #[test]
    fn bitset_roundtrip() {
        let mut b = BitSet::new(130);
        b.set(0, true);
        b.set(129, true);
        b.set(200, true); // ignored
        assert!(b.get(129) && b.get(0) && !b.get(64));
        assert_eq!(b.ones().collect::<Vec<_>>(), vec![0, 129]);
        assert_eq!(b.count_ones(), 2);
    }
}
