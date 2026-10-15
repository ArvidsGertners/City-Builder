//! ALT A* over links with variants, epoch costs, LRU cache and a cost-unit budget (ADR-0048).
//!
//! Layout for speed: CSR adjacency, one 24-byte scratch record per link, landmark distances
//! interleaved per link (one link's 16 × (from, to) pairs are contiguous). Searches are pure
//! functions of (graph, costs, key), so a batch of cache misses can run on the worker pool and
//! be inserted in request order (deterministic for any thread count).

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use crate::net::{Net, Turn};
use crate::pool::Pool;

pub const LANDMARKS: usize = 16;
const ACTIVE: usize = 4;
/// Searches per parallel batch (fixed: independent of the thread count).
pub const BATCH: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Key {
    pub o: u32,
    pub d: u32,
    pub variant: u8,
}

pub struct Entry {
    pub route: Arc<Vec<u32>>,
    /// Lane plan for the route (filled by the sim on first use; topology-only).
    pub plan: Option<Arc<Vec<u16>>>,
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

#[derive(Clone, Copy, Default)]
struct Scr {
    g: u64,
    h: u64,
    parent: u32,
    seen: u32,
}

/// Per-search scratch (one per batch slot).
#[derive(Default)]
pub struct Scratch {
    n: Vec<Scr>,
    epoch_gen: u32,
    active: [usize; ACTIVE],
    /// Landmark distances of the target for the active landmarks: (from L to t, t to L).
    tgt: [(u32, u32); ACTIVE],
    heap: BinaryHeap<Reverse<(u64, Reverse<u64>, u32)>>,
    /// In-progress sliced search: (target, variant), settled so far, finished result.
    run: Option<(u32, u8)>,
    settled: u64,
    done: Option<SearchResult>,
}

pub type SearchResult = (Option<Vec<u32>>, u64, u64);

pub struct Router {
    /// CSR successors: edges[off[l]..off[l+1]] = (next link, turn penalty ms).
    off: Vec<u32>,
    edges: Vec<(u32, u32)>,
    poff: Vec<u32>,
    pedges: Vec<(u32, u32)>,
    /// Free-flow link cost (ms).
    pub free: Vec<u32>,
    /// Current epoch cost (ms).
    pub cost: Vec<u32>,
    /// lm[l * 2 * LANDMARKS + 2 * k] = d(L_k → l), [... + 1] = d(l → L_k).
    lm: Vec<u32>,
    nlm: usize,
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
    slots: Vec<Mutex<Scratch>>,
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
            off: Vec::new(),
            edges: Vec::new(),
            poff: Vec::new(),
            pedges: Vec::new(),
            free: vec![0; n],
            cost: vec![0; n],
            lm: Vec::new(),
            nlm: 0,
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
            slots: (0..BATCH + 1).map(|_| Mutex::new(Scratch::default())).collect(),
        };
        r.rebuild(net);
        r.landmarks(net);
        r
    }

    pub fn rebuild(&mut self, net: &Net) {
        let n = net.links.len();
        let mut succ: Vec<Vec<(u32, u32)>> = vec![Vec::new(); n];
        let mut pred: Vec<Vec<(u32, u32)>> = vec![Vec::new(); n];
        for c in &net.conns {
            let (a, b) = (c.from_link, c.to_link);
            if !net.links[a as usize].alive || !net.links[b as usize].alive {
                continue;
            }
            let p = turn_pen(c.turn);
            if !succ[a as usize].iter().any(|e| e.0 == b) {
                succ[a as usize].push((b, p));
                pred[b as usize].push((a, p));
            }
        }
        let csr = |adj: &mut Vec<Vec<(u32, u32)>>| {
            let mut off = Vec::with_capacity(n + 1);
            let mut e = Vec::new();
            for v in adj.iter_mut() {
                v.sort_unstable();
                off.push(e.len() as u32);
                e.extend_from_slice(v);
            }
            off.push(e.len() as u32);
            (off, e)
        };
        (self.off, self.edges) = csr(&mut succ);
        (self.poff, self.pedges) = csr(&mut pred);
        for l in 0..n {
            let k = &net.links[l];
            self.free[l] = ((k.len / k.v_free) * 1000.0) as u32 + 1;
        }
        if self.cost.iter().all(|c| *c == 0) {
            self.cost = self.free.clone();
        }
    }

    fn succ(&self, l: u32) -> &[(u32, u32)] {
        &self.edges[self.off[l as usize] as usize..self.off[l as usize + 1] as usize]
    }

    fn pred(&self, l: u32) -> &[(u32, u32)] {
        &self.pedges[self.poff[l as usize] as usize..self.poff[l as usize + 1] as usize]
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
            let adj = if fwd { self.succ(v) } else { self.pred(v) };
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
        let alive: Vec<u32> = (0..n as u32).filter(|l| net.links[*l as usize].alive && !self.succ(*l).is_empty()).collect();
        if alive.is_empty() {
            return;
        }
        let mut chosen = vec![alive[0]];
        let mut mind = vec![u32::MAX; n];
        let mut from: Vec<Vec<u32>> = Vec::new();
        let mut to: Vec<Vec<u32>> = Vec::new();
        while from.len() < LANDMARKS {
            let l = *chosen.last().unwrap_or(&alive[0]);
            let f = self.dijkstra(l, true);
            let t = self.dijkstra(l, false);
            for v in 0..n {
                mind[v] = mind[v].min(f[v].min(t[v]));
            }
            from.push(f);
            to.push(t);
            let next = alive.iter().copied().filter(|v| mind[*v as usize] != u32::MAX).max_by_key(|v| (mind[*v as usize], Reverse(*v)));
            match next {
                Some(v) if !chosen.contains(&v) => chosen.push(v),
                _ => break,
            }
        }
        self.nlm = from.len();
        self.lm = vec![u32::MAX; n * 2 * LANDMARKS];
        for v in 0..n {
            for k in 0..self.nlm {
                self.lm[v * 2 * LANDMARKS + 2 * k] = from[k][v];
                self.lm[v * 2 * LANDMARKS + 2 * k + 1] = to[k][v];
            }
        }
    }

    #[inline]
    fn h_one(&self, v: u32, k: usize, tgt: (u32, u32)) -> u64 {
        let b = v as usize * 2 * LANDMARKS + 2 * k;
        let (fv, tv) = (self.lm[b], self.lm[b + 1]);
        let (ft, tt) = tgt;
        let mut best = 0u64;
        if fv != u32::MAX && ft != u32::MAX && ft > fv {
            best = u64::from(ft - fv);
        }
        if tv != u32::MAX && tt != u32::MAX && tv > tt {
            best = best.max(u64::from(tv - tt));
        }
        best
    }

    fn h(&self, sc: &Scratch, v: u32) -> u64 {
        let mut best = 0;
        for i in 0..ACTIVE.min(self.nlm) {
            best = best.max(self.h_one(v, sc.active[i], sc.tgt[i]));
        }
        best
    }

    fn link_cost(&self, l: u32, variant: u8) -> u64 {
        let base = u64::from(self.cost[l as usize]);
        let j = hash3(u64::from(l), u64::from(variant), self.epoch) % 2001; // +0…20 %
        base + base * j / 10_000
    }

    /// Starts an A* (ALT) search from `sources` (each with an initial cost) to `d`.
    pub fn init(&self, sc: &mut Scratch, sources: &[(u32, u64)], d: u32, variant: u8) {
        let n = self.free.len();
        if sc.n.len() != n {
            sc.n = vec![Scr::default(); n];
            sc.epoch_gen = 0;
        }
        sc.epoch_gen = sc.epoch_gen.wrapping_add(1);
        if sc.epoch_gen == 0 {
            sc.n.iter_mut().for_each(|s| s.seen = 0);
            sc.epoch_gen = 1;
        }
        let eg = sc.epoch_gen;
        let o = sources.first().map_or(d, |x| x.0);
        // Active landmarks: the 4 with the best bound for this (o, d).
        let tb = |k: usize| {
            let b = d as usize * 2 * LANDMARKS + 2 * k;
            (self.lm[b], self.lm[b + 1])
        };
        let mut scored: [(u64, usize); LANDMARKS] = [(0, 0); LANDMARKS];
        for (k, e) in scored.iter_mut().enumerate().take(self.nlm) {
            *e = (self.h_one(o, k, tb(k)), k);
        }
        let sl = &mut scored[..self.nlm];
        sl.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        for i in 0..ACTIVE.min(self.nlm) {
            sc.active[i] = sl[i].1;
            sc.tgt[i] = tb(sl[i].1);
        }
        sc.heap.clear();
        for &(src, g0) in sources {
            let h0 = self.h(sc, src);
            let e = sc.n[src as usize];
            if e.seen == eg && e.g <= g0 {
                continue;
            }
            sc.n[src as usize] = Scr { g: g0, h: h0, parent: u32::MAX, seen: eg };
            sc.heap.push(Reverse((g0 + h0, Reverse(g0), src)));
        }
        sc.run = Some((d, variant));
        sc.settled = 0;
        sc.done = None;
    }

    /// Advances the search by up to `budget` settled nodes; true when finished.
    pub fn advance(&self, sc: &mut Scratch, budget: u64) -> bool {
        let Some((d, variant)) = sc.run else { return true };
        let eg = sc.epoch_gen;
        let mut heap = std::mem::take(&mut sc.heap);
        let mut steps = 0u64;
        let mut out: Option<SearchResult> = None;
        while steps < budget {
            let Some(Reverse((_, Reverse(gv), v))) = heap.pop() else {
                out = Some((None, 0, sc.settled));
                break;
            };
            if gv > sc.n[v as usize].g {
                continue;
            }
            steps += 1;
            sc.settled += 1;
            if v == d {
                let mut path = vec![d];
                let mut c = d;
                while sc.n[c as usize].parent != u32::MAX {
                    c = sc.n[c as usize].parent;
                    path.push(c);
                }
                path.reverse();
                out = Some((Some(path), gv, sc.settled));
                break;
            }
            for &(w, p) in self.succ(v) {
                let ng = gv + self.link_cost(w, variant) + u64::from(p);
                let e = sc.n[w as usize];
                let fresh = e.seen != eg;
                if fresh || ng < e.g {
                    let hw = if fresh { self.h(sc, w) } else { e.h };
                    sc.n[w as usize] = Scr { g: ng, h: hw, parent: v, seen: eg };
                    heap.push(Reverse((ng + hw, Reverse(ng), w)));
                }
            }
        }
        sc.heap = heap;
        if out.is_some() {
            sc.done = out;
            sc.run = None;
            true
        } else {
            false
        }
    }

    /// Full search (unsliced). Pure in (graph, costs, key).
    pub fn search(&self, sc: &mut Scratch, o: u32, d: u32, variant: u8) -> SearchResult {
        self.init(sc, &[(o, 0)], d, variant);
        self.advance(sc, u64::MAX);
        sc.done.take().unwrap_or((None, 0, 0))
    }

    /// Cost of entering link `l` (variant jitter) — for multi-source starts.
    pub fn enter_cost(&self, l: u32, variant: u8) -> u64 {
        self.link_cost(l, variant)
    }

    fn path_cost(&self, path: &[u32], variant: u8) -> u64 {
        let mut c = 0;
        for w in path.windows(2) {
            let p = self.succ(w[0]).iter().find(|e| e.0 == w[1]).map_or(u64::MAX / 4, |e| u64::from(e.1));
            c += self.link_cost(w[1], variant) + p;
        }
        c
    }

    /// Cache lookup only (hit path). Returns (route, cost units) on a hit.
    pub fn lookup(&mut self, net: &Net, k: Key) -> Option<(Arc<Vec<u32>>, Option<Arc<Vec<u16>>>, u64)> {
        self.stamp += 1;
        let e = self.cache.get(&k)?;
        let ok = match self.policy {
            CachePolicy::Epoch => e.epoch == self.epoch,
            CachePolicy::Revalidate => {
                let alive = e.route.iter().all(|l| net.links[*l as usize].alive);
                alive && (e.epoch == self.epoch || self.path_cost(&e.route, k.variant) * 100 <= e.cost * 115)
            }
        };
        if !ok {
            return None;
        }
        let r = e.route.clone();
        let plan = e.plan.clone();
        let units = 1 + (r.len() as u64 / 8) * u64::from(self.policy == CachePolicy::Revalidate);
        if let Some(e) = self.cache.get_mut(&k) {
            e.stamp = self.stamp;
        }
        self.lru.push_back((self.stamp, k));
        self.hits += 1;
        self.units_total += units;
        Some((r, plan, units))
    }

    pub fn set_plan(&mut self, k: Key, plan: Arc<Vec<u16>>) {
        if let Some(e) = self.cache.get_mut(&k) {
            e.plan = Some(plan);
        }
    }

    /// Inserts a search result (counts the miss and its units).
    pub fn insert(&mut self, k: Key, res: SearchResult) -> (Option<Arc<Vec<u32>>>, u64) {
        let (path, cost, settled) = res;
        self.misses += 1;
        self.units_total += settled;
        self.stamp += 1;
        let route = path.map(Arc::new);
        if let Some(r) = &route {
            for l in r.iter() {
                self.by_link.entry(*l).or_default().push(k);
            }
            self.cache.insert(k, Entry { route: r.clone(), plan: None, cost, epoch: self.epoch, stamp: self.stamp });
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

    /// Cached route lookup + sequential search on miss. Returns (route, cost units).
    pub fn route(&mut self, net: &Net, k: Key) -> (Option<Arc<Vec<u32>>>, u64) {
        if let Some((r, _, u)) = self.lookup(net, k) {
            return (Some(r), u);
        }
        let res = {
            let mut sc = match self.slots[BATCH].lock() {
                Ok(g) => g,
                Err(_) => return (None, 1),
            };
            self.search(&mut sc, k.o, k.d, k.variant)
        };
        self.insert(k, res)
    }

    pub fn slot_busy(&self, i: usize) -> bool {
        self.slots[i].lock().is_ok_and(|s| s.run.is_some() || s.done.is_some())
    }

    pub fn start(&self, i: usize, sources: &[(u32, u64)], d: u32, variant: u8) {
        if let Ok(mut sc) = self.slots[i].lock() {
            self.init(&mut sc, sources, d, variant);
        }
    }

    /// Advances every running slot by `slice` settled nodes on the pool (deterministic: the
    /// slice is in units, the thread count only changes speed). Returns units spent.
    pub fn step_slots(&self, pool: &Pool, slice: u64) -> u64 {
        let threads = pool.threads();
        let spent: Vec<Mutex<u64>> = (0..BATCH).map(|_| Mutex::new(0)).collect();
        let work = |t: usize| {
            let mut i = t;
            while i < BATCH {
                if let Ok(mut sc) = self.slots[i].lock()
                    && sc.run.is_some()
                {
                    let before = sc.settled;
                    self.advance(&mut sc, slice);
                    if let Ok(mut s) = spent[i].lock() {
                        *s = sc.settled - before;
                    }
                }
                i += threads;
            }
        };
        pool.run(&work);
        spent.iter().map(|m| m.lock().map_or(0, |v| *v)).sum()
    }

    pub fn cancel(&self, i: usize) {
        if let Ok(mut sc) = self.slots[i].lock() {
            sc.run = None;
            sc.done = None;
        }
    }

    /// Takes a finished slot's result.
    pub fn take_done(&self, i: usize) -> Option<SearchResult> {
        self.slots[i].lock().ok().and_then(|mut s| s.done.take())
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
