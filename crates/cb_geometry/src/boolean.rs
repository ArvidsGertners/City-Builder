//! Polygon booleans via `i_overlay` on a 1 mm integer grid (ADR-0023, ALGORITHMS §10): results
//! are robust and identical on every platform. Holes are reported separately so callers can
//! reject or handle them.

use glam::{DVec2, Vec2};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::scale::FixedScaleFloatOverlay;
use i_overlay::float::slice::FloatSlice;

/// Snapping grid: 1 mm.
pub const SCALE: f64 = 1000.0;

/// A boolean result: outer contours (counter-clockwise) and the number of holes dropped.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shapes {
    /// Outer contours.
    pub outers: Vec<Vec<Vec2>>,
    /// Holes found (contours after the first per shape).
    pub holes: usize,
}

fn to_arr(p: &[Vec2]) -> Vec<[f64; 2]> {
    p.iter().map(|v| [f64::from(v.x), f64::from(v.y)]).collect()
}

fn from_shapes(shapes: Vec<Vec<Vec<[f64; 2]>>>) -> Shapes {
    let mut holes = 0;
    let outers = shapes
        .into_iter()
        .filter_map(|s| {
            holes += s.len().saturating_sub(1);
            s.into_iter().next().map(|c| {
                let v: Vec<Vec2> = c.iter().map(|a| DVec2::new(a[0], a[1]).as_vec2()).collect();
                crate::polygon::to_ccw(&v)
            })
        })
        .collect();
    Shapes { outers, holes }
}

fn run(subject: &[Vec<Vec2>], clip: &[Vec<Vec2>], rule: OverlayRule) -> Shapes {
    let s: Vec<Vec<[f64; 2]>> = subject.iter().map(|p| to_arr(p)).collect();
    let c: Vec<Vec<[f64; 2]>> = clip.iter().map(|p| to_arr(p)).collect();
    from_shapes(
        s.overlay_with_fixed_scale(&c, rule, FillRule::NonZero, SCALE)
            .unwrap_or_default(),
    )
}

/// Union of polygons.
#[must_use]
pub fn union(polys: &[Vec<Vec2>]) -> Shapes {
    run(polys, &[], OverlayRule::Subject)
}

/// Intersection `a ∩ b`.
#[must_use]
pub fn intersect(a: &[Vec2], b: &[Vec2]) -> Shapes {
    run(&[a.to_vec()], &[b.to_vec()], OverlayRule::Intersect)
}

/// Difference `a − ∪ clip`.
#[must_use]
pub fn difference(a: &[Vec2], clip: &[Vec<Vec2>]) -> Shapes {
    run(&[a.to_vec()], clip, OverlayRule::Difference)
}

