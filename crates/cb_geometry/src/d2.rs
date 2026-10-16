//! `f64` planar helpers for the plot/lot pipeline, where slicing and merging chains of booleans
//! need the extra precision (inputs and outputs are converted at the API boundary).

use glam::{DVec2, Vec2};

/// Polygon in `f64`.
pub type Poly = Vec<DVec2>;

/// 2D cross product.
#[inline]
#[must_use]
pub fn cross(a: DVec2, b: DVec2) -> f64 {
    a.x * b.y - a.y * b.x
}

/// Left perpendicular.
#[inline]
#[must_use]
pub fn perp(v: DVec2) -> DVec2 {
    DVec2::new(-v.y, v.x)
}

/// Rotates `v` by `ang` radians (counter-clockwise).
#[must_use]
pub fn rotate(v: DVec2, ang: f64) -> DVec2 {
    let (s, c) = cb_core::math::sin_cos_f64(ang);
    DVec2::new(c * v.x - s * v.y, s * v.x + c * v.y)
}

/// Signed angle from `a` to `b` (radians, counter-clockwise positive).
#[must_use]
pub fn signed_angle(a: DVec2, b: DVec2) -> f64 {
    cb_core::math::atan2_f64(cross(a, b), a.dot(b))
}

/// Unsigned angle between two directions.
#[must_use]
pub fn angle_between(a: DVec2, b: DVec2) -> f64 {
    signed_angle(a, b).abs()
}

/// Segment intersection parameters `(t on ab, u on cd)`.
#[must_use]
pub fn seg_intersect(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> Option<(f64, f64)> {
    let r = b - a;
    let s = d - c;
    let den = cross(r, s);
    if den.abs() < 1e-12 {
        return None;
    }
    let t = cross(c - a, s) / den;
    let u = cross(c - a, r) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some((t, u))
}

/// Distance from `p` to segment `ab` and the closest-point parameter.
#[must_use]
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

/// Signed area (counter-clockwise positive).
#[must_use]
pub fn signed_area(p: &[DVec2]) -> f64 {
    let n = p.len();
    (0..n).map(|i| cross(p[i], p[(i + 1) % n])).sum::<f64>() * 0.5
}

/// Absolute area.
#[must_use]
pub fn area(p: &[DVec2]) -> f64 {
    signed_area(p).abs()
}

/// Perimeter.
#[must_use]
pub fn perimeter(p: &[DVec2]) -> f64 {
    (0..p.len())
        .map(|i| p[i].distance(p[(i + 1) % p.len()]))
        .sum()
}

/// Point in polygon (even-odd).
#[must_use]
pub fn point_in_poly(q: DVec2, p: &[DVec2]) -> bool {
    let n = p.len();
    let mut inside = false;
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (p[i], p[j]);
        if (a.y > q.y) != (b.y > q.y) && q.x < (b.x - a.x) * (q.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Axis-aligned bounds.
#[must_use]
pub fn bbox(p: &[DVec2]) -> (DVec2, DVec2) {
    p.iter().fold(
        (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
        |(lo, hi), v| (lo.min(*v), hi.max(*v)),
    )
}

/// Whether two bounds overlap (with padding).
#[must_use]
pub fn bbox_overlap(a: (DVec2, DVec2), b: (DVec2, DVec2), pad: f64) -> bool {
    a.0.x <= b.1.x + pad && b.0.x <= a.1.x + pad && a.0.y <= b.1.y + pad && b.0.y <= a.1.y + pad
}

/// Convex hull (monotone chain), counter-clockwise.
#[must_use]
pub fn convex_hull(pts: &[DVec2]) -> Poly {
    let mut p = pts.to_vec();
    p.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    if p.len() < 3 {
        return p;
    }
    let mut lower: Poly = Vec::new();
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
    let mut upper: Poly = Vec::new();
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

/// Minimum-area oriented bounding box: `(area, edge direction)`.
#[must_use]
pub fn min_obb(poly: &[DVec2]) -> (f64, DVec2) {
    let h = convex_hull(poly);
    let n = h.len();
    if n < 3 {
        return (0.0, DVec2::X);
    }
    let mut best = (f64::INFINITY, DVec2::X);
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
            let (a, b) = (v.dot(e), v.dot(nrm));
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

/// Rectangularity: area / minimum oriented bounding box area.
#[must_use]
pub fn rectangularity(poly: &[DVec2]) -> f64 {
    let (obb, _) = min_obb(poly);
    if obb <= 0.0 { 0.0 } else { area(poly) / obb }
}

/// Smallest interior angle (degrees) at convex vertices of a counter-clockwise polygon.
#[must_use]
pub fn min_convex_angle_deg(poly: &[DVec2]) -> f64 {
    let n = poly.len();
    let mut m = 180.0f64;
    for i in 0..n {
        let (a, b, c) = (poly[(i + n - 1) % n], poly[i], poly[(i + 1) % n]);
        let (u, v) = ((a - b).normalize_or_zero(), (c - b).normalize_or_zero());
        if u == DVec2::ZERO || v == DVec2::ZERO {
            continue;
        }
        if cross(b - a, c - b) > 0.0 {
            m = m.min(angle_between(u, v).to_degrees());
        }
    }
    m
}

/// Removes vertices closer than `tol` to the chord of their neighbours (repeatedly).
#[must_use]
pub fn simplify(poly: &[DVec2], tol: f64) -> Poly {
    let mut p = poly.to_vec();
    let mut changed = true;
    while changed && p.len() > 3 {
        changed = false;
        let n = p.len();
        for i in 0..n {
            let (a, b, c) = (p[(i + n - 1) % n], p[i], p[(i + 1) % n]);
            if dist_point_seg(b, a, c).0 < tol {
                p.remove(i);
                changed = true;
                break;
            }
        }
    }
    p
}

/// Single-pass greedy simplification: drops vertices within `tol` of the chord between the
/// last kept vertex and the next one (cheap pre-pass for fitting).
#[must_use]
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
        if !(anchor + 1..=i).all(|k| dist_point_seg(poly[k], poly[anchor], next).0 < tol) {
            out.push(poly[i]);
            anchor = i;
        }
    }
    if out.len() < 3 { poly.to_vec() } else { out }
}

/// Converts to `f32` world coordinates.
#[must_use]
pub fn to_f32(p: &[DVec2]) -> Vec<Vec2> {
    p.iter().map(|v| v.as_vec2()).collect()
}

/// Converts from `f32` world coordinates.
#[must_use]
pub fn to_f64(p: &[Vec2]) -> Poly {
    p.iter().map(|v| v.as_dvec2()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangularity_and_angles() {
        let sq = vec![
            DVec2::ZERO,
            DVec2::new(10.0, 0.0),
            DVec2::new(10.0, 5.0),
            DVec2::new(0.0, 5.0),
        ];
        assert!((rectangularity(&sq) - 1.0).abs() < 1e-9);
        let tri = vec![DVec2::ZERO, DVec2::new(10.0, 0.0), DVec2::new(0.0, 10.0)];
        assert!((rectangularity(&tri) - 0.5).abs() < 1e-9);
        assert!((min_convex_angle_deg(&tri) - 45.0).abs() < 1e-6);
        assert!((rotate(DVec2::X, std::f64::consts::FRAC_PI_2) - DVec2::Y).length() < 1e-12);
        assert!((signed_angle(DVec2::X, DVec2::Y) - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }
}
