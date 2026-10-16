//! Residential yards (ALGORITHMS §11 step 9, ≤ 400 triangles): driveway in the side strip the
//! house left free, path from the door to the road, fences or hedges on the rear edges and the
//! lot's own side (the side at the end of its frontage interval, so a shared side edge gets
//! exactly one fence), shed, garden beds, patio and trees. The lawn is the terrain itself.

use cb_core::rng::Pcg32;
use cb_geometry::MeshData;
use cb_geometry::rect_fit::EdgeKind;
use glam::{Vec2, Vec3};

use crate::building::house::HouseLayout;
use crate::paint::{Frame, Painter};
use crate::palette::{self, Rgb};
use crate::surface::Surface;
use crate::vegetation::{self, TreeKind};

/// Triangle budget of one yard.
pub const YARD_TRI_BUDGET: usize = 400;
/// Upper bounds of the optional items (checked against the budget before adding them).
const SHED_TRIS: usize = 50;
const BED_TRIS: usize = 30;
const TREE_TRIS: usize = 72;

/// Lot context for the yard.
#[derive(Clone, Debug)]
pub struct YardLot<'a> {
    /// Lot polygon (counter-clockwise).
    pub polygon: &'a [Vec2],
    /// Class of each edge `k → k + 1`.
    pub edges: &'a [EdgeKind],
    /// Frontage point at the start of the lot's interval (s0).
    pub front_start: Vec2,
    /// Frontage point at the end of the lot's interval (s1).
    pub front_end: Vec2,
}

#[derive(Clone, Copy)]
enum FenceStyle {
    Picket,
    Hedge,
    Boards,
}

fn range(rng: &mut Pcg32, lo: f32, hi: f32) -> f32 {
    lo + (hi - lo) * rng.next_f32()
}

/// Ray from `p` along `d`: distance to the first lot edge.
fn ray_to_boundary(poly: &[Vec2], p: Vec2, d: Vec2) -> Option<f32> {
    let n = poly.len();
    let mut best: Option<f32> = None;
    for k in 0..n {
        let (a, b) = (poly[k], poly[(k + 1) % n]);
        let e = b - a;
        let den = d.perp_dot(e);
        if den.abs() < 1e-9 {
            continue;
        }
        let t = (a - p).perp_dot(e) / den;
        let u = (a - p).perp_dot(d) / den;
        if t > 1e-4 && (0.0..=1.0).contains(&u) && best.is_none_or(|b| t < b) {
            best = Some(t);
        }
    }
    best
}

/// How far a strip of half width `hw` centred on `u` can run from `v` toward the road before
/// either side leaves the lot (slanted front edges on fanned lots).
fn reach(poly: &[Vec2], fr: &Frame, u: f32, hw: f32, v: f32) -> Option<f32> {
    [u - hw, u, u + hw]
        .iter()
        .filter_map(|&x| ray_to_boundary(poly, fr.at(x, v), -fr.ey))
        .reduce(f32::min)
}

/// Separating-axis overlap of two convex quads.
fn quads_overlap(a: &[Vec2; 4], b: &[Vec2; 4]) -> bool {
    for poly in [a, b] {
        for k in 0..4 {
            let e = poly[(k + 1) % 4] - poly[k];
            let ax = Vec2::new(-e.y, e.x);
            let proj = |q: &[Vec2; 4]| {
                q.iter()
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
                        (lo.min(p.dot(ax)), hi.max(p.dot(ax)))
                    })
            };
            let (a0, a1) = proj(a);
            let (b0, b1) = proj(b);
            if a1 < b0 || b1 < a0 {
                return false;
            }
        }
    }
    true
}

fn grow(q: [Vec2; 4], d: f32) -> [Vec2; 4] {
    let c = (q[0] + q[2]) * 0.5;
    q.map(|p| {
        let v = p - c;
        p + v.normalize_or_zero() * d * std::f32::consts::SQRT_2
    })
}

struct Placer<'a> {
    lot: &'a [Vec2],
    taken: Vec<[Vec2; 4]>,
}

impl Placer<'_> {
    /// Accepts the quad if it lies `margin` inside the lot and clear of everything placed.
    fn try_place(&mut self, q: [Vec2; 4], margin: f32, clearance: f32) -> bool {
        let inside = q.iter().all(|p| {
            cb_geometry::polygon::contains(self.lot, *p)
                && cb_geometry::polygon::boundary_distance(self.lot, *p) >= margin
        });
        if !inside
            || self
                .taken
                .iter()
                .any(|t| quads_overlap(&grow(q, clearance), t))
        {
            return false;
        }
        self.taken.push(q);
        true
    }
}

