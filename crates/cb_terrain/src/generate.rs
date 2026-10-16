//! World generation from a seed (ALGORITHMS §7): macro shape with a warped valley, ridged
//! hills, detail, thermal erosion, lowland flattening, rivers, lakes, outside connections,
//! constraint checks with sub-stream re-rolls, then derived data (forest mask, materials).
//!
//! Every random choice comes from `derive_seed(world_seed, Domain::X, attempt, k)`, so a
//! re-roll of one sub-stream (e.g. rivers) leaves the others untouched.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use cb_core::hash::{Domain, derive_seed};
use cb_core::math::{powf, sin_cos, smoothstep};
use cb_core::rng::Pcg32;
use glam::Vec2;

use crate::TerrainError;
use crate::heightfield::{Grid, HeightGrid};
use crate::lakes::{self, Lake};
use crate::materials::{self, MaterialWeights};
use crate::noise;
use crate::par::for_rows;
use crate::rivers::{self, RiverPath};
use crate::water::WaterField;

/// Height quantum of the saved base heights (`u16`, 1/256 m, range 0–256 m; ADR-0044).
pub const HEIGHT_QUANTUM_M: f32 = 1.0 / 256.0;
/// Slope below which land counts as buildable lowland.
pub const BUILDABLE_SLOPE: f32 = 0.08;
/// Minimum buildable lowland (km²).
pub const MIN_BUILDABLE_KM2: f32 = 3.0;
/// Required water depth for the tutorial shore (m).
pub const SHORE_DEPTH_M: f32 = 1.5;
/// Maximum distance from the primary outside connection's end to that shore (m).
pub const SHORE_REACH_M: f32 = 400.0;
/// Re-rolls per sub-stream before the constraint is relaxed.
pub const MAX_REROLLS: u32 = 8;
/// Forest-mask spacing (m).
pub const FOREST_SPACING_M: f32 = 8.0;

/// Generation parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldGenParams {
    /// Samples per side (2 049 for the 4 096 m map at 2 m).
    pub samples: usize,
    /// Sample spacing (m).
    pub spacing_m: f32,
}

impl Default for WorldGenParams {
    fn default() -> Self {
        Self {
            samples: 2049,
            spacing_m: 2.0,
        }
    }
}

/// An outside connection: a road path from the map edge to its end inside the map.
#[derive(Clone, Debug, PartialEq)]
pub struct OutsideConnection {
    /// Polyline from the edge point (first) to the end (last), 16 m resolution.
    pub path: Vec<Vec2>,
}

impl OutsideConnection {
    /// End point inside the map.
    #[must_use]
    pub fn end(&self) -> Vec2 {
        self.path.last().copied().unwrap_or(Vec2::ZERO)
    }
}

/// Generation diagnostics.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldReport {
    /// Buildable lowland (dry, slope < 8 %) in km².
    pub buildable_km2: f32,
    /// Distance from the primary connection's end to water ≥ 1.5 m deep (m).
    pub shore_distance_m: f32,
    /// Re-rolls used per sub-stream: (shape, rivers, lakes).
    pub rerolls: (u32, u32, u32),
    /// Constraints relaxed after `MAX_REROLLS` (names).
    pub relaxed: Vec<&'static str>,
}

/// A generated world.
#[derive(Clone, Debug, PartialEq)]
pub struct World {
    /// Base heights (quantized to [`HEIGHT_QUANTUM_M`]); saved.
    pub base: HeightGrid,
    /// Water surfaces; saved.
    pub water: WaterField,
    /// River centrelines.
    pub rivers: Vec<RiverPath>,
    /// Lakes.
    pub lakes: Vec<Lake>,
    /// Outside connections; the first one ends within [`SHORE_REACH_M`] of a shore.
    pub outside: Vec<OutsideConnection>,
    /// Forest density (8 m grid, 0–255); saved.
    pub forest: Grid<u8>,
    /// Material weights (derived).
    pub materials: MaterialWeights,
    /// Seed of the material moisture noise (for incremental repaints).
    pub material_seed: u64,
    /// Diagnostics.
    pub report: WorldReport,
}

