//! Pitched roofs on rectangular volumes (ALGORITHMS §11 step 5): gable and hip roofs built as
//! slabs (top planes, sloped soffit underneath, fascia strips), with overhangs, plus the gable
//! end triangles in wall colour.

use glam::Vec3;

use crate::paint::{Frame, Painter};
use crate::palette::Rgb;
use crate::surface::Surface;

/// Roof forms for rectangles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoofKind {
    /// Two planes; ridge along local `u` (`ridge_u`) or `v`.
    Gable {
        /// Ridge parallel to the frame's first axis.
        ridge_u: bool,
    },
    /// Four planes, ridge along the longer side (pyramid when square).
    Hip,
}

/// Roof geometry parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoofSpec {
    /// Form.
    pub kind: RoofKind,
    /// tan(pitch).
    pub slope: f32,
    /// Eave overhang (m).
    pub eave_m: f32,
    /// Gable (verge) overhang (m).
    pub verge_m: f32,
    /// Slab thickness (m).
    pub thickness_m: f32,
}

/// Ridge height above the eave for a rectangle `[u0, v0, u1, v1]`.
#[must_use]
pub fn rise(spec: &RoofSpec, r: [f32; 4]) -> f32 {
    let (w, d) = (r[2] - r[0], r[3] - r[1]);
    let half = match spec.kind {
        RoofKind::Gable { ridge_u: true } => 0.5 * d,
        RoofKind::Gable { ridge_u: false } => 0.5 * w,
        RoofKind::Hip => 0.5 * w.min(d),
    };
    half * spec.slope
}

