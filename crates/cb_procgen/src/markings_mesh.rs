//! Junction markings and signal heads (ALGORITHMS §5 step 5, ART_DIRECTION): zebra crosswalks,
//! stop and give-way lines and lane arrows as thin decals lifted above the asphalt; raised
//! splitter islands; signal poles with heads and three lamp slots. Lit lamps are separate small meshes the game toggles per phase.

use cb_geometry::MeshData;
use cb_network::markings::{ArrowTurns, Island, LaneArrow, LineKind, Markings, StopLine};
use glam::{Vec2, Vec3};

use crate::paint::Painter;
use crate::palette;
use crate::surface::Surface;

/// Lift of junction decals above the surface (m).
pub const DECAL_LIFT_M: f32 = 0.03;
/// Zebra stripe width and period across the road (m).
const STRIPE_W_M: f32 = 0.5;
const STRIPE_PERIOD_M: f32 = 1.0;
/// Signal pole height (m) and head size (m).
pub const POLE_H_M: f32 = 3.8;
const HEAD_W_M: f32 = 0.36;
const HEAD_H_M: f32 = 1.0;
/// Lamp size (m) and spacing (m).
pub const LAMP_M: f32 = 0.22;
const LAMP_STEP_M: f32 = 0.3;

/// Signal lamp colours, top to bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lamp {
    /// Red (top).
    Red,
    /// Yellow (middle).
    Yellow,
    /// Green (bottom).
    Green,
}

impl Lamp {
    /// Top to bottom.
    pub const ALL: [Self; 3] = [Self::Red, Self::Yellow, Self::Green];

    fn lit(self) -> palette::Rgb {
        match self {
            Self::Red => palette::hex(0xF0503C),
            Self::Yellow => palette::hex(0xF2C14B),
            Self::Green => palette::hex(0x5BE38A),
        }
    }
}

/// Where a signal head's lamps are: centre of each lamp face and the direction it faces.
#[derive(Clone, Debug, PartialEq)]
pub struct LampSlots {
    /// Approach index of the junction.
    pub approach: usize,
    /// Lamp centres (red, yellow, green).
    pub centers: [Vec3; 3],
    /// Facing (toward arriving traffic).
    pub facing: Vec2,
}

fn up(p: Vec3) -> Vec3 {
    p + Vec3::Y * DECAL_LIFT_M
}

/// Decals for one junction's markings.
pub fn paint_markings(p: &mut Painter, m: &Markings) {
    let col = palette::MARKING;
    for cw in &m.crosswalks {
        let [a, b] = cw.ends;
        let across = Vec2::new(b.x - a.x, b.z - a.z);
        let w = across.length();
        if w < 1.0 {
            continue;
        }
        let half_len = cw.along * (0.5 * cw.width_m);
        let n = (w / STRIPE_PERIOD_M).floor() as usize;
        let pad = 0.5 * (w - n as f32 * STRIPE_PERIOD_M + STRIPE_PERIOD_M - STRIPE_W_M);
        for k in 0..n {
            let t0 = pad + k as f32 * STRIPE_PERIOD_M;
            let t1 = t0 + STRIPE_W_M;
            let q = |t: f32, sgn: f32| {
                let base = a.lerp(b, t / w);
                let o = half_len * sgn;
                up(base + Vec3::new(o.x, 0.0, o.y))
            };
            p.face_ao(
                &[q(t0, -1.0), q(t1, -1.0), q(t1, 1.0), q(t0, 1.0)],
                &[1.0; 4],
                col,
                Surface::Paved,
                0,
                Vec3::Y,
            );
        }
    }
    for sl in &m.stop_lines {
        stop_line(p, sl);
    }
    for ar in &m.arrows {
        arrow(p, ar);
    }
    for is in &m.islands {
        island(p, is);
    }
}

/// Give-way dash length and gap across the road (m); shark tooth base and length (m).
const DASH_M: f32 = 0.6;
const DASH_GAP_M: f32 = 0.3;
const TOOTH_BASE_M: f32 = 0.6;
const TOOTH_LEN_M: f32 = 0.9;

