//! Scalar fields (ADR-0028, ALGORITHMS §19): 512 × 512 rasters over the map, sampled
//! bilinearly, updated hourly in 64 slices of 8 rows and double-buffered (each slice writes the
//! back buffer from the front one; the buffers swap after the last slice), so readers never see
//! a half-updated field.
//!
//! - Ground pollution (tick 200): `P ← blur3×3(0.97 P + sources) × (1 − 0.03 tree density)`.
//! - Noise (tick 300, recomputed): road line sources `L = 10 log10(V) + type − 10 log10(d/d0)`
//!   (sampled every 8 m as point emitters whose sum follows the line law) and point sources
//!   `L0 − 20 log10(d/d0)`, summed by energy.
//! - Land value (tick 400): weighted components smoothed with a half-life of one game day;
//!   cells beyond the city margin keep the terrain term only.

use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use cb_core::math::{exp, log10, powf};
use cb_core::time::TICKS_PER_GAME_HOUR;
use cb_procgen::building::civic::CivicKind;
use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::buildings::{Building, BuildingKind, Condition};
use crate::catalog::{Catalog, FieldsSpec, ZoneKind};
use crate::ids::Ids;
use crate::roads::RoadSegment;

/// Cells per side.
pub const N: usize = 512;
/// Slices per hourly update.
pub const SLICES: u64 = 64;
/// Rows per slice.
pub const ROWS_PER_SLICE: usize = N / SLICES as usize;
/// Ticks of the hourly updates in the game hour (phase table).
pub const POLLUTION_OFFSET: u64 = 200;
/// Noise update offset.
pub const NOISE_OFFSET: u64 = 300;
/// Land value update offset.
pub const LAND_VALUE_OFFSET: u64 = 400;
/// Spacing of road noise samples (m).
const NOISE_SAMPLE_M: f32 = 8.0;
/// Bucket size of the noise source index (m).
const BUCKET_M: f32 = 32.0;

/// One raster.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScalarField {
    half_m: f32,
    data: Vec<f32>,
}

impl ScalarField {
    /// A field over `[−half_m, half_m]²` filled with `v`.
    #[must_use]
    pub fn new(half_m: f32, v: f32) -> Self {
        Self {
            half_m,
            data: vec![v; N * N],
        }
    }

    /// Cell size (m).
    #[must_use]
    pub fn cell_m(&self) -> f32 {
        2.0 * self.half_m / N as f32
    }

    /// Value of cell `(i, j)` (column, row).
    #[must_use]
    pub fn get(&self, i: usize, j: usize) -> f32 {
        self.data[j.min(N - 1) * N + i.min(N - 1)]
    }

    fn set(&mut self, i: usize, j: usize, v: f32) {
        self.data[j * N + i] = v;
    }

    /// Centre of cell `(i, j)`.
    #[must_use]
    pub fn center(&self, i: usize, j: usize) -> Vec2 {
        let c = self.cell_m();
        Vec2::new(
            -self.half_m + (i as f32 + 0.5) * c,
            -self.half_m + (j as f32 + 0.5) * c,
        )
    }

    /// Cell containing `p` (clamped).
    #[must_use]
    pub fn cell_of(&self, p: Vec2) -> (usize, usize) {
        let c = self.cell_m();
        let f = |x: f32| (((x + self.half_m) / c).floor().max(0.0) as usize).min(N - 1);
        (f(p.x), f(p.y))
    }

    /// Bilinear sample at a continuous position.
    #[must_use]
    pub fn sample(&self, p: Vec2) -> f32 {
        let c = self.cell_m();
        let fx = ((p.x + self.half_m) / c - 0.5).clamp(0.0, (N - 1) as f32);
        let fy = ((p.y + self.half_m) / c - 0.5).clamp(0.0, (N - 1) as f32);
        let (i, j) = (fx.floor() as usize, fy.floor() as usize);
        let (tx, ty) = (fx - i as f32, fy - j as f32);
        let a = self.get(i, j) * (1.0 - tx) + self.get(i + 1, j) * tx;
        let b = self.get(i, j + 1) * (1.0 - tx) + self.get(i + 1, j + 1) * tx;
        a * (1.0 - ty) + b * ty
    }

    /// Raw values, row-major.
    #[must_use]
    pub fn data(&self) -> &[f32] {
        &self.data
    }
}

