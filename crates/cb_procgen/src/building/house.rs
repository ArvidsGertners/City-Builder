//! Residential L1–L3 (GAME_DESIGN §4): L1 cottage (1–2 floors), L2 detached house (2 floors,
//! optional rear wing), L3 row house / duplex (2–3 floors spanning the footprint). Built on the
//! fitted footprint rectangle (`cb_geometry::rect_fit`), whose first edge faces the road.

use cb_core::rng::Pcg32;
use cb_geometry::MeshData;
use glam::{Vec2, Vec3};

use super::foundation::{self, Foundation, FoundationKind};
use super::roofs::{self, RoofKind, RoofSpec};
use crate::paint::{EAVE_AO, Frame, Painter};
use crate::palette::{self, Rgb};
use crate::surface::{MAX_ID, Surface};

/// Residential floor height (m).
pub const FLOOR_H_M: f32 = 3.0;
/// LOD0 triangle budget of a house (ART_DIRECTION §2: houses 300–1 200).
pub const TRI_BUDGET: usize = 1200;
/// Window bay (m): walls get `⌊len / bay⌋` windows per floor.
const BAY_M: f32 = 2.9;
/// Walls shorter than this stay blank (m).
const MIN_WINDOW_WALL_M: f32 = 2.4;

/// One rectangular volume in the house frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Volume {
    /// `[u0, v0, u1, v1]` (u along the street, v into the lot).
    pub rect: [f32; 4],
    /// Floors.
    pub floors: u8,
    /// Roof.
    pub roof: RoofSpec,
    /// Wall colour.
    pub wall: Rgb,
}

/// Massing and style of one house (deterministic from footprint, level and seed).
#[derive(Clone, Debug, PartialEq)]
pub struct HouseLayout {
    /// Local frame: origin at the footprint's front-left corner, `ex` along the street,
    /// `ey` into the lot.
    pub frame: Frame,
    /// Level 1–3.
    pub level: u8,
    /// Main volume first, then the wing (L2) or the second half (L3 duplex).
    pub volumes: Vec<Volume>,
    /// Roof colour.
    pub roof_c: Rgb,
    /// Door / shutter accent.
    pub accent: Rgb,
    /// Shutters on the windows.
    pub shutters: bool,
    /// Door positions (local u on the front wall, v = front of the volume).
    pub doors: Vec<(f32, f32)>,
    /// Chimney (local u, v) if any.
    pub chimney: Option<Vec2>,
    /// Free strip beside the house for a driveway: local `[u0, u1]`.
    pub driveway: Option<[f32; 2]>,
    /// Footprint depth (m), for the yard.
    pub footprint_depth_m: f32,
    /// Seed for mesh details.
    pub seed: u64,
}

impl HouseLayout {
    /// World outline quads of every volume (for picking, overlap tests and the pad stamp).
    #[must_use]
    pub fn outlines(&self) -> Vec<[Vec2; 4]> {
        self.volumes
            .iter()
            .map(|v| self.frame.rect(v.rect))
            .collect()
    }

    /// Ground sample points for the foundation (corners and centres of every volume).
    #[must_use]
    pub fn sample_points(&self) -> Vec<Vec2> {
        let mut pts = Vec::new();
        for q in self.outlines() {
            pts.extend_from_slice(&q);
            pts.push((q[0] + q[2]) * 0.5);
        }
        pts
    }

    /// Convex outline of all volumes (pad stamp polygon).
    #[must_use]
    pub fn hull(&self) -> Vec<Vec2> {
        let pts: Vec<Vec2> = self.outlines().into_iter().flatten().collect();
        cb_geometry::polygon::convex_hull(&pts)
    }

    /// Height of the highest ridge above the floor (m).
    #[must_use]
    pub fn height_m(&self) -> f32 {
        self.volumes
            .iter()
            .map(|v| f32::from(v.floors) * FLOOR_H_M + roofs::rise(&v.roof, v.rect))
            .fold(0.0, f32::max)
    }

    /// Plans the foundation from the ground under the house (`None` if too steep).
    #[must_use]
    pub fn plan_foundation(&self, ground: &dyn Fn(Vec2) -> f32) -> Option<Foundation> {
        let samples: Vec<f32> = self.sample_points().iter().map(|p| ground(*p)).collect();
        foundation::plan(&self.hull(), &samples, true)
    }
}

fn range(rng: &mut Pcg32, lo: f32, hi: f32) -> f32 {
    lo + (hi - lo) * rng.next_f32()
}

fn deg_slope(deg: f32) -> f32 {
    cb_core::math::tan(cb_core::math::to_radians(deg))
}

