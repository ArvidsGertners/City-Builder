//! Frontage-strip subdivision (ALGORITHMS §10, ADR-0014) with the spike's additions:
//! medial-axis seams on concave arcs tighter than the depth, constrained re-spacing for edits.

use cb_core::rng::Pcg32;
use glam::DVec2;

use crate::frontage::{Frontage, ZoneParams};
use crate::geom::{self, Poly};

#[derive(Clone, Copy, Debug)]
pub struct Params {
    /// Blend toward the PCA depth axis (ALGORITHMS §10 step 2: 0.3).
    pub beta: f64,
    /// Rectangularity threshold for repair merges.
    pub rect_min: f64,
}

#[derive(Clone, Debug)]
pub struct Hint {
    /// World positions of the previous split feet.
    pub split_pts: Vec<DVec2>,
    /// Desired lot count.
    pub n: usize,
    /// Splits that the previous repair removed (merges are re-applied first: hysteresis).
    pub merged_pts: Vec<DVec2>,
}

#[derive(Clone, Debug)]
pub struct Lot {
    pub poly: Poly,
    /// Raw slice pieces (one integer arrangement): merges union these, never derived polygons.
    pub pieces: Vec<Poly>,
    pub s0: f64,
    pub s1: f64,
    pub frontage_len: f64,
    pub area: f64,
    pub rect: f64,
    pub corner: bool,
    pub dir0: DVec2,
    pub dir1: DVec2,
}