fn stop_line(p: &mut Painter, sl: &StopLine) {
    let h = sl.along * (0.5 * cb_network::markings::STOP_LINE_WIDTH_M);
    let o = Vec3::new(h.x, 0.0, h.y);
    let [a, b] = sl.ends;
    let quad = |p: &mut Painter, a: Vec3, b: Vec3| {
        p.face_ao(
            &[up(a - o), up(b - o), up(b + o), up(a + o)],
            &[1.0; 4],
            palette::MARKING,
            Surface::Paved,
            0,
            Vec3::Y,
        );
    };
    match sl.kind {
        LineKind::Solid => quad(p, a, b),
        LineKind::GiveWay => {
            let w = Vec2::new(b.x - a.x, b.z - a.z).length();
            let period = DASH_M + DASH_GAP_M;
            let n = ((w + DASH_GAP_M) / period).floor().max(1.0) as usize;
            let pad = 0.5 * (w - (n as f32 * period - DASH_GAP_M)).max(0.0);
            // Teeth point upstream (toward arriving drivers), one per dash.
            let tip =
                sl.along * (0.5 * cb_network::markings::STOP_LINE_WIDTH_M + 0.3 + TOOTH_LEN_M);
            let base = sl.along * (0.5 * cb_network::markings::STOP_LINE_WIDTH_M + 0.3);
            for k in 0..n {
                let t0 = (pad + k as f32 * period) / w.max(1e-3);
                let t1 = (pad + k as f32 * period + DASH_M) / w.max(1e-3);
                let (d0, d1) = (a.lerp(b, t0), a.lerp(b, t1.min(1.0)));
                quad(p, d0, d1);
                let mid = d0.lerp(d1, 0.5);
                let half = (d1 - d0).normalize_or_zero() * (0.5 * TOOTH_BASE_M);
                let b3 = Vec3::new(base.x, 0.0, base.y);
                let t3 = Vec3::new(tip.x, 0.0, tip.y);
                p.face_ao(
                    &[up(mid - half + b3), up(mid + t3), up(mid + half + b3)],
                    &[1.0; 3],
                    palette::MARKING,
                    Surface::Paved,
                    0,
                    Vec3::Y,
                );
            }
        }
    }
}

/// Raised splitter island: kerb walls around the outline and a paved top.
fn island(p: &mut Painter, is: &Island) {
    let h = cb_network::markings::ISLAND_HEIGHT_M;
    let n = is.left.len().min(is.right.len());
    if n < 2 {
        return;
    }
    let lift = |v: Vec3| v + Vec3::Y * h;
    for k in 0..n - 1 {
        let (l0, l1, r0, r1) = (is.left[k], is.left[k + 1], is.right[k], is.right[k + 1]);
        p.face_ao(
            &[lift(r0), lift(r1), lift(l1), lift(l0)],
            &[1.0; 4],
            palette::PAVING,
            Surface::Paved,
            0,
            Vec3::Y,
        );
        for (a, b, ref_pt) in [(l0, l1, r0), (r1, r0, l0)] {
            let along = b - a;
            let out = Vec3::new(along.z, 0.0, -along.x).normalize_or_zero();
            let out = if out.dot(a - ref_pt) < 0.0 { -out } else { out };
            p.face_ao(
                &[a, b, lift(b), lift(a)],
                &[0.8, 0.8, 1.0, 1.0],
                palette::CURB,
                Surface::Solid,
                0,
                out,
            );
        }
    }
    // End caps.
    for (l, r, sgn) in [
        (is.left[0], is.right[0], -1.0f32),
        (is.left[n - 1], is.right[n - 1], 1.0),
    ] {
        let other = if sgn < 0.0 {
            is.left[1]
        } else {
            is.left[n - 2]
        };
        let out = (l - other) * Vec3::new(1.0, 0.0, 1.0);
        p.face_ao(
            &[l, r, lift(r), lift(l)],
            &[0.8, 0.8, 1.0, 1.0],
            palette::CURB,
            Surface::Solid,
            0,
            out.normalize_or_zero(),
        );
    }
}

/// Lane arrow: shaft, straight head and left/right branches in a local frame (x right, y in
/// travel direction), scaled to `ARROW_LENGTH_M`.
fn arrow(p: &mut Painter, ar: &LaneArrow) {
    let fwd = ar.dir;
    let right = Vec2::new(-fwd.y, fwd.x) * -1.0;
    let s = cb_network::markings::ARROW_LENGTH_M / 3.4;
    let to3 = |x: f32, y: f32| {
        let q = Vec2::new(ar.center.x, ar.center.z) + (right * x + fwd * y) * s;
        up(Vec3::new(q.x, ar.center.y, q.y))
    };
    let mut face = |pts: &[Vec3]| {
        let ao = vec![1.0; pts.len()];
        p.face_ao(pts, &ao, palette::MARKING, Surface::Paved, 0, Vec3::Y);
    };
    let ArrowTurns {
        left,
        straight,
        right: turn_right,
    } = ar.turns;
    let top = if straight { 0.6 } else { -0.2 };
    face(&[
        to3(-0.1, -2.0),
        to3(0.1, -2.0),
        to3(0.1, top),
        to3(-0.1, top),
    ]);
    if straight {
        face(&[to3(-0.35, 0.6), to3(0.35, 0.6), to3(0.0, 1.4)]);
    }
    for (on, sgn) in [(left, -1.0f32), (turn_right, 1.0)] {
        if !on {
            continue;
        }
        let (x0, x1) = (0.0f32, 0.7 * sgn);
        let (lo, hi) = (x0.min(x1), x0.max(x1));
        face(&[to3(lo, -0.4), to3(hi, -0.4), to3(hi, -0.2), to3(lo, -0.2)]);
        face(&[to3(x1, -0.65), to3(x1 + 0.6 * sgn, -0.3), to3(x1, 0.05)]);
    }
}

