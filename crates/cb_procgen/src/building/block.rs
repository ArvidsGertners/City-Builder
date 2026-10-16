//! Polygon blocks (ALGORITHMS §11 step 2, GAME_DESIGN §4): residential L4 small apartment and
//! L5 apartment block, commercial L1 corner shop, L2 shop row / offices and L3 department store /
//! office tower. Massing is the setback-inset lot polygon cut to a depth band behind the
//! frontage and simplified to ≤ 12 vertices; tall levels step their top floors back. The façade
//! plan ([`facade_plan`]) is shared by the mesh and the tests, so windows never cross a wall
//! edge.

use cb_core::rng::Pcg32;
use cb_geometry::MeshData;
use cb_geometry::d2;
use glam::{DVec2, Vec2, Vec3};

use super::foundation::{self, Foundation, FoundationKind};
use super::zoned::{LotShape, Zone};
use crate::paint::{EAVE_AO, Frame, Painter};
use crate::palette::{self, Rgb};
use crate::surface::{MAX_ID, Surface};

/// Residential floor height (m).
pub const R_FLOOR_H_M: f32 = 3.0;
/// Commercial floor height (m).
pub const C_FLOOR_H_M: f32 = 3.6;
/// Commercial ground floor height (shops, m).
pub const C_GROUND_H_M: f32 = 4.4;
/// Walls shorter than this stay blank (m).
pub const MIN_WINDOW_WALL_M: f32 = 2.4;
/// Windows keep this far from a wall's ends (m).
pub const WINDOW_END_MARGIN_M: f32 = 0.4;
/// Parapet height (m).
pub const PARAPET_M: f32 = 0.6;
/// Walls closer than this to the lot line are party walls without openings (m).
pub const PARTY_GAP_M: f32 = 0.3;
/// Polygon vertex cap.
pub const MAX_VERTICES: usize = 12;
/// LOD0 triangle budget of a block (ART_DIRECTION §2).
pub const TRI_BUDGET: usize = 3000;

/// How a façade opening looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowKind {
    /// Punched window with frame and sill (apartments, small shops' upper floors).
    Punched,
    /// Ground-floor shop window.
    Shopfront,
    /// Continuous office glazing band.
    Ribbon,
}

/// One window opening on a wall: edge `edge` of tier `tier`, from arc length `s0` to `s1`
/// along the edge, between heights `y0` and `y1` above the ground floor level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowSlot {
    /// 0 = base volume, 1 = stepped top.
    pub tier: u8,
    /// Edge index in that tier's polygon (from vertex `edge` to `edge + 1`).
    pub edge: usize,
    /// Start along the edge (m).
    pub s0: f32,
    /// End along the edge (m).
    pub s1: f32,
    /// Bottom above the floor level (m).
    pub y0: f32,
    /// Top above the floor level (m).
    pub y1: f32,
    /// Look.
    pub kind: WindowKind,
}

/// Roof of a block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockRoof {
    /// Flat roof with a parapet.
    Flat,
    /// Hip roof over a rectangular base (small apartments).
    Hip,
}

/// One stacked volume.
#[derive(Clone, Debug, PartialEq)]
pub struct Tier {
    /// Counter-clockwise plan polygon.
    pub poly: Vec<Vec2>,
    /// Floors in this tier.
    pub floors: u8,
    /// Height of its first floor above the floor level (m).
    pub base_y: f32,
}

/// Massing and style of one block (deterministic from the lot shape, zone, level and seed).
#[derive(Clone, Debug, PartialEq)]
pub struct BlockLayout {
    /// Local frame: origin on the frontage, `ex` along it, `ey` into the lot.
    pub frame: Frame,
    /// Residential or Commercial.
    pub zone: Zone,
    /// Level.
    pub level: u8,
    /// Base volume, then the stepped top (if any).
    pub tiers: Vec<Tier>,
    /// Ground floor height (m).
    pub ground_h: f32,
    /// Upper floor height (m).
    pub floor_h: f32,
    /// Roof form (hip only on a single rectangular tier).
    pub roof: BlockRoof,
    /// Wall colour.
    pub wall: Rgb,
    /// Ground floor colour (shops: darker base).
    pub base_c: Rgb,
    /// Roof colour.
    pub roof_c: Rgb,
    /// Doors, awnings, signs.
    pub accent: Rgb,
    /// Entrance: base edge and arc length of the door centre.
    pub entrance: (usize, f32),
    /// Balconies on front walls (apartments).
    pub balconies: bool,
    /// Rooftop boxes (stair hut, AC units): plan rectangle centres and half sizes.
    pub rooftop: Vec<(Vec2, Vec2, f32)>,
    /// The lot polygon (protruding features are checked against it).
    pub lot: Vec<Vec2>,
    /// Free distance in front of each base edge to the lot line (m): party walls (< 0.3 m)
    /// get no openings; awnings, canopies and balconies never reach past the lot line.
    pub gap: Vec<f32>,
    /// Seed for mesh details.
    pub seed: u64,
}

