//! Road segment meshes (ALGORITHMS §24): the cross-section swept along the segment's centreline
//! with heights from its vertical profile — lanes, raised sidewalks and medians with curb faces,
//! skirts on the outer edges and dead-end caps, dashed lane markings.
//!
//! Stations come from the 2 m centreline, thinned where the road neither turns (≤ 1°) nor bends
//! vertically (≤ 2 cm off the chord), at most 12 m apart. Junction surfaces trim segments with
//! `s0_m`/`s1_m` (M3).

use cb_network::cross_section::ElementKind;
use cb_network::{CrossSection, Profile, SegmentGeom};
use glam::{Vec2, Vec3};

use cb_geometry::MeshData;

use crate::paint::Painter;
use crate::palette::{self, Rgb};
use crate::surface::Surface;
use cb_network::style::{GRASS_STRIP_M, RoadSurface, SegmentStyle};

/// Curb / raised element height (m).
pub const CURB_H_M: f32 = 0.15;
/// Skirt depth below the surface on outer edges (m).
pub const SKIRT_M: f32 = 0.8;
/// Marking width (m).
pub const MARKING_W_M: f32 = 0.12;
/// Dash length and period for lane lines (m).
pub const DASH_M: (f32, f32) = (3.0, 9.0);
/// Lift of markings above the surface (m).
const MARKING_LIFT_M: f32 = 0.012;
/// Max station spacing (m).
const MAX_STATION_M: f32 = 12.0;

/// Options for one segment mesh.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoadMeshParams {
    /// Start of the meshed range (m; junction cut-back).
    pub s0_m: f32,
    /// End of the meshed range (m).
    pub s1_m: f32,
    /// Close the start with a cap (dead end).
    pub cap_start: bool,
    /// Close the end with a cap.
    pub cap_end: bool,
    /// Surface and grass strips.
    pub style: SegmentStyle,
}

impl RoadMeshParams {
    /// Whole segment with both ends capped.
    #[must_use]
    pub fn whole(len: f32) -> Self {
        Self {
            s0_m: 0.0,
            s1_m: len,
            cap_start: true,
            cap_end: true,
            style: SegmentStyle::default(),
        }
    }
}

struct Station {
    s: f32,
    p: Vec2,
    n: Vec2,
    y: f32,
}

/// One lateral band of the cross-section.
#[derive(Clone, Copy)]
struct Band {
    left: f32,
    right: f32,
    lift: f32,
    colour: Rgb,
    kind: Surface,
}

fn stations(g: &SegmentGeom, prof: &Profile, s0: f32, s1: f32) -> Vec<Station> {
    let pts = g.centerline.points();
    let cum = g.centerline.cumulative();
    let n = pts.len();
    let normal_at = |k: usize| {
        let a = pts[k.saturating_sub(1)];
        let b = pts[(k + 1).min(n - 1)];
        (b - a).normalize_or(Vec2::X).perp()
    };
    let mut raw: Vec<Station> = Vec::with_capacity(n + 2);
    let mut push = |s: f32, p: Vec2, nrm: Vec2| {
        raw.push(Station {
            s,
            p,
            n: nrm,
            y: prof.height_at(s),
        });
    };
    push(
        s0,
        g.centerline.point_at(s0),
        g.centerline.tangent_at(s0).perp(),
    );
    for k in 0..n {
        if cum[k] > s0 + 0.05 && cum[k] < s1 - 0.05 {
            push(cum[k], pts[k], normal_at(k));
        }
    }
    let end_t = g.centerline.tangent_at((s1 - 0.01).max(s0));
    push(s1, g.centerline.point_at(s1), end_t.perp());
    // Thin: keep a station when heading or grade bends, or the spacing gets long.
    let mut out: Vec<Station> = Vec::with_capacity(raw.len());
    let last = raw.len() - 1;
    for (k, st) in raw.into_iter().enumerate() {
        let keep = match out.last() {
            None => true,
            Some(_) if k == last => true,
            Some(prev) => {
                let turned = prev.n.dot(st.n) < cb_core::math::cos(cb_core::math::to_radians(1.0));
                let far = st.s - prev.s >= MAX_STATION_M;
                let mid_s = 0.5 * (prev.s + st.s);
                let bent = (prof.height_at(mid_s) - 0.5 * (prev.y + st.y)).abs() > 0.02;
                turned || far || bent
            }
        };
        if keep {
            out.push(st);
        }
    }
    out
}

