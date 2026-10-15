//! Terrain chunk meshes (128 m chunks, LOD0–3) with skirts (ADR-0065, ALGORITHMS §24).
//!
//! LOD k samples every 2^k-th height sample. LOD1+ uses **constrained decimation**: plain
//! decimation, then the smallest lowering of coarse vertices (least squares over barycentric
//! weights, Gauss–Seidel, ≤ 12 passes) so that at every stamp-core sample in the chunk the
//! coarse surface is ≤ core height − 5 mm — the coarse terrain never pokes through a road.
//! Cells are split along the same `(i, j)`–`(i + 1, j + 1)` diagonal as the height queries.

use glam::Vec3;

use crate::materials::TerrainMaterial;
use crate::terrain::Terrain;

/// Height cells per chunk side at LOD0 (128 m at 2 m spacing).
pub const CHUNK_CELLS: usize = 64;
/// Number of LOD levels.
pub const LODS: u32 = 4;
/// Skirt depth per LOD (m).
pub const SKIRT_M: [f32; 4] = [3.0, 4.0, 6.0, 8.0];
/// Margin kept below core samples by LOD1+ (m).
pub const CORE_MARGIN_M: f32 = 0.005;

/// Mesh data of one chunk-LOD (world-space positions).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChunkMesh {
    /// Positions.
    pub positions: Vec<[f32; 3]>,
    /// Smooth normals.
    pub normals: Vec<[f32; 3]>,
    /// Material weights `[dirt, rock, sand, forest]` (grass = 255 − sum).
    pub materials: Vec<[u8; 4]>,
    /// Triangle list.
    pub indices: Vec<u32>,
}

/// Chunks per map side.
#[must_use]
pub fn chunks_per_side(t: &Terrain) -> usize {
    (t.height().n() - 1).div_ceil(CHUNK_CELLS)
}

/// Vertex heights of chunk `(cx, cz)` at `lod` (row-major, `(CHUNK_CELLS >> lod) + 1` per side).
#[must_use]
pub fn lod_heights(t: &Terrain, cx: usize, cz: usize, lod: u32) -> (Vec<f32>, usize) {
    let h = t.height();
    let n = h.n();
    let step = 1usize << lod;
    let vn = (CHUNK_CELLS >> lod) + 1;
    let (i0, j0) = (cx * CHUNK_CELLS, cz * CHUNK_CELLS);
    let at = |vi: usize, vj: usize| h.get((i0 + vi * step).min(n - 1), (j0 + vj * step).min(n - 1));
    let mut g: Vec<f32> = (0..vn * vn).map(|v| at(v % vn, v / vn)).collect();
    if lod == 0 {
        return (g, vn);
    }
    // Core samples inside the chunk (inclusive borders).
    let mut cores = Vec::new();
    for j in j0..=(j0 + CHUNK_CELLS).min(n - 1) {
        for i in i0..=(i0 + CHUNK_CELLS).min(n - 1) {
            if t.is_core(i, j) {
                cores.push((i - i0, j - j0, h.get(i, j)));
            }
        }
    }
    for _ in 0..12 {
        let mut worst = 0.0f32;
        for &(li, lj, hc) in &cores {
            let (ci, cj) = ((li / step).min(vn - 2), (lj / step).min(vn - 2));
            let u = (li - ci * step) as f32 / step as f32;
            let v = (lj - cj * step) as f32 / step as f32;
            let v00 = cj * vn + ci;
            let (v10, v01, v11) = (v00 + 1, v00 + vn, v00 + vn + 1);
            let (verts, bw) = if u >= v {
                ([v00, v10, v11], [1.0 - u, u - v, v])
            } else {
                ([v00, v11, v01], [1.0 - v, u, v - u])
            };
            let interp: f32 = (0..3).map(|q| bw[q] * g[verts[q]]).sum();
            let c = hc - CORE_MARGIN_M;
            if interp > c {
                let s2: f32 = bw.iter().map(|b| b * b).sum();
                let d = (interp - c) / s2.max(1e-6);
                for q in 0..3 {
                    g[verts[q]] -= d * bw[q];
                }
                worst = worst.max(interp - c);
            }
        }
        if worst < 1e-4 {
            break;
        }
    }
    (g, vn)
}