/// Builds the yard mesh of a house lot.
#[must_use]
pub fn yard(
    l: &HouseLayout,
    lot: &YardLot<'_>,
    floor_y: f32,
    ground: &dyn Fn(Vec2) -> f32,
    slot: u16,
    seed: u64,
) -> MeshData {
    let mut rng = Pcg32::from_seed(seed);
    let mut p = Painter::new(slot, seed ^ 0x9A4D);
    let fr = l.frame;
    let mut placer = Placer {
        lot: lot.polygon,
        taken: l.outlines().to_vec(),
    };
    let main = l.volumes[0].rect;
    let back_v = l.volumes.iter().map(|v| v.rect[3]).fold(main[3], f32::max);

    // Driveway: from the road edge to beside the house.
    let mut drive_u = None;
    if let Some([ua, ub]) = l.driveway {
        let u = 0.5 * (ua + ub);
        let width = (ub - ua - 0.6).clamp(2.4, 3.0);
        if let Some(t) = reach(lot.polygon, &fr, u, 0.5 * width, 0.5) {
            let start = fr.at(u, 0.5) - fr.ey * t;
            let end = fr.at(u, main[3] * 0.85);
            let colour = if rng.chance(0.5) {
                palette::ASPHALT_LIGHT
            } else {
                palette::PAVING
            };
            p.ground_strip(&[start, end], width, 0.05, colour, Surface::Paved, ground);
            placer
                .taken
                .push(fr.rect([u - 0.5 * width, -t, u + 0.5 * width, main[3] * 0.85]));
            drive_u = Some(u);
        }
    }
    // Path from each door to the road (skipped next to the driveway).
    for &(u, v) in &l.doors {
        if drive_u.is_some_and(|d| (d - u).abs() < 2.5) {
            continue;
        }
        if let Some(t) = reach(lot.polygon, &fr, u, 0.55, v - 0.9) {
            let a = fr.at(u, v - 0.9);
            p.ground_strip(
                &[a - fr.ey * t, a],
                1.1,
                0.04,
                palette::PAVING,
                Surface::Paved,
                ground,
            );
            placer
                .taken
                .push(fr.rect([u - 0.6, v - 0.9 - t, u + 0.6, v - 0.9]));
        }
    }

    // Fences on rear edges and on the lot's own side.
    let style = match rng.below(3) {
        0 => FenceStyle::Picket,
        1 => FenceStyle::Hedge,
        _ => FenceStyle::Boards,
    };
    let n = lot.polygon.len();
    let kept: Vec<bool> = (0..n)
        .map(|k| {
            let (a, b) = (lot.polygon[k], lot.polygon[(k + 1) % n]);
            match lot.edges.get(k) {
                Some(EdgeKind::Rear) => true,
                Some(EdgeKind::Side) => {
                    let m = (a + b) * 0.5;
                    m.distance(lot.front_end) < m.distance(lot.front_start)
                }
                _ => false,
            }
        })
        .collect();
    // Chains of consecutive fenced edges, simplified so curved rear edges (many short chords)
    // become a few runs.
    let start = (0..n)
        .find(|&k| kept[k] && !kept[(k + n - 1) % n])
        .unwrap_or(0);
    let mut chain: Vec<Vec2> = Vec::new();
    let fence_c = match style {
        FenceStyle::Picket => palette::WOOD[0],
        FenceStyle::Hedge => palette::HEDGE.building_variant(&mut rng),
        FenceStyle::Boards => palette::WOOD[1],
    };
    for i in 0..=n {
        let k = (start + i) % n;
        if i < n && kept[k] {
            if chain.is_empty() {
                chain.push(lot.polygon[k]);
            }
            chain.push(lot.polygon[(k + 1) % n]);
        } else if chain.len() >= 2 {
            let pts = cb_geometry::polyline::douglas_peucker(&chain, 0.15);
            fence(&mut p, &pts, style, fence_c, ground);
            chain.clear();
        } else {
            chain.clear();
        }
    }

    // Optional items in priority order, each only if it still fits the triangle budget.
    let fits = |p: &Painter, cost: usize| p.mesh.triangle_count() + cost <= YARD_TRI_BUDGET;

    // Patio behind the house.
    if rng.chance(0.5) && fits(&p, 8) {
        let u = range(&mut rng, main[0] + 1.0, (main[2] - 4.0).max(main[0] + 1.0));
        let q = fr.rect([u, back_v + 0.1, u + 3.2, back_v + 2.6]);
        if placer.try_place(q, 0.5, 0.0) {
            let mid_u = u + 1.6;
            p.ground_strip(
                &[fr.at(mid_u, back_v + 0.1), fr.at(mid_u, back_v + 2.6)],
                3.2,
                0.05,
                palette::PAVING,
                Surface::Paved,
                ground,
            );
        }
    }

    // Shed in a rear corner.
    let (lot_lo, lot_hi) = local_bounds(&fr, lot.polygon);
    if lot_hi.y - back_v > 4.5 && fits(&p, SHED_TRIS) {
        let (sw, sd) = (range(&mut rng, 2.2, 2.8), range(&mut rng, 1.8, 2.3));
        let right_first = drive_u.is_some_and(|d| d < 0.5 * (main[0] + main[2]));
        let us = if right_first {
            [lot_hi.x - 1.2 - sw, lot_lo.x + 1.2]
        } else {
            [lot_lo.x + 1.2, lot_hi.x - 1.2 - sw]
        };
        'shed: for back in [1.2, 2.5, 4.0] {
            for u in us {
                let v = lot_hi.y - back - sd;
                let q = fr.rect([u, v, u + sw, v + sd]);
                if placer.try_place(q, 0.4, 0.8) {
                    shed(&mut p, &fr, [u, v, u + sw, v + sd], ground, &mut rng);
                    break 'shed;
                }
            }
        }
    }

    // Garden beds.
    for _ in 0..1 + rng.below(2) {
        if !fits(&p, BED_TRIS) {
            break;
        }
        let (bw, bd) = (range(&mut rng, 2.5, 4.0), range(&mut rng, 1.0, 1.4));
        for _ in 0..6 {
            let u = range(
                &mut rng,
                lot_lo.x + 0.8,
                (lot_hi.x - 0.8 - bw).max(lot_lo.x + 0.8),
            );
            let v = range(
                &mut rng,
                back_v + 1.0,
                (lot_hi.y - 1.0 - bd).max(back_v + 1.0),
            );
            let q = fr.rect([u, v, u + bw, v + bd]);
            if placer.try_place(q, 0.5, 0.5) {
                bed(&mut p, &q, ground, &mut rng);
                break;
            }
        }
    }

    // Trees (1–2) in the back yard, clear of the house.
    let trees = 1 + rng.below(2);
    for _ in 0..trees {
        if !fits(&p, TREE_TRIS) {
            break;
        }
        for _ in 0..8 {
            let u = range(
                &mut rng,
                lot_lo.x + 2.0,
                (lot_hi.x - 2.0).max(lot_lo.x + 2.0),
            );
            let v = range(&mut rng, back_v + 2.5, (lot_hi.y - 1.5).max(back_v + 2.5));
            let q = fr.rect([u - 1.2, v - 1.2, u + 1.2, v + 1.2]);
            if placer.try_place(q, 1.0, 1.0) {
                let c = fr.at(u, v);
                let kind = if rng.chance(0.25) {
                    TreeKind::Conifer
                } else {
                    TreeKind::Deciduous
                };
                let h = range(&mut rng, 5.0, 8.5);
                vegetation::tree(&mut p, Vec3::new(c.x, ground(c), c.y), kind, h, &mut rng);
                break;
            }
        }
    }
    let _ = floor_y;
    debug_assert!(
        p.mesh.triangle_count() <= YARD_TRI_BUDGET + 120,
        "fences alone exceed the budget"
    );
    p.finish()
}

