//! Small 2D geometry helpers and the i_overlay bridge (1 mm fixed scale).

use glam::DVec2;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::scale::FixedScaleFloatOverlay;
use i_overlay::float::slice::FloatSlice;

/// i_overlay grid: 1 mm (ALGORITHMS §10 "1 mm snapping").
pub const SCALE: f64 = 1000.0;

pub type Poly = Vec<DVec2>;

pub fn to_arr(p: &[DVec2]) -> Vec<[f64; 2]> {
    p.iter().map(|v| [v.x, v.y]).collect()
}

pub fn from_arr(p: &[[f64; 2]]) -> Poly {
    p.iter().map(|a| DVec2::new(a[0], a[1])).collect()
}

/// Signed area (CCW positive).
pub fn area(p: &[DVec2]) -> f64 {
    let n = p.len();
    let mut a = 0.0;
    for i in 0..n {
        let (u, v) = (p[i], p[(i + 1) % n]);
        a += u.x * v.y - v.x * u.y;
    }
    0.5 * a
}

pub fn perimeter(p: &[DVec2]) -> f64 {
    let n = p.len();
    (0..n).map(|i| p[i].distance(p[(i + 1) % n])).sum()
}

pub fn cross(a: DVec2, b: DVec2) -> f64 {
    a.x * b.y - a.y * b.x
}

/// Left perpendicular.
pub fn perp(v: DVec2) -> DVec2 {
    DVec2::new(-v.y, v.x)
}

pub fn rotate(v: DVec2, ang: f64) -> DVec2 {
    let (s, c) = ang.sin_cos();
    DVec2::new(c * v.x - s * v.y, s * v.x + c * v.y)
}

/// Unsigned angle between two directions in radians.
pub fn angle_between(a: DVec2, b: DVec2) -> f64 {
    cross(a, b).atan2(a.dot(b)).abs()
}

/// Segment intersection: returns (t, u) along `a→b` and `c→d` when they intersect.
pub fn seg_intersect(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> Option<(f64, f64)> {
    let r = b - a;
    let s = d - c;
    let den = cross(r, s);
    if den.abs() < 1e-12 {
        return None;
    }
    let t = cross(c - a, s) / den;
    let u = cross(c - a, r) / den;
    if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
        Some((t, u))
    } else {
        None
    }
}

pub fn dist_point_seg(p: DVec2, a: DVec2, b: DVec2) -> (f64, f64) {
    let ab = b - a;
    let l2 = ab.length_squared();
    let t = if l2 < 1e-18 {
        0.0
    } else {
        ((p - a).dot(ab) / l2).clamp(0.0, 1.0)
    };
    (p.distance(a + ab * t), t)
}

