//! Road placement validation (ALGORITHMS §1) with typed reasons. Every reason carries its
//! localization key (`road-invalid-*` in `cb_l10n/locales/*/game.ftl`) and a location so the
//! tool preview can point at the problem. Pure: the caller supplies terrain queries and the
//! nearby existing segments.

use cb_geometry::polyline::Polyline2;
use glam::Vec2;

use crate::profile::{self, Profile, ProfileError, ProfileInput};
use crate::segment_geom::SegmentGeom;

/// Minimum segment length (m).
pub const MIN_LENGTH_M: f32 = 6.0;
/// Crossings with |Δh| below this become junctions (m).
pub const JUNCTION_MAX_DH_M: f32 = 2.5;
/// Crossings with |Δh| at least this are grade-separated (m).
pub const OVERPASS_MIN_DH_M: f32 = 5.5;

/// Why a road cannot be placed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Invalid {
    /// Too steep at arc length `s_m`.
    TooSteep {
        /// Position.
        at: Vec2,
    },
    /// Curve tighter than the type's minimum radius.
    TooSharp {
        /// Position.
        at: Vec2,
        /// Actual radius (m).
        radius_m: f32,
    },
    /// Shorter than [`MIN_LENGTH_M`].
    TooShort,
    /// Overlaps an existing corridor without a crossing.
    Overlap {
        /// Position.
        at: Vec2,
    },
    /// Crosses water without a bridge.
    WaterNoBridge {
        /// Position.
        at: Vec2,
    },
    /// Crossing an existing road with a height difference between junction and overpass.
    CrossingClearance {
        /// Position.
        at: Vec2,
        /// Height difference (m).
        dh_m: f32,
    },
    /// Leaves the map.
    OutOfBounds {
        /// Position.
        at: Vec2,
    },
}

impl Invalid {
    /// Localization key of the reason.
    #[must_use]
    pub fn key(&self) -> &'static str {
        match self {
            Self::TooSteep { .. } => "road-invalid-too-steep",
            Self::TooSharp { .. } => "road-invalid-too-sharp",
            Self::TooShort => "road-invalid-too-short",
            Self::Overlap { .. } => "road-invalid-overlap",
            Self::WaterNoBridge { .. } => "road-invalid-water-no-bridge",
            Self::CrossingClearance { .. } => "road-invalid-crossing-clearance",
            Self::OutOfBounds { .. } => "road-invalid-out-of-bounds",
        }
    }

    /// Location to highlight (`None` for whole-road reasons).
    #[must_use]
    pub fn at(&self) -> Option<Vec2> {
        match self {
            Self::TooSteep { at }
            | Self::TooSharp { at, .. }
            | Self::Overlap { at }
            | Self::WaterNoBridge { at }
            | Self::CrossingClearance { at, .. }
            | Self::OutOfBounds { at } => Some(*at),
            Self::TooShort => None,
        }
    }
}

/// Road type limits used by validation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoadLimits {
    /// Half width (m).
    pub half_width_m: f32,
    /// Maximum grade.
    pub max_grade: f32,
    /// Minimum radius (m).
    pub min_radius_m: f32,
}

/// Terrain queries needed by validation.
pub trait Ground {
    /// Ground height (terrain without the new road).
    fn height(&self, p: Vec2) -> f32;
    /// Water depth (0 when dry).
    fn water_depth(&self, p: Vec2) -> f32;
    /// Half extent of the map (m).
    fn half_extent(&self) -> f32;
}

/// An existing segment near the new road.
#[derive(Clone, Debug)]
pub struct Existing<'a> {
    /// Its geometry.
    pub geom: &'a SegmentGeom,
    /// Its profile.
    pub profile: &'a Profile,
    /// Whether the new road starts or ends on one of its nodes (shared endpoints are fine).
    pub shares_node_at: Option<Vec2>,
}

/// How the new road meets an existing one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crossing {
    /// Index into the `existing` slice.
    pub other: usize,
    /// Arc length on the new road.
    pub s_new: f32,
    /// Arc length on the existing road.
    pub s_other: f32,
    /// Crossing point.
    pub at: Vec2,
    /// Height difference (new − existing) before resolution.
    pub dh_m: f32,
}

/// Crossing kinds by |Δh|.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrossingKind {
    /// Becomes a junction (heights merged).
    Junction,
    /// Grade-separated (bridge span).
    Overpass,
    /// Between the two: invalid.
    Invalid,
}

/// Classifies a crossing by height difference.
#[must_use]
pub fn classify(dh_m: f32) -> CrossingKind {
    if dh_m.abs() < JUNCTION_MAX_DH_M {
        CrossingKind::Junction
    } else if dh_m.abs() >= OVERPASS_MIN_DH_M {
        CrossingKind::Overpass
    } else {
        CrossingKind::Invalid
    }
}

