//! Roads: centrelines, vertical profile solver (ALGORITHMS §2), bridges (§3), corridor,
//! abutment and junction stamps (ADR-0046).

use glam::Vec2;

use crate::terrain::{self, Entry, N, SP, Stamp, Terrain, idx, pos};

pub const CARVE: f32 = 0.15;
pub const SHOULDER: f32 = 3.5;
pub const FILL_SLOPE: f32 = 1.0 / 1.5;
pub const CUT_SLOPE: f32 = 1.0;
pub const PROFILE_DS: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Dirt,
    Street,
    Avenue,
}

impl Kind {
    pub fn width(self) -> f32 {
        match self {
            Kind::Dirt => 6.0,
            Kind::Street => 12.0,
            Kind::Avenue => 22.0,
        }
    }
    pub fn grade(self) -> f32 {
        match self {
            Kind::Dirt => 0.14,
            Kind::Street => 0.10,
            Kind::Avenue => 0.08,
        }
    }
    pub fn fill_bridge(self) -> f32 {
        if self == Kind::Avenue { 8.0 } else { 6.0 }
    }
    pub fn deck(self) -> f32 {
        if self == Kind::Avenue { 1.6 } else { 1.2 }
    }
    pub fn pier_spacing(self) -> f32 {
        match self {
            Kind::Dirt => 24.0,
            Kind::Street => 30.0,
            Kind::Avenue => 36.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Road {
    pub id: usize,
    pub kind: Kind,
    /// Centreline at ~1 m.
    pub cl: Vec<Vec2>,
    pub s: Vec<f32>,
    /// Profile samples every PROFILE_DS (last at len) + Fritsch–Carlson tangents.
    pub ps: Vec<f32>,
    pub ph: Vec<f32>,
    pub pm: Vec<f32>,
    /// Bridge spans [s0, s1].
    pub spans: Vec<(f32, f32)>,
    pub piers: Vec<(Vec2, f32, f32)>,
    /// Junction cut-back at start/end (0 = free end).
    pub cut0: f32,
    pub cut1: f32,
}

impl Road {
    pub fn len(&self) -> f32 {
        *self.s.last().unwrap_or(&0.0)
    }
    pub fn hw(&self) -> f32 {
        0.5 * self.kind.width()
    }
    pub fn point_at(&self, s: f32) -> Vec2 {
        let i = self
            .s
            .partition_point(|v| *v <= s)
            .clamp(1, self.s.len() - 1);
        let (s0, s1) = (self.s[i - 1], self.s[i]);
        let t = if s1 > s0 {
            ((s - s0) / (s1 - s0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.cl[i - 1].lerp(self.cl[i], t)
    }
    pub fn dir_at(&self, s: f32) -> Vec2 {
        let i = self
            .s
            .partition_point(|v| *v <= s)
            .clamp(1, self.s.len() - 1);
        (self.cl[i] - self.cl[i - 1]).normalize_or_zero()
    }
    /// Road surface height (monotone cubic Hermite on the profile samples).
    pub fn h(&self, s: f32) -> f32 {
        let n = self.ps.len();
        let s = s.clamp(0.0, self.ps[n - 1]);
        let i = self.ps.partition_point(|v| *v <= s).clamp(1, n - 1) - 1;
        let hh = self.ps[i + 1] - self.ps[i];
        let t = (s - self.ps[i]) / hh;
        let (t2, t3) = (t * t, t * t * t);
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;
        h00 * self.ph[i] + h10 * hh * self.pm[i] + h01 * self.ph[i + 1] + h11 * hh * self.pm[i + 1]
    }
    pub fn on_span(&self, s: f32) -> bool {
        self.spans.iter().any(|(a, b)| s > *a && s < *b)
    }
    /// Arclength of the nearest centreline point within `reach` of the start / end.
    pub fn project_near(&self, p: Vec2, at_start: bool, reach: f32) -> f32 {
        let len = self.len();
        let mut best = (0.0, f32::INFINITY);
        for i in 0..self.cl.len() - 1 {
            let in_range = if at_start {
                self.s[i] <= reach
            } else {
                self.s[i + 1] >= len - reach
            };
            if !in_range {
                continue;
            }
            let (a, b) = (self.cl[i], self.cl[i + 1]);
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
            let d = p.distance(a + ab * t);
            if d < best.1 {
                best = (self.s[i] + t * (self.s[i + 1] - self.s[i]), d);
            }
        }
        best.0
    }
    /// Nearest centreline point: (s, distance).
    pub fn project(&self, p: Vec2) -> (f32, f32) {
        let mut best = (0.0, f32::INFINITY);
        for i in 0..self.cl.len() - 1 {
            let (a, b) = (self.cl[i], self.cl[i + 1]);
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
            let d = p.distance(a + ab * t);
            if d < best.1 {
                best = (self.s[i] + t * (self.s[i + 1] - self.s[i]), d);
            }
        }
        best
    }
}

/// Densifies control points (Catmull-Rom) into a ~1 m centreline.
pub fn centreline(ctrl: &[Vec2]) -> (Vec<Vec2>, Vec<f32>) {
    let mut pts = vec![ctrl[0]];
    let n = ctrl.len();
    for k in 0..n - 1 {
        let p0 = ctrl[k.saturating_sub(1)];
        let p1 = ctrl[k];
        let p2 = ctrl[k + 1];
        let p3 = ctrl[(k + 2).min(n - 1)];
        let seg = p1.distance(p2).ceil().max(1.0) as usize;
        for q in 1..=seg {
            let t = q as f32 / seg as f32;
            let (t2, t3) = (t * t, t * t * t);
            let v = 0.5
                * ((2.0 * p1)
                    + (-p0 + p2) * t
                    + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
                    + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3);
            pts.push(v);
        }
    }
    let mut s = vec![0.0];
    for i in 1..pts.len() {
        let d = s[i - 1] + pts[i].distance(pts[i - 1]);
        s.push(d);
    }
    (pts, s)
}

/// Fritsch–Carlson tangents.
fn fc_tangents(x: &[f32], y: &[f32]) -> Vec<f32> {
    let n = x.len();
    let d: Vec<f32> = (0..n - 1)
        .map(|i| (y[i + 1] - y[i]) / (x[i + 1] - x[i]))
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

pub struct ProfileInput {
    pub g: Vec<f32>,
    /// Terrain attraction weight per sample (0 on bridge spans).
    pub w: Vec<f32>,
    /// Lower bounds (clearance), NEG_INFINITY if none.
    pub lo: Vec<f32>,
    pub h0: f32,
    pub h1: f32,
    pub grade: f32,
    /// Landing (≤ 4 %) distance from start / end.
    pub land0: f32,
    pub land1: f32,
}

/// ALGORITHMS §2: smoothing + grade clamping; None = too steep / infeasible.
pub fn solve_profile(ps: &[f32], inp: &ProfileInput) -> Option<Vec<f32>> {
    let n = ps.len();
    let lam = 1.0f32;
    let len = ps[n - 1];
    let max_step = |i: usize| -> f32 {
        let ds = ps[i + 1] - ps[i];
        let s = 0.5 * (ps[i] + ps[i + 1]);
        let g = if s < inp.land0 || len - s < inp.land1 {
            0.04
        } else {
            inp.grade
        };
        g * ds
    };
    // Grade-feasible envelope of the lower bounds (clearance ramps).
    let mut lo = inp.lo.clone();
    for i in 1..n {
        lo[i] = lo[i].max(lo[i - 1] - max_step(i - 1));
    }
    for i in (0..n - 1).rev() {
        lo[i] = lo[i].max(lo[i + 1] - max_step(i));
    }
    if inp.h0 < lo[0] - 1e-3 || inp.h1 < lo[n - 1] - 1e-3 {
        return None;
    }
    let mut h: Vec<f32> = inp.g.iter().zip(&lo).map(|(g, l)| g.max(*l)).collect();
    h[0] = inp.h0;
    h[n - 1] = inp.h1;
    for _ in 0..60 {
        let mut maxd: f32 = 0.0;
        for i in 1..n - 1 {
            let hm2 = h[i.saturating_sub(2)];
            let hm1 = h[i - 1];
            let hp1 = h[i + 1];
            let hp2 = h[(i + 2).min(n - 1)];
            let w = inp.w[i];
            let nv = (w * inp.g[i] + lam * (4.0 * hm1 + 4.0 * hp1 - hm2 - hp2)) / (w + 6.0 * lam);
            let nv = nv.max(lo[i]);
            maxd = maxd.max((nv - h[i]).abs());
            h[i] = nv;
        }
        for i in 1..n - 1 {
            let m = max_step(i - 1);
            h[i] = h[i].clamp(h[i - 1] - m, h[i - 1] + m);
        }
        for i in (1..n - 1).rev() {
            let m = max_step(i);
            h[i] = h[i].clamp(h[i + 1] - m, h[i + 1] + m);
        }
        for i in 1..n - 1 {
            h[i] = h[i].max(lo[i]);
        }
        if maxd < 0.01 {
            break;
        }
    }
    for i in 0..n - 1 {
        if (h[i + 1] - h[i]).abs() > max_step(i) + 1e-3 {
            return None;
        }
    }
    Some(h)
}

pub fn set_profile(r: &mut Road, h: Vec<f32>) {
    r.pm = fc_tangents(&r.ps, &h);
    r.ph = h;
}

pub fn profile_samples(len: f32) -> Vec<f32> {
    let n = (len / PROFILE_DS).ceil().max(1.0) as usize;
    (0..=n).map(|i| (i as f32 * PROFILE_DS).min(len)).collect()
}

/// Bridge spans from candidates (ALGORITHMS §3 step 2): hysteresis, abutments, merging.
pub fn spans_from(
    ps: &[f32],
    cand_in: &[bool],
    cand_out: &[bool],
    near_ground: &[bool],
    len: f32,
) -> Vec<(f32, f32)> {
    let n = ps.len();
    let mut on = vec![false; n];
    let mut state = false;
    for i in 0..n {
        state = if state { cand_out[i] } else { cand_in[i] };
        on[i] = state;
    }
    let mut spans: Vec<(f32, f32)> = Vec::new();
    let mut i = 0;
    while i < n {
        if on[i] {
            let mut j = i;
            while j + 1 < n && on[j + 1] {
                j += 1;
            }
            // Extend to stable abutments: ≥ 4 m beyond the last sample with h − g ≤ 1 m.
            let mut a = i;
            while a > 0 && !near_ground[a] {
                a -= 1;
            }
            let mut b = j;
            while b + 1 < n && !near_ground[b] {
                b += 1;
            }
            let s0 = (ps[a] - 4.0).max(0.0);
            let s1 = (ps[b] + 4.0).min(len);
            spans.push((s0, s1));
            i = j + 1;
        } else {
            i += 1;
        }
    }
    // Merge closer than 20 m; min span 12 m.
    let mut out: Vec<(f32, f32)> = Vec::new();
    for sp in spans {
        if let Some(l) = out.last_mut()
            && sp.0 - l.1 < 20.0
        {
            l.1 = l.1.max(sp.1);
            continue;
        }
        out.push(sp);
    }
    for sp in &mut out {
        if sp.1 - sp.0 < 12.0 {
            let m = 0.5 * (sp.0 + sp.1);
            *sp = ((m - 6.0).max(0.0), (m + 6.0).min(len));
        }
    }
    out
}

/// Piers evenly spaced; shifted (±5 % steps up to ±25 %) or added/removed when inside a
/// forbidden zone. Returns (piers, conflict-unresolved).
pub fn place_piers(r: &Road, forbidden: &dyn Fn(Vec2) -> bool) -> (Vec<(Vec2, f32, f32)>, usize) {
    let mut out = Vec::new();
    let mut bad = 0;
    for &(s0, s1) in &r.spans {
        let span = s1 - s0;
        let base_n = ((span / r.kind.pier_spacing()).ceil() as usize).saturating_sub(1);
        let mut placed = None;
        'outer: for n in [base_n, base_n + 1, base_n.saturating_sub(1)] {
            if n > 0 && span / (n + 1) as f32 > 60.0 {
                continue;
            }
            if n == 0 && span > 60.0 {
                continue;
            }
            let gap = span / (n + 1) as f32;
            let mut ps = Vec::new();
            for k in 1..=n {
                let s = s0 + gap * k as f32;
                let mut ok = None;
                for step in [
                    0.0f32, 0.05, -0.05, 0.1, -0.1, 0.15, -0.15, 0.2, -0.2, 0.25, -0.25,
                ] {
                    let ss = s + step * gap;
                    if !forbidden(r.point_at(ss)) {
                        ok = Some(ss);
                        break;
                    }
                }
                match ok {
                    Some(ss) => ps.push(ss),
                    None => continue 'outer,
                }
            }
            placed = Some(ps);
            break;
        }
        match placed {
            Some(ps) => {
                for s in ps {
                    out.push((r.point_at(s), s, r.h(s) - r.kind.deck()));
                }
            }
            None => bad += 1,
        }
    }
    (out, bad)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Blend width so the steepest part of the smoothstep equals the nominal side slope.
fn blend_d(e: f32) -> f32 {
    if e > 0.0 {
        1.6 * e / FILL_SLOPE
    } else {
        1.6 * -e / CUT_SLOPE
    }
}

/// Corridor stamp of the road's ground parts (core = carriageway + shoulder, carved).
pub fn corridor_stamp(r: &Road, t: &Terrain, id: u32) -> Stamp {
    let mut k = 1.0;
    loop {
        let (st, clipped) = corridor_stamp_k(r, t, id, k);
        if !clipped || k > 8.0 {
            return st;
        }
        k *= 1.6;
    }
}

/// True if a blend entry with noticeable weight touches the stamp's AABB border.
fn clipped(entries: &[Entry], aabb: (usize, usize, usize, usize)) -> bool {
    entries.iter().any(|e| {
        let (i, j) = (e.idx as usize % N, e.idx as usize / N);
        !e.core
            && e.w > 0.01
            && (i == aabb.0 || i == aabb.2 || j == aabb.1 || j == aabb.3)
            && aabb.0 > 0
            && aabb.1 > 0
            && aabb.2 < N - 1
            && aabb.3 < N - 1
    })
}

fn corridor_stamp_k(r: &Road, t: &Terrain, id: u32, kmul: f32) -> (Stamp, bool) {
    let core_hw = r.hw() + SHOULDER;
    // Generous reach: core + blend for the largest |e| along the road.
    let mut emax: f32 = 2.0;
    for k in 0..r.ps.len() {
        let s = r.ps[k];
        if r.on_span(s) {
            continue;
        }
        let p = r.point_at(s);
        let (i, j) = ((p.x / SP).round() as usize, (p.y / SP).round() as usize);
        if i < N && j < N {
            emax = emax.max((r.h(s) - t.base[idx(i, j)]).abs() + 2.0);
        }
    }
    let reach = (core_hw + blend_d(emax).max(blend_d(-emax)) + 2.0) * kmul;
    let (mut lo, mut hi) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
    for p in &r.cl {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let i0 = (((lo.x - reach) / SP).floor().max(0.0)) as usize;
    let j0 = (((lo.y - reach) / SP).floor().max(0.0)) as usize;
    let i1 = (((hi.x + reach) / SP).ceil() as usize).min(N - 1);
    let j1 = (((hi.y + reach) / SP).ceil() as usize).min(N - 1);
    let w = i1 - i0 + 1;
    let hgt = j1 - j0 + 1;
    // (distance, s, end cap next to a bridge span)
    let mut best = vec![(f32::INFINITY, 0.0f32, f32::NAN); w * hgt];
    // Ground segments (2 m); runs that touch a span get rounded soft end caps.
    let step = 2usize;
    let mut segs: Vec<(usize, usize)> = Vec::new();
    let mut k = 0;
    while k + 1 < r.cl.len() {
        let k2 = (k + step).min(r.cl.len() - 1);
        if !r.on_span(0.5 * (r.s[k] + r.s[k2])) {
            segs.push((k, k2));
        }
        k = k2;
    }
    for (q, &(k, k2)) in segs.iter().enumerate() {
        let (a, b) = (r.cl[k], r.cl[k2]);
        let (sa, sb) = (r.s[k], r.s[k2]);
        let cap_start = (q == 0 || segs[q - 1].1 != k) && r.on_span(sa - 1.0);
        let cap_end = (q + 1 == segs.len() || segs[q + 1].0 != k2) && r.on_span(sb + 1.0);
        let ab = b - a;
        let l2 = ab.length_squared().max(1e-6);
        let bi0 = (((a.x.min(b.x) - reach) / SP).floor().max(i0 as f32)) as usize;
        let bj0 = (((a.y.min(b.y) - reach) / SP).floor().max(j0 as f32)) as usize;
        let bi1 = (((a.x.max(b.x) + reach) / SP).ceil() as usize).min(i1);
        let bj1 = (((a.y.max(b.y) + reach) / SP).ceil() as usize).min(j1);
        for j in bj0..=bj1 {
            for i in bi0..=bi1 {
                let p = pos(i, j);
                let raw = (p - a).dot(ab) / l2;
                let tt = raw.clamp(0.0, 1.0);
                let d = p.distance(a + ab * tt);
                let c = &mut best[(j - j0) * w + (i - i0)];
                if d < c.0 {
                    let l = l2.sqrt();
                    let beyond = if raw < 0.0 && cap_start {
                        -raw * l
                    } else if raw > 1.0 && cap_end {
                        (raw - 1.0) * l
                    } else {
                        f32::NAN
                    };
                    *c = (d, sa + (sb - sa) * tt, beyond);
                }
            }
        }
    }
    let mut entries = Vec::new();
    let len = r.len();
    for j in j0..=j1 {
        for i in i0..=i1 {
            let (d, s, beyond) = best[(j - j0) * w + (i - i0)];
            let cap = !beyond.is_nan();
            if !d.is_finite() {
                continue;
            }
            // Junction approaches: the junction stamp owns s < cut-back.
            if (r.cut0 > 0.0 && s < r.cut0) || (r.cut1 > 0.0 && s > len - r.cut1) {
                continue;
            }
            let tgt = r.h(s) - CARVE;
            let gi = idx(i, j);
            if cap {
                // Soft end cap under the deck start: road level ramping to deck underside − carve
                // over 4 m (continuous with the corridor), blend only.
                let tgt = r.h(s) - CARVE - r.kind.deck() * smoothstep(0.0, 4.0, beyond);
                let e = tgt - t.base[gi];
                let dd = (d - core_hw).max(0.0);
                let bd = blend_d(e);
                if dd < bd || dd == 0.0 {
                    let wgt = if bd > 0.0 {
                        1.0 - smoothstep(0.0, bd, dd)
                    } else {
                        1.0
                    };
                    entries.push(Entry {
                        idx: gi as u32,
                        t: tgt,
                        w: wgt,
                        core: false,
                    });
                }
            } else if d <= core_hw {
                entries.push(Entry {
                    idx: gi as u32,
                    t: tgt,
                    w: 1.0,
                    core: true,
                });
            } else {
                let e = tgt - t.base[gi];
                let dd = d - core_hw;
                let bd = blend_d(e);
                if dd < bd {
                    entries.push(Entry {
                        idx: gi as u32,
                        t: tgt,
                        w: 1.0 - smoothstep(0.0, bd, dd),
                        core: false,
                    });
                }
            }
        }
    }
    let aabb = (i0, j0, i1, j1);
    let c = clipped(&entries, aabb);
    (
        Stamp {
            id,
            owner: r.id,
            prio: 2,
            aabb,
            entries,
        },
        c,
    )
}

/// A junction: node + approach roads (index, at_start).
pub struct Junction {
    pub node: Vec2,
    pub h: f32,
    pub cut: f32,
    pub approaches: Vec<(usize, bool)>,
}

/// Junction surface height (angular blend of approach profiles, exact on every approach
/// carriageway inside the junction).
pub fn junction_surface(jn: &Junction, roads: &[Road], p: Vec2) -> f32 {
    let v = p - jn.node;
    let r = v.length();
    if r < 1e-3 {
        return jn.h;
    }
    let th = v.y.atan2(v.x);
    // (angle, half extent, height at radial distance)
    let mut ap: Vec<(f32, f32, f32)> = jn
        .approaches
        .iter()
        .map(|&(ri, at_start)| {
            let rd = &roads[ri];
            let (dir, hs) = if at_start {
                let d = rd.dir_at(0.0);
                (d, rd.h(rd.project_near(p, true, jn.cut + 12.0)))
            } else {
                let d = -rd.dir_at(rd.len());
                (d, rd.h(rd.project_near(p, false, jn.cut + 12.0)))
            };
            let ext = ((rd.hw() + 0.5) / jn.cut).atan();
            (dir.y.atan2(dir.x), ext, hs)
        })
        .collect();
    ap.sort_by(|a, b| a.0.total_cmp(&b.0));
    let wrap = |a: f32| {
        let mut a = a % std::f32::consts::TAU;
        if a < 0.0 {
            a += std::f32::consts::TAU;
        }
        a
    };
    // Inside an approach's extent: exact.
    for a in &ap {
        let d = wrap(th - a.0);
        let d = d.min(std::f32::consts::TAU - d);
        if d <= a.1 {
            return a.2;
        }
    }
    // Between consecutive approaches a (ccw-before) and b.
    let n = ap.len();
    for k in 0..n {
        let a = ap[k];
        let b = ap[(k + 1) % n];
        let start = wrap(a.0 + a.1);
        let gap = wrap(b.0 - b.1 - start);
        let pos = wrap(th - start);
        if pos <= gap {
            let t = smoothstep(0.0, gap.max(1e-4), pos);
            return a.2 + (b.2 - a.2) * t;
        }
    }
    jn.h
}

pub fn junction_stamp(jn: &Junction, roads: &[Road], t: &Terrain, id: u32) -> Stamp {
    let rad = jn.cut + SHOULDER;
    let reach = rad + 90.0;
    let i0 = (((jn.node.x - reach) / SP).floor().max(0.0)) as usize;
    let j0 = (((jn.node.y - reach) / SP).floor().max(0.0)) as usize;
    let i1 = (((jn.node.x + reach) / SP).ceil() as usize).min(N - 1);
    let j1 = (((jn.node.y + reach) / SP).ceil() as usize).min(N - 1);
    let mut entries = Vec::new();
    for j in j0..=j1 {
        for i in i0..=i1 {
            let p = pos(i, j);
            let d = p.distance(jn.node);
            let gi = idx(i, j);
            if d <= rad {
                entries.push(Entry {
                    idx: gi as u32,
                    t: junction_surface(jn, roads, p) - CARVE,
                    w: 1.0,
                    core: true,
                });
            } else {
                let q = jn.node + (p - jn.node) * (rad / d);
                let tgt = junction_surface(jn, roads, q) - CARVE;
                let e = tgt - t.base[gi];
                let bd = blend_d(e);
                let dd = d - rad;
                if dd < bd {
                    entries.push(Entry {
                        idx: gi as u32,
                        t: tgt,
                        w: 1.0 - smoothstep(0.0, bd, dd),
                        core: false,
                    });
                }
            }
        }
    }
    Stamp {
        id,
        owner: usize::MAX,
        prio: 2,
        aabb: (i0, j0, i1, j1),
        entries,
    }
}

/// Abutment pad under the first 4 m of the deck at each span end (core at deck bottom).
pub fn abutment_stamps(r: &Road, t: &Terrain, first_id: u32) -> Vec<Stamp> {
    let mut out = Vec::new();
    let mut id = first_id;
    for &(s0, s1) in &r.spans {
        for (se, dirn) in [(s0, 1.0f32), (s1, -1.0f32)] {
            if se <= 0.0 || se >= r.len() {
                continue;
            }
            let a = r.point_at(se);
            let dir = r.dir_at(se) * dirn;
            let nrm = Vec2::new(-dir.y, dir.x);
            let hw = r.hw() + 1.0;
            let tgt = r.h(se) - r.kind.deck() - CARVE;
            let reach = hw + 70.0;
            let i0 = (((a.x - reach) / SP).floor().max(0.0)) as usize;
            let j0 = (((a.y - reach) / SP).floor().max(0.0)) as usize;
            let i1 = (((a.x + reach) / SP).ceil() as usize).min(N - 1);
            let j1 = (((a.y + reach) / SP).ceil() as usize).min(N - 1);
            let mut entries = Vec::new();
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let p = pos(i, j);
                    let v = p - a;
                    let along = v.dot(dir);
                    let lat = v.dot(nrm).abs();
                    // Pad: [0, 4] m into the span, ±hw.
                    let dx = if along < 0.0 {
                        -along
                    } else if along > 4.0 {
                        along - 4.0
                    } else {
                        0.0
                    };
                    let dy = (lat - hw).max(0.0);
                    let d = (dx * dx + dy * dy).sqrt();
                    let gi = idx(i, j);
                    if along < 0.0 {
                        continue; // ground side belongs to the corridor
                    }
                    if d == 0.0 {
                        entries.push(Entry {
                            idx: gi as u32,
                            t: tgt,
                            w: 1.0,
                            core: true,
                        });
                    } else {
                        let e = tgt - t.base[gi];
                        let bd = blend_d(e);
                        if d < bd {
                            entries.push(Entry {
                                idx: gi as u32,
                                t: tgt,
                                w: 1.0 - smoothstep(0.0, bd, d),
                                core: false,
                            });
                        }
                    }
                }
            }
            out.push(Stamp {
                id,
                owner: r.id,
                prio: 1,
                aabb: (i0, j0, i1, j1),
                entries,
            });
            id += 1;
        }
    }
    out
}

/// Composite height at a world point on the current terrain (render triangulation, LOD0).
pub fn ground(t: &Terrain, p: Vec2) -> f32 {
    terrain::interp(&t.h, N, SP, p)
}
