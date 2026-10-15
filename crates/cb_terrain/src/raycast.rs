//! Cursor picking against terrain and water (ALGORITHMS §23): a min-max pyramid over the
//! height cells (water levels included in the max), traversed front to back, with exact tests
//! against the two render triangles of each cell and the water plane of wet cells.

use glam::{Vec2, Vec3};

use crate::heightfield::HeightGrid;
use crate::stamps::SampleRect;
use crate::water::WaterField;

/// What a ray hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitKind {
    /// The terrain surface.
    Terrain,
    /// A water surface.
    Water,
}

/// A ray hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    /// Ray parameter (distance for a unit direction).
    pub t: f32,
    /// World position.
    pub point: Vec3,
    /// Surface normal.
    pub normal: Vec3,
    /// Surface kind.
    pub kind: HitKind,
}

/// Min-max pyramid: level 0 has one entry per height cell (corner min/max incl. water), each
/// further level halves the resolution.
#[derive(Clone, Debug, PartialEq)]
pub struct MinMax {
    /// Cells per side at level 0.
    cells: usize,
    levels: Vec<Vec<(f32, f32)>>,
    sizes: Vec<usize>,
}

impl MinMax {
    /// Builds the pyramid.
    #[must_use]
    pub fn build(h: &HeightGrid, water: &WaterField) -> Self {
        let cells = h.n() - 1;
        let mut sizes = vec![cells];
        while *sizes.last().unwrap_or(&1) > 1 {
            let s = *sizes.last().unwrap_or(&1);
            sizes.push(s.div_ceil(2));
        }
        let levels = sizes.iter().map(|s| vec![(0.0, 0.0); s * s]).collect();
        let mut mm = Self {
            cells,
            levels,
            sizes,
        };
        mm.update(
            h,
            water,
            SampleRect {
                i0: 0,
                j0: 0,
                i1: cells,
                j1: cells,
            },
        );
        mm
    }

    /// Recomputes the cells touching sample rectangle `rect` and their ancestors.
    pub fn update(&mut self, h: &HeightGrid, water: &WaterField, rect: SampleRect) {
        let c = self.cells;
        let (ci0, cj0) = (rect.i0.saturating_sub(1), rect.j0.saturating_sub(1));
        let (ci1, cj1) = (rect.i1.min(c - 1), rect.j1.min(c - 1));
        for j in cj0..=cj1 {
            for i in ci0..=ci1 {
                let mut lo = f32::INFINITY;
                let mut hi = f32::NEG_INFINITY;
                for (a, b) in [(i, j), (i + 1, j), (i, j + 1), (i + 1, j + 1)] {
                    let v = h.get(a, b);
                    lo = lo.min(v);
                    hi = hi.max(v);
                    if let Some(l) = water.level(a, b) {
                        hi = hi.max(l);
                    }
                }
                self.levels[0][j * c + i] = (lo, hi);
            }
        }
        let (mut a0, mut b0, mut a1, mut b1) = (ci0, cj0, ci1, cj1);
        for l in 1..self.levels.len() {
            let (s, ps) = (self.sizes[l], self.sizes[l - 1]);
            a0 /= 2;
            b0 /= 2;
            a1 /= 2;
            b1 /= 2;
            for j in b0..=b1.min(s - 1) {
                for i in a0..=a1.min(s - 1) {
                    let mut lo = f32::INFINITY;
                    let mut hi = f32::NEG_INFINITY;
                    for (di, dj) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (x, y) = (2 * i + di, 2 * j + dj);
                        if x < ps && y < ps {
                            let v = self.levels[l - 1][y * ps + x];
                            lo = lo.min(v.0);
                            hi = hi.max(v.1);
                        }
                    }
                    self.levels[l][j * s + i] = (lo, hi);
                }
            }
        }
    }

    /// Height range of the whole terrain.
    #[must_use]
    pub fn range(&self) -> (f32, f32) {
        self.levels
            .last()
            .and_then(|l| l.first().copied())
            .unwrap_or((0.0, 0.0))
    }
}