impl Lot {
    pub fn fan_deg(&self) -> f64 {
        geom::angle_between(self.dir0, self.dir1).to_degrees()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub lots_initial: usize,
    pub rect_fail_initial: usize,
    pub merged: usize,
    pub removed_splits: usize,
    pub decross_rounds: usize,
    pub orphans: usize,
    pub dropped_area: f64,
    pub multi_contact: usize,
    pub respace_fallback: usize,
    pub unmergeable: usize,
    pub rejected_edit: bool,
}

#[derive(Clone, Debug)]
pub struct Output {
    pub lots: Vec<Lot>,
    /// Pre-repair split feet (s, world point) — the hint for the next edit; intervals = len + 1.
    pub splits: Vec<(f64, DVec2)>,
    pub cuts: Vec<[DVec2; 2]>,
    /// Splits removed by repair merges (world points) — persisted with the hint.
    pub merged_pts: Vec<DVec2>,
    pub seams: Vec<[DVec2; 2]>,
    pub stats: Stats,
}

struct Ray {
    s: f64,
    p0: DVec2,
    dir: DVec2,
    p1: DVec2,
    cap: f64,
}

/// Lot-count range that keeps every width inside [w_min, w_max].
fn n_range(len: f64, zp: &ZoneParams) -> (usize, usize) {
    let lo = (len / zp.w_max).ceil().max(1.0) as usize;
    let hi = (len / zp.w_min).floor().max(1.0) as usize;
    (lo.min(hi), hi)
}

/// Jittered widths (step 1), rescaled to `len` and clamped with redistribution.
fn jittered(len: f64, n: usize, zp: &ZoneParams, rng: &mut Pcg32) -> Vec<f64> {
    let mut w: Vec<f64> = (0..n)
        .map(|_| zp.w_target * (1.0 + (rng.next_f64() - 0.5) * 0.5))
        .collect();
    let mut fixed = vec![false; n];
    for _ in 0..12 {
        let fixed_sum: f64 = w
            .iter()
            .zip(&fixed)
            .filter(|(_, f)| **f)
            .map(|(v, _)| *v)
            .sum();
        let free_sum: f64 = w
            .iter()
            .zip(&fixed)
            .filter(|(_, f)| !**f)
            .map(|(v, _)| *v)
            .sum();
        if free_sum <= 0.0 {
            break;
        }
        let k = (len - fixed_sum) / free_sum;
        let mut again = false;
        for i in 0..n {
            if fixed[i] {
                continue;
            }
            w[i] *= k;
            if w[i] < zp.w_min {
                w[i] = zp.w_min;
                fixed[i] = true;
                again = true;
            } else if w[i] > zp.w_max {
                w[i] = zp.w_max;
                fixed[i] = true;
                again = true;
            }
        }
        if !again {
            break;
        }
    }
    // Exact closure on the last lot (rounding).
    let sum: f64 = w.iter().sum();
    if let Some(l) = w.last_mut() {
        *l += len - sum;
    }
    w
}

fn push_widths(out: &mut Vec<f64>, start: f64, w: &[f64]) {
    let mut acc = start;
    for v in &w[..w.len().saturating_sub(1)] {
        acc += v;
        out.push(acc);
    }
}

/// Maximum fan (turn across one lot's frontage) before the lot is narrowed.
const FAN_MAX: f64 = 25.0 * std::f64::consts::PI / 180.0;

fn is_corner_lot(f: &Frontage, a: f64, b: f64) -> bool {
    f.seams
        .iter()
        .any(|c| c.is_corner && a <= 0.5 * (c.s0 + c.s1) && b >= 0.5 * (c.s0 + c.s1))
}

/// Width bounds for the lot [a, b]: zone range, narrowed on curves so the fan stays ≤ 25°
/// (never below w_min + 2 m so tight curves stay feasible); corner lots: ≥ arc + w_min.
fn lot_bounds(f: &Frontage, zp: &ZoneParams, a: f64, b: f64, slack: f64) -> (f64, f64) {
    for c in &f.seams {
        if c.is_corner && a <= 0.5 * (c.s0 + c.s1) && b >= 0.5 * (c.s0 + c.s1) {
            return ((c.s1 - c.s0) + zp.w_min, f64::INFINITY);
        }
    }
    let w = b - a;
    let turn = f.turn(a, b);
    let hi = if turn > FAN_MAX * slack && w > 0.0 {
        (w * FAN_MAX * slack / turn)
            .max(zp.w_min + 2.0)
            .min(zp.w_max * slack)
    } else {
        zp.w_max * slack
    };
    (zp.w_min, hi)
}

/// Gauss–Seidel projection of split positions onto the width bounds with pinned splits.
/// `x` includes the fixed ends 0 and L.
fn solve(f: &Frontage, zp: &ZoneParams, x: &mut [f64], pinned: &[bool], slack: f64) -> bool {
    let m = x.len() - 1;
    for pass in 0..400 {
        let mut worst: f64 = 0.0;
        let order: Vec<usize> = if pass % 2 == 0 {
            (0..m).collect()
        } else {
            (0..m).rev().collect()
        };
        for i in order {
            let w = x[i + 1] - x[i];
            let (lo, hi) = lot_bounds(f, zp, x[i], x[i + 1], slack);
            let v = if w < lo {
                lo - w
            } else if w > hi {
                hi - w
            } else {
                0.0
            };
            if v == 0.0 {
                continue;
            }
            worst = worst.max(v.abs());
            let left_free = i > 0 && !pinned[i];
            let right_free = i + 1 < m && !pinned[i + 1];
            match (left_free, right_free) {
                (true, true) => {
                    x[i] -= v * 0.5;
                    x[i + 1] += v * 0.5;
                }
                (true, false) => x[i] -= v,
                (false, true) => x[i + 1] += v,
                (false, false) => {}
            }
        }
        if worst < 1e-6 {
            return x.windows(2).all(|w| w[1] > w[0]);
        }
    }
    false
}

/// Pins a split at each kink: moves the nearest split there or inserts one.
fn pin_kinks(f: &Frontage, x: &mut Vec<f64>, pinned: &mut Vec<bool>) {
    for &k in &f.kinks {
        let i = x.partition_point(|v| *v < k);
        if i == 0 || i >= x.len() {
            continue;
        }
        let (a, b) = (x[i - 1], x[i]);
        if is_corner_lot(f, a, b) {
            continue;
        }
        let w = b - a;
        let near = if k - a < b - k { i - 1 } else { i };
        if near > 0 && near + 1 < x.len() && (x[near] - k).abs() < 0.35 * w && !pinned[near] {
            x[near] = k;
            pinned[near] = true;
        } else {
            x.insert(i, k);
            pinned.insert(i, true);
        }
    }
}

/// Inserts a split in the widest non-corner, non-pinned-adjacent lot.
fn add_split(f: &Frontage, x: &mut Vec<f64>, pinned: &mut Vec<bool>) -> bool {
    let best = (0..x.len() - 1)
        .filter(|&i| !is_corner_lot(f, x[i], x[i + 1]))
        .max_by(|&a, &b| (x[a + 1] - x[a]).total_cmp(&(x[b + 1] - x[b])));
    match best {
        Some(i) => {
            x.insert(i + 1, 0.5 * (x[i] + x[i + 1]));
            pinned.insert(i + 1, false);
            true
        }
        None => false,
    }
}

fn finish(
    f: &Frontage,
    zp: &ZoneParams,
    mut x: Vec<f64>,
    mut pinned: Vec<bool>,
    slack: f64,
    grow: bool,
    stats: &mut Stats,
) -> Option<Vec<f64>> {
    pin_kinks(f, &mut x, &mut pinned);
    for _ in 0..4 {
        let mut t = x.clone();
        if solve(f, zp, &mut t, &pinned, slack) {
            return Some(t[1..t.len() - 1].to_vec());
        }
        let lo_sum: f64 = (0..x.len() - 1)
            .map(|i| lot_bounds(f, zp, x[i], x[i + 1], slack).0)
            .sum();
        if !grow || lo_sum > f.len() || !add_split(f, &mut x, &mut pinned) {
            break;
        }
    }
    stats.respace_fallback += 1;
    None
}

/// Fresh split positions (interior, ascending).
fn initial_splits(
    f: &Frontage,
    zp: &ZoneParams,
    dn: i32,
    seed: u64,
    stats: &mut Stats,
) -> Vec<f64> {
    let len = f.len();
    let mut rng = Pcg32::from_seed(seed);
    let corner = f.seams.iter().find(|c| c.is_corner);
    let mut out = Vec::new();
    let mut pins: Vec<f64> = Vec::new();
    match corner {
        None => {
            let (lo, hi) = n_range(len, zp);
            let auto = ((len / zp.w_target).round() as i64).clamp(lo as i64, hi as i64);
            let n = (auto + i64::from(dn)).clamp(lo as i64, hi as i64) as usize;
            let w = jittered(len, n, zp, &mut rng);
            push_widths(&mut out, 0.0, &w);
        }
        Some(c) => {
            let e = 0.65 * zp.w_target;
            let mut a_end = (c.s0 - e).max(0.0);
            let mut b_start = (c.s1 + e).min(len);
            if a_end < zp.w_min {
                a_end = 0.0;
            }
            if len - b_start < zp.w_min {
                b_start = len;
            }
            let arms = [(0.0, a_end), (b_start, len)];
            let mut n_arm = [0usize; 2];
            let mut range = [(0usize, 0usize); 2];
            for k in 0..2 {
                let l = arms[k].1 - arms[k].0;
                if l >= zp.w_min {
                    range[k] = n_range(l, zp);
                    n_arm[k] = ((l / zp.w_target).round() as usize).clamp(range[k].0, range[k].1);
                }
            }
            // Distribute dn: one at a time to the arm with the most slack.
            for _ in 0..dn.unsigned_abs() {
                let pick = (0..2)
                    .filter(|&k| range[k].1 > 0)
                    .filter(|&k| {
                        if dn > 0 {
                            n_arm[k] < range[k].1
                        } else {
                            n_arm[k] > range[k].0
                        }
                    })
                    .max_by(|&a, &b| {
                        let wa = (arms[a].1 - arms[a].0) / n_arm[a].max(1) as f64;
                        let wb = (arms[b].1 - arms[b].0) / n_arm[b].max(1) as f64;
                        if dn > 0 {
                            wa.total_cmp(&wb)
                        } else {
                            wb.total_cmp(&wa)
                        }
                    });
                if let Some(k) = pick {
                    if dn > 0 { n_arm[k] += 1 } else { n_arm[k] -= 1 }
                }
            }
            if n_arm[0] > 0 {
                let w = jittered(a_end, n_arm[0], zp, &mut rng);
                push_widths(&mut out, 0.0, &w);
                out.push(a_end);
                pins.push(a_end);
            }
            if n_arm[1] > 0 {
                out.push(b_start);
                pins.push(b_start);
                let w = jittered(len - b_start, n_arm[1], zp, &mut rng);
                push_widths(&mut out, b_start, &w);
            }
        }
    }
    let mut x = vec![0.0];
    x.extend(&out);
    x.push(len);
    let pinned: Vec<bool> = x
        .iter()
        .enumerate()
        .map(|(i, v)| i == 0 || i + 1 == x.len() || pins.contains(v))
        .collect();
    finish(f, zp, x, pinned, 1.0, true, stats).unwrap_or(out)
}

/// Number of old intervals that keep an ID (overlap / longer ≥ 0.6, greedy) — edit scoring.
fn kept_count(old: &[f64], new: &[f64]) -> usize {
    let mut cand: Vec<(f64, usize, usize)> = Vec::new();
    for j in 0..new.len() - 1 {
        for i in 0..old.len() - 1 {
            let ov = (new[j + 1].min(old[i + 1]) - new[j].max(old[i])).max(0.0);
            let r = ov / (new[j + 1] - new[j]).max(old[i + 1] - old[i]).max(1e-9);
            if r >= 0.6 {
                cand.push((r, j, i));
            }
        }
    }
    cand.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut un = vec![false; new.len()];
    let mut uo = vec![false; old.len()];
    let mut k = 0;
    for (_, j, i) in cand {
        if !un[j] && !uo[i] {
            un[j] = true;
            uo[i] = true;
            k += 1;
        }
    }
    k
}

/// Edit path: previous splits projected onto the new frontage. Same count → smallest local
/// moves that restore the bounds. ±1 lot → every insert/remove position is tried and the one
/// keeping the most lot IDs wins. Infeasible counts are rejected (the ± button is disabled).
fn hinted_splits(
    f: &Frontage,
    zp: &ZoneParams,
    hint: &Hint,
    stats: &mut Stats,
) -> Option<Vec<f64>> {
    let len = f.len();
    let mut s: Vec<f64> = hint
        .split_pts
        .iter()
        .map(|p| f.project(*p).0)
        .filter(|v| *v > 0.5 && *v < len - 0.5)
        .collect();
    s.sort_by(f64::total_cmp);
    s.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    let kink_split = |v: f64| f.kinks.iter().any(|k| (k - v).abs() < 0.01);
    let frame = |s: &[f64]| -> Vec<f64> {
        let mut x = vec![0.0];
        x.extend(s);
        x.push(len);
        x
    };
    let pins_for = |x: &[f64]| -> Vec<bool> {
        (0..x.len())
            .map(|i| {
                i == 0
                    || i + 1 == x.len()
                    || (i > 0 && is_corner_lot(f, x[i - 1], x[i]))
                    || (i + 1 < x.len() && is_corner_lot(f, x[i], x[i + 1]))
            })
            .collect()
    };
    let old_x = frame(&s);
    let n_now = s.len() + 1;
    const EDIT_SLACK: f64 = 1.1;
    let inserts = |s: &[f64]| -> Vec<Vec<f64>> {
        let x = frame(s);
        (0..x.len() - 1)
            .filter(|&i| !is_corner_lot(f, x[i], x[i + 1]))
            .map(|i| {
                let mut c = s.to_vec();
                c.insert(i, 0.5 * (x[i] + x[i + 1]));
                c
            })
            .collect()
    };
    let removals = |s: &[f64]| -> Vec<Vec<f64>> {
        let x = frame(s);
        (0..s.len())
            .filter(|&k| {
                !is_corner_lot(f, x[k], x[k + 1])
                    && !is_corner_lot(f, x[k + 1], x[k + 2])
                    && !kink_split(s[k])
            })
            .map(|k| {
                let mut c = s.to_vec();
                c.remove(k);
                c
            })
            .collect()
    };
    // Dissolve lot k: its two splits collapse to their midpoint (half to each neighbour).
    let dissolves = |s: &[f64]| -> Vec<Vec<f64>> {
        let x = frame(s);
        (1..s.len())
            .filter(|&k| {
                !is_corner_lot(f, x[k], x[k + 1]) && !kink_split(s[k - 1]) && !kink_split(s[k])
            })
            .map(|k| {
                let mut c = s.to_vec();
                let mid = 0.5 * (c[k - 1] + c[k]);
                c.remove(k);
                c[k - 1] = mid;
                c
            })
            .collect()
    };
    let try_all = |cands: Vec<Vec<f64>>| -> Option<(usize, Vec<f64>)> {
        let mut best: Option<(usize, Vec<f64>)> = None;
        let mut scratch = Stats::default();
        for c in cands {
            let x = frame(&c);
            let pinned = pins_for(&x);
            if let Some(r) = finish(f, zp, x, pinned, EDIT_SLACK, false, &mut scratch) {
                let score = kept_count(&old_x, &frame(&r));
                if best.as_ref().is_none_or(|b| score > b.0) {
                    best = Some((score, r));
                }
            }
        }
        best
    };
    let result = if hint.n == n_now + 1 {
        try_all(inserts(&s))
    } else if hint.n + 1 == n_now {
        let mut c = removals(&s);
        c.extend(dissolves(&s));
        try_all(c)
    } else {
        // Same count (drag / depth edit); if it no longer fits, the best ±1 change.
        try_all(vec![s.clone()])
            .or_else(|| {
                let mut c = removals(&s);
                c.extend(dissolves(&s));
                try_all(c)
            })
            .or_else(|| try_all(inserts(&s)))
    };
    match result {
        Some((_, r)) => Some(r),
        None => {
            if hint.n != n_now {
                stats.rejected_edit = true;
                let x = frame(&s);
                let pinned = pins_for(&x);
                return finish(f, zp, x, pinned, EDIT_SLACK, true, stats);
            }
            stats.respace_fallback += 1;
            None
        }
    }
}

fn trace(p0: DVec2, dir: DVec2, cap: f64, plot: &Poly, seams: &[[DVec2; 2]]) -> DVec2 {
    let far = p0 + dir * cap;
    let mut tmin = 1.0;
    let n = plot.len();
    let eps = 0.05 / cap;
    for i in 0..n {
        if let Some((t, _)) = geom::seg_intersect(p0, far, plot[i], plot[(i + 1) % n])
            && t > eps
            && t < tmin
        {
            tmin = t;
        }
    }
    for sm in seams {
        if let Some((t, _)) = geom::seg_intersect(p0, far, sm[0], sm[1])
            && t > eps
            && t < tmin
        {
            tmin = t;
        }
    }
    p0 + dir * cap * tmin
}

fn crosses(a: &Ray, b: &Ray) -> bool {
    match geom::seg_intersect(a.p0, a.p1, b.p0, b.p1) {
        Some((t, u)) => t > 1e-6 && t < 1.0 - 1e-6 && u > 1e-6 && u < 1.0 - 1e-6,
        None => false,
    }
}

/// PCA axis of the plot boundary closest to `n` (signed toward it).
fn pca_depth_axis(plot: &Poly, n: DVec2) -> DVec2 {
    let m = plot.len();
    let mut c = DVec2::ZERO;
    let mut wsum = 0.0;
    for i in 0..m {
        let (a, b) = (plot[i], plot[(i + 1) % m]);
        let l = a.distance(b);
        c += (a + b) * 0.5 * l;
        wsum += l;
    }
    c /= wsum.max(1e-9);
    let (mut sxx, mut sxy, mut syy) = (0.0, 0.0, 0.0);
    for i in 0..m {
        let (a, b) = (plot[i], plot[(i + 1) % m]);
        let l = a.distance(b);
        let d = (a + b) * 0.5 - c;
        sxx += d.x * d.x * l;
        sxy += d.x * d.y * l;
        syy += d.y * d.y * l;
    }
    let ang = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    let e1 = DVec2::new(ang.cos(), ang.sin());
    let e2 = geom::perp(e1);
    let pick = if e1.dot(n).abs() >= e2.dot(n).abs() {
        e1
    } else {
        e2
    };
    if pick.dot(n) < 0.0 { -pick } else { pick }
}

pub fn subdivide(
    f: &Frontage,
    plot: &Poly,
    zp: &ZoneParams,
    depth: f64,
    dn: i32,
    seed: u64,
    hint: Option<&Hint>,
    prm: &Params,
) -> Output {
    let t_all = std::time::Instant::now();
    let mut stats = Stats::default();
    let len = f.len();
    let split_s = match hint.and_then(|h| hinted_splits(f, zp, h, &mut stats)) {
        Some(s) => s,
        None => initial_splits(f, zp, dn, seed, &mut stats),
    };

    // Seams (medial axis of concave arcs tighter than the depth).
    let mut seams: Vec<[DVec2; 2]> = Vec::new();
    for c in &f.seams {
        if geom::point_in_poly(c.center, plot) {
            let end = trace(c.center, c.dir, 4.0 * depth + 100.0, plot, &[]);
            seams.push([c.center, end]);
        }
    }

    // Split rays (step 2).
    let axis = pca_depth_axis(plot, f.mean_normal(0.0, len));
    let mut bounds = vec![0.0];
    bounds.extend(&split_s);
    bounds.push(len);
    let mut rays: Vec<Ray> = Vec::with_capacity(split_s.len());
    for (k, &s) in split_s.iter().enumerate() {
        let h = 0.5 * (bounds[k + 1] - bounds[k]).min(bounds[k + 2] - bounds[k + 1]);
        let on_seam = f.seams.iter().find(|c| s > c.s0 && s < c.s1);
        let (dir, cap) = match on_seam {
            // Exactly at the arc centre so the cut joins the seam start.
            Some(c) => (
                (c.center - f.point_at(s)).normalize_or(f.normal_at(s)),
                c.center.distance(f.point_at(s)),
            ),
            None => {
                let n = f.mean_normal(s - h, s + h);
                (
                    ((1.0 - prm.beta) * n + prm.beta * axis).normalize_or(n),
                    4.0 * depth + 100.0,
                )
            }
        };
        let p0 = f.point_at(s);
        let p1 = trace(p0, dir, cap, plot, &seams);
        rays.push(Ray {
            s,
            p0,
            dir,
            p1,
            cap,
        });
    }

    // De-crossing (8 × 5°, else drop the split).
    let step = 5f64.to_radians();
    loop {
        let mut any = false;
        for round in 0..8 {
            let mut crossing = false;
            for k in 0..rays.len().saturating_sub(1) {
                let far = (k + 2).min(rays.len() - 1);
                for j in k + 1..=far {
                    if crosses(&rays[k], &rays[j]) {
                        crossing = true;
                        let b = (rays[k].dir + rays[j].dir).normalize_or(rays[k].dir);
                        for idx in [k, j] {
                            let d = rays[idx].dir;
                            let a = geom::cross(d, b).atan2(d.dot(b));
                            let rot = a.clamp(-step, step);
                            rays[idx].dir = geom::rotate(d, rot);
                            rays[idx].p1 =
                                trace(rays[idx].p0, rays[idx].dir, rays[idx].cap, plot, &seams);
                        }
                    }
                }
            }
            if !crossing {
                break;
            }
            stats.decross_rounds = stats.decross_rounds.max(round + 1);
            any = true;
        }
        let bad = (0..rays.len().saturating_sub(1)).find(|&k| crosses(&rays[k], &rays[k + 1]));
        match bad {
            Some(k) => {
                rays.remove(k + 1);
                stats.removed_splits += 1;
            }
            None => break,
        }
        if !any {
            break;
        }
    }

    geom::prof(0, t_all);
    let t_slice = std::time::Instant::now();
    // Cut (step 3): slice the plot by rays and seams.
    // A ray that ends at a seam start (split on a fillet) continues as that seam: one string.
    let mut seam_used = vec![false; seams.len()];
    let mut lines: Vec<Vec<DVec2>> = Vec::with_capacity(rays.len() + seams.len());
    for r in &rays {
        let joined = seams.iter().position(|sm| sm[0].distance(r.p1) < 1e-6);
        match joined {
            Some(k) => {
                seam_used[k] = true;
                let d = (seams[k][1] - seams[k][0]).normalize_or_zero();
                lines.push(vec![r.p0 - r.dir * 0.02, r.p1, seams[k][1] + d * 0.02]);
            }
            None => lines.push(vec![r.p0 - r.dir * 0.02, r.p1 + r.dir * 0.02]),
        }
    }
    for (k, sm) in seams.iter().enumerate() {
        if !seam_used[k] {
            let d = (sm[1] - sm[0]).normalize_or_zero();
            lines.push(vec![sm[0], sm[1] + d * 0.02]);
        }
    }
    let (pieces, slice_holes) = geom::slice(plot, &lines);
    if slice_holes > 0 && std::env::var("SP_TRACE").is_ok() {
        eprintln!("slice produced {slice_holes} holes");
    }

    geom::prof(1, t_slice);
    let t_assign = std::time::Instant::now();
    let split_s: Vec<f64> = rays.iter().map(|r| r.s).collect();
    let n_int = split_s.len() + 1;
    let interval_of = |s: f64| split_s.partition_point(|v| *v < s);

    // Frontage contact per piece.
    let fidx = crate::frontage::FrontageIndex::new(f);
    struct Piece {
        poly: Poly,
        contact: Vec<(usize, f64)>,
    }
    let mut ps: Vec<Piece> = pieces
        .into_iter()
        .map(|poly| {
            let mut contact: Vec<(usize, f64)> = Vec::new();
            let n = poly.len();
            for i in 0..n {
                let (a, b) = (poly[i], poly[(i + 1) % n]);
                let m = (a + b) * 0.5;
                let on = |q: DVec2| fidx.near(f, q).filter(|v| v.1 < 0.004);
                if let Some((sm, _)) = on(m)
                    && on(a).is_some()
                    && on(b).is_some()
                {
                    let idx = interval_of(sm);
                    let l = a.distance(b);
                    match contact.iter_mut().find(|c| c.0 == idx) {
                        Some(c) => c.1 += l,
                        None => contact.push((idx, l)),
                    }
                }
            }
            Piece { poly, contact }
        })
        .collect();

    // Union-find over intervals for multi-contact pieces.
    let mut parent: Vec<usize> = (0..n_int).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while p[r] != r {
            r = p[r];
        }
        p[i] = r;
        r
    }
    for p in &ps {
        let sig: Vec<usize> = p
            .contact
            .iter()
            .filter(|c| c.1 > 0.5)
            .map(|c| c.0)
            .collect();
        if sig.len() > 1 {
            stats.multi_contact += sig.len() - 1;
            let lo = *sig.iter().min().unwrap_or(&0);
            let hi = *sig.iter().max().unwrap_or(&0);
            for i in lo..hi {
                let (a, b) = (find(&mut parent, i), find(&mut parent, i + 1));
                parent[b.max(a)] = a.min(b);
            }
        }
    }

