//! Building fitting (ADR-0064, ALGORITHMS §11 steps 1–2): per-edge setbacks by polygon erosion,
//! then the largest inscribed oriented rectangle (adaptive raster + continuous edge refinement).

use glam::DVec2;

use crate::boolean::f64ops;
use crate::d2::{self, Poly};
use crate::frontage::{Frontage, FrontageIndex};
use crate::subdivide::Lot;

/// Finest raster cell for the inscribed-rectangle search (m).
pub const RASTER_CELL_M: f64 = 0.5;
/// Raster cells across the longer side at most (refinement recovers sub-cell precision).
pub const RASTER_MAX_CELLS: f64 = 40.0;
/// Setback reduction factors tried before a lot stays green.
pub const SETBACK_FACTORS: [f64; 3] = [1.0, 0.75, 0.5];

/// Inscribed-rectangle requirements (detached housing).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectSpec {
    /// Minimum side (m).
    pub min_side_m: f64,
    /// Minimum area (m²).
    pub min_area_m2: f64,
    /// Footprint width cap along the frontage (m).
    pub max_w_m: f64,
    /// Footprint depth cap (m).
    pub max_d_m: f64,
}

impl RectSpec {
    /// Residential L1–L3 houses.
    pub const HOUSE: Self = Self {
        min_side_m: 6.0,
        min_area_m2: 50.0,
        max_w_m: 16.0,
        max_d_m: 16.0,
    };
}

/// Fitting parameters of one zone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FitParams {
    /// Front, side and rear setbacks (m).
    pub setback: [f64; 3],
    /// Rectangle requirement (`None`: only the inset polygon is produced).
    pub rect: Option<RectSpec>,
}

/// Result of fitting one lot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fit {
    /// Lot shrunk by the setbacks.
    pub inset: Option<Poly>,
    /// Largest inscribed rectangle in the inset, counter-clockwise corners.
    pub rect: Option<[DVec2; 4]>,
    /// Its area (m²).
    pub rect_area: f64,
    /// Footprint used: the rectangle clamped to the caps, at its frontage side. Corners
    /// `[front-left, front-right, back-right, back-left]` seen from the road… in CCW order.
    pub footprint: Option<[DVec2; 4]>,
    /// Setback factor that succeeded.
    pub setback_factor: f64,
    /// Nothing fits: the lot stays green.
    pub green: bool,
}

/// Lot edge classes for setbacks and yard features.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeKind {
    /// On the frontage (road side).
    Front,
    /// Between neighbours.
    Side,
    /// Far half of the lot and within 45° of the frontage direction.
    Rear,
}

fn classify(p: &[DVec2], f: &Frontage, idx: &FrontageIndex) -> Vec<EdgeKind> {
    let n = p.len();
    let dmax = p.iter().map(|v| f.project(*v).1).fold(0.0, f64::max);
    (0..n)
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % n]);
            let m = (a + b) * 0.5;
            if idx.on(f, m) && idx.on(f, a) && idx.on(f, b) {
                return EdgeKind::Front;
            }
            let (sm, dm) = f.project(m);
            let e = (b - a).normalize_or_zero();
            if dm > 0.5 * dmax && e.dot(f.tangent_at(sm)).abs() > std::f64::consts::FRAC_1_SQRT_2 {
                EdgeKind::Rear
            } else {
                EdgeKind::Side
            }
        })
        .collect()
}

fn is_convex(p: &[DVec2]) -> bool {
    let n = p.len();
    (0..n).all(|i| d2::cross(p[(i + 1) % n] - p[i], p[(i + 2) % n] - p[(i + 1) % n]) >= -1e-9)
}

/// Clips `p` to the left of the directed line through `a` with direction `d`.
fn clip_half(p: &[DVec2], a: DVec2, d: DVec2) -> Poly {
    let side = |q: DVec2| d2::cross(d, q - a);
    let n = p.len();
    let mut out = Vec::with_capacity(n + 2);
    for i in 0..n {
        let (u, v) = (p[i], p[(i + 1) % n]);
        let (su, sv) = (side(u), side(v));
        if su >= 0.0 {
            out.push(u);
        }
        if (su >= 0.0) != (sv >= 0.0) {
            out.push(u + (v - u) * (su / (su - sv)));
        }
    }
    out
}