fn range(rng: &mut Pcg32, lo: f32, hi: f32) -> f32 {
    lo + (hi - lo) * rng.next_f32()
}

fn to64(p: &[Vec2]) -> Vec<DVec2> {
    p.iter().map(|v| v.as_dvec2()).collect()
}

/// Keeps the part of `poly` with local `v ≤ v_max` (depth band behind the frontage).
fn clip_depth(poly: &[Vec2], fr: &Frame, v_max: f32) -> Vec<Vec2> {
    let n = poly.len();
    let mut out = Vec::with_capacity(n + 2);
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let (va, vb) = (fr.local(a).y, fr.local(b).y);
        if va <= v_max {
            out.push(a);
        }
        if (va <= v_max) != (vb <= v_max) {
            out.push(a + (b - a) * ((v_max - va) / (vb - va)));
        }
    }
    out
}

/// Simplifies to at most [`MAX_VERTICES`] vertices, dropping slivers and short edges. Only
/// convex vertices are removed, so the result never leaves the input polygon.
fn tidy(poly: &[Vec2]) -> Option<Vec<Vec2>> {
    let mut p = cb_geometry::polygon::to_ccw(poly);
    // Drops the convex vertex whose removal costs least while `pred(cost, edge_len)` holds.
    let drop_while = |p: &mut Vec<Vec2>, limit: &dyn Fn(f32, f32, usize) -> bool| loop {
        let n = p.len();
        if n <= 3 {
            break;
        }
        let mut best: Option<(f32, usize)> = None;
        for i in 0..n {
            let (a, b, c) = (p[(i + n - 1) % n], p[i], p[(i + 1) % n]);
            let convex = (b - a).perp_dot(c - b) > 0.0;
            if !convex {
                continue;
            }
            let (d, _) = cb_geometry::predicates::point_segment(b, a, c);
            let short = a.distance(b).min(b.distance(c));
            if limit(d, short, n) && best.is_none_or(|x| d < x.0) {
                best = Some((d, i));
            }
        }
        match best {
            Some((_, i)) => {
                p.remove(i);
            }
            None => break,
        }
    };
    // Near-collinear vertices and walls shorter than 0.8 m first, then down to the cap.
    drop_while(&mut p, &|d, short, _| d < 0.25 || short < 0.8);
    drop_while(&mut p, &|_, _, n| n > MAX_VERTICES);
    (p.len() >= 3
        && p.len() <= MAX_VERTICES
        && cb_geometry::polygon::is_simple(&p)
        && cb_geometry::polygon::area(&p) > 1.0
        && cb_geometry::polygon::min_interior_angle_deg(&p) >= 20.0)
        .then_some(p)
}

/// Frame on the frontage: origin at the front start, `ey` pointing into the lot.
#[must_use]
pub fn front_frame(lot: &LotShape) -> Frame {
    let (a, b) = lot.front;
    let ex = (b - a).normalize_or(Vec2::X);
    let mut ey = Vec2::new(-ex.y, ex.x);
    let c = cb_geometry::polygon::centroid(lot.polygon);
    if (c - a).dot(ey) < 0.0 {
        ey = -ey;
    }
    // Keep the frame counter-clockwise (ey = ex rotated +90°): flip ex if needed.
    let ex = if ex.perp_dot(ey) > 0.0 { ex } else { -ex };
    Frame { o: a, ex, ey }
}

