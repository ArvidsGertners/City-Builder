//! Trees and bushes (ART_DIRECTION §2: 60–180 triangles per tree): deciduous trees with a
//! hexagonal trunk and 1–3 jittered icosahedron crown blobs, conifers with stacked octagonal
//! cones. Foliage carries [`Surface::Foliage`] (season tint), trunks [`Surface::Trunk`].

use cb_core::rng::Pcg32;
use glam::{Vec2, Vec3};

use crate::paint::Painter;
use crate::palette::{self, Rgb};
use crate::surface::Surface;

/// Tree families.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeKind {
    /// Broadleaf with a round crown.
    Deciduous,
    /// Conifer with stacked cones.
    Conifer,
}

const SQRT5: f32 = 2.236_068;
/// Unit icosahedron (12 vertices, 20 faces).
fn icosahedron() -> ([Vec3; 12], [[usize; 3]; 20]) {
    let t = (1.0 + SQRT5) * 0.5;
    let s = 1.0 / (1.0 + t * t).sqrt();
    let v = |x: f32, y: f32, z: f32| Vec3::new(x * s, y * s, z * s);
    let verts = [
        v(-1.0, t, 0.0),
        v(1.0, t, 0.0),
        v(-1.0, -t, 0.0),
        v(1.0, -t, 0.0),
        v(0.0, -1.0, t),
        v(0.0, 1.0, t),
        v(0.0, -1.0, -t),
        v(0.0, 1.0, -t),
        v(t, 0.0, -1.0),
        v(t, 0.0, 1.0),
        v(-t, 0.0, -1.0),
        v(-t, 0.0, 1.0),
    ];
    let faces = [
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    (verts, faces)
}

/// Regular `n`-gon ring of radius `r` around `c` at height `y`.
fn ring(c: Vec2, r: f32, n: usize, y: f32, phase: f32) -> Vec<Vec3> {
    (0..n)
        .map(|k| {
            let (s, co) = cb_core::math::sin_cos(phase + cb_core::math::TAU * k as f32 / n as f32);
            Vec3::new(c.x + r * co, y, c.y + r * s)
        })
        .collect()
}

/// Tapered vertical prism (trunk, post) between two rings.
pub fn trunk(
    p: &mut Painter,
    base: Vec3,
    r0: f32,
    r1: f32,
    h: f32,
    sides: usize,
    c: Rgb,
    kind: Surface,
) {
    let c2 = Vec2::new(base.x, base.z);
    let lo = ring(c2, r0, sides, base.y, 0.0);
    let hi = ring(c2, r1, sides, base.y + h, 0.0);
    for k in 0..sides {
        let j = (k + 1) % sides;
        let mid = (lo[k] + lo[j]) * 0.5;
        let out = Vec3::new(mid.x - base.x, 0.0, mid.z - base.z);
        p.face(&[lo[k], lo[j], hi[j], hi[k]], c, kind, 0, out);
    }
}

/// A crown blob: jittered, squashed icosahedron.
fn blob(p: &mut Painter, c: Vec3, r: Vec3, col: Rgb, rng: &mut Pcg32) {
    let (verts, faces) = icosahedron();
    let pts: Vec<Vec3> = verts
        .iter()
        .map(|v| c + *v * r * (0.9 + 0.2 * rng.next_f32()))
        .collect();
    for f in faces {
        let tri = [pts[f[0]], pts[f[1]], pts[f[2]]];
        let ctr = (tri[0] + tri[1] + tri[2]) / 3.0;
        // Lower faces sit in the crown's own shade.
        let ao = if ctr.y < c.y - 0.3 * r.y { 0.85 } else { 1.0 };
        p.face_ao(&tri, &[ao; 3], col, Surface::Foliage, 0, ctr - c);
    }
}

/// Appends a tree of height `h` (m) standing at `base`.
pub fn tree(p: &mut Painter, base: Vec3, kind: TreeKind, h: f32, rng: &mut Pcg32) {
    let saved = p.contact_y;
    p.contact_y = base.y;
    match kind {
        TreeKind::Deciduous => {
            let col = palette::pick(rng, &palette::FOLIAGE).building_variant(rng);
            let trunk_h = h * (0.35 + 0.1 * rng.next_f32());
            trunk(
                p,
                base - Vec3::Y * 0.2,
                0.12 * h / 6.0 + 0.08,
                0.08 * h / 6.0 + 0.05,
                trunk_h + 0.4,
                6,
                palette::TRUNK,
                Surface::Trunk,
            );
            let crown_r = h * (0.28 + 0.06 * rng.next_f32());
            let cy = base.y + trunk_h + crown_r * 0.8;
            let blobs = 1 + rng.below(3) as usize;
            for k in 0..blobs {
                let off = if k == 0 {
                    Vec3::ZERO
                } else {
                    let (s, c) = cb_core::math::sin_cos(cb_core::math::TAU * rng.next_f32());
                    Vec3::new(
                        c * crown_r * 0.55,
                        crown_r * (0.1 * rng.next_f32() - 0.15),
                        s * crown_r * 0.55,
                    )
                };
                let r = crown_r * if k == 0 { 1.0 } else { 0.7 };
                blob(
                    p,
                    Vec3::new(base.x, cy, base.z) + off,
                    Vec3::new(r, r * 0.85, r),
                    col.scale(1.0 - 0.04 * k as f32),
                    rng,
                );
            }
        }
        TreeKind::Conifer => {
            let col = palette::pick(rng, &palette::CONIFER).building_variant(rng);
            trunk(
                p,
                base - Vec3::Y * 0.2,
                0.14,
                0.1,
                0.2 + h * 0.2,
                6,
                palette::TRUNK,
                Surface::Trunk,
            );
            let tiers = 3;
            let mut y = base.y + h * 0.18;
            let mut r = h * 0.24;
            let tier_h = h * 0.36;
            let c2 = Vec2::new(base.x, base.z);
            for t in 0..tiers {
                let phase = 0.4 * t as f32;
                let rim = ring(c2, r, 8, y, phase);
                let apex = Vec3::new(base.x, y + tier_h, base.z);
                for k in 0..8 {
                    let j = (k + 1) % 8;
                    let mid = (rim[k] + rim[j]) * 0.5;
                    let out = Vec3::new(mid.x - base.x, r * 0.5, mid.z - base.z);
                    p.face_ao(
                        &[rim[k], rim[j], apex],
                        &[0.88, 0.88, 1.0],
                        col,
                        Surface::Foliage,
                        0,
                        out,
                    );
                }
                p.face_ao(
                    &rim,
                    &[0.8; 8],
                    col.scale(0.85),
                    Surface::Foliage,
                    0,
                    -Vec3::Y,
                );
                y += tier_h * 0.55;
                r *= 0.72;
            }
        }
    }
    p.contact_y = saved;
}

/// Low bush (one squashed blob) at `base` with radius `r`.
pub fn bush(p: &mut Painter, base: Vec3, r: f32, rng: &mut Pcg32) {
    let col = palette::HEDGE.building_variant(rng);
    blob(
        p,
        base + Vec3::Y * (0.55 * r),
        Vec3::new(r, 0.7 * r, r),
        col,
        rng,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_budgets() {
        for kind in [TreeKind::Deciduous, TreeKind::Conifer] {
            for seed in 0..50 {
                let mut p = Painter::new(0, seed);
                let mut rng = Pcg32::from_seed(seed);
                tree(&mut p, Vec3::ZERO, kind, 8.0, &mut rng);
                let m = p.finish();
                let t = m.triangle_count();
                assert!((26..=180).contains(&t), "{kind:?} {t}");
                let (lo, hi) = m.bounds();
                assert!(
                    lo.y >= -0.21 && hi.y <= 8.0 * 1.15,
                    "{kind:?} height {}",
                    hi.y
                );
            }
        }
    }
}