/// A double-buffered field: readers see `front`; slices write `back`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DoubleField {
    front: ScalarField,
    #[serde(skip, default = "empty_field")]
    back: ScalarField,
}

fn empty_field() -> ScalarField {
    ScalarField {
        half_m: 0.0,
        data: Vec::new(),
    }
}

impl DoubleField {
    /// Both buffers filled with `v`.
    #[must_use]
    pub fn new(half_m: f32, v: f32) -> Self {
        Self {
            front: ScalarField::new(half_m, v),
            back: ScalarField::new(half_m, v),
        }
    }

    /// The readable buffer.
    #[must_use]
    pub fn front(&self) -> &ScalarField {
        &self.front
    }

    fn ensure_back(&mut self) {
        if self.back.data.len() != self.front.data.len() {
            self.back = self.front.clone();
        }
    }

    /// Like [`Self::update_slice`] but evaluates `f` on every other cell and fills 2 × 2 blocks
    /// (fields recomputed from sources at 16 m resolution).
    pub fn update_slice_coarse(&mut self, k: u64, f: impl Fn(&ScalarField, usize, usize) -> f32) {
        self.ensure_back();
        let r0 = (k as usize) * ROWS_PER_SLICE;
        for j in (r0..(r0 + ROWS_PER_SLICE).min(N)).step_by(2) {
            for i in (0..N).step_by(2) {
                let v = f(&self.front, i, j);
                for (di, dj) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    if i + di < N && j + dj < N {
                        self.back.set(i + di, j + dj, v);
                    }
                }
            }
        }
        if k + 1 == SLICES {
            std::mem::swap(&mut self.front, &mut self.back);
        }
    }

    /// Writes rows of slice `k` with `f(front, i, j)`; swaps after the last slice.
    pub fn update_slice(&mut self, k: u64, f: impl Fn(&ScalarField, usize, usize) -> f32) {
        self.ensure_back();
        let r0 = (k as usize) * ROWS_PER_SLICE;
        for j in r0..(r0 + ROWS_PER_SLICE).min(N) {
            for i in 0..N {
                let v = f(&self.front, i, j);
                self.back.set(i, j, v);
            }
        }
        if k + 1 == SLICES {
            std::mem::swap(&mut self.front, &mut self.back);
        }
    }
}

/// One pollution step at a cell: decay, emission, 3 × 3 blur, tree absorption.
#[must_use]
pub fn pollution_step(
    front: &ScalarField,
    sources: &[f32],
    trees: &[f32],
    i: usize,
    j: usize,
    decay: f32,
    absorption: f32,
) -> f32 {
    let mut sum = 0.0;
    for dj in -1i32..=1 {
        for di in -1i32..=1 {
            let ii = (i as i32 + di).clamp(0, N as i32 - 1) as usize;
            let jj = (j as i32 + dj).clamp(0, N as i32 - 1) as usize;
            let s = sources.get(jj * N + ii).copied().unwrap_or(0.0);
            sum += decay * front.get(ii, jj) + s;
        }
    }
    let t = trees.get(j * N + i).copied().unwrap_or(0.0);
    (sum / 9.0) * (1.0 - absorption * t)
}

/// Level of a line source at distance `d` (dB).
#[must_use]
pub fn line_db(l0: f32, d: f32, d0: f32) -> f32 {
    l0 - 10.0 * log10(d.max(0.1) / d0)
}

/// Level of a point source at distance `d` (dB).
#[must_use]
pub fn point_db(l0: f32, d: f32, d0: f32) -> f32 {
    l0 - 20.0 * log10(d.max(0.1) / d0)
}

/// Energy of a level (relative).
#[must_use]
pub fn energy(db: f32) -> f32 {
    powf(10.0, db / 10.0)
}

/// Level of a summed energy (dB; 0 for silence).
#[must_use]
pub fn level(energy: f32) -> f32 {
    if energy <= 1.0 {
        0.0
    } else {
        10.0 * log10(energy)
    }
}

/// Emission energy of one road sample so that samples every `spacing` m sum to the line law
/// (`Σ 1/(d² + (k s)²) ≈ π / (s d)`).
#[must_use]
pub fn road_sample_energy(line_l0: f32, spacing: f32, d0: f32) -> f32 {
    energy(line_l0) * spacing / (core::f32::consts::PI * d0)
}

