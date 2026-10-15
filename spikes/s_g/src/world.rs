//! Scenario construction: random roads, sloped junctions, parallel pairs, bridges.

use cb_core::rng::Pcg32;
use glam::Vec2;

use crate::road::{self, Junction, Kind, ProfileInput, Road};
use crate::terrain::{self, Mode, N, SP, Stamp, Terrain, idx};

pub struct World {
    pub t: Terrain,
    pub roads: Vec<Road>,
    pub junctions: Vec<Junction>,
    pub stamps: Vec<Stamp>,
    /// Owner road (+1) of core samples; 0 = free.
    pub owner: Vec<u16>,
    pub next_id: u32,
    /// Deck underside height above each sample under a bridge span (NaN = none).
    pub deck: Vec<f32>,
    pub mode: Mode,
    pub rejected_steep: usize,
    pub rejected_conflict: usize,
    pub pier_unresolved: usize,
}

#[derive(Clone, Default)]
pub struct Opts {
    /// Roads this one may overlap (overpass target, junction siblings).
    pub exempt: Vec<usize>,
    /// Start / end fixed heights (None = terrain).
    pub h0: Option<f32>,
    pub h1: Option<f32>,
    pub land0: f32,
    pub land1: f32,
    pub cut0: f32,
    pub cut1: f32,
    /// Player elevation offset applied to the whole profile target.
    pub g_offset: f32,
    /// Skip stamping (junction approaches stamp after the junction is known).
    pub defer_stamp: bool,
}

impl World {
    pub fn new(t: Terrain, mode: Mode) -> Self {
        World {
            t,
            roads: Vec::new(),
            junctions: Vec::new(),
            stamps: Vec::new(),
            owner: vec![0; N * N],
            next_id: 1,
            deck: vec![f32::NAN; N * N],
            mode,
            rejected_steep: 0,
            rejected_conflict: 0,
            pier_unresolved: 0,
        }
    }

    pub fn height(&self, p: Vec2) -> f32 {
        road::ground(&self.t, p)
    }

