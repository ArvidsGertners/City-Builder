//! Frontage-strip lot subdivision (ADR-0064, ALGORITHMS §10): split positions from jittered
//! widths and a constrained width solver, rays along the mean inward normal, medial-axis seams
//! on concave arcs tighter than the depth, slicing with i_overlay, piece assignment by frontage
//! contact, repair merges, and stable lot IDs by frontage-interval overlap.
//!
//! Persisted state of a plot is the **pre-repair split list** plus the splits its repair removed
//! ([`Hint`]); lots are always derived. All functions are deterministic (`f64`, libm trig, no
//! hashing of floats).

use cb_core::rng::Pcg32;
use glam::DVec2;

use crate::boolean::f64ops;
use crate::d2::{self, Poly};
use crate::frontage::{Frontage, FrontageIndex};

/// Rectangularity below which a fresh lot is repaired by merging.
pub const RECT_MIN: f64 = 0.55;
/// Rectangularity threshold for existing lots during edits (hysteresis).
pub const RECT_MIN_EDIT: f64 = 0.50;
/// Area factor on `a_min` for existing lots during edits (hysteresis).
pub const AREA_FACTOR_EDIT: f64 = 0.9;
/// Slack on `w_max` and the fan cap during edits.
pub const EDIT_SLACK: f64 = 1.1;
/// Maximum frontage turn per lot before the lot is narrowed (rad, 25°).
pub const FAN_MAX_RAD: f64 = 25.0 * std::f64::consts::PI / 180.0;
/// Corner lot extends this fraction of `w_target` beyond the fillet on each side.
pub const CORNER_ARM_FACTOR: f64 = 0.65;
/// Lot-ID match threshold: frontage overlap / longer interval.
pub const ID_MATCH_MIN: f64 = 0.6;
/// Orphan pieces smaller than this are dropped (m²).
const ORPHAN_DROP_M2: f64 = 5.0;

/// Per-zone subdivision parameters (GAME_DESIGN §4; the catalog overrides the defaults).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoneParams {
    /// Minimum lot frontage (m).
    pub w_min: f64,
    /// Target lot frontage (m).
    pub w_target: f64,
    /// Maximum lot frontage (m).
    pub w_max: f64,
    /// Maximum plot depth (m).
    pub d_max: f64,
    /// Minimum lot area (m²).
    pub a_min: f64,
    /// Front, side and rear setbacks (m).
    pub setback: [f64; 3],
}

impl ZoneParams {
    /// Residential defaults.
    pub const RESIDENTIAL: Self = Self {
        w_min: 10.0,
        w_target: 13.0,
        w_max: 18.0,
        d_max: 45.0,
        a_min: 120.0,
        setback: [3.0, 1.5, 4.0],
    };
    /// Commercial defaults.
    pub const COMMERCIAL: Self = Self {
        w_min: 14.0,
        w_target: 20.0,
        w_max: 30.0,
        d_max: 50.0,
        a_min: 200.0,
        setback: [1.0, 0.0, 2.0],
    };
    /// Industrial defaults.
    pub const INDUSTRIAL: Self = Self {
        w_min: 24.0,
        w_target: 36.0,
        w_max: 60.0,
        d_max: 80.0,
        a_min: 600.0,
        setback: [4.0, 2.0, 4.0],
    };
}

/// Edit state carried from the previous subdivision of the same plot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hint {
    /// World positions of the previous pre-repair split feet.
    pub split_pts: Vec<DVec2>,
    /// Desired lot count (pre-repair intervals): unchanged, or ±1 for the sub-plot control.
    pub n: usize,
    /// Splits the previous repair removed (re-applied first: hysteresis).
    pub merged_pts: Vec<DVec2>,
}