/// A noise emitter (energy at `d0` for point law).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Emitter {
    pos: Vec2,
    energy: f32,
}

/// Bucketed emitters: individual emitters per 32 m bucket plus each bucket's total energy at
/// its energy-weighted centroid, used for buckets beyond the near ring.
#[derive(Clone, Debug, Default, PartialEq)]
struct NoiseIndex {
    origin: Vec2,
    nx: usize,
    ny: usize,
    buckets: Vec<Vec<Emitter>>,
    totals: Vec<Emitter>,
}

/// Buckets within this ring (Chebyshev) are summed emitter by emitter.
const NEAR_RING: i64 = 2;

impl NoiseIndex {
    fn build(emitters: &[Emitter], lo: Vec2, hi: Vec2) -> Self {
        let nx = (((hi.x - lo.x) / BUCKET_M).ceil() as usize).max(1);
        let ny = (((hi.y - lo.y) / BUCKET_M).ceil() as usize).max(1);
        let mut buckets = vec![Vec::new(); nx * ny];
        for e in emitters {
            let bx = ((e.pos.x - lo.x) / BUCKET_M).floor();
            let by = ((e.pos.y - lo.y) / BUCKET_M).floor();
            if bx < 0.0 || by < 0.0 {
                continue;
            }
            let (bx, by) = (bx as usize, by as usize);
            if bx < nx && by < ny {
                buckets[by * nx + bx].push(*e);
            }
        }
        let totals = buckets
            .iter()
            .map(|b: &Vec<Emitter>| {
                let energy: f32 = b.iter().map(|e| e.energy).sum();
                let pos = if energy > 0.0 {
                    b.iter().map(|e| e.pos * e.energy).sum::<Vec2>() / energy
                } else {
                    Vec2::ZERO
                };
                Emitter { pos, energy }
            })
            .collect();
        Self {
            origin: lo,
            nx,
            ny,
            buckets,
            totals,
        }
    }

    fn energy_at(&self, p: Vec2, cutoff: f32, d0: f32) -> f32 {
        if self.buckets.is_empty() {
            return 0.0;
        }
        let r = (cutoff / BUCKET_M).ceil() as i64;
        let bx = ((p.x - self.origin.x) / BUCKET_M).floor() as i64;
        let by = ((p.y - self.origin.y) / BUCKET_M).floor() as i64;
        let mut sum = 0.0;
        let c2 = cutoff * cutoff;
        let floor2 = 0.25 * d0 * d0;
        for y in (by - r).max(0)..=(by + r).min(self.ny as i64 - 1) {
            for x in (bx - r).max(0)..=(bx + r).min(self.nx as i64 - 1) {
                let k = y as usize * self.nx + x as usize;
                if (x - bx).abs().max((y - by).abs()) <= NEAR_RING {
                    for e in &self.buckets[k] {
                        let d2 = e.pos.distance_squared(p);
                        if d2 <= c2 {
                            sum += e.energy * d0 * d0 / d2.max(floor2);
                        }
                    }
                } else {
                    let t = self.totals[k];
                    if t.energy > 0.0 {
                        let d2 = t.pos.distance_squared(p);
                        if d2 <= c2 {
                            sum += t.energy * d0 * d0 / d2.max(floor2);
                        }
                    }
                }
            }
        }
        sum
    }

    /// Emitters of the 3 × 3 buckets around `p` (near queries).
    fn near(&self, p: Vec2) -> impl Iterator<Item = &Emitter> + '_ {
        let bx = ((p.x - self.origin.x) / BUCKET_M).floor() as i64;
        let by = ((p.y - self.origin.y) / BUCKET_M).floor() as i64;
        let (nx, ny) = (self.nx as i64, self.ny as i64);
        (by - 1..=by + 1)
            .filter(move |y| *y >= 0 && *y < ny)
            .flat_map(move |y| {
                (bx - 1..=bx + 1)
                    .filter(move |x| *x >= 0 && *x < nx)
                    .map(move |x| (y * nx + x) as usize)
            })
            .flat_map(move |k| self.buckets.get(k).into_iter().flatten())
    }
}