/// Plans a block. `None` when the inset is too small for this zone and level.
#[must_use]
pub fn layout(zone: Zone, level: u8, lot: &LotShape, seed: u64) -> Option<BlockLayout> {
    let mut rng = Pcg32::from_seed(seed);
    let fr = front_frame(lot);
    let resi = zone == Zone::Residential;
    // Depth band behind the frontage (m), minimum area (m²) and floors per GAME_DESIGN §4.
    let (depth, min_area, floors) = match (resi, level) {
        (true, 4) => (range(&mut rng, 12.0, 15.0), 110.0, 3 + rng.below(3) as u8),
        (true, _) => (range(&mut rng, 14.0, 17.0), 180.0, 5 + rng.below(4) as u8),
        (false, 1) => (range(&mut rng, 10.0, 14.0), 60.0, 1 + rng.below(2) as u8),
        (false, 2) => (range(&mut rng, 14.0, 20.0), 120.0, 2 + rng.below(3) as u8),
        (false, _) => (range(&mut rng, 20.0, 30.0), 280.0, 4 + rng.below(7) as u8),
    };
    let v_front = lot
        .inset
        .iter()
        .map(|p| fr.local(*p).y)
        .fold(f32::INFINITY, f32::min);
    // The inset comes from a simplified lot and may graze past a real lot line by centimetres
    // where the setback is 0 (commercial side walls): keep only the part on the lot.
    let band = clip_depth(lot.inset, &fr, v_front + depth);
    let on_lot = cb_geometry::boolean::intersect(&band, lot.polygon)
        .outers
        .into_iter()
        .max_by(|a, b| cb_geometry::polygon::area(a).total_cmp(&cb_geometry::polygon::area(b)))?;
    let base = tidy(&on_lot)?;
    if cb_geometry::polygon::area(&base) < min_area {
        return None;
    }
    let (ground_h, floor_h) = if resi {
        (R_FLOOR_H_M, R_FLOOR_H_M)
    } else {
        (C_GROUND_H_M, C_FLOOR_H_M)
    };
    // Tall blocks step back their top floors.
    let step = match (resi, level) {
        (true, 5) => rng.chance(0.6),
        (false, 3) => true,
        (false, 2) => floors >= 3 && rng.chance(0.4),
        _ => false,
    };
    let mut tiers = Vec::with_capacity(2);
    let mut lower = floors;
    if step && floors >= 3 {
        let top_floors = if !resi && level == 3 {
            // Office tower over a podium of 2–3 floors.
            floors.saturating_sub(2 + rng.below(2) as u8).max(1)
        } else {
            1 + rng.below(2) as u8
        };
        let back = if !resi && level == 3 {
            range(&mut rng, 3.0, 5.0)
        } else {
            range(&mut rng, 1.6, 2.6)
        };
        if let Some(up) = cb_geometry::rect_fit::erode(&to64(&base), f64::from(back))
            .and_then(|q| tidy(&d2::to_f32(&q)))
            .filter(|q| cb_geometry::polygon::area(q) >= 0.3 * min_area)
        {
            lower = floors - top_floors;
            let base_y = ground_h + f32::from(lower.saturating_sub(1)) * floor_h;
            tiers.push(Tier {
                poly: base.clone(),
                floors: lower,
                base_y: 0.0,
            });
            tiers.push(Tier {
                poly: up,
                floors: top_floors,
                base_y,
            });
        }
    }
    if tiers.is_empty() {
        tiers.push(Tier {
            poly: base.clone(),
            floors: lower,
            base_y: 0.0,
        });
    }
    let rect_like = base.len() == 4 && d2::rectangularity(&to64(&base)) > 0.97 && tiers.len() == 1;
    let roof = if resi && level == 4 && rect_like && rng.chance(0.6) {
        BlockRoof::Hip
    } else {
        BlockRoof::Flat
    };
    let (wall, roof_c, accent, base_c) = if resi {
        let wall = palette::pick(&mut rng, &palette::R_WALLS).building_variant(&mut rng);
        let roof_c = if roof == BlockRoof::Hip {
            palette::pick(&mut rng, &palette::R_ROOFS).building_variant(&mut rng)
        } else {
            palette::C_ROOF.building_variant(&mut rng)
        };
        let accent = palette::pick(&mut rng, &palette::R_ACCENTS);
        (wall, roof_c, accent, wall.scale(0.92))
    } else {
        let wall = palette::pick(&mut rng, &palette::C_WALLS).building_variant(&mut rng);
        let accent = palette::pick(&mut rng, &palette::C_SIGNS);
        (
            wall,
            palette::C_ROOF.building_variant(&mut rng),
            accent,
            palette::C_WALLS[1].scale(0.85),
        )
    };
    // Entrance on the base edge that faces the street most directly (longest such edge).
    let n = base.len();
    let mut entrance = (0, 0.0);
    let mut best = f32::NEG_INFINITY;
    for i in 0..n {
        let (a, b) = (base[i], base[(i + 1) % n]);
        let len = a.distance(b);
        let e = (b - a) / len.max(1e-6);
        let out = Vec2::new(e.y, -e.x);
        let facing = out.dot(-fr.ey);
        let score = facing * 10.0 + len.min(12.0) * 0.1 - fr.local((a + b) * 0.5).y * 0.2;
        if facing > 0.6 && len >= 3.0 && score > best {
            best = score;
            entrance = (i, 0.5 * len);
        }
    }
    if best == f32::NEG_INFINITY {
        return None;
    }
    // Rooftop boxes on the top tier: stair hut (apartments) or AC units (commercial).
    let top = tiers.last().map(|t| t.poly.clone()).unwrap_or_default();
    let mut rooftop = Vec::new();
    if roof == BlockRoof::Flat {
        let c = cb_geometry::polygon::centroid(&top);
        let k = if resi { 1 } else { 1 + rng.below(3) as usize };
        for i in 0..k {
            let half = if resi && i == 0 {
                Vec2::new(1.6, 1.4)
            } else {
                Vec2::new(range(&mut rng, 0.6, 1.1), range(&mut rng, 0.5, 0.9))
            };
            let off = fr.dir(range(&mut rng, -4.0, 4.0), range(&mut rng, -3.0, 3.0));
            let p = if i == 0 { c } else { c + off };
            let corners = [
                p + fr.dir(-half.x, -half.y),
                p + fr.dir(half.x, -half.y),
                p + fr.dir(half.x, half.y),
                p + fr.dir(-half.x, half.y),
            ];
            let inside = corners.iter().all(|q| {
                cb_geometry::polygon::contains(&top, *q)
                    && cb_geometry::polygon::boundary_distance(&top, *q) > 0.6
            });
            if inside {
                let h = if resi && i == 0 {
                    2.6
                } else {
                    range(&mut rng, 0.8, 1.4)
                };
                rooftop.push((p, half, h));
            }
        }
    }
    // Free distance in front of each wall: walls on a lot line are party walls (row shops).
    let gap: Vec<f32> = (0..n)
        .map(|i| {
            let (a, _, len, e, out) = edge(&base, i);
            [0.4f32.min(0.5 * len), 0.5 * len, (len - 0.4).max(0.5 * len)]
                .iter()
                .map(|s| ray_gap(lot.polygon, a + e * *s, out))
                .fold(f32::INFINITY, f32::min)
        })
        .collect();
    // Hip roof eaves (0.4 m) and the rectangle's overshoot must stay on the lot.
    let roof = if roof == BlockRoof::Hip
        && (gap.iter().any(|g| *g < 0.7)
            || cb_geometry::polygon::min_area_rect(&base).0
                > 1.03 * cb_geometry::polygon::area(&base))
    {
        BlockRoof::Flat
    } else {
        roof
    };
    if gap.get(entrance.0).is_some_and(|g| *g < 0.3) {
        return None;
    }
    Some(BlockLayout {
        lot: lot.polygon.to_vec(),
        gap,
        frame: fr,
        zone,
        level,
        tiers,
        ground_h,
        floor_h,
        roof,
        wall,
        base_c,
        roof_c,
        accent,
        entrance,
        balconies: resi && rng.chance(0.7),
        rooftop,
        seed: rng.next_u64(),
    })
}