/// Builds the mesh of chunk `(cx, cz)` at `lod`, with skirts.
#[must_use]
pub fn build(t: &Terrain, cx: usize, cz: usize, lod: u32) -> ChunkMesh {
    let h = t.height();
    let n = h.n();
    let step = 1usize << lod;
    let (g, vn) = lod_heights(t, cx, cz, lod);
    let (i0, j0) = (cx * CHUNK_CELLS, cz * CHUNK_CELLS);
    let mut m = ChunkMesh::default();
    let mats = t.materials();
    for vj in 0..vn {
        for vi in 0..vn {
            let (i, j) = ((i0 + vi * step).min(n - 1), (j0 + vj * step).min(n - 1));
            let p = h.pos(i, j);
            m.positions.push([p.x, g[vj * vn + vi], p.y]);
            // Smooth normal from the fine grid with a LOD-sized stencil (continuous across chunks).
            let s = step as isize;
            let (ii, jj) = (i as isize, j as isize);
            let dx = h.get_clamped(ii + s, jj) - h.get_clamped(ii - s, jj);
            let dz = h.get_clamped(ii, jj + s) - h.get_clamped(ii, jj - s);
            let nrm = Vec3::new(-dx, 2.0 * s as f32 * h.spacing_m(), -dz).normalize_or(Vec3::Y);
            m.normals.push(nrm.to_array());
            let w = mats.get(i, j);
            m.materials.push([
                w[TerrainMaterial::Dirt as usize],
                w[TerrainMaterial::Rock as usize],
                w[TerrainMaterial::Sand as usize],
                w[TerrainMaterial::Forest as usize],
            ]);
        }
    }
    let idx = |vi: usize, vj: usize| (vj * vn + vi) as u32;
    for vj in 0..vn - 1 {
        for vi in 0..vn - 1 {
            let (a, b, c, d) = (
                idx(vi, vj),
                idx(vi + 1, vj),
                idx(vi, vj + 1),
                idx(vi + 1, vj + 1),
            );
            // Diagonal (00)–(11); counter-clockwise seen from above (+Y).
            m.indices.extend_from_slice(&[a, d, b, a, c, d]);
        }
    }
    // Skirts: border ring duplicated SKIRT_M below, walls facing outward.
    let depth = SKIRT_M[(lod as usize).min(3)];
    let mut ring: Vec<u32> = Vec::new();
    ring.extend((0..vn).map(|vi| idx(vi, 0)));
    ring.extend((1..vn).map(|vj| idx(vn - 1, vj)));
    ring.extend((0..vn - 1).rev().map(|vi| idx(vi, vn - 1)));
    ring.extend((1..vn - 1).rev().map(|vj| idx(0, vj)));
    let base = m.positions.len() as u32;
    for &v in &ring {
        let p = m.positions[v as usize];
        m.positions.push([p[0], p[1] - depth, p[2]]);
        m.normals.push(m.normals[v as usize]);
        m.materials.push(m.materials[v as usize]);
    }
    let rn = ring.len() as u32;
    for k in 0..rn {
        let (a, b) = (ring[k as usize], ring[((k + 1) % rn) as usize]);
        let (a2, b2) = (base + k, base + (k + 1) % rn);
        m.indices.extend_from_slice(&[a, b, b2, a, b2, a2]);
    }
    m
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::generate::{World, WorldReport};
    use crate::heightfield::{Grid, HeightGrid};
    use crate::stamps::{Stamp, StampShape, priority};
    use crate::water::WaterField;
    use glam::Vec2;

    pub(crate) fn test_terrain(n: usize) -> Terrain {
        let mut base = HeightGrid::filled(n, 2.0, 0.0);
        for j in 0..n {
            for i in 0..n {
                let p = base.pos(i, j);
                base.set(
                    i,
                    j,
                    20.0 + 6.0 * cb_core::math::sin(p.x * 0.05) * cb_core::math::cos(p.y * 0.04),
                );
            }
        }
        let water = WaterField::dry_like(&base);
        let forest = Grid::filled((n - 1) / 4 + 1, 8.0, 200u8);
        let (wd, wl) = crate::materials::water_distance(&water, 64.0);
        let materials = crate::materials::compute(&base, &water, &wd, &wl, &forest, 1);
        let world = World {
            base,
            water,
            rivers: Vec::new(),
            lakes: Vec::new(),
            outside: Vec::new(),
            forest,
            materials,
            material_seed: 1,
            report: WorldReport {
                buildable_km2: 0.0,
                shore_distance_m: 0.0,
                rerolls: (0, 0, 0),
                relaxed: Vec::new(),
            },
        };
        Terrain::from_world(world)
    }

    fn road_terrain() -> Terrain {
        let mut t = test_terrain(257);
        let centerline = (0..=24)
            .map(|k| {
                let x = -120.0 + k as f32 * 10.0;
                Vec3::new(x, 24.0 + x * 0.02, 5.0 + x * 0.1)
            })
            .collect();
        t.add_stamp(Stamp {
            id: 1,
            priority: priority::ROAD,
            shape: StampShape::Corridor {
                centerline,
                core_half_width_m: 7.5,
                carve_m: 0.15,
            },
        });
        t
    }

    #[test]
    fn lod0_matches_heights_and_counts() {
        let t = road_terrain();
        let m = build(&t, 1, 1, 0);
        let vn = CHUNK_CELLS + 1;
        assert_eq!(m.positions.len(), vn * vn + 4 * (vn - 1));
        assert_eq!(m.indices.len() % 3, 0);
        let p = m.positions[vn * 3 + 5];
        assert_eq!(p[1], t.height_at(Vec2::new(p[0], p[2])));
        assert!(m.normals.iter().all(|n| n[1] > 0.0));
    }

    #[test]
    fn coarse_lods_never_above_road_cores() {
        let t = road_terrain();
        let h = t.height();
        for lod in 1..LODS {
            let step = 1usize << lod;
            for (cx, cz) in [(0, 0), (0, 1), (1, 0), (1, 1), (2, 1), (2, 2)] {
                let (g, vn) = lod_heights(&t, cx, cz, lod);
                for j in 0..=CHUNK_CELLS {
                    for i in 0..=CHUNK_CELLS {
                        let (gi, gj) = (cx * CHUNK_CELLS + i, cz * CHUNK_CELLS + j);
                        if gi >= h.n() || gj >= h.n() || !t.is_core(gi, gj) {
                            continue;
                        }
                        let (ci, cj) = ((i / step).min(vn - 2), (j / step).min(vn - 2));
                        let u = (i - ci * step) as f32 / step as f32;
                        let v = (j - cj * step) as f32 / step as f32;
                        let v00 = cj * vn + ci;
                        let hc = if u >= v {
                            g[v00] * (1.0 - u) + g[v00 + 1] * (u - v) + g[v00 + vn + 1] * v
                        } else {
                            g[v00] * (1.0 - v) + g[v00 + vn + 1] * u + g[v00 + vn] * (v - u)
                        };
                        assert!(
                            hc <= h.get(gi, gj) - CORE_MARGIN_M + 2e-3,
                            "lod {lod} chunk ({cx},{cz}) sample ({gi},{gj}): {hc} > {}",
                            h.get(gi, gj)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn skirts_hang_below_the_border() {
        let t = road_terrain();
        for lod in 0..LODS {
            let m = build(&t, 1, 1, lod);
            let vn = (CHUNK_CELLS >> lod) + 1;
            let skirt = &m.positions[vn * vn..];
            assert_eq!(skirt.len(), 4 * (vn - 1));
            assert!(
                skirt.iter().all(
                    |p| p[1] < t.height_at(Vec2::new(p[0], p[2])) - SKIRT_M[lod as usize] + 0.5
                )
            );
        }
    }
}