/// Slab test of a ray against an AABB; returns `(t_enter, t_exit)`.
fn slab(o: Vec3, inv: Vec3, lo: Vec3, hi: Vec3) -> Option<(f32, f32)> {
    let t1 = (lo - o) * inv;
    let t2 = (hi - o) * inv;
    let tmin = t1.min(t2).max_element();
    let tmax = t1.max(t2).min_element();
    (tmax >= tmin.max(0.0)).then_some((tmin.max(0.0), tmax))
}

/// Ray–triangle (Möller–Trumbore), two-sided.
fn tri(o: Vec3, d: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let e1 = b - a;
    let e2 = c - a;
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-9 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = s.dot(p) * inv;
    if !(-1e-6..=1.0 + 1e-6).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < -1e-6 || u + v > 1.0 + 1e-6 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t >= 0.0).then_some(t)
}

/// Casts a ray (`dir` need not be normalized; `t` is in units of `dir`) against terrain and
/// water up to `max_t`; returns the nearest hit.
#[must_use]
pub fn raycast(
    h: &HeightGrid,
    water: &WaterField,
    mm: &MinMax,
    origin: Vec3,
    dir: Vec3,
    max_t: f32,
) -> Option<RayHit> {
    let s = h.spacing_m();
    let half = h.half_extent_m();
    // Nudge zero components so the slab test never computes 0 × ∞ (NaN) on cell boundaries.
    let nz = |c: f32| if c.abs() < 1e-12 { 1e-12 } else { c };
    let inv = Vec3::new(1.0 / nz(dir.x), 1.0 / nz(dir.y), 1.0 / nz(dir.z));
    let mut best: Option<RayHit> = None;
    let top = mm.levels.len() - 1;
    let mut stack: Vec<(usize, usize, usize)> = vec![(top, 0, 0)];
    while let Some((l, i, j)) = stack.pop() {
        let size = mm.sizes[l];
        let (lo_h, hi_h) = mm.levels[l][j * size + i];
        let span = (1usize << l) as f32 * s;
        let x0 = i as f32 * span - half;
        let z0 = j as f32 * span - half;
        let x1 = (x0 + span).min(half);
        let z1 = (z0 + span).min(half);
        let Some((t0, _)) = slab(
            origin,
            inv,
            Vec3::new(x0, lo_h - 1e-3, z0),
            Vec3::new(x1, hi_h + 1e-3, z1),
        ) else {
            continue;
        };
        if t0 > max_t || best.is_some_and(|b| t0 > b.t) {
            continue;
        }
        if l == 0 {
            let p = |a: usize, b: usize| {
                let q = h.pos(a, b);
                Vec3::new(q.x, h.get(a, b), q.y)
            };
            let (p00, p10, p01, p11) = (p(i, j), p(i + 1, j), p(i, j + 1), p(i + 1, j + 1));
            for (a, b, c) in [(p00, p10, p11), (p00, p11, p01)] {
                if let Some(t) = tri(origin, dir, a, b, c)
                    && t <= max_t
                    && best.is_none_or(|bh| t < bh.t)
                {
                    let n = (b - a).cross(c - a).normalize_or(Vec3::Y);
                    let n = if n.y < 0.0 { -n } else { n };
                    best = Some(RayHit {
                        t,
                        point: origin + dir * t,
                        normal: n,
                        kind: HitKind::Terrain,
                    });
                }
            }
            // Water plane of a wet cell (highest corner level).
            let wl = [(i, j), (i + 1, j), (i, j + 1), (i + 1, j + 1)]
                .iter()
                .filter_map(|(a, b)| water.level(*a, *b))
                .fold(f32::NEG_INFINITY, f32::max);
            if wl.is_finite() && dir.y.abs() > 1e-9 {
                let t = (wl - origin.y) / dir.y;
                let hp = origin + dir * t;
                if t >= 0.0
                    && t <= max_t
                    && hp.x >= x0
                    && hp.x <= x1
                    && hp.z >= z0
                    && hp.z <= z1
                    && water.is_water(Vec2::new(hp.x, hp.z))
                    && best.is_none_or(|bh| t < bh.t)
                {
                    best = Some(RayHit {
                        t,
                        point: hp,
                        normal: Vec3::Y,
                        kind: HitKind::Water,
                    });
                }
            }
            continue;
        }
        // Children, pushed far-to-near so the nearest is popped first.
        let cs = mm.sizes[l - 1];
        let mut kids: Vec<(f32, (usize, usize, usize))> = Vec::with_capacity(4);
        for (di, dj) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let (ci, cj) = (2 * i + di, 2 * j + dj);
            if ci < cs && cj < cs {
                let cspan = (1usize << (l - 1)) as f32 * s;
                let cx = ci as f32 * cspan - half + cspan * 0.5;
                let cz = cj as f32 * cspan - half + cspan * 0.5;
                let key = (Vec3::new(cx, origin.y, cz) - origin).dot(dir);
                kids.push((key, (l - 1, ci, cj)));
            }
        }
        kids.sort_by(|a, b| b.0.total_cmp(&a.0));
        stack.extend(kids.into_iter().map(|k| k.1));
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::water::WaterBodyKind;

    fn hill() -> HeightGrid {
        let mut g = HeightGrid::filled(129, 2.0, 0.0);
        for j in 0..129 {
            for i in 0..129 {
                let p = g.pos(i, j);
                g.set(i, j, 20.0 - p.length() * 0.1);
            }
        }
        g
    }

    #[test]
    fn vertical_ray_hits_surface_height() {
        let g = hill();
        let w = WaterField::dry_like(&g);
        let mm = MinMax::build(&g, &w);
        for p in [
            Vec2::new(3.3, -7.1),
            Vec2::new(-60.0, 50.5),
            Vec2::new(0.1, 0.2),
        ] {
            let hit =
                raycast(&g, &w, &mm, Vec3::new(p.x, 500.0, p.y), Vec3::NEG_Y, 1e4).expect("hit");
            assert!((hit.point.y - g.height_at(p)).abs() < 1e-3, "{p}");
            assert_eq!(hit.kind, HitKind::Terrain);
        }
    }

    #[test]
    fn oblique_ray_matches_brute_force_march() {
        let g = hill();
        let w = WaterField::dry_like(&g);
        let mm = MinMax::build(&g, &w);
        let o = Vec3::new(-120.0, 60.0, -100.0);
        let d = Vec3::new(1.0, -0.35, 0.8).normalize();
        let hit = raycast(&g, &w, &mm, o, d, 1e4).expect("hit");
        // Brute force: fine march to the first sign change.
        let mut t = 0.0;
        while o.y + d.y * t > g.height_at(Vec2::new(o.x + d.x * t, o.z + d.z * t)) {
            t += 0.01;
        }
        assert!((hit.t - t).abs() < 0.05, "{} vs {t}", hit.t);
    }

    #[test]
    fn water_is_hit_before_the_bed_and_updates_apply() {
        let mut g = HeightGrid::filled(65, 2.0, 5.0);
        let mut w = WaterField::dry_like(&g);
        let id = w.add_body(WaterBodyKind::Lake);
        for j in 20..40 {
            for i in 20..40 {
                g.set(i, j, 1.0);
                w.set(i, j, 4.0, id);
            }
        }
        let mm = MinMax::build(&g, &w);
        let c = g.pos(30, 30);
        let hit = raycast(&g, &w, &mm, Vec3::new(c.x, 50.0, c.y), Vec3::NEG_Y, 1e3).expect("hit");
        assert_eq!(hit.kind, HitKind::Water);
        assert!((hit.point.y - 4.0).abs() < 1e-4);
        // Raise the dry corner and update only that rect.
        let mut mm2 = mm.clone();
        g.set(2, 2, 30.0);
        mm2.update(
            &g,
            &w,
            SampleRect {
                i0: 2,
                j0: 2,
                i1: 2,
                j1: 2,
            },
        );
        assert_eq!(mm2, MinMax::build(&g, &w));
    }
}
