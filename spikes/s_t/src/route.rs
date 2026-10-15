//! ALT A* over links with variants, epoch costs, LRU cache and a cost-unit budget (ADR-0048).

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::sync::Arc;

use crate::net::{Net, Turn};

pub const LANDMARKS: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Key {
    pub o: u32,
    pub d: u32,
    pub variant: u8,
}

pub struct Entry {
    pub route: Arc<Vec<u32>>,
    pub cost: u64,
    pub epoch: u64,
    pub stamp: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CachePolicy {
    /// ADR-0048: only entries of the current epoch hit.
    Epoch,
    /// Proposed: re-cost cached paths under current costs; hit if ≤ 15 % above the cached cost.
    Revalidate,
}

pub struct Router {
    /// succ[l] = (next link, turn penalty ms)
    pub succ: Vec<Vec<(u32, u32)>>,
    pub pred: Vec<Vec<(u32, u32)>>,
    /// Free-flow link cost (ms).
    pub free: Vec<u32>,
    /// Current epoch cost (ms).
    pub cost: Vec<u32>,
    pub lm_from: Vec<Vec<u32>>,
    pub lm_to: Vec<Vec<u32>>,
    pub epoch: u64,
    pub cache: HashMap<Key, Entry>,
    pub lru: VecDeque<(u64, Key)>,
    pub stamp: u64,
    pub cap: usize,
    pub policy: CachePolicy,
    pub hits: u64,
    pub misses: u64,
    pub units_total: u64,
    /// Reverse index link → keys (for eviction on removal).
    pub by_link: HashMap<u32, Vec<Key>>,
    // Search scratch.
    g: Vec<u64>,
    seen: Vec<u32>,
    parent: Vec<u32>,
    hval: Vec<u64>,
    active: Vec<usize>,
    epoch_gen: u32,
}

fn turn_pen(t: Turn) -> u32 {
    match t {
        Turn::Right => 2000,
        Turn::Straight => 0,
        Turn::Left => 5000,
        Turn::U => 20_000,
    }
}

fn hash3(a: u64, b: u64, c: u64) -> u64 {
    let mut h = a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ b.wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ c.wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^ (h >> 29)
}

impl Router {
    pub fn new(net: &Net, policy: CachePolicy) -> Self {
        let n = net.links.len();
        let mut r = Router {
            succ: vec![Vec::new(); n],
            pred: vec![Vec::new(); n],
            free: vec![0; n],
            cost: vec![0; n],
            lm_from: Vec::new(),
            lm_to: Vec::new(),
            epoch: 0,
            cache: HashMap::new(),
            lru: VecDeque::new(),
            stamp: 0,
            cap: 32_768,
            policy,
            hits: 0,
            misses: 0,
            units_total: 0,
            by_link: HashMap::new(),
            g: vec![u64::MAX; n],
            seen: vec![0; n],
            parent: vec![u32::MAX; n],
            hval: vec![0; n],
            active: Vec::new(),
            epoch_gen: 0,
        };
        r.rebuild(net);
        r.landmarks(net);
        r
    }

    pub fn rebuild(&mut self, net: &Net) {
        let n = net.links.len();
        for v in &mut self.succ {
            v.clear();
        }
        for v in &mut self.pred {
            v.clear();
        }
        for c in &net.conns {
            let (a, b) = (c.from_link, c.to_link);
            if !net.links[a as usize].alive || !net.links[b as usize].alive {
                continue;
            }
            let p = turn_pen(c.turn);
            if !self.succ[a as usize].iter().any(|e| e.0 == b) {
                self.succ[a as usize].push((b, p));
                self.pred[b as usize].push((a, p));
            }
        }
        for l in 0..n {
            self.succ[l].sort_unstable();
            self.pred[l].sort_unstable();
            let k = &net.links[l];
            self.free[l] = ((k.len / k.v_free) * 1000.0) as u32 + 1;
        }
        if self.cost.iter().all(|c| *c == 0) {
            self.cost = self.free.clone();
        }
    }