fn local_bounds(fr: &Frame, poly: &[Vec2]) -> (Vec2, Vec2) {
    poly.iter().map(|p| fr.local(*p)).fold(
        (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
        |(lo, hi), q| (lo.min(q), hi.max(q)),
    )
}

/// Fence along an open chain of lot edges (counter-clockwise lot: inside is on the left),
/// inset to the inside, panels ≤ 2.5 m following the ground, a post every third panel.
fn fence(
    p: &mut Painter,
    chain: &[Vec2],
    style: FenceStyle,
    colour: Rgb,
    ground: &dyn Fn(Vec2) -> f32,
) {
    let (h, thick) = match style {
        FenceStyle::Picket => (1.0, 0.0),
        FenceStyle::Hedge => (0.95, 0.5),
        FenceStyle::Boards => (1.5, 0.0),
    };
    let inset = 0.15 + 0.5 * thick;
    let m = chain.len();
    let seg_n = |k: usize| {
        let d = (chain[k + 1] - chain[k]).normalize_or_zero();
        Vec2::new(-d.y, d.x)
    };
    // Mitred inset of the chain.
    let pts: Vec<Vec2> = (0..m)
        .map(|k| {
            let nrm = if k == 0 {
                seg_n(0)
            } else if k == m - 1 {
                seg_n(m - 2)
            } else {
                let (a, b) = (seg_n(k - 1), seg_n(k));
                let mid = (a + b).normalize_or(b);
                mid / mid.dot(b).max(0.5)
            };
            chain[k] + nrm * inset
        })
        .collect();
    let v3 = |q: Vec2, y: f32| Vec3::new(q.x, y, q.y);
    let mut panel = 0usize;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = a.distance(b);
        if len < 0.05 {
            continue;
        }
        let d = (b - a) / len;
        let inward = Vec2::new(-d.y, d.x);
        let segs = (len / 2.5).ceil().max(1.0) as usize;
        for k in 0..segs {
            let q0 = a + d * (len * k as f32 / segs as f32);
            let q1 = a + d * (len * (k + 1) as f32 / segs as f32);
            let (g0, g1) = (ground(q0), ground(q1));
            p.contact_y = g0.min(g1);
            match style {
                FenceStyle::Hedge => {
                    let o = inward * (0.5 * thick);
                    for (sgn, out) in [(1.0, inward), (-1.0, -inward)] {
                        let (r0, r1) = (q0 + o * sgn, q1 + o * sgn);
                        p.face_ao(
                            &[
                                v3(r0, g0 - 0.1),
                                v3(r1, g1 - 0.1),
                                v3(r1, g1 + h),
                                v3(r0, g0 + h),
                            ],
                            &[0.82, 0.82, 1.0, 1.0],
                            colour,
                            Surface::Foliage,
                            0,
                            Vec3::new(out.x, 0.0, out.y),
                        );
                    }
                    p.face_ao(
                        &[
                            v3(q0 - o, g0 + h),
                            v3(q1 - o, g1 + h),
                            v3(q1 + o, g1 + h),
                            v3(q0 + o, g0 + h),
                        ],
                        &[1.0; 4],
                        colour.scale(1.06),
                        Surface::Foliage,
                        0,
                        Vec3::Y,
                    );
                }
                FenceStyle::Picket | FenceStyle::Boards => {
                    for out in [inward, -inward] {
                        p.face_ao(
                            &[
                                v3(q0, g0 + 0.08),
                                v3(q1, g1 + 0.08),
                                v3(q1, g1 + h),
                                v3(q0, g0 + h),
                            ],
                            &[0.85, 0.85, 1.0, 1.0],
                            colour,
                            Surface::Solid,
                            0,
                            Vec3::new(out.x, 0.0, out.y),
                        );
                    }
                    if panel.is_multiple_of(3) {
                        post(p, q0, g0, h + 0.1, colour.scale(0.9));
                    }
                }
            }
            panel += 1;
        }
    }
    if matches!(style, FenceStyle::Picket | FenceStyle::Boards)
        && let Some(q) = pts.last()
    {
        post(p, *q, ground(*q), h + 0.1, colour.scale(0.9));
    }
}