/// Lays out a house on the footprint `[front-left, front-right, back-right, back-left]`
/// (counter-clockwise; first edge on the street side).
#[must_use]
pub fn layout(footprint: [Vec2; 4], level: u8, seed: u64) -> HouseLayout {
    let mut rng = Pcg32::from_seed(seed);
    let level = level.clamp(1, 3);
    let w_fp = footprint[0].distance(footprint[1]);
    let d_fp = footprint[0].distance(footprint[3]);
    let frame = Frame {
        o: footprint[0],
        ex: (footprint[1] - footprint[0]).normalize_or(Vec2::X),
        ey: (footprint[3] - footprint[0]).normalize_or(Vec2::Y),
    };
    let wall = palette::pick(&mut rng, &palette::R_WALLS).building_variant(&mut rng);
    let roof_c = palette::pick(&mut rng, &palette::R_ROOFS).building_variant(&mut rng);
    let accent = palette::pick(&mut rng, &palette::R_ACCENTS);
    let eave = 0.35;
    let verge = 0.25;
    let thickness = 0.16;
    let spec = |kind, deg: f32| RoofSpec {
        kind,
        slope: deg_slope(deg),
        eave_m: eave,
        verge_m: verge,
        thickness_m: thickness,
    };

    let (w, d, floors, roof) = match level {
        1 => {
            let w = w_fp.min(range(&mut rng, 7.0, 9.0));
            let d = d_fp.min(range(&mut rng, 6.5, 8.5));
            let floors = if rng.chance(0.35) { 2 } else { 1 };
            let ridge_u = rng.chance(0.7) || w >= d + 1.0;
            (
                w,
                d,
                floors,
                spec(RoofKind::Gable { ridge_u }, range(&mut rng, 40.0, 47.0)),
            )
        }
        2 => {
            let w = w_fp.min(range(&mut rng, 9.0, 12.0));
            let d = d_fp.min(range(&mut rng, 8.0, 10.0));
            let kind = if rng.chance(0.5) {
                RoofKind::Hip
            } else {
                RoofKind::Gable { ridge_u: w >= d }
            };
            (w, d, 2, spec(kind, range(&mut rng, 30.0, 40.0)))
        }
        _ => {
            let w = w_fp.min(range(&mut rng, 12.0, 16.0));
            let d = d_fp.min(range(&mut rng, 8.5, 11.0));
            // Three floors only on wide footprints (narrow ones read as towers).
            let floors = if w >= 11.0 && rng.chance(0.4) { 3 } else { 2 };
            (
                w,
                d,
                floors,
                spec(
                    RoofKind::Gable { ridge_u: true },
                    range(&mut rng, 32.0, 38.0),
                ),
            )
        }
    };
    // Lateral placement: leave a driveway strip on one side when there is room.
    let spare = w_fp - w;
    let (u0, driveway) = if spare >= 3.2 {
        if rng.chance(0.5) {
            (0.0, Some([w, w_fp]))
        } else {
            (spare, Some([0.0, spare]))
        }
    } else {
        (0.5 * spare, None)
    };
    let main = [u0, 0.0, u0 + w, d];
    let mut volumes = Vec::with_capacity(2);
    let mut doors = Vec::new();
    if level == 3 && w >= 10.0 && rng.chance(0.7) {
        // Duplex: two halves with sibling colours under one continuous roof line.
        let mid = u0 + 0.5 * w;
        let wall2 = palette::pick(&mut rng, &palette::R_WALLS).building_variant(&mut rng);
        volumes.push(Volume {
            rect: [u0, 0.0, mid, d],
            floors,
            roof,
            wall,
        });
        volumes.push(Volume {
            rect: [mid, 0.0, u0 + w, d],
            floors,
            roof,
            wall: wall2,
        });
        doors.push((u0 + 0.25 * w + range(&mut rng, -0.8, 0.8), 0.0));
        doors.push((u0 + 0.75 * w + range(&mut rng, -0.8, 0.8), 0.0));
    } else {
        volumes.push(Volume {
            rect: main,
            floors,
            roof,
            wall,
        });
        let du = if w > 6.0 {
            range(&mut rng, 0.3, 0.7)
        } else {
            0.5
        };
        doors.push((u0 + du * w, 0.0));
    }
    // L2 rear wing (≥ 25 m²) when the footprint is deeper than the main volume.
    if level == 2 && d_fp - d >= 3.5 && rng.chance(0.65) {
        let ww = (w * range(&mut rng, 0.45, 0.6)).max(4.5).min(w);
        let wd = (d_fp - d).min(range(&mut rng, 4.5, 6.5));
        if ww * wd >= 25.0 {
            let left = rng.chance(0.5);
            let wu0 = if left { u0 } else { u0 + w - ww };
            let wing_roof = spec(
                RoofKind::Gable { ridge_u: false },
                range(&mut rng, 30.0, 40.0),
            );
            // The wing starts inside the main volume so their roofs interpenetrate cleanly.
            volumes.push(Volume {
                rect: [wu0, d - 1.0, wu0 + ww, d + wd],
                floors: 1,
                roof: wing_roof,
                wall,
            });
        }
    }
    let chimney = rng.chance(0.75).then(|| {
        let v = &volumes[0];
        Vec2::new(
            range(&mut rng, v.rect[0] + 1.2, v.rect[2] - 1.2),
            0.5 * (v.rect[1] + v.rect[3]) + range(&mut rng, 0.4, 1.2),
        )
    });
    HouseLayout {
        frame,
        level,
        volumes,
        roof_c,
        accent,
        shutters: level < 3 && rng.chance(0.5),
        doors,
        chimney,
        driveway,
        footprint_depth_m: d_fp,
        seed: rng.next_u64(),
    }
}