impl World {
    /// A flat, dry, treeless world at height `h` (tests, benchmarks, sandbox scenarios).
    #[must_use]
    pub fn flat(params: &WorldGenParams, h: f32) -> Self {
        let base = HeightGrid::filled(params.samples, params.spacing_m, h);
        let water = WaterField::dry_like(&base);
        let (wdist, wlevel) = materials::water_distance(&water, 64.0);
        let step = (FOREST_SPACING_M / params.spacing_m).round().max(1.0) as usize;
        let fnn = (params.samples - 1) / step + 1;
        let forest = Grid::filled(fnn, FOREST_SPACING_M, 0);
        let materials = materials::compute(&base, &water, &wdist, &wlevel, &forest, 0);
        Self {
            base,
            water,
            rivers: Vec::new(),
            lakes: Vec::new(),
            outside: Vec::new(),
            forest,
            materials,
            material_seed: 0,
            report: WorldReport {
                buildable_km2: 0.0,
                shore_distance_m: f32::INFINITY,
                rerolls: (0, 0, 0),
                relaxed: Vec::new(),
            },
        }
    }

    /// Stable hash of everything generation produces (base, water, forest, materials) — compared
    /// across operating systems by the determinism probe.
    #[must_use]
    pub fn content_hash(&self) -> u64 {
        use core::hash::Hasher;
        let mut h = cb_core::hash::StableHasher::with_key(0x7E44);
        for v in self.base.data() {
            h.write_u32(v.to_bits());
        }
        for v in self.water.levels().data() {
            h.write_u32(v.to_bits());
        }
        h.write(self.forest.data());
        for m in self.materials.data() {
            h.write(m);
        }
        for oc in &self.outside {
            for p in &oc.path {
                h.write_u32(p.x.to_bits());
                h.write_u32(p.y.to_bits());
            }
        }
        h.finish()
    }
}

/// Generates a world. Pure in `(params, seed)`.
///
/// # Errors
/// [`TerrainError::InvalidParams`] for grids smaller than 129 samples or non-positive spacing.
pub fn generate(params: &WorldGenParams, seed: u64) -> Result<World, TerrainError> {
    if params.samples < 129 || params.spacing_m <= 0.0 {
        return Err(TerrainError::InvalidParams(
            "samples ≥ 129 and spacing > 0 required",
        ));
    }
    let mut report = WorldReport {
        buildable_km2: 0.0,
        shore_distance_m: f32::INFINITY,
        rerolls: (0, 0, 0),
        relaxed: Vec::new(),
    };

    // Shape (steps 1–3, erosion, flattening) — re-rolled for too little buildable land.
    let mut shape_attempt = 0;
    let mut base = loop {
        let h = shape(params, seed, shape_attempt);
        let km2 = buildable_km2(&h, None);
        if km2 >= MIN_BUILDABLE_KM2 * 1.15 || shape_attempt + 1 >= MAX_REROLLS {
            if km2 < MIN_BUILDABLE_KM2 * 1.15 {
                report.relaxed.push("buildable_lowland");
            }
            break h;
        }
        shape_attempt += 1;
    };
    report.rerolls.0 = shape_attempt;

    // Rivers (1–2), lakes (1–3): each re-rolled on its own stream.
    let shaped = base.clone();
    let mut river_attempt = 0;
    let (mut water, rivers) = loop {
        base.data_mut().copy_from_slice(shaped.data());
        let mut water = WaterField::dry_like(&base);
        let rivers = make_rivers(&mut base, &mut water, seed, river_attempt);
        if !rivers.is_empty() || river_attempt + 1 >= MAX_REROLLS {
            if rivers.is_empty() {
                report.relaxed.push("rivers");
            }
            break (water, rivers);
        }
        river_attempt += 1;
    };
    report.rerolls.1 = river_attempt;
    let with_rivers = base.clone();
    let water_rivers = water.clone();
    let mut lake_attempt = 0;
    let lakes = loop {
        base.data_mut().copy_from_slice(with_rivers.data());
        water = water_rivers.clone();
        let lakes = make_lakes(&mut base, &mut water, seed, lake_attempt);
        if !lakes.is_empty() || lake_attempt + 1 >= MAX_REROLLS {
            if lakes.is_empty() {
                report.relaxed.push("lakes");
            }
            break lakes;
        }
        lake_attempt += 1;
    };
    report.rerolls.2 = lake_attempt;

    // Quantize the base so the saved u16 restores bitwise; water levels stay as computed.
    for v in base.data_mut() {
        *v = quantize_height(*v);
    }

    let (wdist, wlevel) = materials::water_distance(&water, 64.0);
    let outside = make_outside(&base, &water, &wdist, seed, &mut report);
    report.buildable_km2 = buildable_km2(&base, Some(&water));
    if report.buildable_km2 < MIN_BUILDABLE_KM2 && !report.relaxed.contains(&"buildable_lowland") {
        report.relaxed.push("buildable_lowland");
    }
    let forest = forest_mask(&base, &wdist, seed);
    let material_seed = derive_seed(seed, Domain::Terrain, 9, 0);
    let materials = materials::compute(&base, &water, &wdist, &wlevel, &forest, material_seed);
    Ok(World {
        base,
        water,
        rivers,
        lakes,
        outside,
        forest,
        materials,
        material_seed,
        report,
    })
}

