//! Junction markings (ALGORITHMS §5 step 5): crosswalks 1 m behind each cut line on approaches
//! with sidewalks, a line 2 m upstream of the crosswalk across the arriving lanes (solid for stop
//! and signal control, dashed with shark teeth for yield, none for priority), and lane arrows
//! showing each arriving lane's movements. None on bends and continuations (fewer than three
//! approaches). Roundabout nodes: nothing on the ring itself; entries get a give-way line at the
//! ring edge, a crosswalk set back and a raised splitter island with a pedestrian refuge.

use glam::{Vec2, Vec3};

use crate::connectors::{ConnectorKind, Turn};
use crate::junction::{Approach, Junction};

/// Gap between the cut line and the crosswalk (m).
pub const CROSSWALK_OFFSET_M: f32 = 1.0;
/// Crosswalk band width along the road (m).
pub const CROSSWALK_WIDTH_M: f32 = 3.0;
/// Gap between the crosswalk and the stop line (m).
pub const STOP_GAP_M: f32 = 2.0;
/// Stop line width along the road (m).
pub const STOP_LINE_WIDTH_M: f32 = 0.4;
/// Distance of the lane arrow centre upstream of the stop line (m).
pub const ARROW_BEHIND_STOP_M: f32 = 6.0;
/// Arrow length (m).
pub const ARROW_LENGTH_M: f32 = 4.0;
/// Signal poles stand this far behind the kerb line (m).
pub const KERB_INSET_M: f32 = 0.6;
/// Approaches need this much road beyond the arrow to carry markings (m).
const MIN_TAIL_M: f32 = 4.0;
/// Roundabout entries: give-way line this far behind the cut line (m).
pub const ENTRY_LINE_OFFSET_M: f32 = 0.5;
/// Roundabout entries: crosswalk start behind the cut line (m), one car length clear of the ring.
pub const ENTRY_CROSSWALK_OFFSET_M: f32 = 5.0;
/// Splitter island length from its nose (m), maximum half width (m) and kerb height (m).
pub const ISLAND_LENGTH_M: f32 = 16.0;
/// Splitter island maximum half width (m).
pub const ISLAND_HALF_WIDTH_M: f32 = 0.6;
/// Splitter island kerb height (m).
pub const ISLAND_HEIGHT_M: f32 = 0.15;
/// Gap between the island pieces and the crosswalk refuge edges (m).
const REFUGE_GAP_M: f32 = 0.3;

/// How an approach's arrival is marked, from its control class and ring membership.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ApproachMarks {
    /// Solid stop line (stop sign or traffic lights).
    #[default]
    Stop,
    /// Dashed give-way line with shark teeth.
    GiveWay,
    /// Priority approach: no line.
    Free,
    /// Part of a roundabout ring: no markings.
    Ring,
    /// Roundabout entry: give-way line at the ring, set-back crosswalk, splitter island.
    RingEntry,
}

impl ApproachMarks {
    /// Marks for a control class (`lights`: the junction is signalised).
    #[must_use]
    pub fn for_control(c: crate::control::ApproachControl, lights: bool) -> Self {
        use crate::control::ApproachControl as C;
        match (lights, c) {
            (true, _) | (false, C::Stop) => Self::Stop,
            (false, C::Yield) => Self::GiveWay,
            (false, C::Priority) => Self::Free,
        }
    }
}

/// Style of the line across the arriving lanes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    /// Solid stop line.
    Solid,
    /// Dashed give-way line with shark teeth upstream.
    GiveWay,
}

/// A crosswalk band across the carriageway.
#[derive(Clone, Debug, PartialEq)]
pub struct Crosswalk {
    /// Approach index (junction input order).
    pub approach: usize,
    /// Band centre at both kerbs.
    pub ends: [Vec3; 2],
    /// Direction along the road, away from the node.
    pub along: Vec2,
    /// Band width along the road (m).
    pub width_m: f32,
}

/// A stop line across the arriving lanes of an approach.
#[derive(Clone, Debug, PartialEq)]
pub struct StopLine {
    /// Approach index.
    pub approach: usize,
    /// Line ends.
    pub ends: [Vec3; 2],
    /// Direction along the road, away from the node.
    pub along: Vec2,
    /// On the sidewalk beside the line's kerb end (signal pole position).
    pub kerb: Vec3,
    /// Line style.
    pub kind: LineKind,
}

