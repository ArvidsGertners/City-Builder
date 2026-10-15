//! Spike S-J (PROGRESS.md Phase S): junction geometry per ALGORITHMS §5 / ADR-0045 — cut-backs,
//! polygon, connectors with curvature limits, conflict zones, per-vertex surface heights — on
//! 10 000 random junctions plus canonical cases. Prints `metric=` lines, dumps SVGs.
//!
//! Usage: s_j [--random N] [--svg DIR]

use std::fmt::Write as _;
use std::time::Instant;

use cb_core::rng::Pcg32;
use glam::DVec2;

// ---------- road types ----------
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Dirt,
    Street,
    OneWayStreet,
    Avenue,
    OneWayAvenue,
    AvenueTram,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum LaneKind {
    Car,
    Tram,
}

struct CrossSection {
    width: f64,
    /// Lane offsets along the left normal, + = incoming side (right-hand traffic), with kind.
    lanes: Vec<(f64, LaneKind, bool)>, // (offset, kind, incoming)
    corner_radius: f64,
    max_grade: f64,
    min_radius: f64,
    sidewalk: bool,
}

fn section(kind: Kind, one_way_incoming: bool) -> CrossSection {
    use LaneKind::{Car, Tram};
    match kind {
        Kind::Dirt => CrossSection { width: 6.0, lanes: vec![(1.5, Car, true), (-1.5, Car, false)], corner_radius: 4.0, max_grade: 0.14, min_radius: 8.0, sidewalk: false },
        Kind::Street => CrossSection { width: 12.0, lanes: vec![(2.0, Car, true), (-2.0, Car, false)], corner_radius: 6.0, max_grade: 0.10, min_radius: 12.0, sidewalk: true },
        Kind::OneWayStreet => CrossSection { width: 12.0, lanes: vec![(2.0, Car, one_way_incoming), (-2.0, Car, one_way_incoming)], corner_radius: 6.0, max_grade: 0.10, min_radius: 12.0, sidewalk: true },
        Kind::Avenue => CrossSection {
            width: 22.0,
            lanes: vec![(7.0, Car, true), (3.0, Car, true), (-3.0, Car, false), (-7.0, Car, false)],
            corner_radius: 9.0,
            max_grade: 0.08,
            min_radius: 25.0,
            sidewalk: true,
        },
        Kind::OneWayAvenue => CrossSection {
            width: 16.0,
            lanes: vec![(4.0, Car, one_way_incoming), (0.0, Car, one_way_incoming), (-4.0, Car, one_way_incoming)],
            corner_radius: 9.0,
            max_grade: 0.08,
            min_radius: 25.0,
            sidewalk: true,
        },
        Kind::AvenueTram => CrossSection {
            width: 28.0,
            lanes: vec![(10.0, Car, true), (6.0, Car, true), (1.6, Tram, true), (-1.6, Tram, false), (-6.0, Car, false), (-10.0, Car, false)],
            corner_radius: 9.0,
            max_grade: 0.06,
            min_radius: 25.0,
            sidewalk: true,
        },
    }
}

// ---------- approaches (constant-curvature centerlines leaving the node) ----------
#[derive(Clone, Copy, Debug)]
struct Approach {
    kind: Kind,
    one_way_incoming: bool,
    angle: f64,     // outgoing tangent angle at the node (rad)
    curvature: f64, // 1/m, + = turning left (CCW) when walking away from the node
    length: f64,    // m
    grade: f64,     // rise per m walking away from the node
}

impl Approach {
    fn heading(&self, s: f64) -> f64 {
        self.angle + self.curvature * s
    }
    fn point(&self, s: f64) -> DVec2 {
        if self.curvature.abs() < 1e-9 {
            DVec2::new(self.angle.cos(), self.angle.sin()) * s
        } else {
            let k = self.curvature;
            DVec2::new(((self.angle + k * s).sin() - self.angle.sin()) / k, (self.angle.cos() - (self.angle + k * s).cos()) / k)
        }
    }
    fn tangent(&self, s: f64) -> DVec2 {
        let h = self.heading(s);
        DVec2::new(h.cos(), h.sin())
    }
    fn normal(&self, s: f64) -> DVec2 {
        self.tangent(s).perp()
    }
    /// Profile height with the ≤ 4 % landing over [0, cut + 4 m] (ADR-0045).
    fn height(&self, s: f64, cut: f64) -> f64 {
        let landing = (cut + 4.0).min(self.length);
        let g_land = self.grade.clamp(-0.04, 0.04);
        if s <= landing { g_land * s } else { g_land * landing + self.grade * (s - landing) }
    }
}

thread_local! {
    static DIAG: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}
thread_local! {
    static OVERSHOOT: std::cell::RefCell<Vec<f64>> = const { std::cell::RefCell::new(Vec::new()) };
}

thread_local! {
    static INVALID: std::cell::RefCell<Option<Vec<DVec2>>> = const { std::cell::RefCell::new(None) };
}

fn diag(msg: String) {
    DIAG.with(|d| d.borrow_mut().push(msg));
}

/// Sharpest allowed movement deflection at a junction.
const MAX_TURN_DEG: f64 = 135.0;

// ---------- errors ----------
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
enum ValidationError {
    DegreeTooHigh,
    AngleTooSmall,
    TooClose,
    ConnectorTooSharp,
    PolygonInvalid,
}

// ---------- outputs ----------
struct Connector {
    from: usize,
    to: usize,
    kind: LaneKind,
    pts: Vec<DVec2>,
    max_curvature: f64,
}

struct Junction {
    cuts: Vec<f64>,
    polygon: Vec<DVec2>,
    heights: Vec<f64>,
    connectors: Vec<Connector>,
    conflict_zones: usize,
    crosswalks: usize,
    max_cut_step_m: f64,
}

fn sample_edge(a: &Approach, offset: f64, step: f64) -> Vec<(f64, DVec2)> {
    let n = (a.length / step).ceil().max(1.0) as usize;
    (0..=n).map(|i| {
        let s = a.length * i as f64 / n as f64;
        (s, a.point(s) + a.normal(s) * offset)
    }).collect()
}

fn seg_intersect(p: DVec2, p2: DVec2, q: DVec2, q2: DVec2) -> Option<(f64, f64)> {
    let r = p2 - p;
    let s = q2 - q;
    let denom = r.perp_dot(s);
    if denom.abs() < 1e-12 {
        return None;
    }
    let t = (q - p).perp_dot(s) / denom;
    let u = (q - p).perp_dot(r) / denom;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some((t, u))
}

