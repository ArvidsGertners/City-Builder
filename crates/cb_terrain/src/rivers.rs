//! Rivers: routed from high ground to the map edge along the cheapest descent (climbing is
//! expensive, so they follow valleys and cross basin rims at the lowest point), meandered with
//! noise, then carved as a channel with banks under a monotonic water surface
//! (ALGORITHMS §7 step 4).

use cb_core::math::smoothstep;
use glam::Vec2;

use crate::heightfield::HeightGrid;
use crate::noise;
use crate::water::{WaterBodyKind, WaterField};

/// Tracing step along the river (m).
pub const STEP_M: f32 = 8.0;
/// Bank width beside the channel (m).
pub const BANK_M: f32 = 30.0;
/// Length over which a river widens from 25 % to its full width and depth (m).
pub const SOURCE_TAPER_M: f32 = 400.0;
/// Bank top above the water surface at the channel edge (m).
pub const FREEBOARD_M: f32 = 0.6;

/// A traced river: centreline points, water level per point (non-increasing), width and depth.
#[derive(Clone, Debug, PartialEq)]
pub struct RiverPath {
    /// Centreline from source to the map edge.
    pub points: Vec<Vec2>,
    /// Water surface height per point (monotonically non-increasing).
    pub levels: Vec<f32>,
    /// Channel width (m).
    pub width_m: f32,
    /// Channel depth at the centre (m).
    pub depth_m: f32,
}

/// Routing grid spacing for river tracing (m).
const ROUTE_STEP_M: f32 = 16.0;
/// Cost per metre of climb (m-equivalents): rivers cross basin rims at their lowest point.
const CLIMB_COST: f32 = 150.0;
/// Meander amplitude (m) and wavelength (m).
const MEANDER_AMP_M: f32 = 60.0;
const MEANDER_WAVELENGTH_M: f32 = 450.0;

/// Traces a river from `source`: least-cost route on a 16 m grid (length × (1 + 2·slope) +
/// 150 × climb) to the first map-edge node at least `min_end_dist_m` from the source, or to
/// existing water (a tributary joining), Chaikin-smoothed, with a noise meander, resampled
/// every [`STEP_M`]. `None` if no such end is reachable.
#[must_use]
pub fn trace(
    h: &HeightGrid,
    water: &WaterField,
    source: Vec2,
    min_end_dist_m: f32,
    seed: u64,
) -> Option<Vec<Vec2>> {
    let half = h.half_extent_m();
    let cn = (2.0 * half / ROUTE_STEP_M) as usize + 1;
    let pos = |k: usize| {
        Vec2::new(
            (k % cn) as f32 * ROUTE_STEP_M - half,
            (k / cn) as f32 * ROUTE_STEP_M - half,
        )
    };
    let si = ((source.x + half) / ROUTE_STEP_M)
        .round()
        .clamp(0.0, (cn - 1) as f32) as usize;
    let sj = ((source.y + half) / ROUTE_STEP_M)
        .round()
        .clamp(0.0, (cn - 1) as f32) as usize;
    let s = sj * cn + si;
    let heights: Vec<f32> = (0..cn * cn).map(|k| h.height_at(pos(k))).collect();
    let mut g = vec![u64::MAX; cn * cn];
    let mut parent = vec![usize::MAX; cn * cn];
    let mut heap = std::collections::BinaryHeap::new();
    g[s] = 0;
    heap.push(std::cmp::Reverse((0u64, s)));
    let mut end = None;
    while let Some(std::cmp::Reverse((gv, v))) = heap.pop() {
        if gv > g[v] {
            continue;
        }
        let (vi, vj) = (v % cn, v / cn);
        let on_edge = vi == 0 || vj == 0 || vi == cn - 1 || vj == cn - 1;
        if (on_edge && (pos(v) - source).length() >= min_end_dist_m)
            || (v != s && water.is_water(pos(v)))
        {
            end = Some(v);
            break;
        }
        if on_edge && v != s {
            continue;
        }
        for (di, dj) in [
            (-1isize, 0isize),
            (1, 0),
            (0, -1),
            (0, 1),
            (-1, -1),
            (1, -1),
            (-1, 1),
            (1, 1),
        ] {
            let (wi, wj) = (vi as isize + di, vj as isize + dj);
            if wi < 0 || wj < 0 || wi >= cn as isize || wj >= cn as isize {
                continue;
            }
            let w = wj as usize * cn + wi as usize;
            let len = ROUTE_STEP_M
                * if di != 0 && dj != 0 {
                    std::f32::consts::SQRT_2
                } else {
                    1.0
                };
            let dh = heights[w] - heights[v];
            // Cost noise breaks up grid-straight runs.
            let wobble = 1.0 + 0.6 * noise::fbm(pos(w), 300.0, 2, seed ^ 0x51DE);
            // Keep away from the smooth map margin (it would be the cheapest corridor).
            let pw = pos(w);
            let wobble = wobble
                * (1.0 + 4.0 * (1.0 - smoothstep(0.0, 600.0, half - pw.x.abs().max(pw.y.abs()))));
            let c = len * wobble * (1.0 + 2.0 * dh.abs() / len) + CLIMB_COST * dh.max(0.0);
            let ng = gv + (c * 100.0) as u64;
            if ng < g[w] {
                g[w] = ng;
                parent[w] = v;
                heap.push(std::cmp::Reverse((ng, w)));
            }
        }
    }
    let mut node = end?;
    let mut route = vec![pos(node)];
    while parent[node] != usize::MAX {
        node = parent[node];
        route.push(pos(node));
    }
    route.reverse();
    route[0] = source;
    // Chaikin smoothing (endpoints kept).
    for _ in 0..3 {
        let mut sm = vec![route[0]];
        for w in route.windows(2) {
            sm.push(w[0] * 0.75 + w[1] * 0.25);
            sm.push(w[0] * 0.25 + w[1] * 0.75);
        }
        sm.push(*route.last()?);
        route = sm;
    }
    // Meander: perpendicular offset by smooth noise of arc length, faded in at the source and
    // out at the mouth so the river still starts at the source and ends on the edge.
    let total: f32 = route.windows(2).map(|w| (w[1] - w[0]).length()).sum();
    let mut out = Vec::with_capacity(route.len());
    let mut acc = 0.0;
    for k in 0..route.len() {
        if k > 0 {
            acc += (route[k] - route[k - 1]).length();
        }
        let a = route[k.saturating_sub(1)];
        let b = route[(k + 1).min(route.len() - 1)];
        let t = (b - a).normalize_or(Vec2::X);
        let fade = smoothstep(0.0, 150.0, acc) * smoothstep(0.0, 150.0, total - acc);
        let off =
            noise::fbm(Vec2::new(acc, 0.5), MEANDER_WAVELENGTH_M, 2, seed) * MEANDER_AMP_M * fade;
        out.push(route[k] + Vec2::new(-t.y, t.x) * off);
    }
    Some(resample(&out, STEP_M))
}

