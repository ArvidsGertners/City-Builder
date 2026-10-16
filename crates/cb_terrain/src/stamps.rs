//! Deformation stamps and order-independent compositing (ADR-0065).
//!
//! A stamp is rasterized once into per-sample entries: **core** samples take the stamp's target
//! height exactly (highest priority, then lowest id wins), **blend** samples carry a weight.
//! The composite at a sample is `core target` if any core covers it, else
//! `base + max(fill offsets) + min(cut offsets)` with `offset = weight · (target − base)`.
//! The result does not depend on the order stamps were added, and removing every stamp
//! restores the base bitwise.

use cb_core::math::smoothstep;
use glam::{Vec2, Vec3};

use crate::heightfield::HeightGrid;

/// Fill side slope (rise over run, 1:1.5).
pub const FILL_SLOPE: f32 = 1.0 / 1.5;
/// Cut side slope (1:1).
pub const CUT_SLOPE: f32 = 1.0;
/// Default carve below the road surface (m).
pub const CARVE_M: f32 = 0.15;
/// Exclusion margin around cores for scatter (m).
pub const EXCLUSION_MARGIN_M: f32 = 2.0;

/// Inclusive sample rectangle `(i0, j0, i1, j1)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleRect {
    /// First column.
    pub i0: usize,
    /// First row.
    pub j0: usize,
    /// Last column (inclusive).
    pub i1: usize,
    /// Last row (inclusive).
    pub j1: usize,
}

impl SampleRect {
    /// Smallest rectangle covering both.
    #[must_use]
    pub fn union(self, o: Self) -> Self {
        Self {
            i0: self.i0.min(o.i0),
            j0: self.j0.min(o.j0),
            i1: self.i1.max(o.i1),
            j1: self.j1.max(o.j1),
        }
    }

    /// Grows by `k` samples, clamped to an `n × n` grid.
    #[must_use]
    pub fn grow(self, k: usize, n: usize) -> Self {
        Self {
            i0: self.i0.saturating_sub(k),
            j0: self.j0.saturating_sub(k),
            i1: (self.i1 + k).min(n - 1),
            j1: (self.j1 + k).min(n - 1),
        }
    }

    /// Whether the rectangles overlap.
    #[must_use]
    pub fn intersects(self, o: Self) -> bool {
        !(self.i1 < o.i0 || o.i1 < self.i0 || self.j1 < o.j0 || o.j1 < self.j0)
    }

    /// Whether `(i, j)` lies inside.
    #[must_use]
    pub fn contains(self, i: usize, j: usize) -> bool {
        i >= self.i0 && i <= self.i1 && j >= self.j0 && j <= self.j1
    }

    /// Rectangle covering world-space bounds `[lo, hi]` on `grid` (clamped).
    #[must_use]
    pub fn from_world(grid: &HeightGrid, lo: Vec2, hi: Vec2) -> Self {
        let m = (grid.n() - 1) as f32;
        let a = grid.to_grid(lo);
        let b = grid.to_grid(hi);
        Self {
            i0: a.x.floor().clamp(0.0, m) as usize,
            j0: a.y.floor().clamp(0.0, m) as usize,
            i1: b.x.ceil().clamp(0.0, m) as usize,
            j1: b.y.ceil().clamp(0.0, m) as usize,
        }
    }
}

/// Stamp shapes.
#[derive(Clone, Debug, PartialEq)]
pub enum StampShape {
    /// Road ground corridor: core = `core_half_width_m` either side of the centreline at the
    /// centreline height minus `carve_m`; blend zone along the fill/cut side slopes.
    Corridor {
        /// Centreline points `(x, surface height, z)`.
        centerline: Vec<Vec3>,
        /// Carriageway half width + shoulder (m).
        core_half_width_m: f32,
        /// Carve below the surface (m).
        carve_m: f32,
        /// Soft ends `[start, end]`: beyond such an end the corridor has no core, only blend
        /// (ADR-0065 soft end cap where a ground run meets a bridge span), so the abutment pad
        /// under the deck wins.
        soft_ends: [bool; 2],
    },
    /// Flat pad (building foundation, plaza): core inside the polygon, blend over `falloff_m`.
    Pad {
        /// Polygon (any orientation).
        polygon: Vec<Vec2>,
        /// Target height.
        target_h_m: f32,
        /// Blend width outside the polygon (m).
        falloff_m: f32,
    },
    /// Paved surface given as triangles with heights (junctions): core samples inside any
    /// triangle take the interpolated height minus `carve_m`, so the terrain stays below a
    /// surface that is not a single corridor profile. No blend zone (the adjoining corridors
    /// blend into the surroundings).
    Surface {
        /// Triangles `(x, surface height, z)`.
        triangles: Vec<[Vec3; 3]>,
        /// Carve below the surface (m).
        carve_m: f32,
    },
    /// Clears scatter (trees, rocks) inside the polygon without changing heights.
    Clear {
        /// Polygon.
        polygon: Vec<Vec2>,
    },
}