    // Orphans → neighbour sharing the longest edge.
    let shared_len = |a: &Poly, b: &Poly| -> f64 {
        let mut acc = 0.0;
        let (na, nb) = (a.len(), b.len());
        for i in 0..na {
            let (p, q) = (a[i], a[(i + 1) % na]);
            let d = (q - p).normalize_or_zero();
            for j in 0..nb {
                let (r, s) = (b[j], b[(j + 1) % nb]);
                if geom::dist_point_seg(r, p, q)
                    .0
                    .min(geom::dist_point_seg(p, r, s).0)
                    > 0.004
                {
                    continue;
                }
                if geom::cross(d, r - p).abs() < 0.004 && geom::cross(d, s - p).abs() < 0.004 {
                    let (t0, t1) = ((r - p).dot(d), (s - p).dot(d));
                    let (lo, hi) = (t0.min(t1).max(0.0), t0.max(t1).min(p.distance(q)));
                    if hi > lo {
                        acc += hi - lo;
                    }
                }
            }
        }
        acc
    };
    let mut owner: Vec<Option<usize>> = ps
        .iter()
        .map(|p| {
            p.contact
                .iter()
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .filter(|c| c.1 > 0.01)
                .map(|c| c.0)
        })
        .collect();
    for _ in 0..4 {
        for i in 0..ps.len() {
            if owner[i].is_some() {
                continue;
            }
            stats.orphans += 1;
            let a = geom::area(&ps[i].poly);
            if a < 5.0 {
                stats.dropped_area += a;
                owner[i] = Some(usize::MAX);
                continue;
            }
            let best = (0..ps.len())
                .filter(|&j| j != i && owner[j].is_some_and(|o| o != usize::MAX))
                .map(|j| (j, shared_len(&ps[i].poly, &ps[j].poly)))
                .max_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((j, l)) = best
                && l > 0.01
            {
                owner[i] = owner[j];
            }
        }
    }
    for (i, o) in owner.iter().enumerate() {
        if o.is_none() {
            stats.dropped_area += geom::area(&ps[i].poly);
        }
    }