/// Lot shrunk by per-edge setbacks. Convex lots: exact half-plane clipping; others: lot minus
/// inward edge strips plus wedges at reflex vertices.
fn inset(p: &[DVec2], kinds: &[EdgeKind], sb: [f64; 3], k: f64) -> Option<Poly> {
    let n = p.len();
    let set = |i: usize| {
        k * match kinds[i] {
            EdgeKind::Front => sb[0],
            EdgeKind::Side => sb[1],
            EdgeKind::Rear => sb[2],
        }
    };
    if is_convex(p) {
        let mut q = p.to_vec();
        for i in 0..n {
            let (a, b) = (p[i], p[(i + 1) % n]);
            let d = (b - a).normalize_or_zero();
            q = clip_half(&q, a + d2::perp(d) * set(i), d);
            if q.len() < 3 {
                return None;
            }
        }
        return Some(q).filter(|q| d2::area(q) > 1.0);
    }
    let mut cut: Vec<Poly> = Vec::with_capacity(n + 4);
    for i in 0..n {
        let s = set(i);
        let (a, b) = (p[i], p[(i + 1) % n]);
        let nrm = d2::perp((b - a).normalize_or_zero());
        if s > 1e-3 {
            cut.push(vec![a, b, b + nrm * s, a + nrm * s]);
        }
        let c = p[(i + 2) % n];
        if d2::cross(b - a, c - b) < 0.0 {
            let r = s.max(set((i + 1) % n));
            if r > 1e-3 {
                let n2 = d2::perp((c - b).normalize_or_zero());
                let mid = (nrm + n2).normalize_or(nrm);
                cut.push(vec![b, b + n2 * r, b + mid * r, b + nrm * r]);
            }
        }
    }
    let poly = p.to_vec();
    let parts = if cut.is_empty() {
        vec![poly]
    } else {
        f64ops::difference(&poly, &cut)
    };
    parts
        .into_iter()
        .max_by(|a, b| d2::area(a).total_cmp(&d2::area(b)))
        .filter(|q| d2::area(q) > 1.0)
}

/// Inside or within 1 µm of the boundary (even-odd alone is ambiguous on the boundary).
fn inside_or_on(q: DVec2, poly: &[DVec2]) -> bool {
    let n = poly.len();
    d2::point_in_poly(q, poly)
        || (0..n).any(|i| d2::dist_point_seg(q, poly[i], poly[(i + 1) % n]).0 < 1e-6)
}

/// Whether the rectangle lies inside `poly` (boundary touches allowed).
#[must_use]
pub fn rect_inside(r: &[DVec2; 4], poly: &[DVec2]) -> bool {
    if !r.iter().all(|c| inside_or_on(*c, poly)) {
        return false;
    }
    let c = (r[0] + r[2]) * 0.5;
    let shrunk: Vec<DVec2> = r.iter().map(|v| c + (*v - c) * 0.999).collect();
    if poly.iter().any(|v| d2::point_in_poly(*v, &shrunk)) {
        return false;
    }
    let n = poly.len();
    for i in 0..4 {
        for j in 0..n {
            if let Some((t, u)) =
                d2::seg_intersect(r[i], r[(i + 1) % 4], poly[j], poly[(j + 1) % n])
                && t > 1e-6
                && t < 1.0 - 1e-6
                && u > 1e-6
                && u < 1.0 - 1e-6
            {
                return false;
            }
        }
    }
    true
}

