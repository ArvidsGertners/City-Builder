//! Junction geometry (ADR-0063, ALGORITHMS §5 steps 1–4 and 10): approach ordering, cut-backs,
//! corner fillets (quadratic through the kerb-line intersection; reflex corners follow both
//! kerbs to their meeting point), the outer (sidewalk) and inner (carriageway) polygons with
//! per-vertex heights, and polygon-simplicity growth.
//!
//! M2 builds the geometry only; connectors, conflict zones and the connector-driven growth of
//! ADR-0063 are added with the lane graph in M3 (same cut-back rules, so geometry stays put).

use cb_geometry::Polyline2;
use glam::{Vec2, Vec3};

use crate::profile::Profile;

/// Minimum angle between adjacent approaches (deg).
pub const MIN_ANGLE_DEG: f32 = 25.0;
/// Cut-backs may use at most this fraction of an approach.
pub const MAX_CUT_FRACTION: f32 = 0.45;
/// Growth step for polygon simplicity (m) and iterations.
const GROW_M: f32 = 2.0;
const GROW_ITERS: usize = 40;
/// Sampling step of kerb lines (m).
const KERB_STEP_M: f32 = 0.5;
/// Maximum number of approaches.
pub const MAX_DEGREE: usize = 6;

/// One incident segment seen from the node.
#[derive(Clone, Debug)]
pub struct Approach {
    /// Centreline leaving the node (first point at the node).
    pub centerline: Polyline2,
    /// Half width incl. sidewalks (m).
    pub half_width_m: f32,
    /// Half width of the carriageway (curb line, m).
    pub carriageway_half_m: f32,
    /// Corner radius (m): street 6, avenue 9, dirt 4.
    pub corner_radius_m: f32,
    /// Surface profile of the segment.
    pub profile: Profile,
    /// The segment runs toward the node (profile arc length measured from the far end).
    pub reversed: bool,
}

impl Approach {
    fn len(&self) -> f32 {
        self.centerline.length()
    }

    fn point(&self, s: f32) -> Vec2 {
        self.centerline.point_at(s)
    }

    fn tangent(&self, s: f32) -> Vec2 {
        self.centerline.tangent_at(s)
    }

    fn edge(&self, s: f32, off: f32) -> Vec2 {
        self.point(s) + self.tangent(s).perp() * off
    }

    /// Surface height at distance `s` from the node.
    #[must_use]
    pub fn height(&self, s: f32) -> f32 {
        let len = self.len();
        self.profile
            .height_at(if self.reversed { len - s } else { s })
    }

    fn angle(&self) -> f32 {
        let t = self.tangent(0.0);
        let a = cb_core::math::atan2(t.y, t.x);
        if a < 0.0 { a + cb_core::math::TAU } else { a }
    }
}

/// Junction errors (validation values with l10n keys).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JunctionError {
    /// Two approaches closer than 25°.
    AngleTooSmall,
    /// A cut-back needs more than 45 % of its segment.
    TooClose,
    /// The polygon stays self-intersecting.
    PolygonInvalid,
    /// More than six approaches.
    DegreeTooHigh,
}

impl JunctionError {
    /// Localization key.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::AngleTooSmall => "road-invalid-junction-angle",
            Self::TooClose => "road-invalid-junction-too-close",
            Self::PolygonInvalid => "road-invalid-junction-shape",
            Self::DegreeTooHigh => "road-invalid-junction-degree",
        }
    }
}

/// The corner between approach `from` (its left kerb) and the next approach counter-clockwise
/// `to` (its right kerb).
#[derive(Clone, Debug, PartialEq)]
pub struct Corner {
    /// Input index of the approach whose left side bounds the corner.
    pub from: usize,
    /// Input index of the approach whose right side bounds the corner.
    pub to: usize,
    /// Outer kerb (sidewalk outer edge) from `from`'s cut to `to`'s cut, with heights.
    pub outer: Vec<Vec3>,
    /// Inner kerb (carriageway curb) from `from`'s cut to `to`'s cut.
    pub inner: Vec<Vec3>,
    /// Convex corner with a fillet (otherwise a reflex/open corner following the kerbs).
    pub fillet: bool,
}

/// Junction geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct Junction {
    /// Cut-back per approach (input order, m from the node).
    pub cuts: Vec<f32>,
    /// Outer polygon (counter-clockwise in `(x, z)`), with heights.
    pub outer: Vec<Vec3>,
    /// Carriageway polygon, with heights.
    pub inner: Vec<Vec3>,
    /// Corners in counter-clockwise order.
    pub corners: Vec<Corner>,
}

