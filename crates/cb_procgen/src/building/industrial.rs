//! Industrial L1–L3 (GAME_DESIGN §4): L1 workshop (small hall, shallow gable, roll-up door,
//! office annex), L2 factory hall (sawtooth roof with north lights, loading docks), L3 plant
//! (large flat hall, storage tanks, silos and chimney stacks). Built on the fitted hall
//! rectangle (`rect_fit::RectSpec::HALL`, first edge on the street side); yard equipment is
//! placed inside the setback-inset polygon.

use cb_core::rng::Pcg32;
use cb_geometry::MeshData;
use glam::{Vec2, Vec3};

use super::foundation::{self, Foundation, FoundationKind};
use super::roofs::{self, RoofKind, RoofSpec};
use crate::paint::{Frame, Painter};
use crate::palette::{self, Rgb};
use crate::surface::Surface;

/// LOD0 triangle budget of an industrial building.
pub const TRI_BUDGET: usize = 3200;
/// Sides of tanks, silos and stacks.
const ROUND_SIDES: usize = 12;

/// A round structure next to the hall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Round {
    /// Plan centre.
    pub c: Vec2,
    /// Radius (m).
    pub r: f32,
    /// Height above the floor level (m).
    pub h: f32,
    /// Shape.
    pub kind: RoundKind,
}

/// Round structure kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundKind {
    /// Squat storage tank with a flat lid.
    Tank,
    /// Tall silo with a cone top.
    Silo,
    /// Thin chimney stack with a dark rim.
    Stack,
}

/// Hall roof forms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HallRoof {
    /// Shallow gable (workshops).
    Gable,
    /// Sawtooth with glazed north lights (factory halls).
    Sawtooth,
    /// Flat with a parapet (plants).
    Flat,
}

/// Massing of one industrial building.
#[derive(Clone, Debug, PartialEq)]
pub struct IndustrialLayout {
    /// Hall frame: origin at the hall's front-left corner, `ex` along the street.
    pub frame: Frame,
    /// Level 1–3.
    pub level: u8,
    /// Hall rectangle `[u0, v0, u1, v1]` in the frame.
    pub hall: [f32; 4],
    /// Hall wall height (m).
    pub hall_h: f32,
    /// Roof form.
    pub roof: HallRoof,
    /// Office annex rectangle on the street side and its height.
    pub annex: Option<([f32; 4], f32)>,
    /// Tanks, silos, stacks.
    pub rounds: Vec<Round>,
    /// Roll-up doors on the front wall: centre `u`, width.
    pub doors: Vec<(f32, f32)>,
    /// Colours.
    pub wall: Rgb,
    /// Roof colour.
    pub roof_c: Rgb,
    /// Doors and trims.
    pub accent: Rgb,
    /// Seed for mesh details.
    pub seed: u64,
}

fn range(rng: &mut Pcg32, lo: f32, hi: f32) -> f32 {
    lo + (hi - lo) * rng.next_f32()
}