/// A raised splitter island between the arriving and leaving lanes of a roundabout entry:
/// matching edge points along the island (nose first), at road surface height.
#[derive(Clone, Debug, PartialEq)]
pub struct Island {
    /// Approach index.
    pub approach: usize,
    /// Edge on the arriving-lane side.
    pub left: Vec<Vec3>,
    /// Edge on the leaving-lane side.
    pub right: Vec<Vec3>,
}

/// Movements painted on one arriving lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ArrowTurns {
    /// Left turn.
    pub left: bool,
    /// Straight on.
    pub straight: bool,
    /// Right turn.
    pub right: bool,
}

/// A lane arrow.
#[derive(Clone, Debug, PartialEq)]
pub struct LaneArrow {
    /// Approach index.
    pub approach: usize,
    /// Lane index from the right.
    pub lane: u8,
    /// Arrow centre on the lane.
    pub center: Vec3,
    /// Travel direction (toward the node).
    pub dir: Vec2,
    /// Painted movements.
    pub turns: ArrowTurns,
}

/// All markings of a junction.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Markings {
    /// Crosswalks.
    pub crosswalks: Vec<Crosswalk>,
    /// Stop lines.
    pub stop_lines: Vec<StopLine>,
    /// Lane arrows.
    pub arrows: Vec<LaneArrow>,
    /// Splitter islands.
    pub islands: Vec<Island>,
}

fn at(a: &Approach, s: f32, offset: f32) -> Vec3 {
    let p = a.centerline.point_at(s) + a.centerline.tangent_at(s).perp() * offset;
    Vec3::new(p.x, a.height(s), p.y)
}

/// Half width of a splitter island `d` m behind its nose of length `l`: a rounded nose over the
/// first 1.5 m, then a straight taper to a 0.1 m tip.
fn island_half_width(d: f32, l: f32) -> f32 {
    let nose = 1.5f32.min(0.5 * l);
    if d < nose {
        let x = (nose - d) / nose;
        ISLAND_HALF_WIDTH_M * (1.0 - x * x).max(0.0).sqrt().max(0.25)
    } else {
        let t = (d - nose) / (l - nose).max(1e-3);
        ISLAND_HALF_WIDTH_M + (0.1 - ISLAND_HALF_WIDTH_M) * t
    }
}

/// Splitter island pieces along `s0..s1`, split by the crosswalk `refuge` (the gap lets
/// pedestrians cross at road level). The island is centred on `edge`, the boundary between the
/// arriving and leaving lanes; at 0.6 m half width it stays clear of vehicles at lane centres.
#[allow(clippy::too_many_arguments)]
fn island(
    m: &mut Markings,
    a: &Approach,
    i: usize,
    edge: f32,
    side: f32,
    s0: f32,
    s1: f32,
    refuge: Option<(f32, f32)>,
) {
    let l = s1 - s0;
    if l < 3.0 {
        return;
    }
    let centre = edge;
    let mut pieces: smallvec::SmallVec<[(f32, f32); 2]> = smallvec::SmallVec::new();
    match refuge {
        Some((r0, r1)) if r0 > s0 && r1 < s1 => {
            pieces.push((s0, r0 - REFUGE_GAP_M));
            pieces.push((r1 + REFUGE_GAP_M, s1));
        }
        _ => pieces.push((s0, s1)),
    }
    for (p0, p1) in pieces {
        if p1 - p0 < 0.5 {
            continue;
        }
        let n = ((p1 - p0) / 1.0).ceil().max(2.0) as usize;
        let (mut left, mut right) = (Vec::with_capacity(n + 1), Vec::with_capacity(n + 1));
        for k in 0..=n {
            let s = p0 + (p1 - p0) * k as f32 / n as f32;
            let hw = island_half_width(s - s0, l);
            left.push(at(a, s, centre + side * hw));
            right.push(at(a, s, centre - side * hw));
        }
        m.islands.push(Island {
            approach: i,
            left,
            right,
        });
    }
}