fn seg_intersect(p: Vec2, p2: Vec2, q: Vec2, q2: Vec2) -> Option<(f32, f32)> {
    let (r, s) = (p2 - p, q2 - q);
    let den = r.perp_dot(s);
    if den.abs() < 1e-9 {
        return None;
    }
    let t = (q - p).perp_dot(s) / den;
    let u = (q - p).perp_dot(r) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some((t, u))
}

fn sample_edge(a: &Approach, off: f32) -> Vec<(f32, Vec2)> {
    let len = a.len();
    let n = (len / KERB_STEP_M).ceil().max(1.0) as usize;
    (0..=n)
        .map(|i| {
            let s = len * i as f32 / n as f32;
            (s, a.edge(s, off))
        })
        .collect()
}

/// Arc lengths where A's left kerb meets B's right kerb (first crossing along A).
fn kerb_crossing(a: &Approach, b: &Approach) -> Option<(f32, f32)> {
    let ea = sample_edge(a, a.half_width_m);
    let eb = sample_edge(b, -b.half_width_m);
    let mut best: Option<(f32, f32)> = None;
    for i in 0..ea.len() - 1 {
        if best.is_some_and(|(bs, _)| ea[i].0 > bs) {
            break;
        }
        for j in 0..eb.len() - 1 {
            if let Some((t, u)) = seg_intersect(ea[i].1, ea[i + 1].1, eb[j].1, eb[j + 1].1) {
                let sa = ea[i].0 + t * (ea[i + 1].0 - ea[i].0);
                let sb = eb[j].0 + u * (eb[j + 1].0 - eb[j].0);
                if best.is_none_or(|(bs, _)| sa < bs) {
                    best = Some((sa, sb));
                }
            }
        }
    }
    best
}

/// Reflex corner: follow both kerbs back from the cut lines to where they meet (≤ 30 m past the
/// node), else a straight edge.
fn reflex_corner(
    a: &Approach,
    off_a: f32,
    cut_a: f32,
    b: &Approach,
    off_b: f32,
    cut_b: f32,
) -> Vec<Vec2> {
    let walk = |ap: &Approach, off: f32, cut: f32| -> Vec<Vec2> {
        let n = ((cut + 30.0) / KERB_STEP_M).ceil() as usize;
        (0..=n)
            .map(|k| {
                let s = cut - k as f32 * KERB_STEP_M;
                // Beyond the node, extend straight along the initial tangent.
                if s >= 0.0 {
                    ap.edge(s, off)
                } else {
                    ap.edge(0.0, off) + ap.tangent(0.0) * s
                }
            })
            .collect()
    };
    let ea = walk(a, off_a, cut_a);
    let eb = walk(b, off_b, cut_b);
    for i in 0..ea.len() - 1 {
        for j in 0..eb.len() - 1 {
            if let Some((t, _)) = seg_intersect(ea[i], ea[i + 1], eb[j], eb[j + 1]) {
                let x = ea[i] + (ea[i + 1] - ea[i]) * t;
                let mut out: Vec<Vec2> = ea[..=i].to_vec();
                out.push(x);
                out.extend(eb[..=j].iter().rev());
                return out;
            }
        }
    }
    vec![ea[0], eb[0]]
}

/// Corner curve from A's left kerb at `cut_a` to B's right kerb at `cut_b` at offsets
/// `off_a` (left, positive) and `off_b` (right, positive); `(points, is_fillet)`.
fn corner_curve(
    a: &Approach,
    off_a: f32,
    cut_a: f32,
    b: &Approach,
    off_b: f32,
    cut_b: f32,
) -> (Vec<Vec2>, bool) {
    let l_pt = a.edge(cut_a, off_a);
    let r_pt = b.edge(cut_b, -off_b);
    let (da, db) = (-a.tangent(cut_a), -b.tangent(cut_b));
    let den = da.perp_dot(db);
    // Near-parallel kerbs (straight-through side of a T): a plain edge.
    if den.abs() < 0.02 && da.dot(db) < 0.0 {
        return (vec![l_pt, r_pt], false);
    }
    if den.abs() > 1e-6 {
        let aa = (r_pt - l_pt).perp_dot(db) / den;
        let bb = (r_pt - l_pt).perp_dot(da) / den;
        if aa > 0.05 && bb > 0.05 {
            let x = l_pt + da * aa;
            let len = l_pt.distance(x) + x.distance(r_pt);
            let n = (len / 1.0).ceil().clamp(4.0, 24.0) as usize;
            let pts = (0..=n)
                .map(|k| {
                    let t = k as f32 / n as f32;
                    l_pt * ((1.0 - t) * (1.0 - t)) + x * (2.0 * (1.0 - t) * t) + r_pt * (t * t)
                })
                .collect();
            return (pts, true);
        }
    }
    (reflex_corner(a, off_a, cut_a, b, -off_b, cut_b), false)
}

