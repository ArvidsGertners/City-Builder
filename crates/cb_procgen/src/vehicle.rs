//! Vehicles (ALGORITHMS §11, ART_DIRECTION §5 "Vehicles"): parametric low-poly profiles built
//! from a body slab, a sloped cabin frustum, boxes for loads and 8-sided wheels; emissive light
//! faces use the window surface so they glow at night. Meshes are built in a local frame (x
//! forward, y up, z across) and placed at a ground point with a yaw; the tram returns one mesh
//! per articulated section.

use cb_core::rng::Pcg32;
use cb_geometry::MeshData;
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::paint::Painter;
use crate::palette::{self, Rgb, hex};
use crate::surface::Surface;

/// Vehicle kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum VehicleKind {
    /// Small car with a steep tailgate.
    Hatchback,
    /// Three-box car.
    Sedan,
    /// Tall car.
    Suv,
    /// Cab and open bed.
    Pickup,
    /// Delivery van.
    Van,
    /// Box truck (goods deliveries).
    BoxTruck,
    /// Fire engine with ladder.
    FireTruck,
    /// Ambulance (box van).
    Ambulance,
    /// Police car.
    Police,
    /// Refuse collection truck.
    GarbageTruck,
    /// 12 m city bus.
    Bus,
    /// Three-section articulated tram.
    Tram,
}

impl VehicleKind {
    /// Every kind, in gallery order.
    pub const ALL: [Self; 12] = [
        Self::Hatchback,
        Self::Sedan,
        Self::Suv,
        Self::Pickup,
        Self::Van,
        Self::BoxTruck,
        Self::FireTruck,
        Self::Ambulance,
        Self::Police,
        Self::GarbageTruck,
        Self::Bus,
        Self::Tram,
    ];

    /// Overall length (m), all sections and gaps included.
    #[must_use]
    pub const fn length_m(self) -> f32 {
        match self {
            Self::Hatchback => 3.9,
            Self::Sedan | Self::Police => 4.6,
            Self::Suv => 4.7,
            Self::Pickup => 5.3,
            Self::Van => 5.0,
            Self::Ambulance => 5.8,
            Self::BoxTruck => 7.5,
            Self::FireTruck => 9.0,
            Self::GarbageTruck => 8.5,
            Self::Bus => 12.0,
            Self::Tram => 3.0 * TRAM_SECTION_M + 2.0 * TRAM_GAP_M,
        }
    }

    /// Body width (m).
    #[must_use]
    pub const fn width_m(self) -> f32 {
        match self {
            Self::Hatchback => 1.75,
            Self::Sedan | Self::Police => 1.8,
            Self::Suv | Self::Pickup => 1.9,
            Self::Van => 2.0,
            Self::Ambulance => 2.2,
            Self::BoxTruck | Self::FireTruck | Self::GarbageTruck => 2.45,
            Self::Bus | Self::Tram => 2.55,
        }
    }

    /// Articulated sections (1 except the tram).
    #[must_use]
    pub const fn sections(self) -> usize {
        match self {
            Self::Tram => 3,
            _ => 1,
        }
    }
}

/// Tram section length (m).
pub const TRAM_SECTION_M: f32 = 10.0;
/// Gap between tram sections (bellows, m).
pub const TRAM_GAP_M: f32 = 0.6;

/// Car body colours (ART_DIRECTION §5).
pub const CAR_COLORS: [Rgb; 10] = [
    hex(0xC8553D),
    hex(0x3E6E8E),
    hex(0xE8E1D3),
    hex(0x4F5D4A),
    hex(0xD9A441),
    hex(0x6B6E73),
    hex(0x2F3A45),
    hex(0x9C3D54),
    hex(0x7FA0B5),
    hex(0xB9A58A),
];
/// Truck boxes.
pub const TRUCK_BOX: Rgb = hex(0xD9D4C7);
/// Fire engine red.
pub const FIRE_RED: Rgb = hex(0xC0453A);
/// Ambulance body and stripe.
pub const AMBULANCE: [Rgb; 2] = [hex(0xF2F0EB), hex(0x4FA38F)];
/// Police body and band.
pub const POLICE: [Rgb; 2] = [hex(0xE9ECEC), hex(0x3D5A8C)];
/// Garbage truck green.
pub const GARBAGE: Rgb = hex(0x6E8B4E);
/// Bus body; the band takes the line colour.
pub const BUS_BODY: Rgb = hex(0xE8E1D3);
/// Tram body; the lower body takes the line colour.
pub const TRAM_BODY: Rgb = hex(0xF0E9DA);
/// Default line colour (bus depot teal) until transit lines exist.
pub const DEFAULT_LINE: Rgb = hex(0x3F8F8B);