/// Builds LOD0 and LOD1 meshes of a house on its foundation.
#[must_use]
pub fn mesh(
    l: &HouseLayout,
    f: &Foundation,
    ground: &dyn Fn(Vec2) -> f32,
    slot: u16,
) -> (MeshData, MeshData) {
    let mut p = Painter::new(slot, l.seed);
    p.contact_y = f.ground_min;
    let fr = &l.frame;
    let floor_y = f.floor_y;
    let mut window_id = 0u32;

    // Foundation.
    match f.kind {
        FoundationKind::Pad | FoundationKind::Plinth => {
            let bottom = f
                .pad
                .as_ref()
                .map_or(f.ground_min, |pd| pd.target_h_m.min(f.ground_min))
                - 0.5;
            for v in &l.volumes {
                let r = v.rect;
                let q = fr.rect([r[0] - 0.08, r[1] - 0.08, r[2] + 0.08, r[3] + 0.08]);
                p.prism(&q, bottom, floor_y, palette::STONE, None, false);
            }
        }
        FoundationKind::Stilts => {
            for v in &l.volumes {
                let r = v.rect;
                p.frame_box(fr, r, floor_y - 0.35, floor_y, palette::STONE, None);
                let nu = ((r[2] - r[0]) / 3.0).ceil().max(1.0) as usize;
                let nv = ((r[3] - r[1]) / 3.0).ceil().max(1.0) as usize;
                for i in 0..=nu {
                    for j in 0..=nv {
                        if i != 0 && i != nu && j != 0 && j != nv {
                            continue;
                        }
                        let u = r[0] + 0.15 + (r[2] - r[0] - 0.3) * i as f32 / nu as f32;
                        let vv = r[1] + 0.15 + (r[3] - r[1] - 0.3) * j as f32 / nv as f32;
                        let g = ground(fr.at(u, vv));
                        p.frame_box(
                            fr,
                            [u - 0.13, vv - 0.13, u + 0.13, vv + 0.13],
                            g - 0.3,
                            floor_y - 0.35,
                            palette::WOOD[2],
                            None,
                        );
                    }
                }
            }
        }
    }

    // Volumes: walls, windows, roof.
    let mut lod1 = Painter::new(slot, l.seed ^ 1);
    for (vi, v) in l.volumes.iter().enumerate() {
        let r = v.rect;
        let top = floor_y + f32::from(v.floors) * FLOOR_H_M;
        let q = fr.rect(r);
        p.prism(&q, floor_y, top, v.wall, None, true);
        lod1.prism(&q, floor_y, top, v.wall, None, false);
        let gable_ends: [bool; 4] = match v.roof.kind {
            RoofKind::Gable { ridge_u: true } => [false, true, false, true],
            RoofKind::Gable { ridge_u: false } => [true, false, true, false],
            RoofKind::Hip => [false; 4],
        };
        for k in 0..4 {
            let (a, b) = (q[k], q[(k + 1) % 4]);
            let len = a.distance(b);
            // The wing's front wall is inside the main volume; duplex halves share a wall.
            let hidden = (vi > 0 && k == 0 && l.level == 2)
                || (l.volumes.len() == 2
                    && l.level == 3
                    && ((vi == 0 && k == 1) || (vi == 1 && k == 3)));
            if hidden || len < MIN_WINDOW_WALL_M {
                continue;
            }
            let e = (b - a) / len;
            let out = Vec2::new(e.y, -e.x);
            let n_win = (len / BAY_M).floor().max(1.0) as usize;
            for fl in 0..v.floors {
                let y0 = floor_y + f32::from(fl) * FLOOR_H_M;
                for i in 0..n_win {
                    let t = (i as f32 + 0.5) / n_win as f32;
                    let c = a + (b - a) * t;
                    // Keep the door bay free on the ground floor of the front wall.
                    if fl == 0
                        && k == 0
                        && l.doors.iter().any(|d| fr.at(d.0, d.1).distance(c) < 1.3)
                    {
                        continue;
                    }
                    window(&mut p, c, e, out, y0 + 0.9, 1.0, 1.35, l, &mut window_id);
                }
            }
            // Attic window in gable ends of tall roofs.
            if gable_ends[k] && roofs::rise(&v.roof, r) > 2.6 {
                window(
                    &mut p,
                    (a + b) * 0.5,
                    e,
                    out,
                    top + 0.5,
                    0.8,
                    0.9,
                    l,
                    &mut window_id,
                );
            }
        }
        // A duplex gets one continuous roof over both halves (coplanar slabs would z-fight).
        let duplex = l.level == 3 && l.volumes.len() == 2;
        if duplex && vi == 1 {
            continue;
        }
        let (rr, gable) = if duplex {
            let o = &l.volumes[1];
            ([r[0], r[1], o.rect[2], r[3]], [v.wall, o.wall])
        } else {
            (r, [v.wall; 2])
        };
        roofs::roof(&mut p, fr, rr, top, &v.roof, l.roof_c, gable);
        lod1_roof(&mut lod1, fr, rr, top, &v.roof, l.roof_c, gable);
    }

    // Doors with step and canopy.
    let main = l.volumes[0].rect;
    for &(u, _) in &l.doors {
        let v0 = main[1];
        let a = fr.at(u - 0.5, v0);
        let b = fr.at(u + 0.5, v0);
        let out = -fr.ey;
        let o3 = Vec3::new(out.x, 0.0, out.y);
        let lift = |q: Vec2, y: f32| Vec3::new(q.x + out.x * 0.04, y, q.y + out.y * 0.04);
        p.face_ao(
            &[
                lift(a, floor_y),
                lift(b, floor_y),
                lift(b, floor_y + 2.15),
                lift(a, floor_y + 2.15),
            ],
            &[0.9, 0.9, 1.0, 1.0],
            l.accent,
            Surface::Solid,
            0,
            o3,
        );
        let g = ground(fr.at(u, v0 - 0.6));
        if floor_y - g > 0.15 {
            p.frame_box(
                fr,
                [u - 0.8, v0 - 0.9, u + 0.8, v0],
                g - 0.2,
                floor_y,
                palette::STONE,
                Some(palette::STONE),
            );
        }
        if l.level >= 2 {
            let y = floor_y + 2.5;
            p.frame_box(
                fr,
                [u - 0.9, v0 - 0.8, u + 0.9, v0],
                y,
                y + 0.12,
                l.roof_c,
                Some(l.roof_c),
            );
        }
    }

    // Chimney through the roof.
    if let Some(c) = l.chimney {
        let v = &l.volumes[0];
        let top = floor_y + f32::from(v.floors) * FLOOR_H_M + roofs::rise(&v.roof, v.rect) + 0.7;
        let base = floor_y + f32::from(v.floors) * FLOOR_H_M;
        p.frame_box(
            fr,
            [c.x - 0.3, c.y - 0.3, c.x + 0.3, c.y + 0.3],
            base,
            top,
            palette::BRICK,
            None,
        );
        p.frame_box(
            fr,
            [c.x - 0.38, c.y - 0.38, c.x + 0.38, c.y + 0.38],
            top,
            top + 0.12,
            palette::STONE,
            Some(palette::STONE),
        );
    }
    (p.finish(), lod1.finish())
}

