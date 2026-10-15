//! Chunk meshing (LOD0–3 with skirts), road/bridge/water meshes and the lavapipe shot runner.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::CascadeShadowConfigBuilder;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::WindowResolution;
use glam::Vec2 as G2;

use crate::road::{self, Kind, Road};
use crate::terrain::{self, CHUNK, N, SP, Terrain};
use crate::world::{Features, World as SimWorld};

/// Terrain chunk skirt depth per LOD (S-G: crack p99.9 0.6 / 1.6 / 3.3 m, max 2.5 / 3.2 / 4.7 m).
pub const SKIRT_TERRAIN: [f32; 4] = [3.0, 4.0, 6.0, 8.0];
pub const SKIRT_ROAD: f32 = 1.0;

#[derive(Clone, Default)]
pub struct ChunkMesh {
    pub pos: Vec<[f32; 3]>,
    pub nrm: Vec<[f32; 3]>,
    pub col: Vec<[f32; 4]>,
    pub idx: Vec<u32>,
}

impl ChunkMesh {
    fn vert(&mut self, p: Vec3, n: Vec3, c: [f32; 4]) -> u32 {
        self.pos.push(p.to_array());
        self.nrm.push(n.to_array());
        self.col.push(c);
        (self.pos.len() - 1) as u32
    }
    fn quad(&mut self, a: u32, b: u32, c: u32, d: u32) {
        self.idx.extend([a, b, c, a, c, d]);
    }
    /// Flat quad with its own vertices (for boxes).
    fn face(&mut self, q: [Vec3; 4], c: [f32; 4]) {
        let n = (q[1] - q[0]).cross(q[2] - q[0]).normalize_or_zero();
        let i: Vec<u32> = q.iter().map(|p| self.vert(*p, n, c)).collect();
        self.quad(i[0], i[1], i[2], i[3]);
    }
    fn boxy(&mut self, c: Vec3, half: Vec3, dir: Vec3, col: [f32; 4]) {
        let f = dir.normalize_or(Vec3::X);
        let r = Vec3::Y.cross(f).normalize_or(Vec3::Z);
        let corner = |sx: f32, sy: f32, sz: f32| {
            c + f * (half.x * sx) + Vec3::Y * (half.y * sy) + r * (half.z * sz)
        };
        let p = |a: [f32; 3]| corner(a[0], a[1], a[2]);
        let faces = [
            [[1., 1., -1.], [1., 1., 1.], [-1., 1., 1.], [-1., 1., -1.]],
            [
                [1., -1., 1.],
                [1., -1., -1.],
                [-1., -1., -1.],
                [-1., -1., 1.],
            ],
            [
                [1., -1., -1.],
                [1., 1., -1.],
                [-1., 1., -1.],
                [-1., -1., -1.],
            ],
            [[-1., -1., 1.], [-1., 1., 1.], [1., 1., 1.], [1., -1., 1.]],
            [[1., -1., 1.], [1., 1., 1.], [1., 1., -1.], [1., -1., -1.]],
            [
                [-1., -1., -1.],
                [-1., 1., -1.],
                [-1., 1., 1.],
                [-1., -1., 1.],
            ],
        ];
        for fc in faces {
            self.face([p(fc[0]), p(fc[1]), p(fc[2]), p(fc[3])], col);
        }
    }
    pub fn to_mesh(&self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col.clone())
        .with_inserted_indices(Indices::U32(self.idx.clone()))
    }
}