/// Appends the roof over rectangle `r` with walls ending at `eave_y`; returns the ridge height.
/// `gable_c` colours the gable end triangles at the low and high end of the ridge axis.
pub fn roof(
    p: &mut Painter,
    fr: &Frame,
    r: [f32; 4],
    eave_y: f32,
    spec: &RoofSpec,
    roof_c: Rgb,
    gable_c: [Rgb; 2],
) -> f32 {
    // Work in a swapped frame so the ridge always runs along local u.
    let (fr2, r2) = match spec.kind {
        RoofKind::Gable { ridge_u: false } => (swap(fr), [r[1], r[0], r[3], r[2]]),
        RoofKind::Hip if r[2] - r[0] < r[3] - r[1] => (swap(fr), [r[1], r[0], r[3], r[2]]),
        _ => (*fr, r),
    };
    let [u0, v0, u1, v1] = r2;
    let t = spec.slope;
    let half = 0.5 * (v1 - v0);
    let vm = 0.5 * (v0 + v1);
    let ridge_y = eave_y + half * t;
    let (oe, ov) = (spec.eave_m, spec.verge_m);
    let low_y = eave_y - oe * t;
    let th = spec.thickness_m;
    let q = |u: f32, v: f32, y: f32| fr2.p3(u, v, y);
    let up = Vec3::Y;
    let soffit = roof_c.scale(0.62);
    match spec.kind {
        RoofKind::Gable { .. } => {
            let (ua, ub) = (u0 - ov, u1 + ov);
            let fl = q(ua, v0 - oe, low_y);
            let frr = q(ub, v0 - oe, low_y);
            let rr = q(ub, vm, ridge_y);
            let rl = q(ua, vm, ridge_y);
            let br = q(ub, v1 + oe, low_y);
            let bl = q(ua, v1 + oe, low_y);
            let n_front = fr2.d3(0.0, -t, 1.0);
            let n_back = fr2.d3(0.0, t, 1.0);
            p.face_ao(
                &[fl, frr, rr, rl],
                &[1.0; 4],
                roof_c,
                Surface::Roof,
                0,
                n_front,
            );
            p.face_ao(
                &[rl, rr, br, bl],
                &[1.0; 4],
                roof_c,
                Surface::Roof,
                0,
                n_back,
            );
            let dn = |v: Vec3| v - up * th;
            p.face_ao(
                &[dn(fl), dn(frr), dn(rr), dn(rl)],
                &[0.8; 4],
                soffit,
                Surface::Solid,
                0,
                -n_front,
            );
            p.face_ao(
                &[dn(rl), dn(rr), dn(br), dn(bl)],
                &[0.8; 4],
                soffit,
                Surface::Solid,
                0,
                -n_back,
            );
            let rim = [fl, frr, rr, br, bl, rl];
            fascia(
                p,
                &rim,
                th,
                roof_c.scale(0.8),
                fr2.p3(0.5 * (u0 + u1), vm, ridge_y),
            );
            // Gable end triangles (wall colour).
            for (u, out, wall_c) in [(u0, -1.0, gable_c[0]), (u1, 1.0, gable_c[1])] {
                p.face_ao(
                    &[q(u, v0, eave_y), q(u, v1, eave_y), q(u, vm, ridge_y)],
                    &[
                        1.0 - crate::paint::EAVE_AO,
                        1.0 - crate::paint::EAVE_AO,
                        0.92,
                    ],
                    wall_c,
                    Surface::Solid,
                    0,
                    fr2.d3(out, 0.0, 0.0),
                );
            }
        }
        RoofKind::Hip => {
            let (ua, ub, va, vb) = (u0 - oe, u1 + oe, v0 - oe, v1 + oe);
            let inset = half + oe;
            let (ra, rb) = {
                let a = ua + inset;
                let b = ub - inset;
                if b > a {
                    (a, b)
                } else {
                    (0.5 * (ua + ub), 0.5 * (ua + ub))
                }
            };
            let e00 = q(ua, va, low_y);
            let e10 = q(ub, va, low_y);
            let e11 = q(ub, vb, low_y);
            let e01 = q(ua, vb, low_y);
            let r0 = q(ra, vm, ridge_y);
            let r1 = q(rb, vm, ridge_y);
            let planes: [(Vec<Vec3>, Vec3); 4] = [
                (vec![e00, e10, r1, r0], fr2.d3(0.0, -t, 1.0)),
                (vec![e10, e11, r1], fr2.d3(t, 0.0, 1.0)),
                (vec![e11, e01, r0, r1], fr2.d3(0.0, t, 1.0)),
                (vec![e01, e00, r0], fr2.d3(-t, 0.0, 1.0)),
            ];
            for (pts, n) in &planes {
                let pts: Vec<Vec3> = pts.iter().copied().fold(Vec::new(), |mut acc, v| {
                    if acc.last() != Some(&v) {
                        acc.push(v);
                    }
                    acc
                });
                if pts.len() >= 3 {
                    p.face_ao(&pts, &vec![1.0; pts.len()], roof_c, Surface::Roof, 0, *n);
                }
            }
            let dn = |v: Vec3| v - up * th;
            p.face_ao(
                &[dn(e00), dn(e10), dn(e11), dn(e01)],
                &[0.8; 4],
                soffit,
                Surface::Solid,
                0,
                -up,
            );
            fascia(
                p,
                &[e00, e10, e11, e01],
                th,
                roof_c.scale(0.8),
                fr2.p3(0.5 * (u0 + u1), vm, ridge_y),
            );
        }
    }
    ridge_y
}

/// Vertical edge strips along a closed rim (top edge on the roof, bottom `th` lower).
fn fascia(p: &mut Painter, rim: &[Vec3], th: f32, c: Rgb, center: Vec3) {
    let n = rim.len();
    for k in 0..n {
        let (a, b) = (rim[k], rim[(k + 1) % n]);
        let mid = (a + b) * 0.5;
        let out = Vec3::new(mid.x - center.x, 0.0, mid.z - center.z);
        let d = Vec3::Y * th;
        p.face_ao(
            &[a - d, b - d, b, a],
            &[0.85, 0.85, 1.0, 1.0],
            c,
            Surface::Solid,
            0,
            out,
        );
    }
}

/// The same frame with axes swapped (kept counter-clockwise by negating nothing: callers pass
/// swapped rectangles, and faces orient themselves from outward hints).
fn swap(fr: &Frame) -> Frame {
    Frame {
        o: fr.o,
        ex: fr.ey,
        ey: fr.ex,
    }
}
