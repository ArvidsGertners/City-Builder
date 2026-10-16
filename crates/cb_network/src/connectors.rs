//! Junction connectors and conflict zones (ADR-0063, ALGORITHMS §5 steps 6–8): lane-to-lane
//! cubic Béziers through the junction with per-kind curvature limits, monotone lane assignment
//! (right targets from right lanes), re-sourcing of crossing connectors from different lanes of
//! one approach, and conflict zones where connectors of different approaches (or crossing
//! alternatives of one lane) come within 2 m.

use glam::{Vec2, Vec3};

use crate::cross_section::{CrossSection, Dir, LaneKind};

/// Movements deflecting more than this get no connector (deg).
pub const MAX_TURN_DEG: f32 = 135.0;
/// Bézier handle length as a fraction of the chord.
pub const HANDLE_FRACTION: f32 = 0.4;
/// Samples per connector (segments).
pub const CONNECTOR_SAMPLES: usize = 24;
/// Capsule width for conflicts (m).
pub const CONFLICT_WIDTH_M: f32 = 2.0;
/// Conflict zones closer than this merge (m).
pub const ZONE_MERGE_M: f32 = 2.0;
/// Deflection below which a movement counts as straight (deg).
pub const STRAIGHT_DEG: f32 = 30.0;

/// A driving lane as seen from a junction node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ApproachLane {
    /// Offset left of the approach's leaving direction (m).
    pub offset_m: f32,
    /// Traffic on this lane drives toward the node.
    pub incoming: bool,
    /// Index from the right in its travel direction.
    pub index: u8,
    /// Users.
    pub kind: LaneKind,
    /// Lane width (m).
    pub width_m: f32,
}

/// Lanes of a segment seen from one of its nodes (`at_start`: the node is the curve start).
#[must_use]
pub fn approach_lanes(cs: &CrossSection, at_start: bool) -> Vec<ApproachLane> {
    cs.lanes()
        .into_iter()
        .map(|l| {
            // At the start node the leaving direction is the curve direction: forward lanes
            // leave, backward lanes arrive. At the end node everything flips.
            let leaving = (l.dir == Dir::Forward) == at_start;
            ApproachLane {
                offset_m: if at_start { l.offset_m } else { -l.offset_m },
                incoming: !leaving,
                index: l.index,
                kind: l.kind,
                width_m: l.width_m,
            }
        })
        .collect()
}

/// What a connector carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ConnectorKind {
    /// Road vehicles (cars, buses).
    Road,
    /// Trams.
    Tram,
}

impl ConnectorKind {
    fn accepts(self, k: LaneKind) -> bool {
        match self {
            Self::Road => !matches!(k, LaneKind::Tram),
            Self::Tram => k.trams(),
        }
    }
}

/// Movement classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Turn {
    /// |deflection| < 30°.
    Straight,
    /// Turning left.
    Left,
    /// Turning right.
    Right,
}

/// A lane-to-lane path through a junction.
#[derive(Clone, Debug, PartialEq)]
pub struct Connector {
    /// Input index of the incoming approach.
    pub from: usize,
    /// Incoming lane index (from the right).
    pub from_lane: u8,
    /// Input index of the outgoing approach.
    pub to: usize,
    /// Outgoing lane index (from the right).
    pub to_lane: u8,
    /// Users.
    pub kind: ConnectorKind,
    /// Path (x, height, z), `CONNECTOR_SAMPLES + 1` points.
    pub points: Vec<Vec3>,
    /// Plan length (m).
    pub length_m: f32,
    /// Largest curvature (1/m).
    pub max_curvature: f32,
    /// Signed deflection (deg, left positive).
    pub deflection_deg: f32,
    /// Movement class.
    pub turn: Turn,
}

/// Where connectors conflict.
#[derive(Clone, Debug, PartialEq)]
pub struct ConflictZone {
    /// Centre.
    pub center: Vec3,
    /// Member connectors with the arc length on each where the zone lies.
    pub members: Vec<(usize, f32)>,
}