/// Length of a corridor's soft end cap (m, ADR-0065).
pub const SOFT_END_M: f32 = 4.0;

/// Stamp priorities (higher wins on cores): junction > road > abutment > pad.
pub mod priority {
    /// Building pads.
    pub const PAD: u8 = 0;
    /// Bridge abutments.
    pub const ABUTMENT: u8 = 1;
    /// Road corridors.
    pub const ROAD: u8 = 2;
    /// Junction surfaces (win over the corridors that overlap them).
    pub const JUNCTION: u8 = 3;
}

/// A deformation stamp.
#[derive(Clone, Debug, PartialEq)]
pub struct Stamp {
    /// Stable id (tie-break on cores: lower id wins).
    pub id: u32,
    /// Core priority ([`priority`]).
    pub priority: u8,
    /// Geometry.
    pub shape: StampShape,
}

/// One rasterized contribution.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entry {
    /// Sample index in the grid.
    pub idx: u32,
    /// Target height.
    pub target: f32,
    /// Blend weight (1 on cores).
    pub weight: f32,
    /// Core sample.
    pub core: bool,
}

/// A stamp rasterized against a base grid.
#[derive(Clone, Debug, PartialEq)]
pub struct Raster {
    /// Covered samples.
    pub rect: SampleRect,
    /// Contributions.
    pub entries: Vec<Entry>,
    /// Earthwork volume relative to the base (m³; fill + cut magnitudes).
    pub earthwork_m3: f32,
    /// Samples to clear of scatter (beyond cores, e.g. `Clear` stamps).
    pub clear: Vec<u32>,
}

/// Blend width so the steepest part of the smoothstep equals the nominal side slope.
#[must_use]
pub fn blend_width(e: f32) -> f32 {
    if e > 0.0 {
        1.6 * e / FILL_SLOPE
    } else {
        1.6 * -e / CUT_SLOPE
    }
}

