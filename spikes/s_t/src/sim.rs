//! Agents: 1D lane/connector queues, IDM, zone locks + target-lane reservations, junction
//! control, lane plans + MOBIL-lite, spawning, routing budget, stuck/cycle handling.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use crate::net::{Approach, Net, NodeCtrl, Turn};
use crate::pool::Pool;
use std::sync::Mutex;
use crate::route::{BATCH, Key, Router};

pub const NONE: u32 = u32::MAX;
pub const DT: f32 = 0.1;
pub const PARTS: usize = 64;
const A_MAX: f32 = 1.6;
const B_COMF: f32 = 2.5;
const T_HW: f32 = 1.2;
const S0: f32 = 2.0;
const CAR_LEN: f32 = 4.5;
const GAP: f32 = 2.0;
/// Ring entry metering threshold (occupied fraction of the ring path).
const RING_METER: f32 = 0.7;
/// Cache-hit units per tick (hits cost 1 unit + path length / 8 under Revalidate).
pub const ROUTE_BUDGET: u64 = 4_000;
/// Settled nodes per running search slot per tick (BATCH slots advance in parallel).
pub const ROUTE_SLICE: u64 = 1_200;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum On {
    Lane(u32),
    Conn(u32),
    Off,
}

#[derive(Clone)]
pub struct Veh {
    pub on: On,
    pub s: f32,
    pub v: f32,
    pub len: f32,
    pub route: Arc<Vec<u32>>,
    pub plan: Arc<Vec<u16>>,
    pub ri: u32,
    pub want: u32,
    pub granted: bool,
    pub zconn: u32,
    pub zrel: u16,
    pub zoff: f32,
    pub resv_lane: u32,
    pub resv: f32,
    pub req_tick: u64,
    pub stuck_s: f32,
    pub mand_wait: f32,
    /// Tick from which discretionary lane changes are allowed again.
    pub lc_until: u64,
    pub start_tick: u64,
    pub dest_s: f32,
    pub key: Key,
    pub reroute: bool,
    /// Want/need must be recomputed (lane entry, lane change, spawn, reroute, topology).
    pub fresh: bool,
    /// Current lane is not admissible for the plan (mandatory change pending).
    pub need: bool,
    /// Time below 0.5 m/s regardless of legitimacy (wait-for graph input).
    pub wait_s: f32,
    /// Index of the last route link and the destination link (avoid Arc derefs in hot loops).
    pub last_ri: u32,
    pub dest_link: u32,
    /// A reroute request is queued or running.
    pub rr_pending: bool,
}

/// Who waits for a running search slot.
#[derive(Clone, Copy)]
enum Waiter {
    Trip(Key),
    /// (vehicle, its start tick as generation, lane at request time)
    Reroute(u32, u64, u32),
}

struct SlotReq {
    key: Option<Key>,
    waiters: Vec<Waiter>,
    start_tick: u64,
}

/// Per-partition output of the parallel phase (merged in partition order).
#[derive(Default)]
pub struct PartOut {
    kin: Vec<(u32, f32, f32, u8)>,
    lanes: Vec<u32>,
    conns: Vec<u32>,
    rel: Vec<u32>,
    arrive: Vec<u32>,
    work: Vec<u32>,
    kill: Vec<u32>,
    speed: Vec<(u32, f32, u32)>,
    /// Lanes / connectors whose head reached the end this tick.
    cross_l: Vec<u32>,
    cross_c: Vec<u32>,
    /// Junction requests (node, starve, prio, req_tick, vid, allow at collection time).
    req: Vec<Req>,
}

/// A junction request; `feasible` = zones free and target space available at collection time
/// (both change only through grants at the same node).
#[derive(Clone, Copy)]
struct Req {
    key: u128,
    node: u32,
    vid: u32,
    allow: Allow,
    feasible: bool,
}

const F_SLOW_ILLEGIT: u8 = 1;
const F_SLOW: u8 = 2;

/// Raw pointer for disjoint per-partition writes (each vehicle / lane belongs to one partition).
struct SyncPtr<T>(*mut T);
impl<T> Clone for SyncPtr<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for SyncPtr<T> {}
unsafe impl<T> Sync for SyncPtr<T> {}
unsafe impl<T> Send for SyncPtr<T> {}

pub struct Trip {
    pub key: Key,
    pub route: Arc<Vec<u32>>,
    pub plan: Arc<Vec<u16>>,
    pub queued_tick: u64,
}

#[derive(Clone, Copy, Default)]
pub struct Signal {
    pub phase: usize,
    pub stage: u8,
    pub t: f32,
}

#[derive(Default, Clone)]
pub struct Stats {
    pub trips_requested: u64,
    pub trips_no_route: u64,
    pub spawned: u64,
    pub arrived: u64,
    pub macro_spawn_timeout: u64,
    pub despawn_stuck: u64,
    pub despawn_cycle: u64,
    pub despawn_bulldoze: u64,
    pub reroutes: u64,
    pub overlaps: u64,
    pub zone_conflicts: u64,
    pub lane_changes: u64,
    pub grants: u64,
    pub t_par: Vec<f32>,
    pub t_seq: Vec<f32>,
    pub t_junc: Vec<f32>,
    pub t_route: Vec<f32>,
    pub units: Vec<u64>,
    /// Breakdown (ms per tick): parallel apply, merge, transfers, releases, lanes, spawn,
    /// stuck, junction request collection, junction grants, signals.
    pub t_parts: [Vec<f32>; 10],
    /// (node, from lane, tick, vehicle) of each vehicle passing a stop line.
    pub passes: Vec<(u32, u32, u64, u32)>,
    /// Junction requests per tick, sequential allow re-evaluations, sort time (ms).
    pub req_n: Vec<u32>,
    pub reeval: u64,
    pub t_sort: Vec<f32>,
    /// Routing batches per tick and queue length after routing.
    pub batches: Vec<u32>,
    pub route_q_len: Vec<u32>,
    /// Ticks from request to result (trip searches and reroutes).
    pub route_lat: Vec<u32>,
    /// Planned demand per (game hour, link): trips whose route uses the link.
    pub demand: BTreeMap<(u32, u32), u32>,
    /// Links stuck vehicles wanted to enter (their connector's target link).
    pub stuck_targets: Vec<u32>,
    pub node_demand: BTreeMap<(u32, u32), u32>,
    /// (node of the stuck vehicle's connector, downstream node of its target link)
    pub stuck_nodes: Vec<(u32, u32)>,
    pub reroute_lat: Vec<u32>,
}

pub struct Sim {
    pub net: Net,
    pub router: Router,
    pub veh: Vec<Veh>,
    free: Vec<u32>,
    pub lane_q: Vec<Vec<u32>>,
    pub conn_q: Vec<Vec<u32>>,
    pub resv: Vec<f32>,
    /// (holder connector, count)
    pub zone_hold: Vec<(u32, u16)>,
    pub signals: Vec<Signal>,
    pub tick: u64,
    pub threads: usize,
    pub route_q: VecDeque<(Key, u64)>,
    /// Reroute requests (vehicle, generation); served before trip requests.
    reroute_q: VecDeque<(u32, u64)>,
    slot_req: Vec<Option<SlotReq>>,
    pub spawn_q: BTreeMap<u32, VecDeque<Trip>>,
    pub stats: Stats,
    pub check: bool,
    /// Link speed accumulators for epoch costs.
    v_sum: Vec<f32>,
    v_cnt: Vec<u32>,
    pub record_passes: bool,
    pool: Pool,
    outs: Vec<Mutex<PartOut>>,
    /// Ring links per ring node (whole cycle), for metering.
    ring_of: Vec<Vec<u32>>,
    ring_conns: Vec<Vec<u32>>,
    ring_cap: Vec<f32>,
    /// Tick of the last grant per node (a junction that serves traffic is not a jam).
    pub last_grant: Vec<u64>,
    /// Vehicles asking to merge into each lane (mandatory change; cooperative followers yield).
    merge_req: Vec<Vec<u32>>,
    /// Occupied lanes / connectors (sorted), rebuilt at the start of each tick.
    act_lanes: Vec<u32>,
    act_conns: Vec<u32>,
    /// Per incoming link: other alive incoming links at its node, the priority ones, right-of.
    others: Vec<Vec<u32>>,
    majors: Vec<Vec<u32>>,
    rights: Vec<Vec<u32>>,
    sig_demand: Vec<Vec<bool>>,
    /// Per-tick work lists filled by the apply pass.
    arrive_l: Vec<u32>,
    rel_l: Vec<u32>,
    slow_l: Vec<u32>,
    work_l: Vec<u32>,
    kill_l: Vec<u32>,
    cross_l: Vec<u32>,
    cross_c: Vec<u32>,
    req_l: Vec<Req>,
    sig_nodes: Vec<u32>,
    /// Ring occupancy per ring node (same value for every node of a ring), per tick.
    ring_occ: Vec<f32>,
    /// Routing epoch length in ticks (ADR-0048 had 1 game hour = 1 200; the spike uses 15 game
    /// minutes = 300: congestion builds faster than an hour at the peak).
    pub epoch_ticks: u64,
    /// Diagnostic: re-evaluate every vehicle every tick.
    pub full_lanes: bool,
    t_jc: f32,
}

fn idm(v: f32, v0: f32, gap: f32, dv: f32) -> f32 {
    let ss = S0 + v * T_HW + v * dv / (2.0 * (A_MAX * B_COMF).sqrt());
    let r = v / v0.max(0.1);
    A_MAX * (1.0 - r * r * r * r - (ss.max(0.0) / gap.max(0.05)).powi(2))
}