/// Largest rectangle with sides ≥ `min_side` aligned with `e`.
fn max_rect_dir(poly: &[DVec2], e: DVec2, min_side: f64) -> Option<(f64, [DVec2; 4])> {
    let v = d2::perp(e);
    let local: Vec<DVec2> = poly
        .iter()
        .map(|p| DVec2::new(p.dot(e), p.dot(v)))
        .collect();
    let (lo, hi) = d2::bbox(&local);
    let cell = RASTER_CELL_M.max((hi - lo).max_element() / RASTER_MAX_CELLS);
    let nx = ((hi.x - lo.x) / cell).floor() as usize;
    let ny = ((hi.y - lo.y) / cell).floor() as usize;
    let min_cells = (min_side / cell).ceil() as usize;
    if nx < min_cells || ny < min_cells {
        return None;
    }
    // Scanline inside test for the cell-corner lattice.
    let mut corner_in = vec![false; (nx + 1) * (ny + 1)];
    let m = local.len();
    let mut xs: Vec<f64> = Vec::with_capacity(8);
    for j in 0..=ny {
        let y = lo.y + j as f64 * cell;
        xs.clear();
        for k in 0..m {
            let (a, b) = (local[k], local[(k + 1) % m]);
            if (a.y > y) != (b.y > y) {
                xs.push(a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y));
            }
        }
        xs.sort_by(f64::total_cmp);
        for pair in xs.chunks(2) {
            if let [x0, x1] = *pair {
                let i0 = ((x0 - lo.x) / cell).ceil().max(0.0) as usize;
                let i1 = ((x1 - lo.x) / cell).floor();
                if i1 < 0.0 {
                    continue;
                }
                for i in i0..=(i1 as usize).min(nx) {
                    corner_in[j * (nx + 1) + i] = true;
                }
            }
        }
    }
    let ci = |i: usize, j: usize| corner_in[j * (nx + 1) + i];
    // Cells holding a polygon vertex strictly inside are blocked (reflex notches).
    let mut blocked = vec![false; nx * ny];
    for p in &local {
        let (fx, fy) = ((p.x - lo.x) / cell, (p.y - lo.y) / cell);
        if fx > 0.0
            && fy > 0.0
            && fx < nx as f64
            && fy < ny as f64
            && fx.fract() > 1e-6
            && fy.fract() > 1e-6
        {
            blocked[fy as usize * nx + fx as usize] = true;
        }
    }
    let mut heights = vec![0usize; nx];
    let mut best: Option<(f64, usize, usize, usize, usize)> = None;
    let mut stack: Vec<usize> = Vec::with_capacity(nx + 1);
    for j in 0..ny {
        for i in 0..nx {
            let inside = !blocked[j * nx + i]
                && ci(i, j)
                && ci(i + 1, j)
                && ci(i, j + 1)
                && ci(i + 1, j + 1);
            heights[i] = if inside { heights[i] + 1 } else { 0 };
        }
        // Largest rectangle in the histogram (all maximal rectangles enumerated).
        stack.clear();
        for i in 0..=nx {
            let h = if i < nx { heights[i] } else { 0 };
            while let Some(&top) = stack.last() {
                if heights[top] <= h {
                    break;
                }
                stack.pop();
                let ht = heights[top];
                let left = stack.last().map_or(0, |s| s + 1);
                let w = i - left;
                if w >= min_cells && ht >= min_cells {
                    let a = (w * ht) as f64 * cell * cell;
                    if best.is_none_or(|b| a > b.0) {
                        best = Some((a, left, j + 1 - ht, w, ht));
                    }
                }
            }
            stack.push(i);
        }
    }
    let (_, x0, y0, w, h) = best?;
    let mut r = [
        lo.x + x0 as f64 * cell,
        lo.y + y0 as f64 * cell,
        lo.x + (x0 + w) as f64 * cell,
        lo.y + (y0 + h) as f64 * cell,
    ];
    let to_world = |r: &[f64; 4]| -> [DVec2; 4] {
        let w = |x: f64, y: f64| e * x + v * y;
        [w(r[0], r[1]), w(r[2], r[1]), w(r[2], r[3]), w(r[0], r[3])]
    };
    // Continuous refinement: grow each side by up to one cell while still inside.
    for side in 0..4 {
        let (mut a, mut b) = (0.0, cell);
        for _ in 0..7 {
            let m = 0.5 * (a + b);
            let mut t = r;
            if side < 2 {
                t[side] -= m;
            } else {
                t[side] += m;
            }
            if rect_inside(&to_world(&t), poly) {
                a = m;
            } else {
                b = m;
            }
        }
        if side < 2 {
            r[side] -= a;
        } else {
            r[side] += a;
        }
    }
    Some(((r[2] - r[0]) * (r[3] - r[1]), to_world(&r)))
}

/// Largest inscribed rectangle over ≤ 4 orientations (frontage tangent + longest edges).
#[must_use]
pub fn largest_rect(poly: &[DVec2], front_dir: DVec2, min_side: f64) -> Option<(f64, [DVec2; 4])> {
    let mut dirs: Vec<DVec2> = vec![front_dir.normalize_or(DVec2::X)];
    let n = poly.len();
    let mut edges: Vec<(f64, DVec2)> = (0..n)
        .map(|i| {
            (
                poly[i].distance(poly[(i + 1) % n]),
                (poly[(i + 1) % n] - poly[i]).normalize_or_zero(),
            )
        })
        .filter(|e| e.0 >= 2.0)
        .collect();
    edges.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (_, d) in edges {
        // Directions are equivalent modulo 90°.
        let dup = dirs.iter().any(|o| {
            let a = d2::angle_between(*o, d).to_degrees() % 90.0;
            !(2.0..=88.0).contains(&a)
        });
        if !dup && dirs.len() < 4 {
            dirs.push(d);
        }
    }
    dirs.into_iter()
        .filter_map(|d| max_rect_dir(poly, d, min_side))
        .max_by(|a, b| a.0.total_cmp(&b.0))
}