/// One approach's frame at its cut line (input order).
#[derive(Clone, Copy, Debug)]
pub(crate) struct CutFrame {
    pub point: Vec2,
    /// Leaving direction (unit).
    pub tangent: Vec2,
    pub height: f32,
}

impl CutFrame {
    fn lane_point(&self, off: f32) -> Vec2 {
        self.point + self.tangent.perp() * off
    }
}

fn bezier(p: [Vec2; 4], t: f32) -> Vec2 {
    let u = 1.0 - t;
    p[0] * (u * u * u) + p[1] * (3.0 * u * u * t) + p[2] * (3.0 * u * t * t) + p[3] * (t * t * t)
}

fn bezier_d1(p: [Vec2; 4], t: f32) -> Vec2 {
    let u = 1.0 - t;
    (p[1] - p[0]) * (3.0 * u * u) + (p[2] - p[1]) * (6.0 * u * t) + (p[3] - p[2]) * (3.0 * t * t)
}

fn bezier_d2(p: [Vec2; 4], t: f32) -> Vec2 {
    (p[2] - p[1] * 2.0 + p[0]) * (6.0 * (1.0 - t)) + (p[3] - p[2] * 2.0 + p[1]) * (6.0 * t)
}

fn max_curvature(p: [Vec2; 4]) -> f32 {
    (0..=48)
        .map(|k| {
            let t = k as f32 / 48.0;
            let (d1, d2) = (bezier_d1(p, t), bezier_d2(p, t));
            let l = d1.length();
            if l < 1e-6 {
                f32::INFINITY
            } else {
                d1.perp_dot(d2).abs() / (l * l * l)
            }
        })
        .fold(0.0, f32::max)
}

fn connector_path(
    p0: Vec2,
    d_in: Vec2,
    h0: f32,
    p3: Vec2,
    d_out: Vec2,
    h3: f32,
) -> (Vec<Vec3>, f32, f32) {
    let chord = p0.distance(p3);
    let ctrl = [
        p0,
        p0 + d_in * (HANDLE_FRACTION * chord),
        p3 - d_out * (HANDLE_FRACTION * chord),
        p3,
    ];
    let points: Vec<Vec3> = (0..=CONNECTOR_SAMPLES)
        .map(|k| {
            let t = k as f32 / CONNECTOR_SAMPLES as f32;
            let q = bezier(ctrl, t);
            let w = t * t * (3.0 - 2.0 * t);
            Vec3::new(q.x, h0 + (h3 - h0) * w, q.y)
        })
        .collect();
    let len = points
        .windows(2)
        .map(|w| Vec2::new(w[1].x - w[0].x, w[1].z - w[0].z).length())
        .sum();
    (points, len, max_curvature(ctrl))
}

fn deflection_deg(d_in: Vec2, d_out: Vec2) -> f32 {
    cb_core::math::to_degrees(cb_core::math::atan2(d_in.perp_dot(d_out), d_in.dot(d_out)))
}

/// Curvature limit of a connector from a lane of kind `src` (1/m).
fn curvature_limit(kind: ConnectorKind, src: LaneKind) -> f32 {
    match kind {
        ConnectorKind::Tram => 1.0 / LaneKind::Tram.min_connector_radius_m(),
        ConnectorKind::Road if src == LaneKind::Bus => 1.0 / LaneKind::Bus.min_connector_radius_m(),
        ConnectorKind::Road => 1.0 / LaneKind::Car.min_connector_radius_m(),
    }
}

