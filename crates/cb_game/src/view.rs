//! View sync (ARCHITECTURE §6): visuals derived from sim state, rebuilt incrementally from the
//! sim's change messages — terrain chunks (distance LOD), water, road segments and junctions,
//! buildings with yards, scattered trees and rocks. Visual entities are separate from sim
//! entities and keyed by stable ID (ADR-0005).
//!
//! M2 keeps one render entity per road segment, junction and building (≈ 1 000 entities for the
//! 500-house benchmark); per-chunk merging and the slot buffer follow when the benchmark needs it
//! (PERFORMANCE §4).

use std::collections::BTreeMap;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::VisibilityRange;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::tasks::futures::check_ready;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use cb_core::ids::{BuildingId, NodeId, SegmentId};
use cb_core::rng::Pcg32;
use cb_geometry::MeshData;
use cb_network::control::ControlMode;
use cb_procgen::junction_mesh::junction_mesh;
use cb_procgen::markings_mesh::{self, Lamp};
use cb_procgen::paint::Painter;
use cb_procgen::road_mesh::{RoadMeshParams, segment_mesh};
use cb_procgen::vegetation::{self, TreeKind};
use cb_procgen::yard::{self, YardLot};
use cb_sim::buildings::{Building, BuildingChanged};
use cb_sim::ids::Ids;
use cb_sim::roads::{NetworkChanged, RoadNode, RoadSegment};
use cb_sim::terrain::{TerrainChanged, TerrainRes};
use cb_sim::zoning::{Plot, ZoningChanged};
use cb_terrain::chunk_mesh::{self, CHUNK_CELLS};
use cb_terrain::scatter::{self, PropKind};

use crate::camera::CameraRig;
use crate::render::{WorldMaterials, world_mesh};

/// Terrain LOD by distance from the camera eye to the chunk centre (m).
pub const TERRAIN_LOD_M: [f32; 3] = [450.0, 1000.0, 2000.0];
/// Scatter radius with full trees (m) and with far trees (m).
pub const SCATTER_NEAR_M: f32 = 350.0;
/// Far scatter radius (m).
pub const SCATTER_FAR_M: f32 = 1300.0;
/// Eye movement that re-evaluates terrain chunk LODs (m).
pub const TERRAIN_EYE_STEP_M: f32 = 8.0;
/// Building chunk size (m): buildings merge into one mesh per chunk and LOD.
pub const BUILDING_CHUNK_M: f32 = 160.0;
/// Camera distance where building chunks switch to LOD1 (m).
pub const BUILDING_LOD1_M: f32 = 600.0;
/// Chunk meshing jobs in flight at most, per kind (terrain, scatter). Meshing runs on the
/// async compute pool from an `Arc<Terrain>` snapshot; the main thread only uploads results.
pub const MAX_JOBS_IN_FLIGHT: usize = 8;

/// Far terrain (LOD ≥ [`MERGE_FROM_LOD`]) is drawn as one entity per block of
/// `SUPER × SUPER` chunks, which cuts ~850 of 1 024 terrain entities (PERFORMANCE §4).
pub const SUPER: usize = 4;
/// Lowest LOD whose chunks are merged into super-chunks.
pub const MERGE_FROM_LOD: u32 = 2;
/// Super-chunk jobs in flight at most.
pub const MAX_SUPER_JOBS: usize = 2;

/// A merged far-terrain block: ground entity, optional water entity, LOD.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct SuperSlot {
    merged: Option<(Entity, Option<Entity>, u32)>,
}

/// A finished terrain chunk: ground mesh and optional water mesh at `lod`.
struct TerrainJob {
    lod: u32,
    ground: Mesh,
    water: Option<Mesh>,
}

/// A finished scatter chunk (`None` when the chunk has nothing to scatter).
struct ScatterJob {
    level: u8,
    mesh: Option<Mesh>,
}

/// Per-chunk visual state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct ChunkSlot {
    terrain: Option<(Entity, u32)>,
    water: Option<Entity>,
    /// Scatter entity (none for empty chunks) and its level (1 far, 2 near).
    scatter: Option<(Option<Entity>, u8)>,
    dirty: bool,
    /// Scatter must be regenerated (plots or terrain changed).
    scatter_stale: bool,
}

/// Plot polygons with bounds (scatter exclusion), shared with scatter jobs.
type PlotShapes = Arc<Vec<(Vec2, Vec2, Vec<Vec2>)>>;

/// Extra areas without natural scatter (debug scenes that place meshes outside the sim).
#[derive(Resource, Default)]
pub struct ScatterExclusion(pub Vec<Vec<Vec2>>);

/// Visual registries.
#[derive(Resource, Default)]
pub struct ViewState {
    chunks: Vec<ChunkSlot>,
    per_side: usize,
    segments: BTreeMap<SegmentId, Entity>,
    nodes: BTreeMap<NodeId, Entity>,
    buildings: BTreeMap<BuildingId, BuildingVisual>,
    bchunks: BTreeMap<(i32, i32), BuildingChunk>,
    bchunk_jobs: BTreeMap<(i32, i32), Task<BuildingChunkJob>>,
    props: BTreeMap<cb_core::ids::PropId, Entity>,
    scatter_dirty: bool,
    /// Eye position the cached chunk distances were computed for.
    terrain_eye: Option<Vec2>,
    /// Cached eye distance per terrain chunk.
    terrain_dist: Vec<f32>,
    /// The last terrain pass found nothing to do.
    terrain_settled: bool,
    terrain_jobs: BTreeMap<usize, Task<TerrainJob>>,
    supers: Vec<SuperSlot>,
    super_jobs: BTreeMap<usize, Task<TerrainJob>>,
    scatter_jobs: BTreeMap<usize, Task<ScatterJob>>,
    plot_shapes: Option<PlotShapes>,
}

/// One building's visual: its chunk, parameter slot, scaffold while under construction.
#[derive(Debug, Clone, Copy)]
struct BuildingVisual {
    chunk: (i32, i32),
    slot: u16,
    scaffold: Option<Entity>,
    /// Owns a civic parcel (excluded from natural scatter).
    civic: bool,
}

/// Buildings merged into one LOD0 and one LOD1 mesh per square chunk (PERFORMANCE §10).
#[derive(Debug, Default)]
struct BuildingChunk {
    members: std::collections::BTreeSet<BuildingId>,
    lod0: Option<Entity>,
    lod1: Option<Entity>,
    dirty: bool,
}

/// A finished chunk mesh job (`None`: the chunk is empty).
struct BuildingChunkJob {
    meshes: Option<(Mesh, Mesh)>,
}

/// View counters (tests and the benchmark report).
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct ViewStats {
    /// Building meshes generated since start.
    pub building_meshes: u64,
}