fn post(p: &mut Painter, c: Vec2, g: f32, h: f32, colour: Rgb) {
    let r = 0.06;
    let q = [
        c + Vec2::new(-r, -r),
        c + Vec2::new(r, -r),
        c + Vec2::new(r, r),
        c + Vec2::new(-r, r),
    ];
    let v3 = |q: Vec2, y: f32| Vec3::new(q.x, y, q.y);
    for k in 0..4 {
        let (a, b) = (q[k], q[(k + 1) % 4]);
        let out = (a + b) * 0.5 - c;
        p.face_ao(
            &[v3(a, g - 0.1), v3(b, g - 0.1), v3(b, g + h), v3(a, g + h)],
            &[0.82, 0.82, 1.0, 1.0],
            colour,
            Surface::Solid,
            0,
            Vec3::new(out.x, 0.0, out.y),
        );
    }
    let top: Vec<Vec3> = q.iter().map(|v| v3(*v, g + h)).collect();
    p.face_ao(&top, &[1.0; 4], colour, Surface::Solid, 0, Vec3::Y);
}

fn shed(p: &mut Painter, fr: &Frame, r: [f32; 4], ground: &dyn Fn(Vec2) -> f32, rng: &mut Pcg32) {
    let q = fr.rect(r);
    let g: Vec<f32> = q.iter().map(|c| ground(*c)).collect();
    let (lo, hi) = (
        g.iter().copied().fold(f32::INFINITY, f32::min),
        g.iter().copied().fold(f32::NEG_INFINITY, f32::max),
    );
    p.contact_y = lo;
    let colour = palette::pick(rng, &palette::WOOD[1..]);
    let h = range(rng, 2.0, 2.3);
    let base = hi + 0.1;
    p.prism(&q, lo - 0.2, base + h, colour, None, true);
    // Mono-pitch roof slab sloping toward the back.
    let v3 = |u: f32, v: f32, y: f32| fr.p3(u, v, y);
    let (u0, v0, u1, v1) = (r[0] - 0.2, r[1] - 0.2, r[2] + 0.2, r[3] + 0.3);
    let (y_front, y_back) = (base + h + 0.45, base + h - 0.05);
    let roof_c = palette::R_ROOFS[3];
    let top = [
        v3(u0, v0, y_front),
        v3(u1, v0, y_front),
        v3(u1, v1, y_back),
        v3(u0, v1, y_back),
    ];
    p.face_ao(
        &top,
        &[1.0; 4],
        roof_c,
        Surface::Roof,
        0,
        fr.d3(0.0, 0.5, 1.0),
    );
    let dn = |v: Vec3| v - Vec3::Y * 0.1;
    p.face_ao(
        &[dn(top[0]), dn(top[1]), dn(top[2]), dn(top[3])],
        &[0.8; 4],
        roof_c.scale(0.6),
        Surface::Solid,
        0,
        -Vec3::Y,
    );
    // Raise the front wall up to the roof (gable-like triangles on the sides).
    let (a, b) = (fr.at(r[0], r[1]), fr.at(r[2], r[1]));
    let out = -fr.ey;
    let o3 = Vec3::new(out.x, 0.0, out.y);
    let wall_top = base + h;
    p.face_ao(
        &[
            Vec3::new(a.x, wall_top, a.y),
            Vec3::new(b.x, wall_top, b.y),
            Vec3::new(b.x, y_front - 0.1, b.y),
            Vec3::new(a.x, y_front - 0.1, a.y),
        ],
        &[0.95; 4],
        colour,
        Surface::Solid,
        0,
        o3,
    );
    // Door.
    let mid = (a + b) * 0.5;
    let e = (b - a).normalize_or_zero();
    let at = |s: f32, y: f32| {
        let q = mid + e * s + out * 0.03;
        Vec3::new(q.x, y, q.y)
    };
    p.face_ao(
        &[
            at(-0.45, base),
            at(0.45, base),
            at(0.45, base + 1.9),
            at(-0.45, base + 1.9),
        ],
        &[0.9; 4],
        colour.scale(0.75),
        Surface::Solid,
        0,
        o3,
    );
}