/// Window: frame and glass quads standing slightly proud of the wall, optional shutters.
fn window(
    p: &mut Painter,
    c: Vec2,
    e: Vec2,
    out: Vec2,
    y0: f32,
    w: f32,
    h: f32,
    l: &HouseLayout,
    id: &mut u32,
) {
    let o3 = Vec3::new(out.x, 0.0, out.y);
    let at = |s: f32, y: f32, d: f32| {
        let q = c + e * s + out * d;
        Vec3::new(q.x, y, q.y)
    };
    let (hw, fw) = (0.5 * w, 0.08);
    p.face_ao(
        &[
            at(-hw - fw, y0 - fw, 0.03),
            at(hw + fw, y0 - fw, 0.03),
            at(hw + fw, y0 + h + fw, 0.03),
            at(-hw - fw, y0 + h + fw, 0.03),
        ],
        &[1.0; 4],
        palette::WINDOW_FRAME,
        Surface::Solid,
        0,
        o3,
    );
    p.face_ao(
        &[
            at(-hw, y0, 0.05),
            at(hw, y0, 0.05),
            at(hw, y0 + h, 0.05),
            at(-hw, y0 + h, 0.05),
        ],
        &[1.0; 4],
        palette::GLASS,
        Surface::Window,
        *id & MAX_ID,
        o3,
    );
    *id += 1;
    // Sill (top and front face) and mullion cross.
    let sill = [
        at(-hw - 0.12, y0 - fw - 0.06, 0.0),
        at(hw + 0.12, y0 - fw - 0.06, 0.0),
    ];
    let sill_out = |q: Vec3| q + o3 * 0.12;
    p.face_ao(
        &[sill[0], sill[1], sill_out(sill[1]), sill_out(sill[0])],
        &[1.0; 4],
        palette::STONE.scale(1.15),
        Surface::Solid,
        0,
        Vec3::Y,
    );
    let lower = |q: Vec3| q - Vec3::Y * 0.07;
    p.face_ao(
        &[
            lower(sill_out(sill[0])),
            lower(sill_out(sill[1])),
            sill_out(sill[1]),
            sill_out(sill[0]),
        ],
        &[0.9, 0.9, 1.0, 1.0],
        palette::STONE.scale(1.15),
        Surface::Solid,
        0,
        o3,
    );
    if w >= 0.9 {
        let m = 0.035;
        p.face_ao(
            &[
                at(-m, y0, 0.06),
                at(m, y0, 0.06),
                at(m, y0 + h, 0.06),
                at(-m, y0 + h, 0.06),
            ],
            &[1.0; 4],
            palette::WINDOW_FRAME,
            Surface::Solid,
            0,
            o3,
        );
        let ty = y0 + 0.62 * h;
        p.face_ao(
            &[
                at(-hw, ty - m, 0.06),
                at(hw, ty - m, 0.06),
                at(hw, ty + m, 0.06),
                at(-hw, ty + m, 0.06),
            ],
            &[1.0; 4],
            palette::WINDOW_FRAME,
            Surface::Solid,
            0,
            o3,
        );
    }
    if l.shutters {
        for s in [-1.0, 1.0] {
            let (x0, x1) = if s < 0.0 {
                (-hw - fw - 0.45, -hw - fw - 0.02)
            } else {
                (hw + fw + 0.02, hw + fw + 0.45)
            };
            p.face_ao(
                &[
                    at(x0, y0, 0.04),
                    at(x1, y0, 0.04),
                    at(x1, y0 + h, 0.04),
                    at(x0, y0 + h, 0.04),
                ],
                &[1.0 - 0.5 * EAVE_AO; 4],
                l.accent,
                Surface::Solid,
                0,
                o3,
            );
        }
    }
}

