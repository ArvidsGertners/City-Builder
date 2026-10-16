//! View sync (ARCHITECTURE §6): visuals derived from sim state, rebuilt incrementally from the
//! sim's change messages — terrain chunks (distance LOD), water, road segments and junctions,
//! buildings with yards, scattered trees and rocks. Visual entities are separate from sim
//! entities and keyed by stable ID (ADR-0005).
//!
//! M2 keeps one render entity per road segment, junction and building (≈ 1 000 entities for the
//! 500-house benchmark); per-chunk merging and the slot buffer follow when the benchmark needs it
//! (PERFORMANCE §4).

use std::collections::BTreeMap;

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use cb_core::ids::{BuildingId, NodeId, SegmentId};
use cb_core::rng::Pcg32;
use cb_geometry::MeshData;
use cb_procgen::building::house;
use cb_procgen::junction_mesh::junction_mesh;
use cb_procgen::paint::Painter;
use cb_procgen::road_mesh::{RoadMeshParams, segment_mesh};
use cb_procgen::vegetation::{self, TreeKind};
use cb_procgen::yard::{self, YardLot};
use cb_sim::buildings::{Building, BuildingChanged};
use cb_sim::ids::Ids;
use cb_sim::roads::{NetworkChanged, RoadNode, RoadSegment};
use cb_sim::terrain::{TerrainChanged, TerrainRes};
use cb_sim::zoning::{Lot, Plot, ZoningChanged};
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
/// Chunk rebuilds per frame at most (terrain, scatter).
pub const REBUILDS_PER_FRAME: usize = 12;
/// Main-thread time per frame for chunk rebuilds of one kind (ms); at least one rebuild always
/// runs. Off-thread meshing replaces this when the M4 city sizes need it (PERFORMANCE §4).
pub const REBUILD_BUDGET_MS: f32 = 2.0;

/// Shared per-frame rebuild budget of terrain and scatter. The kind that may exceed it with one
/// forced rebuild alternates per frame, so neither starves while the camera keeps moving.
#[derive(Default, Debug)]
struct RebuildBudget {
    used_ms: f32,
    terrain_first: bool,
}

impl RebuildBudget {
    /// Whether another rebuild may start (`done` rebuilds of this kind so far this frame).
    fn allows(&self, start: std::time::Instant, done: usize, forced: bool) -> bool {
        (forced && done == 0)
            || self.used_ms + start.elapsed().as_secs_f32() * 1000.0 < REBUILD_BUDGET_MS
    }
}

/// Per-chunk visual state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct ChunkSlot {
    terrain: Option<(Entity, u32)>,
    water: Option<Entity>,
    scatter: Option<(Entity, u8)>,
    dirty: bool,
    /// Scatter must be regenerated (plots or terrain changed).
    scatter_stale: bool,
}

/// Visual registries.
#[derive(Resource, Default)]
pub struct ViewState {
    chunks: Vec<ChunkSlot>,
    per_side: usize,
    segments: BTreeMap<SegmentId, Entity>,
    nodes: BTreeMap<NodeId, Entity>,
    buildings: BTreeMap<BuildingId, Entity>,
    scatter_dirty: bool,
    budget: RebuildBudget,
}

impl ViewState {
    /// Live building visuals.
    #[must_use]
    pub fn building_count(&self) -> usize {
        self.buildings.len()
    }
    /// Resident terrain chunk-LODs.
    #[must_use]
    pub fn terrain_chunks(&self) -> usize {
        self.chunks.iter().filter(|c| c.terrain.is_some()).count()
    }
    /// Whether every visible chunk is up to date (screenshot settling).
    #[must_use]
    pub fn settled(&self) -> bool {
        self.chunks.iter().all(|c| !c.dirty)
    }
}