/// The fields (pollution and land value saved; noise recomputed hourly).
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fields {
    pollution: DoubleField,
    #[serde(skip, default = "empty_double")]
    noise: DoubleField,
    land_value: DoubleField,
    #[serde(skip)]
    sources: Vec<f32>,
    #[serde(skip)]
    trees: Vec<f32>,
    #[serde(skip)]
    noise_index: NoiseIndex,
    /// Rows and columns within the city margin (lo, hi inclusive).
    #[serde(skip)]
    city: Option<(usize, usize, usize, usize)>,
    #[serde(skip)]
    lv_inputs: LvInputs,
}

fn empty_double() -> DoubleField {
    DoubleField {
        front: empty_field(),
        back: empty_field(),
    }
}

/// Per-hour land value inputs.
#[derive(Clone, Debug, Default, PartialEq)]
struct LvInputs {
    /// Abandoned or ruined building positions.
    abandoned: Vec<Vec2>,
    /// Road samples with their road-type score (as the emitter energy).
    roads: NoiseIndex,
    /// Decoration bonuses per cell (row-major, only within the city).
    parks: Vec<f32>,
}

impl Default for Fields {
    fn default() -> Self {
        Self::new(2048.0)
    }
}

impl Fields {
    /// Empty fields over a map of half extent `half_m`.
    #[must_use]
    pub fn new(half_m: f32) -> Self {
        Self {
            pollution: DoubleField::new(half_m, 0.0),
            noise: DoubleField::new(half_m, 0.0),
            land_value: DoubleField::new(half_m, 0.5),
            sources: Vec::new(),
            trees: Vec::new(),
            noise_index: NoiseIndex::default(),
            city: None,
            lv_inputs: LvInputs::default(),
        }
    }

    /// Ground pollution.
    #[must_use]
    pub fn pollution(&self) -> &ScalarField {
        self.pollution.front()
    }

    /// Noise (dB).
    #[must_use]
    pub fn noise(&self) -> &ScalarField {
        self.noise.front()
    }

    /// Land value 0–1.
    #[must_use]
    pub fn land_value(&self) -> &ScalarField {
        self.land_value.front()
    }

    fn ensure(&mut self, half_m: f32) {
        if (self.pollution.front.half_m - half_m).abs() > 1e-3 {
            *self = Self::new(half_m);
        }
        if self.noise.front.data.is_empty() {
            self.noise = DoubleField::new(half_m, 0.0);
        }
    }
}

fn half_extent(world: &World) -> f32 {
    cb_network::validation::Ground::half_extent(&crate::terrain::ground(world))
}

/// Buildings as `(kind, condition, centroid, level)`.
fn building_points(world: &World) -> Vec<(BuildingKind, Condition, Vec2, u8)> {
    let Some(ids) = world.get_resource::<Ids>() else {
        return Vec::new();
    };
    ids.buildings
        .iter()
        .filter_map(|(_, e)| {
            let b = world.get::<Building>(e)?;
            let c = b
                .outlines()
                .first()
                .map(|o| cb_geometry::polygon::centroid(o))?;
            Some((b.kind(), b.condition(), c, b.level()))
        })
        .collect()
}

/// Road segments as sampled points with their kind.
fn road_samples(world: &World, step: f32) -> Vec<(Vec2, cb_network::RoadKind)> {
    let Some(ids) = world.get_resource::<Ids>() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (_, e) in ids.segments.iter() {
        let Some(s) = world.get::<RoadSegment>(e) else {
            continue;
        };
        let line = &s.geom().centerline;
        let len = line.length();
        let n = ((len / step).ceil() as usize).max(1);
        for k in 0..n {
            let t = (k as f32 + 0.5) / n as f32 * len;
            out.push((line.point_at(t), s.kind()));
        }
    }
    out
}

fn city_bounds(
    world: &World,
    f: &ScalarField,
    margin: f32,
) -> Option<(usize, usize, usize, usize)> {
    let pts = road_samples(world, 64.0);
    let lo = pts.iter().map(|p| p.0).reduce(Vec2::min)? - Vec2::splat(margin);
    let hi = pts.iter().map(|p| p.0).reduce(Vec2::max)? + Vec2::splat(margin);
    let (i0, j0) = f.cell_of(lo);
    let (i1, j1) = f.cell_of(hi);
    Some((i0, j0, i1, j1))
}