const TYRE: Rgb = hex(0x2F3338);
const HUB: Rgb = hex(0x6B6E73);
const TAIL: Rgb = hex(0xB8402F);
const BEACON_BLUE: Rgb = hex(0x4F7FD9);
const BEACON_RED: Rgb = hex(0xD9503F);
const TRIM: Rgb = hex(0x3C4046);

/// Meshes of a vehicle standing at `base` (ground under its centre), heading `yaw` (radians,
/// 0 = +x): one per section, front section first.
#[must_use]
pub fn meshes(kind: VehicleKind, base: Vec3, yaw: f32, seed: u64, slot: u16) -> Vec<MeshData> {
    let mut rng = Pcg32::from_seed(seed);
    let paint = CAR_COLORS[rng.below(CAR_COLORS.len() as u32) as usize];
    let (s, c) = cb_core::math::sin_cos(yaw);
    let place = |mut m: MeshData| {
        for p in &mut m.positions {
            let (x, z) = (p[0], p[2]);
            p[0] = base.x + c * x - s * z;
            p[1] += base.y;
            p[2] = base.z + s * x + c * z;
        }
        m
    };
    let mut out = Vec::with_capacity(kind.sections());
    if kind == VehicleKind::Tram {
        let half = 0.5 * kind.length_m();
        for k in 0..3 {
            let x0 = -half + k as f32 * (TRAM_SECTION_M + TRAM_GAP_M);
            let mut p = Painter::new(slot, seed ^ k as u64);
            p.contact_y = 0.0;
            tram_section(&mut p, x0, x0 + TRAM_SECTION_M, k);
            out.push(place(p.finish()));
        }
        return out;
    }
    let mut p = Painter::new(slot, seed);
    p.contact_y = 0.0;
    build(&mut p, kind, paint);
    out.push(place(p.finish()));
    out
}