fn lin(c: [u8; 3]) -> [f32; 4] {
    let f = |v: u8| {
        let s = v as f32 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    [f(c[0]), f(c[1]), f(c[2]), 1.0]
}

/// Terrain chunk (ci, cj) at LOD k from its grid, with skirts on all four edges.
pub fn terrain_chunk(
    t: &Terrain,
    grid: &[f32],
    gn: usize,
    k: u32,
    ci: usize,
    cj: usize,
) -> ChunkMesh {
    let step = 1usize << k;
    let per = CHUNK / step;
    let cell = SP * step as f32;
    let (vi0, vj0) = (ci * per, cj * per);
    let mut m = ChunkMesh::default();
    let h = |vi: usize, vj: usize| grid[vj.min(gn - 1) * gn + vi.min(gn - 1)];
    for vj in vj0..=vj0 + per {
        for vi in vi0..=vi0 + per {
            let y = h(vi, vj);
            let dx = h(vi + 1, vj) - h(vi.saturating_sub(1), vj);
            let dz = h(vi, vj + 1) - h(vi, vj.saturating_sub(1));
            let n = Vec3::new(-dx, 2.0 * cell, -dz).normalize();
            let gi = terrain::idx((vi * step).min(N - 1), (vj * step).min(N - 1));
            let slope = 1.0 - n.y;
            let col = if t.core[gi] {
                lin([0x8a, 0x76, 0x5c])
            } else if !t.water[gi].is_nan() {
                lin([0x9c, 0x8e, 0x6a])
            } else if slope > 0.18 {
                lin([0x86, 0x82, 0x7a])
            } else if (t.h[gi] - t.base[gi]).abs() > 0.05 {
                lin([0x7e, 0x93, 0x52])
            } else {
                lin([0x6b, 0x8f, 0x45])
            };
            m.vert(Vec3::new(vi as f32 * cell, y, vj as f32 * cell), n, col);
        }
    }
    let row = per + 1;
    let v = |i: usize, j: usize| (j * row + i) as u32;
    for j in 0..per {
        for i in 0..per {
            // Diagonal (i,j)–(i+1,j+1), CCW seen from above (+Y).
            m.idx.extend([
                v(i, j),
                v(i + 1, j + 1),
                v(i + 1, j),
                v(i, j),
                v(i, j + 1),
                v(i + 1, j + 1),
            ]);
        }
    }
    // Skirts.
    let edges: [Vec<(usize, usize)>; 4] = [
        (0..=per).map(|i| (i, 0)).collect(),
        (0..=per).map(|j| (per, j)).collect(),
        (0..=per).rev().map(|i| (i, per)).collect(),
        (0..=per).rev().map(|j| (0, j)).collect(),
    ];
    for e in edges {
        let mut prev: Option<(u32, u32)> = None;
        for (i, j) in e {
            let top = v(i, j);
            let p = Vec3::from_array(m.pos[top as usize]) - Vec3::Y * SKIRT_TERRAIN[k as usize];
            let n = Vec3::from_array(m.nrm[top as usize]);
            let c = m.col[top as usize];
            let bot = m.vert(p, n, c);
            if let Some((pt, pb)) = prev {
                m.idx.extend([pt, top, bot, pt, bot, pb]);
                m.idx.extend([pt, bot, top, pt, pb, bot]);
            }
            prev = Some((top, bot));
        }
    }
    m
}

/// LOD0 meshes for every chunk overlapping `rect` (timing test).
pub fn remesh_chunks(t: &Terrain, rect: (usize, usize, usize, usize)) -> Vec<ChunkMesh> {
    let mut out = Vec::new();
    let c0 = rect.0 / CHUNK;
    let c1 = (rect.2 / CHUNK).min((N - 1) / CHUNK - 1);
    let r0 = rect.1 / CHUNK;
    let r1 = (rect.3 / CHUNK).min((N - 1) / CHUNK - 1);
    for cj in r0..=r1 {
        for ci in c0..=c1 {
            out.push(terrain_chunk(t, &t.h, N, 0, ci, cj));
        }
    }
    out
}

fn road_mesh(r: &Road, m: &mut ChunkMesh) {
    let hw = r.hw();
    let asphalt = lin([0x6a, 0x6b, 0x70]);
    let walk = lin([0xa8, 0xa4, 0x9c]);
    let dirt = lin([0x9a, 0x80, 0x5e]);
    let len = r.len();
    let s_lo = if r.cut0 > 0.0 { r.cut0 } else { 0.0 };
    let s_hi = if r.cut1 > 0.0 { len - r.cut1 } else { len };
    // Cross-section (lateral offset, colour); walks 2 m on streets/avenues.
    let xs: Vec<(f32, [f32; 4])> = match r.kind {
        Kind::Dirt => vec![(-hw, dirt), (hw, dirt)],
        _ => vec![
            (-hw, walk),
            (-hw + 2.0, walk),
            (-hw + 2.0, asphalt),
            (hw - 2.0, asphalt),
            (hw - 2.0, walk),
            (hw, walk),
        ],
    };
    let mut prev: Option<Vec<u32>> = None;
    let mut s = s_lo;
    loop {
        let p = r.point_at(s);
        let d = r.dir_at(s);
        let nrm = G2::new(-d.y, d.x);
        let y = r.h(s) + 0.02;
        let mut row = Vec::new();
        for (off, c) in &xs {
            let q = p + nrm * *off;
            row.push(m.vert(Vec3::new(q.x, y, q.y), Vec3::Y, *c));
        }
        // Skirts down 0.6 m at both edges (on bridges they become the deck sides).
        let deck = if r.on_span(s) {
            r.kind.deck()
        } else {
            SKIRT_ROAD
        };
        for (off, c) in [(-hw, xs[0].1), (hw, xs[xs.len() - 1].1)] {
            let q = p + nrm * off;
            let side = Vec3::new(nrm.x, 0.0, nrm.y) * off.signum();
            row.push(m.vert(
                Vec3::new(q.x, y - deck, q.y),
                side,
                [c[0] * 0.7, c[1] * 0.7, c[2] * 0.7, 1.0],
            ));
        }
        if let Some(pr) = &prev {
            let n = xs.len();
            for k in 0..n - 1 {
                if xs[k].0 == xs[k + 1].0 {
                    continue;
                }
                m.idx
                    .extend([pr[k], pr[k + 1], row[k + 1], pr[k], row[k + 1], row[k]]);
            }
            // Left skirt: top pr[0],row[0]; bottom pr[n], row[n].
            let (lt0, lt1, lb0, lb1) = (pr[0], row[0], pr[n], row[n]);
            m.idx.extend([lt0, lb0, lb1, lt0, lb1, lt1]);
            m.idx.extend([lt0, lb1, lb0, lt0, lt1, lb1]);
            let (rt0, rt1, rb0, rb1) = (pr[n - 1], row[n - 1], pr[n + 1], row[n + 1]);
            m.idx.extend([rt0, rb1, rb0, rt0, rt1, rb1]);
            m.idx.extend([rt0, rb0, rb1, rt0, rb1, rt1]);
        }
        prev = Some(row);
        if s >= s_hi {
            break;
        }
        s = (s + 2.0).min(s_hi);
    }
    // Bridges: deck underside, railings, piers.
    let concrete = lin([0xb4, 0xb0, 0xa8]);
    for &(a, b) in &r.spans {
        let mut s = a;
        while s < b {
            let s2 = (s + 4.0).min(b);
            let (p1, p2) = (r.point_at(s), r.point_at(s2));
            let d = (p2 - p1).normalize_or(G2::X);
            let nrm = G2::new(-d.y, d.x);
            let (y1, y2) = (r.h(s) - r.kind.deck(), r.h(s2) - r.kind.deck());
            let q = |p: G2, o: f32, y: f32| Vec3::new(p.x + nrm.x * o, y, p.y + nrm.y * o);
            m.face(
                [q(p1, -hw, y1), q(p1, hw, y1), q(p2, hw, y2), q(p2, -hw, y2)],
                concrete,
            );
            for side in [-1.0f32, 1.0] {
                let o = side * (hw - 0.15);
                let mid = (p1 + p2) * 0.5;
                let c = Vec3::new(
                    mid.x + nrm.x * o,
                    0.5 * (r.h(s) + r.h(s2)) + 0.55,
                    mid.y + nrm.y * o,
                );
                m.boxy(
                    c,
                    Vec3::new(0.5 * (s2 - s), 0.5, 0.12),
                    Vec3::new(d.x, (r.h(s2) - r.h(s)) / (s2 - s), d.y),
                    concrete,
                );
            }
            s = s2;
        }
    }
    for (p, _, top) in &r.piers {
        let g = r.cl.iter().map(|_| 0.0f32).next().unwrap_or(0.0);
        let _ = g;
        let ground = PIER_GROUND.with(|f| f.get())(*p);
        let hgt = (top - ground).max(0.5);
        let dir = r.dir_at(r.project(*p).0);
        m.boxy(
            Vec3::new(p.x, ground + 0.5 * hgt - 1.0, p.y),
            Vec3::new(1.0, 0.5 * hgt + 1.0, hw - 1.0),
            Vec3::new(d3(dir).x, 0.0, d3(dir).z),
            concrete,
        );
    }
}

fn d3(v: G2) -> Vec3 {
    Vec3::new(v.x, 0.0, v.y)
}

thread_local! {
    static PIER_GROUND: std::cell::Cell<fn(G2) -> f32> = const { std::cell::Cell::new(|_| 0.0) };
}

fn junction_mesh(jn: &road::Junction, roads: &[Road], m: &mut ChunkMesh) {
    let asphalt = lin([0x6a, 0x6b, 0x70]);
    let rad = jn.cut + 0.3;
    let rings = 12;
    let segs = 72;
    let c = m.vert(
        Vec3::new(jn.node.x, jn.h + 0.02, jn.node.y),
        Vec3::Y,
        asphalt,
    );
    let mut prev: Vec<u32> = vec![c; segs];
    for k in 1..=rings {
        let rr = rad * k as f32 / rings as f32;
        let row: Vec<u32> = (0..segs)
            .map(|q| {
                let a = std::f32::consts::TAU * q as f32 / segs as f32;
                let p = jn.node + G2::new(a.cos(), a.sin()) * rr;
                m.vert(
                    Vec3::new(p.x, road::junction_surface(jn, roads, p) + 0.02, p.y),
                    Vec3::Y,
                    asphalt,
                )
            })
            .collect();
        for q in 0..segs {
            let q2 = (q + 1) % segs;
            if k == 1 {
                m.idx.extend([c, row[q2], row[q]]);
            } else {
                m.idx
                    .extend([prev[q], row[q2], row[q], prev[q], prev[q2], row[q2]]);
            }
        }
        prev = row;
    }
}

fn water_mesh(_t: &Terrain) -> ChunkMesh {
    let mut m = ChunkMesh::default();
    let col = [0.08, 0.20, 0.28, 1.0];
    let cell = 4.0f32;
    let n = (4096.0 / cell) as usize;
    for i in 0..n {
        let x0 = i as f32 * cell;
        let x1 = x0 + cell;
        let (z0, z1) = (terrain::river_z(x0), terrain::river_z(x1));
        let (w0, w1) = (terrain::river_level(x0), terrain::river_level(x1));
        let a = m.vert(Vec3::new(x0, w0, z0 - 40.0), Vec3::Y, col);
        let b = m.vert(Vec3::new(x1, w1, z1 - 40.0), Vec3::Y, col);
        let c = m.vert(Vec3::new(x1, w1, z1 + 40.0), Vec3::Y, col);
        let d = m.vert(Vec3::new(x0, w0, z0 + 40.0), Vec3::Y, col);
        m.idx.extend([a, c, b, a, d, c]);
    }
    m
}

#[derive(Clone)]
pub struct Shot {
    pub name: String,
    pub eye: Vec3,
    pub target: Vec3,
    /// Force this terrain LOD within 700 m of the camera (None = distance rings).
    pub lod: Option<u32>,
}

#[derive(Resource)]
struct ShotState {
    shots: Vec<Shot>,
    dir: String,
    i: usize,
    frame: u32,
    lods: Vec<(Vec<f32>, usize)>,
    world: SimWorld,
    terrain_mat: Handle<StandardMaterial>,
}

#[derive(Component)]
struct TerrainChunk;

fn feature_point(w: &SimWorld, ri: usize, s: f32) -> (G2, f32, G2) {
    let r = &w.roads[ri];
    (r.point_at(s), r.h(s), r.dir_at(s))
}

pub fn shot_list(w: &SimWorld, f: &Features) -> Vec<Shot> {
    let mut v = Vec::new();
    let v3 = |p: G2, y: f32| Vec3::new(p.x, y, p.y);
    let side = |d: G2| G2::new(-d.y, d.x);
    let span_mid = |ri: usize| -> f32 {
        let r = &w.roads[ri];
        r.spans
            .first()
            .map_or(0.5 * r.len(), |(a, b)| 0.5 * (a + b))
    };
    if let Some(&ri) = f.river_bridges.first() {
        let s = span_mid(ri);
        let (p, h, d) = feature_point(w, ri, s);
        v.push(Shot {
            name: "S-G-01-river-bridge".into(),
            eye: v3(p + side(d) * 110.0 - d * 60.0, h + 35.0),
            target: v3(p, h - 4.0),
            lod: None,
        });
        let r = &w.roads[ri];
        let a = r.spans.first().map_or(0.0, |x| x.0);
        let (pa, ha, da) = feature_point(w, ri, a);
        v.push(Shot {
            name: "S-G-02-river-bridge-abutment".into(),
            eye: v3(pa + side(da) * 28.0 - da * 18.0, ha + 3.0),
            target: v3(pa + da * 8.0, ha - 1.5),
            lod: None,
        });
    }
    if let Some(&ri) = f.valley_bridges.first() {
        let s = span_mid(ri);
        let (p, h, d) = feature_point(w, ri, s);
        v.push(Shot {
            name: "S-G-03-valley-bridge".into(),
            eye: v3(p + side(d) * 190.0 - d * 80.0, h + 25.0),
            target: v3(p, h - 15.0),
            lod: None,
        });
        v.push(Shot {
            name: "S-G-04-valley-bridge-lod1".into(),
            eye: v3(p + side(d) * 160.0 + d * 120.0, h + 45.0),
            target: v3(p, h - 10.0),
            lod: Some(1),
        });
    }
    if let (Some(&lo), Some(&up)) = (f.overpass_lower.first(), f.overpass_upper.first()) {
        let s = span_mid(up);
        let (p, h, d) = feature_point(w, up, s);
        v.push(Shot {
            name: "S-G-05-overpass".into(),
            eye: v3(p + side(d) * 90.0 - d * 70.0, h + 30.0),
            target: v3(p, h - 5.0),
            lod: None,
        });
        let (sl, _) = w.roads[lo].project(p);
        let (pl, hl, dl) = feature_point(w, lo, sl);
        v.push(Shot {
            name: "S-G-06-under-overpass".into(),
            eye: v3(pl - dl * 45.0 + side(dl) * 2.0, hl + 1.7),
            target: v3(pl + dl * 10.0, hl + 3.0),
            lod: None,
        });
    }
    for (k, &ji) in f.junctions.iter().take(2).enumerate() {
        let jn = &w.junctions[ji];
        let (d0, _) = (G2::new(0.6, 0.8), 0);
        let eye = jn.node + d0 * 55.0;
        v.push(Shot {
            name: if k == 0 {
                "S-G-07-junction-slope".into()
            } else {
                "S-G-08-junction-slope-lod1".into()
            },
            eye: v3(eye, w.height(eye) + 18.0),
            target: v3(jn.node, jn.h),
            lod: if k == 0 { None } else { Some(1) },
        });
    }
    for (k, (a, _b, _, _)) in f.pairs.iter().take(2).enumerate() {
        let r = &w.roads[*a];
        let (p, h, d) = feature_point(w, *a, 0.5 * r.len());
        v.push(Shot {
            name: if k == 0 {
                "S-G-09-parallel-pair".into()
            } else {
                "S-G-10-parallel-pair-lod2".into()
            },
            eye: v3(p - d * 70.0 - side(d) * 25.0, h + 22.0),
            target: v3(p + side(d) * 12.0, h),
            lod: if k == 0 { None } else { Some(2) },
        });
    }
    // Hillside road with the largest cut/fill (excluding bridges).
    let mut best = (0.0f32, 0usize, 0.0f32);
    for &ri in &f.random {
        let r = &w.roads[ri];
        if !r.spans.is_empty() {
            continue;
        }
        let mut s = 0.0;
        while s < r.len() {
            let e = (r.h(s) - terrain::interp(&w.t.base, N, SP, r.point_at(s))).abs();
            if e > best.0 {
                best = (e, ri, s);
            }
            s += 8.0;
        }
    }
    let (p, h, d) = feature_point(w, best.1, best.2);
    let e = p + side(d) * 45.0 - d * 40.0;
    v.push(Shot {
        name: "S-G-11-hillside-cut-fill".into(),
        eye: v3(e, (h + 14.0).max(w.height(e) + 12.0)),
        target: v3(p, h - 2.0),
        lod: None,
    });
    let c = G2::new(2048.0, 2048.0);
    v.push(Shot {
        name: "S-G-12-aerial-lod-rings".into(),
        eye: v3(c + G2::new(-900.0, -1300.0), 700.0),
        target: v3(c, 40.0),
        lod: None,
    });
    v
}

pub fn run(w: SimWorld, f: Features, dir: String) {
    let _ = std::fs::create_dir_all(&dir);
    let shots = shot_list(&w, &f);
    let lods: Vec<(Vec<f32>, usize)> = (0..4).map(|k| terrain::lod_grid(&w.t, k, true)).collect();
    // Pier ground lookup (LOD0 composite) for the pier meshes.
    static GROUND: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
    let _ = GROUND.set(w.t.h.clone());
    PIER_GROUND.with(|c| c.set(|p| GROUND.get().map_or(0.0, |g| terrain::interp(g, N, SP, p))));
    let mut statics = ChunkMesh::default();
    for r in &w.roads {
        road_mesh(r, &mut statics);
    }
    for jn in &w.junctions {
        junction_mesh(jn, &w.roads, &mut statics);
    }
    let water = water_mesh(&w.t);
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "S-G".into(),
            resolution: WindowResolution::new(1600, 900),
            resizable: false,
            ..default()
        }),
        ..default()
    }))
    .insert_resource(ClearColor(Color::srgb(0.62, 0.76, 0.88)))
    .insert_resource(GlobalAmbientLight {
        color: Color::srgb_u8(0xBF, 0xD0, 0xE2),
        brightness: 600.0,
        ..default()
    })
    .insert_resource(Statics(Some((statics, water))))
    .insert_resource(ShotState {
        shots,
        dir,
        i: 0,
        frame: 0,
        lods,
        world: w,
        terrain_mat: Handle::default(),
    })
    .add_systems(Startup, setup)
    .add_systems(Update, step);
    app.run();
}