/// Plans an industrial building on the fitted hall rectangle; `inset` bounds yard equipment.
#[must_use]
pub fn layout(
    footprint: [Vec2; 4],
    inset: &[Vec2],
    level: u8,
    seed: u64,
) -> Option<IndustrialLayout> {
    let mut rng = Pcg32::from_seed(seed);
    let level = level.clamp(1, 3);
    let w_fp = footprint[0].distance(footprint[1]);
    let d_fp = footprint[0].distance(footprint[3]);
    let frame = Frame {
        o: footprint[0],
        ex: (footprint[1] - footprint[0]).normalize_or(Vec2::X),
        ey: (footprint[3] - footprint[0]).normalize_or(Vec2::Y),
    };
    // Hall size caps per level; plants keep room beside the hall for tanks.
    let (w_max, d_max, h, roof) = match level {
        1 => (
            range(&mut rng, 14.0, 22.0),
            range(&mut rng, 12.0, 18.0),
            range(&mut rng, 5.0, 6.5),
            HallRoof::Gable,
        ),
        2 => (
            range(&mut rng, 24.0, 40.0),
            range(&mut rng, 18.0, 30.0),
            range(&mut rng, 7.0, 9.0),
            HallRoof::Sawtooth,
        ),
        _ => (
            0.62 * w_fp,
            range(&mut rng, 22.0, 34.0),
            range(&mut rng, 8.0, 10.0),
            HallRoof::Flat,
        ),
    };
    let (w, d) = (w_fp.min(w_max), d_fp.min(d_max));
    if w < 8.0 || d < 8.0 {
        return None;
    }
    let u0 = if level == 3 { 0.0 } else { 0.5 * (w_fp - w) };
    let hall = [u0, 0.0, u0 + w, d];
    let wall = palette::pick(&mut rng, &palette::I_WALLS).building_variant(&mut rng);
    let roof_c = palette::pick(&mut rng, &palette::I_ROOFS).building_variant(&mut rng);
    let accent = palette::pick(&mut rng, &palette::I_ACCENTS);
    // Office annex in front of one end of the hall (workshops and factories).
    let annex = (level <= 2 && d >= 12.0).then(|| {
        let aw = (0.3 * w).clamp(5.0, 10.0);
        let ad = 5.0f32.min(0.35 * d);
        let left = rng.chance(0.5);
        let r = if left {
            [u0, 0.0, u0 + aw, ad]
        } else {
            [u0 + w - aw, 0.0, u0 + w, ad]
        };
        (r, 3.4)
    });
    // Roll-up doors along the front wall, clear of the annex.
    let mut doors = Vec::new();
    let n_doors = match level {
        1 => 1,
        2 => 2 + rng.below(2) as usize,
        _ => 2,
    };
    for k in 0..n_doors {
        let u = u0 + w * (k as f32 + 0.5) / n_doors as f32;
        let dw = (0.6 * w / n_doors as f32).min(4.5);
        let blocked = annex.is_some_and(|(r, _)| u + 0.5 * dw > r[0] && u - 0.5 * dw < r[2]);
        if !blocked {
            doors.push((u, dw));
        }
    }
    // Tanks, silos and stacks on the free part of the inset.
    let mut rounds = Vec::new();
    let hall_q = frame.rect(hall);
    if level >= 2 {
        let want = if level == 3 {
            3 + rng.below(3) as usize
        } else {
            usize::from(rng.chance(0.5))
        };
        let mut tries = 0;
        while rounds.len() < want && tries < 60 {
            tries += 1;
            let kind = match rng.below(4) {
                0 => RoundKind::Stack,
                1 => RoundKind::Silo,
                _ => RoundKind::Tank,
            };
            let (r, hh) = match kind {
                RoundKind::Tank => (range(&mut rng, 2.5, 4.5), range(&mut rng, 4.0, 7.0)),
                RoundKind::Silo => (range(&mut rng, 1.8, 2.8), range(&mut rng, 12.0, 18.0)),
                RoundKind::Stack => (range(&mut rng, 0.7, 1.1), range(&mut rng, 20.0, 30.0)),
            };
            let (lo, hi) = cb_geometry::polygon::bounds(inset);
            let c = Vec2::new(range(&mut rng, lo.x, hi.x), range(&mut rng, lo.y, hi.y));
            let clear_of_hall = cb_geometry::polygon::boundary_distance(&hall_q, c) > r + 1.0
                && !cb_geometry::polygon::contains(&hall_q, c);
            let inside = cb_geometry::polygon::contains(inset, c)
                && cb_geometry::polygon::boundary_distance(inset, c) > r + 0.3;
            let apart = rounds
                .iter()
                .all(|o: &Round| o.c.distance(c) > o.r + r + 1.0);
            // A stack may stand against the hall wall (it is thin and tall).
            let stack_ok = kind == RoundKind::Stack
                && !cb_geometry::polygon::contains(&hall_q, c)
                && cb_geometry::polygon::boundary_distance(&hall_q, c) > r + 0.2;
            if inside && apart && (clear_of_hall || stack_ok) {
                rounds.push(Round { c, r, h: hh, kind });
            }
        }
        // Plants always get one stack; if no free spot was found, put it on the hall roof.
        if level == 3 && !rounds.iter().any(|o| o.kind == RoundKind::Stack) {
            rounds.push(Round {
                c: frame.at(hall[0] + 0.75 * w, hall[1] + 0.7 * d),
                r: 0.9,
                h: h + range(&mut rng, 12.0, 18.0),
                kind: RoundKind::Stack,
            });
        }
    }
    Some(IndustrialLayout {
        frame,
        level,
        hall,
        hall_h: h,
        roof,
        annex,
        rounds,
        doors,
        wall,
        roof_c,
        accent,
        seed: rng.next_u64(),
    })
}