impl Sim {
    pub fn new(net: Net, router: Router, threads: usize) -> Self {
        let nl = net.lanes.len();
        let nc = net.conns.len();
        let nz = net.zones.len();
        let nn = net.nodes.len();
        Sim {
            net,
            router,
            veh: Vec::new(),
            free: Vec::new(),
            lane_q: vec![Vec::new(); nl],
            conn_q: vec![Vec::new(); nc],
            resv: vec![0.0; nl],
            zone_hold: vec![(NONE, 0); nz],
            signals: vec![Signal::default(); nn],
            tick: 0,
            threads,
            route_q: VecDeque::new(),
            reroute_q: VecDeque::new(),
            slot_req: (0..BATCH).map(|_| None).collect(),
            spawn_q: BTreeMap::new(),
            stats: Stats::default(),
            check: true,
            v_sum: vec![0.0; nl],
            v_cnt: vec![0; nl],
            record_passes: false,
            pool: Pool::new(threads.max(1)),
            outs: (0..PARTS).map(|_| Mutex::new(PartOut::default())).collect(),
            ring_of: Vec::new(),
            ring_conns: Vec::new(),
            ring_cap: Vec::new(),
            last_grant: vec![0; nn],
            merge_req: vec![Vec::new(); nl],
            act_lanes: Vec::new(),
            act_conns: Vec::new(),
            others: Vec::new(),
            majors: Vec::new(),
            rights: Vec::new(),
            sig_demand: vec![Vec::new(); nn],
            arrive_l: Vec::new(),
            rel_l: Vec::new(),
            slow_l: Vec::new(),
            work_l: Vec::new(),
            kill_l: Vec::new(),
            cross_l: Vec::new(),
            cross_c: Vec::new(),
            req_l: Vec::new(),
            sig_nodes: Vec::new(),
            ring_occ: Vec::new(),
            epoch_ticks: std::env::var("ST_EPOCH").ok().and_then(|v| v.parse().ok()).unwrap_or(300),
            full_lanes: std::env::var("ST_FULL").is_ok(),
            t_jc: 0.0,
        }
        .with_rings()
    }

    /// Per-link control lists (rebuilt on topology change).
    pub fn control_lists(&mut self) {
        let nl = self.net.links.len();
        self.others = vec![Vec::new(); nl];
        self.majors = vec![Vec::new(); nl];
        self.rights = vec![Vec::new(); nl];
        for l in 0..nl {
            let k = &self.net.links[l];
            if !k.alive {
                continue;
            }
            let nd = &self.net.nodes[k.to as usize];
            let o: Vec<u32> = nd.inc.iter().copied().filter(|x| *x as usize != l && self.net.links[*x as usize].alive).collect();
            self.majors[l] = o.iter().copied().filter(|x| self.net.links[*x as usize].approach == Approach::Priority).collect();
            self.others[l] = o;
            self.rights[l] = self.net.right_of(l as u32);
        }
    }

    fn with_rings(mut self) -> Self {
        self.control_lists();
        let n = self.net.nodes.len();
        self.ring_of = vec![Vec::new(); n];
        self.ring_conns = vec![Vec::new(); n];
        self.ring_cap = vec![0.0; n];
        for node in 0..n {
            let Some(&start) = self.net.nodes[node].inc.iter().find(|l| self.net.links[**l as usize].ring) else { continue };
            let mut cyc = vec![start];
            let mut cur = start;
            for _ in 0..16 {
                let to = self.net.links[cur as usize].to as usize;
                let Some(&nx) = self.net.nodes[to].out.iter().find(|l| self.net.links[**l as usize].ring) else { break };
                if nx == start {
                    break;
                }
                cyc.push(nx);
                cur = nx;
            }
            // Connectors into ring links (ring→ring and entries); path length = ring lanes +
            // ring→ring connectors.
            let mut conns = Vec::new();
            let mut cap = 0.0;
            for &l in &cyc {
                let lk = &self.net.links[l as usize];
                for i in 0..lk.lanes {
                    cap += self.net.lanes[(lk.lane0 + u32::from(i)) as usize].len;
                }
            }
            for (c, cn) in self.net.conns.iter().enumerate() {
                if cyc.contains(&cn.to_link) {
                    conns.push(c as u32);
                    if self.net.links[cn.from_link as usize].ring {
                        cap += cn.len;
                    }
                }
            }
            self.ring_conns[node] = conns;
            self.ring_cap[node] = cap;
            self.ring_of[node] = cyc;
        }
        self
    }

    pub fn active(&self) -> usize {
        self.veh.iter().filter(|v| v.on != On::Off).count()
    }

    pub fn request(&mut self, k: Key) {
        if k.o == k.d {
            return;
        }
        self.stats.trips_requested += 1;
        self.route_q.push_back((k, self.tick));
    }

    // ---------- plans ----------

    fn mask_all(&self, l: u32) -> u16 {
        (1u16 << self.net.links[l as usize].lanes) - 1
    }

    /// Lane plan (A30): admissible lanes per route link with ≥ 40 m runway per change.
    pub fn plan(&self, route: &[u32]) -> Vec<u16> {
        let m = route.len();
        let mut mask = vec![0u16; m];
        mask[m - 1] = self.mask_all(route[m - 1]);
        for k in (0..m - 1).rev() {
            let (a, b) = (route[k], route[k + 1]);
            let lb = &self.net.links[b as usize];
            let allowed = ((lb.len - 15.0) / 40.0).floor().max(0.0) as i32;
            let la = &self.net.links[a as usize];
            let mut good = 0u16;
            let mut any = 0u16;
            for i in 0..la.lanes {
                let lane = la.lane0 + u32::from(i);
                for &c in &self.net.lanes[lane as usize].conns {
                    let cc = &self.net.conns[c as usize];
                    if cc.to_link != b {
                        continue;
                    }
                    any |= 1 << i;
                    let j = (cc.to_lane - lb.lane0) as i32;
                    let dist = (0..lb.lanes as i32).filter(|q| mask[k + 1] & (1 << q) != 0).map(|q| (q - j).abs()).min().unwrap_or(99);
                    if dist <= allowed {
                        good |= 1 << i;
                    }
                }
            }
            mask[k] = if good != 0 { good } else if any != 0 { any } else { self.mask_all(a) };
        }
        mask
    }

    // ---------- step ----------

    pub fn step(&mut self) {
        self.tick += 1;
        let t0 = Instant::now();
        self.parallel_idm();
        let t1 = Instant::now();
        self.parallel_apply();
        let tb = Instant::now();
        // Merge per-partition lists in partition order (deterministic for any thread count).
        self.act_lanes.clear();
        self.act_conns.clear();
        self.arrive_l.clear();
        self.rel_l.clear();
        self.work_l.clear();
        self.kill_l.clear();
        self.cross_l.clear();
        self.cross_c.clear();
        for p in 0..PARTS {
            let o = self.outs[p].get_mut().expect("poisoned");
            self.act_lanes.extend_from_slice(&o.lanes);
            self.act_conns.extend_from_slice(&o.conns);
            self.arrive_l.extend_from_slice(&o.arrive);
            self.rel_l.extend_from_slice(&o.rel);
            self.work_l.extend_from_slice(&o.work);
            self.kill_l.extend_from_slice(&o.kill);
            self.cross_l.extend_from_slice(&o.cross_l);
            self.cross_c.extend_from_slice(&o.cross_c);
        }
        let ta = Instant::now();
        self.transfers();
        let tt = Instant::now();
        self.releases();
        let tr = Instant::now();
        self.lanes_and_wants();
        let t2 = Instant::now();
        self.junctions();
        let tj = Instant::now();
        self.update_signals();
        let t3 = Instant::now();
        self.spawn();
        let tsp = Instant::now();
        self.stuck();
        let t4 = Instant::now();
        let msd = |a: Instant, b: Instant| (b - a).as_secs_f32() * 1e3;
        let parts = [msd(t1, tb), msd(tb, ta), msd(ta, tt), msd(tt, tr), msd(tr, t2), msd(t3, tsp), msd(tsp, t4), self.t_jc, msd(t2, tj) - self.t_jc, msd(tj, t3)];
        for (k, v) in parts.iter().enumerate() {
            self.stats.t_parts[k].push(*v);
        }
        self.routing();
        let t5 = Instant::now();
        if self.check {
            self.checks();
        }
        self.speeds();
        let ms = |a: Instant, b: Instant| (b - a).as_secs_f32() * 1e3;
        // Parallel phases: IDM + apply. Sequential: merge, transfers … lanes, spawn, stuck.
        self.stats.t_par.push(ms(t0, tb));
        self.stats.t_seq.push(ms(tb, t2) + ms(t3, t4));
        self.stats.t_junc.push(ms(t2, t3));
        self.stats.t_route.push(ms(t4, t5));
    }

    /// Parallel pre-filter for discretionary lane changes (re-validated sequentially).
    fn disc_candidate(&self, vid: u32, l: u32) -> bool {
        let x = &self.veh[vid as usize];
        let lane = &self.net.lanes[l as usize];
        let lk = &self.net.links[lane.link as usize];
        if self.tick < x.lc_until || x.s >= lane.len - 40.0 || x.v >= 0.9 * lk.v_free {
            return false;
        }
        let mask = x.plan[(x.ri as usize).min(x.plan.len() - 1)];
        let idx = lane.idx;
        [idx.wrapping_sub(1), idx + 1]
            .into_iter()
            .any(|ni| ni < lk.lanes && mask & (1 << ni) != 0 && self.change_check(vid as usize, l, lk.lane0 + u32::from(ni), true).is_some())
    }

