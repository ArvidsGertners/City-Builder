//! Bridges (ALGORITHMS §3, ADR-0065): span classification from the terrain-following profile
//! (with hysteresis), the deck re-solve input (no terrain attraction on spans, clearance lower
//! bounds), and pier placement that avoids other roads.

use glam::Vec2;

use crate::RoadKind;
use crate::profile::{Profile, ProfileInput};
use crate::segment_geom::SegmentGeom;

/// Height above ground that marks a bridge candidate on the way in; on the way out the
/// threshold is 1 m lower (hysteresis).
pub const HYSTERESIS_M: f32 = 1.0;
/// Spans extend this far beyond the last near-ground sample (stable abutment).
pub const ABUTMENT_M: f32 = 4.0;
/// "Near ground": road within this of the ground.
pub const NEAR_GROUND_M: f32 = 1.0;
/// Minimum span length (m).
pub const MIN_SPAN_M: f32 = 12.0;
/// Spans closer than this merge (m).
pub const MERGE_GAP_M: f32 = 20.0;
/// Overpass candidate: another road this far below (m).
pub const OVERPASS_DH_M: f32 = 4.0;
/// Extra span beyond a crossed road's half width on each side (m).
pub const OVERPASS_MARGIN_M: f32 = 6.0;
/// Deck underside clearance over water (m).
pub const WATER_CLEARANCE_M: f32 = 4.0;
/// Deck underside clearance over roads (m).
pub const ROAD_CLEARANCE_M: f32 = 5.5;
/// A road built under a deck needs this much below its underside (m).
pub const UNDER_DECK_CLEARANCE_M: f32 = 4.5;
/// Maximum deck height above ground (m).
pub const MAX_DECK_HEIGHT_M: f32 = 40.0;
/// Maximum unsupported span between piers or abutments (m).
pub const MAX_UNSUPPORTED_M: f32 = 60.0;
/// Piers taller than this use the tapered style (m).
pub const TAPERED_PIER_M: f32 = 25.0;

/// Per-type bridge parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BridgeParams {
    /// Fill height (road above ground) that makes a bridge candidate (m).
    pub fill_threshold_m: f32,
    /// Deck thickness below the road surface (m).
    pub deck_m: f32,
    /// Target pier spacing (m).
    pub pier_spacing_m: f32,
}

/// Bridge parameters of a road type.
#[must_use]
pub fn params(kind: RoadKind) -> BridgeParams {
    match kind {
        RoadKind::Avenue | RoadKind::OneWayAvenue => BridgeParams {
            fill_threshold_m: 8.0,
            deck_m: 1.6,
            pier_spacing_m: 36.0,
        },
        RoadKind::TramTrack => BridgeParams {
            fill_threshold_m: 6.0,
            deck_m: 1.4,
            pier_spacing_m: 30.0,
        },
        RoadKind::Dirt | RoadKind::Street | RoadKind::OneWayStreet => BridgeParams {
            fill_threshold_m: 6.0,
            deck_m: 1.2,
            pier_spacing_m: 30.0,
        },
    }
}

/// Per-sample facts along the new road for span classification.
#[derive(Clone, Debug, PartialEq)]
pub struct SpanSamples {
    /// Arc-length positions.
    pub s: Vec<f32>,
    /// Terrain-following profile heights (before any deck re-solve).
    pub follow: Vec<f32>,
    /// Ground heights.
    pub ground: Vec<f32>,
    /// Water surface height where the ground is under water (`None` when dry).
    pub water: Vec<Option<f32>>,
    /// Surface height of another road crossed below (within its half width + margin).
    pub road_below: Vec<Option<f32>>,
}