/// Sawtooth tooth width along the hall's depth (m).
const TOOTH_M: f32 = 6.0;
/// Sawtooth rise (m).
const TOOTH_RISE_M: f32 = 2.2;
/// Gable pitch (tan 15°).
const GABLE_SLOPE: f32 = 0.268;

impl IndustrialLayout {
    /// World outline quads of hall and annex (picking, pad stamp).
    #[must_use]
    pub fn outlines(&self) -> Vec<Vec<Vec2>> {
        let mut v = vec![self.frame.rect(self.hall).to_vec()];
        if let Some((r, _)) = self.annex {
            v.push(self.frame.rect(r).to_vec());
        }
        v
    }

    /// Height above the floor level (m), stacks included.
    #[must_use]
    pub fn height_m(&self) -> f32 {
        let d = self.hall[3] - self.hall[1];
        let roof = match self.roof {
            HallRoof::Gable => 0.5 * d * GABLE_SLOPE,
            HallRoof::Sawtooth => TOOTH_RISE_M,
            HallRoof::Flat => 0.6,
        };
        let rounds = self
            .rounds
            .iter()
            .map(|r| r.h + if r.kind == RoundKind::Silo { r.r } else { 0.0 })
            .fold(0.0, f32::max);
        (self.hall_h + roof).max(rounds)
    }

    /// Foundation from the ground under hall, annex and round structures (no stilts).
    #[must_use]
    pub fn plan_foundation(&self, ground: &dyn Fn(Vec2) -> f32) -> Option<Foundation> {
        let mut pts: Vec<Vec2> = self.outlines().into_iter().flatten().collect();
        for r in &self.rounds {
            pts.push(r.c);
        }
        let hull = cb_geometry::polygon::convex_hull(&pts);
        let samples: Vec<f32> = pts.iter().map(|p| ground(*p)).collect();
        foundation::plan(&hull, &samples, false)
    }
}

fn round_poly(c: Vec2, r: f32) -> Vec<Vec2> {
    round_poly_n(c, r, ROUND_SIDES)
}

fn round_poly_n(c: Vec2, r: f32, n: usize) -> Vec<Vec2> {
    (0..n)
        .map(|k| {
            let a = k as f32 * std::f32::consts::TAU / n as f32;
            let (s, co) = cb_core::math::sin_cos(a);
            c + Vec2::new(co, s) * r
        })
        .collect()
}