/// Resamples a polyline at uniform arc-length spacing (last point kept).
fn resample(pts: &[Vec2], step: f32) -> Vec<Vec2> {
    let mut out = vec![pts[0]];
    let mut carry = 0.0;
    for w in pts.windows(2) {
        let seg = (w[1] - w[0]).length();
        let mut t = step - carry;
        while t <= seg {
            out.push(w[0] + (w[1] - w[0]) * (t / seg));
            t += step;
        }
        carry = seg - (t - step);
    }
    if let Some(last) = pts.last()
        && out.last() != Some(last)
    {
        out.push(*last);
    }
    out
}

/// Water levels along a traced path: never above the terrain minus the freeboard and never
/// rising downstream.
#[must_use]
pub fn levels(h: &HeightGrid, points: &[Vec2]) -> Vec<f32> {
    let mut out = Vec::with_capacity(points.len());
    let mut w = f32::INFINITY;
    for p in points {
        w = w.min(h.height_at(*p) - 1.0);
        out.push(w);
    }
    out
}

/// Carves the channel and banks of `river` into `h` and marks its water in `water` (body `id`).
/// Returns the number of wet samples.
pub fn carve(h: &mut HeightGrid, water: &mut WaterField, river: &RiverPath, id: u8) -> usize {
    let n = h.n();
    let half_w = river.width_m * 0.5;
    // Rivers start as streams: width and depth ramp up over the first SOURCE_TAPER_M.
    let mut acc = 0.0;
    let taper: Vec<f32> = (0..river.points.len())
        .map(|k| {
            if k > 0 {
                acc += (river.points[k] - river.points[k - 1]).length();
            }
            0.25 + 0.75 * smoothstep(0.0, SOURCE_TAPER_M, acc)
        })
        .collect();
    // Bank width per point: 30 m, plus 2.5 m per metre the terrain stands above the bank top
    // (cuts through rises get 1:2.5 side slopes instead of slot canyons).
    let banks: Vec<f32> = river
        .points
        .iter()
        .zip(&river.levels)
        .map(|(p, l)| BANK_M + 2.5 * (h.height_at(*p) - l - FREEBOARD_M).max(0.0))
        .collect();
    let max_bank = banks.iter().copied().fold(BANK_M, f32::max);
    let reach = half_w + max_bank;
    // Nearest centreline distance and interpolated level per affected sample.
    let mut dist = vec![f32::INFINITY; n * n];
    let mut lev = vec![0.0f32; n * n];
    let mut bank = vec![BANK_M; n * n];
    let mut tap = vec![1.0f32; n * n];
    for k in 0..river.points.len().saturating_sub(1) {
        let (a, b) = (river.points[k], river.points[k + 1]);
        let (la, lb) = (river.levels[k], river.levels[k + 1]);
        let lo = h.to_grid(a.min(b) - Vec2::splat(reach));
        let hi = h.to_grid(a.max(b) + Vec2::splat(reach));
        let m = (n - 1) as f32;
        let (i0, j0) = (
            lo.x.floor().clamp(0.0, m) as usize,
            lo.y.floor().clamp(0.0, m) as usize,
        );
        let (i1, j1) = (
            hi.x.ceil().clamp(0.0, m) as usize,
            hi.y.ceil().clamp(0.0, m) as usize,
        );
        let ab = b - a;
        let len2 = ab.length_squared().max(1e-6);
        for j in j0..=j1 {
            for i in i0..=i1 {
                let q = h.pos(i, j);
                let t = ((q - a).dot(ab) / len2).clamp(0.0, 1.0);
                let d = (q - (a + ab * t)).length();
                let idx = j * n + i;
                if d < dist[idx] {
                    dist[idx] = d;
                    lev[idx] = la + (lb - la) * t;
                    bank[idx] = banks[k] + (banks[k + 1] - banks[k]) * t;
                    tap[idx] = taper[k] + (taper[k + 1] - taper[k]) * t;
                }
            }
        }
    }
    let mut wet = 0;
    for j in 0..n {
        for i in 0..n {
            let idx = j * n + i;
            let d = dist[idx];
            let bank_m = bank[idx];
            let (half_w, depth_m) = (half_w * tap[idx], river.depth_m * tap[idx]);
            if d > half_w + bank_m {
                continue;
            }
            let w = lev[idx];
            let h0 = h.get(i, j);
            let top = w + FREEBOARD_M;
            let new_h = if d <= half_w {
                let u = d / half_w;
                (top - (depth_m + FREEBOARD_M) * (1.0 - u * u)).min(h0.max(top))
            } else {
                let t = smoothstep(0.0, 1.0, (d - half_w) / bank_m);
                top + (h0 - top) * t
            };
            h.set(i, j, new_h);
            if d <= half_w && new_h < w {
                water.set(i, j, w, id);
                wet += 1;
            }
        }
    }
    wet
}