#[derive(Resource)]
struct Statics(Option<(ChunkMesh, ChunkMesh)>);

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut st: ResMut<ShotState>,
    mut statics: ResMut<Statics>,
) {
    let mat = mats.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.95,
        ..default()
    });
    st.terrain_mat = mat.clone();
    let statics_mat = mats.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        cull_mode: None,
        double_sided: true,
        ..default()
    });
    if let Some((s, w)) = statics.0.take() {
        commands.spawn((
            Mesh3d(meshes.add(s.to_mesh())),
            MeshMaterial3d(statics_mat),
            NoFrustumCulling,
        ));
        commands.spawn((
            Mesh3d(meshes.add(w.to_mesh())),
            MeshMaterial3d(mats.add(StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 0.15,
                reflectance: 0.6,
                ..default()
            })),
            NoFrustumCulling,
        ));
    }
    commands.spawn((
        DirectionalLight {
            illuminance: 11_000.0,
            shadow_maps_enabled: true,
            color: Color::srgb_u8(0xFF, 0xF3, 0xE2),
            ..default()
        },
        Transform::from_xyz(1.0, 1.6, 0.7).looking_at(Vec3::ZERO, Vec3::Y),
        CascadeShadowConfigBuilder {
            num_cascades: 4,
            maximum_distance: 900.0,
            first_cascade_far_bound: 60.0,
            ..default()
        }
        .build(),
    ));
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 50f32.to_radians(),
            near: 0.5,
            far: 8000.0,
            ..default()
        }),
        Tonemapping::TonyMcMapface,
        Msaa::Sample4,
        DistanceFog {
            color: Color::srgba(0.70, 0.80, 0.90, 1.0),
            falloff: FogFalloff::Linear {
                start: 1500.0,
                end: 6000.0,
            },
            ..default()
        },
        Transform::default(),
    ));
}