fn is_simple(poly: &[Vec2]) -> bool {
    let m = poly.len();
    if m < 3 {
        return false;
    }
    for x in 0..m {
        for y in x + 2..m {
            if (y + 1) % m == x {
                continue;
            }
            if let Some((t, u)) =
                seg_intersect(poly[x], poly[(x + 1) % m], poly[y], poly[(y + 1) % m])
                && t > 1e-6
                && t < 1.0 - 1e-6
                && u > 1e-6
                && u < 1.0 - 1e-6
            {
                return false;
            }
        }
    }
    cb_geometry::polygon::signed_area(poly) > 0.0
}

/// Builds the junction for `apps` (two or more approaches).
///
/// # Errors
/// [`JunctionError`] when the approaches are too close in angle or length, or the polygon
/// cannot be made simple.
pub fn build(apps: &[Approach]) -> Result<Junction, JunctionError> {
    let n = apps.len();
    if n > MAX_DEGREE {
        return Err(JunctionError::DegreeTooHigh);
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| apps[a].angle().total_cmp(&apps[b].angle()).then(a.cmp(&b)));
    let wrap = |a: f32| if a < 0.0 { a + cb_core::math::TAU } else { a };
    let gap = |k: usize| wrap(apps[order[(k + 1) % n]].angle() - apps[order[k]].angle());
    if n >= 2 && (0..n).any(|k| gap(k) < cb_core::math::to_radians(MIN_ANGLE_DEG) - 1e-6) {
        return Err(JunctionError::AngleTooSmall);
    }
    // Cut-backs (in sorted order).
    let mut req = vec![0.0f32; n];
    if n >= 2 {
        for k in 0..n {
            let j = (k + 1) % n;
            if let Some((sa, sb)) = kerb_crossing(&apps[order[k]], &apps[order[j]]) {
                req[k] = req[k].max(sa);
                req[j] = req[j].max(sb);
            }
        }
    }
    let corner_allow = |k: usize| -> f32 {
        let (a, b) = (&apps[order[k]], &apps[order[(k + 1) % n]]);
        let alpha = gap(k).min(cb_core::math::PI - 1e-4);
        let r = a.corner_radius_m.min(b.corner_radius_m);
        r.min(r / cb_core::math::tan(0.5 * alpha).max(1e-4))
    };
    let mut cuts: Vec<f32> = (0..n)
        .map(|k| {
            if n <= 2 {
                return apps[order[k]].corner_radius_m;
            }
            let widest_other = (0..n)
                .filter(|&q| q != k)
                .map(|q| 2.0 * apps[order[q]].half_width_m)
                .fold(0.0, f32::max);
            req[k].max(0.5 * widest_other) + corner_allow(k).max(corner_allow((k + n - 1) % n))
        })
        .collect();

    for _ in 0..GROW_ITERS {
        if (0..n).any(|k| cuts[k] > MAX_CUT_FRACTION * apps[order[k]].len()) {
            return Err(JunctionError::TooClose);
        }
        let mut outer: Vec<Vec3> = Vec::new();
        let mut inner: Vec<Vec3> = Vec::new();
        let mut corners: Vec<Corner> = Vec::with_capacity(n);
        // Owner of each outer vertex (sorted approach index) for growth.
        let mut owner: Vec<usize> = Vec::new();
        for k in 0..n {
            let a = &apps[order[k]];
            let c = cuts[k];
            let h = a.height(c);
            for (poly, half, own) in [
                (&mut outer, a.half_width_m, true),
                (&mut inner, a.carriageway_half_m, false),
            ] {
                let steps = (2.0 * half).ceil().max(1.0) as usize;
                for q in 0..=steps {
                    let off = -half + 2.0 * half * q as f32 / steps as f32;
                    let p = a.edge(c, off);
                    poly.push(Vec3::new(p.x, h, p.y));
                    if own {
                        owner.push(k);
                    }
                }
            }
            if n >= 2 {
                let j = (k + 1) % n;
                let b = &apps[order[j]];
                let hb = b.height(cuts[j]);
                let (oc, fillet) = corner_curve(a, a.half_width_m, c, b, b.half_width_m, cuts[j]);
                let (ic, _) =
                    corner_curve(a, a.carriageway_half_m, c, b, b.carriageway_half_m, cuts[j]);
                let lift = |pts: &[Vec2]| -> Vec<Vec3> {
                    let m = pts.len().max(2) - 1;
                    pts.iter()
                        .enumerate()
                        .map(|(q, p)| Vec3::new(p.x, h + (hb - h) * q as f32 / m as f32, p.y))
                        .collect()
                };
                let (o3, i3) = (lift(&oc), lift(&ic));
                for v in o3.iter().skip(1).take(o3.len().saturating_sub(2)) {
                    outer.push(*v);
                    owner.push(k);
                }
                inner.extend(i3.iter().skip(1).take(i3.len().saturating_sub(2)));
                corners.push(Corner {
                    from: order[k],
                    to: order[j],
                    outer: o3,
                    inner: i3,
                    fillet,
                });
            }
        }
        let flat_o: Vec<Vec2> = outer.iter().map(|v| Vec2::new(v.x, v.z)).collect();
        let flat_i: Vec<Vec2> = inner.iter().map(|v| Vec2::new(v.x, v.z)).collect();
        if n <= 2 || (is_simple(&flat_o) && is_simple(&flat_i)) {
            let mut cuts_in = vec![0.0; n];
            for k in 0..n {
                cuts_in[order[k]] = cuts[k];
            }
            return Ok(Junction {
                cuts: cuts_in,
                outer,
                inner,
                corners,
            });
        }
        // Grow the approaches owning crossing edges.
        let m = flat_o.len();
        let mut grow = vec![false; n];
        for x in 0..m {
            for y in x + 2..m {
                if (y + 1) % m == x {
                    continue;
                }
                if seg_intersect(
                    flat_o[x],
                    flat_o[(x + 1) % m],
                    flat_o[y],
                    flat_o[(y + 1) % m],
                )
                .is_some()
                {
                    grow[owner[x]] = true;
                    grow[owner[y]] = true;
                }
            }
        }
        if !grow.iter().any(|g| *g) {
            grow.fill(true);
        }
        for k in 0..n {
            if grow[k] {
                cuts[k] += GROW_M;
            }
        }
    }
    Err(JunctionError::PolygonInvalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arm(deg: f32, len: f32, half: f32) -> Approach {
        let (sn, cs) = cb_core::math::sin_cos(cb_core::math::to_radians(deg));
        let d = Vec2::new(cs, sn);
        let centerline = Polyline2::new((0..=((len / 2.0) as usize)).map(|k| d * (k as f32 * 2.0)));
        Approach {
            centerline,
            half_width_m: half,
            carriageway_half_m: half - 2.0,
            corner_radius_m: 6.0,
            profile: Profile::flat(len, 10.0),
            reversed: false,
        }
    }

    #[test]
    fn t_and_x_junctions_are_simple() {
        for degs in [
            vec![0.0, 90.0, 180.0],
            vec![0.0, 90.0, 180.0, 270.0],
            vec![0.0, 120.0, 240.0],
            vec![0.0, 60.0, 180.0],
        ] {
            let apps: Vec<Approach> = degs.iter().map(|d| arm(*d, 100.0, 6.0)).collect();
            let j = build(&apps).unwrap();
            let flat: Vec<Vec2> = j.outer.iter().map(|v| Vec2::new(v.x, v.z)).collect();
            assert!(is_simple(&flat), "{degs:?}");
            assert_eq!(j.corners.len(), degs.len());
            for c in &j.cuts {
                assert!(*c >= 6.0 && *c < 30.0, "{degs:?} cut {c}");
            }
            assert!(j.outer.iter().all(|v| (v.y - 10.0).abs() < 1e-4));
        }
    }

    #[test]
    fn t_junction_has_two_fillets_and_one_straight_side() {
        let apps: Vec<Approach> = [0.0, 90.0, 180.0]
            .iter()
            .map(|d| arm(*d, 100.0, 6.0))
            .collect();
        let j = build(&apps).unwrap();
        assert_eq!(j.corners.iter().filter(|c| c.fillet).count(), 2);
        // The straight side (between 180° and 0°) is a plain edge.
        let straight = j.corners.iter().find(|c| !c.fillet).unwrap();
        assert_eq!(straight.outer.len(), 2);
    }

    #[test]
    fn errors() {
        let apps: Vec<Approach> = [0.0, 20.0, 180.0]
            .iter()
            .map(|d| arm(*d, 100.0, 6.0))
            .collect();
        assert_eq!(build(&apps).unwrap_err(), JunctionError::AngleTooSmall);
        let apps: Vec<Approach> = [0.0, 90.0, 180.0]
            .iter()
            .map(|d| arm(*d, 12.0, 11.0))
            .collect();
        assert_eq!(build(&apps).unwrap_err(), JunctionError::TooClose);
    }

    #[test]
    fn every_junction_error_has_a_message_in_every_locale() {
        let l10n = cb_l10n::L10n::new(cb_l10n::Locale::En).expect("bundles load");
        for e in [
            JunctionError::AngleTooSmall,
            JunctionError::TooClose,
            JunctionError::PolygonInvalid,
            JunctionError::DegreeTooHigh,
        ] {
            for loc in cb_l10n::Locale::ALL {
                assert!(
                    l10n.has(loc, cb_l10n::Key(e.key())),
                    "{} missing in {loc:?}",
                    e.key()
                );
            }
        }
    }
}
