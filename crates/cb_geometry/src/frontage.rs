//! Frontage: the road-side edge a plot is built along (sidewalk outer edge), its arc-length
//! parametrisation, the concave arcs that need a medial-axis seam, kinks, and the plot polygon
//! built from it (ADR-0064, ALGORITHMS §9).
//!
//! Convention: the plot lies on the **left** of the frontage direction. A left turn curves
//! toward the plot (concave frontage, plot on the inside of the curve); a right turn curves away
//! from it (convex frontage, lots fan out). All math is `f64` (`DVec2`); callers convert at the
//! boundary.

use glam::DVec2;

use crate::boolean::f64ops;
use crate::d2::{self, Poly, cross, perp};

/// Vertex turns below this are treated as straight when detecting curved runs (rad, 0.05°).
const TURN_EPS_RAD: f64 = 0.05 * std::f64::consts::PI / 180.0;
/// Concave arcs turning at least this much are road corners (corner lot spans them).
pub const CORNER_TURN_DEG: f64 = 45.0;
/// Short sharp non-corner arcs turning more than this get a pinned split (kink).
pub const KINK_TURN_DEG: f64 = 25.0;
/// Arcs longer than this are curves, not kinks (m).
pub const KINK_MAX_LEN_M: f64 = 10.0;
/// Contact tolerance for "this point is on the frontage" (m).
pub(crate) const ON_FRONTAGE_M: f64 = 0.004;

/// One piece of a frontage description.
#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    /// Polyline vertices continuing from the current end (the first part also gives the start).
    /// Curved runs inside it are detected from vertex turns.
    Points(Vec<DVec2>),
    /// Circular arc from the current end around `center` by `turn_rad` (counter-clockwise
    /// positive = left = toward the plot). Used for junction fillets whose geometry is known.
    Arc {
        /// Arc centre.
        center: DVec2,
        /// Signed turn (rad).
        turn_rad: f64,
    },
}

/// Local primitive for building test/citygen frontages: the frontage starts at the origin
/// heading +x.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Prim {
    /// Straight run (m).
    Line(f64),
    /// Arc: radius (m), signed turn (rad, + = left = toward the plot).
    Arc(f64, f64),
}

/// Converts local primitives (origin, heading +x) to frontage parts.
#[must_use]
pub fn prims_to_parts(prims: &[Prim]) -> Vec<Part> {
    let mut parts = vec![Part::Points(vec![DVec2::ZERO])];
    let mut pos = DVec2::ZERO;
    let mut head = DVec2::X;
    for p in prims {
        match *p {
            Prim::Line(l) => {
                pos += head * l;
                parts.push(Part::Points(vec![pos]));
            }
            Prim::Arc(r, turn) => {
                let center = pos + perp(head) * r * turn.signum();
                parts.push(Part::Arc {
                    center,
                    turn_rad: turn,
                });
                pos = center + d2::rotate(pos - center, turn);
                head = d2::rotate(head, turn);
            }
        }
    }
    parts
}

/// Lengthens (or shortens) the last primitive by `delta` metres of arc length (edit fixture).
#[must_use]
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

/// A concave arc whose radius is below the plot depth: the medial axis enters the plot and
/// needs a seam (ray from the arc centre along the bisector, away from the arc).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeamArc {
    /// Arc start (frontage arc length, m).
    pub s0: f64,
    /// Arc end (m).
    pub s1: f64,
    /// Arc centre (seam start).
    pub center: DVec2,
    /// Seam direction (unit, from the arc through the centre).
    pub dir: DVec2,
    /// Radius (m).
    pub radius: f64,
    /// Turn ≥ 45°: a road corner (corner lot spanning it).
    pub is_corner: bool,
}

impl SeamArc {
    /// Arc midpoint (m).
    #[must_use]
    pub fn mid(&self) -> f64 {
        0.5 * (self.s0 + self.s1)
    }
}