/// Quantizes a height to the saved `u16` grid.
#[must_use]
pub fn quantize_height(h: f32) -> f32 {
    (h / HEIGHT_QUANTUM_M).round().clamp(0.0, 65_535.0) * HEIGHT_QUANTUM_M
}

/// Steps 1–3 + erosion + lowland flattening.
fn shape(params: &WorldGenParams, seed: u64, attempt: u32) -> HeightGrid {
    let n = params.samples;
    let s0 = derive_seed(seed, Domain::Terrain, u64::from(attempt), 0);
    let s_warp = derive_seed(seed, Domain::Terrain, u64::from(attempt), 1);
    let s_hill = derive_seed(seed, Domain::Terrain, u64::from(attempt), 2);
    let s_det = derive_seed(seed, Domain::Terrain, u64::from(attempt), 3);
    let mut rng = Pcg32::from_seed(derive_seed(seed, Domain::Terrain, u64::from(attempt), 4));
    let (sn, cs) = sin_cos(rng.range_f32(0.0, cb_core::math::PI));
    let axis_n = Vec2::new(-sn, cs);
    let axis_c = Vec2::new(rng.range_f32(-600.0, 600.0), rng.range_f32(-600.0, 600.0));
    let grid = HeightGrid::filled(n, params.spacing_m, 0.0);
    let half = grid.half_extent_m();
    let mut data = vec![0.0f32; n * n];
    for_rows(&mut data, n, |j, row| {
        for (i, out) in row.iter_mut().enumerate() {
            let p = grid.pos(i, j);
            let pw = noise::warp(p, 1500.0, 320.0, s_warp);
            let m = (0.5 + 0.8 * noise::fbm(pw, 2500.0, 4, s0)).clamp(0.0, 1.0);
            let valley = 1.0 - smoothstep(150.0, 700.0, (pw - axis_c).dot(axis_n).abs());
            let macro_h = (12.0 + 120.0 * powf(m, 1.5)) * (1.0 - 0.6 * valley);
            let edge = smoothstep(0.0, 300.0, half - p.x.abs().max(p.y.abs()));
            // Hills: half ridged (crests, some rock), half rounded fBm (no scalloped bands).
            let r = noise::ridged(p, 650.0, 2, s_hill);
            let round = 0.5 + 0.5 * noise::fbm(p, 520.0, 3, s_hill ^ 0x40);
            let hills =
                115.0 * (0.6 * r * r + 0.4 * round * round) * smoothstep(0.3, 0.75, m) * edge;
            let detail = 3.0 * noise::fbm(p, 120.0, 2, s_det) * edge;
            *out = (macro_h + hills + detail).clamp(8.0, 220.0);
        }
    });
    let mut h = Grid::from_vec(n, params.spacing_m, data).unwrap_or(grid);
    for _ in 0..6 {
        h = thermal_erosion(&h);
    }
    flatten_lowland(&h)
}