    /// Start-of-tick legitimacy of a slow vehicle's wait (read-only; parallel-safe).
    fn legit_wait(&self, l: u32) -> bool {
        match self.lane_q[l as usize].first() {
            Some(&f) => {
                let h = &self.veh[f as usize];
                h.v >= 0.5
                    || (h.want != NONE && !h.granted && {
                        let cn = &self.net.conns[h.want as usize];
                        (self.net.nodes[cn.node as usize].ctrl == NodeCtrl::Signal && !self.signal_green(cn.node, self.net.lanes[l as usize].link, cn.turn).0)
                            || self.tick.saturating_sub(self.last_grant[cn.node as usize]) <= 100
                    })
            }
            None => false,
        }
    }

    /// Parallel phase 1 (read-only on sim state): IDM for every occupied lane/connector of the
    /// partition (lanes and connectors are assigned to partitions by `id % PARTS`), plus the
    /// per-vehicle flags the sequential pass needs.
    fn parallel_idm(&self) {
        let threads = self.pool.threads();
        let (nl, nc) = (self.lane_q.len(), self.conn_q.len());
        let work = |t: usize| {
            let mut p = t;
            while p < PARTS {
                let Ok(mut out) = self.outs[p].lock() else { return };
                let o = &mut *out;
                o.kin.clear();
                o.lanes.clear();
                o.conns.clear();
                o.rel.clear();
                o.arrive.clear();
                o.work.clear();
                o.kill.clear();
                o.speed.clear();
                o.cross_l.clear();
                o.cross_c.clear();
                let mut l = p;
                while l < nl {
                    if !self.lane_q[l].is_empty() {
                        o.lanes.push(l as u32);
                        self.idm_lane(l as u32, o);
                    }
                    l += PARTS;
                }
                let mut c = p;
                while c < nc {
                    if !self.conn_q[c].is_empty() {
                        o.conns.push(c as u32);
                        self.idm_conn(c as u32, o);
                    }
                    c += PARTS;
                }
                p += threads;
            }
        };
        self.pool.run(&work);
    }

    /// Parallel phase 2: each partition writes its own vehicles' kinematics and timers.
    fn parallel_apply(&mut self) {
        let threads = self.pool.threads();
        let vp = SyncPtr(self.veh.as_mut_ptr());
        let sp = SyncPtr(self.v_sum.as_mut_ptr());
        let cp = SyncPtr(self.v_cnt.as_mut_ptr());
        let outs = &self.outs;
        let work = move |t: usize| {
            let (vp, sp, cp) = (vp, sp, cp);
            let mut p = t;
            while p < PARTS {
                let Ok(mut out) = outs[p].lock() else { return };
                let o = &mut *out;
                for &(vid, s, v, f) in &o.kin {
                    // SAFETY (spike): every vehicle is in exactly one queue, every queue in
                    // exactly one partition, so no two threads write the same element.
                    let x = unsafe { &mut *vp.0.add(vid as usize) };
                    x.s = s;
                    x.v = v;
                    if f & F_SLOW != 0 {
                        x.wait_s += DT;
                        if f & F_SLOW_ILLEGIT != 0 {
                            x.stuck_s += DT;
                            if x.stuck_s > 90.0 {
                                o.kill.push(vid);
                            }
                        }
                    } else {
                        x.stuck_s = 0.0;
                        x.wait_s = 0.0;
                    }
                }
                for &(l, sum, n) in &o.speed {
                    // SAFETY (spike): lanes are partition-owned.
                    unsafe {
                        *sp.0.add(l as usize) += sum;
                        *cp.0.add(l as usize) += n;
                    }
                }
                p += threads;
            }
        };
        self.pool.run(&work);
    }

    fn integrate(v: f32, s: f32, a: f32, limit: f32) -> (f32, f32) {
        let mut nv = (v + a * DT).max(0.0);
        let mut ns = s + nv * DT;
        if ns > limit {
            ns = limit.max(s);
            nv = ((ns - s) / DT).max(0.0).min(nv);
        }
        (ns, nv)
    }

    fn idm_lane(&self, l: u32, out: &mut PartOut) {
        let lane = &self.net.lanes[l as usize];
        let link = &self.net.links[lane.link as usize];
        let q = &self.lane_q[l as usize];
        let mut legit: Option<bool> = None;
        let mut vsum = 0.0;
        for (k, &vid) in q.iter().enumerate() {
            let x = &self.veh[vid as usize];
            let d_end = lane.len - x.s;
            let mut v0 = link.v_free;
            if x.want != NONE {
                let cv = self.net.conns[x.want as usize].v_max;
                v0 = v0.min((cv * cv + 2.0 * 1.5 * d_end.max(0.0)).sqrt());
            }
            let at_dest = x.ri == x.last_ri && x.dest_link == lane.link;
            let (gap, dv, limit) = if k > 0 {
                let ld = &self.veh[q[k - 1] as usize];
                (ld.s - ld.len - x.s, x.v - ld.v, ld.s - ld.len - 0.5)
            } else if at_dest {
                (1e6, 0.0, f32::INFINITY)
            } else if x.granted && x.want != NONE {
                let c = &self.net.conns[x.want as usize];
                let cq = &self.conn_q[x.want as usize];
                if let Some(&lv) = cq.last() {
                    let ld = &self.veh[lv as usize];
                    let g = d_end + ld.s - ld.len;
                    (g, x.v - ld.v, lane.len + ld.s - ld.len - 0.5)
                } else if let Some(&lv) = self.lane_q[c.to_lane as usize].last() {
                    let ld = &self.veh[lv as usize];
                    let g = d_end + c.len + ld.s - ld.len;
                    (g, x.v - ld.v, lane.len + c.len + ld.s - ld.len - 0.5)
                } else {
                    (1e6, 0.0, f32::INFINITY)
                }
            } else {
                (d_end - 0.5, x.v, lane.len - 0.5)
            };
            // Cooperative merging: yield to the nearest requester ahead within 40 m.
            let (mut gap, mut dv) = (gap, dv);
            for &r in &self.merge_req[l as usize] {
                let y = &self.veh[r as usize];
                let g = y.s - y.len - x.s;
                if y.s > x.s && g > 0.5 && g < 40.0 && g < gap {
                    gap = g;
                    dv = x.v - y.v;
                }
            }
            let a = idm(x.v, v0, gap, dv);
            let (s, v) = Self::integrate(x.v, x.s, a, limit);
            vsum += v;
            if k == 0 && s >= lane.len {
                out.cross_l.push(l);
            }
            let mut f = 0u8;
            if v < 0.5 {
                f |= F_SLOW;
                if !*legit.get_or_insert_with(|| self.legit_wait(l)) {
                    f |= F_SLOW_ILLEGIT;
                }
            }
            out.kin.push((vid, s, v, f));
            if x.zconn != NONE || x.resv_lane != NONE {
                out.rel.push(vid);
            }
            if at_dest && s >= x.dest_s {
                out.arrive.push(vid);
            }
            if !x.granted && (x.fresh || x.need || x.reroute || self.full_lanes || ((u64::from(vid) + self.tick) % 5 == 0 && self.disc_candidate(vid, l))) {
                out.work.push(vid);
            }
        }
        out.speed.push((l, vsum, q.len() as u32));
    }

    fn idm_conn(&self, c: u32, out: &mut PartOut) {
        let cn = &self.net.conns[c as usize];
        let q = &self.conn_q[c as usize];
        for (k, &vid) in q.iter().enumerate() {
            let x = &self.veh[vid as usize];
            let (gap, dv, limit) = if k > 0 {
                let ld = &self.veh[q[k - 1] as usize];
                (ld.s - ld.len - x.s, x.v - ld.v, ld.s - ld.len - 0.5)
            } else if let Some(&lv) = self.lane_q[cn.to_lane as usize].last() {
                let ld = &self.veh[lv as usize];
                (cn.len - x.s + ld.s - ld.len, x.v - ld.v, cn.len + ld.s - ld.len - 0.5)
            } else {
                (1e6, 0.0, f32::INFINITY)
            };
            let a = idm(x.v, cn.v_max, gap, dv);
            let (s, v) = Self::integrate(x.v, x.s, a, limit);
            if k == 0 && s >= cn.len {
                out.cross_c.push(c);
            }
            // On a connector nothing is a legitimate long wait.
            let f = if v < 0.5 { F_SLOW | F_SLOW_ILLEGIT } else { 0 };
            out.kin.push((vid, s, v, f));
            if x.zconn != NONE || x.resv_lane != NONE {
                out.rel.push(vid);
            }
        }
    }

    fn transfers(&mut self) {
        let lanes = std::mem::take(&mut self.cross_l);
        for &l in &lanes {
            let l = l as usize;
            let len = self.net.lanes[l].len;
            while let Some(&f) = self.lane_q[l].first() {
                let x = &self.veh[f as usize];
                if x.s < len {
                    break;
                }
                if !(x.granted && x.want != NONE) {
                    self.veh[f as usize].s = len - 0.01;
                    break;
                }
                let c = x.want;
                self.lane_q[l].remove(0);
                let x = &mut self.veh[f as usize];
                x.s -= len;
                x.on = On::Conn(c);
                self.conn_q[c as usize].push(f);
                if self.record_passes {
                    self.stats.passes.push((self.net.conns[c as usize].node, self.net.conns[c as usize].from_lane, self.tick, f));
                }
            }
        }
        self.cross_l = lanes;
        let conns = std::mem::take(&mut self.cross_c);
        for &c in &conns {
            let c = c as usize;
            let cn = &self.net.conns[c];
            let (len, to) = (cn.len, cn.to_lane);
            while let Some(&f) = self.conn_q[c].first() {
                if self.veh[f as usize].s < len {
                    break;
                }
                self.conn_q[c].remove(0);
                let x = &mut self.veh[f as usize];
                x.s -= len;
                x.on = On::Lane(to);
                x.zoff = len;
                x.ri += 1;
                x.granted = false;
                x.want = NONE;
                x.req_tick = 0;
                x.fresh = true;
                self.lane_q[to as usize].push(f);
                self.work_l.push(f);
            }
        }
        self.cross_c = conns;
        // Destination.
        let arrive = std::mem::take(&mut self.arrive_l);
        for &vid in &arrive {
            let vid = vid as usize;
            let x = &self.veh[vid];
            if let On::Lane(l) = x.on {
                let link = self.net.lanes[l as usize].link;
                if x.ri as usize + 1 == x.route.len() && x.route.last() == Some(&link) && x.s >= x.dest_s {
                    self.stats.arrived += 1;
                    self.despawn(vid as u32);
                }
            }
        }
        self.arrive_l = arrive;
    }