/// One lot.
#[derive(Clone, Debug)]
pub struct Lot {
    /// Polygon (counter-clockwise).
    pub poly: Poly,
    /// Raw slice pieces: merges union these, never derived polygons.
    pub pieces: Vec<Poly>,
    /// Frontage interval start (m).
    pub s0: f64,
    /// Frontage interval end (m).
    pub s1: f64,
    /// Length of the lot boundary on the frontage (m).
    pub frontage_len: f64,
    /// Area (m²).
    pub area: f64,
    /// Area / minimum oriented bounding box area.
    pub rect: f64,
    /// Spans a road corner (exempt from the fan rule).
    pub corner: bool,
    /// Result of a repair merge (may exceed `w_max` and the fan cap).
    pub merged: bool,
    /// Direction of the side cut at `s0`.
    pub dir0: DVec2,
    /// Direction of the side cut at `s1`.
    pub dir1: DVec2,
}

impl Lot {
    /// Angle between the two side cuts (degrees).
    #[must_use]
    pub fn fan_deg(&self) -> f64 {
        d2::angle_between(self.dir0, self.dir1).to_degrees()
    }
}

/// Diagnostics of one subdivision.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    /// Lots before repair.
    pub lots_initial: usize,
    /// Lots below the rectangularity threshold before repair.
    pub rect_fail_initial: usize,
    /// Repair merges.
    pub merged: usize,
    /// Splits dropped because their rays still crossed after de-crossing.
    pub removed_splits: usize,
    /// De-crossing rounds used.
    pub decross_rounds: usize,
    /// Pieces without frontage contact.
    pub orphans: usize,
    /// Area dropped as tiny orphans (m²).
    pub dropped_area: f64,
    /// Extra frontage intervals touched by single pieces.
    pub multi_contact: usize,
    /// Width solver fell back to unconstrained widths.
    pub respace_fallback: usize,
    /// Failing lots that could not be merged.
    pub unmergeable: usize,
    /// A ±1 edit had no feasible position (the control should be disabled).
    pub rejected_edit: bool,
}

/// Result of [`subdivide`].
#[derive(Clone, Debug)]
pub struct Output {
    /// Lots in frontage order.
    pub lots: Vec<Lot>,
    /// Pre-repair split feet `(s, world point)`: the persisted state (intervals = len + 1).
    pub splits: Vec<(f64, DVec2)>,
    /// Side cuts (ray start and end).
    pub cuts: Vec<[DVec2; 2]>,
    /// Splits removed by repair merges (world points), persisted with the splits.
    pub merged_pts: Vec<DVec2>,
    /// Seams (start, end).
    pub seams: Vec<[DVec2; 2]>,
    /// Diagnostics.
    pub stats: Stats,
}

impl Output {
    /// The hint for the next edit of this plot with `n` desired lots.
    #[must_use]
    pub fn hint(&self, n: usize) -> Hint {
        Hint {
            split_pts: self.splits.iter().map(|s| s.1).collect(),
            n,
            merged_pts: self.merged_pts.clone(),
        }
    }

    /// Pre-repair interval count (the `n` of an unchanged edit).
    #[must_use]
    pub fn intervals(&self) -> usize {
        self.splits.len() + 1
    }
}

struct Ray {
    s: f64,
    p0: DVec2,
    dir: DVec2,
    p1: DVec2,
    cap: f64,
}

/// Lot-count range that keeps every width inside `[w_min, w_max]`.
#[must_use]
pub fn n_range(len: f64, zp: &ZoneParams) -> (usize, usize) {
    let lo = (len / zp.w_max).ceil().max(1.0) as usize;
    let hi = (len / zp.w_min).floor().max(1.0) as usize;
    (lo.min(hi), hi)
}

/// Jittered widths, rescaled to `len` and clamped with redistribution.
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

fn is_corner_lot(f: &Frontage, a: f64, b: f64) -> bool {
    f.seams()
        .iter()
        .any(|c| c.is_corner && a <= c.mid() && b >= c.mid())
}

