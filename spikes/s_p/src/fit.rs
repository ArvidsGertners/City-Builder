//! Building fitting (ALGORITHMS §11 steps 1–2): per-edge setbacks by polygon erosion, then the
//! largest inscribed oriented rectangle (0.5 m raster + continuous edge refinement).

use glam::DVec2;

use crate::frontage::{Frontage, FrontageIndex, Zone, ZoneParams};
use crate::geom::{self, Poly};
use crate::subdivide::Lot;

pub const CELL: f64 = 0.5;
pub const MIN_SIDE: f64 = 6.0;
pub const MIN_FOOTPRINT: f64 = 50.0;

#[derive(Clone, Debug, Default)]
pub struct Fit {
    pub inset: Option<Poly>,
    /// Largest inscribed rectangle (R only), CCW corners.
    pub rect: Option<[DVec2; 4]>,
    pub rect_area: f64,
    /// Footprint actually used (rect clamped to the typology's max size, at the front).
    pub footprint: Option<[DVec2; 4]>,
    pub setback_factor: f64,
    pub green: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EdgeKind {
    Front,
    Side,
    Rear,
}

fn classify(p: &Poly, f: &Frontage, idx: &FrontageIndex) -> Vec<EdgeKind> {
    let n = p.len();
    let dmax = p.iter().map(|v| f.project(*v).1).fold(0.0, f64::max);
    (0..n)
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % n]);
            let m = (a + b) * 0.5;
            let on = |q: DVec2| idx.near(f, q).is_some_and(|v| v.1 < 0.004);
            if on(m) && on(a) && on(b) {
                return EdgeKind::Front;
            }
            let (sm, dm) = f.project(m);
            let e = (b - a).normalize_or_zero();
            let t = f.tangent_at(sm);
            if dm > 0.5 * dmax && e.dot(t).abs() > 45f64.to_radians().cos() {
                EdgeKind::Rear
            } else {
                EdgeKind::Side
            }
        })
        .collect()
}

fn is_convex(p: &[DVec2]) -> bool {
    let n = p.len();
    (0..n).all(|i| geom::cross(p[(i + 1) % n] - p[i], p[(i + 2) % n] - p[(i + 1) % n]) >= -1e-9)
}

