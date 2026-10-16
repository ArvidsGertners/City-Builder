//! Terrain material weights per sample (ALGORITHMS §8): rock by slope, sand near water, forest
//! floor under the forest mask, dirt by moisture noise, grass otherwise. Weights are `u8` and sum
//! to 255. Also the water-distance transform used by sand and the forest mask.

use cb_core::math::smoothstep;

use crate::heightfield::{Grid, HeightGrid};
use crate::noise;
use crate::par::for_rows;
use crate::water::WaterField;

/// Material channels (order of the weight array and of the shader's palette).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum TerrainMaterial {
    Grass = 0,
    Dirt = 1,
    Rock = 2,
    Sand = 3,
    Forest = 4,
}

/// Number of material channels.
pub const MATERIALS: usize = 5;

/// Per-sample material weights.
pub type MaterialWeights = Grid<[u8; MATERIALS]>;

/// Distance to the nearest wet sample (chamfer 3-4, metres, capped at `cap_m`) and that
/// sample's water level.
#[must_use]
pub fn water_distance(water: &WaterField, cap_m: f32) -> (Grid<f32>, Grid<f32>) {
    let lv = water.levels();
    let (n, s) = (lv.n(), lv.spacing_m());
    let mut dist = Grid::filled(n, s, cap_m);
    let mut near = Grid::filled(n, s, f32::NAN);
    for j in 0..n {
        for i in 0..n {
            let l = lv.get(i, j);
            if !l.is_nan() {
                dist.set(i, j, 0.0);
                near.set(i, j, l);
            }
        }
    }
    let (a, d) = (s, s * std::f32::consts::SQRT_2);
    let relax = |dist: &mut Grid<f32>,
                 near: &mut Grid<f32>,
                 i: usize,
                 j: usize,
                 oi: isize,
                 oj: isize,
                 w: f32| {
        let (ni, nj) = (i as isize + oi, j as isize + oj);
        if ni < 0 || nj < 0 || ni >= n as isize || nj >= n as isize {
            return;
        }
        let (ni, nj) = (ni as usize, nj as usize);
        let c = dist.get(ni, nj) + w;
        if c < dist.get(i, j) {
            dist.set(i, j, c);
            near.set(i, j, near.get(ni, nj));
        }
    };
    for j in 0..n {
        for i in 0..n {
            relax(&mut dist, &mut near, i, j, -1, 0, a);
            relax(&mut dist, &mut near, i, j, 0, -1, a);
            relax(&mut dist, &mut near, i, j, -1, -1, d);
            relax(&mut dist, &mut near, i, j, 1, -1, d);
        }
    }
    for j in (0..n).rev() {
        for i in (0..n).rev() {
            relax(&mut dist, &mut near, i, j, 1, 0, a);
            relax(&mut dist, &mut near, i, j, 0, 1, a);
            relax(&mut dist, &mut near, i, j, 1, 1, d);
            relax(&mut dist, &mut near, i, j, -1, 1, d);
        }
    }
    (dist, near)
}

/// Splits fractional weights into `u8`s summing to exactly 255 (largest channel takes the
/// rounding remainder).
#[must_use]
pub fn quantize(w: [f32; MATERIALS]) -> [u8; MATERIALS] {
    let sum: f32 = w.iter().sum::<f32>().max(1e-6);
    let mut out = [0u8; MATERIALS];
    let mut total = 0u32;
    let mut best = 0;
    for k in 0..MATERIALS {
        let v = (w[k] / sum * 255.0).round().clamp(0.0, 255.0) as u8;
        out[k] = v;
        total += u32::from(v);
        if w[k] > w[best] {
            best = k;
        }
    }
    let fix = 255i32 - total as i32;
    out[best] = (i32::from(out[best]) + fix).clamp(0, 255) as u8;
    out
}

/// Inputs shared by every material evaluation.
pub struct MaterialInputs<'a> {
    /// Final heights.
    pub h: &'a HeightGrid,
    /// Water surfaces.
    pub water: &'a WaterField,
    /// Distance to water (m).
    pub wdist: &'a Grid<f32>,
    /// Level of the nearest water.
    pub wlevel: &'a Grid<f32>,
    /// Forest mask (8 m).
    pub forest: &'a Grid<u8>,
    /// Stamp disturbance 0–255 per sample (empty slice = none).
    pub disturbed: &'a [u8],
    /// Seed of the moisture noise.
    pub seed: u64,
}

