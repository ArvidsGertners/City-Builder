//! Simple polygons (`Vec<Vec2>`, implicitly closed): area, orientation, containment,
//! simplicity, distances, convex hull and minimum-area bounding rectangle.

use glam::Vec2;

use crate::predicates::segments_intersect;

/// Signed area (counter-clockwise positive).
#[must_use]
pub fn signed_area(p: &[Vec2]) -> f32 {
    let n = p.len();
    let mut a = 0.0f64;
    for i in 0..n {
        let (u, v) = (p[i], p[(i + 1) % n]);
        a += f64::from(u.x) * f64::from(v.y) - f64::from(v.x) * f64::from(u.y);
    }
    (0.5 * a) as f32
}

/// Absolute area.
#[must_use]
pub fn area(p: &[Vec2]) -> f32 {
    signed_area(p).abs()
}

/// Counter-clockwise?
#[must_use]
pub fn is_ccw(p: &[Vec2]) -> bool {
    signed_area(p) > 0.0
}

/// The polygon in counter-clockwise order.
#[must_use]
pub fn to_ccw(p: &[Vec2]) -> Vec<Vec2> {
    if is_ccw(p) {
        p.to_vec()
    } else {
        p.iter().rev().copied().collect()
    }
}

/// Perimeter.
#[must_use]
pub fn perimeter(p: &[Vec2]) -> f32 {
    (0..p.len())
        .map(|i| (p[(i + 1) % p.len()] - p[i]).length())
        .sum()
}

/// Area centroid.
#[must_use]
pub fn centroid(p: &[Vec2]) -> Vec2 {
    let n = p.len();
    let (mut cx, mut cy, mut a) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..n {
        let (u, v) = (p[i], p[(i + 1) % n]);
        let c = f64::from(u.x) * f64::from(v.y) - f64::from(v.x) * f64::from(u.y);
        a += c;
        cx += (f64::from(u.x) + f64::from(v.x)) * c;
        cy += (f64::from(u.y) + f64::from(v.y)) * c;
    }
    if a.abs() < 1e-12 {
        return p.iter().copied().sum::<Vec2>() / n.max(1) as f32;
    }
    Vec2::new((cx / (3.0 * a)) as f32, (cy / (3.0 * a)) as f32)
}

/// Axis-aligned bounds.
#[must_use]
pub fn bounds(p: &[Vec2]) -> (Vec2, Vec2) {
    p.iter().fold(
        (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
        |(lo, hi), v| (lo.min(*v), hi.max(*v)),
    )
}

/// Point in polygon (even-odd rule; boundary points may go either way).
#[must_use]
pub fn contains(p: &[Vec2], q: Vec2) -> bool {
    let n = p.len();
    let mut c = false;
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (p[i], p[j]);
        if (a.y > q.y) != (b.y > q.y) && q.x < (b.x - a.x) * (q.y - a.y) / (b.y - a.y) + a.x {
            c = !c;
        }
        j = i;
    }
    c
}

/// Distance from `q` to the polygon boundary.
#[must_use]
pub fn boundary_distance(p: &[Vec2], q: Vec2) -> f32 {
    let n = p.len();
    (0..n)
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % n]);
            let ab = b - a;
            let t = ((q - a).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0);
            (q - (a + ab * t)).length()
        })
        .fold(f32::INFINITY, f32::min)
}

/// Whether no two non-adjacent edges intersect (and there are ≥ 3 vertices with area).
#[must_use]
pub fn is_simple(p: &[Vec2]) -> bool {
    let n = p.len();
    if n < 3 || area(p) < 1e-6 {
        return false;
    }
    for i in 0..n {
        for j in i + 1..n {
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            if segments_intersect(p[i], p[(i + 1) % n], p[j], p[(j + 1) % n]) {
                return false;
            }
        }
    }
    true
}

/// Smallest interior angle in degrees (CCW polygon; reflex vertices count as > 180°).
#[must_use]
pub fn min_interior_angle_deg(p: &[Vec2]) -> f32 {
    let n = p.len();
    let ccw = to_ccw(p);
    (0..n)
        .map(|i| {
            let (a, b, c) = (ccw[(i + n - 1) % n], ccw[i], ccw[(i + 1) % n]);
            let (u, v) = ((a - b).normalize_or(Vec2::X), (c - b).normalize_or(Vec2::X));
            let ang = cb_core::math::to_degrees(cb_core::math::atan2(u.perp_dot(v), u.dot(v)));
            // Interior angle at b for a CCW polygon is the angle from v to u, measured CCW.
            if ang <= 0.0 { -ang } else { 360.0 - ang }
        })
        .fold(360.0, f32::min)
}

