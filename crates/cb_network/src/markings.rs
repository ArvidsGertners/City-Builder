//! Junction markings (ALGORITHMS §5 step 5): crosswalks 1 m behind each cut line on approaches
//! with sidewalks, stop lines 2 m upstream of the crosswalk across the arriving lanes, and lane
//! arrows showing each arriving lane's movements. None on bends and continuations (fewer than
//! three approaches).

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
/// Approaches need this much road beyond the arrow to carry markings (m).
const MIN_TAIL_M: f32 = 4.0;

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
}

fn at(a: &Approach, s: f32, offset: f32) -> Vec3 {
    let p = a.centerline.point_at(s) + a.centerline.tangent_at(s).perp() * offset;
    Vec3::new(p.x, a.height(s), p.y)
}

/// Markings for a junction built from `apps`.
#[must_use]
pub fn markings(apps: &[Approach], j: &Junction) -> Markings {
    let mut m = Markings::default();
    if apps.len() < 3 {
        return m;
    }
    for (i, a) in apps.iter().enumerate() {
        let Some(&cut) = j.cuts.get(i) else { continue };
        let len = a.centerline.length();
        let along = a.centerline.tangent_at(cut);
        let ch = a.carriageway_half_m;
        let sidewalks = a.half_width_m - ch > 1.0;
        let mut s_stop = cut + CROSSWALK_OFFSET_M;
        if sidewalks {
            let sc = cut + CROSSWALK_OFFSET_M + 0.5 * CROSSWALK_WIDTH_M;
            if sc + 0.5 * CROSSWALK_WIDTH_M < len {
                m.crosswalks.push(Crosswalk {
                    approach: i,
                    ends: [at(a, sc, -ch), at(a, sc, ch)],
                    along,
                    width_m: CROSSWALK_WIDTH_M,
                });
                s_stop = sc + 0.5 * CROSSWALK_WIDTH_M + STOP_GAP_M;
            }
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
        m.stop_lines.push(StopLine {
            approach: i,
            ends: [at(a, s_line, lo), at(a, s_line, hi)],
            along,
        });
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