/// One builder per kind (local frame, centred on the origin).
fn build(p: &mut Painter, kind: VehicleKind, paint: Rgb) {
    let l = kind.length_m();
    let w = kind.width_m();
    let (f, r) = (0.5 * l, -0.5 * l);
    match kind {
        VehicleKind::Hatchback | VehicleKind::Sedan | VehicleKind::Suv | VehicleKind::Police => {
            let (body, band) = if kind == VehicleKind::Police {
                (POLICE[0], Some(POLICE[1]))
            } else {
                (paint, None)
            };
            let (wr, y_body, y_top) = match kind {
                VehicleKind::Suv => (0.38, 1.05, 1.78),
                _ => (0.31, 0.82, 1.42),
            };
            slab(p, r, f, 0.3, y_body, w, body);
            if let Some(b) = band {
                // Livery band just under the window line.
                slab(p, r - 0.01, f + 0.01, 0.62, 0.74, w + 0.02, b);
            }
            let (c0, c1, t0, t1) = match kind {
                VehicleKind::Hatchback => (r + 0.15, f - 1.25, r + 0.35, f - 1.95),
                VehicleKind::Suv => (r + 0.25, f - 1.3, r + 0.4, f - 1.9),
                _ => (r + 0.85, f - 1.45, r + 1.3, f - 2.1),
            };
            cabin(
                p,
                [c0, c1],
                [t0, t1],
                y_body,
                y_top,
                w - 0.08,
                w - 0.3,
                body,
            );
            wheels(p, &[r + 0.75, f - 0.8], w, wr);
            bumpers(p, r, f, w, 0.3);
            lights(p, r, f, w, 0.55, 0.7);
            if kind == VehicleKind::Police {
                beacon(p, 0.5 * (t0 + t1), y_top, BEACON_BLUE, BEACON_BLUE);
            }
        }
        VehicleKind::Pickup => {
            slab(p, r, f, 0.35, 1.0, w, paint);
            cabin(
                p,
                [f - 2.9, f - 1.5],
                [f - 2.8, f - 2.0],
                1.0,
                1.75,
                w - 0.06,
                w - 0.25,
                paint,
            );
            // Bed walls (open top).
            slab(p, r, f - 3.0, 1.0, 1.35, 0.12, paint);
            for z in [-0.5 * w + 0.06, 0.5 * w - 0.06] {
                slab_at(p, r, f - 3.0, 1.0, 1.35, z, 0.12, paint);
            }
            wheels(p, &[r + 0.9, f - 0.95], w, 0.38);
            bumpers(p, r, f, w, 0.35);
            lights(p, r, f, w, 0.65, 0.8);
        }
        VehicleKind::Van | VehicleKind::Ambulance => {
            let (body, stripe) = if kind == VehicleKind::Ambulance {
                (AMBULANCE[0], Some(AMBULANCE[1]))
            } else {
                (
                    if paint == CAR_COLORS[6] {
                        TRUCK_BOX
                    } else {
                        paint
                    },
                    None,
                )
            };
            let h = if kind == VehicleKind::Ambulance {
                2.55
            } else {
                2.15
            };
            slab(p, r, f - 0.9, 0.35, h, w, body);
            // Sloped nose with the windscreen.
            cabin(
                p,
                [f - 0.9, f],
                [f - 0.9, f - 0.9],
                0.35,
                h - 0.25,
                w,
                w - 0.1,
                body,
            );
            cabin(
                p,
                [f - 0.9, f - 0.35],
                [f - 0.9, f - 0.75],
                1.1,
                h - 0.3,
                w + 0.02,
                w - 0.08,
                body,
            );
            if let Some(s) = stripe {
                slab(p, r - 0.01, f - 0.89, 1.0, 1.25, w + 0.02, s);
            }
            wheels(p, &[r + 0.95, f - 1.0], w, 0.36);
            bumpers(p, r, f, w, 0.35);
            lights(p, r, f, w, 0.65, 0.8);
            if kind == VehicleKind::Ambulance {
                beacon(p, f - 1.2, h, BEACON_BLUE, BEACON_RED);
            }
        }
        VehicleKind::BoxTruck | VehicleKind::FireTruck | VehicleKind::GarbageTruck => {
            let (cab, load) = match kind {
                VehicleKind::FireTruck => (FIRE_RED, FIRE_RED),
                VehicleKind::GarbageTruck => (GARBAGE, GARBAGE),
                _ => (paint, TRUCK_BOX),
            };
            // Chassis, cab, load.
            slab(p, r + 0.2, f - 0.2, 0.55, 0.9, w - 0.4, TRIM);
            slab(p, f - 2.1, f, 0.55, 2.0, w, cab);
            cabin(
                p,
                [f - 2.1, f],
                [f - 2.1, f - 0.25],
                2.0,
                2.9,
                w,
                w - 0.1,
                cab,
            );
            let top = match kind {
                VehicleKind::FireTruck => 2.6,
                VehicleKind::GarbageTruck => 3.2,
                _ => 3.4,
            };
            if kind == VehicleKind::GarbageTruck {
                // Compactor body tapering to the rear hopper.
                cabin(
                    p,
                    [r + 0.6, f - 2.3],
                    [r + 0.9, f - 2.3],
                    0.9,
                    top,
                    w,
                    w - 0.25,
                    load,
                );
                slab(p, r, r + 0.6, 0.7, 2.4, w - 0.1, TRIM);
            } else {
                slab(p, r, f - 2.3, 0.9, top, w, load);
            }
            if kind == VehicleKind::FireTruck {
                // Ladder on the roof and a white side stripe.
                slab(
                    p,
                    r - 0.6,
                    f - 2.6,
                    top,
                    top + 0.25,
                    0.9,
                    palette::WINDOW_FRAME,
                );
                slab(
                    p,
                    r - 0.01,
                    f + 0.01,
                    1.35,
                    1.5,
                    w + 0.02,
                    palette::WINDOW_FRAME,
                );
                beacon(p, f - 1.1, 2.9, BEACON_BLUE, BEACON_BLUE);
            }
            let axles: &[f32] = match kind {
                VehicleKind::FireTruck => &[r + 1.4, r + 2.6, f - 1.2],
                VehicleKind::GarbageTruck => &[r + 1.6, r + 2.8, f - 1.2],
                _ => &[r + 1.4, f - 1.2],
            };
            wheels(p, axles, w, 0.5);
            bumpers(p, r, f, w, 0.5);
            lights(p, r, f, w, 0.8, 0.95);
        }
        VehicleKind::Bus => {
            slab(p, r, f, 0.35, 1.05, w, BUS_BODY);
            // Window band (glass) and roof.
            glass_band(p, r + 0.3, f - 0.05, 1.05, 2.35, w);
            slab(p, r, f, 2.35, 3.1, w, BUS_BODY);
            slab(p, r - 0.01, f + 0.01, 0.75, 1.0, w + 0.02, DEFAULT_LINE);
            slab(p, r + 3.0, r + 6.0, 3.1, 3.35, w - 0.6, TRIM);
            wheels(p, &[r + 2.6, f - 2.6], w, 0.5);
            bumpers(p, r, f, w, 0.35);
            lights(p, r, f, w, 0.6, 0.8);
        }
        VehicleKind::Tram => {}
    }
}