/// Width bounds for the lot `[a, b]`: zone range, narrowed on curves so the fan stays ≤ 25°
/// (never below `w_min + 2 m`); corner lots: ≥ arc + `w_min`.
fn lot_bounds(f: &Frontage, zp: &ZoneParams, a: f64, b: f64, slack: f64) -> (f64, f64) {
    if let Some(c) = f
        .seams()
        .iter()
        .find(|c| c.is_corner && a <= c.mid() && b >= c.mid())
    {
        return ((c.s1 - c.s0) + zp.w_min, f64::INFINITY);
    }
    let w = b - a;
    let turn = f.turn(a, b);
    let hi = if turn > FAN_MAX_RAD * slack && w > 0.0 {
        (w * FAN_MAX_RAD * slack / turn)
            .max(zp.w_min + 2.0)
            .min(zp.w_max * slack)
    } else {
        zp.w_max * slack
    };
    (zp.w_min, hi)
}

/// Gauss–Seidel projection of split positions onto the width bounds; `x` includes the fixed
/// ends 0 and L.
fn solve(f: &Frontage, zp: &ZoneParams, x: &mut [f64], pinned: &[bool], slack: f64) -> bool {
    let m = x.len() - 1;
    for pass in 0..400 {
        let mut worst: f64 = 0.0;
        for k in 0..m {
            let i = if pass % 2 == 0 { k } else { m - 1 - k };
            let w = x[i + 1] - x[i];
            let (lo, hi) = lot_bounds(f, zp, x[i], x[i + 1], slack);
            let v = if w < lo {
                lo - w
            } else if w > hi {
                hi - w
            } else {
                continue;
            };
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
    for &k in f.kinks() {
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

/// Inserts a split in the widest non-corner lot.
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
    grow.then(|| densest(f, zp, &x, &pinned))
}

/// Fallback when no layout meets the fan cap (curves tighter than whole `w_min` lots allow):
/// equal lots of about `w_min` between pinned splits, the smallest fan achievable. Corner lots
/// stay whole.
fn densest(f: &Frontage, zp: &ZoneParams, x: &[f64], pinned: &[bool]) -> Vec<f64> {
    let pins: Vec<usize> = (0..x.len()).filter(|&i| pinned[i]).collect();
    let mut out = Vec::new();
    for w in pins.windows(2) {
        let (a, b) = (x[w[0]], x[w[1]]);
        let n = if w[1] == w[0] + 1 && is_corner_lot(f, a, b) {
            1
        } else {
            ((b - a) / zp.w_min).floor().max(1.0) as usize
        };
        out.extend((1..n).map(|k| a + (b - a) * k as f64 / n as f64));
        if w[1] + 1 < x.len() {
            out.push(b);
        }
    }
    out
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
    let mut out = Vec::new();
    let mut pins: Vec<f64> = Vec::new();
    match f.corner() {
        None => {
            let (lo, hi) = n_range(len, zp);
            let auto = ((len / zp.w_target).round() as i64).clamp(lo as i64, hi as i64);
            let n = (auto + i64::from(dn)).clamp(lo as i64, hi as i64) as usize;
            let w = jittered(len, n, zp, &mut rng);
            push_widths(&mut out, 0.0, &w);
        }
        Some(c) => {
            let e = CORNER_ARM_FACTOR * zp.w_target;
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
            // Distribute dn one at a time to the arm with the most slack.
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
                    if dn > 0 {
                        n_arm[k] += 1;
                    } else {
                        n_arm[k] -= 1;
                    }
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

/// Greedy one-to-one interval matching by overlap / longer interval ≥ [`ID_MATCH_MIN`];
/// returns `(new index, old index)` pairs.
fn match_intervals(old: &[(f64, f64)], new: &[(f64, f64)]) -> Vec<(usize, usize)> {
    let mut cand: Vec<(f64, usize, usize)> = Vec::new();
    for (j, n) in new.iter().enumerate() {
        for (i, o) in old.iter().enumerate() {
            let ov = (n.1.min(o.1) - n.0.max(o.0)).max(0.0);
            let r = ov / (n.1 - n.0).max(o.1 - o.0).max(1e-9);
            if r >= ID_MATCH_MIN {
                cand.push((r, j, i));
            }
        }
    }
    // Ties broken by index so the result never depends on sort stability details.
    cand.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let mut un = vec![false; new.len()];
    let mut uo = vec![false; old.len()];
    let mut out = Vec::new();
    for (_, j, i) in cand {
        if !un[j] && !uo[i] {
            un[j] = true;
            uo[i] = true;
            out.push((j, i));
        }
    }
    out
}

fn intervals(x: &[f64]) -> Vec<(f64, f64)> {
    x.windows(2).map(|w| (w[0], w[1])).collect()
}

/// Edit path: previous splits projected onto the new frontage. Same count → smallest local
/// moves that restore the bounds; ±1 lot → every insert/remove/dissolve position is solved and
/// the one keeping the most lot IDs wins. Infeasible ±1 counts are rejected.
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
    let kink_split = |v: f64| f.kinks().iter().any(|k| (k - v).abs() < 0.01);
    let frame = |s: &[f64]| -> Vec<f64> {
        let mut x = Vec::with_capacity(s.len() + 2);
        x.push(0.0);
        x.extend(s);
        x.push(len);
        x
    };
    let pins_for = |x: &[f64]| -> Vec<bool> {
        (0..x.len())
            .map(|i| {
                i == 0
                    || i + 1 == x.len()
                    || is_corner_lot(f, x[i - 1], x[i])
                    || is_corner_lot(f, x[i], x[i + 1])
            })
            .collect()
    };
    let old_iv = intervals(&frame(&s));
    let n_now = s.len() + 1;
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
                let score = match_intervals(&old_iv, &intervals(&frame(&r))).len();
                if best.as_ref().is_none_or(|b| score > b.0) {
                    best = Some((score, r));
                }
            }
        }
        best
    };
    let remove_or_dissolve = |s: &[f64]| {
        let mut c = removals(s);
        c.extend(dissolves(s));
        c
    };
    let result = if hint.n == n_now + 1 {
        try_all(inserts(&s))
    } else if hint.n + 1 == n_now {
        try_all(remove_or_dissolve(&s))
    } else {
        // Same count (drag / depth edit); if it no longer fits, the best ±1 change.
        try_all(vec![s.clone()])
            .or_else(|| try_all(remove_or_dissolve(&s)))
            .or_else(|| try_all(inserts(&s)))
    };
    if let Some((_, r)) = result {
        return Some(r);
    }
    if hint.n != n_now {
        stats.rejected_edit = true;
        let x = frame(&s);
        let pinned = pins_for(&x);
        return finish(f, zp, x, pinned, EDIT_SLACK, true, stats);
    }
    stats.respace_fallback += 1;
    None
}

/// End of a ray from `p0` along `dir` (≤ `cap`), truncated at the plot boundary and seams.
fn trace(p0: DVec2, dir: DVec2, cap: f64, plot: &[DVec2], seams: &[[DVec2; 2]]) -> DVec2 {
    let far = p0 + dir * cap;
    let mut tmin = 1.0;
    let eps = 0.05 / cap;
    let n = plot.len();
    let edges = (0..n)
        .map(|i| (plot[i], plot[(i + 1) % n]))
        .chain(seams.iter().map(|s| (s[0], s[1])));
    for (a, b) in edges {
        if let Some((t, _)) = d2::seg_intersect(p0, far, a, b)
            && t > eps
            && t < tmin
        {
            tmin = t;
        }
    }
    p0 + dir * cap * tmin
}

fn crosses(a: &Ray, b: &Ray) -> bool {
    match d2::seg_intersect(a.p0, a.p1, b.p0, b.p1) {
        Some((t, u)) => t > 1e-6 && t < 1.0 - 1e-6 && u > 1e-6 && u < 1.0 - 1e-6,
        None => false,
    }
}

fn find(p: &mut [usize], i: usize) -> usize {
    let mut r = i;
    while p[r] != r {
        r = p[r];
    }
    p[i] = r;
    r
}

/// Length of boundary shared by two polygons (collinear overlapping edges within 4 mm).
fn shared_len(a: &[DVec2], b: &[DVec2]) -> f64 {
    let mut acc = 0.0;
    let (na, nb) = (a.len(), b.len());
    for i in 0..na {
        let (p, q) = (a[i], a[(i + 1) % na]);
        let d = (q - p).normalize_or_zero();
        for j in 0..nb {
            let (r, s) = (b[j], b[(j + 1) % nb]);
            if d2::dist_point_seg(r, p, q)
                .0
                .min(d2::dist_point_seg(p, r, s).0)
                > 0.004
            {
                continue;
            }
            if d2::cross(d, r - p).abs() < 0.004 && d2::cross(d, s - p).abs() < 0.004 {
                let (t0, t1) = ((r - p).dot(d), (s - p).dot(d));
                let (lo, hi) = (t0.min(t1).max(0.0), t0.max(t1).min(p.distance(q)));
                if hi > lo {
                    acc += hi - lo;
                }
            }
        }
    }
    acc
}

/// Union of lot pieces as one polygon (`None` if it is not a single polygon).
fn union_one(pieces: &[Poly]) -> Option<Poly> {
    let (mut u, _) = f64ops::union_all(pieces);
    (u.len() == 1).then(|| u.swap_remove(0))
}

fn merge(lo: &Lot, hi: &Lot) -> Option<Lot> {
    let mut pieces = lo.pieces.clone();
    pieces.extend(hi.pieces.iter().cloned());
    let poly = union_one(&pieces)?;
    Some(Lot {
        area: d2::area(&poly),
        rect: d2::rectangularity(&poly),
        poly,
        pieces,
        s0: lo.s0,
        s1: hi.s1,
        frontage_len: lo.frontage_len + hi.frontage_len,
        corner: lo.corner || hi.corner,
        merged: true,
        dir0: lo.dir0,
        dir1: hi.dir1,
    })
}

/// Subdivides `plot` (built from `f` with `depth`) into lots.
///
/// `dn` offsets the automatic lot count (sub-plot control) for fresh subdivisions; `seed`
/// drives the width jitter; `hint` carries the previous state on edits.
#[must_use]
pub fn subdivide(
    f: &Frontage,
    plot: &[DVec2],
    zp: &ZoneParams,
    depth: f64,
    dn: i32,
    seed: u64,
    hint: Option<&Hint>,
) -> Output {
    let mut stats = Stats::default();
    let len = f.len();
    let split_s = match hint.and_then(|h| hinted_splits(f, zp, h, &mut stats)) {
        Some(s) => s,
        None => initial_splits(f, zp, dn, seed, &mut stats),
    };
    let reach = 4.0 * depth + 100.0;

    // Seams: medial axis of concave arcs tighter than the depth.
    let seams: Vec<[DVec2; 2]> = f
        .seams()
        .iter()
        .filter(|c| d2::point_in_poly(c.center, plot))
        .map(|c| [c.center, trace(c.center, c.dir, reach, plot, &[])])
        .collect();

    // Rays: mean inward normal over ±½ the narrower adjacent lot; on a seam arc, at its centre.
    let mut bounds = vec![0.0];
    bounds.extend(&split_s);
    bounds.push(len);
    let mut rays: Vec<Ray> = Vec::with_capacity(split_s.len());
    for (k, &s) in split_s.iter().enumerate() {
        let h = 0.5 * (bounds[k + 1] - bounds[k]).min(bounds[k + 2] - bounds[k + 1]);
        let p0 = f.point_at(s);
        let (dir, cap) = match f.seams().iter().find(|c| s > c.s0 && s < c.s1) {
            Some(c) => (
                (c.center - p0).normalize_or(f.normal_at(s)),
                c.center.distance(p0),
            ),
            None => (f.mean_normal(s - h, s + h), reach),
        };
        let p1 = trace(p0, dir, cap, plot, &seams);
        rays.push(Ray {
            s,
            p0,
            dir,
            p1,
            cap,
        });
    }

    // De-crossing: rotate crossing neighbours toward their bisector (8 × 5°), else drop.
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
                            let rot = d2::signed_angle(d, b).clamp(-step, step);
                            rays[idx].dir = d2::rotate(d, rot);
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
        match (0..rays.len().saturating_sub(1)).find(|&k| crosses(&rays[k], &rays[k + 1])) {
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

    // Cut: slice by rays and seams; a ray ending at a seam start continues as that seam.
    let mut seam_used = vec![false; seams.len()];
    let mut lines: Vec<Vec<DVec2>> = Vec::with_capacity(rays.len() + seams.len());
    for r in &rays {
        match seams.iter().position(|sm| sm[0].distance(r.p1) < 1e-6) {
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
    let plot_poly: Poly = plot.to_vec();
    let (pieces, _) = f64ops::slice(&plot_poly, &lines);

    let split_s: Vec<f64> = rays.iter().map(|r| r.s).collect();
    let n_int = split_s.len() + 1;
    let interval_of = |s: f64| split_s.partition_point(|v| *v < s);

    // Frontage contact per piece: (interval, length).
    let fidx = FrontageIndex::new(f);
    let contacts: Vec<Vec<(usize, f64)>> = pieces
        .iter()
        .map(|poly| {
            let mut contact: Vec<(usize, f64)> = Vec::new();
            let n = poly.len();
            for i in 0..n {
                let (a, b) = (poly[i], poly[(i + 1) % n]);
                let m = (a + b) * 0.5;
                if let Some((sm, d)) = fidx.near(f, m)
                    && d < crate::frontage::ON_FRONTAGE_M
                    && fidx.on(f, a)
                    && fidx.on(f, b)
                {
                    let idx = interval_of(sm);
                    let l = a.distance(b);
                    match contact.iter_mut().find(|c| c.0 == idx) {
                        Some(c) => c.1 += l,
                        None => contact.push((idx, l)),
                    }
                }
            }
            contact
        })
        .collect();

    // Union-find over intervals touched by one piece.
    let mut parent: Vec<usize> = (0..n_int).collect();
    for c in &contacts {
        let sig: Vec<usize> = c.iter().filter(|c| c.1 > 0.5).map(|c| c.0).collect();
        if let (Some(&lo), Some(&hi)) = (sig.iter().min(), sig.iter().max())
            && sig.len() > 1
        {
            stats.multi_contact += sig.len() - 1;
            for i in lo..hi {
                let (a, b) = (find(&mut parent, i), find(&mut parent, i + 1));
                parent[a.max(b)] = a.min(b);
            }
        }
    }

    // Owners; orphans join the neighbour sharing the longest edge.
    const DROPPED: usize = usize::MAX;
    let mut owner: Vec<Option<usize>> = contacts
        .iter()
        .map(|c| {
            c.iter()
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .filter(|c| c.1 > 0.01)
                .map(|c| c.0)
        })
        .collect();
    for _ in 0..4 {
        for i in 0..pieces.len() {
            if owner[i].is_some() {
                continue;
            }
            stats.orphans += 1;
            let a = d2::area(&pieces[i]);
            if a < ORPHAN_DROP_M2 {
                stats.dropped_area += a;
                owner[i] = Some(DROPPED);
                continue;
            }
            let best = (0..pieces.len())
                .filter(|&j| j != i && owner[j].is_some_and(|o| o != DROPPED))
                .map(|j| (j, shared_len(&pieces[i], &pieces[j])))
                .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)));
            if let Some((j, l)) = best
                && l > 0.01
            {
                owner[i] = owner[j];
            }
        }
    }
    for (i, o) in owner.iter().enumerate() {
        if o.is_none() {
            stats.dropped_area += d2::area(&pieces[i]);
        }
    }

    // Group pieces into lots.
    let mut groups: Vec<Vec<usize>> = vec![Vec::new(); n_int];
    for (i, o) in owner.iter().enumerate() {
        if let Some(o) = *o
            && o != DROPPED
        {
            let r = find(&mut parent, o);
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
            let polys: Vec<Poly> = members.iter().map(|&m| pieces[m].clone()).collect();
            let poly = if polys.len() == 1 {
                polys[0].clone()
            } else {
                f64ops::union_all(&polys)
                    .0
                    .into_iter()
                    .max_by(|a, b| d2::area(a).total_cmp(&d2::area(b)))
                    .unwrap_or_default()
            };
            let frontage_len: f64 = members
                .iter()
                .flat_map(|&m| contacts[m].iter().map(|c| c.1))
                .sum();
            lots.push(Lot {
                area: d2::area(&poly),
                rect: d2::rectangularity(&poly),
                poly,
                pieces: polys,
                s0: bounds_of(&split_s, len, i).0,
                s1: bounds_of(&split_s, len, j).1,
                frontage_len,
                corner: is_corner_lot(
                    f,
                    bounds_of(&split_s, len, i).0,
                    bounds_of(&split_s, len, j).1,
                ),
                merged: false,
                dir0: ray_dir(i as isize - 1),
                dir1: ray_dir(j as isize),
            });
        }
        i = j + 1;
    }

    // Repair: re-apply the stored merges first (hysteresis), then merge failing lots.
    stats.lots_initial = lots.len();
    stats.rect_fail_initial = lots.iter().filter(|l| l.rect < RECT_MIN).count();
    let mut merged_pts: Vec<DVec2> = Vec::new();
    if let Some(h) = hint {
        let old: Vec<f64> = h.merged_pts.iter().map(|p| f.project(*p).0).collect();
        let mut k = 0;
        while k + 1 < lots.len() {
            let b = lots[k].s1;
            if old.iter().any(|o| (o - b).abs() < 0.05)
                && let Some(m) = merge(&lots[k], &lots[k + 1])
            {
                merged_pts.push(f.point_at(b));
                lots[k] = m;
                lots.remove(k + 1);
                continue;
            }
            k += 1;
        }
    }
    let (rect_min, a_min) = if hint.is_some() {
        (RECT_MIN_EDIT, zp.a_min * AREA_FACTOR_EDIT)
    } else {
        (RECT_MIN, zp.a_min)
    };
    let mut accepted = vec![false; lots.len()];
    while lots.len() >= 2 {
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
            if let Some(m) = merge(&lots[lo], &lots[hi]) {
                merged_pts.push(f.point_at(lots[lo].s1));
                lots[lo] = m;
                lots.remove(hi);
                accepted.remove(hi);
                stats.merged += 1;
                done = true;
                break;
            }
        }
        if !done {
            accepted[k] = true;
            stats.unmergeable += 1;
        }
    }

    Output {
        lots,
        splits: rays.iter().map(|r| (r.s, r.p0)).collect(),
        cuts: rays.iter().map(|r| [r.p0, r.p1]).collect(),
        merged_pts,
        seams,
        stats,
    }
}

/// Frontage interval `k` of the split list.
fn bounds_of(split_s: &[f64], len: f64, k: usize) -> (f64, f64) {
    let a = if k == 0 { 0.0 } else { split_s[k - 1] };
    let b = split_s.get(k).copied().unwrap_or(len);
    (a, b)
}

/// Stable lot IDs: greedy frontage-interval overlap ≥ [`ID_MATCH_MIN`]. Old intervals are
/// given as world points of their frontage ends; returns, per new lot, the matched old index.
#[must_use]
pub fn match_ids(f_new: &Frontage, old: &[(DVec2, DVec2)], new: &[Lot]) -> Vec<Option<usize>> {
    let old_s: Vec<(f64, f64)> = old
        .iter()
        .map(|(a, b)| (f_new.project(*a).0, f_new.project(*b).0))
        .collect();
    let new_s: Vec<(f64, f64)> = new.iter().map(|l| (l.s0, l.s1)).collect();
    let mut out = vec![None; new.len()];
    for (j, i) in match_intervals(&old_s, &new_s) {
        out[j] = Some(i);
    }
    out
}

/// Determinism fixture (`subdivision_hash_cross_os`, procgen golden hash): one residential plot
/// per S-P frontage family (straight, convex, concave, S-curve, junction fillet, kink),
/// subdivided and fitted; hashes the bit patterns of every lot and footprint corner (libm trig,
/// i_overlay snapping). Pinned in `tests/subdivision.rs` and compared across OSes by
/// `cb_sim`'s `determinism_probe`.
#[must_use]
pub fn reference_families_hash() -> u64 {
    use crate::frontage::{Frontage, Prim};
    use crate::rect_fit::{FitParams, RectSpec, fit_lots};
    let d = f64::to_radians;
    let plots = [
        vec![Prim::Line(120.0)],
        vec![Prim::Arc(60.0, -d(110.0))],
        vec![Prim::Arc(50.0, d(120.0))],
        vec![Prim::Arc(80.0, d(50.0)), Prim::Arc(50.0, -d(70.0))],
        vec![Prim::Line(70.0), Prim::Arc(8.0, d(120.0)), Prim::Line(70.0)],
        vec![
            Prim::Line(60.0),
            Prim::Arc(10.0, -d(35.0)),
            Prim::Line(60.0),
        ],
    ];
    let zp = ZoneParams::RESIDENTIAL;
    let fit = FitParams {
        setback: zp.setback,
        rect: Some(RectSpec::HOUSE),
    };
    let mut bits: Vec<u64> = Vec::new();
    for (k, prims) in plots.iter().enumerate() {
        let depth = 30.0;
        let Some(f) = Frontage::from_prims(prims, depth) else {
            continue;
        };
        let Some(plot) = f.plot_polygon(depth, 12.0) else {
            continue;
        };
        let out = subdivide(&f, &plot, &zp, depth, 0, 0x5EED_0000 + k as u64, None);
        for l in &out.lots {
            bits.extend(l.poly.iter().flat_map(|v| [v.x.to_bits(), v.y.to_bits()]));
        }
        for fp in fit_lots(&out.lots, &f, &fit)
            .iter()
            .filter_map(|x| x.footprint)
        {
            bits.extend(fp.iter().flat_map(|v| [v.x.to_bits(), v.y.to_bits()]));
        }
    }
    cb_core::hash::stable_hash(&bits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontage::Prim;

    #[test]
    fn straight_residential_lots() {
        let f = Frontage::from_prims(&[Prim::Line(130.0)], 30.0).unwrap();
        let plot = f.plot_polygon(30.0, 12.0).unwrap();
        let out = subdivide(&f, &plot, &ZoneParams::RESIDENTIAL, 30.0, 0, 7, None);
        assert_eq!(out.lots.len(), 10);
        let total: f64 = out.lots.iter().map(|l| l.area).sum();
        assert!((total - 3900.0).abs() < 1.0);
        for l in &out.lots {
            assert!(l.frontage_len >= 10.0 - 0.01 && l.frontage_len <= 18.0 + 0.01);
            assert!(l.rect > 0.99);
        }
    }

    #[test]
    fn same_seed_same_result() {
        let f = Frontage::from_prims(&[Prim::Arc(60.0, -(100f64.to_radians()))], 30.0).unwrap();
        let plot = f.plot_polygon(30.0, 12.0).unwrap();
        let a = subdivide(&f, &plot, &ZoneParams::RESIDENTIAL, 30.0, 0, 3, None);
        let b = subdivide(&f, &plot, &ZoneParams::RESIDENTIAL, 30.0, 0, 3, None);
        assert_eq!(a.splits, b.splits);
        assert_eq!(a.lots.len(), b.lots.len());
        for (x, y) in a.lots.iter().zip(&b.lots) {
            assert_eq!(x.poly, y.poly);
        }
    }
}