impl ViewState {
    /// Parameter slot of a building's visual.
    #[must_use]
    pub fn building_slot(&self, id: BuildingId) -> Option<u16> {
        self.buildings.get(&id).map(|v| v.slot)
    }
    /// Whether a building currently shows a scaffold.
    #[must_use]
    pub fn has_scaffold(&self, id: BuildingId) -> bool {
        self.buildings
            .get(&id)
            .is_some_and(|v| v.scaffold.is_some())
    }
    /// Live building visuals.
    #[must_use]
    pub fn building_count(&self) -> usize {
        self.buildings.len()
    }
    /// Terrain entities (individual chunks + merged far blocks).
    #[must_use]
    pub fn terrain_chunks(&self) -> usize {
        self.chunks.iter().filter(|c| c.terrain.is_some()).count()
            + self.supers.iter().filter(|s| s.merged.is_some()).count()
    }
    /// Whether every visible chunk is up to date (screenshot settling).
    #[must_use]
    pub fn settled(&self) -> bool {
        self.chunks.iter().all(|c| !c.dirty)
    }
}

/// View plugin.
pub struct ViewPlugin;

/// The steps of `GameSet::ViewSync`, in order (the benchmark times each one).
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ViewStep {
    /// Road segments, junctions, bridges, decorations, signal heads.
    Network,
    /// Houses and yards.
    Buildings,
    /// Zoning overlay invalidation.
    Zoning,
    /// Terrain and water chunks (off-thread meshing, hand-over).
    Terrain,
    /// Natural scatter (trees, rocks).
    Scatter,
}

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ViewState>()
            .init_resource::<ViewStats>()
            .init_resource::<ViewBacklog>()
            .configure_sets(
                Update,
                (
                    ViewStep::Network,
                    ViewStep::Buildings,
                    ViewStep::Zoning,
                    ViewStep::Terrain,
                    ViewStep::Scatter,
                )
                    .chain()
                    .in_set(crate::app::GameSet::ViewSync),
            )
            .add_systems(
                Update,
                (
                    sync_network.in_set(ViewStep::Network),
                    (sync_buildings, sync_props)
                        .chain()
                        .in_set(ViewStep::Buildings),
                    sync_zoning.in_set(ViewStep::Zoning),
                    sync_terrain.in_set(ViewStep::Terrain),
                    sync_scatter.in_set(ViewStep::Scatter),
                ),
            )
            .add_systems(
                Update,
                update_signal_lamps.in_set(crate::app::GameSet::Presentation),
            );
    }
}

/// Outstanding chunk work (for screenshot settling).
#[derive(Resource, Default, Debug)]
pub struct ViewBacklog(pub usize);

// ---------- terrain ----------

const GRASS: [[u8; 3]; 3] = [[0x8D, 0xB0, 0x6A], [0x9C, 0xBB, 0x72], [0x7F, 0xA3, 0x5E]];
const DIRT: [u8; 3] = [0xB3, 0x90, 0x66];
const ROCK: [u8; 3] = [0x9A, 0x95, 0x90];
const SAND: [u8; 3] = [0xE2, 0xD1, 0xA6];
const FOREST: [u8; 3] = [0x6E, 0x8B, 0x54];

fn lin(c: [u8; 3]) -> Vec3 {
    let f = |v: u8| Color::srgb_u8(v, v, v).to_linear().red;
    Vec3::new(f(c[0]), f(c[1]), f(c[2]))
}

/// Terrain vertex colour from material weights `[dirt, rock, sand, forest]` (linear RGB), with
/// a grass variation field and gentle height banding (ART_DIRECTION §2 posterization).
fn terrain_color(w: [u8; 4], p: Vec3) -> [f32; 4] {
    let wf = |k: usize| f32::from(w[k]) / 255.0;
    let (d, r, s, f) = (wf(0), wf(1), wf(2), wf(3));
    let g = (1.0 - d - r - s - f).max(0.0);
    let q = Vec2::new(p.x, p.z);
    let n = (cb_terrain::noise::fbm(q, 90.0, 3, 0x6A55) * 0.5 + 0.5).clamp(0.0, 1.0);
    let grass = if n < 0.5 {
        lin(GRASS[2]).lerp(lin(GRASS[0]), n * 2.0)
    } else {
        lin(GRASS[0]).lerp(lin(GRASS[1]), (n - 0.5) * 2.0)
    };
    let c = grass * g + lin(DIRT) * d + lin(ROCK) * r + lin(SAND) * s + lin(FOREST) * f;
    // Gentle height bands (4–6 visible over typical relief), softened by the noise.
    let band = 1.0 - 0.018 * ((p.y / 14.0 + 0.3 * n).floor() as i32).rem_euclid(2) as f32;
    let c = c * band;
    [c.x, c.y, c.z, 1.0]
}

fn terrain_mesh(cm: &chunk_mesh::ChunkMesh) -> Mesh {
    let colors: Vec<[f32; 4]> = cm
        .positions
        .iter()
        .zip(&cm.materials)
        .map(|(p, w)| terrain_color(*w, Vec3::from(*p)))
        .collect();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, cm.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, cm.normals.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(cm.indices.clone()))
}

/// Water surface buffers (positions, colours, indices).
#[derive(Default)]
struct WaterData {
    pos: Vec<[f32; 3]>,
    col: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

impl WaterData {
    fn into_mesh(self) -> Option<Mesh> {
        if self.idx.is_empty() {
            return None;
        }
        let normals = vec![[0.0, 1.0, 0.0]; self.pos.len()];
        Some(
            Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::RENDER_WORLD,
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col)
            .with_inserted_indices(Indices::U32(self.idx)),
        )
    }
}

/// Water surface of one chunk: cells with a wet corner, at that water level; vertex colour
/// r = depth / 6 m, g = foam weight.
fn water_mesh(t: &cb_terrain::Terrain, cx: usize, cz: usize, lod: u32) -> Option<Mesh> {
    let mut d = WaterData::default();
    water_data(t, cx, cz, lod, &mut d);
    d.into_mesh()
}

/// Appends one chunk's water surface to `out`.
fn water_data(t: &cb_terrain::Terrain, cx: usize, cz: usize, lod: u32, out: &mut WaterData) {
    let h = t.height();
    let w = t.water();
    let n = h.n();
    let step = 1usize << lod.min(2);
    let (i0, j0) = (cx * CHUNK_CELLS, cz * CHUNK_CELLS);
    let WaterData { pos, col, idx } = out;
    let mut j = j0;
    while j < (j0 + CHUNK_CELLS).min(n - 1) {
        let mut i = i0;
        while i < (i0 + CHUNK_CELLS).min(n - 1) {
            let i1 = (i + step).min(n - 1);
            let j1 = (j + step).min(n - 1);
            let corners = [(i, j), (i1, j), (i1, j1), (i, j1)];
            let level = corners
                .iter()
                .filter_map(|&(a, b)| w.level(a, b))
                .fold(f32::NEG_INFINITY, f32::max);
            if level.is_finite() {
                let base = pos.len() as u32;
                for &(a, b) in &corners {
                    let p = h.pos(a, b);
                    let depth = level - h.get(a, b);
                    pos.push([p.x, level, p.y]);
                    let foam = (1.0 - depth / 0.6).clamp(0.0, 1.0);
                    col.push([(depth / 6.0).clamp(0.0, 1.0), foam, 0.0, 1.0]);
                }
                idx.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
            }
            i += step;
        }
        j += step;
    }
}