/// Intersections of the new centreline with existing centrelines (not at shared nodes).
#[must_use]
pub fn find_crossings(
    new: &SegmentGeom,
    new_profile: &Profile,
    existing: &[Existing<'_>],
) -> Vec<Crossing> {
    let mut out = Vec::new();
    let a = new.centerline.points();
    let ac = new.centerline.cumulative();
    for (k, ex) in existing.iter().enumerate() {
        let b = ex.geom.centerline.points();
        let bc = ex.geom.centerline.cumulative();
        for i in 0..a.len().saturating_sub(1) {
            for j in 0..b.len().saturating_sub(1) {
                if let Some((t, u)) =
                    cb_geometry::predicates::segment_intersection(a[i], a[i + 1], b[j], b[j + 1])
                {
                    let at = a[i].lerp(a[i + 1], t);
                    if ex.shares_node_at.is_some_and(|n| (n - at).length() < 1.0) {
                        continue;
                    }
                    let s_new = ac[i] + t * (ac[i + 1] - ac[i]);
                    let s_other = bc[j] + u * (bc[j + 1] - bc[j]);
                    let dh_m = new_profile.height_at(s_new) - ex.profile.height_at(s_other);
                    out.push(Crossing {
                        other: k,
                        s_new,
                        s_other,
                        at,
                        dh_m,
                    });
                }
            }
        }
    }
    out.sort_by(|x, y| x.s_new.total_cmp(&y.s_new));
    out
}

/// Profile input for a new road over `ground` with endpoint heights.
#[must_use]
pub fn ground_input(
    geom: &SegmentGeom,
    ground: &dyn Ground,
    h0: f32,
    h1: f32,
    limits: &RoadLimits,
) -> ProfileInput {
    let s = profile::sample_positions(geom.length());
    let g: Vec<f32> = s.iter().map(|x| ground.height(geom.point_at(*x))).collect();
    ProfileInput::ground(s, g, h0, h1, limits.max_grade)
}

/// Crossing fixed point (ALGORITHMS §1, ≤ 2 iterations): solve the new road, classify its
/// crossings, pin junction crossings at the mean height of both roads and re-solve; a crossing
/// whose class still changes is invalid. Returns the profile and the resolved crossings.
///
/// # Errors
/// The first [`Invalid`] reason (too steep, crossing clearance).
pub fn resolve_crossings(
    geom: &SegmentGeom,
    base: &ProfileInput,
    existing: &[Existing<'_>],
) -> Result<(Profile, Vec<(Crossing, CrossingKind)>), Invalid> {
    let solve = |inp: &ProfileInput| {
        profile::solve(inp).map_err(|ProfileError::TooSteep { s_m }| Invalid::TooSteep {
            at: geom.point_at(s_m),
        })
    };
    let mut prof = solve(base)?;
    let mut prev: Option<Vec<CrossingKind>> = None;
    for _ in 0..3 {
        let crossings = find_crossings(geom, &prof, existing);
        let kinds: Vec<CrossingKind> = crossings.iter().map(|c| classify(c.dh_m)).collect();
        if let Some(c) = crossings
            .iter()
            .zip(&kinds)
            .find(|(_, k)| **k == CrossingKind::Invalid)
        {
            return Err(Invalid::CrossingClearance {
                at: c.0.at,
                dh_m: c.0.dh_m,
            });
        }
        if prev.as_ref() == Some(&kinds) {
            return Ok((prof, crossings.into_iter().zip(kinds).collect()));
        }
        let mut inp = base.clone();
        for c in crossings
            .iter()
            .zip(&kinds)
            .filter(|(_, k)| **k == CrossingKind::Junction)
            .map(|(c, _)| c)
        {
            let other_h = existing[c.other].profile.height_at(c.s_other);
            let idx = ((c.s_new / geom.length()) * (inp.s.len() - 1) as f32).round() as usize;
            let mine = prof.height_at(c.s_new);
            inp.pins
                .push((idx.min(inp.s.len() - 1), 0.5 * (mine + other_h)));
        }
        prof = solve(&inp)?;
        prev = Some(kinds);
    }
    let crossings = find_crossings(geom, &prof, existing);
    match crossings.first() {
        Some(c) => Err(Invalid::CrossingClearance {
            at: c.at,
            dh_m: c.dh_m,
        }),
        None => Ok((prof, Vec::new())),
    }
}

/// Validates a new road: length, bounds, curvature, water, overlap with non-crossing corridors.
/// (Grade and crossings are validated by [`resolve_crossings`].)
#[must_use]
pub fn validate_shape(
    geom: &SegmentGeom,
    limits: &RoadLimits,
    ground: &dyn Ground,
    existing: &[Existing<'_>],
    crossings: &[Crossing],
) -> Vec<Invalid> {
    let mut out = Vec::new();
    let len = geom.length();
    if len < MIN_LENGTH_M {
        out.push(Invalid::TooShort);
    }
    let half = ground.half_extent();
    let pts = geom.centerline.points();
    if let Some(p) = pts
        .iter()
        .find(|p| p.x.abs() > half - limits.half_width_m || p.y.abs() > half - limits.half_width_m)
    {
        out.push(Invalid::OutOfBounds { at: *p });
    }
    // Curvature: radius below the type minimum anywhere.
    for k in 0..=64 {
        let t = k as f32 / 64.0;
        let kappa = geom.curve.curvature(t).abs();
        if kappa > 1.0 / limits.min_radius_m + 1e-6 {
            out.push(Invalid::TooSharp {
                at: geom.curve.eval(t),
                radius_m: 1.0 / kappa,
            });
            break;
        }
    }
    let samples = ((len / 2.0).ceil() as usize).max(1);
    for k in 0..=samples {
        let p = geom.point_at(len * k as f32 / samples as f32);
        if ground.water_depth(p) > 0.0 {
            out.push(Invalid::WaterNoBridge { at: p });
            break;
        }
    }
    // Overlap: centreline closer to another centreline than the two half widths, away from
    // shared nodes and crossings.
    'outer: for ex in existing {
        let min_d = 0.9 * (limits.half_width_m + ex.geom.half_width_m);
        let keep_out = limits.half_width_m + ex.geom.half_width_m + 4.0;
        for k in 0..=samples {
            let p = geom.point_at(len * k as f32 / samples as f32);
            if ex
                .shares_node_at
                .is_some_and(|n| (n - p).length() < keep_out)
                || crossings.iter().any(|c| (c.at - p).length() < keep_out)
            {
                continue;
            }
            if ex.geom.project(p).1.abs() < min_d && within_span(&ex.geom.centerline, p) {
                out.push(Invalid::Overlap { at: p });
                break 'outer;
            }
        }
    }
    out
}

fn within_span(pl: &Polyline2, p: Vec2) -> bool {
    let (s, _) = pl.project(p);
    s > 0.5 && s < pl.length() - 0.5
}

#[cfg(test)]
mod tests {
    use super::*;
    use cb_geometry::CubicBezier2;

    struct Flat {
        h: f32,
        water: Option<(Vec2, f32)>,
        slope: f32,
    }

    impl Ground for Flat {
        fn height(&self, p: Vec2) -> f32 {
            self.h + p.x * self.slope
        }
        fn water_depth(&self, p: Vec2) -> f32 {
            self.water
                .map_or(0.0, |(c, r)| if (p - c).length() < r { 2.0 } else { 0.0 })
        }
        fn half_extent(&self) -> f32 {
            2048.0
        }
    }

    const STREET: RoadLimits = RoadLimits {
        half_width_m: 6.0,
        max_grade: 0.10,
        min_radius_m: 12.0,
    };

    fn flat() -> Flat {
        Flat {
            h: 10.0,
            water: None,
            slope: 0.0,
        }
    }

    fn straight(a: Vec2, b: Vec2) -> SegmentGeom {
        SegmentGeom::new(CubicBezier2::straight(a, b), 6.0)
    }

    #[test]
    fn road_invalid_too_steep() {
        let g = straight(Vec2::ZERO, Vec2::new(100.0, 0.0));
        let ground = Flat {
            h: 0.0,
            water: None,
            slope: 0.3,
        };
        let inp = ground_input(&g, &ground, 0.0, 30.0, &STREET);
        let r = resolve_crossings(&g, &inp, &[]);
        let e = r.expect_err("30 % is too steep");
        assert_eq!(e.key(), "road-invalid-too-steep");
    }

    #[test]
    fn road_invalid_too_sharp() {
        let c = CubicBezier2::new(
            Vec2::ZERO,
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
        );
        let g = SegmentGeom::new(c, 6.0);
        let r = validate_shape(&g, &STREET, &flat(), &[], &[]);
        assert!(
            r.iter().any(|i| i.key() == "road-invalid-too-sharp"),
            "{r:?}"
        );
    }

    #[test]
    fn road_invalid_too_short() {
        let g = straight(Vec2::ZERO, Vec2::new(4.0, 0.0));
        assert!(validate_shape(&g, &STREET, &flat(), &[], &[]).contains(&Invalid::TooShort));
    }

    #[test]
    fn road_invalid_water_no_bridge() {
        let g = straight(Vec2::ZERO, Vec2::new(100.0, 0.0));
        let ground = Flat {
            h: 10.0,
            water: Some((Vec2::new(50.0, 0.0), 10.0)),
            slope: 0.0,
        };
        let r = validate_shape(&g, &STREET, &ground, &[], &[]);
        assert!(r.iter().any(|i| i.key() == "road-invalid-water-no-bridge"));
    }

    #[test]
    fn road_invalid_overlap() {
        let a = straight(Vec2::ZERO, Vec2::new(100.0, 0.0));
        let pa = Profile::flat(a.length(), 10.0);
        let b = straight(Vec2::new(10.0, 4.0), Vec2::new(90.0, 4.0));
        let ex = [Existing {
            geom: &a,
            profile: &pa,
            shares_node_at: None,
        }];
        let r = validate_shape(&b, &STREET, &flat(), &ex, &[]);
        assert!(r.iter().any(|i| i.key() == "road-invalid-overlap"), "{r:?}");
        // A parallel road 30 m away is fine.
        let c = straight(Vec2::new(10.0, 30.0), Vec2::new(90.0, 30.0));
        assert!(validate_shape(&c, &STREET, &flat(), &ex, &[]).is_empty());
    }

    #[test]
    fn road_invalid_crossing_clearance() {
        let a = straight(Vec2::new(50.0, -50.0), Vec2::new(50.0, 50.0));
        let pa = Profile::flat(a.length(), 14.0); // 4 m above the new road's ground
        let ex = [Existing {
            geom: &a,
            profile: &pa,
            shares_node_at: None,
        }];
        let b = straight(Vec2::ZERO, Vec2::new(100.0, 0.0));
        // Endpoints pinned at ground: the crossing junction would need +4 m at 50 m — too steep
        // for a junction, too low for an overpass.
        let inp = ground_input(
            &b,
            &flat(),
            10.0,
            10.0,
            &RoadLimits {
                max_grade: 0.05,
                ..STREET
            },
        );
        let e = resolve_crossings(&b, &inp, &ex).expect_err("Δh between 2.5 and 5.5 m");
        assert!(
            matches!(
                e.key(),
                "road-invalid-crossing-clearance" | "road-invalid-too-steep"
            ),
            "{e:?}"
        );
    }

    #[test]
    fn crossing_fixed_point_converges() {
        let a = straight(Vec2::new(50.0, -50.0), Vec2::new(50.0, 50.0));
        let pa = Profile::flat(a.length(), 11.5); // 1.5 m above ground: junction
        let ex = [Existing {
            geom: &a,
            profile: &pa,
            shares_node_at: None,
        }];
        let b = straight(Vec2::ZERO, Vec2::new(100.0, 0.0));
        let inp = ground_input(&b, &flat(), 10.0, 10.0, &STREET);
        let (prof, cr) = resolve_crossings(&b, &inp, &ex).expect("junction resolves");
        assert_eq!(cr.len(), 1);
        assert_eq!(cr[0].1, CrossingKind::Junction);
        // The junction pin pulls the new road to the mean height at the crossing.
        assert!(
            (prof.height_at(cr[0].0.s_new) - 10.75).abs() < 0.3,
            "{}",
            prof.height_at(cr[0].0.s_new)
        );
        assert_eq!(classify(6.0), CrossingKind::Overpass);
    }

    #[test]
    fn valid_street_has_no_reasons() {
        let c =
            CubicBezier2::from_quadratic(Vec2::ZERO, Vec2::new(60.0, 40.0), Vec2::new(120.0, 0.0));
        let g = SegmentGeom::new(c, 6.0);
        assert!(validate_shape(&g, &STREET, &flat(), &[], &[]).is_empty());
        let inp = ground_input(&g, &flat(), 10.0, 10.0, &STREET);
        assert!(resolve_crossings(&g, &inp, &[]).is_ok());
    }

    #[test]
    fn every_reason_has_a_message_in_every_locale() {
        let l10n = cb_l10n::L10n::new(cb_l10n::Locale::En).expect("bundles load");
        let at = Vec2::ZERO;
        let all = [
            Invalid::TooSteep { at },
            Invalid::TooSharp { at, radius_m: 5.0 },
            Invalid::TooShort,
            Invalid::Overlap { at },
            Invalid::WaterNoBridge { at },
            Invalid::CrossingClearance { at, dh_m: 3.0 },
            Invalid::OutOfBounds { at },
        ];
        for reason in all {
            for loc in cb_l10n::Locale::ALL {
                assert!(
                    l10n.has(loc, cb_l10n::Key(reason.key())),
                    "{} missing in {loc:?}",
                    reason.key()
                );
            }
        }
    }
}