/// View plugin.
pub struct ViewPlugin;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ViewState>()
            .init_resource::<ViewBacklog>()
            .add_systems(
                Update,
                (
                    sync_network,
                    sync_buildings,
                    sync_zoning,
                    sync_terrain,
                    sync_scatter,
                )
                    .chain()
                    .in_set(crate::app::GameSet::ViewSync),
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

/// Water surface of one chunk: cells with a wet corner, at that water level; vertex colour
/// r = depth / 6 m, g = foam weight.
fn water_mesh(t: &cb_terrain::Terrain, cx: usize, cz: usize, lod: u32) -> Option<Mesh> {
    let h = t.height();
    let w = t.water();
    let n = h.n();
    let step = 1usize << lod.min(2);
    let (i0, j0) = (cx * CHUNK_CELLS, cz * CHUNK_CELLS);
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut col: Vec<[f32; 4]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
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
    if idx.is_empty() {
        return None;
    }
    let normals = vec![[0.0, 1.0, 0.0]; pos.len()];
    Some(
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
        .with_inserted_indices(Indices::U32(idx)),
    )
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
    }
    for msg in changed.read() {
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
                state.chunks[cz * per + cx].dirty = true;
            }
        }
        state.scatter_dirty = true;
    }
    let eye = rig.eye();
    let eye2 = Vec2::new(eye.x, eye.z);
    // Chunks needing work, nearest first.
    let mut work: Vec<(f32, usize, u32)> = Vec::new();
    for cz in 0..per {
        for cx in 0..per {
            let k = cz * per + cx;
            let d = chunk_center(t, cx, cz).distance(eye2);
            let lod = TERRAIN_LOD_M.iter().filter(|l| d > **l).count() as u32;
            let slot = state.chunks[k];
            if slot.dirty || slot.terrain.map(|x| x.1) != Some(lod) {
                work.push((d, k, lod));
            }
        }
    }
    backlog.0 = work.len();
    work.sort_by(|a, b| a.0.total_cmp(&b.0));
    let _span = tracing::info_span!("view_terrain_rebuild").entered();
    let start = std::time::Instant::now();
    state.budget.terrain_first = !state.budget.terrain_first;
    state.budget.used_ms = 0.0;
    let forced = state.budget.terrain_first;
    for (i, &(_, k, lod)) in work.iter().take(REBUILDS_PER_FRAME).enumerate() {
        if !state.budget.allows(start, i, forced) {
            break;
        }
        let (cx, cz) = (k % per, k / per);
        let cm = chunk_mesh::build(t, cx, cz, lod);
        let mesh = meshes.add(terrain_mesh(&cm));
        let slot = &mut state.chunks[k];
        match slot.terrain {
            Some((e, _)) => {
                commands.entity(e).insert(Mesh3d(mesh));
            }
            None => {
                let e = commands
                    .spawn((
                        Mesh3d(mesh),
                        MeshMaterial3d(mats.terrain.clone()),
                        Transform::default(),
                    ))
                    .id();
                slot.terrain = Some((e, lod));
            }
        }
        slot.terrain = slot.terrain.map(|(e, _)| (e, lod));
        if let Some(e) = slot.water.take() {
            commands.entity(e).despawn();
        }
        if let Some(wm) = water_mesh(t, cx, cz, lod) {
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
        slot.dirty = false;
    }
    state.budget.used_ms = start.elapsed().as_secs_f32() * 1000.0;
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
        };
        let md = segment_mesh(seg.geom(), seg.profile(), &seg.cross_section(), &prm, 0);
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
        let Some(j) = all_nodes.get(&nid).and_then(RoadNode::junction) else {
            continue;
        };
        let md = junction_mesh(j, 0);
        let e = commands
            .spawn((
                Mesh3d(meshes.add(world_mesh(&md))),
                MeshMaterial3d(mats.world.clone()),
                Transform::default(),
            ))
            .id();
        state.nodes.insert(nid, e);
    }
}

// ---------- buildings ----------