    // Group pieces into lots.
    let mut groups: Vec<Vec<usize>> = vec![Vec::new(); n_int];
    for (i, o) in owner.iter().enumerate() {
        if let Some(o) = o
            && *o != usize::MAX
        {
            let r = find(&mut parent, *o);
            groups[r].push(i);
        }
    }
    let ray_dir = |k: isize| -> DVec2 {
        if k < 0 {
            f.normal_at(0.0)
        } else if k as usize >= rays.len() {
            f.normal_at(len)
        } else {
            rays[k as usize].dir
        }
    };
    let mut bnd = vec![0.0];
    bnd.extend(&split_s);
    bnd.push(len);
    let mut lots: Vec<Lot> = Vec::new();
    let mut i = 0;
    while i < n_int {
        let r = find(&mut parent, i);
        let mut j = i;
        while j + 1 < n_int && find(&mut parent, j + 1) == r {
            j += 1;
        }
        let members = &groups[r];
        if !members.is_empty() {
            let polys: Vec<Poly> = members.iter().map(|&m| ps[m].poly.clone()).collect();
            let poly = if polys.len() == 1 {
                polys[0].clone()
            } else {
                geom::union_all(&polys)
                    .0
                    .into_iter()
                    .max_by(|a, b| geom::area(a).total_cmp(&geom::area(b)))
                    .unwrap_or_default()
            };
            let fl: f64 = members
                .iter()
                .flat_map(|&m| ps[m].contact.iter().map(|c| c.1))
                .sum();
            let corner = f.seams.iter().any(|c| {
                c.is_corner && bnd[i] <= 0.5 * (c.s0 + c.s1) && bnd[j + 1] >= 0.5 * (c.s0 + c.s1)
            });
            lots.push(Lot {
                area: geom::area(&poly),
                rect: geom::rectangularity(&poly),
                poly,
                pieces: polys,
                s0: bnd[i],
                s1: bnd[j + 1],
                frontage_len: fl,
                corner,
                dir0: ray_dir(i as isize - 1),
                dir1: ray_dir(j as isize),
            });
        }
        i = j + 1;
    }