    fn conflicts(&self, cl: &[Vec2], hw: f32, exempt: &[usize]) -> bool {
        let r = hw + road::SHOULDER;
        for p in cl.iter().step_by(2) {
            let i0 = ((p.x - r) / SP).floor().max(0.0) as usize;
            let j0 = ((p.y - r) / SP).floor().max(0.0) as usize;
            let i1 = (((p.x + r) / SP).ceil() as usize).min(N - 1);
            let j1 = (((p.y + r) / SP).ceil() as usize).min(N - 1);
            if p.x < 50.0 || p.y < 50.0 || p.x > 4046.0 || p.y > 4046.0 {
                return true;
            }
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let o = self.owner[idx(i, j)];
                    if o != 0
                        && !exempt.contains(&(o as usize - 1))
                        && terrain::pos(i, j).distance(*p) <= r
                    {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Adds a road; returns its index or None (conflict / too steep).
    pub fn add_road(&mut self, ctrl: &[Vec2], kind: Kind, o: &Opts) -> Option<usize> {
        let (cl, s) = road::centreline(ctrl);
        if self.conflicts(&cl, 0.5 * kind.width(), &o.exempt) {
            self.rejected_conflict += 1;
            return None;
        }
        let len = *s.last().unwrap_or(&0.0);
        let ps = road::profile_samples(len);
        let mut r = Road {
            id: self.roads.len(),
            kind,
            cl,
            s,
            ps: ps.clone(),
            ph: Vec::new(),
            pm: Vec::new(),
            spans: Vec::new(),
            piers: Vec::new(),
            cut0: o.cut0,
            cut1: o.cut1,
        };
        let n = ps.len();
        let pts: Vec<Vec2> = ps.iter().map(|v| r.point_at(*v)).collect();
        let g: Vec<f32> = pts.iter().map(|p| self.height(*p) + o.g_offset).collect();
        let water: Vec<f32> = pts
            .iter()
            .map(|p| {
                let (i, j) = ((p.x / SP).round() as usize, (p.y / SP).round() as usize);
                self.t.water[idx(i.min(N - 1), j.min(N - 1))]
            })
            .collect();
        // Other roads under this one (overpass): (other index, other surface h).
        let under: Vec<Option<f32>> = pts
            .iter()
            .map(|p| {
                let (i, j) = ((p.x / SP).round() as usize, (p.y / SP).round() as usize);
                let ow = self.owner[idx(i.min(N - 1), j.min(N - 1))];
                if ow == 0 {
                    return None;
                }
                let other = &self.roads[ow as usize - 1];
                let (so, _) = other.project(*p);
                Some(other.h(so))
            })
            .collect();
        let h0 = o.h0.unwrap_or(g[0]);
        let h1 = o.h1.unwrap_or(g[n - 1]);
        let mut inp = ProfileInput {
            g: g.clone(),
            w: vec![1.0; n],
            lo: vec![f32::NEG_INFINITY; n],
            h0,
            h1,
            grade: kind.grade(),
            land0: o.land0,
            land1: o.land1,
        };
        // Clearance over other roads is a hard requirement for crossings that are not junctions.
        for i in 0..n {
            if let Some(hu) = under[i] {
                inp.lo[i] = hu + 5.5 + kind.deck();
            }
        }
        let Some(mut h) = road::solve_profile(&ps, &inp) else {
            self.rejected_steep += 1;
            return None;
        };
        // Bridge candidates (hysteresis: enter at threshold, exit at threshold − 1 m), from the
        // terrain-following profile only (re-solving the deck must not grow the spans).
        {
            let thr = kind.fill_bridge();
            let mut cin = vec![false; n];
            let mut cout = vec![false; n];
            let mut near = vec![false; n];
            for i in 0..n {
                let fill = h[i] - g[i];
                let wet = !water[i].is_nan();
                let over = under[i].is_some_and(|hu| h[i] - hu >= 4.0);
                let over_out = under[i].is_some_and(|hu| h[i] - hu >= 3.0);
                cin[i] = wet || fill > thr || over;
                cout[i] = wet || fill > thr - 1.0 || over_out;
                near[i] = !wet && fill <= 1.0 && under[i].is_none();
            }
            r.spans = road::spans_from(&ps, &cin, &cout, &near, len);
            // Re-solve the deck: no terrain attraction on spans, clearance over water/roads.
            for i in 0..n {
                let on = r.spans.iter().any(|(a, b)| ps[i] >= *a && ps[i] <= *b);
                inp.w[i] = if on { 0.0 } else { 1.0 };
                let mut lo = f32::NEG_INFINITY;
                if !water[i].is_nan() {
                    lo = water[i] + 4.0 + kind.deck();
                }
                if let Some(hu) = under[i] {
                    lo = lo.max(hu + 5.5 + kind.deck());
                }
                inp.lo[i] = lo;
            }
            if !r.spans.is_empty() {
                match road::solve_profile(&ps, &inp) {
                    Some(hh) => h = hh,
                    None => {
                        self.rejected_steep += 1;
                        return None;
                    }
                }
            }
        }
        road::set_profile(&mut r, h);
        // A road passing under an existing deck needs 4.5 m clearance (else a conflict).
        for k in 0..r.cl.len() {
            if k % 2 != 0 {
                continue;
            }
            let p = r.cl[k];
            let (i, j) = ((p.x / SP).round() as usize, (p.y / SP).round() as usize);
            let dk = self.deck[idx(i.min(N - 1), j.min(N - 1))];
            if !dk.is_nan() && dk - r.h(r.s[k]) < 4.5 && !r.on_span(r.s[k]) {
                self.rejected_conflict += 1;
                return None;
            }
        }
        let idx_r = self.roads.len();
        // Mark this road's decks.
        for &(a, b) in &r.spans {
            let mut sv = a;
            while sv <= b {
                let p = r.point_at(sv);
                let under = r.h(sv) - kind.deck();
                let rad = r.hw() + 1.0;
                let (i0, j0) = (
                    ((p.x - rad) / SP).floor().max(0.0) as usize,
                    ((p.y - rad) / SP).floor().max(0.0) as usize,
                );
                let (i1, j1) = (
                    (((p.x + rad) / SP).ceil() as usize).min(N - 1),
                    (((p.y + rad) / SP).ceil() as usize).min(N - 1),
                );
                for jj in j0..=j1 {
                    for ii in i0..=i1 {
                        if terrain::pos(ii, jj).distance(p) <= rad {
                            let d = &mut self.deck[idx(ii, jj)];
                            *d = if d.is_nan() { under } else { d.min(under) };
                        }
                    }
                }
                sv += 1.0;
            }
        }
        if !r.spans.is_empty() {
            let owner = &self.owner;
            let roads = &self.roads;
            let forbidden = |p: Vec2| {
                let (i, j) = ((p.x / SP).round() as usize, (p.y / SP).round() as usize);
                let _ = roads;
                let r = 2usize;
                for dj in 0..=2 * r {
                    for di in 0..=2 * r {
                        let (ii, jj) = (
                            (i + di).saturating_sub(r).min(N - 1),
                            (j + dj).saturating_sub(r).min(N - 1),
                        );
                        if owner[idx(ii, jj)] != 0 {
                            return true;
                        }
                    }
                }
                false
            };
            let (piers, bad) = road::place_piers(&r, &forbidden);
            r.piers = piers;
            self.pier_unresolved += bad;
        }
        self.roads.push(r);
        if !o.defer_stamp {
            self.stamp_road(idx_r);
        }
        Some(idx_r)
    }

    pub fn stamp_road(&mut self, ri: usize) {
        let id = self.next_id;
        let st = road::corridor_stamp(&self.roads[ri], &self.t, id);
        let mut new = vec![st];
        let ab = road::abutment_stamps(&self.roads[ri], &self.t, id + 1);
        self.next_id = id + 1 + ab.len() as u32;
        new.extend(ab);
        for s in &new {
            for e in &s.entries {
                if e.core && s.prio == 2 && self.owner[e.idx as usize] == 0 {
                    self.owner[e.idx as usize] = (ri + 1) as u16;
                }
            }
        }
        let rect = union_rect(new.iter().map(|s| s.aabb));
        self.stamps.extend(new);
        terrain::composite(&mut self.t, &self.stamps, rect, self.mode);
    }

    pub fn add_junction(
        &mut self,
        node: Vec2,
        dirs: &[(Vec2, f32, Kind)],
        rng: &mut Pcg32,
    ) -> bool {
        let nh = self.height(node);
        let wmax = dirs.iter().map(|d| d.2.width()).fold(0.0, f32::max);
        let cut = 0.5 * wmax + 6.0;
        let first = self.roads.len();
        let mut approaches = Vec::new();
        let siblings: Vec<usize> = (first..first + dirs.len()).collect();
        for (dir, len, kind) in dirs {
            let perp = Vec2::new(-dir.y, dir.x);
            let j1 = (rng.next_f32() - 0.5) * 0.3 * len;
            let j2 = (rng.next_f32() - 0.5) * 0.4 * len;
            let ctrl = [
                node,
                node + *dir * (len * 0.35),
                node + *dir * (len * 0.7) + perp * j1,
                node + *dir * *len + perp * j2,
            ];
            let o = Opts {
                exempt: siblings.clone(),
                h0: Some(nh),
                land0: cut + 4.0,
                cut0: cut,
                defer_stamp: true,
                ..Default::default()
            };
            match self.add_road(&ctrl, *kind, &o) {
                Some(ri) => approaches.push((ri, true)),
                None => {
                    // Roll back this junction's roads.
                    self.roads.truncate(first);
                    return false;
                }
            }
        }
        let jn = Junction {
            node,
            h: nh,
            cut,
            approaches,
        };
        let id = self.next_id;
        self.next_id += 1;
        let st = road::junction_stamp(&jn, &self.roads, &self.t, id);
        let rect = st.aabb;
        for e in &st.entries {
            if e.core && self.owner[e.idx as usize] == 0 {
                self.owner[e.idx as usize] = (first + 1) as u16;
            }
        }
        self.stamps.push(st);
        terrain::composite(&mut self.t, &self.stamps, rect, self.mode);
        for ri in first..self.roads.len() {
            self.stamp_road(ri);
        }
        self.junctions.push(jn);
        true
    }

    /// Re-places all piers against the final corridors (roads built under existing spans).
    pub fn replace_piers(&mut self) {
        self.pier_unresolved = 0;
        for ri in 0..self.roads.len() {
            if self.roads[ri].spans.is_empty() {
                continue;
            }
            // Any other road's footprint (ground corridor or deck), checked geometrically.
            let roads = &self.roads;
            let bbs: Vec<(Vec2, Vec2)> = roads
                .iter()
                .map(|r| {
                    r.cl.iter().fold(
                        (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
                        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
                    )
                })
                .collect();
            let forbidden = |p: Vec2| {
                roads.iter().enumerate().any(|(k, o)| {
                    let m = o.hw() + road::SHOULDER + 1.0;
                    k != ri
                        && p.x >= bbs[k].0.x - m
                        && p.x <= bbs[k].1.x + m
                        && p.y >= bbs[k].0.y - m
                        && p.y <= bbs[k].1.y + m
                        && o.project(p).1 <= m
                })
            };
            let (piers, bad) = road::place_piers(&self.roads[ri], &forbidden);
            self.pier_unresolved += bad;
            self.roads[ri].piers = piers;
        }
    }
}

pub fn union_rect(
    it: impl Iterator<Item = (usize, usize, usize, usize)>,
) -> (usize, usize, usize, usize) {
    let mut r = (usize::MAX, usize::MAX, 0, 0);
    for a in it {
        r.0 = r.0.min(a.0);
        r.1 = r.1.min(a.1);
        r.2 = r.2.max(a.2);
        r.3 = r.3.max(a.3);
    }
    if r.0 == usize::MAX { (0, 0, 0, 0) } else { r }
}

pub fn slope_at(t: &Terrain, p: Vec2) -> f32 {
    let h = |q: Vec2| terrain::interp(&t.base, N, SP, q);
    let dx = (h(p + Vec2::X * 4.0) - h(p - Vec2::X * 4.0)) / 8.0;
    let dz = (h(p + Vec2::Y * 4.0) - h(p - Vec2::Y * 4.0)) / 8.0;
    (dx * dx + dz * dz).sqrt()
}

fn rand_kind(rng: &mut Pcg32) -> Kind {
    match rng.weighted_index(&[0.2, 0.6, 0.2]) {
        Some(0) => Kind::Dirt,
        Some(1) => Kind::Street,
        _ => Kind::Avenue,
    }
}

fn rot(v: Vec2, a: f32) -> Vec2 {
    let (s, c) = a.sin_cos();
    Vec2::new(c * v.x - s * v.y, s * v.x + c * v.y)
}

/// Feature indices for the report and the shots.
#[derive(Default, Debug, Clone)]
pub struct Features {
    pub river_bridges: Vec<usize>,
    pub valley_bridges: Vec<usize>,
    pub overpass_lower: Vec<usize>,
    pub overpass_upper: Vec<usize>,
    pub pairs: Vec<(usize, usize, f32, f32)>,
    pub junctions: Vec<usize>,
    pub random: Vec<usize>,
}

pub fn build(seed: u32, mode: Mode, n_random: usize, n_junctions: usize) -> (World, Features) {
    let t = terrain::generate(seed);
    let mut w = World::new(t, mode);
    let mut f = Features::default();
    let mut rng = Pcg32::from_seed(u64::from(seed) ^ 0x5_6000);

    // 1. River bridges: two streets/avenues crossing the river roughly north–south.
    for (x, kind) in [(1100.0f32, Kind::Street), (3000.0, Kind::Avenue)] {
        let z = terrain::river_z(x);
        let ctrl = [
            Vec2::new(x - 20.0, z - 170.0),
            Vec2::new(x - 8.0, z - 60.0),
            Vec2::new(x + 8.0, z + 60.0),
            Vec2::new(x + 20.0, z + 170.0),
        ];
        if let Some(ri) = w.add_road(&ctrl, kind, &Opts::default()) {
            f.river_bridges.push(ri);
        }
    }
    // 2. Valley bridges: across the synthetic 30 m valley.
    let (va, vb) = (terrain::VALLEY[0], terrain::VALLEY[1]);
    let vd = (vb - va).normalize();
    let vn = Vec2::new(-vd.y, vd.x);
    for (t, kind) in [(0.35f32, Kind::Street), (0.7, Kind::Street)] {
        let c = va + (vb - va) * t;
        let ctrl = [c - vn * 230.0, c - vn * 80.0, c + vn * 80.0, c + vn * 230.0];
        if let Some(ri) = w.add_road(&ctrl, kind, &Opts::default()) {
            f.valley_bridges.push(ri);
        }
    }
    // 3. Overpass: a ground street and two roads crossing over it.
    let base_pt = Vec2::new(2600.0, 1200.0);
    let lower = w.add_road(
        &[
            base_pt - Vec2::X * 350.0,
            base_pt - Vec2::X * 120.0,
            base_pt + Vec2::X * 120.0,
            base_pt + Vec2::X * 350.0,
        ],
        Kind::Street,
        &Opts::default(),
    );
    if let Some(lo) = lower {
        f.overpass_lower.push(lo);
        for (dx, ang, kind) in [
            (-120.0f32, 0.15f32, Kind::Street),
            (140.0, -0.35, Kind::Avenue),
        ] {
            let c = base_pt + Vec2::X * dx;
            let d = rot(Vec2::Y, ang);
            let ctrl = [c - d * 380.0, c - d * 130.0, c + d * 130.0, c + d * 380.0];
            let o = Opts {
                exempt: vec![lo],
                ..Default::default()
            };
            if let Some(ri) = w.add_road(&ctrl, kind, &o) {
                f.overpass_upper.push(ri);
            }
        }
    }
    // 4. Junctions on 4–10 % slopes.
    let mut tries = 0;
    while f.junctions.len() < n_junctions && tries < 4000 {
        tries += 1;
        let p = Vec2::new(
            300.0 + rng.next_f32() * 3500.0,
            300.0 + rng.next_f32() * 3500.0,
        );
        let sl = slope_at(&w.t, p);
        if !(0.04..=0.10).contains(&sl) || (p.y - terrain::river_z(p.x)).abs() < 200.0 {
            continue;
        }
        let k = if rng.chance(0.5) { 3 } else { 4 };
        let a0 = rng.next_f32() * std::f32::consts::TAU;
        let dirs: Vec<(Vec2, f32, Kind)> = (0..k)
            .map(|q| {
                let a =
                    a0 + std::f32::consts::TAU * q as f32 / k as f32 + (rng.next_f32() - 0.5) * 0.5;
                let kind = if rng.chance(0.2) {
                    Kind::Avenue
                } else {
                    Kind::Street
                };
                (rot(Vec2::X, a), 120.0 + rng.next_f32() * 130.0, kind)
            })
            .collect();
        if w.add_junction(p, &dirs, &mut rng) {
            f.junctions.push(w.junctions.len() - 1);
        }
    }
    // 5. Parallel pairs 8–20 m apart (edge to edge) at different heights.
    tries = 0;
    while f.pairs.len() < 5 && tries < 2000 {
        tries += 1;
        let p = Vec2::new(
            300.0 + rng.next_f32() * 3500.0,
            300.0 + rng.next_f32() * 3500.0,
        );
        let sl = slope_at(&w.t, p);
        if !(0.03..=0.12).contains(&sl) {
            continue;
        }
        let d = rot(Vec2::X, rng.next_f32() * std::f32::consts::TAU);
        let nrm = Vec2::new(-d.y, d.x);
        let (ka, kb) = (
            Kind::Street,
            if rng.chance(0.5) {
                Kind::Street
            } else {
                Kind::Avenue
            },
        );
        let gap = 8.0 + rng.next_f32() * 12.0;
        let off = 0.5 * ka.width() + gap + 0.5 * kb.width();
        let dh = (1.0 + rng.next_f32() * 3.0) * if rng.chance(0.5) { 1.0 } else { -1.0 };
        let ca = [p - d * 160.0, p - d * 50.0, p + d * 50.0, p + d * 160.0];
        let cb: Vec<Vec2> = ca.iter().map(|q| *q + nrm * off).collect();
        let Some(a) = w.add_road(&ca, ka, &Opts::default()) else {
            continue;
        };
        let ob = Opts {
            g_offset: dh,
            ..Default::default()
        };
        if let Some(b) = w.add_road(&cb, kb, &ob) {
            f.pairs.push((a, b, gap, dh));
        }
    }
    // 6. Random roads.
    tries = 0;
    while f.random.len() < n_random && tries < 20_000 {
        tries += 1;
        let kind = rand_kind(&mut rng);
        let len = 150.0 + rng.next_f32() * 450.0;
        let p = Vec2::new(
            150.0 + rng.next_f32() * 3800.0,
            150.0 + rng.next_f32() * 3800.0,
        );
        let mut d = rot(Vec2::X, rng.next_f32() * std::f32::consts::TAU);
        let mut ctrl = vec![p];
        let segs = 4;
        for _ in 0..segs {
            d = rot(d, (rng.next_f32() - 0.5) * 0.7);
            let q = *ctrl.last().unwrap_or(&p) + d * (len / segs as f32);
            ctrl.push(q);
        }
        if let Some(ri) = w.add_road(&ctrl, kind, &Opts::default()) {
            f.random.push(ri);
        }
    }
    w.replace_piers();
    (w, f)
}