    fn releases(&mut self) {
        let list = std::mem::take(&mut self.rel_l);
        for &vid in &list {
            let vid = vid as usize;
            let x = &self.veh[vid];
            if x.on == On::Off {
                continue;
            }
            if x.zconn != NONE {
                let cn = &self.net.conns[x.zconn as usize];
                let pos = match x.on {
                    On::Conn(c) if c == x.zconn => x.s,
                    On::Lane(l) if l == cn.to_lane && !(x.granted && x.want == x.zconn) => x.zoff + x.s,
                    // Granted, still on the approach lane: before the connector start.
                    On::Lane(l) if x.granted && x.want == x.zconn => x.s - self.net.lanes[l as usize].len,
                    _ => f32::INFINITY,
                };
                let rear = pos - x.len;
                let mut zr = x.zrel as usize;
                let zc = x.zconn;
                while zr < cn.zones.len() && (rear >= cn.zones[zr].2 || !pos.is_finite()) {
                    let z = cn.zones[zr].0 as usize;
                    let h = &mut self.zone_hold[z];
                    if h.0 == zc && h.1 > 0 {
                        h.1 -= 1;
                        if h.1 == 0 {
                            h.0 = NONE;
                        }
                    }
                    zr += 1;
                }
                let x = &mut self.veh[vid];
                x.zrel = zr as u16;
                if zr >= self.net.conns[zc as usize].zones.len() && !matches!(x.on, On::Conn(c) if c == zc) {
                    x.zconn = NONE;
                }
            }
            let x = &self.veh[vid];
            if x.resv_lane != NONE {
                let done = match x.on {
                    On::Lane(l) => l != x.resv_lane || x.s >= x.len + GAP,
                    On::Conn(_) => false,
                    On::Off => true,
                };
                if done {
                    let (rl, amt) = (x.resv_lane, x.resv);
                    self.resv[rl as usize] = (self.resv[rl as usize] - amt).max(0.0);
                    let x = &mut self.veh[vid];
                    x.resv_lane = NONE;
                    x.resv = 0.0;
                }
            }
        }
        self.rel_l = list;
    }

    fn despawn(&mut self, vid: u32) {
        let x = self.veh[vid as usize].clone();
        match x.on {
            On::Lane(l) => self.lane_q[l as usize].retain(|v| *v != vid),
            On::Conn(c) => self.conn_q[c as usize].retain(|v| *v != vid),
            On::Off => return,
        }
        // Release held zones (from the next unreleased one) and reservations.
        let mut zc = x.zconn;
        let mut from = x.zrel as usize;
        if x.granted && x.want != NONE && zc == NONE {
            zc = x.want;
            from = 0;
        }
        if x.granted && x.want != NONE && x.want != zc {
            // Granted for the next connector while zones of the previous one are pending.
            for &(z, _, _) in &self.net.conns[x.want as usize].zones {
                let h = &mut self.zone_hold[z as usize];
                if h.0 == x.want && h.1 > 0 {
                    h.1 -= 1;
                    if h.1 == 0 {
                        h.0 = NONE;
                    }
                }
            }
        }
        if zc != NONE {
            for &(z, _, _) in self.net.conns[zc as usize].zones.iter().skip(from) {
                let h = &mut self.zone_hold[z as usize];
                if h.0 == zc && h.1 > 0 {
                    h.1 -= 1;
                    if h.1 == 0 {
                        h.0 = NONE;
                    }
                }
            }
        }
        if x.resv_lane != NONE {
            self.resv[x.resv_lane as usize] = (self.resv[x.resv_lane as usize] - x.resv).max(0.0);
        }
        // A granted vehicle that never entered its connector also holds a reservation on the
        // target lane (stored in resv_lane) — handled above.
        let y = &mut self.veh[vid as usize];
        y.on = On::Off;
        y.granted = false;
        y.want = NONE;
        y.zconn = NONE;
        y.resv_lane = NONE;
        self.free.push(vid);
    }

    /// Picks the connector toward the next route link from the current lane.
    fn pick_want(&self, vid: usize) -> u32 {
        let x = &self.veh[vid];
        let On::Lane(l) = x.on else { return NONE };
        let ri = x.ri as usize;
        if ri + 1 >= x.route.len() {
            return NONE;
        }
        let next = x.route[ri + 1];
        if !self.net.links[next as usize].alive {
            return NONE;
        }
        let mask = x.plan[ri + 1];
        let lb0 = self.net.links[next as usize].lane0;
        let mut best = NONE;
        for &c in &self.net.lanes[l as usize].conns {
            let cn = &self.net.conns[c as usize];
            if cn.to_link != next {
                continue;
            }
            if mask & (1 << (cn.to_lane - lb0)) != 0 {
                return c;
            }
            if best == NONE {
                best = c;
            }
        }
        best
    }

    fn lanes_and_wants(&mut self) {
        for m in &mut self.merge_req {
            m.clear();
        }
        let work = std::mem::take(&mut self.work_l);
        for &vid in &work {
            let vid = vid as usize;
            let x = &self.veh[vid];
            let On::Lane(l) = x.on else { continue };
            if x.granted {
                continue;
            }
            let disc_turn = (vid as u64 + self.tick) % 5 == 0;
            if !x.fresh && !x.need && !x.reroute && !disc_turn && !self.full_lanes {
                continue;
            }
            let link = self.net.lanes[l as usize].link;
            let (lk_lanes, lk_lane0, lk_vfree) = {
                let k = &self.net.links[link as usize];
                (k.lanes, k.lane0, k.v_free)
            };
            let ri = x.ri as usize;
            if x.reroute || (x.fresh && ri + 1 < x.route.len() && !self.net.links[x.route[ri + 1] as usize].alive) {
                self.reroute(vid);
                continue;
            }
            let idx = self.net.lanes[l as usize].idx;
            let mask = x.plan[ri.min(x.plan.len() - 1)];
            let lane_len = self.net.lanes[l as usize].len;
            let need = mask & (1 << idx) == 0;
            let (s, v) = (x.s, x.v);
            let mut changed = false;
            if need && lane_len - s > 3.0 {
                let target = (0..lk_lanes).filter(|q| mask & (1 << q) != 0).min_by_key(|q| (i32::from(*q) - i32::from(idx)).abs());
                if let Some(t) = target {
                    let ni = if t > idx { idx + 1 } else { idx - 1 };
                    let tl = lk_lane0 + u32::from(ni);
                    changed = self.try_change(vid, l, tl, false);
                    if !changed && lane_len - s < 300.0 {
                        self.merge_req[tl as usize].push(vid as u32);
                    }
                }
            } else if !need && disc_turn && s < lane_len - 40.0 && self.tick >= self.veh[vid].lc_until && v < 0.9 * lk_vfree {
                for ni in [idx.wrapping_sub(1), idx + 1] {
                    if ni < lk_lanes && mask & (1 << ni) != 0 && self.try_change(vid, l, lk_lane0 + u32::from(ni), true) {
                        changed = true;
                        break;
                    }
                }
            }
            if changed {
                self.stats.lane_changes += 1;
                let y = &mut self.veh[vid];
                y.lc_until = self.tick + 40;
                y.fresh = true;
            }
            if need && !changed {
                let x = &self.veh[vid];
                if let On::Lane(l2) = x.on
                    && self.net.lanes[l2 as usize].len - x.s < 15.0
                    && x.v < 0.5
                {
                    let y = &mut self.veh[vid];
                    y.mand_wait += DT;
                    if y.mand_wait > 20.0 {
                        y.reroute = true;
                    }
                }
            }
            if self.veh[vid].fresh || need != self.veh[vid].need || self.full_lanes {
                // Recompute need/want for the (possibly new) lane.
                let x = &self.veh[vid];
                let On::Lane(l2) = x.on else { continue };
                let idx2 = self.net.lanes[l2 as usize].idx;
                let need2 = mask & (1 << idx2) == 0;
                let w = if need2 && !x.reroute { NONE } else { self.pick_want(vid) };
                let y = &mut self.veh[vid];
                y.need = need2;
                y.want = w;
                y.fresh = false;
            }
        }
        self.work_l = work;
    }

    /// 1D lane change with gap checks (never ahead of a granted vehicle).
    fn try_change(&mut self, vid: usize, from: u32, to: u32, discretionary: bool) -> bool {
        let Some(p) = self.change_check(vid, from, to, discretionary) else { return false };
        self.lane_q[from as usize].retain(|o| *o as usize != vid);
        self.lane_q[to as usize].insert(p, vid as u32);
        let x = &mut self.veh[vid];
        x.on = On::Lane(to);
        x.req_tick = 0;
        true
    }