impl BlockLayout {
    /// Whether the rectangle in front of a wall (`s0..s1` along it, `0..depth` out) stays on
    /// the lot.
    #[must_use]
    pub fn fits_out(&self, a: Vec2, e: Vec2, out: Vec2, s0: f32, s1: f32, depth: f32) -> bool {
        [
            a + e * s0,
            a + e * s1,
            a + e * s1 + out * depth,
            a + e * s0 + out * depth,
        ]
        .iter()
        .all(|q| {
            cb_geometry::polygon::contains(&self.lot, *q)
                || cb_geometry::polygon::boundary_distance(&self.lot, *q) < 0.005
        })
    }

    /// Deepest of `depths` (descending) whose rectangle fits; `None` if none does.
    fn fit_depth(
        &self,
        a: Vec2,
        e: Vec2,
        out: Vec2,
        s0: f32,
        s1: f32,
        depths: &[f32],
    ) -> Option<f32> {
        depths
            .iter()
            .copied()
            .find(|d| self.fits_out(a, e, out, s0, s1, *d))
    }

    /// Height of a tier's top (above the floor level, m).
    #[must_use]
    pub fn tier_top(&self, t: &Tier) -> f32 {
        let first = if t.base_y <= 0.0 {
            self.ground_h
        } else {
            self.floor_h
        };
        t.base_y + first + f32::from(t.floors.saturating_sub(1)) * self.floor_h
    }