fn bands(cs: &CrossSection, style: &SegmentStyle) -> (Vec<Band>, bool) {
    use cb_network::LaneKind;
    let lane = |e: &cb_network::cross_section::Element| {
        matches!(e.kind, ElementKind::LaneForward | ElementKind::LaneBackward)
    };
    // Dirt paths: verges and car lanes; tram tracks have verges too but ballast lanes.
    let dirt = cs.elements.iter().any(|e| e.kind == ElementKind::Verge)
        && cs
            .elements
            .iter()
            .any(|e| lane(e) && e.lane != LaneKind::Tram);
    let bands = cs
        .offsets()
        .into_iter()
        .map(|(e, c)| {
            let (lift, colour, kind) = match e.kind {
                ElementKind::Sidewalk => (CURB_H_M, palette::SIDEWALK, Surface::Paved),
                ElementKind::Median => (CURB_H_M, palette::GRASS[2], Surface::Foliage),
                ElementKind::Verge => (0.0, palette::DIRT[1], Surface::Ground),
                ElementKind::LaneForward | ElementKind::LaneBackward
                    if e.lane == LaneKind::Tram =>
                {
                    (0.0, palette::BALLAST, Surface::Ground)
                }
                ElementKind::LaneForward | ElementKind::LaneBackward => (
                    0.0,
                    if dirt {
                        palette::DIRT_PATH
                    } else {
                        match style.surface {
                            RoadSurface::LightAsphalt => palette::ASPHALT_LIGHT,
                            RoadSurface::Cobblestone => palette::COBBLESTONE,
                            RoadSurface::DarkAsphalt | RoadSurface::Dirt => palette::ASPHALT,
                        }
                    },
                    if dirt {
                        Surface::Ground
                    } else {
                        Surface::Paved
                    },
                ),
            };
            Band {
                left: c + 0.5 * e.width_m,
                right: c - 0.5 * e.width_m,
                lift,
                colour,
                kind,
            }
        })
        .collect::<Vec<Band>>();
    // Grass strips along the kerb: the curb-side metre of each flagged sidewalk.
    let n = bands.len();
    let mut out = Vec::with_capacity(n + 2);
    for (k, b) in bands.into_iter().enumerate() {
        let is_walk = b.lift > 0.0 && b.colour == palette::SIDEWALK;
        let left_side = k == 0;
        let right_side = k + 1 == n;
        let grass = palette::GRASS[1];
        if is_walk && left_side && style.grass_strip[0] {
            let edge = b.right + GRASS_STRIP_M;
            out.push(Band { right: edge, ..b });
            out.push(Band {
                left: edge,
                colour: grass,
                kind: Surface::Foliage,
                ..b
            });
        } else if is_walk && right_side && style.grass_strip[1] {
            let edge = b.left - GRASS_STRIP_M;
            out.push(Band {
                right: edge,
                colour: grass,
                kind: Surface::Foliage,
                ..b
            });
            out.push(Band { left: edge, ..b });
        } else {
            out.push(b);
        }
    }
    (out, dirt)
}