    /// Read-only lane-change feasibility; returns the insertion index in the target queue.
    fn change_check(&self, vid: usize, from: u32, to: u32, discretionary: bool) -> Option<usize> {
        let x = &self.veh[vid];
        let (s, v, len) = (x.s, x.v, x.len);
        let q = &self.lane_q[to as usize];
        let p = q.iter().take_while(|o| self.veh[**o as usize].s > s).count();
        let lead = (p > 0).then(|| &self.veh[q[p - 1] as usize]);
        let fol = q.get(p).map(|o| &self.veh[*o as usize]);
        let (g_min, b_safe) = if discretionary { (S0, -4.0) } else { (1.0, -6.0) };
        if let Some(f) = fol {
            if f.granted {
                return None;
            }
            let g = s - len - f.s;
            if g < g_min + 0.5 * f.v {
                return None;
            }
            if idm(f.v, 13.9, g, f.v - v) < b_safe {
                return None;
            }
        }
        if let Some(ld) = lead {
            let g = ld.s - ld.len - s;
            if g < g_min + 0.3 * v {
                return None;
            }
            if discretionary {
                let lane = &self.net.lanes[from as usize];
                let v0 = self.net.links[lane.link as usize].v_free;
                let cur_q = &self.lane_q[from as usize];
                let my = cur_q.iter().position(|o| *o as usize == vid).unwrap_or(0);
                let a_cur = if my > 0 {
                    let c = &self.veh[cur_q[my - 1] as usize];
                    idm(v, v0, c.s - c.len - s, v - c.v)
                } else {
                    idm(v, v0, 1e6, 0.0)
                };
                let a_new = idm(v, v0, g, v - ld.v);
                if a_new - a_cur < 0.3 {
                    return None;
                }
            }
        } else if discretionary {
            let cur_q = &self.lane_q[from as usize];
            let my = cur_q.iter().position(|o| *o as usize == vid).unwrap_or(0);
            if my == 0 {
                return None;
            }
        }
        Some(p)
    }

    /// Queues an asynchronous reroute (served by the routing slots).
    fn reroute(&mut self, vid: usize) {
        let x = &mut self.veh[vid];
        if x.rr_pending {
            return;
        }
        x.rr_pending = true;
        x.want = NONE;
        self.reroute_q.push_back((vid as u32, x.start_tick));
    }

    /// Applies a reroute result (route from one of the lane's exits) or despawns.
    fn apply_reroute(&mut self, vid: usize, vgen: u64, lane: u32, res: Option<Vec<u32>>, req_tick: u64) {
        let x = &self.veh[vid];
        if x.start_tick != vgen || x.on == On::Off || !x.rr_pending {
            return;
        }
        if x.on != On::Lane(lane) {
            // Moved meanwhile (cannot happen while waiting at the line); ask again.
            self.veh[vid].rr_pending = false;
            self.veh[vid].reroute = true;
            return;
        }
        self.stats.reroutes += 1;
        self.stats.reroute_lat.push((self.tick - req_tick) as u32);
        let l = lane;
        let cur = self.net.lanes[l as usize].link;
        match res {
            Some(path) => {
                let mut r = vec![cur];
                r.extend(path);
                let mut plan = self.plan(&r);
                // The new route leaves via a connector of the current lane: that lane is
                // admissible by construction (else the vehicle loops reroute → wrong lane).
                plan[0] |= 1 << self.net.lanes[l as usize].idx;
                let y = &mut self.veh[vid];
                y.last_ri = (r.len() - 1) as u32;
                y.route = Arc::new(r);
                y.plan = Arc::new(plan);
                y.ri = 0;
                y.reroute = false;
                y.rr_pending = false;
                y.mand_wait = 0.0;
                y.want = NONE;
                y.fresh = true;
                self.work_l.push(vid as u32);
            }
            None => {
                if std::env::var("ST_DEBUG").is_ok() {
                    println!("reroute-fail v{vid} lane{l} link{cur}");
                }
                self.stats.despawn_stuck += 1;
                self.despawn(vid as u32);
            }
        }
    }

    // ---------- junction control ----------

