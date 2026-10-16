//! Street decorations (GAME_DESIGN §3, ART_DIRECTION): street trees and lamps placed by
//! `cb_network::style::decor_items`. Lamps stand plumb on slopes with their arm toward the road.

use cb_core::rng::Pcg32;
use cb_geometry::MeshData;
use cb_network::style::{DecorItem, DecorKind};
use glam::{Vec2, Vec3};

use crate::paint::Painter;
use crate::palette;
use crate::vegetation::{self, TreeKind};

/// Lamp pole height (m), arm reach (m) and pole half thickness (m).
pub const LAMP_H_M: f32 = 6.0;
const ARM_M: f32 = 1.4;
const POLE_HALF_M: f32 = 0.07;

/// Mesh of a segment's decorations; `seed` varies tree shapes per segment.
#[must_use]
pub fn decor_mesh(items: &[DecorItem], seed: u64, slot: u16) -> MeshData {
    let mut p = Painter::new(slot, seed ^ 0xDEC0);
    for (k, it) in items.iter().enumerate() {
        match it.kind {
            DecorKind::Tree => {
                let mut rng = Pcg32::new(seed ^ 0x7EE5, k as u64);
                let h = 6.5 + 2.0 * rng.next_f32();
                vegetation::tree(&mut p, it.pos, TreeKind::Deciduous, h, &mut rng);
            }
            DecorKind::Lamp => lamp(&mut p, it.pos, it.toward_road),
        }
    }
    p.finish()
}

fn square(c: Vec2, a: Vec2, half_a: f32, half_b: f32) -> Vec<Vec2> {
    let b = a.perp();
    vec![
        c - a * half_a - b * half_b,
        c + a * half_a - b * half_b,
        c + a * half_a + b * half_b,
        c - a * half_a + b * half_b,
    ]
}

fn lamp(p: &mut Painter, foot: Vec3, toward: Vec2) {
    let c = Vec2::new(foot.x, foot.z);
    let t = toward.try_normalize().unwrap_or(Vec2::X);
    let top = foot.y + LAMP_H_M;
    p.prism(
        &square(c, t, POLE_HALF_M, POLE_HALF_M),
        foot.y,
        top,
        palette::LAMP_METAL,
        None,
        false,
    );
    // Arm toward the road and the lamp head at its end.
    let mid = c + t * (0.5 * ARM_M);
    p.prism(
        &square(mid, t, 0.5 * ARM_M, 0.04),
        top - 0.08,
        top,
        palette::LAMP_METAL,
        None,
        false,
    );
    let head = c + t * ARM_M;
    p.prism(
        &square(head, t, 0.28, 0.14),
        top - 0.16,
        top + 0.02,
        palette::LAMP_METAL,
        Some(palette::LAMP_METAL),
        false,
    );
    p.prism(
        &square(head, t, 0.22, 0.1),
        top - 0.22,
        top - 0.16,
        palette::LAMP_GLASS,
        None,
        false,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lamps_plumb_and_trees_present() {
        let items = [
            DecorItem {
                kind: DecorKind::Lamp,
                pos: Vec3::new(0.0, 5.0, 0.0),
                s_m: 0.0,
                toward_road: Vec2::X,
            },
            DecorItem {
                kind: DecorKind::Tree,
                pos: Vec3::new(10.0, 6.0, 0.0),
                s_m: 10.0,
                toward_road: Vec2::X,
            },
        ];
        let m = decor_mesh(&items[..1], 1, 0);
        let (lo, hi) = m.bounds();
        // Plumb: the pole's footprint does not drift with height; the arm reaches the road side.
        assert!((lo.y - 5.0).abs() < 1e-3 && (hi.y - (5.0 + LAMP_H_M + 0.02)).abs() < 1e-3);
        assert!(lo.x > -0.2 && hi.x < ARM_M + 0.3);
        let both = decor_mesh(&items, 1, 0);
        assert!(both.triangle_count() > m.triangle_count() + 10);
    }
}