/// One tram section between `x0` and `x1` (`k` = 0 front, 2 rear).
fn tram_section(p: &mut Painter, x0: f32, x1: f32, k: usize) {
    let w = VehicleKind::Tram.width_m();
    slab(p, x0, x1, 0.35, 1.0, w, DEFAULT_LINE);
    glass_band(p, x0 + 0.2, x1 - 0.2, 1.0, 2.4, w);
    slab(p, x0, x1, 2.4, 3.25, w, TRAM_BODY);
    // Bogie under the section centre (wheels hidden by skirts: two low boxes).
    let m = 0.5 * (x0 + x1);
    slab(p, m - 1.2, m + 1.2, 0.05, 0.35, w - 0.5, TRIM);
    if k == 1 {
        // Pantograph on the middle section.
        slab(p, m - 1.0, m + 1.0, 3.25, 3.45, 1.2, TRIM);
        slab(p, m - 0.1, m + 0.1, 3.45, 4.1, 0.9, TRIM);
    }
    if k == 0 {
        lights(p, f32::NEG_INFINITY, x1, w, 0.6, 0.8);
    } else if k == 2 {
        lights(p, x0, f32::INFINITY, w, 0.6, 0.8);
    }
}

fn rect(x0: f32, x1: f32, z0: f32, z1: f32) -> [Vec2; 4] {
    [
        Vec2::new(x0, z0),
        Vec2::new(x1, z0),
        Vec2::new(x1, z1),
        Vec2::new(x0, z1),
    ]
}

/// Box over `[x0, x1]` × width `w` (centred) between `y0` and `y1`, with a top.
fn slab(p: &mut Painter, x0: f32, x1: f32, y0: f32, y1: f32, w: f32, c: Rgb) {
    slab_at(p, x0, x1, y0, y1, 0.0, w, c);
}

/// Like [`slab`] centred on `z`.
#[allow(clippy::too_many_arguments)]
fn slab_at(p: &mut Painter, x0: f32, x1: f32, y0: f32, y1: f32, z: f32, w: f32, c: Rgb) {
    let q = rect(x0, x1, z - 0.5 * w, z + 0.5 * w);
    p.prism(&q, y0, y1, c, Some(c), false);
}

