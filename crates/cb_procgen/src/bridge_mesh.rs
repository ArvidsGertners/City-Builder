//! Bridge meshes (ALGORITHMS §3 step 5, ART_DIRECTION): for each span, deck fascias and the
//! underside below the road surface, railings along both edges; piers with a cap beam, tapered
//! when taller than 25 m. The road surface itself comes from `road_mesh` (one profile for road
//! and deck).

use cb_geometry::MeshData;
use cb_network::bridge::{BridgeParams, Pier, TAPERED_PIER_M};
use cb_network::{Profile, SegmentGeom};
use glam::{Vec2, Vec3};

use crate::paint::Painter;
use crate::palette;
use crate::surface::Surface;

/// Sample step along decks (m).
const STEP_M: f32 = 2.0;
/// Railing height (m) and post spacing (m).
const RAIL_H_M: f32 = 1.0;
const POST_STEP_M: f32 = 2.5;
/// Pier column thickness along the road (m) and width across it as a fraction of the deck.
const PIER_T_M: f32 = 1.4;
const PIER_W_FRAC: f32 = 0.45;
/// Cap beam depth (m).
const CAP_M: f32 = 0.6;

fn concrete() -> palette::Rgb {
    palette::hex(0xB9B4AA)
}

/// Deck, railings and piers of one segment's spans.
#[must_use]
pub fn bridge_mesh(
    geom: &SegmentGeom,
    profile: &Profile,
    spans: &[(f32, f32)],
    piers: &[Pier],
    p: &BridgeParams,
    slot: u16,
) -> MeshData {
    let mut pt = Painter::new(slot, 0xB41D);
    let hw = geom.half_width_m;
    for &(s0, s1) in spans {
        let n = (((s1 - s0) / STEP_M).ceil() as usize).max(1);
        let frame = |k: usize| {
            let s = s0 + (s1 - s0) * k as f32 / n as f32;
            let c = geom.point_at(s);
            let nrm = geom.dir_at(s).perp();
            (s, c, nrm, profile.height_at(s))
        };
        for k in 0..n {
            let (_, c0, n0, h0) = frame(k);
            let (_, c1, n1, h1) = frame(k + 1);
            for side in [-1.0f32, 1.0] {
                let e0 = c0 + n0 * (hw * side);
                let e1 = c1 + n1 * (hw * side);
                let v = |e: Vec2, y: f32| Vec3::new(e.x, y, e.y);
                let out = Vec3::new(n0.x * side, 0.0, n0.y * side);
                // Fascia from the surface down to the deck underside.
                pt.face_ao(
                    &[
                        v(e0, h0 - p.deck_m),
                        v(e1, h1 - p.deck_m),
                        v(e1, h1),
                        v(e0, h0),
                    ],
                    &[0.75, 0.75, 1.0, 1.0],
                    concrete(),
                    Surface::Solid,
                    0,
                    out,
                );
                // Top rail.
                let r = |e: Vec2, y: f32| v(e, y + RAIL_H_M);
                pt.face_ao(
                    &[
                        r(e0, h0) - Vec3::Y * 0.12,
                        r(e1, h1) - Vec3::Y * 0.12,
                        r(e1, h1),
                        r(e0, h0),
                    ],
                    &[1.0; 4],
                    palette::CURB,
                    Surface::Solid,
                    0,
                    out,
                );
                pt.face_ao(
                    &[
                        r(e0, h0) - Vec3::Y * 0.12,
                        r(e1, h1) - Vec3::Y * 0.12,
                        r(e1, h1),
                        r(e0, h0),
                    ],
                    &[1.0; 4],
                    palette::CURB,
                    Surface::Solid,
                    0,
                    -out,
                );
            }
            // Underside.
            let (l0, r0) = (c0 - n0 * hw, c0 + n0 * hw);
            let (l1, r1) = (c1 - n1 * hw, c1 + n1 * hw);
            let u = |e: Vec2, y: f32| Vec3::new(e.x, y - p.deck_m, e.y);
            pt.face_ao(
                &[u(l0, h0), u(r0, h0), u(r1, h1), u(l1, h1)],
                &[0.7; 4],
                concrete().scale(0.85),
                Surface::Solid,
                0,
                -Vec3::Y,
            );
        }
        // Railing posts.
        let posts = (((s1 - s0) / POST_STEP_M).floor() as usize).max(1);
        for k in 0..=posts {
            let s = s0 + (s1 - s0) * k as f32 / posts as f32;
            let c = geom.point_at(s);
            let d = geom.dir_at(s);
            let nrm = d.perp();
            let h = profile.height_at(s);
            for side in [-1.0f32, 1.0] {
                let e = c + nrm * ((hw - 0.08) * side);
                let sq = [
                    e - d * 0.05 - nrm * 0.05,
                    e + d * 0.05 - nrm * 0.05,
                    e + d * 0.05 + nrm * 0.05,
                    e - d * 0.05 + nrm * 0.05,
                ];
                pt.prism(&sq, h, h + RAIL_H_M - 0.12, palette::CURB, None, false);
            }
        }
    }
    for pier in piers {
        pier_mesh(&mut pt, geom, pier, hw);
    }
    pt.finish()
}