/// Markings for a junction built from `apps`; `marks[i]` styles approach `i` (missing entries
/// are `Stop`).
#[must_use]
pub fn markings(apps: &[Approach], j: &Junction, marks: &[ApproachMarks]) -> Markings {
    let mut m = Markings::default();
    if apps.len() < 3 {
        return m;
    }
    for (i, a) in apps.iter().enumerate() {
        let Some(&cut) = j.cuts.get(i) else { continue };
        let mk = marks.get(i).copied().unwrap_or_default();
        if mk == ApproachMarks::Ring {
            continue;
        }
        let entry = mk == ApproachMarks::RingEntry;
        let len = a.centerline.length();
        let along = a.centerline.tangent_at(cut);
        let ch = a.carriageway_half_m;
        let sidewalks = a.half_width_m - ch > 1.0;
        let mut s_stop = cut + CROSSWALK_OFFSET_M;
        let cw_start = if entry {
            cut + ENTRY_CROSSWALK_OFFSET_M
        } else {
            cut + CROSSWALK_OFFSET_M
        };
        let mut refuge = None;
        if sidewalks {
            let sc = cw_start + 0.5 * CROSSWALK_WIDTH_M;
            if sc + 0.5 * CROSSWALK_WIDTH_M < len {
                m.crosswalks.push(Crosswalk {
                    approach: i,
                    ends: [at(a, sc, -ch), at(a, sc, ch)],
                    along,
                    width_m: CROSSWALK_WIDTH_M,
                });
                s_stop = sc + 0.5 * CROSSWALK_WIDTH_M + STOP_GAP_M;
                refuge = Some((cw_start, cw_start + CROSSWALK_WIDTH_M));
            }
        }
        if entry {
            s_stop = cut + ENTRY_LINE_OFFSET_M;
        }
        let incoming: Vec<_> = a
            .lanes
            .iter()
            .filter(|l| l.incoming && l.kind != crate::LaneKind::Tram)
            .collect();
        if incoming.is_empty() || s_stop + STOP_LINE_WIDTH_M > len {
            continue;
        }
        let lo = incoming
            .iter()
            .map(|l| l.offset_m - 0.5 * l.width_m)
            .fold(f32::INFINITY, f32::min);
        let hi = incoming
            .iter()
            .map(|l| l.offset_m + 0.5 * l.width_m)
            .fold(f32::NEG_INFINITY, f32::max);
        let s_line = s_stop + 0.5 * STOP_LINE_WIDTH_M;
        let side = if lo + hi >= 0.0 { 1.0 } else { -1.0 };
        if mk != ApproachMarks::Free {
            m.stop_lines.push(StopLine {
                approach: i,
                ends: [at(a, s_line, lo), at(a, s_line, hi)],
                along,
                kerb: at(a, s_line, side * (ch + KERB_INSET_M)),
                kind: if mk == ApproachMarks::Stop {
                    LineKind::Solid
                } else {
                    LineKind::GiveWay
                },
            });
        }
        if entry {
            // The island sits on the arriving lanes' inner edge (toward the leaving lanes).
            let leaving = a
                .lanes
                .iter()
                .any(|l| !l.incoming && l.kind != crate::LaneKind::Tram);
            if leaving {
                let edge = if side > 0.0 { lo } else { hi };
                let s0 = s_line + 0.5 * STOP_LINE_WIDTH_M + REFUGE_GAP_M;
                let s1 = (s0 + ISLAND_LENGTH_M).min(len - MIN_TAIL_M);
                island(&mut m, a, i, edge, side, s0, s1, refuge);
            }
            continue;
        }
        let s_arrow = s_stop + ARROW_BEHIND_STOP_M;
        if s_arrow + 0.5 * ARROW_LENGTH_M + MIN_TAIL_M > len {
            continue;
        }
        for l in incoming {
            let mut turns = ArrowTurns::default();
            for c in j
                .connectors
                .iter()
                .filter(|c| c.from == i && c.from_lane == l.index && c.kind == ConnectorKind::Road)
            {
                match c.turn {
                    Turn::Left => turns.left = true,
                    Turn::Straight => turns.straight = true,
                    Turn::Right => turns.right = true,
                }
            }
            if turns == ArrowTurns::default() {
                continue;
            }
            m.arrows.push(LaneArrow {
                approach: i,
                lane: l.index,
                center: at(a, s_arrow, l.offset_m),
                dir: -a.centerline.tangent_at(s_arrow),
                turns,
            });
        }
    }
    m
}