/// Bridge spans `[s0, s1]` (ALGORITHMS §3 steps 1–2): candidates over water, over fill above
/// the type's threshold, or over another road ≥ 4 m below; entering at the threshold and leaving
/// at threshold − 1 m; extended to stable abutments; merged when closer than 20 m; at least
/// 12 m long.
#[must_use]
pub fn classify_spans(x: &SpanSamples, p: &BridgeParams, len: f32) -> Vec<(f32, f32)> {
    let n = x.s.len();
    let cand = |i: usize, thr: f32| {
        let fill = x.follow[i] - x.ground[i];
        x.water[i].is_some()
            || fill > thr
            || x.road_below[i].is_some_and(|h| x.follow[i] - h >= OVERPASS_DH_M - HYSTERESIS_M)
    };
    let mut on = vec![false; n];
    let mut state = false;
    for (i, o) in on.iter_mut().enumerate() {
        state = if state {
            cand(i, p.fill_threshold_m - HYSTERESIS_M)
        } else {
            cand(i, p.fill_threshold_m)
        };
        *o = state;
    }
    let near_ground = |i: usize| x.water[i].is_none() && x.follow[i] - x.ground[i] <= NEAR_GROUND_M;
    let mut spans: Vec<(f32, f32)> = Vec::new();
    let mut i = 0;
    while i < n {
        if !on[i] {
            i += 1;
            continue;
        }
        let mut j = i;
        while j + 1 < n && on[j + 1] {
            j += 1;
        }
        let (mut a, mut b) = (i, j);
        while a > 0 && !near_ground(a) {
            a -= 1;
        }
        while b + 1 < n && !near_ground(b) {
            b += 1;
        }
        spans.push((
            (x.s[a] - ABUTMENT_M).max(0.0),
            (x.s[b] + ABUTMENT_M).min(len),
        ));
        i = j + 1;
    }
    let mut out: Vec<(f32, f32)> = Vec::new();
    for sp in spans {
        if let Some(l) = out.last_mut()
            && sp.0 - l.1 < MERGE_GAP_M
        {
            l.1 = l.1.max(sp.1);
            continue;
        }
        out.push(sp);
    }
    for sp in &mut out {
        if sp.1 - sp.0 < MIN_SPAN_M {
            let m = 0.5 * (sp.0 + sp.1);
            *sp = (
                (m - 0.5 * MIN_SPAN_M).max(0.0),
                (m + 0.5 * MIN_SPAN_M).min(len),
            );
        }
    }
    out
}

/// Whether `s` lies on one of `spans`.
#[must_use]
pub fn on_span(spans: &[(f32, f32)], s: f32) -> bool {
    spans.iter().any(|(a, b)| s >= *a && s <= *b)
}