/// Builds the mesh of one road segment.
#[must_use]
pub fn segment_mesh(
    g: &SegmentGeom,
    prof: &Profile,
    cs: &CrossSection,
    prm: &RoadMeshParams,
    slot: u16,
) -> MeshData {
    let mut p = Painter::new(slot, u64::from(slot) ^ 0x50AD);
    let s0 = prm.s0_m.clamp(0.0, g.length());
    let s1 = prm.s1_m.clamp(s0, g.length());
    if s1 - s0 < 0.05 {
        return p.finish();
    }
    let st = stations(g, prof, s0, s1);
    let (bands, dirt) = bands(cs, &prm.style);
    let at = |s: &Station, off: f32, lift: f32| {
        Vec3::new(s.p.x + s.n.x * off, s.y + lift, s.p.y + s.n.y * off)
    };
    for w in st.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        let side = |s: &Station| Vec3::new(s.n.x, 0.0, s.n.y);
        for (k, band) in bands.iter().enumerate() {
            let q = [
                at(a, band.left, band.lift),
                at(b, band.left, band.lift),
                at(b, band.right, band.lift),
                at(a, band.right, band.lift),
            ];
            p.face_ao(&q, &[1.0; 4], band.colour, band.kind, 0, Vec3::Y);
            // Curb face toward a lower neighbour on the right.
            if let Some(next) = bands.get(k + 1)
                && (band.lift - next.lift).abs() > 1e-3
            {
                let (hi, lo) = if band.lift > next.lift {
                    (band.lift, next.lift)
                } else {
                    (next.lift, band.lift)
                };
                let out = if band.lift > next.lift {
                    -side(a)
                } else {
                    side(a)
                };
                let o = band.right;
                p.face_ao(
                    &[at(a, o, lo), at(b, o, lo), at(b, o, hi), at(a, o, hi)],
                    &[0.85, 0.85, 1.0, 1.0],
                    palette::CURB,
                    Surface::Solid,
                    0,
                    out,
                );
            }
        }
        // Outer skirts.
        let (l, r) = (&bands[0], &bands[bands.len() - 1]);
        let skirt_c = if dirt {
            palette::DIRT[1]
        } else {
            palette::CURB.scale(0.9)
        };
        p.face_ao(
            &[
                at(a, l.left, -SKIRT_M),
                at(b, l.left, -SKIRT_M),
                at(b, l.left, l.lift),
                at(a, l.left, l.lift),
            ],
            &[0.8, 0.8, 1.0, 1.0],
            skirt_c,
            Surface::Solid,
            0,
            side(a),
        );
        p.face_ao(
            &[
                at(a, r.right, -SKIRT_M),
                at(b, r.right, -SKIRT_M),
                at(b, r.right, r.lift),
                at(a, r.right, r.lift),
            ],
            &[0.8, 0.8, 1.0, 1.0],
            skirt_c,
            Surface::Solid,
            0,
            -side(a),
        );
    }
    // Dead-end caps: one vertical face per band across the end.
    for (s, cap, out_sign) in [
        (&st[0], prm.cap_start, -1.0f32),
        (&st[st.len() - 1], prm.cap_end, 1.0),
    ] {
        if !cap {
            continue;
        }
        let t = Vec2::new(s.n.y, -s.n.x) * out_sign;
        for band in &bands {
            p.face_ao(
                &[
                    at(s, band.left, -SKIRT_M),
                    at(s, band.right, -SKIRT_M),
                    at(s, band.right, band.lift),
                    at(s, band.left, band.lift),
                ],
                &[0.8, 0.8, 1.0, 1.0],
                palette::CURB.scale(0.9),
                Surface::Solid,
                0,
                Vec3::new(t.x, 0.0, t.y),
            );
        }
    }
    if !dirt {
        markings(&mut p, g, prof, cs, s0, s1);
    }
    p.finish()
}