/// Cabin frustum: bottom spans `bx` with width `wb` at `y0`, top spans `tx` with width `wt` at
/// `y1`. Front, rear and sides are glass; the top takes the body colour.
#[allow(clippy::too_many_arguments)]
fn cabin(p: &mut Painter, bx: [f32; 2], tx: [f32; 2], y0: f32, y1: f32, wb: f32, wt: f32, c: Rgb) {
    let (hb, ht) = (0.5 * wb, 0.5 * wt);
    let b = [
        Vec3::new(bx[0], y0, -hb),
        Vec3::new(bx[1], y0, -hb),
        Vec3::new(bx[1], y0, hb),
        Vec3::new(bx[0], y0, hb),
    ];
    let t = [
        Vec3::new(tx[0], y1, -ht),
        Vec3::new(tx[1], y1, -ht),
        Vec3::new(tx[1], y1, ht),
        Vec3::new(tx[0], y1, ht),
    ];
    let glass = palette::GLASS;
    p.face(
        &[b[1], b[2], t[2], t[1]],
        glass,
        Surface::Window,
        1,
        Vec3::X,
    );
    p.face(
        &[b[0], b[3], t[3], t[0]],
        glass,
        Surface::Window,
        2,
        -Vec3::X,
    );
    p.face(
        &[b[0], b[1], t[1], t[0]],
        glass,
        Surface::Window,
        3,
        -Vec3::Z,
    );
    p.face(
        &[b[3], b[2], t[2], t[3]],
        glass,
        Surface::Window,
        3,
        Vec3::Z,
    );
    p.face(&t, c, Surface::Roof, 0, Vec3::Y);
}

/// Glass all round between `y0` and `y1` (bus and tram window bands).
fn glass_band(p: &mut Painter, x0: f32, x1: f32, y0: f32, y1: f32, w: f32) {
    let h = 0.5 * w - 0.03;
    let v = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
    let g = palette::GLASS;
    for (z, n) in [(-h, -Vec3::Z), (h, Vec3::Z)] {
        p.face(
            &[v(x0, y0, z), v(x1, y0, z), v(x1, y1, z), v(x0, y1, z)],
            g,
            Surface::Window,
            4,
            n,
        );
    }
    p.face(
        &[v(x1, y0, -h), v(x1, y0, h), v(x1, y1, h), v(x1, y1, -h)],
        g,
        Surface::Window,
        1,
        Vec3::X,
    );
    p.face(
        &[v(x0, y0, -h), v(x0, y0, h), v(x0, y1, h), v(x0, y1, -h)],
        g,
        Surface::Window,
        2,
        -Vec3::X,
    );
}

/// Pairs of 8-sided wheels at the axle positions `xs`.
fn wheels(p: &mut Painter, xs: &[f32], w: f32, radius: f32) {
    let ring: Vec<Vec2> = (0..8)
        .map(|k| {
            let a = (k as f32 + 0.5) * std::f32::consts::TAU / 8.0;
            let (s, c) = cb_core::math::sin_cos(a);
            Vec2::new(c, s) * radius
        })
        .collect();
    for &x in xs {
        for side in [-1.0f32, 1.0] {
            let zo = side * (0.5 * w + 0.02);
            let zi = side * (0.5 * w - 0.24);
            let at = |q: Vec2, z: f32| Vec3::new(x + q.x, radius + q.y, z);
            for k in 0..8 {
                let (a, b) = (ring[k], ring[(k + 1) % 8]);
                let m = (a + b) * 0.5;
                p.face(
                    &[at(a, zi), at(b, zi), at(b, zo), at(a, zo)],
                    TYRE,
                    Surface::Solid,
                    0,
                    Vec3::new(m.x, m.y, 0.0),
                );
            }
            let outer: Vec<Vec3> = ring.iter().map(|q| at(*q, zo)).collect();
            p.face(&outer, HUB, Surface::Solid, 0, Vec3::Z * side);
            let inner: Vec<Vec3> = ring.iter().map(|q| at(*q, zi)).collect();
            p.face(&inner, TYRE, Surface::Solid, 0, -Vec3::Z * side);
        }
    }
}

/// Front and rear bumpers just above `y`.
fn bumpers(p: &mut Painter, r: f32, f: f32, w: f32, y: f32) {
    slab(p, f - 0.05, f + 0.08, y, y + 0.22, w - 0.04, TRIM);
    slab(p, r - 0.08, r + 0.05, y, y + 0.22, w - 0.04, TRIM);
}