/// Registers a river body in `water`.
pub fn add_body(water: &mut WaterField) -> u8 {
    water.add_body(WaterBodyKind::River)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tilted(n: usize) -> HeightGrid {
        let mut g = HeightGrid::filled(n, 2.0, 0.0);
        for j in 0..n {
            for i in 0..n {
                let p = g.pos(i, j);
                g.set(i, j, 60.0 - p.x * 0.05);
            }
        }
        g
    }

    #[test]
    fn trace_reaches_an_edge_with_monotonic_surface() {
        let g = tilted(257);
        let w = WaterField::dry_like(&g);
        let path = trace(&g, &w, Vec2::new(-200.0, 0.0), 0.0, 3).expect("river reaches an edge");
        let last = *path.last().expect("non-empty");
        let e = g.half_extent_m();
        assert!(
            (last.x.abs() - e).abs() < 1e-3 || (last.y.abs() - e).abs() < 1e-3,
            "ends on an edge: {last}"
        );
        let lv = levels(&g, &path);
        assert!(lv.windows(2).all(|w| w[1] <= w[0]), "monotonic surface");
        // A minimum end distance forbids the near edges.
        let far = trace(&g, &w, Vec2::new(-200.0, 0.0), 400.0, 3).expect("far edge reachable");
        assert!((*far.last().expect("non-empty") - Vec2::new(-200.0, 0.0)).length() >= 390.0);
    }

    #[test]
    fn carve_makes_channel_with_water_below_banks() {
        let mut g = tilted(257);
        let mut w = WaterField::dry_like(&g);
        let points: Vec<Vec2> = (0..60)
            .map(|k| Vec2::new(-240.0 + k as f32 * 8.0, 0.0))
            .collect();
        let lv = levels(&g, &points);
        let r = RiverPath {
            points,
            levels: lv,
            width_m: 20.0,
            depth_m: 4.0,
        };
        let id = add_body(&mut w);
        let wet = carve(&mut g, &mut w, &r, id);
        assert!(wet > 300);
        let c = Vec2::new(0.0, 0.0);
        let level = w.level_at(c).expect("centre is wet");
        // 240 m from the source the channel is at ~74 % of its full 4 m depth (source taper).
        let depth = level - g.height_at(c);
        assert!((2.6..=4.0).contains(&depth), "centre depth {depth}");
        // Banks are above the water surface beside the channel.
        assert!(g.height_at(Vec2::new(0.0, 14.0)) > level);
        assert!(!w.is_water(Vec2::new(0.0, 14.0)));
    }
}
