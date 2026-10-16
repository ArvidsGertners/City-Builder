//! Road cross-sections (GAME_DESIGN §3): ordered elements from the left edge to the right edge
//! of a segment (in curve direction). Widths are geometry, not balance; speeds, costs and
//! capacities live in the catalog (`cb_sim/data/roads.ron`).

use serde::{Deserialize, Serialize};

/// Element kinds across a road.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ElementKind {
    /// Pedestrian sidewalk (raised, with curb).
    Sidewalk,
    /// Car lane driving forward (along the curve).
    LaneForward,
    /// Car lane driving backward.
    LaneBackward,
    /// Central median (raised, grass).
    Median,
    /// Unpaved strip (dirt paths).
    Verge,
}

/// One element.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Element {
    /// Kind.
    pub kind: ElementKind,
    /// Width (m).
    pub width_m: f32,
}

/// Road types with built-in cross-sections.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RoadKind {
    /// Dirt path, 1 + 1 narrow lanes, 6 m.
    Dirt,
    /// Street, 1 + 1 lanes, 12 m incl. 2 × 2 m sidewalks.
    Street,
    /// One-way street, 2 lanes, 12 m.
    OneWayStreet,
    /// Avenue, 2 + 2 lanes and a 2 m median, 22 m.
    Avenue,
    /// One-way avenue, 3 lanes, 16 m.
    OneWayAvenue,
}

/// A cross-section.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CrossSection {
    /// Elements from the left edge to the right edge.
    pub elements: Vec<Element>,
}

impl CrossSection {
    /// Built-in cross-section of a road kind.
    #[must_use]
    pub fn of(kind: RoadKind) -> Self {
        use ElementKind::{
            LaneBackward as B, LaneForward as F, Median as M, Sidewalk as S, Verge as V,
        };
        let e = |kind, width_m| Element { kind, width_m };
        let elements = match kind {
            RoadKind::Dirt => vec![e(V, 0.5), e(B, 2.5), e(F, 2.5), e(V, 0.5)],
            RoadKind::Street => vec![e(S, 2.0), e(B, 4.0), e(F, 4.0), e(S, 2.0)],
            RoadKind::OneWayStreet => vec![e(S, 2.0), e(F, 4.0), e(F, 4.0), e(S, 2.0)],
            RoadKind::Avenue => vec![
                e(S, 2.0),
                e(B, 4.0),
                e(B, 4.0),
                e(M, 2.0),
                e(F, 4.0),
                e(F, 4.0),
                e(S, 2.0),
            ],
            RoadKind::OneWayAvenue => vec![e(S, 2.0), e(F, 4.0), e(F, 4.0), e(F, 4.0), e(S, 2.0)],
        };
        Self { elements }
    }

    /// Total width (m).
    #[must_use]
    pub fn width_m(&self) -> f32 {
        self.elements.iter().map(|e| e.width_m).sum()
    }

    /// Half width (m).
    #[must_use]
    pub fn half_width_m(&self) -> f32 {
        0.5 * self.width_m()
    }

    /// Elements with their signed centre offsets (left of the centreline positive, ADR-0033
    /// left = +perp of the curve direction).
    #[must_use]
    pub fn offsets(&self) -> Vec<(Element, f32)> {
        let half = self.half_width_m();
        let mut acc = 0.0;
        self.elements
            .iter()
            .map(|e| {
                let c = half - acc - 0.5 * e.width_m;
                acc += e.width_m;
                (*e, c)
            })
            .collect()
    }

    /// Driving lanes `(forward, backward)`.
    #[must_use]
    pub fn lane_counts(&self) -> (usize, usize) {
        let f = self
            .elements
            .iter()
            .filter(|e| e.kind == ElementKind::LaneForward)
            .count();
        let b = self
            .elements
            .iter()
            .filter(|e| e.kind == ElementKind::LaneBackward)
            .count();
        (f, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_match_game_design() {
        assert_eq!(CrossSection::of(RoadKind::Dirt).width_m(), 6.0);
        assert_eq!(CrossSection::of(RoadKind::Street).width_m(), 12.0);
        assert_eq!(CrossSection::of(RoadKind::OneWayStreet).width_m(), 12.0);
        assert_eq!(CrossSection::of(RoadKind::Avenue).width_m(), 22.0);
        assert_eq!(CrossSection::of(RoadKind::OneWayAvenue).width_m(), 16.0);
        assert_eq!(CrossSection::of(RoadKind::Avenue).lane_counts(), (2, 2));
        assert_eq!(
            CrossSection::of(RoadKind::OneWayAvenue).lane_counts(),
            (3, 0)
        );
    }

    #[test]
    fn offsets_are_symmetric_for_streets() {
        let o = CrossSection::of(RoadKind::Street).offsets();
        assert_eq!(o[0].1, 5.0);
        assert_eq!(o[3].1, -5.0);
        assert_eq!(o[1].1, 2.0);
        assert_eq!(o[2].1, -2.0);
    }
}