/// Deck re-solve input (ALGORITHMS §3 step 3): the terrain-following solution attracts the road
/// off the spans; on spans only smoothness and grade act, with the deck underside held above
/// water + 4 m and roads below + 5.5 m.
#[must_use]
pub fn deck_input(
    base: &ProfileInput,
    x: &SpanSamples,
    spans: &[(f32, f32)],
    p: &BridgeParams,
) -> ProfileInput {
    let mut inp = base.clone();
    let follow_at = |s: f32| {
        let k =
            x.s.partition_point(|v| *v <= s)
                .clamp(1, x.s.len().max(2) - 1);
        let (s0, s1) = (x.s[k - 1], x.s[k]);
        let t = if s1 > s0 {
            ((s - s0) / (s1 - s0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        x.follow[k - 1] + (x.follow[k] - x.follow[k - 1]) * t
    };
    for i in 0..inp.s.len() {
        inp.ground[i] = x.follow[i];
        let Some(&(a, b)) = spans.iter().find(|(a, b)| inp.s[i] >= *a && inp.s[i] <= *b) else {
            continue;
        };
        // Start from the straight line between the span ends: with no terrain attraction the
        // smoothing alone would need far more passes to lift a long deck out of the valley.
        let t = if b > a { (inp.s[i] - a) / (b - a) } else { 0.0 };
        inp.ground[i] = follow_at(a) + (follow_at(b) - follow_at(a)) * t;
        inp.weight[i] = 0.0;
        let mut lo = f32::NEG_INFINITY;
        if let Some(w) = x.water[i] {
            lo = lo.max(w + WATER_CLEARANCE_M + p.deck_m);
        }
        if let Some(r) = x.road_below[i] {
            lo = lo.max(r + ROAD_CLEARANCE_M + p.deck_m);
        }
        inp.lower[i] = inp.lower[i].max(lo);
    }
    inp
}

/// A pier.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Pier {
    /// Arc length on the segment.
    pub s_m: f32,
    /// Plan position.
    pub at: Vec2,
    /// Deck underside height.
    pub top_m: f32,
    /// Ground height at the foot.
    pub foot_m: f32,
}

impl Pier {
    /// Height (m).
    #[must_use]
    pub fn height_m(&self) -> f32 {
        self.top_m - self.foot_m
    }
}

/// Piers per span (ALGORITHMS §3 step 4): evenly spaced at the type spacing; a pier in a
/// forbidden zone (other road footprints) shifts in 5 % steps up to ±25 % of the gap, else the
/// span tries one more and one fewer pier (keeping ≤ 60 m unsupported).
///
/// # Errors
/// The arc length of a span whose piers cannot be placed.
pub fn place_piers(
    geom: &SegmentGeom,
    profile: &Profile,
    spans: &[(f32, f32)],
    p: &BridgeParams,
    ground: &dyn Fn(Vec2) -> f32,
    forbidden: &dyn Fn(Vec2) -> bool,
) -> Result<Vec<Pier>, f32> {
    const SHIFTS: [f32; 11] = [
        0.0, 0.05, -0.05, 0.1, -0.1, 0.15, -0.15, 0.2, -0.2, 0.25, -0.25,
    ];
    let mut out = Vec::new();
    for &(s0, s1) in spans {
        let span = s1 - s0;
        let base_n = ((span / p.pier_spacing_m).ceil() as usize).saturating_sub(1);
        let mut placed: Option<Vec<f32>> = None;
        'counts: for n in [base_n, base_n + 1, base_n.saturating_sub(1)] {
            if span / (n + 1) as f32 > MAX_UNSUPPORTED_M {
                continue;
            }
            let gap = span / (n + 1) as f32;
            let mut ss = Vec::with_capacity(n);
            for k in 1..=n {
                let s = s0 + gap * k as f32;
                match SHIFTS
                    .iter()
                    .map(|d| s + d * gap)
                    .find(|q| !forbidden(geom.point_at(*q)))
                {
                    Some(q) => ss.push(q),
                    None => continue 'counts,
                }
            }
            placed = Some(ss);
            break;
        }
        let Some(ss) = placed else {
            return Err(0.5 * (s0 + s1));
        };
        for s in ss {
            let at = geom.point_at(s);
            out.push(Pier {
                s_m: s,
                at,
                top_m: profile.height_at(s) - p.deck_m,
                foot_m: ground(at),
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(follow: &[f32]) -> SpanSamples {
        let n = follow.len();
        SpanSamples {
            s: (0..n).map(|i| i as f32 * 4.0).collect(),
            follow: follow.to_vec(),
            ground: vec![0.0; n],
            water: vec![None; n],
            road_below: vec![None; n],
        }
    }

    #[test]
    fn bridge_classification_hysteresis() {
        // Fill hovering around the 6 m threshold (5.4 … 6.6 m) must give one span, not many.
        let mut f = vec![0.0f32; 10];
        for k in 0..30 {
            f.push(if k % 2 == 0 { 6.6 } else { 5.4 });
        }
        f.extend(vec![0.0; 10]);
        let len = (f.len() - 1) as f32 * 4.0;
        let spans = classify_spans(&samples(&f), &params(RoadKind::Street), len);
        assert_eq!(spans.len(), 1, "{spans:?}");
        // Without hysteresis-crossing values it would drop at every 5.4 m sample: entering needs
        // > 6 m, leaving needs ≤ 5 m.
        let (a, b) = spans[0];
        assert!(a <= 40.0 - ABUTMENT_M + 1e-3 && b >= 156.0, "{spans:?}");
        // A fill that never exceeds the threshold gives no bridge.
        let low = vec![5.9f32; 30];
        assert!(classify_spans(&samples(&low), &params(RoadKind::Street), 116.0).is_empty());
    }

    #[test]
    fn spans_merge_and_respect_min_length() {
        // Two short water crossings 12 m apart merge; a single 4 m crossing grows to 12 m.
        let mut x = samples(&[0.0; 40]);
        for i in [10, 11, 15, 16] {
            x.water[i] = Some(0.0);
        }
        x.water[30] = Some(0.0);
        let spans = classify_spans(&x, &params(RoadKind::Street), 156.0);
        assert_eq!(spans.len(), 2, "{spans:?}");
        assert!(spans.iter().all(|(a, b)| b - a >= MIN_SPAN_M - 1e-3));
    }
}