fn pollution_sources(world: &World, spec: &FieldsSpec, f: &ScalarField) -> Vec<f32> {
    let mut s = vec![0.0f32; N * N];
    for (kind, cond, p, level) in building_points(world) {
        if !matches!(cond, Condition::Active | Condition::Declining) {
            continue;
        }
        let e = match kind {
            BuildingKind::Zoned(ZoneKind::Industrial) => {
                spec.pollution.industrial_per_level * f32::from(level)
            }
            BuildingKind::Civic(CivicKind::CoalPlant) => spec.pollution.coal_plant,
            BuildingKind::Civic(CivicKind::DieselGenerator) => spec.pollution.diesel_generator,
            BuildingKind::Civic(CivicKind::Landfill) => spec.pollution.landfill,
            _ => 0.0,
        };
        if e > 0.0 {
            let (i, j) = f.cell_of(p);
            s[j * N + i] += e;
        }
    }
    s
}

fn tree_density(world: &World, f: &ScalarField) -> Vec<f32> {
    let Some(t) = world.get_resource::<crate::terrain::TerrainRes>() else {
        return Vec::new();
    };
    let forest = t.terrain().forest();
    let mut out = vec![0.0f32; N * N];
    for j in 0..N {
        for i in 0..N {
            let p = f.center(i, j);
            let gi = ((p.x + forest.half_extent_m()) / forest.spacing_m()).round();
            let gj = ((p.y + forest.half_extent_m()) / forest.spacing_m()).round();
            if gi >= 0.0 && gj >= 0.0 {
                out[j * N + i] = f32::from(forest.get_clamped(gi as isize, gj as isize)) / 255.0;
            }
        }
    }
    out
}

fn noise_emitters(world: &World, spec: &FieldsSpec) -> Vec<Emitter> {
    let n = &spec.noise;
    let mut out: Vec<Emitter> = road_samples(world, NOISE_SAMPLE_M)
        .into_iter()
        .map(|(p, k)| {
            let l0 = 10.0 * log10(n.volume_stub_per_sim_h.get(k).max(1.0)) + n.road_db.get(k);
            Emitter {
                pos: p,
                energy: road_sample_energy(l0, NOISE_SAMPLE_M, n.d0_m),
            }
        })
        .collect();
    for (kind, cond, p, level) in building_points(world) {
        let db = match (kind, cond) {
            (_, Condition::Constructing { .. }) => Some(n.construction_db),
            (_, Condition::Abandoned | Condition::Ruined) => None,
            (BuildingKind::Zoned(ZoneKind::Industrial), _) => {
                Some(n.industrial_db + 2.0 * f32::from(level.saturating_sub(1)))
            }
            (BuildingKind::Civic(CivicKind::CoalPlant | CivicKind::DieselGenerator), _) => {
                Some(n.power_plant_db)
            }
            _ => None,
        };
        if let Some(db) = db {
            out.push(Emitter {
                pos: p,
                energy: energy(db),
            });
        }
    }
    out
}

fn lv_inputs(
    world: &World,
    f: &ScalarField,
    city: Option<(usize, usize, usize, usize)>,
) -> LvInputs {
    let abandoned = building_points(world)
        .into_iter()
        .filter(|(_, c, _, _)| matches!(c, Condition::Abandoned | Condition::Ruined))
        .map(|(_, _, p, _)| p)
        .collect();
    let samples: Vec<Emitter> = road_samples(world, 16.0)
        .into_iter()
        .map(|(p, k)| Emitter {
            pos: p,
            energy: match k {
                cb_network::RoadKind::Dirt => 0.3,
                cb_network::RoadKind::Street | cb_network::RoadKind::OneWayStreet => 0.6,
                _ => 0.4,
            },
        })
        .collect();
    let half = Vec2::splat(f.half_m);
    let roads = NoiseIndex::build(&samples, -half, half);
    let mut parks = vec![0.0f32; N * N];
    let has_props = world
        .get_resource::<Ids>()
        .is_some_and(|ids| !ids.props.is_empty());
    if has_props && let Some((i0, j0, i1, j1)) = city {
        for j in j0..=j1 {
            for i in i0..=i1 {
                parks[j * N + i] = crate::props::land_value_bonus_at(world, f.center(i, j));
            }
        }
    }
    LvInputs {
        abandoned,
        roads,
        parks,
    }
}

