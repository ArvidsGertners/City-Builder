//! Deterministic natural scatter per 32 m cell (ALGORITHMS §8, presentation only): forest
//! trees on a jittered 5 m lattice, single trees in meadows, rocks on rocky slopes and grass
//! clumps near water. Filtered by water, slope and the stamp exclusion mask, so roads, plots
//! and footprints stay clear. The simulation never reads instances (it uses
//! [`Terrain::tree_density`](crate::terrain::Terrain::tree_density)).

use cb_core::hash::{Domain, derive_seed};
use cb_core::math::TAU;
use cb_core::rng::Pcg32;
use glam::{Vec2, Vec3};

use crate::materials::TerrainMaterial;
use crate::terrain::Terrain;

/// Scatter cell size (m).
pub const CELL_M: f32 = 32.0;
/// Candidate lattice spacing inside a cell (m).
const LATTICE_M: f32 = 4.0;

/// Kinds of scattered props.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum PropKind {
    Conifer,
    Deciduous,
    Birch,
    Bush,
    Rock,
    GrassClump,
}

/// One scattered instance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PropInstance {
    /// Base position on the terrain.
    pub pos: Vec3,
    /// Kind.
    pub kind: PropKind,
    /// Mesh variant 0–3.
    pub variant: u8,
    /// Uniform scale.
    pub scale: f32,
    /// Rotation about Y (radians).
    pub yaw: f32,
    /// Per-instance seed (colour jitter, sway phase).
    pub seed: u32,
}

/// Cell index of a world position.
#[must_use]
pub fn cell_of(p: Vec2) -> (i32, i32) {
    ((p.x / CELL_M).floor() as i32, (p.y / CELL_M).floor() as i32)
}