/// An analysed frontage polyline.
#[derive(Clone, Debug)]
pub struct Frontage {
    pts: Vec<DVec2>,
    s: Vec<f64>,
    cum_turn: Vec<f64>,
    seams: Vec<SeamArc>,
    kinks: Vec<f64>,
}

/// A curved run found in the polyline (vertex index range, signed turn, circle).
struct Run {
    i0: usize,
    i1: usize,
    turn: f64,
    /// Known circle of an explicit arc.
    circle: Option<(DVec2, f64)>,
}

impl Frontage {
    /// Builds a frontage from local primitives (see [`Prim`]).
    #[must_use]
    pub fn from_prims(prims: &[Prim], depth: f64) -> Option<Self> {
        Self::new(&prims_to_parts(prims), depth)
    }

    /// Builds a frontage for a plot of the given depth. `None` if it has fewer than two
    /// distinct points.
    #[must_use]
    pub fn new(parts: &[Part], depth: f64) -> Option<Self> {
        let mut pts: Vec<DVec2> = Vec::new();
        // Explicit arcs: (first vertex, last vertex, centre, radius, signed turn).
        let mut arcs: Vec<(usize, usize, DVec2, f64, f64)> = Vec::new();
        let push = |pts: &mut Vec<DVec2>, p: DVec2| {
            if pts.last().is_none_or(|l| l.distance(p) > 1e-6) {
                pts.push(p);
            }
        };
        for part in parts {
            match part {
                Part::Points(v) => {
                    for p in v {
                        push(&mut pts, *p);
                    }
                }
                Part::Arc { center, turn_rad } => {
                    let start = *pts.last()?;
                    let rel = start - *center;
                    let r = rel.length();
                    let len = r * turn_rad.abs();
                    let steps = ((turn_rad.abs() / 3f64.to_radians()).ceil() as usize)
                        .max((len / 2.0).ceil() as usize)
                        .max(1);
                    let i0 = pts.len() - 1;
                    for k in 1..=steps {
                        push(
                            &mut pts,
                            *center + d2::rotate(rel, turn_rad * k as f64 / steps as f64),
                        );
                    }
                    arcs.push((i0, pts.len() - 1, *center, r, *turn_rad));
                }
            }
        }
        if pts.len() < 2 {
            return None;
        }
        let n = pts.len();
        let mut s = vec![0.0; n];
        for i in 1..n {
            s[i] = s[i - 1] + pts[i].distance(pts[i - 1]);
        }
        let mut turns = vec![0.0; n];
        for i in 1..n - 1 {
            turns[i] = d2::signed_angle(pts[i] - pts[i - 1], pts[i + 1] - pts[i]);
        }
        let mut cum_turn = vec![0.0; n];
        for i in 1..n {
            cum_turn[i] = cum_turn[i - 1] + turns[i].abs();
        }

        let mut runs: Vec<Run> = arcs
            .iter()
            .map(|&(i0, i1, c, r, t)| Run {
                i0,
                i1,
                turn: t,
                circle: Some((c, r)),
            })
            .collect();
        // Curved runs inside polyline parts: consecutive same-sign turning vertices.
        let in_arc = |i: usize| arcs.iter().any(|a| i >= a.0 && i <= a.1);
        let mut cur: Option<(usize, usize, f64)> = None;
        for (i, &turn_i) in turns.iter().enumerate().skip(1) {
            let t = if i < n - 1 && !in_arc(i) { turn_i } else { 0.0 };
            let turning = t.abs() > TURN_EPS_RAD;
            match cur {
                Some((a, _, acc)) if turning && acc.signum() == t.signum() => {
                    cur = Some((a, i, acc + t));
                }
                _ => {
                    if let Some((a, b, acc)) = cur.take() {
                        runs.push(Run {
                            i0: a,
                            i1: b,
                            turn: acc,
                            circle: None,
                        });
                    }
                    if turning {
                        cur = Some((i, i, t));
                    }
                }
            }
        }
        runs.sort_by_key(|r| r.i0);

        let mut seams = Vec::new();
        let mut kinks = Vec::new();
        for run in &runs {
            let (s0, s1) = (s[run.i0], s[run.i1]);
            let len = s1 - s0;
            let turn = run.turn;
            let mid_s = 0.5 * (s0 + s1);
            if turn.abs() > KINK_TURN_DEG.to_radians()
                && turn.abs() < CORNER_TURN_DEG.to_radians()
                && len < KINK_MAX_LEN_M
            {
                kinks.push(mid_s);
            }
            if turn <= 0.0 {
                continue;
            }
            // Chord lengths underestimate the arc slightly; explicit arcs use the exact radius.
            let radius = run.circle.map_or(len / turn, |c| c.1);
            if radius >= depth {
                continue;
            }
            let mid_pt = point_at_impl(&pts, &s, mid_s);
            let center = run
                .circle
                .map(|c| c.0)
                .or_else(|| circumcenter(pts[run.i0], pts[(run.i0 + run.i1) / 2], pts[run.i1]))
                .unwrap_or_else(|| {
                    // Sharp corner without a fillet: start the seam just inside the corner.
                    let a = (pts[run.i0] - pts[run.i0.saturating_sub(1)]).normalize_or_zero();
                    let b = (pts[(run.i1 + 1).min(n - 1)] - pts[run.i1]).normalize_or_zero();
                    mid_pt + (perp(a) + perp(b)).normalize_or(perp(a)) * radius.max(0.05)
                });
            let dir = (center - mid_pt).normalize_or(perp(tangent_impl(&pts, &s, mid_s)));
            seams.push(SeamArc {
                s0,
                s1,
                center,
                dir,
                radius,
                is_corner: turn >= CORNER_TURN_DEG.to_radians(),
            });
        }
        Some(Self {
            pts,
            s,
            cum_turn,
            seams,
            kinks,
        })
    }