    /// Total height above the floor level (m).
    #[must_use]
    pub fn height_m(&self) -> f32 {
        let walls = self
            .tiers
            .iter()
            .map(|t| self.tier_top(t))
            .fold(0.0, f32::max);
        let extra = match self.roof {
            BlockRoof::Flat => PARAPET_M.max(self.rooftop.iter().map(|r| r.2).fold(0.0, f32::max)),
            BlockRoof::Hip => 0.5 * hip_span(&self.tiers[0].poly) * HIP_SLOPE,
        };
        walls + extra
    }

    /// Plan outline (base polygon).
    #[must_use]
    pub fn outline(&self) -> &[Vec2] {
        &self.tiers[0].poly
    }

    /// Foundation from the ground under the base (no stilts for blocks).
    #[must_use]
    pub fn plan_foundation(&self, ground: &dyn Fn(Vec2) -> f32) -> Option<Foundation> {
        let mut pts = self.outline().to_vec();
        pts.push(cb_geometry::polygon::centroid(self.outline()));
        let samples: Vec<f32> = pts.iter().map(|p| ground(*p)).collect();
        foundation::plan(self.outline(), &samples, false)
    }
}

/// tan(pitch) of hip roofs on small apartments (≈ 28°).
const HIP_SLOPE: f32 = 0.53;

fn hip_span(poly: &[Vec2]) -> f32 {
    let (_, _, q) = cb_geometry::polygon::min_area_rect(poly);
    q[0].distance(q[1]).min(q[1].distance(q[2]))
}

/// Distance from wall point `p` along `dir` to the polygon boundary (∞ if it never leaves).
/// The ray starts 2 cm behind the wall so a wall lying on the boundary measures 0.
fn ray_gap(poly: &[Vec2], p: Vec2, dir: Vec2) -> f32 {
    const BACK: f32 = 0.02;
    let p = p - dir * BACK;
    let n = poly.len();
    let mut best = f32::INFINITY;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let e = b - a;
        let den = dir.perp_dot(e);
        if den.abs() < 1e-9 {
            continue;
        }
        let t = (a - p).perp_dot(e) / den;
        let u = (a - p).perp_dot(dir) / den;
        if t > 1e-4 && (-1e-6..=1.0 + 1e-6).contains(&u) {
            best = best.min(t);
        }
    }
    (best - BACK).max(0.0)
}

fn edge(poly: &[Vec2], i: usize) -> (Vec2, Vec2, f32, Vec2, Vec2) {
    let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
    let len = a.distance(b);
    let e = (b - a) / len.max(1e-6);
    (a, b, len, e, Vec2::new(e.y, -e.x))
}

/// Window openings of every wall (shared by the mesh and the tests). A window is left out where
/// its frame and sill would reach past the lot line.
#[must_use]
pub fn facade_plan(l: &BlockLayout) -> Vec<WindowSlot> {
    let mut all = facade_slots(l);
    all.retain(|w| {
        let (a, _, _, e, out) = edge(&l.tiers[w.tier as usize].poly, w.edge);
        l.fits_out(a, e, out, w.s0 - 0.2, w.s1 + 0.2, 0.15)
    });
    all
}

fn facade_slots(l: &BlockLayout) -> Vec<WindowSlot> {
    let mut out = Vec::new();
    let resi = l.zone == Zone::Residential;
    for (ti, t) in l.tiers.iter().enumerate() {
        let n = t.poly.len();
        for i in 0..n {
            let (_, _, len, _, outn) = edge(&t.poly, i);
            if len < MIN_WINDOW_WALL_M
                || (ti == 0 && l.gap.get(i).is_some_and(|g| *g < PARTY_GAP_M))
            {
                continue;
            }
            let street = outn.dot(-l.frame.ey) > 0.6;
            let lo = WINDOW_END_MARGIN_M;
            let hi = len - WINDOW_END_MARGIN_M;
            for fl in 0..t.floors {
                let ground_floor = ti == 0 && fl == 0;
                let y = if ground_floor {
                    0.0
                } else {
                    t.base_y.max(l.ground_h)
                        + f32::from(fl.saturating_sub(u8::from(ti == 0))) * l.floor_h
                };
                let fh = if ground_floor { l.ground_h } else { l.floor_h };
                let door = ti == 0 && i == l.entrance.0 && ground_floor;
                let (d0, d1) = (l.entrance.1 - 1.2, l.entrance.1 + 1.2);
                if !resi && ground_floor && street {
                    // Shop windows across the façade, interrupted by the door.
                    let spans: Vec<(f32, f32)> = if door {
                        vec![(lo, d0 - 0.2), (d1 + 0.2, hi)]
                    } else {
                        vec![(lo, hi)]
                    };
                    for (s0, s1) in spans {
                        if s1 - s0 >= 1.0 {
                            out.push(WindowSlot {
                                tier: ti as u8,
                                edge: i,
                                s0,
                                s1,
                                y0: y + 0.4,
                                y1: y + fh - 1.2,
                                kind: WindowKind::Shopfront,
                            });
                        }
                    }
                    continue;
                }
                if !resi && l.level >= 2 && !ground_floor {
                    out.push(WindowSlot {
                        tier: ti as u8,
                        edge: i,
                        s0: lo,
                        s1: hi,
                        y0: y + 0.9,
                        y1: y + fh - 0.6,
                        kind: WindowKind::Ribbon,
                    });
                    continue;
                }
                let bay = if resi { 3.0 } else { 3.2 };
                let k = ((hi - lo) / bay).floor().max(1.0) as usize;
                let w = 1.2f32.min(hi - lo - 0.2);
                for j in 0..k {
                    let c = lo + (hi - lo) * (j as f32 + 0.5) / k as f32;
                    if door && (c - l.entrance.1).abs() < 1.5 {
                        continue;
                    }
                    out.push(WindowSlot {
                        tier: ti as u8,
                        edge: i,
                        s0: c - 0.5 * w,
                        s1: c + 0.5 * w,
                        y0: y + 0.9,
                        y1: y + 0.9 + 1.4f32.min(fh - 1.4),
                        kind: WindowKind::Punched,
                    });
                }
            }
        }
    }
    out
}