/// Clips polygon `p` to the left side of the directed line through `a` with direction `d`.
fn clip_half(p: &[DVec2], a: DVec2, d: DVec2) -> Poly {
    let side = |q: DVec2| geom::cross(d, q - a);
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

/// Lot shrunk by per-edge setbacks. Convex lots: exact half-plane clipping. Others: lot minus
/// the union of inward edge strips plus wedges at reflex vertices (i_overlay).
fn inset(p: &Poly, kinds: &[EdgeKind], sb: [f64; 3], k: f64) -> Option<Poly> {
    let n = p.len();
    let set = |i: usize| {
        k * match kinds[i] {
            EdgeKind::Front => sb[0],
            EdgeKind::Side => sb[1],
            EdgeKind::Rear => sb[2],
        }
    };
    if is_convex(p) {
        let mut q = p.clone();
        for i in 0..n {
            let (a, b) = (p[i], p[(i + 1) % n]);
            let d = (b - a).normalize_or_zero();
            q = clip_half(&q, a + geom::perp(d) * set(i), d);
            if q.len() < 3 {
                return None;
            }
        }
        return Some(q).filter(|q| geom::area(q) > 1.0);
    }
    let mut cut: Vec<Poly> = Vec::with_capacity(n + 4);
    for i in 0..n {
        let s = set(i);
        let (a, b) = (p[i], p[(i + 1) % n]);
        let nrm = geom::perp((b - a).normalize_or_zero());
        if s > 1e-3 {
            cut.push(vec![a, b, b + nrm * s, a + nrm * s]);
        }
        // Reflex vertex at b: fill the wedge between this strip and the next.
        let c = p[(i + 2) % n];
        if geom::cross(b - a, c - b) < 0.0 {
            let s2 = set((i + 1) % n);
            let r = s.max(s2);
            if r > 1e-3 {
                let n2 = geom::perp((c - b).normalize_or_zero());
                let mid = (nrm + n2).normalize_or(nrm);
                cut.push(vec![b, b + n2 * r, b + mid * r, b + nrm * r]);
            }
        }
    }
    let parts = if cut.is_empty() {
        vec![p.clone()]
    } else {
        geom::difference(p, &cut)
    };
    parts
        .into_iter()
        .max_by(|a, b| geom::area(a).total_cmp(&geom::area(b)))
        .filter(|q| geom::area(q) > 1.0)
}

/// Rectangle (in world space) fully inside `poly`.
pub fn rect_inside(r: &[DVec2; 4], poly: &[DVec2]) -> bool {
    if !r.iter().all(|c| geom::point_in_poly(*c, poly)) {
        return false;
    }
    if poly.iter().any(|v| geom::point_in_poly(*v, r)) {
        // A vertex strictly inside the rectangle (boundary touches are fine for snapping).
        let shrunk: Vec<DVec2> = {
            let c = (r[0] + r[2]) * 0.5;
            r.iter().map(|v| c + (*v - c) * 0.999).collect()
        };
        if poly.iter().any(|v| geom::point_in_poly(*v, &shrunk)) {
            return false;
        }
    }
    let n = poly.len();
    for i in 0..4 {
        for j in 0..n {
            if let Some((t, u)) =
                geom::seg_intersect(r[i], r[(i + 1) % 4], poly[j], poly[(j + 1) % n])
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

/// Largest rectangle with sides ≥ MIN_SIDE aligned with `e`, or None.
fn max_rect_dir(poly: &[DVec2], e: DVec2) -> Option<(f64, [DVec2; 4])> {
    let v = geom::perp(e);
    let local: Vec<DVec2> = poly
        .iter()
        .map(|p| DVec2::new(p.dot(e), p.dot(v)))
        .collect();
    let (lo, hi) = geom::bbox(&local);
    // Adaptive raster: ≥ 0.5 m, at most ~40 cells across the longer side (refinement recovers
    // the sub-cell precision).
    let cell = CELL.max((hi - lo).max_element() / 40.0);
    let nx = ((hi.x - lo.x) / cell).floor() as usize;
    let ny = ((hi.y - lo.y) / cell).floor() as usize;
    let min_cells = (MIN_SIDE / cell).ceil() as usize;
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
        for e in 0..m {
            let (a, b) = (local[e], local[(e + 1) % m]);
            if (a.y > y) != (b.y > y) {
                xs.push(a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y));
            }
        }
        xs.sort_by(f64::total_cmp);
        for pair in xs.chunks(2) {
            if let [x0, x1] = *pair {
                let i0 = ((x0 - lo.x) / cell).ceil().max(0.0) as usize;
                let i1 = (((x1 - lo.x) / cell).floor() as isize).min(nx as isize);
                for i in i0..=i1.max(-1) as usize {
                    if i as isize <= i1 {
                        corner_in[j * (nx + 1) + i] = true;
                    }
                }
            }
        }
    }
    let ci = |i: usize, j: usize| corner_in[j * (nx + 1) + i];
    let mut blocked = vec![false; nx * ny];
    for p in &local {
        let fx = (p.x - lo.x) / cell;
        let fy = (p.y - lo.y) / cell;
        let (ix, iy) = (fx.floor() as isize, fy.floor() as isize);
        if ix >= 0
            && iy >= 0
            && (ix as usize) < nx
            && (iy as usize) < ny
            && fx.fract() > 1e-6
            && fy.fract() > 1e-6
        {
            blocked[iy as usize * nx + ix as usize] = true;
        }
    }
    let mut heights = vec![0usize; nx];
    let mut best: Option<(f64, usize, usize, usize, usize)> = None; // area, x0, y0, w, h
    for j in 0..ny {
        for i in 0..nx {
            let inside = !blocked[j * nx + i]
                && ci(i, j)
                && ci(i + 1, j)
                && ci(i, j + 1)
                && ci(i + 1, j + 1);
            heights[i] = if inside { heights[i] + 1 } else { 0 };
        }
        // Largest rectangle in histogram (all maximal rectangles enumerated).
        let mut stack: Vec<usize> = Vec::new();
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
                t[side] -= m
            } else {
                t[side] += m
            }
            if rect_inside(&to_world(&t), poly) {
                a = m
            } else {
                b = m
            }
        }
        if side < 2 { r[side] -= a } else { r[side] += a }
    }
    let area = (r[2] - r[0]) * (r[3] - r[1]);
    Some((area, to_world(&r)))
}