/// `TickSet::SubRate`: the sliced field updates.
pub(crate) fn tick(world: &mut World) {
    let now = world
        .get_resource::<crate::SimClock>()
        .map_or(0, |c| c.tick().0);
    let t = now % TICKS_PER_GAME_HOUR;
    let in_window = |o: u64| (o..o + SLICES).contains(&t);
    if !(in_window(POLLUTION_OFFSET) || in_window(NOISE_OFFSET) || in_window(LAND_VALUE_OFFSET)) {
        return;
    }
    let Some(spec) = world.get_resource::<Catalog>().map(|c| *c.fields()) else {
        return;
    };
    let half = half_extent(world);
    let Some(mut fields) = world.remove_resource::<Fields>() else {
        return;
    };
    fields.ensure(half);
    if in_window(POLLUTION_OFFSET) {
        let k = t - POLLUTION_OFFSET;
        if k == 0 {
            fields.sources = pollution_sources(world, &spec, fields.pollution.front());
            if fields.trees.is_empty() {
                fields.trees = tree_density(world, fields.pollution.front());
            }
        }
        let (sources, trees) = (&fields.sources, &fields.trees);
        let p = spec.pollution;
        fields.pollution.update_slice(k, |front, i, j| {
            pollution_step(front, sources, trees, i, j, p.decay, p.tree_absorption)
        });
    }
    if in_window(NOISE_OFFSET) {
        let k = t - NOISE_OFFSET;
        if k == 0 {
            fields.city = city_bounds(world, fields.noise.front(), spec.land_value.city_margin_m);
            let em = noise_emitters(world, &spec);
            let half_v = Vec2::splat(half);
            fields.noise_index = NoiseIndex::build(&em, -half_v, half_v);
        }
        let (index, city) = (&fields.noise_index, fields.city);
        let n = spec.noise;
        fields
            .noise
            .update_slice_coarse(k, |front, i, j| match city {
                Some((i0, j0, i1, j1)) if (i0..=i1).contains(&i) && (j0..=j1).contains(&j) => {
                    level(index.energy_at(front.center(i, j), n.cutoff_m, n.d0_m))
                }
                _ => 0.0,
            });
    }
    if in_window(LAND_VALUE_OFFSET) {
        let k = t - LAND_VALUE_OFFSET;
        if k == 0 {
            fields.city = city_bounds(
                world,
                fields.land_value.front(),
                spec.land_value.city_margin_m,
            );
            fields.lv_inputs = lv_inputs(world, fields.land_value.front(), fields.city);
        }
        let terrain = world.get_resource::<crate::terrain::TerrainRes>();
        let lv = spec.land_value;
        let n = spec.noise;
        let alpha = 1.0 - exp(-core::f32::consts::LN_2 / 24.0);
        let Fields {
            pollution,
            noise,
            land_value,
            lv_inputs: inputs,
            city,
            ..
        } = &mut fields;
        let (pollution, noise, city) = (pollution.front(), noise.front(), *city);
        let sat = spec.pollution.saturation;
        land_value.update_slice(k, |front, i, j| {
            let p = front.center(i, j);
            let terrain_term = terrain.map_or(0.5, |t| {
                let near_water = (1.0 - t.terrain().water_distance_at(p) / 150.0).clamp(0.0, 1.0);
                let slope = t.terrain().height().slope_at(p);
                (0.5 + 0.4 * near_water - 3.0 * slope).clamp(0.0, 1.0)
            });
            let in_city = city
                .is_some_and(|(i0, j0, i1, j1)| (i0..=i1).contains(&i) && (j0..=j1).contains(&j));
            let target = if in_city {
                let road = inputs
                    .roads
                    .near(p)
                    .filter(|e| e.pos.distance_squared(p) < 40.0 * 40.0)
                    .map(|e| e.energy)
                    .fold(0.0f32, f32::max);
                let abandoned = inputs
                    .abandoned
                    .iter()
                    .filter(|q| q.distance(p) < lv.abandoned_radius_m)
                    .count()
                    .min(3) as f32
                    / 3.0;
                let parks = inputs.parks.get(j * N + i).copied().unwrap_or(0.0).min(1.0);
                let poll = (pollution.get(i, j) / sat).clamp(0.0, 1.0);
                let loud =
                    ((noise.get(i, j) - n.quiet_db) / (n.loud_db - n.quiet_db)).clamp(0.0, 1.0);
                lv.base
                    + lv.services * crate::citizens::happiness::SERVICES_STUB
                    + lv.parks * parks
                    + lv.terrain * terrain_term
                    + lv.road * road
                    + lv.pollution * poll
                    + lv.noise * loud
                    + lv.crime * crate::citizens::happiness::CRIME_STUB
                    + lv.abandoned * abandoned
            } else {
                lv.base + lv.terrain * terrain_term
            };
            let cur = front.get(i, j);
            cur + alpha * (target.clamp(0.0, 1.0) - cur)
        });
    }
    world.insert_resource(fields);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pollution_decays_0_97_per_hour() {
        let mut f = DoubleField::new(2048.0, 1.0);
        for k in 0..SLICES {
            f.update_slice(k, |front, i, j| {
                pollution_step(front, &[], &[], i, j, 0.97, 0.03)
            });
        }
        assert!(f.front().data().iter().all(|v| (v - 0.97).abs() < 1e-6));
        // Trees absorb on top of the decay.
        let trees = vec![1.0f32; N * N];
        let mut f = DoubleField::new(2048.0, 1.0);
        for k in 0..SLICES {
            f.update_slice(k, |front, i, j| {
                pollution_step(front, &[], &trees, i, j, 0.97, 0.03)
            });
        }
        assert!((f.front().get(10, 10) - 0.97 * 0.97).abs() < 1e-6);
        // A source spreads to its neighbours.
        let mut src = vec![0.0f32; N * N];
        src[100 * N + 100] = 9.0;
        let mut f = DoubleField::new(2048.0, 0.0);
        for k in 0..SLICES {
            f.update_slice(k, |front, i, j| {
                pollution_step(front, &src, &[], i, j, 0.97, 0.03)
            });
        }
        assert!((f.front().get(101, 100) - 1.0).abs() < 1e-6);
        assert_eq!(f.front().get(102, 100), 0.0);
    }

    #[test]
    fn noise_line_and_point_attenuation() {
        let d0 = 10.0;
        // Doubling the distance: −3 dB for a line, −6 dB for a point.
        assert!((line_db(60.0, 20.0, d0) - line_db(60.0, 40.0, d0) - 3.0103).abs() < 1e-3);
        assert!((point_db(60.0, 20.0, d0) - point_db(60.0, 40.0, d0) - 6.0206).abs() < 1e-3);
        // Two equal sources: +3 dB.
        assert!((level(2.0 * energy(50.0)) - 53.0103).abs() < 1e-3);
        // Road samples every 8 m reproduce the line law within 1 dB from 20 to 100 m.
        let l0 = 10.0 * log10(300.0);
        let e = road_sample_energy(l0, NOISE_SAMPLE_M, d0);
        let em: Vec<Emitter> = (-200..=200)
            .map(|k| Emitter {
                pos: Vec2::new(k as f32 * NOISE_SAMPLE_M, 0.0),
                energy: e,
            })
            .collect();
        let idx = NoiseIndex::build(&em, Vec2::splat(-2048.0), Vec2::splat(2048.0));
        for d in [20.0f32, 40.0, 100.0] {
            let got = level(idx.energy_at(Vec2::new(4.0, d), 1600.0, d0));
            let want = line_db(l0, d, d0);
            assert!((got - want).abs() < 1.0, "d {d}: {got} vs {want}");
        }
    }

    #[test]
    fn fields_double_buffer_no_seams() {
        // While slices run, readers see the old field everywhere; after the last slice, the
        // new one everywhere — never a mix.
        let mut f = DoubleField::new(2048.0, 1.0);
        for k in 0..SLICES - 1 {
            f.update_slice(k, |_, _, _| 2.0);
            assert!(f.front().data().iter().all(|v| *v == 1.0), "slice {k}");
        }
        f.update_slice(SLICES - 1, |_, _, _| 2.0);
        assert!(f.front().data().iter().all(|v| *v == 2.0));
        // Sampling is continuous across rows written by different slices.
        let mut g = DoubleField::new(2048.0, 0.0);
        for k in 0..SLICES {
            g.update_slice(k, |front, i, _| front.center(i, 0).x);
        }
        let a = g.front().sample(Vec2::new(10.0, 63.9));
        let b = g.front().sample(Vec2::new(10.0, 64.1));
        assert!((a - b).abs() < 1e-3, "{a} {b}");
    }
}