fn tri_top(p: &mut Painter, poly: &[Vec2], y: f32, c: Rgb, kind: Surface) {
    for t in cb_geometry::triangulate::triangulate(poly) {
        let v = |i: u32| {
            let q = poly[i as usize];
            Vec3::new(q.x, y, q.y)
        };
        p.face_ao(&[v(t[0]), v(t[1]), v(t[2])], &[1.0; 3], c, kind, 0, Vec3::Y);
    }
}

/// Parapet: a low wall ring on the inside of the roof edge.
fn parapet(p: &mut Painter, poly: &[Vec2], y: f32, c: Rgb) {
    let n = poly.len();
    for i in 0..n {
        let (a, b, len, e, out) = edge(poly, i);
        if len < 0.3 {
            continue;
        }
        let inn = -out * 0.25;
        let quad = [a, b, b + inn, a + inn];
        // Shorten along the edge so neighbouring pieces do not z-fight at corners.
        let q: Vec<Vec2> = quad
            .iter()
            .enumerate()
            .map(|(k, v)| {
                if k == 1 || k == 2 {
                    *v - e * 0.02
                } else {
                    *v + e * 0.02
                }
            })
            .collect();
        p.prism(&q, y, y + PARAPET_M, c, Some(c.scale(1.08)), false);
    }
}