/// One Jacobi pass of thermal erosion (talus 35°); mass-conserving and order-independent.
fn thermal_erosion(h: &HeightGrid) -> HeightGrid {
    let n = h.n();
    let talus = 0.700_207_5 * h.spacing_m(); // tan 35° × spacing
    let mut out = h.data().to_vec();
    for_rows(&mut out, n, |j, row| {
        for (i, v) in row.iter_mut().enumerate() {
            let c = h.get(i, j);
            let mut delta = 0.0;
            for (di, dj) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)] {
                let nb = h.get_clamped(i as isize + di, j as isize + dj);
                let d = c - nb;
                if d > talus {
                    delta -= 0.125 * (d - talus);
                } else if -d > talus {
                    delta += 0.125 * (-d - talus);
                }
            }
            *v = c + delta;
        }
    });
    Grid::from_vec(n, h.spacing_m(), out).unwrap_or_else(|| h.clone())
}

/// Moves gentle land 20 % toward a 33-sample box blur (slope < 8 %, faded 6–8 %).
fn flatten_lowland(h: &HeightGrid) -> HeightGrid {
    let n = h.n();
    let r = 8usize;
    let blur_rows = |src: &[f32], transpose: bool| {
        let mut out = vec![0.0f32; n * n];
        for_rows(&mut out, n, |j, row| {
            let at = |i: usize| {
                if transpose {
                    src[i * n + j]
                } else {
                    src[j * n + i]
                }
            };
            let mut acc = 0.0f64;
            let mut cnt = 0usize;
            for k in 0..=r.min(n - 1) {
                acc += f64::from(at(k));
                cnt += 1;
            }
            for (i, v) in row.iter_mut().enumerate() {
                *v = (acc / cnt as f64) as f32;
                if i + r + 1 < n {
                    acc += f64::from(at(i + r + 1));
                    cnt += 1;
                }
                if i >= r {
                    acc -= f64::from(at(i - r));
                    cnt -= 1;
                }
            }
        });
        out
    };
    let a = blur_rows(h.data(), false);
    let b = blur_rows(&a, true); // b[j*n+i] = blur along the first axis of a, transposed
    let mut out = h.data().to_vec();
    for_rows(&mut out, n, |j, row| {
        for (i, v) in row.iter_mut().enumerate() {
            let w = 0.2 * (1.0 - smoothstep(0.06, BUILDABLE_SLOPE, h.sample_slope(i, j)));
            let smooth = b[i * n + j];
            *v += (smooth - *v) * w;
        }
    });
    Grid::from_vec(n, h.spacing_m(), out).unwrap_or_else(|| h.clone())
}

/// Buildable lowland: dry samples with slope < 8 % (km²).
fn buildable_km2(h: &HeightGrid, water: Option<&WaterField>) -> f32 {
    let n = h.n();
    let mut count = 0usize;
    for j in 0..n {
        for i in 0..n {
            if water.is_some_and(|w| w.level(i, j).is_some()) {
                continue;
            }
            if h.sample_slope(i, j) < BUILDABLE_SLOPE {
                count += 1;
            }
        }
    }
    count as f32 * h.spacing_m() * h.spacing_m() / 1e6
}

fn make_rivers(
    h: &mut HeightGrid,
    water: &mut WaterField,
    seed: u64,
    attempt: u32,
) -> Vec<RiverPath> {
    let mut rng = Pcg32::from_seed(derive_seed(seed, Domain::River, u64::from(attempt), 0));
    let half = h.half_extent_m();
    let mut out: Vec<RiverPath> = Vec::new();
    // River 1 flows through the map: it enters at the highest of 48 edge points and leaves by
    // an edge at least 2.5 km away.
    let mut best: Option<(f32, Vec2)> = None;
    for _ in 0..48 {
        let t = rng.range_f32(-half + 400.0, half - 400.0);
        let p = match rng.below(4) {
            0 => Vec2::new(half, t),
            1 => Vec2::new(-half, t),
            2 => Vec2::new(t, half),
            _ => Vec2::new(t, -half),
        };
        let ht = h.height_at(p);
        if best.is_none_or(|b| ht > b.0) {
            best = Some((ht, p));
        }
    }
    let add = |h: &mut HeightGrid,
               water: &mut WaterField,
               points: Vec<Vec2>,
               rng: &mut Pcg32,
               out: &mut Vec<RiverPath>| {
        let levels = rivers::levels(h, &points);
        let river = RiverPath {
            points,
            levels,
            width_m: rng.range_f32(18.0, 40.0),
            depth_m: rng.range_f32(3.0, 5.0),
        };
        let id = rivers::add_body(water);
        rivers::carve(h, water, &river, id);
        out.push(river);
    };
    if let Some((_, src)) = best
        && let Some(points) = rivers::trace(
            h,
            water,
            src,
            2500.0,
            derive_seed(seed, Domain::River, u64::from(attempt), 10),
        )
    {
        add(h, water, points, &mut rng, &mut out);
    }
    // Optional tributary from inner high ground (to an edge or into river 1).
    if !out.is_empty() && rng.chance(0.5) {
        let mut best: Option<(f32, Vec2)> = None;
        for _ in 0..32 {
            let p = Vec2::new(
                rng.range_f32(-1100.0, 1100.0),
                rng.range_f32(-1100.0, 1100.0),
            );
            if out[0]
                .points
                .iter()
                .step_by(8)
                .any(|q| (*q - p).length() < 700.0)
            {
                continue;
            }
            let ht = h.height_at(p);
            if best.is_none_or(|b| ht > b.0) {
                best = Some((ht, p));
            }
        }
        if let Some((_, src)) = best
            && let Some(points) = rivers::trace(
                h,
                water,
                src,
                0.0,
                derive_seed(seed, Domain::River, u64::from(attempt), 11),
            )
        {
            add(h, water, points, &mut rng, &mut out);
        }
    }
    out
}

