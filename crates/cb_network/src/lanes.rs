//! Lane generation (ALGORITHMS §4, ADR-0008): per segment, one 3D centreline per driving lane
//! over the range left after the junction cut-backs, in travel direction, with cumulative arc
//! length and the tightest radius (for curve speed caps and the min-radius assertion).

use glam::{Vec2, Vec3};

use crate::cross_section::{CrossSection, Dir, LaneSpec};
use crate::profile::Profile;
use crate::segment_geom::SegmentGeom;

/// Sample spacing on straight-ish stretches (m).
pub const LANE_STEP_STRAIGHT_M: f32 = 2.0;
/// Sample spacing where the alignment turns (m).
pub const LANE_STEP_CURVE_M: f32 = 1.0;
/// Heading change over a straight step above which the curve step is used (rad).
pub const LANE_CURVE_TURN_RAD: f32 = 0.03;
/// Lateral acceleration for curve speed caps (m/s²): `v ≤ sqrt(a · r)`.
pub const CURVE_LAT_ACCEL_MPS2: f32 = 2.5;

/// One lane of a segment, in travel direction.
#[derive(Clone, Debug, PartialEq)]
pub struct LanePath {
    /// Cross-section lane.
    pub spec: LaneSpec,
    /// Centreline points (x, surface height, z), in travel direction.
    pub points: Vec<Vec3>,
    /// Cumulative plan arc length per point (m).
    pub cum: Vec<f32>,
    /// Tightest circumradius of consecutive samples (m; `INFINITY` when straight).
    pub min_radius_m: f32,
}

impl LanePath {
    /// Plan length (m).
    #[must_use]
    pub fn length(&self) -> f32 {
        self.cum.last().copied().unwrap_or(0.0)
    }

    /// Point at arc length `s` (clamped).
    #[must_use]
    pub fn point_at(&self, s: f32) -> Vec3 {
        let n = self.points.len();
        if n == 0 {
            return Vec3::ZERO;
        }
        let s = s.clamp(0.0, self.length());
        let k = self.cum.partition_point(|c| *c <= s).clamp(1, n.max(2) - 1);
        if n == 1 {
            return self.points[0];
        }
        let (c0, c1) = (self.cum[k - 1], self.cum[k]);
        let t = if c1 > c0 { (s - c0) / (c1 - c0) } else { 0.0 };
        self.points[k - 1].lerp(self.points[k], t)
    }

    /// Plan direction at the start (unit).
    #[must_use]
    pub fn start_dir(&self) -> Vec2 {
        plan_dir(&self.points, 0)
    }

    /// Plan direction at the end (unit).
    #[must_use]
    pub fn end_dir(&self) -> Vec2 {
        plan_dir(&self.points, self.points.len().saturating_sub(2))
    }

    /// Curve speed cap (m/s) from the tightest radius.
    #[must_use]
    pub fn curve_speed_cap_mps(&self) -> f32 {
        (CURVE_LAT_ACCEL_MPS2 * self.min_radius_m.min(1e6)).sqrt()
    }
}

fn plan_dir(p: &[Vec3], k: usize) -> Vec2 {
    if p.len() < 2 {
        return Vec2::X;
    }
    let k = k.min(p.len() - 2);
    let d = Vec2::new(p[k + 1].x - p[k].x, p[k + 1].z - p[k].z);
    d.try_normalize().unwrap_or(Vec2::X)
}

/// Circumradius of three plan points (`INFINITY` when collinear).
fn circumradius(a: Vec2, b: Vec2, c: Vec2) -> f32 {
    let (ab, bc, ca) = (a.distance(b), b.distance(c), c.distance(a));
    let area2 = (b - a).perp_dot(c - a).abs();
    if area2 < 1e-9 {
        return f32::INFINITY;
    }
    ab * bc * ca / (2.0 * area2)
}

/// Centreline arc positions in `[s0, s1]`: 2 m steps, 1 m where the alignment turns, both ends
/// included.
#[must_use]
pub fn sample_positions(geom: &SegmentGeom, s0: f32, s1: f32) -> Vec<f32> {
    let mut out = vec![s0];
    let mut s = s0;
    while s < s1 - 1e-3 {
        let d0 = geom.dir_at(s);
        let probe = (s + LANE_STEP_STRAIGHT_M).min(s1);
        let turn = cb_core::math::acos(d0.dot(geom.dir_at(probe)).clamp(-1.0, 1.0));
        let step = if turn > LANE_CURVE_TURN_RAD {
            LANE_STEP_CURVE_M
        } else {
            LANE_STEP_STRAIGHT_M
        };
        s = (s + step).min(s1);
        // Avoid a sliver step at the end.
        if s1 - s < 0.25 * step && s < s1 {
            s = s1;
        }
        out.push(s);
    }
    out
}

