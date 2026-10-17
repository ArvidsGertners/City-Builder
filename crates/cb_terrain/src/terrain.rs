//! The live terrain (DATA_MODEL §2): saved base heights + stamps → composited heights, core
//! and scatter-exclusion masks, materials, water, forest mask and the raycast pyramid. Every
//! edit recomputes only the dirty rectangle.

use std::collections::BTreeMap;

use glam::{Vec2, Vec3};

use crate::generate::World;
use crate::heightfield::{Grid, HeightGrid};
use crate::materials::{self, MaterialInputs, MaterialWeights};
use crate::raycast::{self, MinMax, RayHit};
use crate::stamps::{self, CompositeOut, Raster, SampleRect, Stamp};
use crate::water::WaterField;

/// Scatter exclusion dilation around stamp cores, in samples (covers the 2 m margin for any
/// point in a cell whose corner is excluded: 2 m + 2 cell diagonals ≤ 3 samples).
pub const EXCLUSION_DILATE: usize = 3;

/// Terrain state.
#[derive(Clone, Debug)]
pub struct Terrain {
    base: HeightGrid,
    height: HeightGrid,
    core: Vec<bool>,
    disturbed: Vec<u8>,
    /// Number of `Clear` stamps covering each sample.
    cleared: Vec<u16>,
    excluded: Vec<bool>,
    stamps: BTreeMap<u32, (Stamp, Raster)>,
    water: WaterField,
    water_dist: Grid<f32>,
    water_near: Grid<f32>,
    forest: Grid<u8>,
    materials: MaterialWeights,
    material_seed: u64,
    minmax: MinMax,
}

impl Terrain {
    /// Wraps a generated world (no stamps).
    #[must_use]
    pub fn from_world(world: World) -> Self {
        let n = world.base.n();
        let (water_dist, water_near) = materials::water_distance(&world.water, 64.0);
        let minmax = MinMax::build(&world.base, &world.water);
        Self {
            height: world.base.clone(),
            base: world.base,
            core: vec![false; n * n],
            disturbed: vec![0; n * n],
            cleared: vec![0; n * n],
            excluded: vec![false; n * n],
            stamps: BTreeMap::new(),
            water: world.water,
            water_dist,
            water_near,
            forest: world.forest,
            materials: world.materials,
            material_seed: world.material_seed,
            minmax,
        }
    }

    /// Saved base heights.
    #[must_use]
    pub fn base(&self) -> &HeightGrid {
        &self.base
    }

    /// Final (composited) heights.
    #[must_use]
    pub fn height(&self) -> &HeightGrid {
        &self.height
    }

    /// Water surfaces.
    #[must_use]
    pub fn water(&self) -> &WaterField {
        &self.water
    }

    /// Material weights.
    #[must_use]
    pub fn materials(&self) -> &MaterialWeights {
        &self.materials
    }

    /// Forest mask (8 m).
    #[must_use]
    pub fn forest(&self) -> &Grid<u8> {
        &self.forest
    }

    /// Seed of the material moisture noise (saved with the base terrain).
    #[must_use]
    pub fn material_seed(&self) -> u64 {
        self.material_seed
    }

    /// Whether sample `(i, j)` is a stamp core.
    #[must_use]
    pub fn is_core(&self, i: usize, j: usize) -> bool {
        self.core[self.height.index(i, j)]
    }

    /// Height at a world position on the render triangulation.
    #[must_use]
    pub fn height_at(&self, p: Vec2) -> f32 {
        self.height.height_at(p)
    }

    /// Water depth at a world position (0 when dry).
    #[must_use]
    pub fn water_depth_at(&self, p: Vec2) -> f32 {
        self.water.depth_at(p, &self.height)
    }

    /// Distance to the nearest water (m, capped at 64).
    #[must_use]
    pub fn water_distance_at(&self, p: Vec2) -> f32 {
        let (i, j) = self.water_dist.nearest(p);
        self.water_dist.get(i, j)
    }

    /// Whether scatter is excluded at `p` (any corner of its cell excluded).
    #[must_use]
    pub fn is_excluded(&self, p: Vec2) -> bool {
        let g = self.height.to_grid(p);
        let m = (self.height.n() - 1) as f32;
        let (i, j) = (
            g.x.floor().clamp(0.0, m - 1.0) as usize,
            g.y.floor().clamp(0.0, m - 1.0) as usize,
        );
        let n = self.height.n();
        [(i, j), (i + 1, j), (i, j + 1), (i + 1, j + 1)]
            .iter()
            .any(|(a, b)| self.excluded[b * n + a])
    }