/// Generates the instances of scatter cell `(cx, cz)`. Pure in `(terrain, world_seed, cell)`.
#[must_use]
pub fn scatter_cell(t: &Terrain, world_seed: u64, cell: (i32, i32)) -> Vec<PropInstance> {
    let cid = (u64::from(cell.0 as u32) << 32) | u64::from(cell.1 as u32);
    let mut rng = Pcg32::from_seed(derive_seed(world_seed, Domain::Scatter, cid, 0));
    let origin = Vec2::new(cell.0 as f32 * CELL_M, cell.1 as f32 * CELL_M);
    let h = t.height();
    let half = h.half_extent_m();
    let forest = t.forest();
    let per = (CELL_M / LATTICE_M) as usize;
    let mut out = Vec::new();
    for gy in 0..per {
        for gx in 0..per {
            // Draw every random value up front so filtering never shifts the stream.
            let jitter = Vec2::new(rng.next_f32(), rng.next_f32()) * LATTICE_M;
            let (roll, yaw, scale, variant, kind_roll, seed) = (
                rng.next_f32(),
                rng.next_f32() * TAU,
                rng.range_f32(0.8, 1.25),
                rng.below(4) as u8,
                rng.next_f32(),
                rng.next_u32(),
            );
            let p = origin + Vec2::new(gx as f32, gy as f32) * LATTICE_M + jitter;
            if p.x.abs() >= half || p.y.abs() >= half || t.water().is_water(p) || t.is_excluded(p) {
                continue;
            }
            let (i, j) = h.nearest(p);
            let slope = h.slope_at(p);
            let height = h.height_at(p);
            let m = t.materials().get(i, j);
            let rock = f32::from(m[TerrainMaterial::Rock as usize]) / 255.0;
            let grass = f32::from(m[TerrainMaterial::Grass as usize]) / 255.0;
            let fg = forest.to_grid(p);
            let fm = (forest.n() - 1) as f32;
            let f = f32::from(forest.get(
                fg.x.round().clamp(0.0, fm) as usize,
                fg.y.round().clamp(0.0, fm) as usize,
            )) / 255.0;
            // Forest trees: lattice 4 m with ~60 % fill at full density (≈ 5 m spacing).
            let kind = if slope < 0.6 && roll < f * 0.6 {
                Some(if height > 95.0 || kind_roll < 0.35 {
                    PropKind::Conifer
                } else if kind_roll < 0.8 {
                    PropKind::Deciduous
                } else if kind_roll < 0.92 {
                    PropKind::Birch
                } else {
                    PropKind::Bush
                })
            } else if slope < 0.4 && f < 0.2 && roll > 0.985 {
                // Singles in meadows: ≈ 1 per 30 × 30 m.
                Some(if kind_roll < 0.6 {
                    PropKind::Deciduous
                } else if kind_roll < 0.8 {
                    PropKind::Birch
                } else {
                    PropKind::Bush
                })
            } else if rock > 0.5 && roll < 0.08 {
                Some(PropKind::Rock)
            } else if grass > 0.5
                && ((t.water_distance_at(p) < 20.0 && roll < 0.25)
                    || (grass > 0.85 && (0.9..0.91).contains(&roll)))
            {
                // Grass clumps along shores, sparse in open meadows.
                Some(PropKind::GrassClump)
            } else {
                None
            };
            if let Some(kind) = kind {
                out.push(PropInstance {
                    pos: Vec3::new(p.x, height, p.y),
                    kind,
                    variant,
                    scale,
                    yaw,
                    seed,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk_mesh::tests::test_terrain;
    use crate::stamps::{Stamp, StampShape, priority};

    fn cells() -> Vec<(i32, i32)> {
        (-4..4).flat_map(|x| (-4..4).map(move |z| (x, z))).collect()
    }

    fn road() -> Stamp {
        let centerline = (0..=20)
            .map(|k| Vec3::new(-100.0 + k as f32 * 10.0, 26.0, 3.0))
            .collect();
        Stamp {
            id: 7,
            priority: priority::ROAD,
            shape: StampShape::Corridor {
                centerline,
                core_half_width_m: 7.0,
                carve_m: 0.15,
                soft_ends: [false; 2],
            },
        }
    }

    #[test]
    fn scatter_deterministic_and_dense_in_forest() {
        let t = test_terrain(257);
        let a: Vec<_> = cells()
            .iter()
            .flat_map(|c| scatter_cell(&t, 9, *c))
            .collect();
        let b: Vec<_> = cells()
            .iter()
            .flat_map(|c| scatter_cell(&t, 9, *c))
            .collect();
        assert_eq!(a, b);
        let trees = a
            .iter()
            .filter(|p| {
                matches!(
                    p.kind,
                    PropKind::Conifer | PropKind::Deciduous | PropKind::Birch
                )
            })
            .count();
        // 256 × 256 m of dense forest: about one tree per 25–35 m².
        assert!((1_500..4_000).contains(&trees), "{trees}");
    }

    #[test]
    fn scatter_excluded_by_corridor_plus_2m() {
        let mut t = test_terrain(257);
        t.add_stamp(road());
        let all: Vec<_> = cells()
            .iter()
            .flat_map(|c| scatter_cell(&t, 9, *c))
            .collect();
        assert!(!all.is_empty());
        for p in &all {
            if p.pos.x.abs() < 95.0 {
                assert!((p.pos.z - 3.0).abs() > 7.0 + 2.0, "instance at {:?}", p.pos);
            }
        }
    }

    #[test]
    fn scatter_restored_after_stamp_removed() {
        let mut t = test_terrain(257);
        let before: Vec<_> = cells()
            .iter()
            .flat_map(|c| scatter_cell(&t, 9, *c))
            .collect();
        t.add_stamp(road());
        let during: Vec<_> = cells()
            .iter()
            .flat_map(|c| scatter_cell(&t, 9, *c))
            .collect();
        assert_ne!(before, during);
        t.remove_stamp(7);
        let after: Vec<_> = cells()
            .iter()
            .flat_map(|c| scatter_cell(&t, 9, *c))
            .collect();
        assert_eq!(before, after);
    }

    #[test]
    fn tree_density_field_matches_exclusion() {
        let mut t = test_terrain(257);
        let before = t.tree_density();
        assert!(before.data().iter().all(|v| *v == 200));
        t.add_stamp(road());
        let d = t.tree_density();
        // On the centreline: fully excluded; 60 m away: untouched.
        let on = d.nearest(Vec2::new(0.0, 3.0));
        let off = d.nearest(Vec2::new(0.0, 63.0));
        assert_eq!(d.get(on.0, on.1), 0);
        assert_eq!(d.get(off.0, off.1), 200);
        t.remove_stamp(7);
        assert_eq!(t.tree_density(), before);
    }
}