/// Head lights on the front face (`f`) and tail lights on the rear face (`r`); infinite
/// positions skip that end.
fn lights(p: &mut Painter, r: f32, f: f32, w: f32, y0: f32, y1: f32) {
    let h = 0.5 * w;
    for side in [-1.0f32, 1.0] {
        let (z0, z1) = (side * (h - 0.12), side * (h - 0.42));
        if f.is_finite() {
            let x = f + 0.09;
            let q = [
                Vec3::new(x, y0, z0),
                Vec3::new(x, y0, z1),
                Vec3::new(x, y1, z1),
                Vec3::new(x, y1, z0),
            ];
            p.face_exact(
                &q,
                &[1.0; 4],
                palette::LAMP_GLASS,
                Surface::Window,
                5,
                Vec3::X,
            );
        }
        if r.is_finite() {
            let x = r - 0.09;
            let q = [
                Vec3::new(x, y0, z0),
                Vec3::new(x, y0, z1),
                Vec3::new(x, y1, z1),
                Vec3::new(x, y1, z0),
            ];
            p.face_exact(&q, &[1.0; 4], TAIL, Surface::Window, 6, -Vec3::X);
        }
    }
}

/// Roof light bar centred at `x` on a roof at `y`.
fn beacon(p: &mut Painter, x: f32, y: f32, left: Rgb, right: Rgb) {
    slab_at(p, x - 0.2, x + 0.2, y, y + 0.14, -0.3, 0.55, left);
    slab_at(p, x - 0.2, x + 0.2, y, y + 0.14, 0.3, 0.55, right);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_within_triangle_budget_and_footprint() {
        for kind in VehicleKind::ALL {
            let ms = meshes(kind, Vec3::ZERO, 0.0, 7, 0);
            assert_eq!(ms.len(), kind.sections(), "{kind:?}");
            // ART_DIRECTION §2: 150–400 per vehicle; tram sections are plain boxes, so the
            // lower bound holds for the whole tram and the upper one per section.
            let total: usize = ms.iter().map(MeshData::triangle_count).sum();
            assert!(total >= 150, "{kind:?}: {total} tris");
            for m in &ms {
                let t = m.triangle_count();
                assert!(t <= 400, "{kind:?}: {t} tris");
            }
            let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
            for m in &ms {
                let (a, b) = m.bounds();
                lo = lo.min(a);
                hi = hi.max(b);
            }
            let len = hi.x - lo.x;
            assert!(
                (len - kind.length_m()).abs() < 1.5,
                "{kind:?}: length {len} vs {}",
                kind.length_m()
            );
            assert!(hi.z - lo.z < kind.width_m() + 0.2, "{kind:?}: too wide");
            assert!(lo.y >= -1e-4, "{kind:?}: below ground");
        }
    }

    #[test]
    fn placement_rotates_and_translates() {
        let a = meshes(VehicleKind::Sedan, Vec3::ZERO, 0.0, 3, 0);
        let b = meshes(
            VehicleKind::Sedan,
            Vec3::new(10.0, 2.0, -5.0),
            std::f32::consts::FRAC_PI_2,
            3,
            0,
        );
        let (lo, hi) = b[0].bounds();
        let (la, ha) = a[0].bounds();
        // Heading +z after a quarter turn: the length now spans z.
        assert!((hi.z - lo.z - (ha.x - la.x)).abs() < 1e-3);
        assert!((lo.y - (la.y + 2.0)).abs() < 1e-4);
        assert!(((lo.x + hi.x) * 0.5 - 10.0).abs() < 0.2);
    }

    #[test]
    fn deterministic_per_seed() {
        let a = meshes(VehicleKind::Hatchback, Vec3::ZERO, 0.3, 11, 2);
        let b = meshes(VehicleKind::Hatchback, Vec3::ZERO, 0.3, 11, 2);
        assert_eq!(a[0].positions, b[0].positions);
        assert_eq!(a[0].colors, b[0].colors);
    }
}