/// Builds LOD0 and LOD1 meshes of a block on its foundation.
#[must_use]
pub fn mesh(l: &BlockLayout, f: &Foundation, slot: u16) -> (MeshData, MeshData) {
    let mut p = Painter::new(slot, l.seed);
    let mut lod1 = Painter::new(slot, l.seed ^ 1);
    p.contact_y = f.ground_min;
    let fy = f.floor_y;
    let mut rng = Pcg32::from_seed(l.seed ^ 0xB10C);
    // Plinth down into the ground.
    let bottom = f
        .pad
        .as_ref()
        .map_or(f.ground_min, |pd| pd.target_h_m.min(f.ground_min))
        - 0.5;
    if f.kind != FoundationKind::Stilts {
        p.prism(l.outline(), bottom, fy, palette::STONE, None, false);
    }
    let shop = l.zone == Zone::Commercial;
    for (ti, t) in l.tiers.iter().enumerate() {
        let y0 = fy + t.base_y;
        let top = fy + l.tier_top(t);
        if ti == 0 && shop {
            // Darker shop base, then the upper wall colour.
            let split = fy + l.ground_h;
            p.prism(&t.poly, y0, split.min(top), l.base_c, None, false);
            if top > split + 0.01 {
                p.prism(&t.poly, split, top, l.wall, None, true);
            }
        } else {
            p.prism(&t.poly, y0, top, l.wall, None, true);
        }
        lod1.prism(&t.poly, y0, top, l.wall, None, false);
        let last = ti + 1 == l.tiers.len();
        match l.roof {
            BlockRoof::Hip if last => {
                hip_roof(&mut p, &t.poly, top, l.roof_c);
                hip_roof(&mut lod1, &t.poly, top, l.roof_c);
            }
            _ => {
                tri_top(&mut p, &t.poly, top, l.roof_c, Surface::Roof);
                tri_top(&mut lod1, &t.poly, top, l.roof_c, Surface::Roof);
                parapet(&mut p, &t.poly, top, l.wall.scale(0.95));
            }
        }
    }
    // Windows.
    let mut id = 0u32;
    for w in facade_plan(l) {
        let poly = &l.tiers[w.tier as usize].poly;
        let (a, _, _, e, out) = edge(poly, w.edge);
        window(&mut p, a, e, out, fy, &w, &mut id);
        let gap = if w.tier == 0 {
            l.gap.get(w.edge).copied().unwrap_or(0.0)
        } else {
            f32::INFINITY
        };
        let awning = (w.kind == WindowKind::Shopfront && gap >= 0.45)
            .then(|| l.fit_depth(a, e, out, w.s0, w.s1, &[0.8, 0.6, 0.45]))
            .flatten();
        if let Some(depth) = awning {
            // Awning and sign above the shop window.
            let (s0, s1) = (w.s0, w.s1);
            let y = fy + w.y1 + 0.25;
            let q = [
                a + e * s0 + out * 0.02,
                a + e * s1 + out * 0.02,
                a + e * s1 + out * depth,
                a + e * s0 + out * depth,
            ];
            p.prism(&q, y, y + 0.12, l.accent, Some(l.accent), false);
            let sign = [
                a + e * (s0 + 0.2) + out * 0.02,
                a + e * (s1 - 0.2) + out * 0.02,
                a + e * (s1 - 0.2) + out * 0.12,
                a + e * (s0 + 0.2) + out * 0.12,
            ];
            p.prism(&sign, y + 0.25, y + 0.85, l.accent.scale(0.9), None, false);
        }
        // Balconies under every other punched apartment window on street walls.
        if l.balconies
            && w.kind == WindowKind::Punched
            && w.y0 > l.ground_h
            && out.dot(-l.frame.ey) > 0.6
            && gap >= 1.2
            && l.fits_out(a, e, out, w.s0 - 0.3, w.s1 + 0.3, 1.0)
            && (w.edge + id as usize).is_multiple_of(2)
        {
            let (s0, s1) = (w.s0 - 0.3, w.s1 + 0.3);
            let y = fy + w.y0 - 0.95;
            let slab = [
                a + e * s0,
                a + e * s1,
                a + e * s1 + out * 1.0,
                a + e * s0 + out * 1.0,
            ];
            p.prism(
                &slab,
                y,
                y + 0.15,
                palette::STONE.scale(1.1),
                Some(palette::STONE.scale(1.1)),
                false,
            );
            let rail = [
                a + e * s0 + out * 0.94,
                a + e * s1 + out * 0.94,
                a + e * s1 + out * 1.0,
                a + e * s0 + out * 1.0,
            ];
            p.prism(&rail, y + 0.15, y + 1.05, l.accent, None, false);
        }
    }
    // Entrance door with canopy.
    {
        let poly = &l.tiers[0].poly;
        let (a, _, _, e, out) = edge(poly, l.entrance.0);
        let c = a + e * l.entrance.1;
        let o3 = Vec3::new(out.x, 0.0, out.y);
        let at = |s: f32, y: f32| {
            let q = c + e * s + out * 0.04;
            Vec3::new(q.x, y, q.y)
        };
        let dw = if shop { 1.0 } else { 0.8 };
        p.face_ao(
            &[at(-dw, fy), at(dw, fy), at(dw, fy + 2.3), at(-dw, fy + 2.3)],
            &[0.9, 0.9, 1.0, 1.0],
            if shop { palette::GLASS } else { l.accent },
            if shop {
                Surface::Window
            } else {
                Surface::Solid
            },
            0,
            o3,
        );
        let depth = l
            .fit_depth(
                a,
                e,
                out,
                l.entrance.1 - dw - 0.3,
                l.entrance.1 + dw + 0.3,
                &[0.8, 0.6, 0.4],
            )
            .unwrap_or(0.0);
        if depth >= 0.3 {
            let canopy = [
                c - e * (dw + 0.3) + out * 0.02,
                c + e * (dw + 0.3) + out * 0.02,
                c + e * (dw + 0.3) + out * depth,
                c - e * (dw + 0.3) + out * depth,
            ];
            let cy = fy + 2.6;
            p.prism(
                &canopy,
                cy,
                cy + 0.15,
                palette::STONE,
                Some(palette::STONE),
                false,
            );
        }
    }
    // Rooftop boxes.
    if let Some(t) = l.tiers.last() {
        let top = fy + l.tier_top(t);
        for &(c, half, h) in &l.rooftop {
            let fr = &l.frame;
            let q = [
                c + fr.dir(-half.x, -half.y),
                c + fr.dir(half.x, -half.y),
                c + fr.dir(half.x, half.y),
                c + fr.dir(-half.x, half.y),
            ];
            let col = if l.zone == Zone::Residential {
                l.wall.scale(0.9)
            } else {
                palette::C_WALLS[1].scale(0.95)
            };
            p.prism(&q, top, top + h, col, Some(col.scale(1.05)), false);
            lod1.prism(&q, top, top + h, col, Some(col), false);
        }
        // An antenna on some apartment blocks.
        if l.zone == Zone::Residential && l.level == 5 && rng.chance(0.5) {
            let c = cb_geometry::polygon::centroid(&t.poly) + l.frame.dir(2.0, 0.0);
            if cb_geometry::polygon::contains(&t.poly, c) {
                let q = [
                    c + Vec2::new(-0.05, -0.05),
                    c + Vec2::new(0.05, -0.05),
                    c + Vec2::new(0.05, 0.05),
                    c + Vec2::new(-0.05, 0.05),
                ];
                p.prism(&q, top, top + 3.5, palette::LAMP_METAL, None, false);
            }
        }
    }
    (p.finish(), lod1.finish())
}