fn chunk_center(t: &cb_terrain::Terrain, cx: usize, cz: usize) -> Vec2 {
    let h = t.height();
    let c = CHUNK_CELLS / 2;
    let n = h.n() - 1;
    h.pos((cx * CHUNK_CELLS + c).min(n), (cz * CHUNK_CELLS + c).min(n))
}

#[allow(clippy::too_many_arguments, reason = "Bevy system parameters")]
fn sync_terrain(
    mut commands: Commands,
    terrain: Option<Res<TerrainRes>>,
    mut changed: MessageReader<TerrainChanged>,
    rig: Res<CameraRig>,
    mats: Option<Res<WorldMaterials>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut state: ResMut<ViewState>,
    mut backlog: ResMut<ViewBacklog>,
) {
    let (Some(terrain), Some(mats)) = (terrain, mats) else {
        return;
    };
    let t = terrain.terrain();
    let per = chunk_mesh::chunks_per_side(t);
    if state.per_side != per {
        state.per_side = per;
        state.chunks = vec![
            ChunkSlot {
                dirty: true,
                ..Default::default()
            };
            per * per
        ];
        state.terrain_jobs.clear();
        state.scatter_jobs.clear();
    }
    let mut any_msg = false;
    for msg in changed.read() {
        any_msg = true;
        let r = msg.rect;
        let (c0, c1) = (
            (r.i0.saturating_sub(1)) / CHUNK_CELLS,
            ((r.i1 + 1) / CHUNK_CELLS).min(per - 1),
        );
        let (d0, d1) = (
            (r.j0.saturating_sub(1)) / CHUNK_CELLS,
            ((r.j1 + 1) / CHUNK_CELLS).min(per - 1),
        );
        for cz in d0..=d1 {
            for cx in c0..=c1 {
                // A job already in flight meshes the old terrain; rebuild after it lands.
                state.chunks[cz * per + cx].dirty = true;
            }
        }
        state.scatter_dirty = true;
    }
    let _span = tracing::info_span!("view_terrain_sync").entered();
    // 1. Upload finished jobs.
    let done: Vec<(usize, TerrainJob)> = state
        .terrain_jobs
        .iter_mut()
        .filter_map(|(k, task)| check_ready(task).map(|j| (*k, j)))
        .collect();
    let any_done = !done.is_empty();
    for (k, job) in done {
        state.terrain_jobs.remove(&k);
        let mesh = meshes.add(job.ground);
        let slot = &mut state.chunks[k];
        match slot.terrain {
            Some((e, _)) => {
                commands.entity(e).insert(Mesh3d(mesh));
                slot.terrain = Some((e, job.lod));
            }
            None => {
                let e = commands
                    .spawn((
                        Mesh3d(mesh),
                        MeshMaterial3d(mats.terrain.clone()),
                        Transform::default(),
                    ))
                    .id();
                slot.terrain = Some((e, job.lod));
            }
        }
        if let Some(e) = slot.water.take() {
            commands.entity(e).despawn();
        }
        if let Some(wm) = job.water {
            slot.water = Some(
                commands
                    .spawn((
                        Mesh3d(meshes.add(wm)),
                        MeshMaterial3d(mats.water.clone()),
                        Transform::default(),
                        NotShadowCaster,
                    ))
                    .id(),
            );
        }
    }
    // 2. Desired LOD per chunk and merge decision per super-chunk. Distances are recomputed only
    //    when the eye moved more than `TERRAIN_EYE_STEP_M`; a settled view with nothing in flight
    //    and nothing changed skips the bookkeeping entirely.
    let eye = rig.eye();
    let eye2 = Vec2::new(eye.x, eye.z);
    let moved = state.terrain_dist.len() != per * per
        || state
            .terrain_eye
            .is_none_or(|e| e.distance(eye2) > TERRAIN_EYE_STEP_M);
    let idle = state.terrain_jobs.is_empty() && state.super_jobs.is_empty();
    if !moved && !any_done && !any_msg && idle && state.terrain_settled {
        backlog.0 = 0;
        return;
    }
    if moved {
        state.terrain_eye = Some(eye2);
        state.terrain_dist = (0..per * per)
            .map(|k| chunk_center(t, k % per, k / per).distance(eye2))
            .collect();
    }
    let dist = state.terrain_dist.clone();
    let want: Vec<u32> = dist
        .iter()
        .map(|d| TERRAIN_LOD_M.iter().filter(|l| *d > **l).count() as u32)
        .collect();
    let sp = per.div_ceil(SUPER);
    if state.supers.len() != sp * sp {
        for sl in std::mem::take(&mut state.supers) {
            if let Some((e, w, _)) = sl.merged {
                commands.entity(e).despawn();
                if let Some(w) = w {
                    commands.entity(w).despawn();
                }
            }
        }
        state.supers = vec![SuperSlot::default(); sp * sp];
        state.super_jobs.clear();
    }
    let members = |s: usize| {
        let (sx, sz) = (s % sp, s / sp);
        (sz * SUPER..((sz + 1) * SUPER).min(per)).flat_map(move |cz| {
            (sx * SUPER..((sx + 1) * SUPER).min(per)).map(move |cx| cz * per + cx)
        })
    };
    let super_of = |k: usize| (k / per / SUPER) * sp + (k % per) / SUPER;
    let merge_want: Vec<Option<u32>> = (0..sp * sp)
        .map(|s| {
            let l = want[members(s).next()?];
            (l >= MERGE_FROM_LOD && members(s).all(|k| want[k] == l)).then_some(l)
        })
        .collect();
    // 3. Upload finished super jobs.
    let done: Vec<(usize, TerrainJob)> = state
        .super_jobs
        .iter_mut()
        .filter_map(|(s, task)| check_ready(task).map(|j| (*s, j)))
        .collect();
    for (si, job) in done {
        state.super_jobs.remove(&si);
        if let Some((e, w, _)) = state.supers[si].merged.take() {
            commands.entity(e).despawn();
            if let Some(w) = w {
                commands.entity(w).despawn();
            }
        }
        let e = commands
            .spawn((
                Mesh3d(meshes.add(job.ground)),
                MeshMaterial3d(mats.terrain.clone()),
                Transform::default(),
            ))
            .id();
        let w = job.water.map(|wm| {
            commands
                .spawn((
                    Mesh3d(meshes.add(wm)),
                    MeshMaterial3d(mats.water.clone()),
                    Transform::default(),
                    NotShadowCaster,
                ))
                .id()
        });
        state.supers[si].merged = Some((e, w, job.lod));
    }
    // 4. Hand-over: a merged block replaces its members once it exists at the wanted LOD; a
    //    block that should split stays until every member has its own mesh (no holes).
    for (si, mw) in merge_want.iter().enumerate() {
        let merged = state.supers[si].merged;
        match (*mw, merged) {
            (Some(l), Some((_, _, ml))) if l == ml => {
                for k in members(si) {
                    let slot = &mut state.chunks[k];
                    if let Some((e, _)) = slot.terrain.take() {
                        commands.entity(e).despawn();
                    }
                    if let Some(e) = slot.water.take() {
                        commands.entity(e).despawn();
                    }
                }
            }
            (None, Some((e, w, _))) if members(si).all(|k| state.chunks[k].terrain.is_some()) => {
                commands.entity(e).despawn();
                if let Some(w) = w {
                    commands.entity(w).despawn();
                }
                state.supers[si].merged = None;
            }
            _ => {}
        }
    }
    // 5. Work: individual chunks outside merged blocks, and merged blocks (nearest first).
    let mut work: Vec<(f32, usize, u32)> = Vec::new();
    for k in 0..per * per {
        if state.terrain_jobs.contains_key(&k) || merge_want[super_of(k)].is_some() {
            continue;
        }
        let slot = state.chunks[k];
        if slot.dirty || slot.terrain.map(|x| x.1) != Some(want[k]) {
            work.push((dist[k], k, want[k]));
        }
    }
    let mut super_work: Vec<(f32, usize, u32)> = Vec::new();
    for (si, mw) in merge_want.iter().enumerate() {
        let Some(l) = *mw else { continue };
        if state.super_jobs.contains_key(&si) {
            continue;
        }
        let have = state.supers[si].merged.map(|m| m.2);
        if have != Some(l) || members(si).any(|k| state.chunks[k].dirty) {
            let d = members(si).map(|k| dist[k]).fold(f32::INFINITY, f32::min);
            super_work.push((d, si, l));
        }
    }
    backlog.0 = work.len() + state.terrain_jobs.len() + super_work.len() + state.super_jobs.len();
    state.terrain_settled = work.is_empty() && super_work.is_empty();
    work.sort_by(|a, b| a.0.total_cmp(&b.0));
    super_work.sort_by(|a, b| a.0.total_cmp(&b.0));
    // 6. Start jobs from the current snapshot.
    let pool = AsyncComputeTaskPool::get();
    let free = MAX_JOBS_IN_FLIGHT.saturating_sub(state.terrain_jobs.len());
    for &(_, k, lod) in work.iter().take(free) {
        state.chunks[k].dirty = false;
        let snap = terrain.snapshot();
        let (cx, cz) = (k % per, k / per);
        let task = pool.spawn(async move {
            let cm = chunk_mesh::build(&snap, cx, cz, lod);
            TerrainJob {
                lod,
                ground: terrain_mesh(&cm),
                water: water_mesh(&snap, cx, cz, lod),
            }
        });
        state.terrain_jobs.insert(k, task);
    }
    let free = MAX_SUPER_JOBS.saturating_sub(state.super_jobs.len());
    for &(_, si, lod) in super_work.iter().take(free) {
        let ks: Vec<usize> = members(si).collect();
        for &k in &ks {
            state.chunks[k].dirty = false;
        }
        let snap = terrain.snapshot();
        let task = pool.spawn(async move {
            let mut all = chunk_mesh::ChunkMesh {
                positions: Vec::new(),
                normals: Vec::new(),
                materials: Vec::new(),
                indices: Vec::new(),
            };
            let mut water = WaterData::default();
            for k in ks {
                let (cx, cz) = (k % per, k / per);
                let cm = chunk_mesh::build(&snap, cx, cz, lod);
                let base = all.positions.len() as u32;
                all.positions.extend_from_slice(&cm.positions);
                all.normals.extend_from_slice(&cm.normals);
                all.materials.extend_from_slice(&cm.materials);
                all.indices.extend(cm.indices.iter().map(|i| i + base));
                water_data(&snap, cx, cz, lod, &mut water);
            }
            TerrainJob {
                lod,
                ground: terrain_mesh(&all),
                water: water.into_mesh(),
            }
        });
        state.super_jobs.insert(si, task);
    }
}

