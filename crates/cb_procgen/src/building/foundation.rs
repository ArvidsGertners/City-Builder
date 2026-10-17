//! Foundations on slopes (ALGORITHMS §11 step 4). Shared by the sim (which emits the pad stamp
//! and stores `floor_y`) and the mesh generator, so both always agree.
//!
//! Decided from the ground under the footprint *before* the building's own pad is applied:
//! `Δh < 0.5 m` pad; `0.5–2.5 m` plinth on a pad; `2.5–5 m` stilts (residential); more is
//! unbuildable.

use glam::Vec2;

/// Pad / plinth threshold (m of height difference under the footprint).
pub const PAD_MAX_DH_M: f32 = 0.5;
/// Plinth / stilts threshold (m).
pub const PLINTH_MAX_DH_M: f32 = 2.5;
/// Stilts / unbuildable threshold (m).
pub const STILTS_MAX_DH_M: f32 = 5.0;
/// Floor above the pad (m).
pub const FLOOR_ABOVE_PAD_M: f32 = 0.2;
/// Pad stamp margin around the footprint (m).
pub const PAD_MARGIN_M: f32 = 0.6;

/// Foundation kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FoundationKind {
    /// Levelled pad, short plinth.
    Pad,
    /// Visible plinth on a pad at mean ground height.
    Plinth,
    /// Floor slab on posts (no terrain change).
    Stilts,
}

/// A planned foundation.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Foundation {
    /// Kind.
    pub kind: FoundationKind,
    /// Ground floor level (m).
    pub floor_y: f32,
    /// Lowest ground under the footprint before levelling (m).
    pub ground_min: f32,
    /// Pad stamp to emit: polygon, target height, falloff (m).
    pub pad: Option<PadStamp>,
}

/// Terrain pad request (becomes a `cb_terrain` `Pad` stamp with building priority).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PadStamp {
    /// Pad polygon (counter-clockwise).
    pub polygon: Vec<Vec2>,
    /// Target height (m).
    pub target_h_m: f32,
    /// Blend width (m).
    pub falloff_m: f32,
}

/// Plans the foundation for footprint quads (`corners` = all footprint corners and centres)
/// from ground heights `h`. `None` if the slope is too steep or stilts are not allowed.
#[must_use]
pub fn plan(outline: &[Vec2], samples: &[f32], allow_stilts: bool) -> Option<Foundation> {
    plan_with(outline, samples, allow_stilts, PLINTH_MAX_DH_M)
}

/// Like [`plan`] with a custom plinth limit (large civic parcels level a terrace).
#[must_use]
pub fn plan_with(
    outline: &[Vec2],
    samples: &[f32],
    allow_stilts: bool,
    plinth_max_dh_m: f32,
) -> Option<Foundation> {
    if samples.is_empty() {
        return None;
    }
    let lo = samples.iter().copied().fold(f32::INFINITY, f32::min);
    let hi = samples.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mean = samples.iter().sum::<f32>() / samples.len() as f32;
    // Centimetre grid so the stored floor level is exact in saves.
    let cm = |v: f32| (v * 100.0).round() / 100.0;
    let dh = hi - lo;
    let pad = |target: f32, falloff: f32| PadStamp {
        polygon: grow(outline, PAD_MARGIN_M),
        target_h_m: cm(target),
        falloff_m: falloff,
    };
    if dh < PAD_MAX_DH_M {
        let t = cm(mean);
        Some(Foundation {
            kind: FoundationKind::Pad,
            floor_y: cm(t + FLOOR_ABOVE_PAD_M),
            ground_min: lo,
            pad: Some(pad(t, 3.0)),
        })
    } else if dh < plinth_max_dh_m {
        let t = cm(mean);
        Some(Foundation {
            kind: FoundationKind::Plinth,
            floor_y: cm(t + FLOOR_ABOVE_PAD_M + 0.25 * dh),
            ground_min: lo,
            pad: Some(pad(t, 1.5)),
        })
    } else if dh < STILTS_MAX_DH_M && allow_stilts {
        Some(Foundation {
            kind: FoundationKind::Stilts,
            floor_y: cm(hi + 0.4),
            ground_min: lo,
            pad: None,
        })
    } else {
        None
    }
}

/// Grows a convex counter-clockwise polygon by `d` (vertices moved along the bisector).
fn grow(poly: &[Vec2], d: f32) -> Vec<Vec2> {
    let n = poly.len();
    let c = poly.iter().copied().sum::<Vec2>() / n.max(1) as f32;
    poly.iter()
        .map(|p| {
            let v = *p - c;
            let l = v.length();
            if l < 1e-6 {
                *p
            } else {
                *p + v / l * d * std::f32::consts::SQRT_2
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds() {
        let sq = [
            Vec2::ZERO,
            Vec2::X * 8.0,
            Vec2::new(8.0, 8.0),
            Vec2::Y * 8.0,
        ];
        let f = plan(&sq, &[10.0, 10.2, 10.3, 10.1], true).unwrap();
        assert_eq!(f.kind, FoundationKind::Pad);
        assert!((f.floor_y - 10.35).abs() < 0.011);
        assert_eq!(
            plan(&sq, &[10.0, 11.0, 12.0, 11.0], true).unwrap().kind,
            FoundationKind::Plinth
        );
        let s = plan(&sq, &[10.0, 12.0, 13.5, 11.0], true).unwrap();
        assert_eq!(s.kind, FoundationKind::Stilts);
        assert!(s.pad.is_none() && s.floor_y > 13.5);
        assert!(plan(&sq, &[10.0, 12.0, 13.5, 11.0], false).is_none());
        assert!(plan(&sq, &[10.0, 16.0], true).is_none());
    }
}
