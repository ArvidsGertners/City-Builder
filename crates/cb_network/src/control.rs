//! Junction control (ALGORITHMS §6, ADR-0066): per-approach priority classes, automatic
//! classification by road hierarchy, and signal phase plans built from the junction's
//! connectors and conflict zones. The runtime (grants, gap checks, actuation) lives in
//! `cb_traffic` (M8/M9); M3 needs the plans for validation, signal heads and the overlay.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::connectors::{ConflictZone, Connector, ConnectorKind, Turn};

/// Minimum green (sim s).
pub const MIN_GREEN_SIM_S: f32 = 8.0;
/// Maximum green (sim s).
pub const MAX_GREEN_SIM_S: f32 = 30.0;
/// Yellow (sim s).
pub const YELLOW_SIM_S: f32 = 3.0;
/// All-red clearance (sim s).
pub const ALL_RED_SIM_S: f32 = 1.5;
/// Green of the fixed-time stub clock used until actuation arrives (sim s).
pub const STUB_GREEN_SIM_S: f32 = 20.0;

/// How one approach enters the junction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ApproachControl {
    /// Major road: goes first.
    Priority,
    /// Gives way to priority approaches.
    Yield,
    /// Full stop, then gives way.
    Stop,
}

/// Node-level control mode (the per-approach overrides are keyed by segment in the sim).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ControlMode {
    /// Priority classes from the road hierarchy.
    #[default]
    Auto,
    /// Player-set classes per approach.
    Custom,
    /// Every approach stops; first come, first served.
    AllWayStop,
    /// Traffic lights with a generated phase plan.
    Lights,
}

/// Hierarchy rank of a road (higher = more major).
#[must_use]
pub fn rank(kind: crate::RoadKind) -> u8 {
    use crate::RoadKind as K;
    match kind {
        K::Dirt => 0,
        K::Street | K::OneWayStreet | K::TramTrack => 1,
        K::Avenue | K::OneWayAvenue => 2,
    }
}

/// Automatic classes (ALGORITHMS §6): the two most opposite approaches among the highest rank
/// are Priority, every other approach yields. Two approaches (bends) are both Priority.
/// `dirs` are approach directions leaving the node.
#[must_use]
pub fn auto_controls(ranks: &[u8], dirs: &[Vec2]) -> Vec<ApproachControl> {
    let n = ranks.len().min(dirs.len());
    if n <= 2 {
        return vec![ApproachControl::Priority; n];
    }
    let top = ranks.iter().take(n).copied().max().unwrap_or(0);
    let majors: Vec<usize> = (0..n).filter(|&i| ranks[i] == top).collect();
    let mut out = vec![ApproachControl::Yield; n];
    if majors.len() == 1 {
        out[majors[0]] = ApproachControl::Priority;
        // The best continuation of the single major road is major too.
        if let Some(j) = (0..n).filter(|&j| j != majors[0]).min_by(|&a, &b| {
            dirs[majors[0]]
                .dot(dirs[a])
                .total_cmp(&dirs[majors[0]].dot(dirs[b]))
                .then(ranks[b].cmp(&ranks[a]))
                .then(a.cmp(&b))
        }) {
            out[j] = ApproachControl::Priority;
        }
        return out;
    }
    let mut best: Option<(f32, usize, usize)> = None;
    for (x, &a) in majors.iter().enumerate() {
        for &b in &majors[x + 1..] {
            let d = dirs[a].dot(dirs[b]);
            if best.is_none_or(|(bd, ..)| d < bd) {
                best = Some((d, a, b));
            }
        }
    }
    if let Some((_, a, b)) = best {
        out[a] = ApproachControl::Priority;
        out[b] = ApproachControl::Priority;
    }
    out
}

/// One signal phase: the connectors (indices into the junction's connectors) that are green.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SignalPhase {
    /// Green connectors, ascending.
    pub connectors: Vec<usize>,
}

/// Connectors `a` and `b` conflict: they share a conflict zone and come from different
/// approaches (same-approach alternatives are sorted out by the lane they use).
fn conflicting(conns: &[Connector], member_zones: &[Vec<usize>], a: usize, b: usize) -> bool {
    conns[a].from != conns[b].from && member_zones[a].iter().any(|z| member_zones[b].contains(z))
}