// ---------- network ----------

fn dead_end(world_nodes: &BTreeMap<NodeId, RoadNode>, n: NodeId) -> bool {
    world_nodes.get(&n).is_some_and(|x| x.segments().len() <= 1)
}

#[allow(clippy::too_many_arguments, reason = "Bevy system parameters")]
fn sync_network(
    mut commands: Commands,
    mut msgs: MessageReader<NetworkChanged>,
    ids: Option<Res<Ids>>,
    segs: Query<&RoadSegment>,
    nodes: Query<&RoadNode>,
    mats: Option<Res<WorldMaterials>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut state: ResMut<ViewState>,
) {
    let (Some(ids), Some(mats)) = (ids, mats) else {
        return;
    };
    let mut seg_dirty: Vec<SegmentId> = Vec::new();
    let mut node_dirty: Vec<NodeId> = Vec::new();
    for m in msgs.read() {
        match *m {
            NetworkChanged::Segment(s) | NetworkChanged::SegmentRemoved(s) => seg_dirty.push(s),
            NetworkChanged::Node(n) | NetworkChanged::NodeRemoved(n) => node_dirty.push(n),
        }
    }
    if seg_dirty.is_empty() && node_dirty.is_empty() {
        return;
    }
    seg_dirty.sort();
    seg_dirty.dedup();
    node_dirty.sort();
    node_dirty.dedup();
    let all_nodes: BTreeMap<NodeId, RoadNode> = ids
        .nodes
        .iter()
        .filter_map(|(id, e)| nodes.get(e).ok().map(|n| (id, n.clone())))
        .collect();
    // A node change affects the caps of its segments.
    for n in &node_dirty {
        if let Some(node) = all_nodes.get(n) {
            seg_dirty.extend(node.segments());
        }
    }
    seg_dirty.sort();
    seg_dirty.dedup();
    for sid in seg_dirty {
        if let Some(e) = state.segments.remove(&sid) {
            commands.entity(e).despawn();
        }
        let Some(seg) = ids.segments.get(sid).and_then(|e| segs.get(e).ok()) else {
            continue;
        };
        let (c0, c1) = seg.cuts();
        let len = seg.geom().length();
        let prm = RoadMeshParams {
            s0_m: c0,
            s1_m: len - c1,
            cap_start: dead_end(&all_nodes, seg.start()),
            cap_end: dead_end(&all_nodes, seg.end()),
            style: *seg.style(),
        };
        let mut md = segment_mesh(seg.geom(), seg.profile(), &seg.cross_section(), &prm, 0);
        if !seg.spans().is_empty() {
            md.append(&cb_procgen::bridge_mesh::bridge_mesh(
                seg.geom(),
                seg.profile(),
                seg.spans(),
                seg.piers(),
                &cb_network::bridge::params(seg.kind()),
                0,
            ));
        }
        let items = cb_network::style::decor_items(
            seg.geom(),
            seg.profile(),
            &seg.cross_section(),
            seg.style(),
            (c0, len - c1),
            seg.spans(),
        );
        if !items.is_empty() {
            md.append(&cb_procgen::road_decor::decor_mesh(
                &items,
                u64::from(sid.get()),
                0,
            ));
        }
        let e = commands
            .spawn((
                Mesh3d(meshes.add(world_mesh(&md))),
                MeshMaterial3d(mats.world.clone()),
                Transform::default(),
            ))
            .id();
        state.segments.insert(sid, e);
    }
    for nid in node_dirty {
        if let Some(e) = state.nodes.remove(&nid) {
            commands.entity(e).despawn();
        }
        let Some(node) = all_nodes.get(&nid) else {
            continue;
        };
        let Some(j) = node.junction() else {
            continue;
        };
        let mut md = junction_mesh(j, 0);
        let mut p = Painter::new(0, u64::from(nid.get()));
        markings_mesh::paint_markings(&mut p, node.markings());
        let slots = if node.control() == ControlMode::Lights {
            markings_mesh::paint_signals(&mut p, &node.markings().stop_lines)
        } else {
            Vec::new()
        };
        md.append(&p.finish());
        let e = commands
            .spawn((
                Mesh3d(meshes.add(world_mesh(&md))),
                MeshMaterial3d(mats.world.clone()),
                Transform::default(),
            ))
            .id();
        for slot in &slots {
            for lamp in Lamp::ALL {
                let lm = markings_mesh::lit_lamp_mesh(slot, lamp);
                commands.spawn((
                    Mesh3d(meshes.add(world_mesh(&lm))),
                    MeshMaterial3d(mats.world.clone()),
                    Transform::default(),
                    Visibility::Hidden,
                    NotShadowCaster,
                    SignalLamp {
                        node: nid,
                        approach: slot.approach,
                        lamp,
                    },
                    ChildOf(e),
                ));
            }
        }
        state.nodes.insert(nid, e);
    }
}