/// Clamps the rectangle to `max_w × max_d` (width along the frontage), keeping its front side.
fn clamp_footprint(r: &[DVec2; 4], front_pt: DVec2, max_w: f64, max_d: f64) -> [DVec2; 4] {
    let (e0, e1) = (r[1] - r[0], r[3] - r[0]);
    let (l0, l1) = (e0.length(), e1.length());
    let (u0, u1) = (e0 / l0, e1 / l1);
    let c = (r[0] + r[2]) * 0.5;
    let to_front = front_pt - c;
    let (wu, wl, du, dl) = if to_front.dot(u1).abs() >= to_front.dot(u0).abs() {
        (u0, l0, u1, l1)
    } else {
        (u1, l1, u0, l0)
    };
    let du = if to_front.dot(du) > 0.0 { -du } else { du };
    let (w, d) = (wl.min(max_w), dl.min(max_d));
    let fm = c - du * (dl * 0.5);
    let a = fm - wu * (w * 0.5);
    let b = fm + wu * (w * 0.5);
    let out = [a, b, b + du * d, a + du * d];
    if d2::signed_area(&out) < 0.0 {
        [out[1], out[0], out[3], out[2]]
    } else {
        out
    }
}

fn fit_one(lot: &Lot, f: &Frontage, idx: &FrontageIndex, p: &FitParams) -> Fit {
    let poly = d2::simplify_fast(&lot.poly, 0.1);
    let kinds = classify(&poly, f, idx);
    let mid = 0.5 * (lot.s0 + lot.s1);
    let (front_dir, front_pt) = (f.tangent_at(mid), f.point_at(mid));
    for k in SETBACK_FACTORS {
        let Some(ins) = inset(&poly, &kinds, p.setback, k) else {
            continue;
        };
        let Some(spec) = p.rect else {
            return Fit {
                inset: Some(ins),
                setback_factor: k,
                ..Default::default()
            };
        };
        if let Some((a, r)) = largest_rect(&ins, front_dir, spec.min_side_m)
            && a >= spec.min_area_m2
        {
            return Fit {
                inset: Some(ins),
                rect: Some(r),
                rect_area: a,
                footprint: Some(clamp_footprint(&r, front_pt, spec.max_w_m, spec.max_d_m)),
                setback_factor: k,
                green: false,
            };
        }
    }
    Fit {
        green: true,
        ..Default::default()
    }
}

/// Edge classes of every edge of `lot.poly` (edge `k` runs from vertex `k` to `k + 1`).
#[must_use]
pub fn lot_edge_kinds(lot: &Lot, f: &Frontage) -> Vec<EdgeKind> {
    classify(&lot.poly, f, &FrontageIndex::new(f))
}

/// Fits every lot of one plot.
#[must_use]
pub fn fit_lots(lots: &[Lot], f: &Frontage, p: &FitParams) -> Vec<Fit> {
    let idx = FrontageIndex::new(f);
    lots.iter().map(|l| fit_one(l, f, &idx, p)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn largest_rect_in_square_and_l_shape() {
        let sq = vec![
            DVec2::ZERO,
            DVec2::new(20.0, 0.0),
            DVec2::new(20.0, 10.0),
            DVec2::new(0.0, 10.0),
        ];
        let (a, r) = largest_rect(&sq, DVec2::X, 6.0).unwrap();
        assert!(a > 199.0 && a <= 200.0 + 1e-9, "{a}");
        assert!(rect_inside(&r, &sq));
        let l = vec![
            DVec2::ZERO,
            DVec2::new(20.0, 0.0),
            DVec2::new(20.0, 8.0),
            DVec2::new(8.0, 8.0),
            DVec2::new(8.0, 20.0),
            DVec2::new(0.0, 20.0),
        ];
        let (a, r) = largest_rect(&l, DVec2::X, 6.0).unwrap();
        assert!(a > 159.0 && a <= 160.0 + 1e-9, "{a}");
        assert!(rect_inside(&r, &l));
        assert!(largest_rect(&sq, DVec2::X, 12.0).is_none());
    }

    #[test]
    fn convex_inset_is_exact() {
        let sq = vec![
            DVec2::ZERO,
            DVec2::new(20.0, 0.0),
            DVec2::new(20.0, 30.0),
            DVec2::new(0.0, 30.0),
        ];
        let kinds = [
            EdgeKind::Front,
            EdgeKind::Side,
            EdgeKind::Rear,
            EdgeKind::Side,
        ];
        let q = inset(&sq, &kinds, [3.0, 1.5, 4.0], 1.0).unwrap();
        assert!((d2::area(&q) - 17.0 * 23.0).abs() < 1e-9);
    }
}