fn make_lakes(h: &mut HeightGrid, water: &mut WaterField, seed: u64, attempt: u32) -> Vec<Lake> {
    let mut rng = Pcg32::from_seed(derive_seed(seed, Domain::Lake, u64::from(attempt), 0));
    let want = 1 + rng.below(3) as usize;
    // Lowland reference: median height of a coarse sample.
    let mut hs: Vec<f32> = (0..1024)
        .map(|k| {
            h.height_at(Vec2::new(
                ((k % 32) as f32 - 15.5) * 120.0,
                ((k / 32) as f32 - 15.5) * 120.0,
            ))
        })
        .collect();
    hs.sort_by(f32::total_cmp);
    let low = hs[hs.len() * 2 / 5];
    let mut out: Vec<Lake> = Vec::new();
    for k in 0..want * 6 {
        if out.len() >= want {
            break;
        }
        let c = Vec2::new(
            rng.range_f32(-1500.0, 1500.0),
            rng.range_f32(-1500.0, 1500.0),
        );
        let radius = rng.range_f32(180.0, 320.0);
        let depth = rng.range_f32(6.0, 10.0);
        if h.height_at(c) > low
            || h.slope_at(c) > 0.06
            || out.iter().any(|l| (l.center - c).length() < 600.0 + radius)
        {
            continue;
        }
        let ring_dry = (0..32).all(|q| {
            let (sn, cs) = sin_cos(q as f32 * cb_core::math::TAU / 32.0);
            !water.is_water(c + Vec2::new(cs, sn) * (radius + 150.0))
        });
        if !ring_dry || water.is_water(c) {
            continue;
        }
        let before = h.clone();
        lakes::dig_bowl(
            h,
            c,
            radius,
            depth,
            derive_seed(seed, Domain::Lake, u64::from(attempt), 100 + k as u64),
        );
        match lakes::fill(h, water, c, radius, SHORE_DEPTH_M + 1.0) {
            Some(l) => out.push(l),
            None => h.data_mut().copy_from_slice(before.data()),
        }
    }
    out
}