    fn approaching(&self, link: u32, horizon: f32) -> impl Iterator<Item = &Veh> + '_ {
        let lk = &self.net.links[link as usize];
        (0..lk.lanes).flat_map(move |i| {
            let lane = lk.lane0 + u32::from(i);
            let len = self.net.lanes[lane as usize].len;
            self.lane_q[lane as usize].iter().map(|v| &self.veh[*v as usize]).take_while(move |x| len - x.s <= horizon)
        })
    }

    /// Gap check against moving vehicles on `links` whose movement conflicts with `c`.
    fn gap_ok(&self, c: u32, links: &[u32], crit: f32) -> bool {
        for &p in links {
            let len = self.net.links[p as usize].len;
            let my_target = self.net.conns[c as usize].to_lane;
            for x in self.approaching(p, 100.0) {
                if x.granted {
                    continue;
                }
                let in_range = len - x.s <= x.v * x.v / (2.0 * B_COMF) + 10.0;
                // A priority vehicle at the line waiting for the same target-lane space goes
                // first (else entries steal its space and rings/merges lock up).
                if in_range && x.want != NONE && self.net.conns[x.want as usize].to_lane == my_target {
                    return false;
                }
                if x.v < 0.5 {
                    continue;
                }
                // In request range but not granted: it is blocked (priority requests are
                // processed first), so it will not enter now.
                if in_range {
                    continue;
                }
                let conflict = x.want == NONE || self.net.conflicts[c as usize].binary_search(&x.want).is_ok();
                if !conflict {
                    continue;
                }
                let d = len - x.s;
                if d / x.v.max(0.5) < crit {
                    return false;
                }
            }
        }
        true
    }

    fn signal_green(&self, node: u32, link: u32, turn: Turn) -> (bool, f32) {
        let sg = &self.signals[node as usize];
        let n = &self.net.nodes[node as usize];
        if n.phases.is_empty() {
            return (true, f32::INFINITY);
        }
        let k = sg.phase % n.phases.len();
        let in_phase = n.phases[k].contains(&link) && n.phase_turns[k] & turn.bit() != 0;
        match sg.stage {
            0 => (in_phase, f32::INFINITY),
            1 => (in_phase, (3.0 - sg.t) + 1.5),
            _ => (false, 0.0),
        }
    }

    /// Parallel junction prepass: collect requests (up to 3 per lane, each behind granted
    /// predecessors only) and evaluate control. `allow()` depends only on grants at the same
    /// node, so the sequential pass re-evaluates it only after a grant at that node.
    fn junction_prepass(&self) {
        let threads = self.pool.threads();
        let work = |t: usize| {
            let mut p = t;
            while p < PARTS {
                let Ok(mut out) = self.outs[p].lock() else { return };
                let o = &mut *out;
                o.req.clear();
                for &l in &o.lanes {
                    let l = l as usize;
                    let len = self.net.lanes[l].len;
                    for (k, &vid) in self.lane_q[l].iter().enumerate().take(3) {
                        let x = &self.veh[vid as usize];
                        if x.granted {
                            continue;
                        }
                        if x.want == NONE || (k > 0 && !self.veh[self.lane_q[l][k - 1] as usize].granted) {
                            break;
                        }
                        let d = len - x.s;
                        if d > x.v * x.v / (2.0 * B_COMF) + 10.0 {
                            break;
                        }
                        let c = &self.net.conns[x.want as usize];
                        let starve = u8::from(self.tick.saturating_sub(x.req_tick) > 120 && x.req_tick != 0);
                        let prio: u8 = match self.net.links[c.from_link as usize].approach {
                            Approach::Priority => 1,
                            Approach::Yield => 0,
                        };
                        let rt = if x.req_tick == 0 { self.tick } else { x.req_tick };
                        // Sort key: node asc, starving first, priority first, earliest, vid.
                        let key = u128::from(c.node) << 96 | u128::from(1 - starve) << 95 | u128::from(1 - prio) << 94 | u128::from(rt) << 32 | u128::from(vid);
                        let allow = self.allow(vid);
                        let feasible = allow != Allow::No && self.grant_feasible(vid, &[]);
                        o.req.push(Req { key, node: c.node, vid, allow, feasible });
                        break;
                    }
                }
                p += threads;
            }
        };
        self.pool.run(&work);
    }

    /// Ring occupancy: vehicles on ring lanes, ring→ring connectors and granted / driving
    /// entry connectors, × (length + s0), over the ring path length.
    fn update_ring_occ(&mut self) {
        let n = self.net.nodes.len();
        if self.ring_occ.len() != n {
            self.ring_occ = vec![0.0; n];
        }
        for node in 0..n {
            let ring = &self.ring_of[node];
            if ring.is_empty() {
                continue;
            }
            let mut occ = 0.0f32;
            for &l in ring {
                let lk = &self.net.links[l as usize];
                for i in 0..lk.lanes {
                    let lane = lk.lane0 + u32::from(i);
                    occ += self.lane_q[lane as usize].iter().map(|v| self.veh[*v as usize].len + S0).sum::<f32>();
                }
            }
            for &c in &self.ring_conns[node] {
                occ += self.conn_q[c as usize].iter().map(|v| self.veh[*v as usize].len + S0).sum::<f32>();
            }
            // Granted entries still on their approach lane.
            for &l in ring {
                let nd = self.net.links[l as usize].to as usize;
                for &inc in &self.net.nodes[nd].inc {
                    let lk = &self.net.links[inc as usize];
                    if lk.ring {
                        continue;
                    }
                    for i in 0..lk.lanes {
                        let lane = lk.lane0 + u32::from(i);
                        for &f in self.lane_q[lane as usize].iter().take(3) {
                            if self.veh[f as usize].granted {
                                occ += self.veh[f as usize].len + S0;
                            }
                        }
                    }
                }
            }
            self.ring_occ[node] = if self.ring_cap[node] > 0.0 { occ / self.ring_cap[node] } else { 0.0 };
        }
    }

    fn junctions(&mut self) {
        let tj0 = Instant::now();
        self.update_ring_occ();
        self.junction_prepass();
        let mut req = std::mem::take(&mut self.req_l);
        req.clear();
        for p in 0..PARTS {
            let o = self.outs[p].get_mut().expect("poisoned");
            req.extend_from_slice(&o.req);
        }
        for r in &req {
            if self.veh[r.vid as usize].req_tick == 0 {
                self.veh[r.vid as usize].req_tick = self.tick;
            }
        }
        self.t_jc = (Instant::now() - tj0).as_secs_f32() * 1e3;
        self.stats.req_n.push(req.len() as u32);
        let ts0 = Instant::now();
        req.sort_unstable_by_key(|r| r.key);
        self.stats.t_sort.push((Instant::now() - ts0).as_secs_f32() * 1e3);
        let mut i = 0;
        while i < req.len() {
            let node = req[i].node;
            let mut j = i;
            while j < req.len() && req[j].node == node {
                j += 1;
            }
            let mut granted_any = false;
            let mut rbl_only: Vec<u32> = Vec::new();
            // Zones wanted by earlier arrivals that control allowed but zones/space blocked:
            // later requests may not take them (no overtaking via the same-connector rule).
            let mut claimed: Vec<u32> = Vec::new();
            for r in &req[i..j] {
                let vid = r.vid;
                let allow = if granted_any {
                    self.stats.reeval += 1;
                    self.allow(vid)
                } else {
                    r.allow
                };
                match allow {
                    Allow::Yes => {
                        if (granted_any || r.feasible) && self.try_grant(vid, &claimed) {
                            granted_any = true;
                        } else if self.tick.saturating_sub(self.veh[vid as usize].req_tick) > 60 {
                            let c = self.veh[vid as usize].want;
                            claimed.extend(self.net.conns[c as usize].zones.iter().map(|z| z.0));
                        }
                    }
                    Allow::Rbl => rbl_only.push(vid),
                    Allow::No => {}
                }
            }
            // Equal priority: if every waiting approach yields, grant the earliest arrival, then
            // greedily every later one whose zones are still free (zone checks exclude
            // conflicts with the vehicles just granted).
            if !granted_any && !rbl_only.is_empty() {
                rbl_only.sort_by_key(|v| (self.veh[*v as usize].req_tick, *v));
                for &v in &rbl_only {
                    if self.try_grant(v, &claimed) {
                        granted_any = true;
                    }
                }
            }
            if granted_any {
                self.last_grant[node as usize] = self.tick;
            }
            i = j;
        }
        self.req_l = req;
    }

    fn allow(&self, vid: u32) -> Allow {
        let x = &self.veh[vid as usize];
        let c = x.want;
        let cn = &self.net.conns[c as usize];
        let node = cn.node;
        let link = cn.from_link;
        let nd = &self.net.nodes[node as usize];
        let crit = match cn.turn {
            Turn::Left | Turn::U => 4.0,
            Turn::Straight => 3.5,
            Turn::Right => 3.0,
        };
        match nd.ctrl {
            NodeCtrl::None => Allow::Yes,
            NodeCtrl::Signal => {
                let (green, clear) = self.signal_green(node, link, cn.turn);
                if !green {
                    return Allow::No;
                }
                if clear.is_finite() {
                    let lane = &self.net.lanes[match x.on {
                        crate::sim::On::Lane(l) => l as usize,
                        _ => 0,
                    }];
                    let t = (lane.len - x.s + cn.len + x.len) / x.v.max(3.0);
                    if t > clear {
                        return Allow::No;
                    }
                }
                Allow::Yes
            }
            NodeCtrl::Priority | NodeCtrl::Ring => {
                let mine = self.net.links[link as usize].approach;
                let major = &self.majors[link as usize];
                if nd.ctrl == NodeCtrl::Ring && mine == Approach::Yield {
                    // Metering: ring occupancy (lanes + connectors, incl. granted entries)
                    // ≤ RING_METER of the ring path length.
                    if self.ring_occ[node as usize] > RING_METER {
                        return Allow::No;
                    }
                }
                let ok = match mine {
                    Approach::Yield => self.gap_ok(c, major, crit),
                    Approach::Priority if cn.turn == Turn::Left => self.gap_ok(c, major, crit),
                    Approach::Priority => true,
                };
                if ok { Allow::Yes } else { Allow::No }
            }
            NodeCtrl::Equal => {
                for &r in &self.rights[link as usize] {
                    let rl = self.net.links[r as usize].len;
                    for y in self.approaching(r, 40.0) {
                        if y.granted {
                            continue;
                        }
                        // Yield to vehicles that are coming, or waiting at the line able to go.
                        let able = y.want != NONE && {
                            let t = self.net.conns[y.want as usize].to_lane as usize;
                            let space = self.lane_q[t].last().map_or(self.net.lanes[t].len, |lv| self.veh[*lv as usize].s - self.veh[*lv as usize].len) - self.resv[t];
                            space >= y.len + GAP
                        };
                        let coming = y.v >= 0.5;
                        let at_line = rl - y.s < 3.0;
                        if !(coming || (at_line && able)) {
                            continue;
                        }
                        if y.want == NONE || self.net.conflicts[c as usize].binary_search(&y.want).is_ok() {
                            return Allow::Rbl;
                        }
                    }
                }
                Allow::Yes
            }
        }
    }

    /// Zones free (and not claimed), target space available, nothing pending.
    fn grant_feasible(&self, vid: u32, claimed: &[u32]) -> bool {
        let x = &self.veh[vid as usize];
        // Pending zones / reservation of the previous connector must be released first.
        if x.resv_lane != NONE || x.zconn != NONE {
            return false;
        }
        let c = x.want;
        let cn = &self.net.conns[c as usize];
        for &(z, _, _) in &cn.zones {
            let h = self.zone_hold[z as usize];
            if (h.1 > 0 && h.0 != c) || claimed.contains(&z) {
                return false;
            }
        }
        let t = cn.to_lane as usize;
        let tlen = self.net.lanes[t].len;
        let space = match self.lane_q[t].last() {
            Some(&lv) => {
                let ld = &self.veh[lv as usize];
                ld.s - ld.len
            }
            None => tlen,
        } - self.resv[t];
        space >= x.len + GAP
    }

    fn try_grant(&mut self, vid: u32, claimed: &[u32]) -> bool {
        if !self.grant_feasible(vid, claimed) {
            return false;
        }
        let x = &self.veh[vid as usize];
        let c = x.want;
        let cn = &self.net.conns[c as usize];
        let t = cn.to_lane as usize;
        let need = x.len + GAP;
        let zones: Vec<u32> = cn.zones.iter().map(|z| z.0).collect();
        for z in zones {
            let h = &mut self.zone_hold[z as usize];
            h.0 = c;
            h.1 += 1;
        }
        self.resv[t] += need;
        let x = &mut self.veh[vid as usize];
        x.granted = true;
        x.zconn = c;
        x.zrel = 0;
        x.zoff = 0.0;
        x.resv_lane = t as u32;
        x.resv = need;
        self.stats.grants += 1;
        true
    }

    fn update_signals(&mut self) {
        if self.sig_nodes.is_empty() {
            self.sig_nodes = (0..self.net.nodes.len() as u32).filter(|n| self.net.nodes[*n as usize].ctrl == NodeCtrl::Signal && !self.net.nodes[*n as usize].phases.is_empty()).collect();
        }
        let sig_nodes = std::mem::take(&mut self.sig_nodes);
        for &n in &sig_nodes {
            let n = n as usize;
            let np = self.net.nodes[n].phases.len();
            let nd = &self.net.nodes[n];
            if self.tick % 5 == 0 || self.sig_demand[n].len() != nd.phases.len() {
                let d: Vec<bool> = nd
                .phases
                .iter()
                .zip(&nd.phase_turns)
                .map(|(ph, tb)| {
                    ph.iter().any(|l| {
                        self.approaching(*l, 60.0).any(|x| {
                            !x.granted && x.want != NONE && self.net.conns[x.want as usize].turn.bit() & tb != 0
                        })
                    })
                })
                .collect();
                self.sig_demand[n] = d;
            }
            let demand = self.sig_demand[n].clone();
            let sg = &mut self.signals[n];
            sg.t += DT;
            match sg.stage {
                0 => {
                    let other = (1..np).any(|k| demand[(sg.phase + k) % np]);
                    if sg.t >= 30.0 || (sg.t >= 8.0 && !demand[sg.phase % np] && other) {
                        sg.stage = 1;
                        sg.t = 0.0;
                    }
                }
                1 => {
                    if sg.t >= 3.0 {
                        sg.stage = 2;
                        sg.t = 0.0;
                    }
                }
                _ => {
                    if sg.t >= 1.5 {
                        let next = (1..=np).map(|k| (sg.phase + k) % np).find(|p| demand[*p]).unwrap_or((sg.phase + 1) % np);
                        sg.phase = next;
                        sg.stage = 0;
                        sg.t = 0.0;
                    }
                }
            }
        }
        self.sig_nodes = sig_nodes;
    }

    // ---------- spawning / routing / stuck ----------

    fn alloc(&mut self, x: Veh) -> u32 {
        match self.free.pop() {
            Some(i) => {
                self.veh[i as usize] = x;
                i
            }
            None => {
                self.veh.push(x);
                (self.veh.len() - 1) as u32
            }
        }
    }

    fn spawn(&mut self) {
        let links: Vec<u32> = self.spawn_q.keys().copied().collect();
        for l in links {
            let Some(trip) = self.spawn_q.get(&l).and_then(|q| q.front()) else { continue };
            if self.tick - trip.queued_tick > 300 {
                self.stats.macro_spawn_timeout += 1;
                if let Some(q) = self.spawn_q.get_mut(&l) {
                    q.pop_front();
                }
                continue;
            }
            let lk = {
                let k = &self.net.links[l as usize];
                (k.alive, k.lanes, k.lane0, k.len, k.v_free)
            };
            if !lk.0 {
                if let Some(q) = self.spawn_q.get_mut(&l) {
                    q.clear();
                }
                continue;
            }
            let mask = trip.plan[0];
            let s0 = 0.5 * lk.3;
            let mut placed = None;
            for i in (0..lk.1).rev() {
                let lane = lk.2 + u32::from(i);
                if mask & (1 << i) == 0 {
                    continue;
                }
                let q = &self.lane_q[lane as usize];
                let p = q.iter().take_while(|o| self.veh[**o as usize].s > s0).count();
                let ok_lead = p == 0 || {
                    let ld = &self.veh[q[p - 1] as usize];
                    ld.s - ld.len - s0 >= S0 + 2.0
                };
                let ok_fol = q.get(p).is_none_or(|o| {
                    let f = &self.veh[*o as usize];
                    let g = s0 - CAR_LEN - f.s;
                    g >= S0 && (f.v < 0.1 || g / f.v > 2.0) && !f.granted
                });
                if ok_lead && ok_fol {
                    placed = Some((lane, p));
                    break;
                }
            }
            let Some((lane, p)) = placed else { continue };
            // Enter at the speed the gap ahead allows (arrivals come in moving).
            let v_in = {
                let q = &self.lane_q[lane as usize];
                let g = if p == 0 { 1e6 } else {
                    let ld = &self.veh[q[p - 1] as usize];
                    ld.s - ld.len - s0
                };
                ((g - S0) / T_HW).clamp(0.0, lk.4)
            };
            let Some(trip) = self.spawn_q.get_mut(&l).and_then(|q| q.pop_front()) else { continue };
            let dest = *trip.route.last().unwrap_or(&l);
            let trip_len = trip.route.len();
            let x = Veh {
                on: On::Lane(lane),
                s: s0,
                v: v_in,
                len: CAR_LEN,
                dest_s: 0.5 * self.net.links[dest as usize].len,
                route: trip.route,
                plan: trip.plan,
                ri: 0,
                want: NONE,
                granted: false,
                zconn: NONE,
                zrel: 0,
                zoff: 0.0,
                resv_lane: NONE,
                resv: 0.0,
                req_tick: 0,
                stuck_s: 0.0,
                mand_wait: 0.0,
                lc_until: 0,
                start_tick: self.tick,
                key: trip.key,
                reroute: false,
                fresh: true,
                need: false,
                wait_s: 0.0,
                last_ri: (trip_len - 1) as u32,
                dest_link: dest,
                rr_pending: false,
            };
            let vid = self.alloc(x);
            self.lane_q[lane as usize].insert(p, vid);
            self.stats.spawned += 1;
        }
        self.spawn_q.retain(|_, q| !q.is_empty());
    }

    /// Queues a trip for spawning; returns plan units (route length when the plan is computed).
    fn enqueue_trip(&mut self, k: Key, route: Option<Arc<Vec<u32>>>, plan: Option<Arc<Vec<u16>>>) -> u64 {
        let mut units = 0;
        match route {
            Some(route) => {
                let plan = match plan {
                    Some(p) => p,
                    None => {
                        units = route.len() as u64;
                        let p = Arc::new(self.plan(&route));
                        self.router.set_plan(k, p.clone());
                        p
                    }
                };
                let hour = (self.tick / 1200) as u32;
                for l in route.iter() {
                    *self.stats.demand.entry((hour, *l)).or_default() += 1;
                }
                for w in route.windows(2) {
                    let node = self.net.links[w[0] as usize].to;
                    *self.stats.node_demand.entry((hour, node)).or_default() += 1;
                }
                self.spawn_q.entry(k.o).or_default().push_back(Trip { key: k, route, plan, queued_tick: self.tick });
            }
            None => self.stats.trips_no_route += 1,
        }
        units
    }

    /// Routing (ADR-0048 budget in deterministic units): reroutes first, then trip requests in
    /// queue order. Cache hits resolve immediately (≤ ROUTE_BUDGET units per tick); misses
    /// occupy one of BATCH search slots, which advance ROUTE_SLICE settled nodes per tick in
    /// parallel and deliver results in slot order.
    fn routing(&mut self) {
        let mut units = 0u64;
        // 1. Fill idle slots.
        for i in 0..BATCH {
            if self.slot_req[i].is_some() {
                continue;
            }
            // Reroutes first.
            while let Some((vid, vgen)) = self.reroute_q.pop_front() {
                let x = &self.veh[vid as usize];
                let On::Lane(l) = x.on else { continue };
                if x.start_tick != vgen || !x.rr_pending {
                    continue;
                }
                let dest = x.dest_link;
                let variant = x.key.variant;
                let exits: Vec<u32> = self.net.lanes[l as usize].conns.iter().map(|c| self.net.conns[*c as usize].to_link).filter(|t| self.net.links[*t as usize].alive).collect();
                if exits.contains(&dest) {
                    self.apply_reroute(vid as usize, vgen, l, Some(vec![dest]), self.tick);
                    continue;
                }
                if exits.is_empty() {
                    self.apply_reroute(vid as usize, vgen, l, None, self.tick);
                    continue;
                }
                let sources: Vec<(u32, u64)> = exits.iter().map(|e| (*e, self.router.enter_cost(*e, variant))).collect();
                self.router.start(i, &sources, dest, variant);
                self.slot_req[i] = Some(SlotReq { key: None, waiters: vec![Waiter::Reroute(vid, vgen, l)], start_tick: self.tick });
                break;
            }
            if self.slot_req[i].is_some() {
                continue;
            }
            // Trip requests: hits resolve now; the first miss takes this slot.
            while units < ROUTE_BUDGET {
                let Some((k, _)) = self.route_q.pop_front() else { break };
                if let Some((r, plan, u)) = self.router.lookup(&self.net, k) {
                    units += u;
                    units += self.enqueue_trip(k, Some(r), plan);
                    continue;
                }
                if let Some(sr) = self.slot_req.iter_mut().flatten().find(|sr| sr.key == Some(k)) {
                    sr.waiters.push(Waiter::Trip(k));
                    continue;
                }
                self.router.start(i, &[(k.o, 0)], k.d, k.variant);
                self.slot_req[i] = Some(SlotReq { key: Some(k), waiters: vec![Waiter::Trip(k)], start_tick: self.tick });
                break;
            }
        }
        // 2. Advance running searches.
        let busy = self.slot_req.iter().filter(|s| s.is_some()).count() as u32;
        if busy > 0 {
            units += self.router.step_slots(&self.pool, ROUTE_SLICE);
        }
        // 3. Deliver finished results in slot order.
        for i in 0..BATCH {
            if self.slot_req[i].is_none() {
                continue;
            }
            let Some(res) = self.router.take_done(i) else { continue };
            let Some(sr) = self.slot_req[i].take() else { continue };
            match sr.key {
                Some(k) => {
                    self.stats.route_lat.push((self.tick - sr.start_tick) as u32);
                    let (route, _) = self.router.insert(k, res);
                    for w in sr.waiters {
                        if let Waiter::Trip(k) = w {
                            let plan = self.router.cache.get(&k).and_then(|e| e.plan.clone());
                            units += self.enqueue_trip(k, route.clone(), plan);
                        }
                    }
                }
                None => {
                    self.router.units_total += res.2;
                    for w in sr.waiters {
                        if let Waiter::Reroute(vid, vgen, l) = w {
                            self.apply_reroute(vid as usize, vgen, l, res.0.clone(), sr.start_tick);
                        }
                    }
                }
            }
        }
        self.stats.units.push(units);
        self.stats.batches.push(busy);
        self.stats.route_q_len.push((self.route_q.len() + self.reroute_q.len()) as u32);
    }

    fn stuck(&mut self) {
        let kill = std::mem::take(&mut self.kill_l);
        for &v in &kill {
            if self.veh[v as usize].on == On::Off {
                continue;
            }
            if std::env::var("ST_DEBUG").is_ok() {
                let x = &self.veh[v as usize];
                let (cont, len) = match x.on {
                    On::Lane(l) => (format!("lane{} link{} ring={}", l, self.net.lanes[l as usize].link, self.net.links[self.net.lanes[l as usize].link as usize].ring), self.net.lanes[l as usize].len),
                    On::Conn(c) => (format!("conn{c}"), self.net.conns[c as usize].len),
                    On::Off => ("off".into(), 0.0),
                };
                let mut why = String::new();
                if x.want != NONE {
                    let cn = &self.net.conns[x.want as usize];
                    for &(z, _, _) in &cn.zones {
                        let h = self.zone_hold[z as usize];
                        if h.1 > 0 && h.0 != x.want {
                            why += &format!(" zone{z}@conn{}x{}", h.0, h.1);
                        }
                    }
                    let t = cn.to_lane as usize;
                    let space = self.lane_q[t].last().map_or(self.net.lanes[t].len, |lv| self.veh[*lv as usize].s - self.veh[*lv as usize].len) - self.resv[t];
                    why += &format!(" to_lane{t} space={space:.1} resv={:.1} turn={:?} node={} ctrl={:?} from_ring={}", self.resv[t], cn.turn, cn.node, self.net.nodes[cn.node as usize].ctrl, self.net.links[cn.from_link as usize].ring);
                }
                if let On::Lane(l) = x.on {
                    let ri = x.ri as usize;
                    why += &format!(" need={} fresh={} reroute={} idx={} plan={:?} route={:?} lane_to={:?}", x.need, x.fresh, x.reroute, self.net.lanes[l as usize].idx, &x.plan[ri..(ri + 2).min(x.plan.len())], &x.route[ri..(ri + 2).min(x.route.len())],
                        self.net.lanes[l as usize].conns.iter().map(|c| self.net.conns[*c as usize].to_link).collect::<Vec<_>>());
                }
                println!("stuck v{v} {cont} s={:.1}/{len:.1} v={:.2} want={} granted={} zconn={} resv_lane={} ri={}/{}{why}", x.s, x.v, x.want, x.granted, x.zconn, x.resv_lane, x.ri, x.route.len());
            }
            self.stats.despawn_stuck += 1;
            let w = self.veh[v as usize].want;
            if w != NONE {
                let cn = &self.net.conns[w as usize];
                self.stats.stuck_targets.push(cn.to_link);
                self.stats.stuck_nodes.push((cn.node, self.net.links[cn.to_link as usize].to));
            } else if let On::Lane(l) = self.veh[v as usize].on {
                let n = self.net.links[self.net.lanes[l as usize].link as usize].to;
                self.stats.stuck_nodes.push((n, n));
            } else if let On::Conn(c) = self.veh[v as usize].on {
                let cn = &self.net.conns[c as usize];
                self.stats.stuck_nodes.push((cn.node, self.net.links[cn.to_link as usize].to));
            }
            self.despawn(v);
        }
        self.kill_l = kill;
        if self.tick % 100 == 0 {
            self.break_cycles();
        }
    }

    /// Wait-for graph over vehicles stuck > 20 s; break cycles by despawning the latest trip.
    fn break_cycles(&mut self) {
        let stuck: Vec<u32> = (0..self.veh.len() as u32).filter(|v| self.veh[*v as usize].on != On::Off && self.veh[*v as usize].wait_s > 20.0).collect();
        if stuck.is_empty() {
            return;
        }
        let mut is_stuck = vec![false; self.veh.len()];
        for v in &stuck {
            is_stuck[*v as usize] = true;
        }
        // Holders per connector (granted vehicles not yet past their zones).
        let mut edges: Vec<Vec<u32>> = vec![Vec::new(); self.veh.len()];
        for &v in &stuck {
            let x = &self.veh[v as usize];
            let (q, idx) = match x.on {
                On::Lane(l) => (&self.lane_q[l as usize], l),
                On::Conn(c) => (&self.conn_q[c as usize], c),
                On::Off => continue,
            };
            let _ = idx;
            let pos = q.iter().position(|o| *o == v).unwrap_or(0);
            if pos > 0 {
                edges[v as usize].push(q[pos - 1]);
                continue;
            }
            let target = match x.on {
                On::Conn(c) => Some(self.net.conns[c as usize].to_lane),
                On::Lane(_) if x.want != NONE => Some(self.net.conns[x.want as usize].to_lane),
                _ => None,
            };
            if let Some(t) = target
                && let Some(&b) = self.lane_q[t as usize].last()
            {
                edges[v as usize].push(b);
            }
            if x.want != NONE && !x.granted {
                for &(z, _, _) in &self.net.conns[x.want as usize].zones {
                    let h = self.zone_hold[z as usize];
                    if h.1 > 0 && h.0 != x.want {
                        // Holders: vehicles granted on h.0 or with zones pending on it.
                        for (o, y) in self.veh.iter().enumerate() {
                            if y.on != On::Off && (y.zconn == h.0 || (y.granted && y.want == h.0)) {
                                edges[v as usize].push(o as u32);
                            }
                        }
                    }
                }
            }
        }
        // DFS cycle detection among stuck vehicles (deterministic order).
        let n = self.veh.len();
        let mut color = vec![0u8; n];
        let mut victims: Vec<u32> = Vec::new();
        for &s in &stuck {
            if color[s as usize] != 0 {
                continue;
            }
            let mut stack: Vec<(u32, usize)> = vec![(s, 0)];
            let mut path: Vec<u32> = vec![s];
            color[s as usize] = 1;
            while let Some(&mut (u, ref mut i)) = stack.last_mut() {
                if *i < edges[u as usize].len() {
                    let w = edges[u as usize][*i];
                    *i += 1;
                    if !is_stuck[w as usize] {
                        continue;
                    }
                    if color[w as usize] == 1 {
                        if let Some(p) = path.iter().position(|x| *x == w) {
                            let cyc = &path[p..];
                            if let Some(&victim) = cyc.iter().max_by_key(|v| (self.veh[**v as usize].start_tick, **v)) {
                                victims.push(victim);
                            }
                        }
                    } else if color[w as usize] == 0 {
                        color[w as usize] = 1;
                        stack.push((w, 0));
                        path.push(w);
                    }
                } else {
                    color[u as usize] = 2;
                    stack.pop();
                    path.pop();
                }
            }
        }
        victims.sort_unstable();
        victims.dedup();
        for v in victims {
            self.stats.despawn_cycle += 1;
            self.despawn(v);
        }
    }

    fn speeds(&mut self) {
        // New routing epoch every 1 game hour = 120 sim s.
        if self.tick % self.epoch_ticks == 0 {
            let cost: Vec<u32> = (0..self.net.links.len())
                .map(|l| {
                    let lk = &self.net.links[l];
                    let lanes = lk.lane0 as usize..(lk.lane0 + u32::from(lk.lanes)) as usize;
                    let (sum, cnt) = lanes.fold((0.0f32, 0u32), |a, q| (a.0 + self.v_sum[q], a.1 + self.v_cnt[q]));
                    let v = if cnt > 0 { (sum / cnt as f32).clamp(0.2 * lk.v_free, lk.v_free) } else { lk.v_free };
                    ((lk.len / v) * 1000.0) as u32 + 1
                })
                .collect();
            let cost = if std::env::var("ST_NOEPOCH").is_ok() { self.router.free.clone() } else { cost };
            self.router.new_epoch(self.tick / self.epoch_ticks, cost);
            self.v_sum.iter_mut().for_each(|v| *v = 0.0);
            self.v_cnt.iter_mut().for_each(|v| *v = 0);
        }
    }

    /// No overlaps in any queue; no zone physically occupied by two connectors.
    fn checks(&mut self) {
        for q in self.lane_q.iter().chain(self.conn_q.iter()) {
            for w in q.windows(2) {
                let (a, b) = (&self.veh[w[0] as usize], &self.veh[w[1] as usize]);
                if b.s > a.s - a.len + 1e-3 {
                    self.stats.overlaps += 1;
                }
            }
        }
        let mut occ: Vec<(u32, u32)> = Vec::new(); // (zone, conn)
        for x in &self.veh {
            if x.on == On::Off || x.zconn == NONE {
                continue;
            }
            let cn = &self.net.conns[x.zconn as usize];
            let pos = match x.on {
                On::Conn(c) if c == x.zconn => x.s,
                On::Lane(l) if l == cn.to_lane => x.zoff + x.s,
                _ => continue,
            };
            for &(z, a, b) in &cn.zones {
                if pos >= a && pos - x.len <= b {
                    occ.push((z, x.zconn));
                }
            }
        }
        occ.sort_unstable();
        occ.dedup();
        for w in occ.windows(2) {
            if w[0].0 == w[1].0 && w[0].1 != w[1].1 {
                self.stats.zone_conflicts += 1;
            }
        }
    }

    /// Removes a link (bulldoze): despawns vehicles on it and on its connectors.
    pub fn bulldoze(&mut self, l: u32) {
        self.net.remove_link(l);
        let lk = self.net.links[l as usize].clone();
        let mut victims: Vec<u32> = Vec::new();
        for i in 0..lk.lanes {
            victims.extend(self.lane_q[(lk.lane0 + u32::from(i)) as usize].iter());
        }
        for (c, cn) in self.net.conns.iter().enumerate() {
            if cn.from_link == l || cn.to_link == l {
                victims.extend(self.conn_q[c].iter());
            }
        }
        for (vid, x) in self.veh.iter().enumerate() {
            if x.granted && x.want != NONE && self.net.conns[x.want as usize].to_link == l {
                victims.push(vid as u32);
            }
        }
        victims.sort_unstable();
        victims.dedup();
        for v in victims {
            self.stats.despawn_bulldoze += 1;
            self.despawn(v);
        }
        self.router.rebuild(&self.net);
        self.router.evict_link(l);
        // Searches in flight may cross the removed link: restart them on the new graph.
        for i in 0..BATCH {
            let Some(sr) = self.slot_req[i].take() else { continue };
            let _ = self.router.take_done(i);
            match sr.key {
                Some(k) => {
                    self.router.start(i, &[(k.o, 0)], k.d, k.variant);
                    self.slot_req[i] = Some(sr);
                }
                None => {
                    self.router.cancel(i);
                    for w in sr.waiters {
                        if let Waiter::Reroute(vid, _, _) = w {
                            self.veh[vid as usize].rr_pending = false;
                            self.veh[vid as usize].reroute = true;
                        }
                    }
                }
            }
        }
        self.control_lists();
        for y in &mut self.veh {
            y.fresh = true;
        }
        self.net.version += 1;
    }

    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |v: u64| {
            h ^= v;
            h = h.wrapping_mul(0x100_0000_01b3);
        };
        for (i, x) in self.veh.iter().enumerate() {
            if x.on == On::Off {
                continue;
            }
            mix(i as u64);
            mix(match x.on {
                On::Lane(l) => u64::from(l),
                On::Conn(c) => 1 << 40 | u64::from(c),
                On::Off => 0,
            });
            mix(u64::from(x.s.to_bits()));
            mix(u64::from(x.v.to_bits()));
            mix(u64::from(x.ri));
        }
        mix(self.stats.arrived);
        h
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Allow {
    Yes,
    No,
    Rbl,
}