/// Splits a polygon by open polylines (each line cuts where it crosses the polygon).
#[must_use]
pub fn slice(poly: &[Vec2], lines: &[Vec<Vec2>]) -> Shapes {
    let strings: Vec<Vec<[f64; 2]>> = lines.iter().map(|l| to_arr(l)).collect();
    from_shapes(
        to_arr(poly)
            .slice_by_fixed_scale(&strings, FillRule::NonZero, SCALE)
            .unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::polygon::area;

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<Vec2> {
        vec![
            Vec2::new(x0, y0),
            Vec2::new(x1, y0),
            Vec2::new(x1, y1),
            Vec2::new(x0, y1),
        ]
    }

    #[test]
    fn booleans_on_rectangles() {
        let a = rect(0.0, 0.0, 10.0, 10.0);
        let b = rect(5.0, 5.0, 15.0, 15.0);
        let i = intersect(&a, &b);
        assert_eq!(i.outers.len(), 1);
        assert!((area(&i.outers[0]) - 25.0).abs() < 1e-3);
        let u = union(&[a.clone(), b.clone()]);
        assert!((area(&u.outers[0]) - 175.0).abs() < 1e-3);
        let d = difference(&a, &[b]);
        assert!((area(&d.outers[0]) - 75.0).abs() < 1e-3);
        // A road strip across the plot splits it in two.
        let road = rect(-1.0, 4.0, 11.0, 6.0);
        let split = difference(&a, &[road]);
        assert_eq!(split.outers.len(), 2);
        assert_eq!(split.holes, 0);
        // A hole is reported.
        let hole = difference(&a, &[rect(4.0, 4.0, 6.0, 6.0)]);
        assert_eq!(hole.holes, 1);
    }

    #[test]
    fn slice_by_line() {
        let a = rect(0.0, 0.0, 10.0, 10.0);
        let s = slice(&a, &[vec![Vec2::new(4.0, -1.0), Vec2::new(4.0, 11.0)]]);
        assert_eq!(s.outers.len(), 2);
        let mut areas: Vec<f32> = s.outers.iter().map(|p| area(p)).collect();
        areas.sort_by(f32::total_cmp);
        assert!((areas[0] - 40.0).abs() < 1e-3 && (areas[1] - 60.0).abs() < 1e-3);
    }
}

/// `f64` boolean helpers for the plot/lot pipeline (same 1 mm grid). Results are outer
/// contours and the number of holes dropped.
pub mod f64ops {
    use glam::DVec2;
    use i_overlay::core::fill_rule::FillRule;
    use i_overlay::core::overlay_rule::OverlayRule;
    use i_overlay::float::scale::FixedScaleFloatOverlay;
    use i_overlay::float::slice::FloatSlice;

    use super::SCALE;
    use crate::d2::Poly;

    fn arr(p: &[DVec2]) -> Vec<[f64; 2]> {
        p.iter().map(|v| [v.x, v.y]).collect()
    }

    fn outers(shapes: Vec<Vec<Vec<[f64; 2]>>>) -> (Vec<Poly>, usize) {
        let mut holes = 0;
        let out = shapes
            .into_iter()
            .filter_map(|s| {
                holes += s.len().saturating_sub(1);
                s.into_iter()
                    .next()
                    .map(|c| c.iter().map(|a| DVec2::new(a[0], a[1])).collect())
            })
            .collect();
        (out, holes)
    }

    /// Union of polygons.
    #[must_use]
    pub fn union_all(polys: &[Poly]) -> (Vec<Poly>, usize) {
        let subj: Vec<Vec<[f64; 2]>> = polys.iter().map(|p| arr(p)).collect();
        let empty: Vec<Vec<[f64; 2]>> = Vec::new();
        outers(
            subj.overlay_with_fixed_scale(&empty, OverlayRule::Subject, FillRule::NonZero, SCALE)
                .unwrap_or_default(),
        )
    }

    /// Intersection.
    #[must_use]
    pub fn intersect(a: &Poly, b: &Poly) -> Vec<Poly> {
        outers(
            arr(a)
                .overlay_with_fixed_scale(&arr(b), OverlayRule::Intersect, FillRule::NonZero, SCALE)
                .unwrap_or_default(),
        )
        .0
    }

    /// Difference `a − ∪ b`.
    #[must_use]
    pub fn difference(a: &Poly, b: &[Poly]) -> Vec<Poly> {
        let clip: Vec<Vec<[f64; 2]>> = b.iter().map(|p| arr(p)).collect();
        outers(
            arr(a)
                .overlay_with_fixed_scale(&clip, OverlayRule::Difference, FillRule::NonZero, SCALE)
                .unwrap_or_default(),
        )
        .0
    }

    /// Slices a polygon by open polylines.
    #[must_use]
    pub fn slice(poly: &Poly, lines: &[Vec<DVec2>]) -> (Vec<Poly>, usize) {
        let strings: Vec<Vec<[f64; 2]>> = lines.iter().map(|l| arr(l)).collect();
        outers(
            arr(poly)
                .slice_by_fixed_scale(&strings, FillRule::NonZero, SCALE)
                .unwrap_or_default(),
        )
    }
}