/// LOD0 house + yard mesh of a building.
#[must_use]
pub fn building_mesh(b: &Building, lot: &Lot, ground: &dyn Fn(Vec2) -> f32) -> MeshData {
    let layout = b.layout();
    let (mut m, _lod1) = house::mesh(&layout, b.foundation(), ground, 0);
    let yl = YardLot {
        polygon: lot.polygon(),
        edges: lot.edges(),
        front_start: lot.front().0,
        front_end: lot.front().1,
    };
    let y = yard::yard(
        &layout,
        &yl,
        b.foundation().floor_y,
        ground,
        0,
        b.seed() ^ 0xA5D,
    );
    m.append(&y);
    m
}

#[allow(clippy::too_many_arguments, reason = "Bevy system parameters")]
fn sync_buildings(
    mut commands: Commands,
    mut msgs: MessageReader<BuildingChanged>,
    ids: Option<Res<Ids>>,
    buildings: Query<&Building>,
    lots: Query<&Lot>,
    terrain: Option<Res<TerrainRes>>,
    mats: Option<Res<WorldMaterials>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut state: ResMut<ViewState>,
) {
    let (Some(ids), Some(mats)) = (ids, mats) else {
        return;
    };
    let ground = |p: Vec2| terrain.as_ref().map_or(0.0, |t| t.terrain().height_at(p));
    for m in msgs.read() {
        let (BuildingChanged::Built(id) | BuildingChanged::Removed(id)) = *m;
        if let Some(e) = state.buildings.remove(&id) {
            commands.entity(e).despawn();
        }
        let Some(b) = ids.buildings.get(id).and_then(|e| buildings.get(e).ok()) else {
            continue;
        };
        let Some(lot) = ids.lots.get(b.lot()).and_then(|e| lots.get(e).ok()) else {
            continue;
        };
        let md = building_mesh(b, lot, &ground);
        let e = commands
            .spawn((
                Mesh3d(meshes.add(world_mesh(&md))),
                MeshMaterial3d(mats.world.clone()),
                Transform::default(),
            ))
            .id();
        state.buildings.insert(id, e);
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
    let seed = seed.map_or(1, |s| s.0);
    let eye = rig.eye();
    let eye2 = Vec2::new(eye.x, eye.z);
    if std::mem::take(&mut state.scatter_dirty) {
        for c in &mut state.chunks {
            c.scatter_stale = true;
        }
    }
    let mut work: Vec<(f32, usize, u8)> = Vec::new();
    for k in 0..per * per {
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
        if want == 0 && stale {
            state.chunks[k].scatter_stale = false;
        }
        if want != have || (stale && want > 0) {
            work.push((d, k, want));
        }
    }
    backlog.0 += work.len();
    if work.is_empty() {
        return;
    }
    work.sort_by(|a, b| a.0.total_cmp(&b.0));
    let plots: Vec<(Vec2, Vec2, Vec<Vec2>)> = ids
        .plots
        .iter()
        .filter_map(|(_, e)| plots_q.get(e).ok())
        .map(|p| {
            let (l, h) = cb_geometry::polygon::bounds(p.polygon());
            (l, h, p.polygon().to_vec())
        })
        .collect();
    let _span = tracing::info_span!("view_scatter_rebuild").entered();
    let start = std::time::Instant::now();
    let forced = !state.budget.terrain_first;
    for (i, &(_, k, want)) in work.iter().take(REBUILDS_PER_FRAME).enumerate() {
        if !state.budget.allows(start, i, forced) {
            break;
        }
        state.chunks[k].scatter_stale = false;
        if let Some((e, _)) = state.chunks[k].scatter.take() {
            commands.entity(e).despawn();
        }
        if want == 0 {
            continue;
        }
        let md = scatter_mesh(t, seed, k % per, k / per, want == 2, &plots);
        if md.triangle_count() > 0 {
            let e = commands
                .spawn((
                    Mesh3d(meshes.add(world_mesh(&md))),
                    MeshMaterial3d(mats.world.clone()),
                    Transform::default(),
                ))
                .id();
            state.chunks[k].scatter = Some((e, want));
        } else {
            let e = commands.spawn(Transform::default()).id();
            state.chunks[k].scatter = Some((e, want));
        }
    }
}
