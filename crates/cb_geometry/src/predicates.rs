//! Orientation and intersection predicates. Computed in `f64` with a relative epsilon so
//! nearly-collinear cases are classified consistently (no adaptive exact arithmetic needed at
//! map scale: coordinates ≤ 2 048 m, features ≥ 1 cm).

use glam::Vec2;

/// Relative epsilon for orientation tests.
const EPS: f64 = 1e-12;

/// Orientation of `c` relative to `a → b`: +1 left (counter-clockwise), −1 right, 0 collinear.
#[must_use]
pub fn orient(a: Vec2, b: Vec2, c: Vec2) -> i8 {
    let (ax, ay) = (f64::from(a.x), f64::from(a.y));
    let d = (f64::from(b.x) - ax) * (f64::from(c.y) - ay)
        - (f64::from(b.y) - ay) * (f64::from(c.x) - ax);
    let scale = (f64::from(b.x) - ax).abs().max((f64::from(b.y) - ay).abs())
        * (f64::from(c.x) - ax).abs().max((f64::from(c.y) - ay).abs());
    if d.abs() <= EPS * scale.max(1.0) {
        0
    } else if d > 0.0 {
        1
    } else {
        -1
    }
}

fn on_segment(a: Vec2, b: Vec2, p: Vec2) -> bool {
    p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}

/// Whether segments `ab` and `cd` intersect (touching counts).
#[must_use]
pub fn segments_intersect(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let (o1, o2, o3, o4) = (
        orient(a, b, c),
        orient(a, b, d),
        orient(c, d, a),
        orient(c, d, b),
    );
    if o1 != o2 && o3 != o4 && o1 != 0 && o2 != 0 && o3 != 0 && o4 != 0 {
        return true;
    }
    (o1 == 0 && on_segment(a, b, c))
        || (o2 == 0 && on_segment(a, b, d))
        || (o3 == 0 && on_segment(c, d, a))
        || (o4 == 0 && on_segment(c, d, b))
}

/// Proper intersection parameters `(t on ab, u on cd)` for non-parallel segments.
#[must_use]
pub fn segment_intersection(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Option<(f32, f32)> {
    let r = b - a;
    let s = d - c;
    let den = r.perp_dot(s);
    if den.abs() < 1e-12 {
        return None;
    }
    let t = (c - a).perp_dot(s) / den;
    let u = (c - a).perp_dot(r) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some((t, u))
}

/// Distance from `p` to segment `ab` and the parameter of the closest point.
#[must_use]
pub fn point_segment(p: Vec2, a: Vec2, b: Vec2) -> (f32, f32) {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0);
    ((p - (a + ab * t)).length(), t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orientation_and_intersection() {
        let (a, b) = (Vec2::ZERO, Vec2::new(10.0, 0.0));
        assert_eq!(orient(a, b, Vec2::new(5.0, 1.0)), 1);
        assert_eq!(orient(a, b, Vec2::new(5.0, -1.0)), -1);
        assert_eq!(orient(a, b, Vec2::new(20.0, 0.0)), 0);
        assert!(segments_intersect(
            a,
            b,
            Vec2::new(5.0, -5.0),
            Vec2::new(5.0, 5.0)
        ));
        assert!(
            segments_intersect(a, b, Vec2::new(10.0, 0.0), Vec2::new(12.0, 5.0)),
            "touching"
        );
        assert!(!segments_intersect(
            a,
            b,
            Vec2::new(11.0, -5.0),
            Vec2::new(11.0, 5.0)
        ));
        let (t, u) =
            segment_intersection(a, b, Vec2::new(4.0, -2.0), Vec2::new(4.0, 2.0)).expect("cross");
        assert!((t - 0.4).abs() < 1e-6 && (u - 0.5).abs() < 1e-6);
        assert_eq!(point_segment(Vec2::new(3.0, 4.0), a, b), (4.0, 0.3));
    }
}