    fn dijkstra(&self, src: u32, fwd: bool) -> Vec<u32> {
        let n = self.free.len();
        let mut d = vec![u32::MAX; n];
        let mut h = BinaryHeap::new();
        d[src as usize] = 0;
        h.push(Reverse((0u32, src)));
        while let Some(Reverse((dv, v))) = h.pop() {
            if dv > d[v as usize] {
                continue;
            }
            let adj = if fwd { &self.succ[v as usize] } else { &self.pred[v as usize] };
            for &(w, p) in adj {
                // Cost of entering w (forward) / leaving v toward w (backward) uses the head link.
                let c = if fwd { self.free[w as usize] + p } else { self.free[v as usize] + p };
                let nd = dv.saturating_add(c);
                if nd < d[w as usize] {
                    d[w as usize] = nd;
                    h.push(Reverse((nd, w)));
                }
            }
        }
        d
    }

    /// 16 landmarks by farthest-point selection on free-flow costs (deterministic).
    pub fn landmarks(&mut self, net: &Net) {
        let n = net.links.len();
        let alive: Vec<u32> = (0..n as u32).filter(|l| net.links[*l as usize].alive && !self.succ[*l as usize].is_empty()).collect();
        if alive.is_empty() {
            return;
        }
        let mut chosen = vec![alive[0]];
        let mut mind = vec![u32::MAX; n];
        self.lm_from.clear();
        self.lm_to.clear();
        while chosen.len() <= LANDMARKS {
            let l = *chosen.last().unwrap_or(&alive[0]);
            let f = self.dijkstra(l, true);
            let t = self.dijkstra(l, false);
            for v in 0..n {
                mind[v] = mind[v].min(f[v].min(t[v]));
            }
            if chosen.len() > 1 || self.lm_from.is_empty() {
                self.lm_from.push(f);
                self.lm_to.push(t);
            }
            let next = alive.iter().copied().filter(|v| mind[*v as usize] != u32::MAX).max_by_key(|v| (mind[*v as usize], Reverse(*v)));
            match next {
                Some(v) if !chosen.contains(&v) => chosen.push(v),
                _ => break,
            }
            if self.lm_from.len() >= LANDMARKS {
                break;
            }
        }
    }

    fn h(&self, v: u32, t: u32) -> u64 {
        let mut best = 0u64;
        for &k in &self.active {
            let (fv, ft) = (self.lm_from[k][v as usize], self.lm_from[k][t as usize]);
            let (tv, tt) = (self.lm_to[k][v as usize], self.lm_to[k][t as usize]);
            if fv != u32::MAX && ft != u32::MAX && ft > fv {
                best = best.max(u64::from(ft - fv));
            }
            if tv != u32::MAX && tt != u32::MAX && tv > tt {
                best = best.max(u64::from(tv - tt));
            }
        }
        best
    }

    fn link_cost(&self, l: u32, variant: u8) -> u64 {
        let base = u64::from(self.cost[l as usize]);
        let j = hash3(u64::from(l), u64::from(variant), self.epoch) % 2001; // +0…20 %
        base + base * j / 10_000
    }