/// Builds LOD0 and LOD1 meshes on the foundation.
#[must_use]
pub fn mesh(l: &IndustrialLayout, f: &Foundation, slot: u16) -> (MeshData, MeshData) {
    let mut p = Painter::new(slot, l.seed);
    let mut lod1 = Painter::new(slot, l.seed ^ 1);
    p.contact_y = f.ground_min;
    let fy = f.floor_y;
    let fr = &l.frame;
    let bottom = f
        .pad
        .as_ref()
        .map_or(f.ground_min, |pd| pd.target_h_m.min(f.ground_min))
        - 0.5;
    let hq = fr.rect(l.hall);
    if f.kind != FoundationKind::Stilts {
        p.prism(&hq, bottom, fy, palette::STONE, None, false);
    }
    let top = fy + l.hall_h;
    p.prism(&hq, fy, top, l.wall, None, true);
    lod1.prism(&hq, fy, top, l.wall, None, false);
    let [u0, v0, u1, v1] = l.hall;
    match l.roof {
        HallRoof::Gable => {
            let spec = RoofSpec {
                kind: RoofKind::Gable { ridge_u: true },
                slope: GABLE_SLOPE,
                eave_m: 0.3,
                verge_m: 0.3,
                thickness_m: 0.14,
            };
            roofs::roof(&mut p, fr, l.hall, top, &spec, l.roof_c, [l.wall, l.wall]);
            roofs::roof(
                &mut lod1,
                fr,
                l.hall,
                top,
                &spec,
                l.roof_c,
                [l.wall, l.wall],
            );
        }
        HallRoof::Sawtooth => {
            let n = ((v1 - v0) / TOOTH_M).round().max(1.0) as usize;
            let tw = (v1 - v0) / n as f32;
            for k in 0..n {
                let (a, b) = (v0 + tw * k as f32, v0 + tw * (k + 1) as f32);
                // Sloped face rises toward the back; the vertical face is glazed.
                for pp in [&mut p, &mut lod1] {
                    pp.face_ao(
                        &[
                            fr.p3(u0, a, top),
                            fr.p3(u1, a, top),
                            fr.p3(u1, b, top + TOOTH_RISE_M),
                            fr.p3(u0, b, top + TOOTH_RISE_M),
                        ],
                        &[1.0; 4],
                        l.roof_c,
                        Surface::Roof,
                        0,
                        Vec3::Y,
                    );
                    let back = Vec3::new(fr.ey.x, 0.0, fr.ey.y);
                    pp.face_ao(
                        &[
                            fr.p3(u0, b, top),
                            fr.p3(u1, b, top),
                            fr.p3(u1, b, top + TOOTH_RISE_M),
                            fr.p3(u0, b, top + TOOTH_RISE_M),
                        ],
                        &[1.0; 4],
                        palette::C_GLASS,
                        Surface::Window,
                        k as u32,
                        back,
                    );
                    // Triangular end caps.
                    for (u, s) in [(u0, -1.0f32), (u1, 1.0)] {
                        let side = Vec3::new(fr.ex.x * s, 0.0, fr.ex.y * s);
                        pp.face_ao(
                            &[
                                fr.p3(u, a, top),
                                fr.p3(u, b, top),
                                fr.p3(u, b, top + TOOTH_RISE_M),
                            ],
                            &[1.0; 3],
                            l.wall,
                            Surface::Solid,
                            0,
                            side,
                        );
                    }
                }
            }
        }
        HallRoof::Flat => {
            let roof_q: Vec<Vec3> = hq.iter().map(|q| Vec3::new(q.x, top, q.y)).collect();
            p.face_ao(&roof_q, &[1.0; 4], l.roof_c, Surface::Roof, 0, Vec3::Y);
            lod1.face_ao(&roof_q, &[1.0; 4], l.roof_c, Surface::Roof, 0, Vec3::Y);
            // Parapet strips along the four edges.
            let t = 0.3;
            for r in [
                [u0, v0, u1, v0 + t],
                [u0, v1 - t, u1, v1],
                [u0, v0 + t, u0 + t, v1 - t],
                [u1 - t, v0 + t, u1, v1 - t],
            ] {
                p.frame_box(fr, r, top, top + 0.6, l.wall.scale(0.95), Some(l.wall));
            }
            // Roof vents.
            let mut rng = Pcg32::from_seed(l.seed ^ 0x7E47);
            for _ in 0..3 {
                let u = u0 + range(&mut rng, 3.0, (u1 - u0 - 3.0).max(3.5));
                let v = v0 + range(&mut rng, 3.0, (v1 - v0 - 3.0).max(3.5));
                p.frame_box(
                    fr,
                    [u - 1.0, v - 0.8, u + 1.0, v + 0.8],
                    top,
                    top + 1.1,
                    palette::I_ACCENTS[1],
                    Some(palette::I_ACCENTS[1].scale(1.1)),
                );
            }
        }
    }
    // Clerestory window bands high on the side and rear walls; skylights on gable roofs.
    {
        let band = |p: &mut Painter, a: Vec2, b: Vec2, out: Vec2, id: u32| {
            let len = a.distance(b);
            if len < 6.0 {
                return;
            }
            let e = (b - a) / len;
            let o3 = Vec3::new(out.x, 0.0, out.y);
            let at = |s: f32, y: f32| {
                let q = a + e * s + out * 0.04;
                Vec3::new(q.x, fy + y, q.y)
            };
            let (y0, y1) = (l.hall_h - 1.8, l.hall_h - 0.6);
            p.face_ao(
                &[
                    at(1.0, y0),
                    at(len - 1.0, y0),
                    at(len - 1.0, y1),
                    at(1.0, y1),
                ],
                &[1.0; 4],
                palette::C_GLASS,
                Surface::Window,
                id,
                o3,
            );
            // Downpipe at each end.
            for s in [0.4, len - 0.4] {
                let c = a + e * s + out * 0.12;
                let q = [
                    c + Vec2::new(-0.08, -0.08),
                    c + Vec2::new(0.08, -0.08),
                    c + Vec2::new(0.08, 0.08),
                    c + Vec2::new(-0.08, 0.08),
                ];
                p.prism(&q, fy, fy + l.hall_h, palette::I_ACCENTS[1], None, false);
            }
        };
        let q = &hq;
        for k in 1..4 {
            let (a, b) = (q[k], q[(k + 1) % 4]);
            let e = (b - a).normalize_or_zero();
            band(&mut p, a, b, Vec2::new(e.y, -e.x), k as u32);
        }
        if l.roof == HallRoof::Gable {
            let w = u1 - u0;
            let n = ((w / 5.0).floor() as usize).clamp(1, 4);
            let d = v1 - v0;
            for k in 0..n {
                let u = u0 + w * (k as f32 + 0.5) / n as f32;
                // On the front roof plane, between eave and ridge.
                let (va, vb) = (v0 + 0.25 * d, v0 + 0.4 * d);
                let ya = top + (va - v0) * GABLE_SLOPE + 0.16;
                let yb = top + (vb - v0) * GABLE_SLOPE + 0.16;
                p.face_ao(
                    &[
                        fr.p3(u - 0.9, va, ya),
                        fr.p3(u + 0.9, va, ya),
                        fr.p3(u + 0.9, vb, yb),
                        fr.p3(u - 0.9, vb, yb),
                    ],
                    &[1.0; 4],
                    palette::C_GLASS,
                    Surface::Window,
                    10 + k as u32,
                    Vec3::Y,
                );
            }
        }
    }
    // Roll-up doors with hazard-striped frames on the front wall.
    let out = -fr.ey;
    let o3 = Vec3::new(out.x, 0.0, out.y);
    for &(u, dw) in &l.doors {
        let dh = (l.hall_h - 1.2).min(4.5);
        let q = |du: f32, y: f32, off: f32| {
            let pt = fr.at(u + du, v0) + out * off;
            Vec3::new(pt.x, fy + y, pt.y)
        };
        p.face_ao(
            &[
                q(-0.5 * dw - 0.25, 0.0, 0.03),
                q(0.5 * dw + 0.25, 0.0, 0.03),
                q(0.5 * dw + 0.25, dh + 0.25, 0.03),
                q(-0.5 * dw - 0.25, dh + 0.25, 0.03),
            ],
            &[0.9, 0.9, 1.0, 1.0],
            palette::HAZARD[0],
            Surface::Solid,
            0,
            o3,
        );
        p.face_ao(
            &[
                q(-0.5 * dw, 0.0, 0.05),
                q(0.5 * dw, 0.0, 0.05),
                q(0.5 * dw, dh, 0.05),
                q(-0.5 * dw, dh, 0.05),
            ],
            &[0.85, 0.85, 1.0, 1.0],
            l.accent,
            Surface::Solid,
            0,
            o3,
        );
        // Loading dock platform in front of factory doors.
        if l.level >= 2 {
            p.frame_box(
                fr,
                [u - 0.5 * dw - 0.4, v0 - 2.0, u + 0.5 * dw + 0.4, v0],
                fy - 0.3,
                fy + 1.0,
                palette::STONE,
                Some(palette::STONE.scale(1.05)),
            );
        }
    }
    // Office annex with a window band.
    if let Some((r, ah)) = l.annex {
        let q = fr.rect(r);
        p.prism(&q, bottom, fy, palette::STONE, None, false);
        p.prism(&q, fy, fy + ah, l.wall.scale(1.08), Some(l.roof_c), true);
        lod1.prism(&q, fy, fy + ah, l.wall.scale(1.08), Some(l.roof_c), false);
        let (a, b) = (fr.at(r[0] + 0.5, r[1]), fr.at(r[2] - 0.5, r[1]));
        let at = |pt: Vec2, y: f32| {
            let w = pt + out * 0.04;
            Vec3::new(w.x, fy + y, w.y)
        };
        p.face_ao(
            &[at(a, 1.0), at(b, 1.0), at(b, 2.4), at(a, 2.4)],
            &[1.0; 4],
            palette::C_GLASS,
            Surface::Window,
            0,
            o3,
        );
    }
    // Tanks, silos and stacks.
    for rd in &l.rounds {
        let poly = round_poly(rd.c, rd.r);
        let col = match rd.kind {
            RoundKind::Tank => palette::hex(0xDCE3E8),
            RoundKind::Silo => l.wall.scale(1.1),
            RoundKind::Stack => palette::BRICK,
        };
        let base = fy - 0.2;
        p.prism(&poly, base, fy + rd.h, col, None, false);
        // LOD1: hexagonal prisms with flat tops.
        let coarse = round_poly_n(rd.c, rd.r, 6);
        lod1.prism(&coarse, base, fy + rd.h, col, Some(col.scale(0.95)), false);
        let tc = Vec3::new(rd.c.x, fy + rd.h, rd.c.y);
        match rd.kind {
            RoundKind::Tank => {
                let pts: Vec<Vec3> = poly
                    .iter()
                    .map(|q| Vec3::new(q.x, fy + rd.h, q.y))
                    .collect();
                p.face_ao(
                    &pts,
                    &[1.0; ROUND_SIDES],
                    col.scale(0.95),
                    Surface::Roof,
                    0,
                    Vec3::Y,
                );
            }
            RoundKind::Silo => {
                let apex = tc + Vec3::Y * rd.r;
                for k in 0..ROUND_SIDES {
                    let (a, b) = (poly[k], poly[(k + 1) % ROUND_SIDES]);
                    let mid = (a + b) * 0.5 - rd.c;
                    let tri = [
                        Vec3::new(a.x, fy + rd.h, a.y),
                        Vec3::new(b.x, fy + rd.h, b.y),
                        apex,
                    ];
                    let o = Vec3::new(mid.x, rd.r, mid.y);
                    p.face_ao(&tri, &[1.0; 3], l.roof_c, Surface::Roof, 0, o);
                }
            }
            RoundKind::Stack => {
                // Dark rim at the top; open (unlit inside is not visible from the ground).
                let rim = round_poly(rd.c, rd.r * 1.15);
                p.prism(
                    &rim,
                    fy + rd.h - 1.0,
                    fy + rd.h,
                    palette::HAZARD[1],
                    None,
                    false,
                );
                let pts: Vec<Vec3> = poly
                    .iter()
                    .map(|q| Vec3::new(q.x, fy + rd.h, q.y))
                    .collect();
                p.face_ao(
                    &pts,
                    &[0.4; ROUND_SIDES],
                    palette::HAZARD[1],
                    Surface::Solid,
                    0,
                    Vec3::Y,
                );
            }
        }
    }
    (p.finish(), lod1.finish())
}
