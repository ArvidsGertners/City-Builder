//! Segment geometry derived from the horizontal alignment and the cross-section: arc-length
//! table, 2 m centreline, left/right edges and the corridor polygon (DATA_MODEL §4.1).

use cb_geometry::{ArcLengthLut, CubicBezier2, Polyline2};
use glam::{Vec2, Vec3};

use crate::profile::Profile;

/// Centreline sampling for edges and corridors (m).
pub const CENTERLINE_STEP_M: f32 = 2.0;
/// Chord error tolerance of the sampled centreline (m).
pub const CENTERLINE_TOL_M: f32 = 0.02;

/// Derived geometry of one segment.
#[derive(Clone, Debug, PartialEq)]
pub struct SegmentGeom {
    /// Horizontal alignment.
    pub curve: CubicBezier2,
    /// Arc-length table.
    pub lut: ArcLengthLut,
    /// Sampled centreline.
    pub centerline: Polyline2,
    /// Left edge (positive offset).
    pub left: Polyline2,
    /// Right edge.
    pub right: Polyline2,
    /// Corridor polygon (left edge forward, right edge backward), counter-clockwise.
    pub corridor: Vec<Vec2>,
    /// Half width (m).
    pub half_width_m: f32,
}

impl SegmentGeom {
    /// Builds the geometry of a segment `half_width_m` wide on each side.
    #[must_use]
    pub fn new(curve: CubicBezier2, half_width_m: f32) -> Self {
        let lut = ArcLengthLut::new(&curve);
        let centerline = Polyline2::from_bezier(&curve, CENTERLINE_STEP_M, CENTERLINE_TOL_M);
        let left = centerline.offset(half_width_m);
        let right = centerline.offset(-half_width_m);
        let mut corridor: Vec<Vec2> = left.points().to_vec();
        corridor.extend(right.points().iter().rev());
        let corridor = cb_geometry::polygon::to_ccw(&corridor);
        Self {
            curve,
            lut,
            centerline,
            left,
            right,
            corridor,
            half_width_m,
        }
    }

    /// Length (m).
    #[must_use]
    pub fn length(&self) -> f32 {
        self.lut.length()
    }

    /// Point at arc length `s`.
    #[must_use]
    pub fn point_at(&self, s: f32) -> Vec2 {
        self.curve.eval(self.lut.t_at(s))
    }

    /// Unit direction at arc length `s`.
    #[must_use]
    pub fn dir_at(&self, s: f32) -> Vec2 {
        self.curve.tangent(self.lut.t_at(s))
    }

    /// 3D centreline every `step` metres with heights from `profile`.
    #[must_use]
    pub fn centerline3(&self, profile: &Profile, step: f32) -> Vec<Vec3> {
        let len = self.length();
        // Tolerance: an arc length of 100.0001 m at 10 m steps is 10 intervals, not 11.
        let n = ((len / step - 1e-3).ceil() as usize).max(1);
        (0..=n)
            .map(|k| {
                let s = len * k as f32 / n as f32;
                let p = self.point_at(s);
                Vec3::new(p.x, profile.height_at(s), p.y)
            })
            .collect()
    }

    /// Centreline points with profile heights over `[s0, s1]` at ≤ `step` spacing.
    #[must_use]
    pub fn centerline3_range(&self, profile: &Profile, s0: f32, s1: f32, step: f32) -> Vec<Vec3> {
        let (a, b) = (s0.max(0.0), s1.min(self.length()));
        if b <= a {
            return Vec::new();
        }
        let n = (((b - a) / step - 1e-3).ceil() as usize).max(1);
        (0..=n)
            .map(|k| {
                let s = a + (b - a) * k as f32 / n as f32;
                let p = self.point_at(s);
                Vec3::new(p.x, profile.height_at(s), p.y)
            })
            .collect()
    }

    /// Smallest turning radius (m) along the curve.
    #[must_use]
    pub fn min_radius_m(&self) -> f32 {
        1.0 / self.curve.max_curvature(64).max(1e-6)
    }

    /// Projection of a point: `(arc length, signed lateral offset)`.
    #[must_use]
    pub fn project(&self, p: Vec2) -> (f32, f32) {
        self.centerline.project(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_segment_geometry() {
        let g = SegmentGeom::new(
            CubicBezier2::straight(Vec2::ZERO, Vec2::new(100.0, 0.0)),
            6.0,
        );
        assert!((g.length() - 100.0).abs() < 1e-2);
        assert!((g.left.points()[0] - Vec2::new(0.0, 6.0)).length() < 1e-4);
        assert!((cb_geometry::polygon::area(&g.corridor) - 1200.0).abs() < 1.0);
        assert!(g.min_radius_m() > 1e5);
        let (s, d) = g.project(Vec2::new(30.0, -2.0));
        assert!((s - 30.0).abs() < 1e-3 && (d + 2.0).abs() < 1e-3);
        let c3 = g.centerline3(&Profile::flat(g.length(), 5.0), 10.0);
        assert_eq!(c3.len(), 11);
        assert!(c3.iter().all(|p| (p.y - 5.0).abs() < 1e-4));
    }
}