/// Coarse grid A* (16 m) for outside connections: cost = length × (1 + 40 slope²), ×8 above
/// 8 % slope, water impassable. Ties by node index.
fn coarse_path(
    h: &HeightGrid,
    water: &WaterField,
    from: Vec2,
    to: Vec2,
    allow_water: bool,
) -> Option<Vec<Vec2>> {
    let step = 16.0;
    let half = h.half_extent_m();
    let cn = (2.0 * half / step) as usize + 1;
    let node_pos =
        |k: usize| Vec2::new((k % cn) as f32 * step - half, (k / cn) as f32 * step - half);
    let node_of = |p: Vec2| {
        let i = ((p.x + half) / step).round().clamp(0.0, (cn - 1) as f32) as usize;
        let j = ((p.y + half) / step).round().clamp(0.0, (cn - 1) as f32) as usize;
        j * cn + i
    };
    let (s, t) = (node_of(from), node_of(to));
    let mut g = vec![u64::MAX; cn * cn];
    let mut parent = vec![usize::MAX; cn * cn];
    let mut heap = BinaryHeap::new();
    let hcost = |k: usize| ((node_pos(k) - node_pos(t)).length() * 1000.0) as u64;
    g[s] = 0;
    heap.push(Reverse((hcost(s), s)));
    while let Some(Reverse((_, v))) = heap.pop() {
        if v == t {
            let mut path = vec![node_pos(t)];
            let mut c = t;
            while parent[c] != usize::MAX {
                c = parent[c];
                path.push(node_pos(c));
            }
            path.reverse();
            return Some(path);
        }
        let (vi, vj) = ((v % cn) as isize, (v / cn) as isize);
        let pv = node_pos(v);
        let hv = h.height_at(pv);
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
            let (wi, wj) = (vi + di, vj + dj);
            if wi < 0 || wj < 0 || wi >= cn as isize || wj >= cn as isize {
                continue;
            }
            let w = wj as usize * cn + wi as usize;
            let pw = node_pos(w);
            let wet = water.is_water(pw) || water.is_water((pv + pw) * 0.5);
            if wet && !allow_water {
                continue;
            }
            let len = (pw - pv).length();
            let slope = if wet {
                0.0
            } else {
                (h.height_at(pw) - hv).abs() / len
            };
            let mut c = len * (1.0 + 40.0 * slope * slope);
            if wet {
                c *= 30.0; // a bridge
            }
            if slope > BUILDABLE_SLOPE {
                c *= 8.0;
            }
            let ng = g[v].saturating_add((c * 1000.0) as u64);
            if ng < g[w] {
                g[w] = ng;
                parent[w] = v;
                heap.push(Reverse((ng + hcost(w), w)));
            }
        }
        let _ = (vi, vj);
    }
    None
}

/// 2–3 outside connections; the first ends at a lowland hub within reach of a ≥ 1.5 m shore.
fn make_outside(
    h: &HeightGrid,
    water: &WaterField,
    wdist: &Grid<f32>,
    seed: u64,
    report: &mut WorldReport,
) -> Vec<OutsideConnection> {
    let mut rng = Pcg32::from_seed(derive_seed(seed, Domain::OutsideConnection, 0, 0));
    let half = h.half_extent_m();
    let deep_near = |p: Vec2| -> Option<f32> {
        // Distance to the nearest ≥ 1.5 m deep water along 16 rays (≤ SHORE_REACH_M).
        let mut best: Option<f32> = None;
        for q in 0..16 {
            let (sn, cs) = sin_cos(q as f32 * cb_core::math::TAU / 16.0);
            let dir = Vec2::new(cs, sn);
            let mut d = 0.0;
            while d <= SHORE_REACH_M {
                let x = p + dir * d;
                if water.depth_at(x, h) >= SHORE_DEPTH_M {
                    best = Some(best.map_or(d, |b: f32| b.min(d)));
                    break;
                }
                d += 4.0;
            }
        }
        best
    };
    // Hub: lowland point near deep water, closest to an edge (but ≥ 400 m inside).
    let mut hub: Option<(f32, Vec2, f32)> = None;
    for _ in 0..600 {
        let p = Vec2::new(
            rng.range_f32(-half + 400.0, half - 400.0),
            rng.range_f32(-half + 400.0, half - 400.0),
        );
        let (i, j) = h.nearest(p);
        if water.level(i, j).is_some()
            || h.slope_at(p) > 0.05
            || wdist.get(i, j) < 40.0
            || wdist.get(i, j) > 300.0
        {
            continue;
        }
        let Some(sd) = deep_near(p) else { continue };
        let edge_d = half - p.x.abs().max(p.y.abs());
        if hub.is_none_or(|b| edge_d < b.0) {
            hub = Some((edge_d, p, sd));
        }
    }
    let mut out = Vec::new();
    let Some((_, hub_p, shore_d)) = hub else {
        report.relaxed.push("shore_near_connection");
        return out;
    };
    report.shore_distance_m = shore_d;
    // Edges ordered by distance from the hub; the nearest gets the primary connection.
    let mut edges: Vec<(f32, u8)> = (0..4u8)
        .map(|e| {
            let d = match e {
                0 => half - hub_p.x,
                1 => hub_p.x + half,
                2 => half - hub_p.y,
                _ => hub_p.y + half,
            };
            (d, e)
        })
        .collect();
    edges.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let count = 2 + rng.below(2) as usize;
    // Dry routes first (edges by distance), then bridged ones if fewer than two connected.
    let mut used = [false; 4];
    for (pass, &(_, e)) in edges.iter().chain(edges.iter()).enumerate() {
        let allow_water = pass >= 4;
        if out.len() >= count || (allow_water && out.len() >= 2) || used[e as usize] {
            continue;
        }
        let k = out.len();
        // Best of 48 edge points by slope (dry).
        let mut best: Option<(f32, Vec2)> = None;
        for _ in 0..48 {
            let t = rng.range_f32(-half + 200.0, half - 200.0);
            let p = match e {
                0 => Vec2::new(half, t),
                1 => Vec2::new(-half, t),
                2 => Vec2::new(t, half),
                _ => Vec2::new(t, -half),
            };
            let inner = p - p.signum()
                * Vec2::new(
                    if e < 2 { 32.0 } else { 0.0 },
                    if e >= 2 { 32.0 } else { 0.0 },
                );
            if water.is_water(p) || water.is_water(inner) {
                continue;
            }
            let score = h.slope_at(inner) * 1000.0 + (p - hub_p).length() * 0.01;
            if best.is_none_or(|b| score < b.0) {
                best = Some((score, p));
            }
        }
        let Some((_, edge_p)) = best else { continue };
        let Some(mut path) =
            coarse_path(h, water, edge_p, hub_p, allow_water).map(|p| chaikin(&p, 2))
        else {
            continue;
        };
        used[e as usize] = true;
        if k > 0 {
            // Secondary connections end 400 m into the map.
            let mut acc = 0.0;
            let mut cut = path.len();
            for q in 1..path.len() {
                acc += (path[q] - path[q - 1]).length();
                if acc >= 400.0 {
                    cut = q + 1;
                    break;
                }
            }
            path.truncate(cut);
        }
        out.push(OutsideConnection { path });
    }
    if out.is_empty() {
        report.relaxed.push("outside_connections");
    }
    out
}

