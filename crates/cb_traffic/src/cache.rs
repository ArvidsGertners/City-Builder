//! Route cache (ALGORITHMS §12): LRU over `(origin, destination, mode, variant)` entries tagged
//! with the graph version and cost epoch — only current-epoch entries hit. A reverse index from
//! links to entries evicts every route through a removed link; a negative cache remembers
//! unreachable pairs per graph version. Ordered maps keep every operation deterministic.

use std::collections::{BTreeMap, BTreeSet};

use crate::router::Route;

/// Cache key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key {
    /// Origin node.
    pub origin: u32,
    /// Destination node.
    pub dest: u32,
    /// Mode view (0 = car).
    pub mode: u8,
    /// Route variant 0–3.
    pub variant: u8,
}

#[derive(Clone, Debug, PartialEq)]
struct Entry {
    route: Route,
    version: u64,
    epoch: u64,
    stamp: u64,
}

/// The cache.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RouteCache {
    capacity: usize,
    entries: BTreeMap<Key, Entry>,
    lru: BTreeMap<u64, Key>,
    by_link: BTreeMap<u32, BTreeSet<Key>>,
    negative: BTreeMap<Key, u64>,
    clock: u64,
    /// Hits and misses since creation.
    pub hits: u64,
    /// Misses.
    pub misses: u64,
}

impl RouteCache {
    /// A cache holding at most `capacity` routes.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            ..Self::default()
        }
    }

    /// Number of cached routes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// A route for `key` valid for this graph version and epoch. `Some(None)`: known
    /// unreachable; `None`: not cached.
    pub fn get(&mut self, key: Key, version: u64, epoch: u64) -> Option<Option<Route>> {
        if self.negative.get(&key) == Some(&version) {
            self.hits += 1;
            return Some(None);
        }
        self.clock += 1;
        let stamp = self.clock;
        match self.entries.get_mut(&key) {
            Some(e) if e.version == version && e.epoch == epoch => {
                self.lru.remove(&e.stamp);
                e.stamp = stamp;
                self.lru.insert(stamp, key);
                self.hits += 1;
                Some(Some(e.route.clone()))
            }
            _ => {
                self.misses += 1;
                None
            }
        }
    }

    /// Stores a search result.
    pub fn insert(&mut self, key: Key, route: Option<Route>, version: u64, epoch: u64) {
        let Some(route) = route else {
            self.negative.insert(key, version);
            return;
        };
        self.remove(key);
        while self.entries.len() >= self.capacity {
            let Some((_, oldest)) = self.lru.iter().next().map(|(s, k)| (*s, *k)) else {
                break;
            };
            self.remove(oldest);
        }
        self.clock += 1;
        for l in &route.links {
            self.by_link.entry(*l).or_default().insert(key);
        }
        self.lru.insert(self.clock, key);
        self.entries.insert(
            key,
            Entry {
                route,
                version,
                epoch,
                stamp: self.clock,
            },
        );
    }

    fn remove(&mut self, key: Key) {
        if let Some(e) = self.entries.remove(&key) {
            self.lru.remove(&e.stamp);
            for l in &e.route.links {
                if let Some(s) = self.by_link.get_mut(l) {
                    s.remove(&key);
                    if s.is_empty() {
                        self.by_link.remove(l);
                    }
                }
            }
        }
    }

    /// Evicts every route through link `l` (the link was removed or rebuilt).
    pub fn evict_link(&mut self, l: u32) {
        let keys: Vec<Key> = self
            .by_link
            .get(&l)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default();
        for k in keys {
            self.remove(k);
        }
    }

    /// Drops every entry (graph rebuilt with new link numbering).
    pub fn clear(&mut self) {
        let cap = self.capacity;
        let (h, m) = (self.hits, self.misses);
        *self = Self::new(cap);
        self.hits = h;
        self.misses = m;
    }
}