    /// Polyline vertices.
    #[must_use]
    pub fn points(&self) -> &[DVec2] {
        &self.pts
    }

    /// Cumulative arc length per vertex.
    #[must_use]
    pub fn arc_lengths(&self) -> &[f64] {
        &self.s
    }

    /// Concave arcs tighter than the depth.
    #[must_use]
    pub fn seams(&self) -> &[SeamArc] {
        &self.seams
    }

    /// The first corner arc, if any.
    #[must_use]
    pub fn corner(&self) -> Option<&SeamArc> {
        self.seams.iter().find(|c| c.is_corner)
    }

    /// Kink midpoints (arc length): a split is pinned there so no lot spans the whole kink.
    #[must_use]
    pub fn kinks(&self) -> &[f64] {
        &self.kinks
    }

    /// Total length (m).
    #[must_use]
    pub fn len(&self) -> f64 {
        self.s.last().copied().unwrap_or(0.0)
    }

    /// Whether the frontage has zero length.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() <= 0.0
    }

    /// Absolute heading change at vertices strictly inside `(a, b)` (rad).
    #[must_use]
    pub fn turn(&self, a: f64, b: f64) -> f64 {
        let i0 = self.s.partition_point(|v| *v <= a);
        let i1 = self.s.partition_point(|v| *v < b);
        if i1 <= i0 {
            return 0.0;
        }
        self.cum_turn[i1 - 1] - if i0 == 0 { 0.0 } else { self.cum_turn[i0 - 1] }
    }

    /// Point at arc length `s` (clamped).
    #[must_use]
    pub fn point_at(&self, s: f64) -> DVec2 {
        point_at_impl(&self.pts, &self.s, s)
    }

    /// Unit tangent at arc length `s`.
    #[must_use]
    pub fn tangent_at(&self, s: f64) -> DVec2 {
        tangent_impl(&self.pts, &self.s, s)
    }

    /// Inward (plot-side) unit normal at arc length `s`.
    #[must_use]
    pub fn normal_at(&self, s: f64) -> DVec2 {
        perp(self.tangent_at(s))
    }

    fn seg_normal(&self, i: usize) -> DVec2 {
        perp((self.pts[i + 1] - self.pts[i]).normalize_or_zero())
    }

    /// Length-weighted mean inward normal over `[a, b]`.
    #[must_use]
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

    /// `(arc length, distance)` of the nearest frontage point.
    #[must_use]
    pub fn project(&self, p: DVec2) -> (f64, f64) {
        let mut best = (0.0, f64::INFINITY);
        for i in 0..self.pts.len() - 1 {
            let (d, t) = d2::dist_point_seg(p, self.pts[i], self.pts[i + 1]);
            if d < best.1 {
                best = (self.s[i] + t * (self.s[i + 1] - self.s[i]), d);
            }
        }
        best
    }

    /// Plot polygon (ALGORITHMS §9): union of per-segment depth quads plus wedges at convex
    /// vertices (one-sided offset band), minus a `road_clip_m` band on the road side
    /// (auto-clip), keeping the component that holds the frontage. Counter-clockwise.
    #[must_use]
    pub fn plot_polygon(&self, depth: f64, road_clip_m: f64) -> Option<Poly> {
        let n = self.pts.len();
        let mut parts: Vec<Poly> = Vec::with_capacity(2 * n);
        let mut road: Vec<Poly> = Vec::with_capacity(2 * n);
        for i in 0..n - 1 {
            let nrm = self.seg_normal(i);
            let (a, b) = (self.pts[i], self.pts[i + 1]);
            parts.push(vec![a, b, b + nrm * depth, a + nrm * depth]);
            road.push(vec![a, a - nrm * road_clip_m, b - nrm * road_clip_m, b]);
            if i + 2 < n {
                let n2 = self.seg_normal(i + 1);
                let mid = (nrm + n2).normalize_or(nrm);
                if cross(b - a, self.pts[i + 2] - b) < 0.0 {
                    // Right turn (away from the plot) leaves a wedge gap on the plot side.
                    parts.push(vec![b, b + n2 * depth, b + mid * depth, b + nrm * depth]);
                } else {
                    road.push(vec![
                        b,
                        b - nrm * road_clip_m,
                        b - mid * road_clip_m,
                        b - n2 * road_clip_m,
                    ]);
                }
            }
        }
        let (u, holes) = f64ops::union_all(&parts);
        if u.len() != 1 || holes != 0 {
            return None;
        }
        let pieces = f64ops::difference(&u[0], &road);
        let mid = 0.5 * self.len();
        let probe = self.point_at(mid) + self.normal_at(mid) * 0.05;
        let main = pieces.into_iter().find(|p| d2::point_in_poly(probe, p))?;
        Some(d2::simplify(&main, 0.002))
    }
}