/// Chaikin corner cutting (endpoints kept).
fn chaikin(pts: &[Vec2], iterations: u32) -> Vec<Vec2> {
    let mut route = pts.to_vec();
    for _ in 0..iterations {
        if route.len() < 3 {
            break;
        }
        let mut sm = vec![route[0]];
        for w in route.windows(2) {
            sm.push(w[0] * 0.75 + w[1] * 0.25);
            sm.push(w[0] * 0.25 + w[1] * 0.75);
        }
        sm.push(route[route.len() - 1]);
        route = sm;
    }
    route
}

/// Forest density on the 8 m grid: noise patches, thinned on steep slopes, high ground and
/// near water.
fn forest_mask(h: &HeightGrid, wdist: &Grid<f32>, seed: u64) -> Grid<u8> {
    let step = (FOREST_SPACING_M / h.spacing_m()).round().max(1.0) as usize;
    let fnn = (h.n() - 1) / step + 1;
    let s = derive_seed(seed, Domain::Scatter, 0, 1);
    let mut data = vec![0u8; fnn * fnn];
    for_rows(&mut data, fnn, |j, row| {
        for (i, v) in row.iter_mut().enumerate() {
            let (hi, hj) = ((i * step).min(h.n() - 1), (j * step).min(h.n() - 1));
            let p = h.pos(hi, hj);
            let dens = smoothstep(0.02, 0.32, noise::fbm(p, 700.0, 4, s));
            let slope = h.sample_slope(hi, hj);
            let height = h.get(hi, hj);
            let f = dens
                * (1.0 - smoothstep(0.35, 0.6, slope))
                * (1.0 - smoothstep(130.0, 175.0, height))
                * smoothstep(12.0, 30.0, wdist.get(hi, hj));
            *v = (f * 255.0).round() as u8;
        }
    });
    Grid::from_vec(fnn, FOREST_SPACING_M, data)
        .unwrap_or_else(|| Grid::filled(fnn, FOREST_SPACING_M, 0))
}
