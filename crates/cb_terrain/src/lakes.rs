//! Lakes: a noise-warped bowl is dug at a lowland site, the depression is filled by a
//! priority flood (ties by sample index) inside a window, and the lake surface is set to
//! `spill − 0.5 m` (ALGORITHMS §7 step 5).

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

use glam::Vec2;

use crate::heightfield::HeightGrid;
use crate::noise;
use crate::water::{WaterBodyKind, WaterField};

/// Minimum lake area (m²).
pub const MIN_AREA_M2: f32 = 50_000.0;
/// Lake surface below the spill level (m).
pub const SPILL_MARGIN_M: f32 = 0.5;
/// Extra window around the bowl used for the flood (m).
const WINDOW_MARGIN_M: f32 = 500.0;

/// A generated lake.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lake {
    /// Site the lake was grown from.
    pub center: Vec2,
    /// Flat water surface height.
    pub level: f32,
    /// Wet samples.
    pub samples: usize,
    /// Water body id.
    pub id: u8,
}

/// Digs a smooth bowl of `radius_m` and `depth_m` around `center`, its rim warped by noise.
pub fn dig_bowl(h: &mut HeightGrid, center: Vec2, radius_m: f32, depth_m: f32, seed: u64) {
    let reach = radius_m * 1.35;
    let lo = h.to_grid(center - Vec2::splat(reach));
    let hi = h.to_grid(center + Vec2::splat(reach));
    let m = (h.n() - 1) as f32;
    let (i0, j0) = (
        lo.x.floor().clamp(0.0, m) as usize,
        lo.y.floor().clamp(0.0, m) as usize,
    );
    let (i1, j1) = (
        hi.x.ceil().clamp(0.0, m) as usize,
        hi.y.ceil().clamp(0.0, m) as usize,
    );
    for j in j0..=j1 {
        for i in i0..=i1 {
            let p = h.pos(i, j);
            let warped = noise::warp(p, radius_m * 0.8, radius_m * 0.25, seed);
            let d = (warped - center).length() / radius_m;
            if d < 1.0 {
                let f = 1.0 - d * d;
                h.set(i, j, h.get(i, j) - depth_m * f * f);
            }
        }
    }
}

/// Orderable key for non-negative finite heights (total order on the bit pattern).
fn key(v: f32) -> u32 {
    let b = v.to_bits();
    if b >> 31 == 0 { b | 0x8000_0000 } else { !b }
}

/// Fills the depression containing `center` and marks it as a lake (body registered on
/// success). `None` if the lake would be shallower than `min_depth_m`, smaller than
/// [`MIN_AREA_M2`], touch the flood window, or touch existing water.
pub fn fill(
    h: &HeightGrid,
    water: &mut WaterField,
    center: Vec2,
    radius_m: f32,
    min_depth_m: f32,
) -> Option<Lake> {
    let n = h.n();
    let reach = radius_m * 1.35 + WINDOW_MARGIN_M;
    let m = (n - 1) as f32;
    let lo = h.to_grid(center - Vec2::splat(reach));
    let hi = h.to_grid(center + Vec2::splat(reach));
    let (i0, j0) = (
        lo.x.floor().clamp(0.0, m) as usize,
        lo.y.floor().clamp(0.0, m) as usize,
    );
    let (i1, j1) = (
        hi.x.ceil().clamp(0.0, m) as usize,
        hi.y.ceil().clamp(0.0, m) as usize,
    );
    let (wn, hn) = (i1 - i0 + 1, j1 - j0 + 1);
    let widx = |i: usize, j: usize| (j - j0) * wn + (i - i0);
    // Priority flood from the window border inward.
    let mut filled = vec![f32::NAN; wn * hn];
    let mut heap: BinaryHeap<Reverse<(u32, usize)>> = BinaryHeap::new();
    for j in j0..=j1 {
        for i in i0..=i1 {
            if i == i0 || i == i1 || j == j0 || j == j1 {
                let v = h.get(i, j);
                filled[widx(i, j)] = v;
                heap.push(Reverse((key(v), h.index(i, j))));
            }
        }
    }
    while let Some(Reverse((k, gi))) = heap.pop() {
        let (i, j) = (gi % n, gi / n);
        let level = filled[widx(i, j)];
        let _ = k;
        for (di, dj) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
            let (ni, nj) = (i as i32 + di, j as i32 + dj);
            if ni < i0 as i32 || nj < j0 as i32 || ni > i1 as i32 || nj > j1 as i32 {
                continue;
            }
            let (ni, nj) = (ni as usize, nj as usize);
            let w = widx(ni, nj);
            if !filled[w].is_nan() {
                continue;
            }
            let v = h.get(ni, nj).max(level);
            filled[w] = v;
            heap.push(Reverse((key(v), h.index(ni, nj))));
        }
    }
    let (ci, cj) = h.nearest(center);
    let spill = filled[widx(ci, cj)];
    let level = spill - SPILL_MARGIN_M;
    if level - h.get(ci, cj) < min_depth_m {
        return None;
    }
    // Lake = samples below the level connected to the site.
    let mut seen = vec![false; wn * hn];
    let mut cells = Vec::new();
    let mut queue = VecDeque::from([(ci, cj)]);
    seen[widx(ci, cj)] = true;
    while let Some((i, j)) = queue.pop_front() {
        if i == i0 || i == i1 || j == j0 || j == j1 || water.level(i, j).is_some() {
            return None;
        }
        cells.push((i, j));
        for (di, dj) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
            let (ni, nj) = ((i as i32 + di) as usize, (j as i32 + dj) as usize);
            let w = widx(ni, nj);
            if !seen[w] && h.get(ni, nj) < level {
                seen[w] = true;
                queue.push_back((ni, nj));
            }
        }
    }
    let area = cells.len() as f32 * h.spacing_m() * h.spacing_m();
    if area < MIN_AREA_M2 {
        return None;
    }
    let id = water.add_body(WaterBodyKind::Lake);
    for (i, j) in &cells {
        water.set(*i, *j, level, id);
    }
    Some(Lake {
        center,
        level,
        samples: cells.len(),
        id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bowl_becomes_lake_at_spill_minus_margin() {
        let mut g = HeightGrid::filled(513, 2.0, 20.0);
        dig_bowl(&mut g, Vec2::ZERO, 200.0, 8.0, 5);
        let mut w = WaterField::dry_like(&g);
        let lake = fill(&g, &mut w, Vec2::ZERO, 200.0, 1.5).expect("lake");
        assert!((lake.level - (20.0 - SPILL_MARGIN_M)).abs() < 1e-4);
        assert!(lake.samples as f32 * 4.0 >= MIN_AREA_M2);
        assert!(w.depth_at(Vec2::ZERO, &g) > 6.0);
        assert!(!w.is_water(Vec2::new(400.0, 0.0)));
    }

    #[test]
    fn shallow_or_flat_sites_are_rejected() {
        let g = HeightGrid::filled(257, 2.0, 20.0);
        let mut w = WaterField::dry_like(&g);
        assert!(fill(&g, &mut w, Vec2::ZERO, 100.0, 1.5).is_none());
        assert!(w.bodies().is_empty());
    }

    #[test]
    fn height_key_orders_like_floats() {
        let v = [0.0f32, 0.5, 1.0, 20.0, 250.0];
        assert!(v.windows(2).all(|p| key(p[0]) < key(p[1])));
    }
}