/// Signal poles and heads (lamps unlit) at the stop lines; returns the lamp slots.
pub fn paint_signals(p: &mut Painter, stop_lines: &[StopLine]) -> Vec<LampSlots> {
    let dark = palette::hex(0x2B2E33);
    let mut slots = Vec::new();
    for sl in stop_lines {
        let base = sl.kerb;
        let f = sl.along;
        let side = Vec2::new(-f.y, f.x);
        let rect = |c: Vec2, hw: f32, hd: f32| {
            vec![
                c - side * hw - f * hd,
                c + side * hw - f * hd,
                c + side * hw + f * hd,
                c - side * hw + f * hd,
            ]
        };
        let c = Vec2::new(base.x, base.z);
        p.prism(
            &rect(c, 0.07, 0.07),
            base.y,
            base.y + POLE_H_M,
            dark,
            None,
            false,
        );
        let head_c = c + f * 0.15;
        let y1 = base.y + POLE_H_M + 0.2;
        p.prism(
            &rect(head_c, 0.5 * HEAD_W_M, 0.12),
            y1 - HEAD_H_M,
            y1,
            dark,
            None,
            false,
        );
        let mid_y = y1 - 0.5 * HEAD_H_M;
        let face_c = head_c + f * 0.13;
        let mut centers = [Vec3::ZERO; 3];
        for (k, lamp) in Lamp::ALL.iter().enumerate() {
            let y = mid_y + (1.0 - k as f32) * LAMP_STEP_M;
            centers[k] = Vec3::new(face_c.x, y, face_c.y);
            // Unlit lamp: a dim version of its colour.
            lamp_quad(p, centers[k], f, lamp.lit().scale(0.25));
        }
        slots.push(LampSlots {
            approach: sl.approach,
            centers,
            facing: f,
        });
    }
    slots
}

fn lamp_quad(p: &mut Painter, c: Vec3, facing: Vec2, col: palette::Rgb) {
    let side = Vec3::new(-facing.y, 0.0, facing.x) * (0.5 * LAMP_M);
    let upv = Vec3::Y * (0.5 * LAMP_M);
    let out = Vec3::new(facing.x, 0.0, facing.y);
    p.face_ao(
        &[
            c - side - upv,
            c + side - upv,
            c + side + upv,
            c - side + upv,
        ],
        &[1.0; 4],
        col,
        Surface::Solid,
        0,
        out,
    );
}

/// A lit lamp face (drawn slightly in front of the unlit slot).
#[must_use]
pub fn lit_lamp_mesh(slot: &LampSlots, lamp: Lamp) -> MeshData {
    let mut p = Painter::new(0, 0x1A3F);
    let k = Lamp::ALL.iter().position(|l| *l == lamp).unwrap_or(0);
    let c = slot.centers[k] + Vec3::new(slot.facing.x, 0.0, slot.facing.y) * 0.01;
    lamp_quad(&mut p, c, slot.facing, lamp.lit());
    p.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cb_geometry::Polyline2;
    use cb_network::connectors::approach_lanes;
    use cb_network::junction::{Approach, build};
    use cb_network::markings::markings;
    use cb_network::{CrossSection, Profile, RoadKind};

    #[test]
    fn markings_and_signals_mesh() {
        let cs = CrossSection::of(RoadKind::Street);
        let apps: Vec<Approach> = [Vec2::X, Vec2::Y, -Vec2::X, -Vec2::Y]
            .iter()
            .map(|d| Approach {
                centerline: Polyline2::new((0..=40).map(|k| *d * (k as f32 * 2.5))),
                half_width_m: cs.half_width_m(),
                carriageway_half_m: cs.carriageway_half_m(),
                corner_radius_m: 6.0,
                profile: Profile::flat(100.0, 3.0),
                reversed: false,
                lanes: approach_lanes(&cs, true),
            })
            .collect();
        let j = build(&apps).unwrap();
        let m = markings(&apps, &j, &[]);
        let mut p = Painter::new(0, 1);
        paint_markings(&mut p, &m);
        let slots = paint_signals(&mut p, &m.stop_lines);
        let md = p.finish();
        assert!(md.triangle_count() > 50 && md.triangle_count() < 1500);
        let (lo, hi) = md.bounds();
        assert!(lo.y >= 3.0 - 1e-3 && hi.y <= 3.0 + POLE_H_M + 0.3);
        assert_eq!(slots.len(), 4);
        // Poles stand outside the carriageway (on the sidewalk) and heads face arriving traffic.
        for s in &slots {
            let c = Vec2::new(s.centers[0].x, s.centers[0].z);
            let a = &apps[s.approach];
            let (_, d) = a.centerline.project(c);
            assert!(d.abs() > cs.carriageway_half_m(), "{d}");
            assert!(s.centers[0].y > s.centers[2].y);
            assert!(lit_lamp_mesh(s, Lamp::Green).triangle_count() == 2);
        }
    }
}
