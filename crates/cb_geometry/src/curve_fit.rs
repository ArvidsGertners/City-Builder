//! Freehand stroke fitting (ALGORITHMS §1): Chaikin smoothing, then Schneider's algorithm —
//! least-squares cubic per span with chord-length parameters, Newton reparametrisation and
//! recursive splitting at the worst point with G1 tangents across splits.

use glam::Vec2;

use crate::bezier::CubicBezier2;

/// Chaikin corner cutting (endpoints kept).
#[must_use]
pub fn chaikin(pts: &[Vec2], iterations: u32) -> Vec<Vec2> {
    let mut route = pts.to_vec();
    for _ in 0..iterations {
        if route.len() < 3 {
            break;
        }
        let mut sm = vec![route[0]];
        for w in route.windows(2) {
            sm.push(w[0] * 0.75 + w[1] * 0.25);
            sm.push(w[0] * 0.25 + w[1] * 0.75);
        }
        sm.push(route[route.len() - 1]);
        route = sm;
    }
    route
}

/// Fits cubic Béziers through `pts` with maximum error `max_err` (m). Consecutive curves share
/// endpoints and tangent directions (G1).
#[must_use]
pub fn fit(pts: &[Vec2], max_err: f32) -> Vec<CubicBezier2> {
    let pts: Vec<Vec2> = {
        let mut v: Vec<Vec2> = Vec::with_capacity(pts.len());
        for p in pts {
            if v.last().is_none_or(|l| (*l - *p).length() > 1e-4) {
                v.push(*p);
            }
        }
        v
    };
    if pts.len() < 2 {
        return Vec::new();
    }
    if pts.len() == 2 {
        return vec![CubicBezier2::straight(pts[0], pts[1])];
    }
    let t0 = (pts[1] - pts[0]).normalize_or(Vec2::X);
    let t1 = (pts[pts.len() - 2] - pts[pts.len() - 1]).normalize_or(-Vec2::X);
    let mut out = Vec::new();
    fit_span(&pts, t0, t1, max_err, 0, &mut out);
    out
}

fn chord_params(pts: &[Vec2]) -> Vec<f32> {
    let mut u = vec![0.0];
    for w in pts.windows(2) {
        let last = *u.last().unwrap_or(&0.0);
        u.push(last + (w[1] - w[0]).length());
    }
    let total = u.last().copied().unwrap_or(1.0).max(1e-9);
    u.iter().map(|v| v / total).collect()
}

fn generate(pts: &[Vec2], u: &[f32], t0: Vec2, t1: Vec2) -> CubicBezier2 {
    let (first, last) = (pts[0], pts[pts.len() - 1]);
    let (mut c00, mut c01, mut c11, mut x0, mut x1) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for (p, &t) in pts.iter().zip(u) {
        let s = 1.0 - t;
        let (b0, b1, b2, b3) = (s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t);
        let (a0, a1) = (t0 * b1, t1 * b2);
        c00 += a0.dot(a0);
        c01 += a0.dot(a1);
        c11 += a1.dot(a1);
        let tmp = *p - (first * (b0 + b1) + last * (b2 + b3));
        x0 += a0.dot(tmp);
        x1 += a1.dot(tmp);
    }
    let det = c00 * c11 - c01 * c01;
    let chord = (last - first).length();
    let (mut al, mut ar) = if det.abs() > 1e-12 {
        ((x0 * c11 - x1 * c01) / det, (c00 * x1 - c01 * x0) / det)
    } else {
        (chord / 3.0, chord / 3.0)
    };
    if al < 1e-3 * chord || ar < 1e-3 * chord {
        al = chord / 3.0;
        ar = chord / 3.0;
    }
    CubicBezier2::new(first, first + t0 * al, last + t1 * ar, last)
}

fn max_error(c: &CubicBezier2, pts: &[Vec2], u: &[f32]) -> (f32, usize) {
    let mut worst = (0.0, pts.len() / 2);
    for (k, (p, &t)) in pts.iter().zip(u).enumerate() {
        let d = (c.eval(t) - *p).length();
        if d > worst.0 {
            worst = (d, k);
        }
    }
    worst
}

fn reparam(c: &CubicBezier2, pts: &[Vec2], u: &mut [f32]) {
    for (p, t) in pts.iter().zip(u.iter_mut()) {
        let r = c.eval(*t) - *p;
        let d1 = c.deriv(*t);
        let den = d1.length_squared() + r.dot(c.deriv2(*t));
        if den.abs() > 1e-12 {
            *t = (*t - r.dot(d1) / den).clamp(0.0, 1.0);
        }
    }
}

fn fit_span(
    pts: &[Vec2],
    t0: Vec2,
    t1: Vec2,
    max_err: f32,
    depth: u32,
    out: &mut Vec<CubicBezier2>,
) {
    if pts.len() == 2 {
        let d = (pts[1] - pts[0]).length() / 3.0;
        out.push(CubicBezier2::new(
            pts[0],
            pts[0] + t0 * d,
            pts[1] + t1 * d,
            pts[1],
        ));
        return;
    }
    let mut u = chord_params(pts);
    let mut c = generate(pts, &u, t0, t1);
    let (mut err, mut split) = max_error(&c, pts, &u);
    for _ in 0..4 {
        if err <= max_err {
            break;
        }
        reparam(&c, pts, &mut u);
        c = generate(pts, &u, t0, t1);
        (err, split) = max_error(&c, pts, &u);
    }
    if err <= max_err || depth > 24 {
        out.push(c);
        return;
    }
    let split = split.clamp(1, pts.len() - 2);
    let tc = (pts[split - 1] - pts[split + 1]).normalize_or(-t0);
    fit_span(&pts[..=split], t0, tc, max_err, depth + 1, out);
    fit_span(&pts[split..], -tc, t1, max_err, depth + 1, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_a_noisy_arc_within_tolerance() {
        let pts: Vec<Vec2> = (0..=90)
            .map(|k| {
                let a = k as f32 / 90.0 * 3.0;
                let (s, c) = cb_core::math::sin_cos(a);
                Vec2::new(c, s) * 80.0 + Vec2::splat(((k * 7919) % 13) as f32 * 0.02)
            })
            .collect();
        let sm = chaikin(&pts, 2);
        let curves = fit(&sm, 0.75);
        assert!(
            !curves.is_empty() && curves.len() < 12,
            "{} curves",
            curves.len()
        );
        // Endpoints and G1 continuity.
        assert!((curves[0].start() - sm[0]).length() < 1e-4);
        assert!((curves[curves.len() - 1].end() - sm[sm.len() - 1]).length() < 1e-4);
        for w in curves.windows(2) {
            assert!((w[0].end() - w[1].start()).length() < 1e-4);
            assert!(w[0].tangent(1.0).dot(w[1].tangent(0.0)) > 0.999);
        }
        for p in &sm {
            let d = curves
                .iter()
                .map(|c| c.nearest(*p).1)
                .fold(f32::INFINITY, f32::min);
            assert!(d < 0.8, "{d}");
        }
    }

    #[test]
    fn two_points_make_a_straight() {
        let c = fit(&[Vec2::ZERO, Vec2::new(10.0, 0.0)], 0.5);
        assert_eq!(c.len(), 1);
        assert!(c[0].max_curvature(8) < 1e-6);
    }
}