/// LOD1 roof: top planes only (gable ends included).
fn lod1_roof(
    p: &mut Painter,
    fr: &Frame,
    r: [f32; 4],
    eave_y: f32,
    spec: &RoofSpec,
    roof_c: Rgb,
    gable_c: [Rgb; 2],
) {
    let simple = RoofSpec {
        eave_m: 0.2,
        verge_m: 0.1,
        thickness_m: 0.0,
        ..*spec
    };
    let before = p.mesh.triangle_count();
    roofs::roof(p, fr, r, eave_y, &simple, roof_c, gable_c);
    // Drop soffits and fascias (zero-thickness slabs): keep only up-facing and gable faces.
    let mut keep = Vec::with_capacity(p.mesh.indices.len());
    for k in 0..p.mesh.triangle_count() {
        let tri = &p.mesh.indices[3 * k..3 * k + 3];
        if k < before || p.mesh.face_normal(k).y > -0.05 && !is_degenerate(&p.mesh, tri) {
            keep.extend_from_slice(tri);
        }
    }
    p.mesh.indices = keep;
}

fn is_degenerate(m: &MeshData, tri: &[u32]) -> bool {
    let q = |i: u32| Vec3::from_array(m.positions[i as usize]);
    (q(tri[1]) - q(tri[0]))
        .cross(q(tri[2]) - q(tri[0]))
        .length_squared()
        < 1e-8
}