/// Hip roof over a rectangle-like polygon (oriented by its minimum-area rectangle).
fn hip_roof(p: &mut Painter, poly: &[Vec2], eave_y: f32, c: Rgb) {
    let (_, _, q) = cb_geometry::polygon::min_area_rect(poly);
    let (mut ex, mut ey) = (
        (q[1] - q[0]).normalize_or(Vec2::X),
        (q[3] - q[0]).normalize_or(Vec2::Y),
    );
    let (mut w, mut d) = (q[0].distance(q[1]), q[0].distance(q[3]));
    if ex.perp_dot(ey) < 0.0 {
        std::mem::swap(&mut ex, &mut ey);
        std::mem::swap(&mut w, &mut d);
    }
    let fr = Frame { o: q[0], ex, ey };
    let spec = super::roofs::RoofSpec {
        kind: super::roofs::RoofKind::Hip,
        slope: HIP_SLOPE,
        eave_m: 0.4,
        verge_m: 0.4,
        thickness_m: 0.16,
    };
    super::roofs::roof(p, &fr, [0.0, 0.0, w, d], eave_y, &spec, c, [c, c]);
}

/// A window opening: frame, glass and (punched) sill.
fn window(p: &mut Painter, a: Vec2, e: Vec2, out: Vec2, fy: f32, w: &WindowSlot, id: &mut u32) {
    let o3 = Vec3::new(out.x, 0.0, out.y);
    let at = |s: f32, y: f32, d: f32| {
        let q = a + e * s + out * d;
        Vec3::new(q.x, fy + y, q.y)
    };
    let fw = 0.08;
    if w.kind != WindowKind::Ribbon {
        p.face_ao(
            &[
                at(w.s0 - fw, w.y0 - fw, 0.03),
                at(w.s1 + fw, w.y0 - fw, 0.03),
                at(w.s1 + fw, w.y1 + fw, 0.03),
                at(w.s0 - fw, w.y1 + fw, 0.03),
            ],
            &[1.0; 4],
            palette::WINDOW_FRAME,
            Surface::Solid,
            0,
            o3,
        );
    }
    p.face_ao(
        &[
            at(w.s0, w.y0, 0.05),
            at(w.s1, w.y0, 0.05),
            at(w.s1, w.y1, 0.05),
            at(w.s0, w.y1, 0.05),
        ],
        &[1.0, 1.0, 1.0 - 0.5 * EAVE_AO, 1.0 - 0.5 * EAVE_AO],
        if w.kind == WindowKind::Punched {
            palette::GLASS
        } else {
            palette::C_GLASS
        },
        Surface::Window,
        *id & MAX_ID,
        o3,
    );
    *id += 1;
    if w.kind == WindowKind::Punched {
        let y = w.y0 - fw - 0.06;
        p.face_ao(
            &[
                at(w.s0 - 0.12, y, 0.0),
                at(w.s1 + 0.12, y, 0.0),
                at(w.s1 + 0.12, y, 0.12),
                at(w.s0 - 0.12, y, 0.12),
            ],
            &[1.0; 4],
            palette::STONE.scale(1.15),
            Surface::Solid,
            0,
            Vec3::Y,
        );
    }
}