fn seg_at(s_tab: &[f64], s: f64) -> usize {
    let n = s_tab.len() - 1;
    match s_tab.binary_search_by(|v| v.total_cmp(&s)) {
        Ok(i) => i.min(n - 1),
        Err(i) => i.saturating_sub(1).min(n - 1),
    }
}

fn point_at_impl(pts: &[DVec2], s_tab: &[f64], s: f64) -> DVec2 {
    let i = seg_at(s_tab, s);
    let (s0, s1) = (s_tab[i], s_tab[i + 1]);
    let t = if s1 > s0 {
        ((s - s0) / (s1 - s0)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    pts[i].lerp(pts[i + 1], t)
}

fn tangent_impl(pts: &[DVec2], s_tab: &[f64], s: f64) -> DVec2 {
    let i = seg_at(s_tab, s);
    (pts[i + 1] - pts[i]).normalize_or(DVec2::X)
}

/// Circumcentre of three points (`None` if nearly collinear).
fn circumcenter(a: DVec2, b: DVec2, c: DVec2) -> Option<DVec2> {
    let (ab, ac) = (b - a, c - a);
    let d = 2.0 * cross(ab, ac);
    if d.abs() < 1e-9 {
        return None;
    }
    let (l1, l2) = (ab.length_squared(), ac.length_squared());
    Some(a + DVec2::new(ac.y * l1 - ab.y * l2, ab.x * l2 - ac.x * l1) / d)
}

/// Uniform grid over frontage segments for near-point queries (on-frontage tests).
pub(crate) struct FrontageIndex {
    lo: DVec2,
    cell: f64,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<u32>>,
}

impl FrontageIndex {
    pub(crate) fn new(f: &Frontage) -> Self {
        let (lo, hi) = d2::bbox(&f.pts);
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
        Self {
            lo,
            cell,
            nx,
            ny,
            cells,
        }
    }

    /// `(s, d)` of the nearest frontage point if one lies within 0.05 m.
    pub(crate) fn near(&self, f: &Frontage, p: DVec2) -> Option<(f64, f64)> {
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
            let (d, t) = d2::dist_point_seg(p, f.pts[i], f.pts[i + 1]);
            if d < 0.05 && best.is_none_or(|b| d < b.1) {
                best = Some((f.s[i] + t * (f.s[i + 1] - f.s[i]), d));
            }
        }
        best
    }

    /// Whether `p` lies on the frontage (within [`ON_FRONTAGE_M`]).
    pub(crate) fn on(&self, f: &Frontage, p: DVec2) -> bool {
        self.near(f, p).is_some_and(|v| v.1 < ON_FRONTAGE_M)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_frontage_plot() {
        let f = Frontage::from_prims(&[Prim::Line(100.0)], 30.0).unwrap();
        assert!((f.len() - 100.0).abs() < 1e-9);
        assert!(f.seams().is_empty() && f.kinks().is_empty());
        let p = f.plot_polygon(30.0, 12.0).unwrap();
        assert!((d2::area(&p) - 3000.0).abs() < 0.5);
        assert!((f.normal_at(10.0) - DVec2::Y).length() < 1e-12);
    }

    #[test]
    fn corner_fillet_has_corner_seam() {
        let prims = [
            Prim::Line(60.0),
            Prim::Arc(8.0, 90f64.to_radians()),
            Prim::Line(60.0),
        ];
        let f = Frontage::from_prims(&prims, 30.0).unwrap();
        let c = f.corner().unwrap();
        assert!((c.radius - 8.0).abs() < 1e-6);
        assert!((c.s0 - 60.0).abs() < 1e-6);
        assert!((c.center - DVec2::new(60.0, 8.0)).length() < 1e-9);
        assert!(f.plot_polygon(30.0, 12.0).is_some());
    }

    #[test]
    fn detected_run_matches_explicit_arc() {
        // Same corner given as a sampled polyline instead of an explicit arc.
        let explicit = Frontage::from_prims(
            &[
                Prim::Line(40.0),
                Prim::Arc(10.0, 90f64.to_radians()),
                Prim::Line(40.0),
            ],
            30.0,
        )
        .unwrap();
        let f = Frontage::new(&[Part::Points(explicit.points().to_vec())], 30.0).unwrap();
        let (a, b) = (f.corner().unwrap(), explicit.corner().unwrap());
        assert!(
            (a.radius - b.radius).abs() < 2e-3,
            "{} vs {}",
            a.radius,
            b.radius
        );
        assert!((a.center - b.center).length() < 1e-6);
        assert!((a.s0 - b.s0).abs() < 1e-9 && (a.s1 - b.s1).abs() < 1e-9);
    }

    #[test]
    fn kink_detected() {
        let f = Frontage::from_prims(
            &[
                Prim::Line(60.0),
                Prim::Arc(10.0, -(35f64.to_radians())),
                Prim::Line(60.0),
            ],
            30.0,
        )
        .unwrap();
        assert_eq!(f.kinks().len(), 1);
        assert!(f.seams().is_empty(), "convex kink needs no seam");
    }
}