/// Convex hull (Andrew's monotone chain), counter-clockwise.
#[must_use]
pub fn convex_hull(pts: &[Vec2]) -> Vec<Vec2> {
    let mut v = pts.to_vec();
    v.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    v.dedup();
    if v.len() < 3 {
        return v;
    }
    let mut hull: Vec<Vec2> = Vec::with_capacity(v.len() * 2);
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &Vec2>> = if pass == 0 {
            Box::new(v.iter())
        } else {
            Box::new(v.iter().rev())
        };
        for p in iter {
            while hull.len() >= start + 2
                && (hull[hull.len() - 1] - hull[hull.len() - 2]).perp_dot(*p - hull[hull.len() - 2])
                    <= 0.0
            {
                hull.pop();
            }
            hull.push(*p);
        }
        hull.pop();
    }
    hull
}

/// Minimum-area enclosing rectangle (rotating calipers over hull edges):
/// `(area, axis direction (unit), corners CCW)`.
#[must_use]
pub fn min_area_rect(p: &[Vec2]) -> (f32, Vec2, [Vec2; 4]) {
    let hull = convex_hull(p);
    let mut best = (f32::INFINITY, Vec2::X, [Vec2::ZERO; 4]);
    let n = hull.len();
    for i in 0..n {
        let e = (hull[(i + 1) % n] - hull[i]).normalize_or(Vec2::X);
        let nrm = e.perp();
        let (mut a0, mut a1, mut b0, mut b1) = (
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
        );
        for q in &hull {
            let (a, b) = (q.dot(e), q.dot(nrm));
            a0 = a0.min(a);
            a1 = a1.max(a);
            b0 = b0.min(b);
            b1 = b1.max(b);
        }
        let ar = (a1 - a0) * (b1 - b0);
        if ar < best.0 {
            let c = |a: f32, b: f32| e * a + nrm * b;
            best = (ar, e, [c(a0, b0), c(a1, b0), c(a1, b1), c(a0, b1)]);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(s: f32) -> Vec<Vec2> {
        vec![
            Vec2::ZERO,
            Vec2::new(s, 0.0),
            Vec2::new(s, s),
            Vec2::new(0.0, s),
        ]
    }

    #[test]
    fn area_orientation_centroid() {
        let sq = square(10.0);
        assert_eq!(signed_area(&sq), 100.0);
        assert!(is_ccw(&sq));
        let cw: Vec<Vec2> = sq.iter().rev().copied().collect();
        assert!(!is_ccw(&cw));
        assert!(is_ccw(&to_ccw(&cw)));
        assert!((centroid(&sq) - Vec2::splat(5.0)).length() < 1e-5);
        assert_eq!(perimeter(&sq), 40.0);
    }

    #[test]
    fn containment_simplicity_angles() {
        let sq = square(10.0);
        assert!(contains(&sq, Vec2::new(5.0, 5.0)));
        assert!(!contains(&sq, Vec2::new(15.0, 5.0)));
        assert!(is_simple(&sq));
        let bow = vec![
            Vec2::ZERO,
            Vec2::new(10.0, 10.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(0.0, 10.0),
        ];
        assert!(!is_simple(&bow));
        assert!((min_interior_angle_deg(&sq) - 90.0).abs() < 1e-3);
        let tri = vec![Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0)];
        assert!((min_interior_angle_deg(&tri) - 45.0).abs() < 1e-3);
        assert!((boundary_distance(&sq, Vec2::new(3.0, 5.0)) - 3.0).abs() < 1e-5);
    }

    #[test]
    fn hull_and_min_rect() {
        let pts = [
            Vec2::ZERO,
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 4.0),
            Vec2::new(0.0, 4.0),
            Vec2::new(5.0, 2.0),
        ];
        let h = convex_hull(&pts);
        assert_eq!(h.len(), 4);
        assert!(is_ccw(&h));
        // A rotated 10 × 4 rectangle.
        let (s, c) = cb_core::math::sin_cos(0.5);
        let rot: Vec<Vec2> = pts
            .iter()
            .map(|p| Vec2::new(p.x * c - p.y * s, p.x * s + p.y * c))
            .collect();
        let (a, _, _) = min_area_rect(&rot);
        assert!((a - 40.0).abs() < 0.01, "{a}");
    }
}