/// Dashed lines between adjacent lanes (same direction: lane line; opposite: centre line).
fn markings(p: &mut Painter, g: &SegmentGeom, prof: &Profile, cs: &CrossSection, s0: f32, s1: f32) {
    let offs = cs.offsets();
    let lane = |e: &cb_network::cross_section::Element| {
        matches!(e.kind, ElementKind::LaneForward | ElementKind::LaneBackward)
            && e.lane != cb_network::LaneKind::Tram
    };
    for w in offs.windows(2) {
        let ((ea, ca), (eb, _)) = (w[0], w[1]);
        if !(lane(&ea) && lane(&eb)) {
            continue;
        }
        let off = ca - 0.5 * ea.width_m;
        let colour = palette::MARKING;
        let (dash, period) = if ea.kind == eb.kind {
            DASH_M
        } else {
            (4.0, 8.0)
        };
        let mut s = s0 + 0.5 * (period - dash);
        while s + dash <= s1 {
            let pts: Vec<Vec3> = [s, s + 0.5 * dash, s + dash]
                .iter()
                .map(|&t| {
                    let c = g.centerline.point_at(t);
                    let n = g.centerline.tangent_at(t).perp();
                    let q = c + n * off;
                    Vec3::new(q.x, prof.height_at(t) + MARKING_LIFT_M, q.y)
                })
                .collect();
            for k in 0..2 {
                let (a, b) = (pts[k], pts[k + 1]);
                let d = (b - a).normalize_or_zero();
                let side = Vec3::new(-d.z, 0.0, d.x) * (0.5 * MARKING_W_M);
                p.face_ao(
                    &[a - side, b - side, b + side, a + side],
                    &[1.0; 4],
                    colour,
                    Surface::Marking,
                    0,
                    Vec3::Y,
                );
            }
            s += period;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cb_geometry::CubicBezier2;
    use cb_network::RoadKind;

    #[test]
    fn straight_street_surface_and_budget() {
        let g = SegmentGeom::new(
            CubicBezier2::straight(Vec2::ZERO, Vec2::new(100.0, 0.0)),
            6.0,
        );
        let prof = Profile::flat(g.length(), 10.0);
        let cs = CrossSection::of(RoadKind::Street);
        let m = segment_mesh(&g, &prof, &cs, &RoadMeshParams::whole(g.length()), 3);
        let (lo, hi) = m.bounds();
        assert!(
            (lo.z + 6.0).abs() < 1e-3 && (hi.z - 6.0).abs() < 1e-3,
            "{lo} {hi}"
        );
        assert!(
            (hi.y - (10.0 + CURB_H_M + MARKING_LIFT_M)).abs() < 1e-3
                || (hi.y - (10.0 + CURB_H_M)).abs() < 1e-3
        );
        // Straight and flat: stations at most 12 m apart → 9 intervals.
        let tris = m.triangle_count();
        assert!(tris < 400, "{tris}");
        for k in 0..tris {
            let n = m.face_normal(k);
            assert!(n.is_finite());
        }
    }

    #[test]
    fn curved_avenue_follows_profile() {
        let c = CubicBezier2::new(
            Vec2::ZERO,
            Vec2::new(60.0, 0.0),
            Vec2::new(100.0, 40.0),
            Vec2::new(100.0, 100.0),
        );
        let g = SegmentGeom::new(c, 11.0);
        let s: Vec<f32> = (0..=10).map(|k| g.length() * k as f32 / 10.0).collect();
        let h: Vec<f32> = s.iter().map(|v| 0.06 * v).collect();
        let prof = Profile::new(s, h);
        let cs = CrossSection::of(RoadKind::Avenue);
        let m = segment_mesh(&g, &prof, &cs, &RoadMeshParams::whole(g.length()), 1);
        // Every surface vertex is within 2 cm + curb of the profile at its projection.
        for v in &m.positions {
            let q = Vec2::new(v[0], v[2]);
            let (s, d) = g.project(q);
            if d.abs() < 10.9 && v[1] > prof.height_at(s) - 0.05 {
                assert!(v[1] - prof.height_at(s) < CURB_H_M + 0.05, "{v:?}");
            }
        }
    }
}