    geom::prof(2, t_assign);
    let t_rep = std::time::Instant::now();
    // Repair / validate (step 5).
    stats.lots_initial = lots.len();
    stats.rect_fail_initial = lots.iter().filter(|l| l.rect < prm.rect_min).count();
    let mut merged_pts: Vec<DVec2> = Vec::new();
    if let Some(h) = hint {
        let old: Vec<f64> = h.merged_pts.iter().map(|p| f.project(*p).0).collect();
        let mut k = 0;
        while k + 1 < lots.len() {
            let b = lots[k].s1;
            if old.iter().any(|o| (o - b).abs() < 0.05) {
                let mut pcs = lots[k].pieces.clone();
                pcs.extend(lots[k + 1].pieces.iter().cloned());
                let u = geom::union_all(&pcs).0;
                if u.len() == 1 {
                    let poly = u.into_iter().next().unwrap_or_default();
                    merged_pts.push(f.point_at(b));
                    let hi = lots.remove(k + 1);
                    let lo = &mut lots[k];
                    lo.pieces = pcs;
                    lo.area = geom::area(&poly);
                    lo.rect = geom::rectangularity(&poly);
                    lo.poly = poly;
                    lo.s1 = hi.s1;
                    lo.frontage_len += hi.frontage_len;
                    lo.corner |= hi.corner;
                    lo.dir1 = hi.dir1;
                    continue;
                }
            }
            k += 1;
        }
    }
    // Edit hysteresis: existing lots get a margin before a new repair merge.
    let (rect_min, a_min) = if hint.is_some() {
        (prm.rect_min - 0.05, zp.a_min * 0.9)
    } else {
        (prm.rect_min, zp.a_min)
    };
    let mut accepted = vec![false; lots.len()];
    loop {
        if lots.len() < 2 {
            break;
        }
        let fail = (0..lots.len())
            .filter(|&k| {
                let l = &lots[k];
                !accepted[k]
                    && (l.rect < rect_min || l.area < a_min || l.frontage_len < zp.w_min - 0.01)
            })
            .min_by(|&a, &b| lots[a].rect.total_cmp(&lots[b].rect));
        let Some(k) = fail else { break };
        // Neighbours in frontage order, narrower first; must union into one polygon.
        let mut nbs: Vec<usize> = [k.checked_sub(1), (k + 1 < lots.len()).then_some(k + 1)]
            .into_iter()
            .flatten()
            .collect();
        nbs.sort_by(|&a, &b| lots[a].frontage_len.total_cmp(&lots[b].frontage_len));
        let mut done = false;
        for nb in nbs {
            let (lo, hi) = (k.min(nb), k.max(nb));
            let mut pcs = lots[lo].pieces.clone();
            pcs.extend(lots[hi].pieces.iter().cloned());
            let u = geom::union_all(&pcs).0;
            if u.len() != 1 {
                continue;
            }
            let poly = u.into_iter().next().unwrap_or_default();
            let merged = Lot {
                area: geom::area(&poly),
                rect: geom::rectangularity(&poly),
                poly,
                pieces: pcs,
                s0: lots[lo].s0,
                s1: lots[hi].s1,
                frontage_len: lots[lo].frontage_len + lots[hi].frontage_len,
                corner: lots[lo].corner || lots[hi].corner,
                dir0: lots[lo].dir0,
                dir1: lots[hi].dir1,
            };
            merged_pts.push(f.point_at(lots[lo].s1));
            lots[lo] = merged;
            lots.remove(hi);
            accepted.remove(hi);
            stats.merged += 1;
            done = true;
            break;
        }
        if !done {
            accepted[k] = true;
            stats.unmergeable += 1;
        }
    }