pub fn largest_rect(poly: &[DVec2], front_dir: DVec2) -> Option<(f64, [DVec2; 4])> {
    let mut dirs: Vec<DVec2> = vec![front_dir.normalize()];
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
            let a = geom::angle_between(*o, d).to_degrees() % 90.0;
            a < 2.0 || a > 88.0
        });
        if !dup && dirs.len() < 4 {
            dirs.push(d);
        }
    }
    dirs.into_iter()
        .filter_map(|d| max_rect_dir(poly, d))
        .max_by(|a, b| a.0.total_cmp(&b.0))
}

/// Clamps the rectangle to `max_w × max_d` (width along `front`), keeping the frontage side.
fn clamp_footprint(r: &[DVec2; 4], front_pt: DVec2, max_w: f64, max_d: f64) -> [DVec2; 4] {
    let e0 = r[1] - r[0];
    let e1 = r[3] - r[0];
    let (l0, l1) = (e0.length(), e1.length());
    let (u0, u1) = (e0 / l0, e1 / l1);
    // Which axis points away from the frontage?
    let c = (r[0] + r[2]) * 0.5;
    let to_front = front_pt - c;
    let (wu, wl, du, dl) = if to_front.dot(u1).abs() >= to_front.dot(u0).abs() {
        (u0, l0, u1, l1)
    } else {
        (u1, l1, u0, l0)
    };
    let du = if to_front.dot(du) > 0.0 { -du } else { du };
    let w = wl.min(max_w);
    let d = dl.min(max_d);
    // Front edge midpoint of the rect.
    let fm = c - du * (dl * 0.5);
    let a = fm - wu * (w * 0.5);
    let b = fm + wu * (w * 0.5);
    let out = [a, b, b + du * d, a + du * d];
    if geom::area(&out) < 0.0 {
        [out[1], out[0], out[3], out[2]]
    } else {
        out
    }
}

pub fn fit_lot(lot: &Lot, f: &Frontage, idx: &FrontageIndex, zone: Zone, zp: &ZoneParams) -> Fit {
    let t0 = std::time::Instant::now();
    let poly = geom::simplify_fast(&lot.poly, 0.1);
    let kinds = classify(&poly, f, idx);
    geom::prof(4, t0);
    let front_dir = f.tangent_at(0.5 * (lot.s0 + lot.s1));
    let front_pt = f.point_at(0.5 * (lot.s0 + lot.s1));
    for k in [1.0, 0.75, 0.5] {
        let t1 = std::time::Instant::now();
        let ins = inset(&poly, &kinds, zp.setback, k);
        geom::prof(5, t1);
        let Some(ins) = ins else { continue };
        if zone != Zone::R {
            return Fit {
                inset: Some(ins),
                setback_factor: k,
                ..Default::default()
            };
        }
        let t2 = std::time::Instant::now();
        let lr = largest_rect(&ins, front_dir);
        geom::prof(6, t2);
        if let Some((a, r)) = lr
            && a >= MIN_FOOTPRINT
        {
            let fp = clamp_footprint(&r, front_pt, 16.0, 16.0);
            return Fit {
                inset: Some(ins),
                rect: Some(r),
                rect_area: a,
                footprint: Some(fp),
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
