//! Condition meshes (ALGORITHMS §11 step 8): the scaffold shown around a building under
//! construction (the shader's reveal clamp cuts it at the same height as the building) and the
//! ruin that replaces a collapsed building's mesh.

use cb_core::rng::Pcg32;
use cb_geometry::MeshData;
use glam::{Vec2, Vec3};

use crate::paint::Painter;
use crate::palette::{Rgb, hex};
use crate::surface::Surface;

/// Scaffold distance from the walls (m).
const SCAFFOLD_GAP_M: f32 = 0.9;
/// Scaffold pole spacing (m).
const POLE_SPACING_M: f32 = 2.5;
/// Scaffold deck spacing (m).
const DECK_SPACING_M: f32 = 2.0;
/// Scaffold metal.
const SCAFFOLD_METAL: Rgb = hex(0x8A9099);
/// Scaffold boards.
const SCAFFOLD_BOARDS: Rgb = hex(0xC9A46B);
/// Ruin rubble.
const RUBBLE: Rgb = hex(0x8C857A);

/// Outward unit normal of edge `a → b` of a polygon with centroid `c`.
fn outward(a: Vec2, b: Vec2, c: Vec2) -> Vec2 {
    let e = (b - a).normalize_or_zero();
    let n = Vec2::new(e.y, -e.x);
    if n.dot((a + b) * 0.5 - c) < 0.0 {
        -n
    } else {
        n
    }
}

/// Scaffold around every outline from `y0` up to `y1` (poles, ledgers, boards).
#[must_use]
pub fn scaffold(outlines: &[Vec<Vec2>], y0: f32, y1: f32, slot: u16) -> MeshData {
    let mut p = Painter::new(slot, 0x5CAF);
    p.contact_y = y0;
    for o in outlines {
        let n = o.len();
        if n < 3 {
            continue;
        }
        let c = o.iter().copied().sum::<Vec2>() / n as f32;
        for k in 0..n {
            let (a, b) = (o[k], o[(k + 1) % n]);
            let len = a.distance(b);
            if len < 1.0 {
                continue;
            }
            let out = outward(a, b, c);
            let e = (b - a) / len;
            let (pa, pb) = (a + out * SCAFFOLD_GAP_M, b + out * SCAFFOLD_GAP_M);
            let poles = (len / POLE_SPACING_M).ceil().max(1.0) as usize;
            for i in 0..=poles {
                let q = pa + (pb - pa) * (i as f32 / poles as f32);
                let r = 0.05;
                p.prism(
                    &[
                        q + Vec2::new(-r, -r),
                        q + Vec2::new(r, -r),
                        q + Vec2::new(r, r),
                        q + Vec2::new(-r, r),
                    ],
                    y0,
                    y1,
                    SCAFFOLD_METAL,
                    None,
                    false,
                );
            }
            // Decks: a board on each level, a ledger at handrail height.
            let levels = ((y1 - y0) / DECK_SPACING_M).floor() as usize;
            for l in 1..=levels {
                let y = y0 + DECK_SPACING_M * l as f32;
                let w = out * 0.35;
                let q = [
                    pa - e * 0.1 - w,
                    pb + e * 0.1 - w,
                    pb + e * 0.1 + w,
                    pa - e * 0.1 + w,
                ];
                p.prism(
                    &q,
                    y - 0.06,
                    y,
                    SCAFFOLD_BOARDS,
                    Some(SCAFFOLD_BOARDS),
                    false,
                );
                let r = out * 0.03;
                let h = y + 1.0;
                let rail = [
                    pa + out * 0.32 - r,
                    pb + out * 0.32 - r,
                    pb + out * 0.32 + r,
                    pa + out * 0.32 + r,
                ];
                p.prism(
                    &rail,
                    h - 0.04,
                    h,
                    SCAFFOLD_METAL,
                    Some(SCAFFOLD_METAL),
                    false,
                );
            }
        }
    }
    p.finish()
}