    geom::prof(3, t_rep);
    // Persistent state = the pre-repair split list (repair is a deterministic function of it).
    let splits: Vec<(f64, DVec2)> = rays.iter().map(|r| (r.s, r.p0)).collect();
    let cuts = rays.iter().map(|r| [r.p0, r.p1]).collect();
    Output {
        lots,
        splits,
        cuts,
        seams,
        merged_pts,
        stats,
    }
}

/// Stable lot IDs (step 6): greedy frontage-interval overlap ≥ `thr` (overlap / longer).
/// Old intervals are given as world points of their frontage ends; returns new→old index.
pub fn match_ids(
    f_new: &Frontage,
    old: &[(DVec2, DVec2)],
    new: &[Lot],
    thr: f64,
) -> Vec<Option<usize>> {
    let old_s: Vec<(f64, f64)> = old
        .iter()
        .map(|(a, b)| (f_new.project(*a).0, f_new.project(*b).0))
        .collect();
    let mut cand: Vec<(f64, usize, usize)> = Vec::new();
    for (j, l) in new.iter().enumerate() {
        for (i, o) in old_s.iter().enumerate() {
            let ov = (l.s1.min(o.1) - l.s0.max(o.0)).max(0.0);
            let r = ov / (l.s1 - l.s0).max(o.1 - o.0).max(1e-9);
            if r >= thr {
                cand.push((r, j, i));
            }
        }
    }
    cand.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut out = vec![None; new.len()];
    let mut used = vec![false; old.len()];
    for (_, j, i) in cand {
        if out[j].is_none() && !used[i] {
            out[j] = Some(i);
            used[i] = true;
        }
    }
    out
}
