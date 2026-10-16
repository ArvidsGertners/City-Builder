//! Free decorations (GAME_DESIGN §3, decoration tool): single trees, tree clusters, benches,
//! planters, fountains and flower beds, generated around their ground point.

use cb_core::rng::Pcg32;
use cb_geometry::MeshData;
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::paint::{Frame, Painter};
use crate::palette::{self, hex};
use crate::surface::Surface;
use crate::vegetation::{self, TreeKind};

/// Decoration kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DecorKind {
    /// A single tree.
    Tree,
    /// A group of trees.
    TreeCluster,
    /// A park bench.
    Bench,
    /// A planter box.
    Planter,
    /// A fountain.
    Fountain,
    /// A flower bed.
    FlowerBed,
}

impl DecorKind {
    /// Every kind, in toolbar order.
    pub const ALL: [Self; 6] = [
        Self::Tree,
        Self::TreeCluster,
        Self::Bench,
        Self::Planter,
        Self::Fountain,
        Self::FlowerBed,
    ];
}

/// Mesh of a decoration standing at `base` (on the ground), turned by `yaw` (radians).
#[must_use]
pub fn mesh(kind: DecorKind, base: Vec3, yaw: f32, seed: u64, slot: u16) -> MeshData {
    let mut p = Painter::new(slot, seed);
    let mut rng = Pcg32::from_seed(seed);
    p.contact_y = base.y;
    let (s, c) = cb_core::math::sin_cos(yaw);
    let ex = Vec2::new(c, s);
    let fr = Frame {
        o: Vec2::new(base.x, base.z),
        ex,
        ey: ex.perp(),
    };
    let y = base.y;
    match kind {
        DecorKind::Tree => {
            let h = 6.0 + 3.0 * rng.next_f32();
            vegetation::tree(&mut p, base, TreeKind::Deciduous, h, &mut rng);
        }
        DecorKind::TreeCluster => {
            let n = 4 + rng.below(3) as usize;
            for k in 0..n {
                let a = k as f32 * std::f32::consts::TAU / n as f32 + rng.next_f32();
                let r = 1.5 + 2.0 * rng.next_f32();
                let (sa, ca) = cb_core::math::sin_cos(a);
                let q = base + Vec3::new(ca * r, 0.0, sa * r);
                let kind = if rng.chance(0.3) {
                    TreeKind::Conifer
                } else {
                    TreeKind::Deciduous
                };
                let h = 5.0 + 4.0 * rng.next_f32();
                vegetation::tree(&mut p, q, kind, h, &mut rng);
            }
        }
        DecorKind::Bench => {
            let wood = palette::WOOD[1];
            let metal = palette::LAMP_METAL;
            for u in [-0.75, 0.75] {
                p.frame_box(
                    &fr,
                    [u - 0.05, -0.25, u + 0.05, 0.25],
                    y,
                    y + 0.45,
                    metal,
                    Some(metal),
                );
            }
            p.frame_box(
                &fr,
                [-0.9, -0.25, 0.9, 0.2],
                y + 0.42,
                y + 0.48,
                wood,
                Some(wood),
            );
            p.frame_box(
                &fr,
                [-0.9, 0.2, 0.9, 0.26],
                y + 0.5,
                y + 0.9,
                wood,
                Some(wood),
            );
        }
        DecorKind::Planter => {
            let stone = palette::STONE;
            p.frame_box(&fr, [-0.6, -0.6, 0.6, 0.6], y - 0.1, y + 0.55, stone, None);
            let soil = [
                fr.p3(-0.5, -0.5, y + 0.5),
                fr.p3(0.5, -0.5, y + 0.5),
                fr.p3(0.5, 0.5, y + 0.5),
                fr.p3(-0.5, 0.5, y + 0.5),
            ];
            p.face(&soil, palette::DIRT[1], Surface::Ground, 0, Vec3::Y);
            vegetation::bush(&mut p, base + Vec3::Y * 0.5, 0.45, &mut rng);
        }
        DecorKind::Fountain => {
            let stone = hex(0xC9C2B4);
            let water = hex(0x7CC4C0);
            // Octagonal basin, water surface, a two-tier centre column.
            let ring = |r: f32| -> Vec<Vec2> {
                (0..8)
                    .map(|k| {
                        let a = (k as f32 + 0.5) * std::f32::consts::TAU / 8.0;
                        let (sa, ca) = cb_core::math::sin_cos(a);
                        fr.o + Vec2::new(ca, sa) * r
                    })
                    .collect()
            };
            let outer = ring(3.0);
            p.prism(
                &outer,
                y - 0.1,
                y + 0.5,
                stone,
                Some(stone.scale(1.05)),
                false,
            );
            let pool: Vec<Vec3> = ring(2.7)
                .iter()
                .map(|q| Vec3::new(q.x, y + 0.52, q.y))
                .collect();
            p.face(&pool, water, Surface::Paved, 0, Vec3::Y);
            p.prism(&ring(0.5), y + 0.5, y + 1.6, stone, Some(stone), false);
            p.prism(&ring(1.1), y + 1.6, y + 1.8, stone, Some(water), false);
            p.prism(&ring(0.25), y + 1.8, y + 2.4, stone, Some(stone), false);
        }
        DecorKind::FlowerBed => {
            let edge = palette::CURB;
            p.frame_box(
                &fr,
                [-1.6, -0.9, 1.6, 0.9],
                y - 0.05,
                y + 0.18,
                edge,
                Some(palette::DIRT[0]),
            );
            for k in 0..10 {
                let u = -1.3 + 2.6 * (k % 5) as f32 / 4.0;
                let v = if k < 5 { -0.4 } else { 0.4 };
                let col = palette::pick(&mut rng, &palette::FLOWERS);
                p.frame_box(
                    &fr,
                    [u - 0.18, v - 0.18, u + 0.18, v + 0.18],
                    y + 0.18,
                    y + 0.42,
                    palette::HEDGE,
                    Some(col),
                );
            }
        }
    }
    p.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_decor_kind_has_a_small_mesh() {
        for k in DecorKind::ALL {
            let m = mesh(k, Vec3::new(10.0, 2.0, -4.0), 0.7, 3, 0);
            let n = m.triangle_count();
            assert!((12..=1200).contains(&n), "{k:?}: {n}");
            let (lo, hi) = m.bounds();
            assert!(lo.y >= 1.8 && hi.y < 14.0, "{k:?}");
            assert!((hi.x - lo.x) < 12.0 && (hi.z - lo.z) < 12.0, "{k:?}");
        }
    }
}