/// Lanes of one segment over `[cut.0, length − cut.1]` (cut-backs at the start and end node).
/// Forward lanes run along the curve, backward lanes against it.
#[must_use]
pub fn segment_lanes(
    geom: &SegmentGeom,
    profile: &Profile,
    cs: &CrossSection,
    cut: (f32, f32),
) -> Vec<LanePath> {
    let len = geom.length();
    let (s0, s1) = (cut.0.clamp(0.0, len), (len - cut.1).clamp(0.0, len));
    if s1 <= s0 {
        return Vec::new();
    }
    let ss = sample_positions(geom, s0, s1);
    let frames: Vec<(Vec2, Vec2, f32)> = ss
        .iter()
        .map(|&s| {
            (
                geom.point_at(s),
                geom.dir_at(s).perp(),
                profile.height_at(s),
            )
        })
        .collect();
    cs.lanes()
        .into_iter()
        .map(|spec| {
            let mut pts: Vec<Vec3> = frames
                .iter()
                .map(|(p, n, h)| {
                    let q = *p + *n * spec.offset_m;
                    Vec3::new(q.x, *h, q.y)
                })
                .collect();
            if spec.dir == Dir::Backward {
                pts.reverse();
            }
            let mut cum = Vec::with_capacity(pts.len());
            let mut acc = 0.0;
            for (k, p) in pts.iter().enumerate() {
                if k > 0 {
                    let q = pts[k - 1];
                    acc += Vec2::new(p.x - q.x, p.z - q.z).length();
                }
                cum.push(acc);
            }
            let plan: Vec<Vec2> = pts.iter().map(|p| Vec2::new(p.x, p.z)).collect();
            let min_radius_m = plan
                .windows(3)
                .map(|w| circumradius(w[0], w[1], w[2]))
                .fold(f32::INFINITY, f32::min);
            LanePath {
                spec,
                points: pts,
                cum,
                min_radius_m,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cross_section::{LaneKind, RoadKind, Variant};
    use cb_geometry::CubicBezier2;

    fn street(curve: CubicBezier2) -> (SegmentGeom, Profile, CrossSection) {
        let cs = CrossSection::of(RoadKind::Street);
        let g = SegmentGeom::new(curve, cs.half_width_m());
        let p = Profile::flat(g.length(), 5.0);
        (g, p, cs)
    }

    #[test]
    fn straight_lanes_are_offset_trimmed_and_directed() {
        let (g, p, cs) = street(CubicBezier2::straight(Vec2::ZERO, Vec2::new(100.0, 0.0)));
        let lanes = segment_lanes(&g, &p, &cs, (10.0, 20.0));
        assert_eq!(lanes.len(), 2);
        let f = lanes.iter().find(|l| l.spec.dir == Dir::Forward).unwrap();
        let b = lanes.iter().find(|l| l.spec.dir == Dir::Backward).unwrap();
        // Forward lane right of the curve (−z side for a +x road: perp of +x is +z).
        assert!((f.points[0] - Vec3::new(10.0, 5.0, -2.0)).length() < 1e-4);
        assert!((f.points.last().unwrap().x - 80.0).abs() < 1e-4);
        // Backward lane runs from the end back to the start on the +z side.
        assert!((b.points[0] - Vec3::new(80.0, 5.0, 2.0)).length() < 1e-4);
        assert!((b.start_dir() - Vec2::new(-1.0, 0.0)).length() < 1e-4);
        assert!((f.length() - 70.0).abs() < 1e-3 && (b.length() - 70.0).abs() < 1e-3);
        assert!(f.min_radius_m > 1e5);
        assert!((f.point_at(35.0).x - 45.0).abs() < 1e-3);
    }

    #[test]
    fn curved_lane_radius_follows_offset() {
        // Quarter circle of radius 60 approximated by a cubic Bézier.
        let r = 60.0;
        let k = 0.552_284_8 * r;
        let curve = CubicBezier2::new(
            Vec2::new(r, 0.0),
            Vec2::new(r, k),
            Vec2::new(k, r),
            Vec2::new(0.0, r),
        );
        let (g, p, cs) = street(curve);
        let lanes = segment_lanes(&g, &p, &cs, (0.0, 0.0));
        // The curve turns left (counter-clockwise): the backward lane (left side, +2 m) is
        // on the inside.
        let inner = lanes.iter().find(|l| l.spec.dir == Dir::Backward).unwrap();
        let outer = lanes.iter().find(|l| l.spec.dir == Dir::Forward).unwrap();
        assert!(
            (inner.min_radius_m - 58.0).abs() < 1.5,
            "{}",
            inner.min_radius_m
        );
        assert!(
            (outer.min_radius_m - 62.0).abs() < 1.5,
            "{}",
            outer.min_radius_m
        );
        // Curve sampling is ~1 m.
        let steps: Vec<f32> = inner.cum.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(steps.iter().all(|d| *d <= 1.3), "{steps:?}");
        assert!((inner.curve_speed_cap_mps() - (2.5f32 * 58.0).sqrt()).abs() < 0.2);
    }

    #[test]
    fn heights_follow_profile_and_variants_carry_kinds() {
        let cs = CrossSection::with(
            RoadKind::Avenue,
            Variant {
                bus_lanes: true,
                tram_tracks: true,
            },
        );
        let g = SegmentGeom::new(
            CubicBezier2::straight(Vec2::ZERO, Vec2::new(80.0, 0.0)),
            cs.half_width_m(),
        );
        let p = Profile::new(vec![0.0, 80.0], vec![0.0, 8.0]);
        let lanes = segment_lanes(&g, &p, &cs, (0.0, 0.0));
        assert_eq!(lanes.len(), 4);
        for l in &lanes {
            for q in &l.points {
                assert!((q.y - 0.1 * q.x).abs() < 0.05, "{q}");
            }
        }
        let kinds: Vec<LaneKind> = lanes.iter().map(|l| l.spec.kind).collect();
        assert_eq!(kinds.iter().filter(|k| **k == LaneKind::Bus).count(), 2);
        assert_eq!(kinds.iter().filter(|k| k.trams()).count(), 2);
    }

    #[test]
    fn degenerate_cut_gives_no_lanes() {
        let (g, p, cs) = street(CubicBezier2::straight(Vec2::ZERO, Vec2::new(20.0, 0.0)));
        assert!(segment_lanes(&g, &p, &cs, (12.0, 12.0)).is_empty());
        let tiny = segment_lanes(&g, &p, &cs, (9.9, 9.9));
        assert!(tiny.iter().all(|l| l.points.len() >= 2 && l.length() > 0.1));
    }
}