pub fn point_in_poly(p: DVec2, poly: &[DVec2]) -> bool {
    let n = poly.len();
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

pub fn bbox(p: &[DVec2]) -> (DVec2, DVec2) {
    let mut lo = DVec2::splat(f64::INFINITY);
    let mut hi = DVec2::splat(f64::NEG_INFINITY);
    for v in p {
        lo = lo.min(*v);
        hi = hi.max(*v);
    }
    (lo, hi)
}

pub fn bbox_overlap(a: (DVec2, DVec2), b: (DVec2, DVec2), pad: f64) -> bool {
    a.0.x <= b.1.x + pad && b.0.x <= a.1.x + pad && a.0.y <= b.1.y + pad && b.0.y <= a.1.y + pad
}

pub fn convex_hull(pts: &[DVec2]) -> Poly {
    let mut p: Vec<DVec2> = pts.to_vec();
    p.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    if p.len() < 3 {
        return p;
    }
    let mut lower: Vec<DVec2> = Vec::new();
    for v in &p {
        while lower.len() >= 2
            && cross(
                lower[lower.len() - 1] - lower[lower.len() - 2],
                *v - lower[lower.len() - 2],
            ) <= 0.0
        {
            lower.pop();
        }
        lower.push(*v);
    }
    let mut upper: Vec<DVec2> = Vec::new();
    for v in p.iter().rev() {
        while upper.len() >= 2
            && cross(
                upper[upper.len() - 1] - upper[upper.len() - 2],
                *v - upper[upper.len() - 2],
            ) <= 0.0
        {
            upper.pop();
        }
        upper.push(*v);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// Minimum-area oriented bounding box: (area, edge direction).
pub fn min_obb(poly: &[DVec2]) -> (f64, DVec2) {
    let h = convex_hull(poly);
    let n = h.len();
    let mut best = (f64::INFINITY, DVec2::X);
    if n < 3 {
        return (0.0, DVec2::X);
    }
    for i in 0..n {
        let e = (h[(i + 1) % n] - h[i]).normalize_or_zero();
        if e == DVec2::ZERO {
            continue;
        }
        let nrm = perp(e);
        let (mut a0, mut a1, mut b0, mut b1) = (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        );
        for v in &h {
            let a = v.dot(e);
            let b = v.dot(nrm);
            a0 = a0.min(a);
            a1 = a1.max(a);
            b0 = b0.min(b);
            b1 = b1.max(b);
        }
        let ar = (a1 - a0) * (b1 - b0);
        if ar < best.0 {
            best = (ar, e);
        }
    }
    best
}

/// Area / min-OBB area.
pub fn rectangularity(poly: &[DVec2]) -> f64 {
    let (obb, _) = min_obb(poly);
    if obb <= 0.0 {
        0.0
    } else {
        area(poly).abs() / obb
    }
}

/// Smallest interior angle (degrees) at convex vertices — spikes.
pub fn min_interior_angle_deg(poly: &[DVec2]) -> f64 {
    let n = poly.len();
    let mut m = 180.0f64;
    for i in 0..n {
        let a = poly[(i + n - 1) % n];
        let b = poly[i];
        let c = poly[(i + 1) % n];
        let u = (a - b).normalize_or_zero();
        let v = (c - b).normalize_or_zero();
        if u == DVec2::ZERO || v == DVec2::ZERO {
            continue;
        }
        // Convex vertex in CCW polygon: cross(b-a, c-b) > 0.
        if cross(b - a, c - b) > 0.0 {
            m = m.min(angle_between(u, v).to_degrees());
        }
    }
    m
}

/// Removes vertices closer than `tol` to the line through their neighbours.
pub fn simplify(poly: &[DVec2], tol: f64) -> Poly {
    let mut p = poly.to_vec();
    let mut changed = true;
    while changed && p.len() > 3 {
        changed = false;
        let n = p.len();
        for i in 0..n {
            let a = p[(i + n - 1) % n];
            let b = p[i];
            let c = p[(i + 1) % n];
            let (d, _) = dist_point_seg(b, a, c);
            if d < tol {
                p.remove(i);
                changed = true;
                break;
            }
        }
    }
    p
}

/// Single-pass greedy simplification: drops vertices within `tol` of the chord between the
/// last kept vertex and the next one (open-curve style around the ring).
pub fn simplify_fast(poly: &[DVec2], tol: f64) -> Poly {
    let n = poly.len();
    if n <= 4 {
        return poly.to_vec();
    }
    let mut out: Poly = Vec::with_capacity(n);
    out.push(poly[0]);
    let mut anchor = 0;
    for i in 1..n {
        let next = poly[(i + 1) % n];
        // Keep i if any vertex between anchor and next deviates more than tol.
        let ok = (anchor + 1..=i).all(|k| dist_point_seg(poly[k], poly[anchor], next).0 < tol);
        if !ok {
            out.push(poly[i]);
            anchor = i;
        }
    }
    if out.len() < 3 { poly.to_vec() } else { out }
}

/// Outer contours of a boolean result (holes dropped — callers assert none).
fn outers(shapes: Vec<Vec<Vec<[f64; 2]>>>) -> (Vec<Poly>, usize) {
    let mut holes = 0;
    let out = shapes
        .into_iter()
        .filter_map(|s| {
            holes += s.len().saturating_sub(1);
            s.into_iter().next().map(|c| from_arr(&c))
        })
        .collect();
    (out, holes)
}

pub fn union_all(polys: &[Poly]) -> (Vec<Poly>, usize) {
    let subj: Vec<Vec<[f64; 2]>> = polys.iter().map(|p| to_arr(p)).collect();
    let empty: Vec<Vec<[f64; 2]>> = Vec::new();
    let r = subj
        .overlay_with_fixed_scale(&empty, OverlayRule::Subject, FillRule::NonZero, SCALE)
        .unwrap_or_default();
    outers(r)
}

pub fn intersect(a: &Poly, b: &Poly) -> Vec<Poly> {
    let r = to_arr(a)
        .overlay_with_fixed_scale(&to_arr(b), OverlayRule::Intersect, FillRule::NonZero, SCALE)
        .unwrap_or_default();
    outers(r).0
}

pub fn difference(a: &Poly, b: &[Poly]) -> Vec<Poly> {
    let clip: Vec<Vec<[f64; 2]>> = b.iter().map(|p| to_arr(p)).collect();
    let r = to_arr(a)
        .overlay_with_fixed_scale(&clip, OverlayRule::Difference, FillRule::NonZero, SCALE)
        .unwrap_or_default();
    outers(r).0
}

pub fn slice(poly: &Poly, lines: &[Vec<DVec2>]) -> (Vec<Poly>, usize) {
    let strings: Vec<Vec<[f64; 2]>> = lines.iter().map(|l| to_arr(l)).collect();
    let r = to_arr(poly)
        .slice_by_fixed_scale(&strings, FillRule::NonZero, SCALE)
        .unwrap_or_default();
    outers(r)
}

/// Regular n-gon approximating a disk.
pub fn disk(c: DVec2, r: f64, n: usize) -> Poly {
    (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            c + DVec2::new(a.cos(), a.sin()) * r
        })
        .collect()
}

thread_local! {
    pub static PROF: std::cell::RefCell<[f64; 10]> = const { std::cell::RefCell::new([0.0; 10]) };
}

/// Adds elapsed ms since `t` to profile slot `k` (spike instrumentation).
pub fn prof(k: usize, t: std::time::Instant) {
    PROF.with(|p| p.borrow_mut()[k] += t.elapsed().as_secs_f64() * 1e3);
}