/// Builds all connectors for the cut frames; `Err(approaches)` lists input indices whose
/// cut-backs must grow because a connector exceeds its curvature limit.
pub(crate) fn build_connectors(
    frames: &[CutFrame],
    lanes: &[Vec<ApproachLane>],
) -> Result<Vec<Connector>, Vec<usize>> {
    let n = frames.len();
    let mut out = Vec::new();
    let mut sharp: Vec<usize> = Vec::new();
    for i in 0..n {
        let d_in = -frames[i].tangent;
        // Targets ordered from sharpest right to sharpest left.
        let mut targets: Vec<(f32, usize)> = (0..n)
            .filter(|&j| j != i)
            .map(|j| (deflection_deg(d_in, frames[j].tangent), j))
            .filter(|(d, _)| d.abs() <= MAX_TURN_DEG)
            .collect();
        targets.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for kind in [ConnectorKind::Road, ConnectorKind::Tram] {
            let mut src: Vec<ApproachLane> = lanes[i]
                .iter()
                .copied()
                .filter(|l| l.incoming && kind.accepts(l.kind))
                .collect();
            src.sort_by_key(|l| l.index);
            let tg: Vec<(f32, usize)> = targets
                .iter()
                .copied()
                .filter(|&(_, j)| lanes[j].iter().any(|l| !l.incoming && kind.accepts(l.kind)))
                .collect();
            if src.is_empty() || tg.is_empty() {
                continue;
            }
            let tn = tg.len();
            for (rank, &(delta, j)) in tg.iter().enumerate() {
                // Monotone source choice: right targets from right lanes, left from left.
                let si = if tn == 1 {
                    // Single movement (e.g. a bend): every lane continues.
                    usize::MAX
                } else {
                    (rank * (src.len() - 1) + (tn - 1) / 2) / (tn - 1)
                };
                let mut dst: Vec<ApproachLane> = lanes[j]
                    .iter()
                    .copied()
                    .filter(|l| !l.incoming && kind.accepts(l.kind))
                    .collect();
                dst.sort_by_key(|l| l.index);
                let pairs: Vec<(ApproachLane, ApproachLane)> = if si == usize::MAX {
                    // Lane k continues into lane min(k, last).
                    src.iter()
                        .filter(|s| (s.index as usize) <= dst.len())
                        .map(|s| (*s, dst[(s.index as usize).min(dst.len() - 1)]))
                        .collect()
                } else {
                    let di = if rank == 0 {
                        0
                    } else if rank == tn - 1 {
                        dst.len() - 1
                    } else {
                        si.min(dst.len() - 1)
                    };
                    // Straight movements use every source lane (lane k → k, merging at most 2).
                    if delta.abs() < STRAIGHT_DEG && src.len() > 1 {
                        // ≤ 2 source lanes merge into the leftmost target lane.
                        src.iter()
                            .filter(|s| (s.index as usize) <= dst.len())
                            .map(|s| (*s, dst[(s.index as usize).min(dst.len() - 1)]))
                            .collect()
                    } else {
                        vec![(src[si], dst[di])]
                    }
                };
                for (s, d) in pairs {
                    let (fi, fj) = (&frames[i], &frames[j]);
                    let (p0, p3) = (fi.lane_point(s.offset_m), fj.lane_point(d.offset_m));
                    let (points, length_m, kmax) =
                        connector_path(p0, d_in, fi.height, p3, fj.tangent, fj.height);
                    if kmax > curvature_limit(kind, s.kind) * 1.001 {
                        sharp.push(i);
                        sharp.push(j);
                    }
                    let turn = if delta.abs() < STRAIGHT_DEG {
                        Turn::Straight
                    } else if delta > 0.0 {
                        Turn::Left
                    } else {
                        Turn::Right
                    };
                    out.push(Connector {
                        from: i,
                        from_lane: s.index,
                        to: j,
                        to_lane: d.index,
                        kind,
                        points,
                        length_m,
                        max_curvature: kmax,
                        deflection_deg: delta,
                        turn,
                    });
                }
            }
        }
    }
    if sharp.is_empty() {
        Ok(out)
    } else {
        sharp.sort_unstable();
        sharp.dedup();
        Err(sharp)
    }
}