/// A lit signal lamp, shown while its approach's signal shows that colour.
#[derive(Component, Clone, Copy, Debug)]
pub struct SignalLamp {
    node: NodeId,
    approach: usize,
    lamp: Lamp,
}

/// Shows the lit lamps of every signal head from the fixed-time stub clock
/// (`cb_network::control::stub_phase_at`) until the traffic runtime drives the phases (M9).
fn update_signal_lamps(
    clock: Option<Res<cb_sim::SimClock>>,
    ids: Option<Res<Ids>>,
    nodes: Query<&RoadNode>,
    mut lamps: Query<(&SignalLamp, &mut Visibility)>,
) {
    use cb_network::control::{SignalState, stub_phase_at};
    let (Some(clock), Some(ids)) = (clock, ids) else {
        return;
    };
    let t_sim_s = clock.tick().0 as f64 * f64::from(cb_core::time::TICK_DT_SIM_S);
    for (lamp, mut vis) in &mut lamps {
        let Some(node) = ids.nodes.get(lamp.node).and_then(|e| nodes.get(e).ok()) else {
            continue;
        };
        let Some(j) = node.junction() else {
            continue;
        };
        // Neighbouring junctions are offset so they do not switch in lock-step.
        let offset = f64::from(lamp.node.get() % 17) * 3.7;
        let showing = match stub_phase_at(node.phases().len(), t_sim_s, offset) {
            Some((k, state)) => {
                let green = node.phases()[k].connectors.iter().any(|c| {
                    j.connectors
                        .get(*c)
                        .is_some_and(|c| c.from == lamp.approach)
                });
                match (green, state) {
                    (true, SignalState::Green) => Lamp::Green,
                    (true, SignalState::Yellow) => Lamp::Yellow,
                    _ => Lamp::Red,
                }
            }
            None => Lamp::Red,
        };
        vis.set_if_neq(if showing == lamp.lamp {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

// ---------- buildings ----------

/// LOD0 (houses with their yard) and LOD1 meshes of a building using parameter `slot`.
#[must_use]
pub fn building_meshes(
    b: &Building,
    ground: &dyn Fn(Vec2) -> f32,
    slot: u16,
) -> (MeshData, MeshData) {
    use cb_procgen::building::{civic, zoned};
    use cb_sim::buildings::{Massing, Site};
    let Some(m) = b.massing() else {
        return (MeshData::default(), MeshData::default());
    };
    match &m {
        Massing::Zoned(zm) => {
            let (mut lod0, lod1) = zm.mesh(b.foundation(), ground, slot);
            if let (zoned::Massing::House(h), Site::Zoned(s)) = (zm, b.site()) {
                let yl = YardLot {
                    polygon: &s.polygon,
                    edges: &s.edges,
                    front_start: s.front.0,
                    front_end: s.front.1,
                };
                let y = yard::yard(
                    h,
                    &yl,
                    b.foundation().floor_y,
                    ground,
                    slot,
                    b.seed() ^ 0xA5D,
                );
                lod0.append(&y);
            }
            (lod0, lod1)
        }
        Massing::Civic(c) => civic::mesh(c, b.foundation(), slot),
    }
}

/// Condition code and reveal height of a building at `tick`.
fn building_params(b: &Building, tick: u64) -> (f32, f32) {
    use crate::render::{NO_REVEAL_Y, condition_code};
    use cb_sim::buildings::Condition;
    match b.condition() {
        Condition::Constructing { .. } => {
            let t = b.condition().progress(tick);
            // Foundations first, then the walls rise; quantized to 5 cm so the buffer is
            // rewritten only when the visible height changes.
            let y = b.foundation().floor_y - 0.05 + t * (b.height_m() + 1.5);
            (condition_code::ACTIVE, (y * 20.0).floor() / 20.0)
        }
        Condition::Active => (condition_code::ACTIVE, NO_REVEAL_Y),
        Condition::Declining => (condition_code::DECLINING, NO_REVEAL_Y),
        Condition::Abandoned => (condition_code::ABANDONED, NO_REVEAL_Y),
        Condition::Ruined => (condition_code::RUINED, NO_REVEAL_Y),
    }
}

/// LOD0 and LOD1 meshes of a building in its condition (ruins swap to the ruin generator).
fn condition_meshes(b: &Building, ground: &dyn Fn(Vec2) -> f32, slot: u16) -> (MeshData, MeshData) {
    if b.condition() == cb_sim::buildings::Condition::Ruined {
        let r = cb_procgen::building::condition::ruin(
            b.outlines(),
            b.foundation().floor_y,
            b.height_m(),
            cb_procgen::palette::STONE,
            b.seed(),
            slot,
        );
        return (r.clone(), r);
    }
    building_meshes(b, ground, slot)
}

/// Chunk of a building (by the centroid of its first outline).
fn building_chunk(b: &Building) -> (i32, i32) {
    let c = b.outlines().first().map_or(Vec2::ZERO, |o| {
        o.iter().copied().sum::<Vec2>() / o.len().max(1) as f32
    });
    (
        (c.x / BUILDING_CHUNK_M).floor() as i32,
        (c.y / BUILDING_CHUNK_M).floor() as i32,
    )
}

#[allow(clippy::too_many_arguments, reason = "Bevy system parameters")]
fn sync_buildings(
    mut commands: Commands,
    mut msgs: MessageReader<BuildingChanged>,
    ids: Option<Res<Ids>>,
    buildings: Query<&Building>,
    terrain: Option<Res<TerrainRes>>,
    clock: Option<Res<cb_sim::SimClock>>,
    mats: Option<Res<WorldMaterials>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut slots: ResMut<crate::render::ObjectSlots>,
    mut stats: ResMut<ViewStats>,
    mut state: ResMut<ViewState>,
) {
    use cb_sim::buildings::Condition;
    let (Some(ids), Some(mats)) = (ids, mats) else {
        return;
    };
    let _span = tracing::info_span!("view_buildings_sync").entered();
    let tick = clock.map_or(0, |c| c.tick().0);
    for m in msgs.read() {
        let id = match *m {
            BuildingChanged::Built(id)
            | BuildingChanged::Removed(id)
            | BuildingChanged::Condition(id) => id,
        };
        let b = ids.buildings.get(id).and_then(|e| buildings.get(e).ok());
        if let (BuildingChanged::Condition(_), Some(b), Some(v)) = (m, b, state.buildings.get(&id))
        {
            // Shader parameters only; the scaffold goes when construction ends.
            let mut v = *v;
            let (code, reveal) = building_params(b, tick);
            slots.set(v.slot, code, reveal);
            let constructing = matches!(b.condition(), Condition::Constructing { .. });
            if !constructing && let Some(sc) = v.scaffold.take() {
                commands.entity(sc).despawn();
            }
            state.buildings.insert(id, v);
            continue;
        }
        let slot = match state.buildings.remove(&id) {
            Some(v) => {
                if let Some(sc) = v.scaffold {
                    commands.entity(sc).despawn();
                }
                if v.civic {
                    state.scatter_dirty = true;
                }
                if let Some(ch) = state.bchunks.get_mut(&v.chunk) {
                    ch.members.remove(&id);
                    ch.dirty = true;
                }
                if b.is_none() {
                    slots.release(v.slot);
                    continue;
                }
                v.slot
            }
            None if b.is_some() => slots.alloc(),
            None => continue,
        };
        let Some(b) = b else { continue };
        let chunk = building_chunk(b);
        let ch = state.bchunks.entry(chunk).or_default();
        ch.members.insert(id);
        ch.dirty = true;
        let scaffold = matches!(b.condition(), Condition::Constructing { .. }).then(|| {
            let fy = b.foundation().floor_y;
            let sc = cb_procgen::building::condition::scaffold(
                b.outlines(),
                fy,
                fy + b.height_m() + 1.0,
                slot,
            );
            commands
                .spawn((
                    Mesh3d(meshes.add(world_mesh(&sc))),
                    MeshMaterial3d(mats.world.clone()),
                    Transform::default(),
                ))
                .id()
        });
        let (code, reveal) = building_params(b, tick);
        slots.set(slot, code, reveal);
        let civic = b.parcel().is_some();
        if civic {
            state.scatter_dirty = true;
        }
        state.buildings.insert(
            id,
            BuildingVisual {
                chunk,
                slot,
                scaffold,
                civic,
            },
        );
    }
    // Construction progress: raise the reveal height of buildings with a scaffold.
    for (id, v) in &state.buildings {
        if v.scaffold.is_none() {
            continue;
        }
        if let Some(b) = ids.buildings.get(*id).and_then(|e| buildings.get(e).ok()) {
            let (code, reveal) = building_params(b, tick);
            slots.set(v.slot, code, reveal);
        }
    }
    // Finished chunk meshes replace the old ones (LOD0 near, LOD1 far).
    let done: Vec<((i32, i32), BuildingChunkJob)> = state
        .bchunk_jobs
        .iter_mut()
        .filter_map(|(k, task)| check_ready(task).map(|j| (*k, j)))
        .collect();
    for (k, job) in done {
        state.bchunk_jobs.remove(&k);
        let Some(ch) = state.bchunks.get_mut(&k) else {
            continue;
        };
        for e in [ch.lod0.take(), ch.lod1.take()].into_iter().flatten() {
            commands.entity(e).despawn();
        }
        let Some((lod0, lod1)) = job.meshes else {
            continue;
        };
        let mut spawn = |commands: &mut Commands, mesh: Mesh, range: VisibilityRange| {
            commands
                .spawn((
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(mats.world.clone()),
                    Transform::default(),
                    range,
                ))
                .id()
        };
        ch.lod0 = Some(spawn(
            &mut commands,
            lod0,
            VisibilityRange {
                start_margin: 0.0..0.0,
                end_margin: BUILDING_LOD1_M..BUILDING_LOD1_M,
                use_aabb: true,
            },
        ));
        ch.lod1 = Some(spawn(
            &mut commands,
            lod1,
            VisibilityRange {
                start_margin: BUILDING_LOD1_M..BUILDING_LOD1_M,
                end_margin: f32::MAX..f32::MAX,
                use_aabb: true,
            },
        ));
    }
    // Rebuild dirty chunks off the main thread from snapshots of their buildings.
    let free = MAX_JOBS_IN_FLIGHT.saturating_sub(state.bchunk_jobs.len());
    if free == 0 {
        return;
    }
    let dirty: Vec<(i32, i32)> = state
        .bchunks
        .iter()
        .filter(|(k, c)| c.dirty && !state.bchunk_jobs.contains_key(*k))
        .map(|(k, _)| *k)
        .take(free)
        .collect();
    let pool = AsyncComputeTaskPool::get();
    for k in dirty {
        let Some(ch) = state.bchunks.get_mut(&k) else {
            continue;
        };
        ch.dirty = false;
        let members: Vec<BuildingId> = ch.members.iter().copied().collect();
        let items: Vec<(Building, u16)> = members
            .iter()
            .filter_map(|id| {
                let b = ids.buildings.get(*id).and_then(|e| buildings.get(e).ok())?;
                Some((b.clone(), state.buildings.get(id)?.slot))
            })
            .collect();
        stats.building_meshes += items.len() as u64;
        let snap = terrain.as_ref().map(|t| t.snapshot());
        let task = pool.spawn(async move {
            if items.is_empty() {
                return BuildingChunkJob { meshes: None };
            }
            let ground = |p: Vec2| snap.as_ref().map_or(0.0, |t| t.height_at(p));
            let (mut lod0, mut lod1) = (MeshData::default(), MeshData::default());
            for (b, slot) in &items {
                let (a, c) = condition_meshes(b, &ground, *slot);
                lod0.append(&a);
                lod1.append(&c);
            }
            BuildingChunkJob {
                meshes: Some((world_mesh(&lod0), world_mesh(&lod1))),
            }
        });
        state.bchunk_jobs.insert(k, task);
    }
}

/// Placed decorations: one mesh entity each.
fn sync_props(
    mut commands: Commands,
    mut msgs: MessageReader<cb_sim::props::PropChanged>,
    ids: Option<Res<Ids>>,
    props: Query<&cb_sim::props::PlacedProp>,
    mats: Option<Res<WorldMaterials>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut state: ResMut<ViewState>,
) {
    let (Some(ids), Some(mats)) = (ids, mats) else {
        return;
    };
    for m in msgs.read() {
        let id = match *m {
            cb_sim::props::PropChanged::Placed(id) | cb_sim::props::PropChanged::Removed(id) => id,
        };
        if let Some(e) = state.props.remove(&id) {
            commands.entity(e).despawn();
        }
        let Some(p) = ids.props.get(id).and_then(|e| props.get(e).ok()) else {
            continue;
        };
        let md = cb_procgen::decor::mesh(p.kind(), p.pos(), p.yaw(), p.seed(), 0);
        let e = commands
            .spawn((
                Mesh3d(meshes.add(world_mesh(&md))),
                MeshMaterial3d(mats.world.clone()),
                Transform::default(),
            ))
            .id();
        state.props.insert(id, e);
    }
}

fn sync_zoning(mut msgs: MessageReader<ZoningChanged>, mut state: ResMut<ViewState>) {
    if msgs.read().next().is_some() {
        // Plots exclude scattered trees.
        state.scatter_dirty = true;
    }
}

// ---------- scatter ----------

/// Cheap far tree (4–8 triangles).
fn far_tree(p: &mut Painter, base: Vec3, conifer: bool, h: f32, rng: &mut Pcg32) {
    use cb_procgen::palette;
    use cb_procgen::surface::Surface;
    let col = if conifer {
        palette::pick(rng, &palette::CONIFER)
    } else {
        palette::pick(rng, &palette::FOLIAGE)
    };
    let r = h * if conifer { 0.24 } else { 0.32 };
    let y0 = base.y + h * if conifer { 0.12 } else { 0.35 };
    let top = Vec3::new(base.x, base.y + h, base.z);
    let ring = [
        Vec3::new(r, 0.0, 0.0),
        Vec3::new(0.0, 0.0, r),
        Vec3::new(-r, 0.0, 0.0),
        Vec3::new(0.0, 0.0, -r),
    ]
    .map(|o| Vec3::new(base.x, y0 + if conifer { 0.0 } else { h * 0.25 }, base.z) + o);
    for k in 0..4 {
        let (a, b) = (ring[k], ring[(k + 1) % 4]);
        let out = (a + b) * 0.5 - Vec3::new(base.x, a.y, base.z) + Vec3::Y * 0.3 * r;
        p.face_ao(
            &[a, b, top],
            &[0.9, 0.9, 1.0],
            col,
            Surface::Foliage,
            0,
            out,
        );
        if !conifer {
            let bottom = Vec3::new(base.x, y0, base.z);
            p.face_ao(
                &[b, a, bottom],
                &[0.8, 0.8, 0.75],
                col.scale(0.9),
                Surface::Foliage,
                0,
                out - Vec3::Y * r,
            );
        }
    }
}

/// Scatter mesh of one chunk: trees, bushes and rocks outside plots.
fn scatter_mesh(
    t: &cb_terrain::Terrain,
    seed: u64,
    cx: usize,
    cz: usize,
    near: bool,
    plots: &[(Vec2, Vec2, Vec<Vec2>)],
) -> MeshData {
    let mut p = Painter::new(0, seed ^ (cx as u64) << 20 ^ cz as u64);
    let c = chunk_center(t, cx, cz);
    let half = (CHUNK_CELLS as f32 * t.height().spacing_m()) * 0.5;
    let (lo, hi) = (c - Vec2::splat(half), c + Vec2::splat(half));
    let (a, b) = (
        scatter::cell_of(lo),
        scatter::cell_of(hi - Vec2::splat(0.01)),
    );
    for gz in a.1..=b.1 {
        for gx in a.0..=b.0 {
            for inst in scatter::scatter_cell(t, seed, (gx, gz)) {
                let q = Vec2::new(inst.pos.x, inst.pos.z);
                if plots.iter().any(|(l, h, poly)| {
                    q.cmpge(*l).all()
                        && q.cmple(*h).all()
                        && cb_geometry::polygon::contains(poly, q)
                }) {
                    continue;
                }
                let mut rng = Pcg32::from_seed(u64::from(inst.seed));
                let h = 7.0 * inst.scale;
                match inst.kind {
                    PropKind::Conifer => {
                        if near {
                            vegetation::tree(
                                &mut p,
                                inst.pos,
                                TreeKind::Conifer,
                                h * 1.3,
                                &mut rng,
                            );
                        } else {
                            far_tree(&mut p, inst.pos, true, h * 1.3, &mut rng);
                        }
                    }
                    PropKind::Deciduous | PropKind::Birch => {
                        if near {
                            vegetation::tree(&mut p, inst.pos, TreeKind::Deciduous, h, &mut rng);
                        } else {
                            far_tree(&mut p, inst.pos, false, h, &mut rng);
                        }
                    }
                    PropKind::Bush if near => {
                        vegetation::bush(&mut p, inst.pos, 0.9 * inst.scale, &mut rng);
                    }
                    PropKind::Rock if near => {
                        let r = 0.6 * inst.scale;
                        let col = cb_procgen::palette::Rgb(0x9A, 0x95, 0x90);
                        p.contact_y = inst.pos.y;
                        p.prism(
                            &[
                                Vec2::new(-r, -r) + q,
                                Vec2::new(r, -0.6 * r) + q,
                                Vec2::new(0.7 * r, r) + q,
                                Vec2::new(-0.8 * r, 0.7 * r) + q,
                            ],
                            inst.pos.y - 0.3,
                            inst.pos.y + 0.7 * r,
                            col,
                            Some(col.scale(1.08)),
                            false,
                        );
                    }
                    _ => {}
                }
            }
        }
    }
    p.finish()
}

#[allow(clippy::too_many_arguments, reason = "Bevy system parameters")]
fn sync_scatter(
    mut commands: Commands,
    terrain: Option<Res<TerrainRes>>,
    seed: Option<Res<cb_sim::WorldSeed>>,
    ids: Option<Res<Ids>>,
    plots_q: Query<&Plot>,
    buildings_q: Query<&Building>,
    exclusion: Option<Res<ScatterExclusion>>,
    rig: Res<CameraRig>,
    mats: Option<Res<WorldMaterials>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut state: ResMut<ViewState>,
    mut backlog: ResMut<ViewBacklog>,
) {
    let (Some(terrain), Some(mats), Some(ids)) = (terrain, mats, ids) else {
        return;
    };
    let t = terrain.terrain();
    let per = state.per_side;
    if per == 0 {
        return;
    }
    let _span = tracing::info_span!("view_scatter_sync").entered();
    let seed = seed.map_or(1, |s| s.0);
    if exclusion.as_ref().is_some_and(DetectChanges::is_changed) {
        state.scatter_dirty = true;
    }
    if std::mem::take(&mut state.scatter_dirty) {
        for c in &mut state.chunks {
            c.scatter_stale = true;
        }
        state.plot_shapes = None;
    }
    // 1. Upload finished jobs.
    let done: Vec<(usize, ScatterJob)> = state
        .scatter_jobs
        .iter_mut()
        .filter_map(|(k, task)| check_ready(task).map(|j| (*k, j)))
        .collect();
    for (k, job) in done {
        state.scatter_jobs.remove(&k);
        if let Some((Some(e), _)) = state.chunks[k].scatter.take() {
            commands.entity(e).despawn();
        }
        let e = job.mesh.map(|m| {
            commands
                .spawn((
                    Mesh3d(meshes.add(m)),
                    MeshMaterial3d(mats.world.clone()),
                    Transform::default(),
                ))
                .id()
        });
        state.chunks[k].scatter = Some((e, job.level));
    }
    // 2. Chunks whose scatter level changed or is stale.
    let eye = rig.eye();
    let eye2 = Vec2::new(eye.x, eye.z);
    let mut work: Vec<(f32, usize, u8)> = Vec::new();
    for k in 0..per * per {
        if state.scatter_jobs.contains_key(&k) {
            continue;
        }
        let (cx, cz) = (k % per, k / per);
        let d = chunk_center(t, cx, cz).distance(eye2);
        let want = if d < SCATTER_NEAR_M {
            2u8
        } else if d < SCATTER_FAR_M {
            1
        } else {
            0
        };
        let have = state.chunks[k].scatter.map_or(0, |s| s.1);
        let stale = state.chunks[k].scatter_stale;
        if want == 0 {
            // Out of range: drop immediately (cheap, no job).
            if let Some((e, _)) = state.chunks[k].scatter.take()
                && let Some(e) = e
            {
                commands.entity(e).despawn();
            }
            state.chunks[k].scatter_stale = false;
            continue;
        }
        if want != have || stale {
            work.push((d, k, want));
        }
    }
    backlog.0 += work.len() + state.scatter_jobs.len();
    if work.is_empty() {
        return;
    }
    work.sort_by(|a, b| a.0.total_cmp(&b.0));
    let plots = state
        .plot_shapes
        .get_or_insert_with(|| {
            Arc::new(
                ids.plots
                    .iter()
                    .filter_map(|(_, e)| plots_q.get(e).ok())
                    .map(|p| p.polygon().to_vec())
                    .chain(exclusion.iter().flat_map(|x| x.0.iter().cloned()))
                    .chain(
                        ids.buildings
                            .iter()
                            .filter_map(|(_, e)| buildings_q.get(e).ok()?.parcel()),
                    )
                    .map(|poly| {
                        let (l, h) = cb_geometry::polygon::bounds(&poly);
                        (l, h, poly)
                    })
                    .collect(),
            )
        })
        .clone();
    // 3. Start jobs.
    let pool = AsyncComputeTaskPool::get();
    let free = MAX_JOBS_IN_FLIGHT.saturating_sub(state.scatter_jobs.len());
    for &(_, k, want) in work.iter().take(free) {
        state.chunks[k].scatter_stale = false;
        let snap = terrain.snapshot();
        let plots = Arc::clone(&plots);
        let task = pool.spawn(async move {
            let md = scatter_mesh(&snap, seed, k % per, k / per, want == 2, &plots);
            ScatterJob {
                level: want,
                mesh: (md.triangle_count() > 0).then(|| world_mesh(&md)),
            }
        });
        state.scatter_jobs.insert(k, task);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cb_geometry::CubicBezier2;
    use cb_network::RoadKind;
    use cb_sim::buildings::Condition;
    use cb_sim::catalog::ZoneKind;
    use cb_sim::commands::{
        BuildRoad, CreatePlot, DebugCommand, Endpoint, FrontageSpan, PlayerCommand,
    };
    use cb_sim::testing::Scenario;

    fn ok(s: &mut Scenario, cmd: PlayerCommand) {
        let r = s.command(cmd);
        assert!(r.outcome.is_ok(), "{:?}", r.outcome);
    }

    /// A sim scenario with the building view system (no renderer: meshes are plain assets).
    fn scenario() -> Scenario {
        let mut s = Scenario::new(11);
        s.app
            .init_resource::<Assets<Mesh>>()
            .init_resource::<crate::render::ObjectSlots>()
            .init_resource::<ViewState>()
            .init_resource::<ViewStats>()
            .insert_resource(WorldMaterials {
                world: Handle::default(),
                terrain: Handle::default(),
                water: Handle::default(),
                slots: Handle::default(),
            })
            .add_systems(Update, sync_buildings);
        ok(
            &mut s,
            PlayerCommand::BuildRoad(BuildRoad {
                kind: RoadKind::Street,
                curve: CubicBezier2::straight(Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0)),
                start: Endpoint::Free,
                end: Endpoint::Free,
            }),
        );
        let sid = s
            .world()
            .resource::<Ids>()
            .segments
            .iter()
            .next()
            .map(|(id, _)| id)
            .unwrap();
        ok(
            &mut s,
            PlayerCommand::CreatePlot(CreatePlot {
                zone: ZoneKind::Residential,
                frontage: vec![FrontageSpan {
                    segment: sid,
                    left: true,
                    s0: 10.0,
                    s1: 190.0,
                }],
                depth_m: 30.0,
                dn: 0,
            }),
        );
        s.run_ticks(800);
        s.app.update();
        s
    }

    #[test]
    fn condition_change_does_not_remesh() {
        let mut s = scenario();
        let state = s.world().resource::<ViewState>();
        assert!(state.building_count() >= 5, "{}", state.building_count());
        let (id, slot) = {
            let (id, v) = state.buildings.iter().next().unwrap();
            (*id, v.slot)
        };
        assert!(slot != 0, "each building owns a parameter slot");
        assert!(
            state.has_scaffold(id),
            "fresh buildings are under construction"
        );
        let meshes_before = s.world().resource::<ViewStats>().building_meshes;
        let writes_before = s.world().resource::<crate::render::ObjectSlots>().writes;
        for (cond, code) in [
            (Condition::Active, crate::render::condition_code::ACTIVE),
            (
                Condition::Declining,
                crate::render::condition_code::DECLINING,
            ),
            (
                Condition::Abandoned,
                crate::render::condition_code::ABANDONED,
            ),
        ] {
            ok(
                &mut s,
                PlayerCommand::Debug(DebugCommand::SetCondition {
                    building: id,
                    condition: cond,
                }),
            );
            s.app.update();
            let slots = s.world().resource::<crate::render::ObjectSlots>();
            let (params, reveal) = slots.get(slot);
            assert!((params[3] - code).abs() < 1e-6, "{cond:?}");
            assert!(
                reveal[0] >= crate::render::NO_REVEAL_Y,
                "{cond:?}: nothing clipped"
            );
        }
        let stats = s.world().resource::<ViewStats>();
        assert_eq!(
            stats.building_meshes, meshes_before,
            "condition changes never remesh"
        );
        assert!(s.world().resource::<crate::render::ObjectSlots>().writes > writes_before);
        assert!(!s.world().resource::<ViewState>().has_scaffold(id));
        // A ruin is a different mesh.
        ok(
            &mut s,
            PlayerCommand::Debug(DebugCommand::SetCondition {
                building: id,
                condition: Condition::Ruined,
            }),
        );
        // Its chunk is rebuilt off the main thread (once the previous job of that chunk lands).
        for _ in 0..200 {
            s.app.update();
            if s.world().resource::<ViewStats>().building_meshes > meshes_before {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(s.world().resource::<ViewStats>().building_meshes > meshes_before);
    }

    #[test]
    fn construction_reveal_rises_with_progress() {
        let mut s = scenario();
        let (id, slot) = {
            let st = s.world().resource::<ViewState>();
            let (id, v) = st.buildings.iter().next().unwrap();
            (*id, v.slot)
        };
        let floor = {
            let e = s.world().resource::<Ids>().buildings.get(id).unwrap();
            s.world().get::<Building>(e).unwrap().foundation().floor_y
        };
        let r0 = s
            .world()
            .resource::<crate::render::ObjectSlots>()
            .get(slot)
            .1[0];
        s.run_ticks(1200);
        s.app.update();
        let r1 = s
            .world()
            .resource::<crate::render::ObjectSlots>()
            .get(slot)
            .1[0];
        assert!(r0 < floor + 2.0, "starts near the floor: {r0} vs {floor}");
        assert!(r1 > r0, "rises: {r0} → {r1}");
    }
}