/// Ruin of a building: broken wall stubs along the outlines (jagged heights up to a third of
/// the building, at most 6 m) and rubble heaps inside, in wall colour `wall`.
#[must_use]
pub fn ruin(
    outlines: &[Vec<Vec2>],
    y0: f32,
    height_m: f32,
    wall: Rgb,
    seed: u64,
    slot: u16,
) -> MeshData {
    let mut p = Painter::new(slot, seed);
    let mut rng = Pcg32::from_seed(seed ^ 0x2B1E);
    p.contact_y = y0;
    let max_h = (height_m / 3.0).clamp(1.0, 6.0);
    let wall = wall.scale(0.85);
    for o in outlines {
        let n = o.len();
        if n < 3 {
            continue;
        }
        let c = o.iter().copied().sum::<Vec2>() / n as f32;
        for k in 0..n {
            let (a, b) = (o[k], o[(k + 1) % n]);
            let len = a.distance(b);
            if len < 0.5 {
                continue;
            }
            let out = outward(a, b, c);
            let e = (b - a) / len;
            let pieces = (len / 2.5).ceil().max(1.0) as usize;
            for i in 0..pieces {
                // Some pieces are gone entirely.
                if rng.chance(0.25) {
                    continue;
                }
                let s0 = len * i as f32 / pieces as f32;
                let s1 = len * (i + 1) as f32 / pieces as f32;
                let (h0, h1) = (
                    max_h * (0.2 + 0.8 * rng.next_f32()),
                    max_h * (0.2 + 0.8 * rng.next_f32()),
                );
                let t = 0.3;
                let (q0, q1) = (a + e * s0, a + e * s1);
                let inner = -out * t;
                let v = |q: Vec2, y: f32| Vec3::new(q.x, y, q.y);
                let o3 = Vec3::new(out.x, 0.0, out.y);
                // Outer and inner faces with a sloped (jagged) top, top cap and end caps.
                p.face(
                    &[v(q0, y0), v(q1, y0), v(q1, y0 + h1), v(q0, y0 + h0)],
                    wall,
                    Surface::Solid,
                    0,
                    o3,
                );
                p.face(
                    &[
                        v(q0 + inner, y0),
                        v(q1 + inner, y0),
                        v(q1 + inner, y0 + h1),
                        v(q0 + inner, y0 + h0),
                    ],
                    wall.scale(0.8),
                    Surface::Solid,
                    0,
                    -o3,
                );
                p.face(
                    &[
                        v(q0, y0 + h0),
                        v(q1, y0 + h1),
                        v(q1 + inner, y0 + h1),
                        v(q0 + inner, y0 + h0),
                    ],
                    wall.scale(1.05),
                    Surface::Solid,
                    0,
                    Vec3::Y,
                );
                let e3 = Vec3::new(e.x, 0.0, e.y);
                p.face(
                    &[
                        v(q0, y0),
                        v(q0 + inner, y0),
                        v(q0 + inner, y0 + h0),
                        v(q0, y0 + h0),
                    ],
                    wall.scale(0.9),
                    Surface::Solid,
                    0,
                    -e3,
                );
                p.face(
                    &[
                        v(q1, y0),
                        v(q1 + inner, y0),
                        v(q1 + inner, y0 + h1),
                        v(q1, y0 + h1),
                    ],
                    wall.scale(0.9),
                    Surface::Solid,
                    0,
                    e3,
                );
            }
        }
        // Rubble heaps: low pyramids scattered inside the outline.
        let (lo, hi) = cb_geometry::polygon::bounds(o);
        let area = cb_geometry::polygon::area(o);
        let heaps = ((area / 40.0).ceil() as usize).clamp(2, 12);
        let mut placed = 0;
        for _ in 0..heaps * 4 {
            if placed >= heaps {
                break;
            }
            let q = Vec2::new(
                lo.x + (hi.x - lo.x) * rng.next_f32(),
                lo.y + (hi.y - lo.y) * rng.next_f32(),
            );
            if !cb_geometry::polygon::contains(o, q)
                || cb_geometry::polygon::boundary_distance(o, q) < 1.2
            {
                continue;
            }
            placed += 1;
            let r = 0.8 + 1.4 * rng.next_f32();
            let h = (0.4 + 0.5 * rng.next_f32()) * r;
            let col = if rng.chance(0.5) {
                RUBBLE
            } else {
                wall.mix(RUBBLE, 0.5)
            };
            let apex = Vec3::new(q.x, y0 + h, q.y);
            let ring: Vec<Vec3> = (0..5)
                .map(|k| {
                    let a = k as f32 * std::f32::consts::TAU / 5.0 + rng.next_f32();
                    let (s, co) = cb_core::math::sin_cos(a);
                    Vec3::new(q.x + co * r, y0 - 0.05, q.y + s * r)
                })
                .collect();
            for k in 0..5 {
                let (a, b) = (ring[k], ring[(k + 1) % 5]);
                let mid = (a + b) * 0.5 - Vec3::new(q.x, y0, q.y);
                p.face(&[a, b, apex], col, Surface::Ground, 0, mid + Vec3::Y * r);
            }
        }
    }
    p.finish()
}