fn step(
    mut commands: Commands,
    mut st: ResMut<ShotState>,
    mut meshes: ResMut<Assets<Mesh>>,
    chunks: Query<Entity, With<TerrainChunk>>,
    mut cam: Query<&mut Transform, With<Camera3d>>,
    mut exit: MessageWriter<AppExit>,
) {
    if st.i >= st.shots.len() {
        if st.frame > 20 {
            exit.write(AppExit::Success);
        }
        st.frame += 1;
        return;
    }
    if st.frame == 0 {
        let shot = st.shots[st.i].clone();
        for e in &chunks {
            commands.entity(e).despawn();
        }
        if let Ok(mut tf) = cam.single_mut() {
            *tf = Transform::from_translation(shot.eye).looking_at(shot.target, Vec3::Y);
        }
        let per_side = (N - 1) / CHUNK;
        let mut counts = [0usize; 4];
        for cj in 0..per_side {
            for ci in 0..per_side {
                let c = Vec3::new(
                    (ci as f32 + 0.5) * CHUNK as f32 * SP,
                    0.0,
                    (cj as f32 + 0.5) * CHUNK as f32 * SP,
                );
                let d = Vec2::new(c.x - shot.eye.x, c.z - shot.eye.z).length();
                let mut k = if d < 350.0 {
                    0
                } else if d < 900.0 {
                    1
                } else if d < 2000.0 {
                    2
                } else {
                    3
                };
                if let Some(f) = shot.lod
                    && d < 700.0
                {
                    k = f;
                }
                counts[k as usize] += 1;
                let (g, gn) = &st.lods[k as usize];
                let m = terrain_chunk(&st.world.t, g, *gn, k, ci, cj);
                commands.spawn((
                    Mesh3d(meshes.add(m.to_mesh())),
                    MeshMaterial3d(st.terrain_mat.clone()),
                    TerrainChunk,
                    NoFrustumCulling,
                ));
            }
        }
        println!("shot {} lod_chunks={counts:?}", shot.name);
    }
    st.frame += 1;
    let wait = if st.i == 0 { 150 } else { 25 };
    if st.frame == wait {
        let path = format!("{}/{}.png", st.dir, st.shots[st.i].name);
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
    if st.frame == wait + 15 {
        st.i += 1;
        st.frame = 0;
    }
}