fn pier_mesh(pt: &mut Painter, geom: &SegmentGeom, pier: &Pier, hw: f32) {
    let d = geom.dir_at(pier.s_m);
    let nrm = d.perp();
    let c = pier.at;
    let rect = |half_w: f32, half_t: f32| {
        vec![
            c - nrm * half_w - d * half_t,
            c + nrm * half_w - d * half_t,
            c + nrm * half_w + d * half_t,
            c - nrm * half_w + d * half_t,
        ]
    };
    let top = pier.top_m;
    // Cap beam across the deck.
    pt.prism(
        &rect(hw * 0.9, 0.5 * PIER_T_M + 0.2),
        top - CAP_M,
        top,
        concrete(),
        None,
        false,
    );
    let col_top = top - CAP_M;
    let foot = pier.foot_m - 1.0;
    let (w_top, t_top) = (hw * PIER_W_FRAC, 0.5 * PIER_T_M);
    if pier.height_m() > TAPERED_PIER_M {
        // Tapered: wider at the foot, in four stacked frusta.
        let k = 4;
        for q in 0..k {
            let f0 = q as f32 / k as f32;
            let f1 = (q + 1) as f32 / k as f32;
            let y0 = foot + (col_top - foot) * f0;
            let y1 = foot + (col_top - foot) * f1;
            let widen = |f: f32| 1.0 + 0.6 * (1.0 - f);
            let lo = rect(w_top * widen(f0), t_top * widen(f0));
            let hi = rect(w_top * widen(f1), t_top * widen(f1));
            for e in 0..4 {
                let (a, b) = (e, (e + 1) % 4);
                let v = |p: Vec2, y: f32| Vec3::new(p.x, y, p.y);
                let mid = (lo[a] + lo[b]) * 0.5 - c;
                pt.face_ao(
                    &[v(lo[a], y0), v(lo[b], y0), v(hi[b], y1), v(hi[a], y1)],
                    &[0.8, 0.8, 1.0, 1.0],
                    concrete(),
                    Surface::Solid,
                    0,
                    Vec3::new(mid.x, 0.0, mid.y),
                );
            }
        }
    } else {
        pt.prism(&rect(w_top, t_top), foot, col_top, concrete(), None, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cb_geometry::CubicBezier2;
    use cb_network::RoadKind;
    use cb_network::bridge::params;

    #[test]
    fn deck_and_piers_within_bounds() {
        let geom = SegmentGeom::new(
            CubicBezier2::straight(Vec2::ZERO, Vec2::new(120.0, 0.0)),
            6.0,
        );
        let profile = Profile::flat(geom.length(), 30.0);
        let p = params(RoadKind::Street);
        let piers = [
            Pier {
                s_m: 40.0,
                at: Vec2::new(40.0, 0.0),
                top_m: 30.0 - p.deck_m,
                foot_m: 20.0,
            },
            Pier {
                s_m: 80.0,
                at: Vec2::new(80.0, 0.0),
                top_m: 30.0 - p.deck_m,
                foot_m: 0.0,
            },
        ];
        let m = bridge_mesh(&geom, &profile, &[(10.0, 110.0)], &piers, &p, 0);
        assert!(
            m.triangle_count() > 100 && m.triangle_count() < 4000,
            "{}",
            m.triangle_count()
        );
        let (lo, hi) = m.bounds();
        assert!(lo.y >= -1.0 - 1e-3 && hi.y <= 30.0 + RAIL_H_M + 1e-3);
        assert!(lo.x >= 9.0 && hi.x <= 111.0);
    }
}