/// Material weights of sample `(i, j)`: rock by slope, sand near water, forest floor under the
/// forest mask, dirt by moisture noise and where stamps disturbed the ground (shoulders,
/// embankments, cut faces), grass otherwise.
#[must_use]
pub fn sample(inp: &MaterialInputs<'_>, i: usize, j: usize) -> [u8; MATERIALS] {
    let h = inp.h;
    let n = h.n();
    let p = h.pos(i, j);
    let height = h.get(i, j);
    let slope = h.sample_slope(i, j);
    let rock = smoothstep(0.53, 0.84, slope);
    let sand = if inp.water.level(i, j).is_some() {
        1.0
    } else {
        let lv = inp.wlevel.get(i, j);
        if lv.is_nan() {
            0.0
        } else {
            (1.0 - smoothstep(18.0, 25.0, inp.wdist.get(i, j)))
                * (1.0 - smoothstep(1.0, 1.5, height - lv))
        }
    };
    let disturbed = inp
        .disturbed
        .get(j * n + i)
        .map_or(0.0, |d| f32::from(*d) / 255.0);
    let fscale = (inp.forest.n() - 1) as f32 / (n - 1) as f32;
    let fi = ((i as f32 * fscale).round() as usize).min(inp.forest.n() - 1);
    let fj = ((j as f32 * fscale).round() as usize).min(inp.forest.n() - 1);
    let forest_w = f32::from(inp.forest.get(fi, fj)) / 255.0 * (1.0 - disturbed);
    let dirt =
        (smoothstep(0.35, 0.6, noise::fbm(p, 200.0, 3, inp.seed)) * 0.6).max(disturbed * 0.3);
    let r = rock;
    let sa = sand * (1.0 - r);
    let f = forest_w * (1.0 - r - sa).max(0.0);
    let d = dirt * (1.0 - r - sa - f).max(0.0);
    let g = (1.0 - r - sa - f - d).max(0.0);
    quantize([g, d, r, sa, f])
}

/// Material weights for every height sample.
#[must_use]
pub fn compute(
    h: &HeightGrid,
    water: &WaterField,
    wdist: &Grid<f32>,
    wlevel: &Grid<f32>,
    forest: &Grid<u8>,
    seed: u64,
) -> MaterialWeights {
    let n = h.n();
    let inp = MaterialInputs {
        h,
        water,
        wdist,
        wlevel,
        forest,
        disturbed: &[],
        seed,
    };
    let mut data = vec![[0u8; MATERIALS]; n * n];
    for_rows(&mut data, n, |j, row| {
        for (i, out) in row.iter_mut().enumerate() {
            *out = sample(&inp, i, j);
        }
    });
    Grid::from_vec(n, h.spacing_m(), data)
        .unwrap_or_else(|| Grid::filled(n, h.spacing_m(), [255, 0, 0, 0, 0]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::water::WaterBodyKind;

    #[test]
    fn quantized_weights_sum_to_255() {
        for w in [
            [1.0, 0.0, 0.0, 0.0, 0.0],
            [0.3, 0.3, 0.3, 0.05, 0.05],
            [0.2; 5],
            [0.0; 5],
        ] {
            let q = quantize(w);
            assert_eq!(
                q.iter().map(|v| u32::from(*v)).sum::<u32>(),
                255,
                "{w:?} → {q:?}"
            );
        }
    }

    #[test]
    fn distance_transform_and_sand_by_water() {
        let mut h = HeightGrid::filled(65, 2.0, 5.0);
        let mut w = WaterField::dry_like(&h);
        let id = w.add_body(WaterBodyKind::Lake);
        for j in 0..65 {
            for i in 0..4 {
                w.set(i, j, 4.5, id);
                h.set(i, j, 3.0);
            }
        }
        let (d, lv) = water_distance(&w, 60.0);
        assert_eq!(d.get(3, 10), 0.0);
        assert!((d.get(13, 10) - 20.0).abs() < 0.1);
        assert_eq!(lv.get(13, 10), 4.5);
        let forest = Grid::filled(17, 8.0, 0u8);
        let m = compute(&h, &w, &d, &lv, &forest, 1);
        assert!(
            m.get(6, 10)[TerrainMaterial::Sand as usize] > 200,
            "beach next to water"
        );
        assert!(
            m.get(40, 10)[TerrainMaterial::Sand as usize] < 10,
            "no sand 70 m away"
        );
    }
}
