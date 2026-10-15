//! Frontage polylines (sidewalk outer edges) and the plot polygons built from them.
//!
//! Convention: the frontage starts at the origin heading +x; the plot lies on the LEFT.
//! A left turn curves toward the plot (concave frontage, plot on the inside of the curve);
//! a right turn curves away from it (convex frontage, lots fan out).

use cb_core::rng::Pcg32;
use glam::DVec2;

use crate::geom::{self, Poly, cross, perp};

#[derive(Clone, Copy, Debug)]
pub enum Prim {
    Line(f64),
    /// radius, signed turn in radians (+ = left = toward the plot).
    Arc(f64, f64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Straight,
    Convex,
    Concave,
    SCurve,
    Corner,
    TwoSeg,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Straight => "straight",
            Kind::Convex => "convex",
            Kind::Concave => "concave",
            Kind::SCurve => "s-curve",
            Kind::Corner => "corner",
            Kind::TwoSeg => "two-seg",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    R,
    C,
    I,
}

#[derive(Clone, Copy, Debug)]
pub struct ZoneParams {
    pub w_min: f64,
    pub w_target: f64,
    pub w_max: f64,
    pub d_max: f64,
    pub a_min: f64,
    /// front, side, rear setbacks (m).
    pub setback: [f64; 3],
}

impl Zone {
    pub fn params(self) -> ZoneParams {
        match self {
            Zone::R => ZoneParams {
                w_min: 10.0,
                w_target: 13.0,
                w_max: 18.0,
                d_max: 45.0,
                a_min: 120.0,
                setback: [3.0, 1.5, 4.0],
            },
            Zone::C => ZoneParams {
                w_min: 14.0,
                w_target: 20.0,
                w_max: 30.0,
                d_max: 50.0,
                a_min: 200.0,
                setback: [1.0, 0.0, 2.0],
            },
            Zone::I => ZoneParams {
                w_min: 24.0,
                w_target: 36.0,
                w_max: 60.0,
                d_max: 80.0,
                a_min: 600.0,
                setback: [4.0, 2.0, 4.0],
            },
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Zone::R => "R",
            Zone::C => "C",
            Zone::I => "I",
        }
    }
}

/// A concave arc whose radius is below the plot depth: the medial axis enters the plot and
/// needs a seam (ray from the arc centre along the bisector, away from the arc).
#[derive(Clone, Copy, Debug)]
pub struct SeamArc {
    pub s0: f64,
    pub s1: f64,
    pub center: DVec2,
    pub dir: DVec2,
    pub radius: f64,
    /// Turn ≥ 45°: a road corner (corner lot spanning it).
    pub is_corner: bool,
}

#[derive(Clone, Debug)]
pub struct Frontage {
    pub prims: Vec<Prim>,
    pub pts: Vec<DVec2>,
    /// Cumulative arclength per point.
    pub s: Vec<f64>,
    pub seams: Vec<SeamArc>,
    /// Cumulative absolute heading change at each vertex (radians).
    pub cum_turn: Vec<f64>,
    /// Midpoints (s) of short sharp non-corner arcs (> 25°, shorter than 10 m): a split is pinned
    /// there so no lot spans the whole kink.
    pub kinks: Vec<f64>,
}

impl Frontage {
    pub fn build(prims: &[Prim], depth: f64) -> Self {
        let mut pts = vec![DVec2::ZERO];
        let mut s = vec![0.0];
        let mut pos = DVec2::ZERO;
        let mut head = DVec2::X;
        let mut acc = 0.0;
        let mut seams = Vec::new();
        for p in prims {
            match *p {
                Prim::Line(l) => {
                    pos += head * l;
                    acc += l;
                    pts.push(pos);
                    s.push(acc);
                }
                Prim::Arc(r, turn) => {
                    let side = turn.signum();
                    let center = pos + perp(head) * r * side;
                    let len = r * turn.abs();
                    let steps = ((turn.abs() / 3f64.to_radians()).ceil() as usize)
                        .max((len / 2.0).ceil() as usize)
                        .max(1);
                    let s0 = acc;
                    let start_rel = pos - center;
                    for k in 1..=steps {
                        let a = turn * k as f64 / steps as f64;
                        let q = center + geom::rotate(start_rel, a);
                        acc = s0 + len * k as f64 / steps as f64;
                        pts.push(q);
                        s.push(acc);
                    }
                    pos = *pts.last().unwrap_or(&pos);
                    head = geom::rotate(head, turn);
                    if turn > 0.0 && depth > r {
                        let mid_rel = geom::rotate(start_rel, turn * 0.5);
                        seams.push(SeamArc {
                            s0,
                            s1: acc,
                            center,
                            dir: (-mid_rel).normalize(),
                            radius: r,
                            is_corner: turn >= 45f64.to_radians(),
                        });
                    }
                }
            }
        }
        let mut cum_turn = vec![0.0; pts.len()];
        for i in 1..pts.len() - 1 {
            let a = (pts[i] - pts[i - 1]).normalize_or_zero();
            let b = (pts[i + 1] - pts[i]).normalize_or_zero();
            cum_turn[i] = cum_turn[i - 1] + geom::angle_between(a, b);
        }
        if pts.len() > 1 {
            let n = pts.len();
            cum_turn[n - 1] = cum_turn[n - 2];
        }
        let mut kinks = Vec::new();
        let mut acc = 0.0;
        for p in prims {
            match *p {
                Prim::Line(l) => acc += l,
                Prim::Arc(r, t) => {
                    let len = r * t.abs();
                    if t.abs() > 25f64.to_radians() && t.abs() < 45f64.to_radians() && len < 10.0 {
                        kinks.push(acc + 0.5 * len);
                    }
                    acc += len;
                }
            }
        }
        Frontage {
            prims: prims.to_vec(),
            pts,
            s,
            seams,
            cum_turn,
            kinks,
        }
    }