/// Arc-length positions where A's left edge meets B's right edge (first crossing along A).
fn edge_crossing(a: &Approach, wa: f64, b: &Approach, wb: f64) -> Option<(f64, f64)> {
    let ea = sample_edge(a, wa / 2.0, 0.5);
    let eb = sample_edge(b, -wb / 2.0, 0.5);
    let mut best: Option<(f64, f64)> = None;
    // Chunk bounding boxes (8 segments each) so only nearby chunks are tested pairwise.
    let chunk = 8;
    let bbox = |pts: &[(f64, DVec2)]| pts.iter().fold((DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
    let ca: Vec<(DVec2, DVec2)> = (0..ea.len() - 1).step_by(chunk).map(|i| bbox(&ea[i..(i + chunk + 1).min(ea.len())])).collect();
    let cb: Vec<(DVec2, DVec2)> = (0..eb.len() - 1).step_by(chunk).map(|j| bbox(&eb[j..(j + chunk + 1).min(eb.len())])).collect();
    for (ci, ba) in ca.iter().enumerate() {
        if best.is_some_and(|(bs, _)| ea[ci * chunk].0 > bs) {
            break;
        }
        for (cj, bb) in cb.iter().enumerate() {
            if ba.0.x > bb.1.x || bb.0.x > ba.1.x || ba.0.y > bb.1.y || bb.0.y > ba.1.y {
                continue;
            }
            for i in ci * chunk..((ci + 1) * chunk).min(ea.len() - 1) {
                for j in cj * chunk..((cj + 1) * chunk).min(eb.len() - 1) {
            if let Some((t, u)) = seg_intersect(ea[i].1, ea[i + 1].1, eb[j].1, eb[j + 1].1) {
                let sa = ea[i].0 + t * (ea[i + 1].0 - ea[i].0);
                let sb = eb[j].0 + u * (eb[j + 1].0 - eb[j].0);
                if best.is_none_or(|(bs, _)| sa < bs) {
                    best = Some((sa, sb));
                }
            }
                }
            }
        }
    }
    best
}

/// Boundary on the open (reflex) side between A's left kerb and B's right kerb: follow both curved
/// kerbs back from their cut lines to where they meet (extended past the node if needed).
fn reflex_corner(a: &Approach, hwa: f64, cut_a: f64, b: &Approach, hwb: f64, cut_b: f64) -> Vec<DVec2> {
    let step = 0.5;
    let ea: Vec<(f64, DVec2)> = {
        let n = ((cut_a + 30.0) / step).ceil() as usize;
        (0..=n).map(|k| { let s = cut_a - k as f64 * step; (s, a.point(s) + a.normal(s) * hwa) }).collect()
    };
    let eb: Vec<(f64, DVec2)> = {
        let n = ((cut_b + 30.0) / step).ceil() as usize;
        (0..=n).map(|k| { let s = cut_b - k as f64 * step; (s, b.point(s) - b.normal(s) * hwb) }).collect()
    };
    for i in 0..ea.len() - 1 {
        for j in 0..eb.len() - 1 {
            if let Some((t, _u)) = seg_intersect(ea[i].1, ea[i + 1].1, eb[j].1, eb[j + 1].1) {
                let x = ea[i].1 + (ea[i + 1].1 - ea[i].1) * t;
                let mut out: Vec<DVec2> = ea[..=i].iter().map(|e| e.1).collect();
                out.push(x);
                out.extend(eb[..=j].iter().rev().map(|e| e.1));
                return out;
            }
        }
    }
    vec![ea[0].1, eb[0].1]
}

fn bezier(p0: DVec2, p1: DVec2, p2: DVec2, p3: DVec2, n: usize) -> Vec<DVec2> {
    (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            p0 * u * u * u + p1 * 3.0 * u * u * t + p2 * 3.0 * u * t * t + p3 * t * t * t
        })
        .collect()
}

fn bezier_max_curvature(p0: DVec2, p1: DVec2, p2: DVec2, p3: DVec2) -> f64 {
    let mut kmax: f64 = 0.0;
    for i in 0..=64 {
        let t = i as f64 / 64.0;
        let u = 1.0 - t;
        let d1 = (p1 - p0) * 3.0 * u * u + (p2 - p1) * 6.0 * u * t + (p3 - p2) * 3.0 * t * t;
        let d2 = (p2 - p1 * 2.0 + p0) * 6.0 * u + (p3 - p2 * 2.0 + p1) * 6.0 * t;
        let sp = d1.length();
        if sp > 1e-9 {
            kmax = kmax.max(d1.perp_dot(d2).abs() / (sp * sp * sp));
        }
    }
    kmax
}

fn polygon_is_simple(poly: &[DVec2]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    for i in 0..n {
        let (a, a2) = (poly[i], poly[(i + 1) % n]);
        if (a2 - a).length() < 1e-9 {
            continue;
        }
        for j in i + 1..n {
            if j == i || (j + 1) % n == i || (i + 1) % n == j {
                continue;
            }
            let (b, b2) = (poly[j], poly[(j + 1) % n]);
            if (b2 - b).length() < 1e-9 {
                continue;
            }
            if let Some((t, u)) = seg_intersect(a, a2, b, b2)
                && t > 1e-9 && t < 1.0 - 1e-9 && u > 1e-9 && u < 1.0 - 1e-9
            {
                return false;
            }
        }
    }
    signed_area(poly) > 0.0
}

fn signed_area(poly: &[DVec2]) -> f64 {
    let n = poly.len();
    (0..n).map(|i| poly[i].perp_dot(poly[(i + 1) % n])).sum::<f64>() * 0.5
}

fn point_in_polygon(p: DVec2, poly: &[DVec2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn dist_to_segment(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let t = if ab.length_squared() < 1e-12 { 0.0 } else { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) };
    (a + ab * t - p).length()
}

fn dist_to_polygon(p: DVec2, poly: &[DVec2]) -> f64 {
    (0..poly.len()).map(|i| dist_to_segment(p, poly[i], poly[(i + 1) % poly.len()])).fold(f64::INFINITY, f64::min)
}

fn polylines_cross(a: &[DVec2], b: &[DVec2]) -> bool {
    // Connectors from one lane share their start (and merges share their end): contacts within
    // 0.5 m of a shared endpoint are tangential touching, not crossings.
    let shared: Vec<DVec2> = [a[0], a[a.len() - 1]]
        .into_iter()
        .filter(|p| (*p - b[0]).length() < 1e-6 || (*p - b[b.len() - 1]).length() < 1e-6)
        .collect();
    for i in 0..a.len() - 1 {
        for j in 0..b.len() - 1 {
            if let Some((t, _u)) = seg_intersect(a[i], a[i + 1], b[j], b[j + 1]) {
                let x = a[i] + (a[i + 1] - a[i]) * t;
                if !shared.iter().any(|p| (*p - x).length() < 0.5) {
                    return true;
                }
            }
        }
    }
    false
}

fn polylines_min_dist(a: &[DVec2], b: &[DVec2]) -> f64 {
    let mut d = f64::INFINITY;
    for p in a {
        for j in 0..b.len() - 1 {
            d = d.min(dist_to_segment(*p, b[j], b[j + 1]));
        }
    }
    d
}

fn angle_wrap(a: f64) -> f64 {
    let t = std::f64::consts::TAU;
    ((a % t) + t) % t
}

/// Builds the junction or a typed error.
fn build(input: &[Approach]) -> Result<Junction, ValidationError> {
    if input.len() > 6 {
        return Err(ValidationError::DegreeTooHigh);
    }
    let mut apps: Vec<Approach> = input.to_vec();
    apps.sort_by(|a, b| angle_wrap(a.angle).total_cmp(&angle_wrap(b.angle)));
    let n = apps.len();
    let secs: Vec<CrossSection> = apps.iter().map(|a| section(a.kind, a.one_way_incoming)).collect();
    for i in 0..n {
        let d = angle_wrap(apps[(i + 1) % n].angle - apps[i].angle);
        if n > 1 && d.to_degrees() < 25.0 - 1e-9 {
            return Err(ValidationError::AngleTooSmall);
        }
    }

    // 1–2: cut-backs from neighbour edge crossings, half the widest crossing width, corner radius.
    let mut req = vec![0.0f64; n];
    if n >= 2 {
        for i in 0..n {
            let j = (i + 1) % n;
            if let Some((sa, sb)) = edge_crossing(&apps[i], secs[i].width, &apps[j], secs[j].width) {
                req[i] = req[i].max(sa);
                req[j] = req[j].max(sb);
            }
        }
    }
    // Corner allowance: fillet tangent length min(r, r / tan(α/2)) for the corner's interior angle α
    // (acute corners get a proportionally smaller radius, as in real kerb design).
    let corner = |i: usize, j: usize| -> f64 {
        let alpha = angle_wrap(apps[j].angle - apps[i].angle).min(std::f64::consts::PI - 1e-6);
        let r = secs[i].corner_radius.min(secs[j].corner_radius);
        r.min(r / (alpha / 2.0).tan().max(1e-6))
    };
    let mut cuts: Vec<f64> = (0..n)
        .map(|i| {
            if n <= 2 {
                return secs[i].corner_radius;
            }
            let widest_other = (0..n).filter(|&k| k != i).map(|k| secs[k].width).fold(0.0, f64::max);
            let next = (i + 1) % n;
            let prev = (i + n - 1) % n;
            req[i].max(widest_other / 2.0) + corner(i, next).max(corner(prev, i))
        })
        .collect();

    // Iterate: grow cut-backs until connectors satisfy curvature limits (or cap → error).
    for attempt in 0..40 {
        for i in 0..n {
            if cuts[i] > 0.45 * apps[i].length {
                diag(format!("too_close attempt={attempt} kind={:?} cut={:.1} len={:.1} req={:.1}", apps[i].kind, cuts[i], apps[i].length, req[i]));
                return Err(ValidationError::TooClose);
            }
        }
        let mut sharp: Vec<usize> = Vec::new();
        let mut connectors = Vec::new();
        // (from, to, kind, p0, p3, d_in, d_out, delta)
        let mut pending: Vec<(usize, usize, LaneKind, DVec2, DVec2, DVec2, DVec2, f64)> = Vec::new();
        for i in 0..n {
            let incoming: Vec<(f64, LaneKind)> = secs[i].lanes.iter().filter(|l| l.2).map(|l| (l.0, l.1)).collect();
            if incoming.is_empty() {
                continue;
            }
            // Targets ordered from sharpest right to sharpest left (signed deflection).
            let d_in = -apps[i].tangent(cuts[i]);
            let mut targets: Vec<(f64, usize)> = (0..n)
                .filter(|&j| j != i && secs[j].lanes.iter().any(|l| !l.2))
                .map(|j| {
                    let d_out = apps[j].tangent(cuts[j]);
                    (d_in.perp_dot(d_out).atan2(d_in.dot(d_out)).to_degrees(), j)
                })
                // Near-U-turn movements (> 135° deflection) are not allowed at junctions.
                .filter(|&(delta, _)| delta.abs() <= MAX_TURN_DEG)
                .collect();
            targets.sort_by(|a, b| a.0.total_cmp(&b.0));
            for lane_kind in [LaneKind::Car, LaneKind::Tram] {
                // incoming lanes, rightmost first (vehicle heading −t: right = +n = larger offset)
                let mut src: Vec<f64> = incoming.iter().filter(|l| l.1 == lane_kind).map(|l| l.0).collect();
                src.sort_by(|a, b| b.total_cmp(a));
                let tg: Vec<(f64, usize)> = targets
                    .iter()
                    .copied()
                    .filter(|&(delta, j)| {
                        secs[j].lanes.iter().any(|l| !l.2 && l.1 == lane_kind)
                            && (lane_kind == LaneKind::Car || delta.abs() <= 30.0 || matches!(apps[j].kind, Kind::AvenueTram))
                    })
                    .collect();
                if src.is_empty() || tg.is_empty() {
                    continue;
                }
                let t_count = tg.len();
                for (rank, &(delta, j)) in tg.iter().enumerate() {
                    // Monotone source-lane choice: right targets from right lanes, left from left.
                    let si = if t_count == 1 { 0 } else { (rank * (src.len() - 1) + (t_count - 1) / 2) / (t_count - 1) };
                    let mut dst: Vec<f64> = secs[j].lanes.iter().filter(|l| !l.2 && l.1 == lane_kind).map(|l| l.0).collect();
                    dst.sort_by(f64::total_cmp); // outgoing rightmost first (heading +t: right = −n)
                    let di = if rank == 0 { 0 } else if rank == t_count - 1 { dst.len() - 1 } else { si.min(dst.len() - 1) };
                    let d_out = apps[j].tangent(cuts[j]);
                    let p0 = apps[i].point(cuts[i]) + apps[i].normal(cuts[i]) * src[si];
                    let p3 = apps[j].point(cuts[j]) + apps[j].normal(cuts[j]) * dst[di];
                    pending.push((i, j, lane_kind, p0, p3, d_in, d_out, delta));
                }
            }
        }
        // Shared handles: connectors leaving one lane (same p0) use the same initial handle, and
        // connectors merging into one lane (same p3) the same final handle, so they never cross.
        let key = |p: DVec2| ((p.x * 1000.0).round() as i64, (p.y * 1000.0).round() as i64);
        let mut min_out: std::collections::BTreeMap<(i64, i64), f64> = std::collections::BTreeMap::new();
        let mut min_in: std::collections::BTreeMap<(i64, i64), f64> = std::collections::BTreeMap::new();
        for c in &pending {
            let chord = (c.4 - c.3).length();
            let e = min_out.entry(key(c.3)).or_insert(f64::INFINITY);
            *e = e.min(chord);
            let e = min_in.entry(key(c.4)).or_insert(f64::INFINITY);
            *e = e.min(chord);
        }
        for &(i, j, lane_kind, p0, p3, d_in, d_out, delta) in &pending {
            let chord = (p3 - p0).length();
            let _ = (&min_out, &min_in, key);
            let (h0, h3) = (0.4 * chord, 0.4 * chord);
            let (p1, p2) = (p0 + d_in * h0, p3 - d_out * h3);
            let kmax = bezier_max_curvature(p0, p1, p2, p3);
            let limit = if lane_kind == LaneKind::Tram { 1.0 / 20.0 } else { 1.0 / 5.0 };
            if kmax > limit * 1.001 {
                diag(format!("sharp kind={lane_kind:?} turn={delta:.0} chord={chord:.1} k={kmax:.3}"));
                sharp.push(i);
                sharp.push(j);
            }
            connectors.push(Connector { from: i, to: j, kind: lane_kind, pts: bezier(p0, p1, p2, p3, 24), max_curvature: kmax });
        }
        if !sharp.is_empty() {
            sharp.sort_unstable();
            sharp.dedup();
            for i in sharp {
                cuts[i] += 1.0;
            }
            continue;
        }

        // 3: polygon = cut lines (with camber vertices) + corner fillets.
        let mut polygon = Vec::new();
        let mut tags: Vec<String> = Vec::new();
        let mut heights = Vec::new();
        let camber = 0.02;
        let mut cut_vertex_ranges = Vec::new();
        for i in 0..n {
            let a = &apps[i];
            let c = cuts[i];
            let hw = secs[i].width / 2.0;
            let base_h = a.height(c, c);
            // right edge → left edge across the cut line, a vertex every ≤ 1 m (camber peak at 0)
            let steps = (secs[i].width).ceil() as usize;
            let start = polygon.len();
            for k in 0..=steps {
                let off = -hw + secs[i].width * k as f64 / steps as f64;
                polygon.push(a.point(c) + a.normal(c) * off);
                tags.push(format!("cut{i}"));
                heights.push(base_h - camber * off.abs());
            }
            cut_vertex_ranges.push((start, polygon.len()));
            if n >= 2 {
                let j = (i + 1) % n;
                let b = &apps[j];
                let l_pt = a.point(c) + a.normal(c) * hw;
                let r_pt = b.point(cuts[j]) - b.normal(cuts[j]) * secs[j].width / 2.0;
                let gap = angle_wrap(b.angle - a.angle).to_degrees();
                let (h_l, h_r) = (base_h - camber * hw, b.height(cuts[j], cuts[j]) - camber * secs[j].width / 2.0);
                let _ = gap;
                // Corner fillet: quadratic Bézier through the kerb-line intersection X
                // (L + a·(−t_A) = R + b·(−t_B), a, b > 0); straight edge if the kerbs diverge.
                let (da, db) = (-a.tangent(c), -b.tangent(cuts[j]));
                let denom = da.perp_dot(db);
                let fillet: Vec<DVec2> = if denom.abs() > 1e-9 {
                    let aa = (r_pt - l_pt).perp_dot(db) / denom;
                    let bb = (r_pt - l_pt).perp_dot(da) / denom;
                    if aa > 0.05 && bb > 0.05 {
                        let x = l_pt + da * aa;
                        (0..=8)
                            .map(|k| {
                                let t = k as f64 / 8.0;
                                l_pt * (1.0 - t) * (1.0 - t) + x * 2.0 * (1.0 - t) * t + r_pt * t * t
                            })
                            .collect()
                    } else {
                        reflex_corner(a, hw, c, b, secs[j].width / 2.0, cuts[j])
                    }
                } else {
                    reflex_corner(a, hw, c, b, secs[j].width / 2.0, cuts[j])
                };
                let m = fillet.len();
                for (k, p) in fillet.iter().enumerate().skip(1).take(m.saturating_sub(2)) {
                    polygon.push(*p);
                    tags.push(format!("corner{i}-{j}{}", if fillet.len() == 9 { "q" } else { "r" }));
                    let t = k as f64 / (m - 1) as f64;
                    heights.push(h_l * (1.0 - t) + h_r * t);
                }
            }
        }
        if n >= 3 && !polygon_is_simple(&polygon) {
            if std::env::var("SJ_DEBUG").is_ok() {
                let m = polygon.len();
                'outer: for x in 0..m {
                    for y in x + 2..m {
                        if (y + 1) % m == x { continue; }
                        if let Some((t, u)) = seg_intersect(polygon[x], polygon[(x + 1) % m], polygon[y], polygon[(y + 1) % m])
                            && t > 1e-9 && t < 1.0 - 1e-9 && u > 1e-9 && u < 1.0 - 1e-9 {
                            eprintln!("invalid: seg {x}({}->{}) x seg {y}({}->{}) area={:.1} n={n} cuts={:?}", tags[x], tags[(x+1)%m], tags[y], tags[(y+1)%m], signed_area(&polygon), cuts.iter().map(|c| (c*10.0).round()/10.0).collect::<Vec<_>>());
                            eprintln!("   apps: {:?}", apps.iter().zip(&secs).map(|(ap, sc)| format!("{:?} w{} ang{:.0} len{:.0}", ap.kind, sc.width, angle_wrap(ap.angle).to_degrees(), ap.length)).collect::<Vec<_>>());
                            break 'outer;
                        }
                    }
                }
                if signed_area(&polygon) <= 0.0 { eprintln!("invalid: non-positive area {:.1}", signed_area(&polygon)); }
            }
            // Grow the approaches whose cut lines / corners intersect, then rebuild.
            let m = polygon.len();
            let mut grow: Vec<usize> = Vec::new();
            for x in 0..m {
                for y in x + 2..m {
                    if (y + 1) % m == x {
                        continue;
                    }
                    if let Some((t, u)) = seg_intersect(polygon[x], polygon[(x + 1) % m], polygon[y], polygon[(y + 1) % m])
                        && t > 1e-9 && t < 1.0 - 1e-9 && u > 1e-9 && u < 1.0 - 1e-9
                    {
                        for tag in [&tags[x], &tags[y]] {
                            let digits: String = tag.trim_start_matches(|ch: char| !ch.is_ascii_digit()).chars().take_while(char::is_ascii_digit).collect();
                            if let Ok(k) = digits.parse::<usize>() {
                                grow.push(k);
                                if let Some(rest) = tag.split('-').nth(1) {
                                    let d2: String = rest.chars().take_while(char::is_ascii_digit).collect();
                                    if let Ok(k2) = d2.parse::<usize>() {
                                        grow.push(k2);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if !grow.is_empty() && signed_area(&polygon) > 0.0 {
                grow.sort_unstable();
                grow.dedup();
                for k in grow {
                    cuts[k] += 2.0;
                }
                continue;
            }
            INVALID.with(|v| *v.borrow_mut() = Some(polygon.clone()));
            return Err(ValidationError::PolygonInvalid);
        }

        // Different lanes of one approach must not cross: swap the source lanes of a crossing pair
        // (restores monotone order where curved geometry broke the angular ordering).
        for _ in 0..8 {
            let mut swapped = false;
            'pairs: for x in 0..connectors.len() {
                for y in x + 1..connectors.len() {
                    let (cx, cy) = (&connectors[x], &connectors[y]);
                    if cx.from != cy.from || cx.kind != cy.kind || (cx.pts[0] - cy.pts[0]).length() < 1e-6 {
                        continue;
                    }
                    if polylines_cross(&cx.pts, &cy.pts) {
                        // Re-source the second connector from the first one's lane: they become
                        // alternatives of one lane (allowed; conflict zone between them).
                        let p0 = cx.pts[0];
                        let p3 = cy.pts[24];
                        let d_in = -apps[cy.from].tangent(cuts[cy.from]);
                        let d_out = apps[cy.to].tangent(cuts[cy.to]);
                        let chord = (p3 - p0).length();
                        let (p1, p2) = (p0 + d_in * 0.4 * chord, p3 - d_out * 0.4 * chord);
                        connectors[y].pts = bezier(p0, p1, p2, p3, 24);
                        connectors[y].max_curvature = bezier_max_curvature(p0, p1, p2, p3);
                        swapped = true;
                        break 'pairs;
                    }
                }
            }
            if !swapped {
                break;
            }
        }

        // Connectors must stay on the junction surface: grow both cut-backs where they do not.
        if n >= 3 {
            let mut grow: Vec<usize> = Vec::new();
            for c in &connectors {
                let outside = c.pts.iter().any(|p| !point_in_polygon(*p, &polygon) && dist_to_polygon(*p, &polygon) > 0.3);
                if outside {
                    grow.push(c.from);
                    grow.push(c.to);
                }
            }
            if !grow.is_empty() {
                grow.sort_unstable();
                grow.dedup();
                for i in grow {
                    cuts[i] += 2.0;
                }
                continue;
            }
        }

        // 8: conflict zones between connectors of different approaches (capsule width 2 m).
        let mut zones = 0;
        let boxes: Vec<(DVec2, DVec2)> = connectors
            .iter()
            .map(|c| c.pts.iter().fold((DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)), |(lo, hi), p| (lo.min(*p - 2.0), hi.max(*p + 2.0))))
            .collect();
        for x in 0..connectors.len() {
            for y in x + 1..connectors.len() {
                let (a, b) = (boxes[x], boxes[y]);
                if a.0.x > b.1.x || b.0.x > a.1.x || a.0.y > b.1.y || b.0.y > a.1.y {
                    continue;
                }
                if connectors[x].from != connectors[y].from && polylines_min_dist(&connectors[x].pts, &connectors[y].pts) < 2.0 {
                    zones += 1;
                }
            }
        }

        // 4: step at cut lines: approach surface (profile + camber) vs. linear interpolation along
        // the polygon's cut-line edges.
        let mut max_step: f64 = 0.0;
        for (i, &(s0, s1)) in cut_vertex_ranges.iter().enumerate() {
            let a = &apps[i];
            let c = cuts[i];
            for v in s0..s1 - 1 {
                for k in 0..=10 {
                    let t = k as f64 / 10.0;
                    let p = polygon[v] * (1.0 - t) + polygon[v + 1] * t;
                    let h_poly = heights[v] * (1.0 - t) + heights[v + 1] * t;
                    let off = (p - a.point(c)).dot(a.normal(c));
                    let h_road = a.height(c, c) - camber * off.abs();
                    max_step = max_step.max((h_poly - h_road).abs());
                }
            }
        }
        let crosswalks = (0..n).filter(|&i| secs[i].sidewalk && n >= 3).count();
        return Ok(Junction { cuts, polygon, heights, connectors, conflict_zones: zones, crosswalks, max_cut_step_m: max_step });
    }
    Err(ValidationError::ConnectorTooSharp)
}

// ---------- checks ----------
#[derive(Default, Debug)]
struct Violations {
    missing_movement: usize,
    same_approach_cross: usize,
    same_lane_alternatives_cross: usize,
    curvature: usize,
    outside_polygon: usize,
    step: usize,
}

fn check(input: &[Approach], j: &Junction) -> Violations {
    let mut v = Violations::default();
    let mut apps = input.to_vec();
    apps.sort_by(|a, b| angle_wrap(a.angle).total_cmp(&angle_wrap(b.angle)));
    let n = apps.len();
    // every allowed movement (car: incoming lanes at i, outgoing lanes at j≠i) has a connector
    for i in 0..n {
        for k in 0..n {
            if i == k {
                continue;
            }
            let si = section(apps[i].kind, apps[i].one_way_incoming);
            let sk = section(apps[k].kind, apps[k].one_way_incoming);
            let has_in = si.lanes.iter().any(|l| l.2 && l.1 == LaneKind::Car);
            let has_out = sk.lanes.iter().any(|l| !l.2 && l.1 == LaneKind::Car);
            let ci = j.cuts[i];
            let ck = j.cuts[k];
            let d_in = -apps[i].tangent(ci);
            let d_out = apps[k].tangent(ck);
            let allowed = d_in.perp_dot(d_out).atan2(d_in.dot(d_out)).to_degrees().abs() <= MAX_TURN_DEG;
            if allowed && has_in && has_out && !j.connectors.iter().any(|c| c.from == i && c.to == k && c.kind == LaneKind::Car) {
                v.missing_movement += 1;
            }
        }
    }
    for x in 0..j.connectors.len() {
        let c = &j.connectors[x];
        let limit = if c.kind == LaneKind::Tram { 1.0 / 20.0 } else { 1.0 / 5.0 };
        if c.max_curvature > limit * 1.001 {
            v.curvature += 1;
        }
        if n >= 3 {
            let over = c.pts.iter().filter(|p| !point_in_polygon(**p, &j.polygon)).map(|p| dist_to_polygon(*p, &j.polygon)).fold(0.0, f64::max);
            if over > 0.3 {
                v.outside_polygon += 1;
                if std::env::var("SJ_DEBUG").is_ok() {
                    let (k, worst) = c.pts.iter().enumerate().map(|(k, p)| (k, if point_in_polygon(*p, &j.polygon) { 0.0 } else { dist_to_polygon(*p, &j.polygon) })).fold((0, 0.0), |a, b| if b.1 > a.1 { b } else { a });
                    let d_in = -apps[c.from].tangent(j.cuts[c.from]);
                    let d_out = apps[c.to].tangent(j.cuts[c.to]);
                    let delta = d_in.perp_dot(d_out).atan2(d_in.dot(d_out)).to_degrees();
                    let gap = angle_wrap(apps[c.to].angle - apps[c.from].angle).to_degrees();
                    eprintln!("outside: over={worst:.2} at k={k}/24 turn={delta:.0} gap_from_to={gap:.0} kinds={:?}->{:?} n={n}", apps[c.from].kind, apps[c.to].kind);
                }
                OVERSHOOT.with(|o| o.borrow_mut().push(over));
            }
        }
        for y in x + 1..j.connectors.len() {
            let d = &j.connectors[y];
            if c.from == d.from && c.kind == d.kind && polylines_cross(&c.pts, &d.pts) {
                if (c.pts[0] - d.pts[0]).length() < 1e-6 {
                    v.same_lane_alternatives_cross += 1;
                    continue;
                }
                v.same_approach_cross += 1;
                if std::env::var("SJ_DEBUG").is_ok() {
                    eprintln!(
                        "cross from={} to={}/{} start=({:.1},{:.1})/({:.1},{:.1}) end=({:.1},{:.1})/({:.1},{:.1}) n={n}",
                        c.from, c.to, d.to, c.pts[0].x, c.pts[0].y, d.pts[0].x, d.pts[0].y,
                        c.pts[24].x, c.pts[24].y, d.pts[24].x, d.pts[24].y
                    );
                }
            }
        }
    }
    if j.max_cut_step_m > 0.02 {
        v.step += 1;
    }
    v
}

// ---------- SVG ----------
fn svg(apps: &[Approach], j: &Junction) -> String {
    let mut s = String::new();
    let scale = 4.0;
    let _ = writeln!(s, "<svg xmlns='http://www.w3.org/2000/svg' viewBox='-300 -300 600 600' style='background:#f4f1ea'>");
    let tr = |p: DVec2| (p.x * scale, -p.y * scale);
    let mut sorted = apps.to_vec();
    sorted.sort_by(|a, b| angle_wrap(a.angle).total_cmp(&angle_wrap(b.angle)));
    for (i, a) in sorted.iter().enumerate() {
        let sec = section(a.kind, a.one_way_incoming);
        for off in [-sec.width / 2.0, sec.width / 2.0] {
            let pts: Vec<String> = (0..=40).map(|k| a.length * k as f64 / 40.0).map(|sv| { let (x, y) = tr(a.point(sv) + a.normal(sv) * off); format!("{x:.1},{y:.1}") }).collect();
            let _ = writeln!(s, "<polyline points='{}' fill='none' stroke='#999' stroke-width='1'/>", pts.join(" "));
        }
        let (x, y) = tr(a.point(j.cuts[i]));
        let _ = writeln!(s, "<circle cx='{x:.1}' cy='{y:.1}' r='3' fill='#c33'/>");
    }
    let poly: Vec<String> = j.polygon.iter().map(|p| { let (x, y) = tr(*p); format!("{x:.1},{y:.1}") }).collect();
    let _ = writeln!(s, "<polygon points='{}' fill='#8885' stroke='#333' stroke-width='1.5'/>", poly.join(" "));
    for c in &j.connectors {
        let pts: Vec<String> = c.pts.iter().map(|p| { let (x, y) = tr(*p); format!("{x:.1},{y:.1}") }).collect();
        let col = if c.kind == LaneKind::Tram { "#c80" } else { "#26a" };
        let _ = writeln!(s, "<polyline points='{}' fill='none' stroke='{col}' stroke-width='1'/>", pts.join(" "));
    }
    s.push_str("</svg>\n");
    s
}

// ---------- cases ----------
fn app(kind: Kind, deg: f64) -> Approach {
    Approach { kind, one_way_incoming: false, angle: deg.to_radians(), curvature: 0.0, length: 60.0, grade: 0.0 }
}

fn canonical() -> Vec<(&'static str, Vec<Approach>)> {
    use Kind::*;
    let mut v = vec![
        ("T", vec![app(Street, 0.0), app(Street, 90.0), app(Street, 180.0)]),
        ("X", vec![app(Street, 0.0), app(Street, 90.0), app(Street, 180.0), app(Street, 270.0)]),
        ("Y-30", vec![app(Street, 0.0), app(Street, 165.0), app(Street, 195.0)]),
        ("5-way", vec![app(Street, 0.0), app(Street, 72.0), app(Street, 144.0), app(Street, 216.0), app(Street, 288.0)]),
        ("avenue x street", vec![app(Avenue, 0.0), app(Street, 90.0), app(Avenue, 180.0), app(Street, 270.0)]),
        ("avenue x avenue", vec![app(Avenue, 0.0), app(Avenue, 90.0), app(Avenue, 180.0), app(Avenue, 270.0)]),
    ];
    let mut ow_in = app(OneWayStreet, 0.0);
    ow_in.one_way_incoming = true;
    let mut ow_out = app(OneWayStreet, 180.0);
    ow_out.one_way_incoming = false;
    v.push(("one-way pair x street", vec![ow_in, app(Street, 90.0), ow_out, app(Street, 270.0)]));
    let mut short = vec![app(Street, 0.0), app(Street, 90.0), app(Street, 180.0), app(Street, 270.0)];
    short[0].length = 30.0;
    v.push(("two junctions 30 m apart", short));
    let mut slope = vec![app(Street, 0.0), app(Street, 90.0), app(Street, 180.0), app(Street, 270.0)];
    slope[0].grade = 0.08;
    slope[2].grade = -0.08;
    slope[1].grade = 0.08;
    v.push(("8 % slope", slope));
    v.push(("tram straight + cross street", vec![app(AvenueTram, 0.0), app(Street, 90.0), app(AvenueTram, 180.0), app(Street, 270.0)]));
    v.push(("tram turn on avenue median", vec![app(AvenueTram, 0.0), app(AvenueTram, 90.0), app(Avenue, 180.0)]));
    // Roundabout ring node: ring in (curved), ring out (curved), approach street.
    let mut ring_in = app(OneWayStreet, 100.0);
    ring_in.one_way_incoming = true;
    ring_in.curvature = -1.0 / 20.0;
    ring_in.length = 40.0;
    let mut ring_out = app(OneWayStreet, 260.0);
    ring_out.curvature = 1.0 / 20.0;
    ring_out.length = 40.0;
    v.push(("roundabout ring node", vec![ring_in, ring_out, app(Street, 0.0)]));
    v
}

/// Minimum segment length the drawing guide shows for a kind: two X-junction cut-backs at ≤ 45 %.
fn guide_length(kind: Kind) -> f64 {
    let s = section(kind, false);
    2.0 * (s.width / 2.0 + s.corner_radius) / 0.9
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    StandardRaw,
    StandardGuided,
    Realistic,
    Full,
}

fn random_case(rng: &mut Pcg32, mode: Mode) -> Vec<Approach> {
    let standard = mode != Mode::Full;
    use Kind::*;
    let degree = if mode == Mode::Realistic {
        let r = rng.next_f64();
        if r < 0.55 { 3 } else if r < 0.90 { 4 } else if r < 0.98 { 5 } else { 6 }
    } else {
        [2, 3, 3, 3, 4, 4, 4, 5, 6][rng.below(9) as usize]
    };
    let min_sep = if standard { 40.0 } else { 25.0 };
    // angles with minimum separation: random gaps normalized to 360°
    let mut gaps: Vec<f64> = (0..degree).map(|_| rng.next_f64() + 0.15).collect();
    let total: f64 = gaps.iter().sum();
    for g in &mut gaps {
        *g = *g / total * 360.0;
    }
    if gaps.iter().any(|&g| g < min_sep) || (degree == 2 && gaps.iter().any(|&g| g > 360.0 - min_sep)) {
        // redistribute: clamp to the minimum and renormalize the rest
        let deficit: f64 = gaps.iter().map(|&g| (min_sep - g).max(0.0)).sum();
        let surplus: f64 = gaps.iter().map(|&g| (g - min_sep).max(0.0)).sum();
        for g in &mut gaps {
            *g = if *g < min_sep { min_sep } else { min_sep + (*g - min_sep) * (1.0 - deficit / surplus.max(1e-9)) };
        }
    }
    let start = rng.next_f64() * 360.0;
    let kinds = [Dirt, Street, Street, Street, OneWayStreet, Avenue, Avenue, OneWayAvenue, AvenueTram];
    let mut angle = start;
    let mut out = Vec::new();
    for g in gaps {
        let kind = if mode == Mode::Realistic {
            let r = rng.next_f64();
            if r < 0.55 { Street } else if r < 0.65 { Dirt } else if r < 0.75 { OneWayStreet } else if r < 0.92 { Avenue } else if r < 0.97 { OneWayAvenue } else { AvenueTram }
        } else {
            kinds[rng.below(kinds.len() as u32) as usize]
        };
        let sec = section(kind, false);
        let r_min = sec.min_radius;
        let curvature_raw = (rng.next_f64() * 2.0 - 1.0) / r_min;
        let length = match mode {
            Mode::StandardRaw => 30.0 + rng.next_f64() * 30.0,
            Mode::StandardGuided | Mode::Realistic => guide_length(kind).max(30.0) * (1.0 + rng.next_f64() * 0.6),
            Mode::Full => 6.0 + rng.next_f64() * 54.0,
        };
        let grade = (rng.next_f64() * 2.0 - 1.0) * sec.max_grade;
        // Standard: approaches turn at most 60° over their length (a gentle curve near a junction).
        let max_turn = if mode == Mode::Realistic { 30f64.to_radians() } else { 60f64.to_radians() };
        let curvature = if standard { curvature_raw.clamp(-max_turn / length, max_turn / length) } else { curvature_raw };
        let curvature = if std::env::var("SJ_STRAIGHT").is_ok() { 0.0 } else { curvature };
        out.push(Approach { kind, one_way_incoming: rng.chance(0.5), angle: angle.to_radians(), curvature, length, grade });
        angle += g;
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let count: usize = args.iter().position(|a| a == "--random").and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(10_000);
    let svg_dir = args.iter().position(|a| a == "--svg").and_then(|i| args.get(i + 1)).cloned();
    if let Some(d) = &svg_dir {
        let _ = std::fs::create_dir_all(d);
    }

    println!("== canonical");
    for (name, case) in canonical() {
        match build(&case) {
            Ok(j) => {
                let v = check(&case, &j);
                println!(
                    "case={name:32} OK cuts={:?} connectors={} zones={} crosswalks={} step={:.4} violations={v:?}",
                    j.cuts.iter().map(|c| (c * 10.0).round() / 10.0).collect::<Vec<_>>(),
                    j.connectors.len(),
                    j.conflict_zones,
                    j.crosswalks,
                    j.max_cut_step_m
                );
                if let Some(d) = &svg_dir {
                    let _ = std::fs::write(format!("{d}/canonical-{}.svg", name.replace([' ', '%'], "_")), svg(&case, &j));
                }
            }
            Err(e) => println!("case={name:32} ERR {e:?}"),
        }
    }

    for (name, case) in canonical() {
        DIAG.with(|d| d.borrow_mut().clear());
        if build(&case).is_err() {
            let d = DIAG.with(|d| d.borrow().clone());
            println!("diag {name}: first={:?} last={:?} n={}", d.first(), d.last(), d.len());
        }
    }
    for k in [Kind::Dirt, Kind::Street, Kind::OneWayStreet, Kind::Avenue, Kind::OneWayAvenue, Kind::AvenueTram] {
        println!("guide_length {k:?} = {:.1} m", guide_length(k));
    }
    for (label, mode) in [("standard_raw", Mode::StandardRaw), ("standard_guided", Mode::StandardGuided), ("realistic_guided", Mode::Realistic), ("full", Mode::Full)] {
        DIAG.with(|d| d.borrow_mut().clear());
        let mut rng = Pcg32::from_seed(match mode { Mode::StandardRaw => 0x5_1, Mode::StandardGuided => 0x5_3, Mode::Realistic => 0x5_4, Mode::Full => 0x5_2 });
        let mut times = Vec::with_capacity(count);
        let mut errors: std::collections::BTreeMap<ValidationError, usize> = std::collections::BTreeMap::new();
        let mut ok = 0usize;
        let mut viol = Violations::default();
        let mut dumped = 0;
        let mut required: Vec<f64> = Vec::new();
        for idx in 0..count {
            let mut case = random_case(&mut rng, mode);
            if mode == Mode::StandardGuided || mode == Mode::Realistic {
                // The drawing tool shows the live minimum length (computed by this same builder);
                // a player following it lengthens segments until the junction fits (≤ 6 steps).
                for _ in 0..6 {
                    if !matches!(build(&case), Err(ValidationError::TooClose)) {
                        break;
                    }
                    for a in &mut case {
                        a.length *= 1.25;
                        // keep the drawn curve's total turn when lengthening
                        a.curvature /= 1.25;
                    }
                }
                for a in &case {
                    required.push(a.length);
                }
            }
            let t = Instant::now();
            let r = std::panic::catch_unwind(|| build(&case));
            times.push(t.elapsed().as_secs_f64() * 1e3);
            match r {
                Err(_) => *errors.entry(ValidationError::PolygonInvalid).or_default() += 1_000_000, // panic marker
                Ok(Err(e)) => {
                    *errors.entry(e).or_default() += 1;
                    if e == ValidationError::PolygonInvalid && dumped < 12 && let Some(d) = &svg_dir {
                        let poly = INVALID.with(|v| v.borrow().clone()).unwrap_or_default();
                        let fake = Junction { cuts: vec![0.0; case.len()], polygon: poly, heights: vec![], connectors: vec![], conflict_zones: 0, crosswalks: 0, max_cut_step_m: 0.0 };
                        let _ = std::fs::write(format!("{d}/{label}-invalid-{idx}.svg"), svg(&case, &fake));
                        dumped += 1;
                    }
                }
                Ok(Ok(j)) => {
                    ok += 1;
                    let v = check(&case, &j);
                    let bad = v.missing_movement + v.same_approach_cross + v.curvature + v.outside_polygon + v.step > 0;
                    viol.missing_movement += v.missing_movement;
                    viol.same_approach_cross += v.same_approach_cross;
                    viol.same_lane_alternatives_cross += v.same_lane_alternatives_cross;
                    viol.curvature += v.curvature;
                    viol.outside_polygon += v.outside_polygon;
                    viol.step += v.step;
                    if bad && dumped < 12 {
                        if let Some(d) = &svg_dir {
                            let _ = std::fs::write(format!("{d}/{label}-violation-{idx}.svg"), svg(&case, &j));
                        }
                        dumped += 1;
                    }
                }
            }
        }
        let d = DIAG.with(|d| d.borrow().clone());
        let attempt0 = d.iter().filter(|m| m.starts_with("too_close attempt=0 ")).count();
        let mut turns = std::collections::BTreeMap::new();
        for m in d.iter().filter(|m| m.starts_with("sharp")) {
            let t: i64 = m.split("turn=").nth(1).and_then(|x| x.split(' ').next()).and_then(|x| x.parse().ok()).unwrap_or(0);
            *turns.entry((t.abs() / 30) * 30).or_insert(0usize) += 1;
        }
        println!("diag {label}: too_close at attempt 0 = {attempt0}; sharp connectors by |turn| bucket = {turns:?}");
        let mut over = OVERSHOOT.with(|o| std::mem::take(&mut *o.borrow_mut()));
        if !over.is_empty() {
            over.sort_by(f64::total_cmp);
            let q = |x: f64| over[((over.len() - 1) as f64 * x) as usize];
            println!("metric=s_j_{label}_outside_overshoot_m p50={:.2} p95={:.2} max={:.2}", q(0.5), q(0.95), q(1.0));
        }
        if !required.is_empty() {
            required.sort_by(f64::total_cmp);
            let q = |x: f64| required[((required.len() - 1) as f64 * x) as usize];
            println!("metric=s_j_guided_segment_length p50={:.1} p90={:.1} p99={:.1} m", q(0.5), q(0.9), q(0.99));
        }
        times.sort_by(f64::total_cmp);
        let p = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
        let rejected: usize = errors.values().sum();
        println!(
            "metric=s_j_{label} cases={count} ok={ok} rejected={rejected} ({:.2} %) errors={errors:?} violations={viol:?} ms p50={:.3} p99={:.3} max={:.3}",
            rejected as f64 / count as f64 * 100.0,
            p(0.5),
            p(0.99),
            p(1.0)
        );
    }
}