fn bed(p: &mut Painter, q: &[Vec2; 4], ground: &dyn Fn(Vec2) -> f32, rng: &mut Pcg32) {
    let g = q.iter().map(|c| ground(*c)).fold(f32::INFINITY, f32::min);
    p.contact_y = g;
    let soil = palette::pick(rng, &palette::DIRT);
    p.prism(
        q,
        g - 0.15,
        g + 0.18,
        palette::STONE.scale(1.05),
        Some(soil),
        false,
    );
    // Flower / plant specks: small pyramids.
    let c = (q[0] + q[2]) * 0.5;
    let (e1, e2) = ((q[1] - q[0]) * 0.5, (q[3] - q[0]) * 0.5);
    for _ in 0..4 {
        let s = c + e1 * range(rng, -0.75, 0.75) + e2 * range(rng, -0.6, 0.6);
        let col = if rng.chance(0.6) {
            palette::pick(rng, &palette::FLOWERS)
        } else {
            palette::pick(rng, &palette::FOLIAGE)
        };
        let (r, h) = (range(rng, 0.18, 0.3), range(rng, 0.25, 0.5));
        let top = Vec3::new(s.x, g + 0.18 + h, s.y);
        let base = |dx: f32, dz: f32| Vec3::new(s.x + dx, g + 0.18, s.y + dz);
        let ring = [base(-r, -r), base(r, -r), base(r, r), base(-r, r)];
        for k in 0..4 {
            let (a, b) = (ring[k], ring[(k + 1) % 4]);
            let mid = (a + b) * 0.5;
            p.face_ao(
                &[a, b, top],
                &[0.9, 0.9, 1.0],
                col,
                Surface::Foliage,
                0,
                Vec3::new(mid.x - s.x, 0.3, mid.z - s.y),
            );
        }
    }
}