fn plan(p: &[Vec3]) -> Vec<Vec2> {
    p.iter().map(|v| Vec2::new(v.x, v.z)).collect()
}

fn seg_intersect(p: Vec2, p2: Vec2, q: Vec2, q2: Vec2) -> bool {
    let (r, s) = (p2 - p, q2 - q);
    let den = r.perp_dot(s);
    if den.abs() < 1e-9 {
        return false;
    }
    let t = (q - p).perp_dot(s) / den;
    let u = (q - p).perp_dot(r) / den;
    t > 1e-4 && t < 1.0 - 1e-4 && u > 1e-4 && u < 1.0 - 1e-4
}

/// Whether two polylines cross (proper intersections only).
#[must_use]
pub fn polylines_cross(a: &[Vec2], b: &[Vec2]) -> bool {
    a.windows(2)
        .any(|x| b.windows(2).any(|y| seg_intersect(x[0], x[1], y[0], y[1])))
}

/// Re-sources connectors from different lanes of one approach that cross: the second becomes an
/// alternative of the first one's lane (ADR-0063). `Err(approaches)` when crossings remain or a
/// re-sourced connector exceeds its curvature limit: those cut-backs must grow.
pub(crate) fn resolve_crossings(
    conns: &mut [Connector],
    frames: &[CutFrame],
    lanes: &[Vec<ApproachLane>],
) -> Result<(), Vec<usize>> {
    // Each pass re-sources one connector; a lane change can expose another crossing, so allow
    // a few passes per connector before giving up.
    let mut grow: Vec<usize> = Vec::new();
    for _ in 0..=2 * conns.len() {
        let mut changed = false;
        'pairs: for x in 0..conns.len() {
            for y in x + 1..conns.len() {
                let (cx, cy) = (&conns[x], &conns[y]);
                if cx.from != cy.from || cx.kind != cy.kind || cx.from_lane == cy.from_lane {
                    continue;
                }
                if polylines_cross(&plan(&cx.points), &plan(&cy.points)) {
                    let (i, j) = (cy.from, cy.to);
                    let lane = cx.from_lane;
                    let off = lanes[i]
                        .iter()
                        .find(|l| l.incoming && l.index == lane && cx.kind.accepts(l.kind))
                        .map_or(0.0, |l| l.offset_m);
                    let p3 = Vec2::new(
                        cy.points[CONNECTOR_SAMPLES].x,
                        cy.points[CONNECTOR_SAMPLES].z,
                    );
                    let (fi, fj) = (&frames[i], &frames[j]);
                    let (points, length_m, kmax) = connector_path(
                        fi.lane_point(off),
                        -fi.tangent,
                        fi.height,
                        p3,
                        fj.tangent,
                        fj.height,
                    );
                    let src_kind = lanes[i]
                        .iter()
                        .find(|l| l.incoming && l.index == lane && cx.kind.accepts(l.kind))
                        .map_or(LaneKind::Car, |l| l.kind);
                    if kmax > curvature_limit(cx.kind, src_kind) * 1.001 {
                        grow.push(i);
                        grow.push(j);
                    }
                    let c = &mut conns[y];
                    c.from_lane = lane;
                    c.points = points;
                    c.length_m = length_m;
                    c.max_curvature = kmax;
                    changed = true;
                    break 'pairs;
                }
            }
        }
        if !changed {
            break;
        }
    }
    for x in 0..conns.len() {
        for y in x + 1..conns.len() {
            let (cx, cy) = (&conns[x], &conns[y]);
            if cx.from == cy.from
                && cx.kind == cy.kind
                && cx.from_lane != cy.from_lane
                && polylines_cross(&plan(&cx.points), &plan(&cy.points))
            {
                grow.extend([cx.from, cx.to, cy.to]);
            }
        }
    }
    if grow.is_empty() {
        Ok(())
    } else {
        grow.sort_unstable();
        grow.dedup();
        Err(grow)
    }
}