fn turn_order(t: Turn) -> u8 {
    match t {
        Turn::Straight => 0,
        Turn::Right => 1,
        Turn::Left => 2,
    }
}

/// Phase plan (ALGORITHMS §6): greedy maximal compatible sets seeded by the most opposite
/// approach pairs; lefts are protected (they never share a phase with a conflicting through
/// movement). Every connector is green in at least one phase and no phase holds two
/// conflicting connectors. `dirs` are approach directions leaving the node.
#[must_use]
pub fn phase_plan(conns: &[Connector], zones: &[ConflictZone], dirs: &[Vec2]) -> Vec<SignalPhase> {
    let m = conns.len();
    let mut member_zones: Vec<Vec<usize>> = vec![Vec::new(); m];
    for (zi, z) in zones.iter().enumerate() {
        for (c, _) in &z.members {
            if let Some(v) = member_zones.get_mut(*c)
                && !v.contains(&zi)
            {
                v.push(zi);
            }
        }
    }
    // Approach pairs, most opposite first; then single approaches.
    let n = dirs.len();
    let mut pairs: Vec<(f32, usize, usize)> = Vec::new();
    for a in 0..n {
        for b in a + 1..n {
            pairs.push((dirs[a].dot(dirs[b]), a, b));
        }
    }
    pairs.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)).then(x.2.cmp(&y.2)));
    // Priority order of connectors: by seed pair, then turn (straight, right, left), road
    // before tram, index.
    let mut order: Vec<usize> = Vec::with_capacity(m);
    for &(_, a, b) in &pairs {
        let mut group: Vec<usize> = (0..m)
            .filter(|&c| (conns[c].from == a || conns[c].from == b) && !order.contains(&c))
            .collect();
        group.sort_by_key(|&c| {
            (
                turn_order(conns[c].turn),
                conns[c].kind == ConnectorKind::Tram,
                c,
            )
        });
        order.extend(group);
    }
    for c in 0..m {
        if !order.contains(&c) {
            order.push(c);
        }
    }
    let mut covered = vec![false; m];
    let mut phases: Vec<SignalPhase> = Vec::new();
    while let Some(seed) = order.iter().copied().find(|&c| !covered[c]) {
        let mut green: Vec<usize> = vec![seed];
        // Fill with uncovered connectors first (fewest phases), then with covered ones.
        for pass_uncovered in [true, false] {
            for &c in &order {
                if green.contains(&c) || covered[c] != !pass_uncovered {
                    continue;
                }
                if green
                    .iter()
                    .all(|&g| !conflicting(conns, &member_zones, g, c))
                {
                    green.push(c);
                }
            }
        }
        for &g in &green {
            covered[g] = true;
        }
        green.sort_unstable();
        phases.push(SignalPhase { connectors: green });
    }
    phases
}

/// Signal state of a phase at a time on the fixed-time stub clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalState {
    /// Green.
    Green,
    /// Yellow.
    Yellow,
    /// Red (incl. all-red clearance).
    Red,
}

/// Active phase and its state at `t_sim_s` on the stub clock (each phase: green, yellow,
/// all-red), offset by `offset_sim_s` so neighbouring junctions are not in lock-step.
#[must_use]
pub fn stub_phase_at(
    phases: usize,
    t_sim_s: f64,
    offset_sim_s: f64,
) -> Option<(usize, SignalState)> {
    if phases == 0 {
        return None;
    }
    let slot = f64::from(STUB_GREEN_SIM_S + YELLOW_SIM_S + ALL_RED_SIM_S);
    let cycle = slot * phases as f64;
    let t = (t_sim_s + offset_sim_s).rem_euclid(cycle);
    let k = ((t / slot) as usize).min(phases - 1);
    let in_slot = t - k as f64 * slot;
    let state = if in_slot < f64::from(STUB_GREEN_SIM_S) {
        SignalState::Green
    } else if in_slot < f64::from(STUB_GREEN_SIM_S + YELLOW_SIM_S) {
        SignalState::Yellow
    } else {
        SignalState::Red
    };
    Some((k, state))
}