    /// Absolute heading change strictly inside (a, b).
    pub fn turn(&self, a: f64, b: f64) -> f64 {
        // Vertices with a < s < b.
        let i0 = self.s.partition_point(|v| *v <= a);
        let i1 = self.s.partition_point(|v| *v < b);
        if i1 <= i0 {
            return 0.0;
        }
        self.cum_turn[i1 - 1] - if i0 == 0 { 0.0 } else { self.cum_turn[i0 - 1] }
    }

    pub fn len(&self) -> f64 {
        *self.s.last().unwrap_or(&0.0)
    }

    fn seg_at(&self, s: f64) -> usize {
        let n = self.pts.len() - 1;
        match self.s.binary_search_by(|v| v.total_cmp(&s)) {
            Ok(i) => i.min(n - 1),
            Err(i) => i.saturating_sub(1).min(n - 1),
        }
    }

    pub fn point_at(&self, s: f64) -> DVec2 {
        let i = self.seg_at(s);
        let (s0, s1) = (self.s[i], self.s[i + 1]);
        let t = if s1 > s0 {
            ((s - s0) / (s1 - s0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.pts[i].lerp(self.pts[i + 1], t)
    }

    pub fn seg_normal(&self, i: usize) -> DVec2 {
        perp((self.pts[i + 1] - self.pts[i]).normalize())
    }

    pub fn tangent_at(&self, s: f64) -> DVec2 {
        let i = self.seg_at(s);
        (self.pts[i + 1] - self.pts[i]).normalize()
    }

    /// Exact inward normal at `s` (vertex normals blended).
    pub fn normal_at(&self, s: f64) -> DVec2 {
        perp(self.tangent_at(s))
    }

    /// Length-weighted mean inward normal over [a, b].
    pub fn mean_normal(&self, a: f64, b: f64) -> DVec2 {
        let a = a.max(0.0);
        let b = b.min(self.len());
        let mut acc = DVec2::ZERO;
        for i in 0..self.pts.len() - 1 {
            let lo = self.s[i].max(a);
            let hi = self.s[i + 1].min(b);
            if hi > lo {
                acc += self.seg_normal(i) * (hi - lo);
            }
        }
        acc.normalize_or(self.normal_at(0.5 * (a + b)))
    }

    /// Total absolute heading change over [a, b] (radians).
    pub fn turn_between(&self, a: f64, b: f64) -> f64 {
        let ta = self.tangent_at(a);
        let mut acc = 0.0;
        let mut prev = ta;
        for i in 0..self.pts.len() - 1 {
            if self.s[i] > a && self.s[i] < b {
                let t = (self.pts[i + 1] - self.pts[i]).normalize();
                acc += geom::angle_between(prev, t);
                prev = t;
            }
        }
        acc
    }

    /// (s, distance) of the nearest frontage point.
    pub fn project(&self, p: DVec2) -> (f64, f64) {
        let mut best = (0.0, f64::INFINITY);
        for i in 0..self.pts.len() - 1 {
            let (d, t) = geom::dist_point_seg(p, self.pts[i], self.pts[i + 1]);
            if d < best.1 {
                best = (self.s[i] + t * (self.s[i + 1] - self.s[i]), d);
            }
        }
        best
    }

    /// Plot polygon: union of the per-segment depth quads plus wedges at convex vertices
    /// (= one-sided offset band with round-ish joins and straight side edges).
    pub fn plot_polygon(&self, depth: f64) -> Option<Poly> {
        let n = self.pts.len();
        let mut parts: Vec<Poly> = Vec::with_capacity(2 * n);
        for i in 0..n - 1 {
            let nrm = self.seg_normal(i);
            let (a, b) = (self.pts[i], self.pts[i + 1]);
            parts.push(vec![a, b, b + nrm * depth, a + nrm * depth]);
            if i + 2 < n {
                let n2 = self.seg_normal(i + 1);
                // Right turn (away from the plot) leaves a wedge gap.
                if cross(b - a, self.pts[i + 2] - b) < 0.0 {
                    parts.push(vec![
                        b,
                        b + n2 * depth,
                        b + (nrm + n2).normalize() * depth,
                        b + nrm * depth,
                    ]);
                }
            }
        }
        let (u, holes) = geom::union_all(&parts);
        if u.len() != 1 || holes != 0 {
            return None;
        }
        // Auto-clip (ALGORITHMS §9): subtract the road corridor on the outer side of the
        // frontage, then keep the component that holds the frontage midpoint.
        const ROAD_W: f64 = 12.0;
        let mut road: Vec<Poly> = Vec::with_capacity(2 * n);
        for i in 0..n - 1 {
            let nrm = self.seg_normal(i);
            let (a, b) = (self.pts[i], self.pts[i + 1]);
            road.push(vec![a, a - nrm * ROAD_W, b - nrm * ROAD_W, b]);
            if i + 2 < n {
                let n2 = self.seg_normal(i + 1);
                if cross(b - a, self.pts[i + 2] - b) > 0.0 {
                    road.push(vec![
                        b,
                        b - nrm * ROAD_W,
                        b - (nrm + n2).normalize() * ROAD_W,
                        b - n2 * ROAD_W,
                    ]);
                }
            }
        }
        let parts = geom::difference(&u[0], &road);
        let probe = self.point_at(0.5 * self.len()) + self.normal_at(0.5 * self.len()) * 0.05;
        let main = parts.into_iter().find(|p| geom::point_in_poly(probe, p))?;
        Some(geom::simplify(&main, 0.002))
    }
}

/// Uniform grid over frontage segments for near-point queries (on-frontage tests).
pub struct FrontageIndex {
    lo: DVec2,
    cell: f64,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<u32>>,
}

impl FrontageIndex {
    pub fn new(f: &Frontage) -> Self {
        let (lo, hi) = geom::bbox(&f.pts);
        let cell = 4.0;
        let lo = lo - DVec2::splat(0.5);
        let nx = (((hi.x - lo.x) / cell).ceil() as usize + 1).max(1);
        let ny = (((hi.y - lo.y) / cell).ceil() as usize + 1).max(1);
        let mut cells = vec![Vec::new(); nx * ny];
        for i in 0..f.pts.len() - 1 {
            let (a, b) = (f.pts[i].min(f.pts[i + 1]), f.pts[i].max(f.pts[i + 1]));
            let x0 = ((a.x - 0.05 - lo.x) / cell).floor().max(0.0) as usize;
            let y0 = ((a.y - 0.05 - lo.y) / cell).floor().max(0.0) as usize;
            let x1 = (((b.x + 0.05 - lo.x) / cell).floor() as usize).min(nx - 1);
            let y1 = (((b.y + 0.05 - lo.y) / cell).floor() as usize).min(ny - 1);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    cells[y * nx + x].push(i as u32);
                }
            }
        }
        FrontageIndex {
            lo,
            cell,
            nx,
            ny,
            cells,
        }
    }

    /// (s, d) of the nearest frontage point if one lies within 0.05 m.
    pub fn near(&self, f: &Frontage, p: DVec2) -> Option<(f64, f64)> {
        let q = (p - self.lo) / self.cell;
        if q.x < 0.0 || q.y < 0.0 {
            return None;
        }
        let (x, y) = (q.x as usize, q.y as usize);
        if x >= self.nx || y >= self.ny {
            return None;
        }
        let mut best: Option<(f64, f64)> = None;
        for &i in &self.cells[y * self.nx + x] {
            let i = i as usize;
            let (d, t) = geom::dist_point_seg(p, f.pts[i], f.pts[i + 1]);
            if d < 0.05 && best.is_none_or(|b| d < b.1) {
                best = Some((f.s[i] + t * (f.s[i + 1] - f.s[i]), d));
            }
        }
        best
    }
}

/// A random (or showcase) plot definition.
#[derive(Clone, Debug)]
pub struct PlotDef {
    pub kind: Kind,
    pub zone: Zone,
    pub prims: Vec<Prim>,
    pub depth: f64,
    pub seed: u64,
    /// Offset added to N auto (−3..=3).
    pub dn: i32,
}

fn rng_range(r: &mut Pcg32, lo: f64, hi: f64) -> f64 {
    lo + (hi - lo) * r.next_f64()
}

pub fn random_plot(seed: u64) -> PlotDef {
    let mut r = Pcg32::from_seed(seed);
    let kind = match r.below(6) {
        0 => Kind::Straight,
        1 => Kind::Convex,
        2 => Kind::Concave,
        3 => Kind::SCurve,
        4 => Kind::Corner,
        _ => Kind::TwoSeg,
    };
    let zone = match r.weighted_index(&[0.6, 0.25, 0.15]) {
        Some(0) => Zone::R,
        Some(1) => Zone::C,
        _ => Zone::I,
    };
    let zp = zone.params();
    let lmin = (zp.w_min * 1.2).max(20.0);
    let mut depth = rng_range(&mut r, 15.0, 80.0);
    let prims = match kind {
        Kind::Straight => vec![Prim::Line(rng_range(&mut r, lmin, 250.0))],
        Kind::Convex => {
            let rad = rng_range(&mut r, 30.0, 300.0);
            let len = rng_range(&mut r, lmin, 250.0).min(rad * 150f64.to_radians());
            vec![Prim::Arc(rad, -len / rad)]
        }
        Kind::Concave => {
            let rad = rng_range(&mut r, 25.0, 300.0);
            depth = depth.min(0.8 * rad);
            let len = rng_range(&mut r, lmin, 250.0).min(rad * 150f64.to_radians());
            vec![Prim::Arc(rad, len / rad)]
        }
        Kind::SCurve => {
            let r1 = rng_range(&mut r, 40.0, 300.0);
            let r2 = rng_range(&mut r, 40.0, 300.0);
            let l1 = rng_range(&mut r, lmin * 0.5, 120.0).min(r1 * 90f64.to_radians());
            let l2 = rng_range(&mut r, lmin * 0.5, 120.0).min(r2 * 90f64.to_radians());
            let first_convex = r.chance(0.5);
            let (s1, s2) = if first_convex {
                (-1.0, 1.0)
            } else {
                (1.0, -1.0)
            };
            let concave_r = if first_convex { r2 } else { r1 };
            depth = depth.min(0.8 * concave_r);
            vec![Prim::Arc(r1, s1 * l1 / r1), Prim::Arc(r2, s2 * l2 / r2)]
        }
        Kind::Corner => {
            let alpha = rng_range(&mut r, 60.0, 120.0);
            let rf = rng_range(&mut r, 6.0, 12.0);
            let la = rng_range(&mut r, lmin * 0.6, 150.0);
            let lb = rng_range(&mut r, lmin * 0.6, 150.0);
            vec![
                Prim::Line(la),
                Prim::Arc(rf, (180.0 - alpha).to_radians()),
                Prim::Line(lb),
            ]
        }
        Kind::TwoSeg => {
            let kink = rng_range(&mut r, 10.0, 40.0) * if r.chance(0.5) { 1.0 } else { -1.0 };
            let la = rng_range(&mut r, lmin * 0.5, 130.0);
            let lb = rng_range(&mut r, lmin * 0.5, 130.0);
            vec![
                Prim::Line(la),
                Prim::Arc(10.0, kink.to_radians()),
                Prim::Line(lb),
            ]
        }
    };
    let dn = r.below(7) as i32 - 3;
    PlotDef {
        kind,
        zone,
        prims,
        depth,
        seed,
        dn,
    }
}

/// 24-plot showcase set (fixed, typical cases incl. the extremes).
pub fn showcase() -> Vec<PlotDef> {
    let d = |deg: f64| deg.to_radians();
    let mk = |kind, zone, prims: Vec<Prim>, depth| PlotDef {
        kind,
        zone,
        prims,
        depth,
        seed: 0x5EED,
        dn: 0,
    };
    let mut v = vec![
        mk(Kind::Straight, Zone::R, vec![Prim::Line(120.0)], 30.0),
        mk(Kind::Straight, Zone::C, vec![Prim::Line(140.0)], 35.0),
        mk(Kind::Straight, Zone::I, vec![Prim::Line(180.0)], 50.0),
        mk(Kind::Straight, Zone::R, vec![Prim::Line(90.0)], 70.0),
        mk(
            Kind::Convex,
            Zone::R,
            vec![Prim::Arc(30.0, -d(150.0))],
            30.0,
        ),
        mk(
            Kind::Convex,
            Zone::R,
            vec![Prim::Arc(60.0, -d(110.0))],
            30.0,
        ),
        mk(
            Kind::Convex,
            Zone::C,
            vec![Prim::Arc(150.0, -d(60.0))],
            35.0,
        ),
        mk(
            Kind::Convex,
            Zone::I,
            vec![Prim::Arc(300.0, -d(35.0))],
            50.0,
        ),
        mk(
            Kind::Concave,
            Zone::R,
            vec![Prim::Arc(25.0, d(150.0))],
            20.0,
        ),
        mk(
            Kind::Concave,
            Zone::R,
            vec![Prim::Arc(50.0, d(120.0))],
            30.0,
        ),
        mk(
            Kind::Concave,
            Zone::C,
            vec![Prim::Arc(120.0, d(70.0))],
            35.0,
        ),
        mk(
            Kind::Concave,
            Zone::I,
            vec![Prim::Arc(300.0, d(35.0))],
            50.0,
        ),
        mk(
            Kind::SCurve,
            Zone::R,
            vec![Prim::Arc(60.0, -d(60.0)), Prim::Arc(60.0, d(60.0))],
            30.0,
        ),
        mk(
            Kind::SCurve,
            Zone::R,
            vec![Prim::Arc(80.0, d(50.0)), Prim::Arc(50.0, -d(70.0))],
            30.0,
        ),
        mk(
            Kind::SCurve,
            Zone::C,
            vec![Prim::Arc(150.0, -d(35.0)), Prim::Arc(100.0, d(40.0))],
            35.0,
        ),
        mk(
            Kind::SCurve,
            Zone::I,
            vec![Prim::Arc(250.0, d(25.0)), Prim::Arc(250.0, -d(25.0))],
            50.0,
        ),
        mk(
            Kind::Corner,
            Zone::R,
            vec![Prim::Line(70.0), Prim::Arc(8.0, d(120.0)), Prim::Line(70.0)],
            30.0,
        ),
        mk(
            Kind::Corner,
            Zone::R,
            vec![Prim::Line(80.0), Prim::Arc(8.0, d(90.0)), Prim::Line(60.0)],
            30.0,
        ),
        mk(
            Kind::Corner,
            Zone::C,
            vec![Prim::Line(90.0), Prim::Arc(10.0, d(90.0)), Prim::Line(90.0)],
            35.0,
        ),
        mk(
            Kind::Corner,
            Zone::R,
            vec![Prim::Line(60.0), Prim::Arc(10.0, d(60.0)), Prim::Line(80.0)],
            30.0,
        ),
        mk(
            Kind::TwoSeg,
            Zone::R,
            vec![Prim::Line(60.0), Prim::Arc(10.0, d(15.0)), Prim::Line(60.0)],
            30.0,
        ),
        mk(
            Kind::TwoSeg,
            Zone::R,
            vec![
                Prim::Line(60.0),
                Prim::Arc(10.0, -d(35.0)),
                Prim::Line(60.0),
            ],
            30.0,
        ),
        mk(
            Kind::TwoSeg,
            Zone::C,
            vec![Prim::Line(70.0), Prim::Arc(10.0, d(35.0)), Prim::Line(70.0)],
            35.0,
        ),
        mk(
            Kind::TwoSeg,
            Zone::I,
            vec![
                Prim::Line(90.0),
                Prim::Arc(10.0, -d(20.0)),
                Prim::Line(90.0),
            ],
            50.0,
        ),
    ];
    for (i, p) in v.iter_mut().enumerate() {
        p.seed = 0x5EED_0000 + i as u64;
    }
    v
}

/// Frontage-end drag: lengthen/shorten the last primitive by `delta` metres of arclength.
pub fn drag_end(prims: &[Prim], delta: f64) -> Vec<Prim> {
    let mut p = prims.to_vec();
    if let Some(last) = p.last_mut() {
        *last = match *last {
            Prim::Line(l) => Prim::Line((l + delta).max(1.0)),
            Prim::Arc(r, t) => {
                let len = (r * t.abs() + delta).max(1.0);
                Prim::Arc(r, t.signum() * len / r)
            }
        };
    }
    p
}