    /// A* (ALT); returns (route, cost, settled nodes).
    pub fn search(&mut self, net: &Net, o: u32, d: u32, variant: u8) -> (Option<Vec<u32>>, u64, u64) {
        self.epoch_gen = self.epoch_gen.wrapping_add(1);
        let epoch_gen = self.epoch_gen;
        // Active landmarks: the 4 with the best bound for this (o, d).
        let mut scored: Vec<(u64, usize)> = (0..self.lm_from.len())
            .map(|k| {
                self.active = vec![k];
                (self.h(o, d), k)
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        self.active = scored.iter().take(4).map(|x| x.1).collect();
        let mut heap: BinaryHeap<Reverse<(u64, Reverse<u64>, u32)>> = BinaryHeap::new();
        self.g[o as usize] = 0;
        self.seen[o as usize] = epoch_gen;
        self.parent[o as usize] = u32::MAX;
        heap.push(Reverse((self.h(o, d), Reverse(0), o)));
        let mut settled = 0u64;
        while let Some(Reverse((_, Reverse(gv), v))) = heap.pop() {
            if gv > self.g[v as usize] {
                continue;
            }
            settled += 1;
            if v == d {
                let mut path = vec![d];
                let mut c = d;
                while self.parent[c as usize] != u32::MAX {
                    c = self.parent[c as usize];
                    path.push(c);
                }
                path.reverse();
                return (Some(path), gv, settled);
            }
            for i in 0..self.succ[v as usize].len() {
                let (w, p) = self.succ[v as usize][i];
                if !net.links[w as usize].alive {
                    continue;
                }
                let ng = gv + self.link_cost(w, variant) + u64::from(p);
                let fresh = self.seen[w as usize] != epoch_gen;
                if fresh || ng < self.g[w as usize] {
                    if fresh {
                        self.hval[w as usize] = self.h(w, d);
                    }
                    self.seen[w as usize] = epoch_gen;
                    self.g[w as usize] = ng;
                    self.parent[w as usize] = v;
                    heap.push(Reverse((ng + self.hval[w as usize], Reverse(ng), w)));
                }
            }
        }
        (None, 0, settled)
    }

    fn path_cost(&self, path: &[u32], variant: u8) -> u64 {
        let mut c = 0;
        for w in path.windows(2) {
            let p = self.succ[w[0] as usize].iter().find(|e| e.0 == w[1]).map_or(u64::MAX / 4, |e| u64::from(e.1));
            c += self.link_cost(w[1], variant) + p;
        }
        c
    }

    /// Cached route lookup + search on miss. Returns (route, cost units).
    pub fn route(&mut self, net: &Net, k: Key) -> (Option<Arc<Vec<u32>>>, u64) {
        self.stamp += 1;
        if let Some(e) = self.cache.get(&k) {
            let ok = match self.policy {
                CachePolicy::Epoch => e.epoch == self.epoch,
                CachePolicy::Revalidate => {
                    let alive = e.route.iter().all(|l| net.links[*l as usize].alive);
                    alive && (e.epoch == self.epoch || self.path_cost(&e.route.clone(), k.variant) * 100 <= e.cost * 115)
                }
            };
            if ok {
                let r = e.route.clone();
                let units = 1 + (r.len() as u64 / 8) * u64::from(self.policy == CachePolicy::Revalidate);
                if let Some(e) = self.cache.get_mut(&k) {
                    e.stamp = self.stamp;
                }
                self.lru.push_back((self.stamp, k));
                self.hits += 1;
                self.units_total += units;
                return (Some(r), units);
            }
        }
        self.misses += 1;
        let (path, cost, settled) = self.search(net, k.o, k.d, k.variant);
        self.units_total += settled;
        let route = path.map(Arc::new);
        if let Some(r) = &route {
            for l in r.iter() {
                self.by_link.entry(*l).or_default().push(k);
            }
            self.cache.insert(k, Entry { route: r.clone(), cost, epoch: self.epoch, stamp: self.stamp });
            self.lru.push_back((self.stamp, k));
            while self.cache.len() > self.cap {
                let Some((st, kk)) = self.lru.pop_front() else { break };
                if self.cache.get(&kk).is_some_and(|e| e.stamp == st) {
                    self.cache.remove(&kk);
                }
            }
        }
        (route, settled.max(1))
    }

    pub fn evict_link(&mut self, l: u32) {
        if let Some(keys) = self.by_link.remove(&l) {
            for k in keys {
                self.cache.remove(&k);
            }
        }
    }

    /// New epoch: snapshot congested costs.
    pub fn new_epoch(&mut self, epoch: u64, cost: Vec<u32>) {
        self.epoch = epoch;
        self.cost = cost;
    }
}