/// Closest approach of two polylines: (distance, s on a, s on b, midpoint).
fn closest(a: &[Vec3], b: &[Vec3]) -> (f32, f32, f32, Vec3) {
    let mut best = (f32::INFINITY, 0.0, 0.0, Vec3::ZERO);
    let (mut sa, mut sb0) = (0.0f32, 0.0f32);
    let cum_b: Vec<f32> = std::iter::once(0.0)
        .chain(b.windows(2).scan(0.0, |acc, w| {
            *acc += Vec2::new(w[1].x - w[0].x, w[1].z - w[0].z).length();
            Some(*acc)
        }))
        .collect();
    for wa in a.windows(2) {
        let la = Vec2::new(wa[1].x - wa[0].x, wa[1].z - wa[0].z).length();
        for (k, wb) in b.windows(2).enumerate() {
            // Sample both segments at their ends and midpoints (connectors are dense).
            for ta in [0.0, 0.5, 1.0] {
                let pa = wa[0].lerp(wa[1], ta);
                let (pa2, qb0, qb1) = (
                    Vec2::new(pa.x, pa.z),
                    Vec2::new(wb[0].x, wb[0].z),
                    Vec2::new(wb[1].x, wb[1].z),
                );
                let ab = qb1 - qb0;
                let tb = ((pa2 - qb0).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
                let pb = wb[0].lerp(wb[1], tb);
                let d = pa2.distance(Vec2::new(pb.x, pb.z));
                if d < best.0 {
                    sb0 = cum_b[k] + tb * ab.length();
                    best = (d, sa + ta * la, sb0, pa.lerp(pb, 0.5));
                }
            }
        }
        sa += la;
    }
    let _ = sb0;
    best
}

/// Conflict zones between connectors of different approaches and crossing alternatives of one
/// lane; zones closer than [`ZONE_MERGE_M`] merge.
#[must_use]
pub fn conflict_zones(conns: &[Connector]) -> Vec<ConflictZone> {
    let boxes: Vec<(Vec2, Vec2)> = conns
        .iter()
        .map(|c| {
            c.points.iter().fold(
                (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
                |(lo, hi), p| {
                    let q = Vec2::new(p.x, p.z);
                    (lo.min(q - CONFLICT_WIDTH_M), hi.max(q + CONFLICT_WIDTH_M))
                },
            )
        })
        .collect();
    let mut zones: Vec<ConflictZone> = Vec::new();
    for x in 0..conns.len() {
        for y in x + 1..conns.len() {
            let (a, b) = (&conns[x], &conns[y]);
            let (ba, bb) = (boxes[x], boxes[y]);
            if ba.0.x > bb.1.x || bb.0.x > ba.1.x || ba.0.y > bb.1.y || bb.0.y > ba.1.y {
                continue;
            }
            let same_lane = a.from == b.from && a.from_lane == b.from_lane && a.kind == b.kind;
            let relevant = if a.from == b.from {
                same_lane && polylines_cross(&plan(&a.points), &plan(&b.points))
            } else {
                true
            };
            if !relevant {
                continue;
            }
            let (d, sa, sb, mid) = closest(&a.points, &b.points);
            if d >= CONFLICT_WIDTH_M {
                continue;
            }
            match zones
                .iter_mut()
                .find(|z| Vec2::new(z.center.x - mid.x, z.center.z - mid.z).length() < ZONE_MERGE_M)
            {
                Some(z) => {
                    for (c, s) in [(x, sa), (y, sb)] {
                        if !z.members.iter().any(|m| m.0 == c) {
                            z.members.push((c, s));
                        }
                    }
                }
                None => zones.push(ConflictZone {
                    center: mid,
                    members: vec![(x, sa), (y, sb)],
                }),
            }
        }
    }
    zones
}