/// Point-in-polygon (even-odd).
fn inside(poly: &[Vec2], p: Vec2) -> bool {
    let mut c = false;
    let n = poly.len();
    for k in 0..n {
        let (a, b) = (poly[k], poly[(k + n - 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            c = !c;
        }
    }
    c
}

/// Height of triangle `t` above `p` (plan coordinates), if `p` lies inside it (edges included).
fn surface_height(t: &[Vec3; 3], p: Vec2) -> Option<f32> {
    let [a, b, c] = t.map(|v| Vec2::new(v.x, v.z));
    let den = (b - a).perp_dot(c - a);
    if den.abs() < 1e-9 {
        return None;
    }
    let u = (p - a).perp_dot(c - a) / den;
    let v = (b - a).perp_dot(p - a) / den;
    let eps = 1e-5;
    (u >= -eps && v >= -eps && u + v <= 1.0 + eps)
        .then(|| t[0].y + (t[1].y - t[0].y) * u + (t[2].y - t[0].y) * v)
}

/// Distance from `p` to the polygon boundary.
fn poly_dist(poly: &[Vec2], p: Vec2) -> f32 {
    let n = poly.len();
    (0..n)
        .map(|k| {
            let (a, b) = (poly[k], poly[(k + 1) % n]);
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
            (p - (a + ab * t)).length()
        })
        .fold(f32::INFINITY, f32::min)
}

fn bounds(points: impl Iterator<Item = Vec2>) -> (Vec2, Vec2) {
    points.fold(
        (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(p), hi.max(p)),
    )
}

/// Rasterizes a stamp against `base`. The extent grows (×1.6, ≤ 3 times) until no blend
/// weight > 0.01 touches its border (ADR-0065), unless that border is the grid edge.
#[must_use]
pub fn rasterize(stamp: &Stamp, base: &HeightGrid) -> Raster {
    let mut k = 1.0;
    for _ in 0..4 {
        let r = rasterize_k(stamp, base, k);
        if !touches_border(&r, base.n()) {
            return r;
        }
        k *= 1.6;
    }
    rasterize_k(stamp, base, k)
}

fn touches_border(r: &Raster, n: usize) -> bool {
    let rc = r.rect;
    r.entries.iter().any(|e| {
        let (i, j) = (e.idx as usize % n, e.idx as usize / n);
        !e.core
            && e.weight > 0.01
            && ((i == rc.i0 && rc.i0 > 0)
                || (j == rc.j0 && rc.j0 > 0)
                || (i == rc.i1 && rc.i1 < n - 1)
                || (j == rc.j1 && rc.j1 < n - 1))
    })
}

fn rasterize_k(stamp: &Stamp, base: &HeightGrid, kmul: f32) -> Raster {
    let n = base.n();
    let cell = base.spacing_m() * base.spacing_m();
    match &stamp.shape {
        StampShape::Corridor {
            centerline,
            core_half_width_m,
            carve_m,
            soft_ends,
        } => {
            let cl2: Vec<Vec2> = centerline.iter().map(|p| Vec2::new(p.x, p.z)).collect();
            // Reach: core + blend for the largest |e| along the centreline.
            let emax = centerline
                .iter()
                .map(|p| (p.y - base.height_at(Vec2::new(p.x, p.z))).abs() + 2.0)
                .fold(2.0, f32::max);
            let reach =
                (core_half_width_m + blend_width(emax).max(blend_width(-emax)) + 2.0) * kmul;
            let (lo, hi) = bounds(cl2.iter().copied());
            let rect =
                SampleRect::from_world(base, lo - Vec2::splat(reach), hi + Vec2::splat(reach));
            let w = rect.i1 - rect.i0 + 1;
            let hgt = rect.j1 - rect.j0 + 1;
            // Nearest centreline distance, interpolated surface height and whether the sample
            // lies beyond a soft end, per sample.
            let mut best = vec![(f32::INFINITY, 0.0f32, false); w * hgt];
            let last = cl2.len().saturating_sub(2);
            for q in 0..cl2.len().saturating_sub(1) {
                let (a, b) = (cl2[q], cl2[q + 1]);
                let (ha, hb) = (centerline[q].y, centerline[q + 1].y);
                let sr = SampleRect::from_world(
                    base,
                    a.min(b) - Vec2::splat(reach),
                    a.max(b) + Vec2::splat(reach),
                );
                let ab = b - a;
                let l2 = ab.length_squared().max(1e-9);
                for j in sr.j0.max(rect.j0)..=sr.j1.min(rect.j1) {
                    for i in sr.i0.max(rect.i0)..=sr.i1.min(rect.i1) {
                        let p = base.pos(i, j);
                        let raw = (p - a).dot(ab) / l2;
                        let t = raw.clamp(0.0, 1.0);
                        let d = (p - (a + ab * t)).length();
                        let beyond = (q == 0 && raw < 0.0 && soft_ends[0])
                            || (q == last && raw > 1.0 && soft_ends[1]);
                        let c = &mut best[(j - rect.j0) * w + (i - rect.i0)];
                        if d < c.0 {
                            *c = (d, ha + (hb - ha) * t, beyond);
                        }
                    }
                }
            }
            let mut entries = Vec::new();
            let mut earth = 0.0;
            for j in rect.j0..=rect.j1 {
                for i in rect.i0..=rect.i1 {
                    let (d, surf, beyond) = best[(j - rect.j0) * w + (i - rect.i0)];
                    if !d.is_finite() {
                        continue;
                    }
                    let target = surf - carve_m;
                    let bh = base.get(i, j);
                    let idx = base.index(i, j) as u32;
                    if beyond {
                        // Soft end cap: blend only, fading over the abutment length.
                        if d < SOFT_END_M {
                            let weight = 1.0 - smoothstep(0.0, SOFT_END_M, d);
                            entries.push(Entry {
                                idx,
                                target,
                                weight,
                                core: false,
                            });
                        }
                    } else if d <= *core_half_width_m {
                        entries.push(Entry {
                            idx,
                            target,
                            weight: 1.0,
                            core: true,
                        });
                        earth += (target - bh).abs() * cell;
                    } else {
                        let e = target - bh;
                        let dd = d - core_half_width_m;
                        let bw = blend_width(e);
                        if dd < bw {
                            let weight = 1.0 - smoothstep(0.0, bw, dd);
                            entries.push(Entry {
                                idx,
                                target,
                                weight,
                                core: false,
                            });
                            earth += (weight * e).abs() * cell;
                        }
                    }
                }
            }
            Raster {
                rect,
                entries,
                earthwork_m3: earth,
                clear: Vec::new(),
            }
        }
        StampShape::Pad {
            polygon,
            target_h_m,
            falloff_m,
        } => {
            let reach = falloff_m * kmul + base.spacing_m();
            let (lo, hi) = bounds(polygon.iter().copied());
            let rect =
                SampleRect::from_world(base, lo - Vec2::splat(reach), hi + Vec2::splat(reach));
            let mut entries = Vec::new();
            let mut earth = 0.0;
            for j in rect.j0..=rect.j1 {
                for i in rect.i0..=rect.i1 {
                    let p = base.pos(i, j);
                    let idx = base.index(i, j) as u32;
                    let bh = base.get(i, j);
                    if inside(polygon, p) {
                        entries.push(Entry {
                            idx,
                            target: *target_h_m,
                            weight: 1.0,
                            core: true,
                        });
                        earth += (target_h_m - bh).abs() * cell;
                    } else {
                        let d = poly_dist(polygon, p);
                        if d < *falloff_m {
                            let weight = 1.0 - smoothstep(0.0, *falloff_m, d);
                            entries.push(Entry {
                                idx,
                                target: *target_h_m,
                                weight,
                                core: false,
                            });
                            earth += (weight * (target_h_m - bh)).abs() * cell;
                        }
                    }
                }
            }
            Raster {
                rect,
                entries,
                earthwork_m3: earth,
                clear: Vec::new(),
            }
        }
        StampShape::Surface { triangles, carve_m } => {
            let (lo, hi) = bounds(triangles.iter().flatten().map(|v| Vec2::new(v.x, v.z)));
            let rect = SampleRect::from_world(base, lo, hi);
            let mut entries = Vec::new();
            let mut earth = 0.0;
            for j in rect.j0..=rect.j1 {
                for i in rect.i0..=rect.i1 {
                    let p = base.pos(i, j);
                    let Some(h) = triangles.iter().find_map(|t| surface_height(t, p)) else {
                        continue;
                    };
                    let target = h - carve_m;
                    let bh = base.get(i, j);
                    entries.push(Entry {
                        idx: base.index(i, j) as u32,
                        target,
                        weight: 1.0,
                        core: true,
                    });
                    earth += (target - bh).abs() * cell;
                }
            }
            Raster {
                rect,
                entries,
                earthwork_m3: earth,
                clear: Vec::new(),
            }
        }
        StampShape::Clear { polygon } => {
            let (lo, hi) = bounds(polygon.iter().copied());
            let rect = SampleRect::from_world(base, lo, hi);
            let mut clear = Vec::new();
            for j in rect.j0..=rect.j1 {
                for i in rect.i0..=rect.i1 {
                    if inside(polygon, base.pos(i, j)) {
                        clear.push(base.index(i, j) as u32);
                    }
                }
            }
            let _ = n;
            Raster {
                rect,
                entries: Vec::new(),
                earthwork_m3: 0.0,
                clear,
            }
        }
    }
}

/// Composite output for one rectangle.
pub struct CompositeOut<'a> {
    /// Final heights (full grid; only `rect` is written).
    pub height: &'a mut [f32],
    /// Core flags (full grid).
    pub core: &'a mut [bool],
    /// Disturbance weight 0–255 (max blend weight; 255 on cores) for material painting.
    pub disturbed: &'a mut [u8],
}

/// Recomposites `rect` of the output from `base` and all `(stamp, raster)` pairs.
pub fn composite<'s>(
    base: &HeightGrid,
    stamps: impl Iterator<Item = (&'s Stamp, &'s Raster)>,
    rect: SampleRect,
    out: &mut CompositeOut<'_>,
) {
    let n = base.n();
    let w = rect.i1 - rect.i0 + 1;
    let cnt = w * (rect.j1 - rect.j0 + 1);
    let mut core_t = vec![f32::NAN; cnt];
    let mut core_key = vec![(0u8, u32::MAX); cnt];
    let mut fill = vec![0.0f32; cnt];
    let mut cut = vec![0.0f32; cnt];
    let mut dist = vec![0u8; cnt];
    for (s, r) in stamps {
        if !r.rect.intersects(rect) {
            continue;
        }
        for e in &r.entries {
            let gi = e.idx as usize;
            let (i, j) = (gi % n, gi / n);
            if !rect.contains(i, j) {
                continue;
            }
            let k = (j - rect.j0) * w + (i - rect.i0);
            if e.core {
                let cur = core_key[k];
                if core_t[k].is_nan() || s.priority > cur.0 || (s.priority == cur.0 && s.id < cur.1)
                {
                    core_t[k] = e.target;
                    core_key[k] = (s.priority, s.id);
                }
                dist[k] = 255;
            } else {
                let d = e.weight * (e.target - base.data()[gi]);
                if d > 0.0 {
                    fill[k] = fill[k].max(d);
                } else {
                    cut[k] = cut[k].min(d);
                }
                dist[k] = dist[k].max((e.weight * 255.0) as u8);
            }
        }
    }
    for j in rect.j0..=rect.j1 {
        for i in rect.i0..=rect.i1 {
            let k = (j - rect.j0) * w + (i - rect.i0);
            let gi = j * n + i;
            let b = base.data()[gi];
            out.disturbed[gi] = dist[k];
            if core_t[k].is_nan() {
                out.core[gi] = false;
                out.height[gi] = if fill[k] == 0.0 && cut[k] == 0.0 {
                    b
                } else {
                    b + fill[k] + cut[k]
                };
            } else {
                out.core[gi] = true;
                out.height[gi] = core_t[k];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(h: f32) -> HeightGrid {
        HeightGrid::filled(257, 2.0, h)
    }

    fn road(y: f32) -> Stamp {
        let centerline = (0..=20)
            .map(|k| Vec3::new(-100.0 + k as f32 * 10.0, y, 0.0))
            .collect();
        Stamp {
            id: 1,
            priority: priority::ROAD,
            shape: StampShape::Corridor {
                centerline,
                core_half_width_m: 7.0,
                carve_m: CARVE_M,
                soft_ends: [false; 2],
            },
        }
    }

    fn run(base: &HeightGrid, stamps: &[(Stamp, Raster)]) -> (Vec<f32>, Vec<bool>) {
        let n = base.n();
        let mut h = base.data().to_vec();
        let mut c = vec![false; n * n];
        let mut d = vec![0u8; n * n];
        let rect = SampleRect {
            i0: 0,
            j0: 0,
            i1: n - 1,
            j1: n - 1,
        };
        composite(
            base,
            stamps.iter().map(|(s, r)| (s, r)),
            rect,
            &mut CompositeOut {
                height: &mut h,
                core: &mut c,
                disturbed: &mut d,
            },
        );
        (h, c)
    }

    #[test]
    fn corridor_core_at_target_and_blend_within_side_slope() {
        let base = flat(10.0);
        let s = road(14.0);
        let r = rasterize(&s, &base);
        let (h, core) = run(&base, &[(s, r.clone())]);
        let g = HeightGrid::from_vec(base.n(), 2.0, h).expect("size");
        assert!((g.height_at(Vec2::new(0.0, 0.0)) - (14.0 - CARVE_M)).abs() < 1e-4);
        assert!(core[base.index(128, 128)]);
        // Fill embankment: slope never steeper than 1:1.5 (+ sampling tolerance).
        for k in 0..30 {
            let a = g.height_at(Vec2::new(0.0, 7.0 + k as f32));
            let b = g.height_at(Vec2::new(0.0, 8.0 + k as f32));
            assert!(
                (a - b).abs() <= FILL_SLOPE * 1.0 + 0.05,
                "slope at {k}: {}",
                a - b
            );
        }
        assert!(
            (g.height_at(Vec2::new(0.0, 40.0)) - 10.0).abs() < 1e-3,
            "back to base"
        );
        assert!(r.earthwork_m3 > 0.0);
    }

    #[test]
    fn composite_order_independent_and_removal_restores_base() {
        let base = flat(10.0);
        let a = road(12.0);
        let mut b = road(8.0);
        b.id = 2;
        if let StampShape::Corridor { centerline, .. } = &mut b.shape {
            for p in centerline.iter_mut() {
                *p = Vec3::new(p.z + 20.0, p.y, p.x);
            }
        }
        let (ra, rb) = (rasterize(&a, &base), rasterize(&b, &base));
        let (h1, _) = run(&base, &[(a.clone(), ra.clone()), (b.clone(), rb.clone())]);
        let (h2, _) = run(&base, &[(b, rb), (a, ra)]);
        assert!(h1.iter().zip(&h2).all(|(x, y)| x.to_bits() == y.to_bits()));
        let (h0, _) = run(&base, &[]);
        assert!(
            h0.iter()
                .zip(base.data())
                .all(|(x, y)| x.to_bits() == y.to_bits())
        );
    }

    #[test]
    fn grading_continuous_compositing() {
        // A fill corridor and a cut corridor whose blend zones overlap: base + max fill + min
        // cut stays continuous, so outside cores no step exceeds the sum of both side slopes.
        let base = flat(10.0);
        let a = road(14.0);
        let mut b = road(6.0);
        b.id = 2;
        if let StampShape::Corridor { centerline, .. } = &mut b.shape {
            for p in centerline.iter_mut() {
                p.z += 24.0;
            }
        }
        let (ra, rb) = (rasterize(&a, &base), rasterize(&b, &base));
        let (h, core) = run(&base, &[(a, ra), (b, rb)]);
        let n = base.n();
        let bound = base.spacing_m() * (FILL_SLOPE + CUT_SLOPE) + 0.1;
        let mut worst = 0.0f32;
        for j in 0..n {
            for i in 0..n {
                let k = j * n + i;
                assert!(h[k].is_finite());
                for k2 in [(i + 1 < n).then(|| k + 1), (j + 1 < n).then(|| k + n)]
                    .into_iter()
                    .flatten()
                {
                    if !core[k] && !core[k2] {
                        worst = worst.max((h[k] - h[k2]).abs());
                    }
                }
            }
        }
        assert!(worst <= bound, "step {worst} > {bound}");
    }

    #[test]
    fn pad_and_clear() {
        let base = flat(5.0);
        let poly = vec![
            Vec2::new(-10.0, -10.0),
            Vec2::new(10.0, -10.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(-10.0, 10.0),
        ];
        let pad = Stamp {
            id: 3,
            priority: priority::PAD,
            shape: StampShape::Pad {
                polygon: poly.clone(),
                target_h_m: 6.0,
                falloff_m: 6.0,
            },
        };
        let r = rasterize(&pad, &base);
        let (h, _) = run(&base, &[(pad, r)]);
        assert_eq!(h[base.index(128, 128)], 6.0);
        let clear = Stamp {
            id: 4,
            priority: priority::PAD,
            shape: StampShape::Clear { polygon: poly },
        };
        let rc = rasterize(&clear, &base);
        assert!(rc.entries.is_empty());
        assert_eq!(
            rc.clear.len(),
            100,
            "10 × 10 samples strictly inside a 20 m square"
        );
    }

    #[test]
    fn surface_stamp_follows_triangles_and_wins_over_roads() {
        let base = HeightGrid::filled(65, 2.0, 5.0);
        // A tilted quad (two triangles) rising 1 m per 10 m in x.
        let h = |x: f32| 5.0 + 0.1 * x;
        let q = [(-20.0, -10.0), (20.0, -10.0), (20.0, 10.0), (-20.0, 10.0)]
            .map(|(x, z)| Vec3::new(x, h(x), z));
        let surf = Stamp {
            id: 9,
            priority: priority::JUNCTION,
            shape: StampShape::Surface {
                triangles: vec![[q[0], q[1], q[2]], [q[0], q[2], q[3]]],
                carve_m: CARVE_M,
            },
        };
        let r = rasterize(&surf, &base);
        assert!(!r.entries.is_empty());
        for e in &r.entries {
            let i = e.idx as usize % base.n();
            let j = e.idx as usize / base.n();
            let p = base.pos(i, j);
            assert!(p.x.abs() <= 20.0 + 1e-3 && p.y.abs() <= 10.0 + 1e-3);
            assert!(e.core);
            assert!(
                (e.target - (h(p.x) - CARVE_M)).abs() < 1e-4,
                "{p}: {}",
                e.target
            );
        }
        // Outside the triangles nothing is touched.
        assert!(surface_height(&[q[0], q[1], q[2]], Vec2::new(30.0, 0.0)).is_none());
        const { assert!(priority::JUNCTION > priority::ROAD) };
    }
}