    /// Ray cast against terrain and water.
    #[must_use]
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max_t: f32) -> Option<RayHit> {
        raycast::raycast(&self.height, &self.water, &self.minmax, origin, dir, max_t)
    }

    /// Stamps by id.
    pub fn stamps(&self) -> impl Iterator<Item = &Stamp> {
        self.stamps.values().map(|(s, _)| s)
    }

    /// Earthwork volume of a stamp (m³).
    #[must_use]
    pub fn earthwork_m3(&self, id: u32) -> Option<f32> {
        self.stamps.get(&id).map(|(_, r)| r.earthwork_m3)
    }

    /// Adds (or replaces) a stamp; returns the dirty sample rectangle.
    pub fn add_stamp(&mut self, stamp: Stamp) -> SampleRect {
        let mut dirty = self.remove_raw(stamp.id);
        let raster = stamps::rasterize(&stamp, &self.base);
        for c in &raster.clear {
            self.cleared[*c as usize] += 1;
        }
        dirty = Some(dirty.map_or(raster.rect, |d| d.union(raster.rect)));
        self.stamps.insert(stamp.id, (stamp, raster));
        let rect = dirty.unwrap_or(SampleRect {
            i0: 0,
            j0: 0,
            i1: 0,
            j1: 0,
        });
        self.recomposite(rect)
    }

    /// Removes a stamp; returns the dirty rectangle (`None` if unknown).
    pub fn remove_stamp(&mut self, id: u32) -> Option<SampleRect> {
        let rect = self.remove_raw(id)?;
        Some(self.recomposite(rect))
    }

    fn remove_raw(&mut self, id: u32) -> Option<SampleRect> {
        let (_, raster) = self.stamps.remove(&id)?;
        for c in &raster.clear {
            self.cleared[*c as usize] = self.cleared[*c as usize].saturating_sub(1);
        }
        Some(raster.rect)
    }

    /// Recomposites `rect` and refreshes everything derived from it; returns the rectangle
    /// actually touched (grown for slopes, exclusion and the pyramid).
    fn recomposite(&mut self, rect: SampleRect) -> SampleRect {
        let n = self.height.n();
        let rect = rect.grow(1, n);
        {
            let mut out = CompositeOut {
                height: self.height.data_mut(),
                core: &mut self.core,
                disturbed: &mut self.disturbed,
            };
            stamps::composite(
                &self.base,
                self.stamps.values().map(|(s, r)| (s, r)),
                rect,
                &mut out,
            );
        }
        let wide = rect.grow(EXCLUSION_DILATE, n);
        // Exclusion: cores dilated by EXCLUSION_DILATE samples, plus Clear stamps.
        for j in wide.j0..=wide.j1 {
            for i in wide.i0..=wide.i1 {
                let d = EXCLUSION_DILATE;
                let mut ex = self.cleared[j * n + i] > 0;
                if !ex {
                    'scan: for b in j.saturating_sub(d)..=(j + d).min(n - 1) {
                        for a in i.saturating_sub(d)..=(i + d).min(n - 1) {
                            if self.core[b * n + a] {
                                ex = true;
                                break 'scan;
                            }
                        }
                    }
                }
                self.excluded[j * n + i] = ex;
            }
        }
        // Materials (slope uses neighbours, so one more sample).
        let mrect = rect.grow(1, n);
        let inp = MaterialInputs {
            h: &self.height,
            water: &self.water,
            wdist: &self.water_dist,
            wlevel: &self.water_near,
            forest: &self.forest,
            disturbed: &self.disturbed,
            seed: self.material_seed,
        };
        let mut updates = Vec::with_capacity((mrect.i1 - mrect.i0 + 1) * (mrect.j1 - mrect.j0 + 1));
        for j in mrect.j0..=mrect.j1 {
            for i in mrect.i0..=mrect.i1 {
                updates.push((i, j, materials::sample(&inp, i, j)));
            }
        }
        for (i, j, m) in updates {
            self.materials.set(i, j, m);
        }
        self.minmax.update(&self.height, &self.water, rect);
        wide.union(mrect)
    }

    /// Tree density on the 8 m forest grid: `forest × (1 − excluded fraction)` over each
    /// cell's samples (deterministic; read by the sim for pollution absorption and land value).
    #[must_use]
    pub fn tree_density(&self) -> Grid<u8> {
        let fnn = self.forest.n();
        let n = self.height.n();
        let step = (n - 1) / (fnn - 1);
        let mut out = Grid::filled(fnn, self.forest.spacing_m(), 0u8);
        for fj in 0..fnn {
            for fi in 0..fnn {
                let f = self.forest.get(fi, fj);
                if f == 0 {
                    continue;
                }
                let (ci, cj) = (fi * step, fj * step);
                let (i0, j0) = (ci.saturating_sub(step / 2), cj.saturating_sub(step / 2));
                let (i1, j1) = ((ci + step / 2).min(n - 1), (cj + step / 2).min(n - 1));
                let mut ex = 0usize;
                let mut tot = 0usize;
                for j in j0..=j1 {
                    for i in i0..=i1 {
                        tot += 1;
                        ex += usize::from(self.excluded[j * n + i]);
                    }
                }
                let frac = 1.0 - ex as f32 / tot.max(1) as f32;
                out.set(fi, fj, (f32::from(f) * frac).round() as u8);
            }
        }
        out
    }
}
