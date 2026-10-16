//! Road cross-sections (GAME_DESIGN §3, ADR-0008): ordered elements from the left edge to the
//! right edge of a segment (in curve direction), with lane kinds for buses and trams. Traffic
//! drives on the right: forward lanes lie right of the centreline. Widths are geometry, not
//! balance; speeds, costs and capacities live in the catalog (`cb_sim/data/roads.ron`).

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

/// What may use a driving lane (ADR-0008; `Bike` is reserved).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum LaneKind {
    /// Cars, trucks, buses.
    #[default]
    Car,
    /// Buses only (and emergency vehicles).
    Bus,
    /// Trams only (standalone track).
    Tram,
    /// Cars and trams share the lane (tracks in the road).
    CarTramShared,
}

impl LaneKind {
    /// Cars may use the lane.
    #[must_use]
    pub fn cars(self) -> bool {
        matches!(self, Self::Car | Self::CarTramShared)
    }
    /// Trams may use the lane.
    #[must_use]
    pub fn trams(self) -> bool {
        matches!(self, Self::Tram | Self::CarTramShared)
    }
    /// Minimum connector radius (m) of the most demanding user (ADR-0063).
    #[must_use]
    pub fn min_connector_radius_m(self) -> f32 {
        match self {
            Self::Car => 5.0,
            Self::Bus => 8.0,
            Self::Tram | Self::CarTramShared => 20.0,
        }
    }
}

/// One element.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Element {
    /// Kind.
    pub kind: ElementKind,
    /// Width (m).
    pub width_m: f32,
    /// Users of a driving lane (ignored for other elements).
    #[serde(default)]
    pub lane: LaneKind,
}

/// Travel direction relative to the segment curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Dir {
    /// Along the curve (start → end).
    Forward,
    /// Against the curve.
    Backward,
}

/// A driving lane of a cross-section.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LaneSpec {
    /// Travel direction.
    pub dir: Dir,
    /// Index within its direction, 0 = rightmost in travel direction.
    pub index: u8,
    /// Users.
    pub kind: LaneKind,
    /// Centre offset from the centreline (m, left of the curve positive).
    pub offset_m: f32,
    /// Width (m).
    pub width_m: f32,
}

/// Optional lane equipment of a segment (ADR-0008; UI hidden until M10).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Variant {
    /// Outermost lane per direction reserved for buses (roads with ≥ 2 lanes per direction).
    pub bus_lanes: bool,
    /// Tram tracks in the road (streets and avenues).
    pub tram_tracks: bool,
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
    /// Standalone tram track, 1 + 1 tram lanes on ballast, 8 m.
    TramTrack,
}

impl RoadKind {
    /// Every road kind (catalog completeness, UI lists).
    pub const ALL: [Self; 6] = [
        Self::Dirt,
        Self::Street,
        Self::OneWayStreet,
        Self::Avenue,
        Self::OneWayAvenue,
        Self::TramTrack,
    ];

    /// Whether the variant is possible on this kind.
    #[must_use]
    pub fn allows(self, v: Variant) -> bool {
        let bus_ok = matches!(self, Self::OneWayStreet | Self::Avenue | Self::OneWayAvenue);
        let tram_ok = matches!(self, Self::Street | Self::Avenue);
        (!v.bus_lanes || bus_ok) && (!v.tram_tracks || tram_ok)
    }
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
        let e = |kind, width_m| Element {
            kind,
            width_m,
            lane: LaneKind::Car,
        };
        let t = |kind, width_m| Element {
            kind,
            width_m,
            lane: LaneKind::Tram,
        };
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
            RoadKind::TramTrack => vec![e(V, 1.0), t(B, 3.0), t(F, 3.0), e(V, 1.0)],
        };
        Self { elements }
    }

    /// Cross-section with optional bus lanes / tram tracks; a variant the kind does not allow is
    /// ignored (see [`RoadKind::allows`]).
    #[must_use]
    pub fn with(kind: RoadKind, v: Variant) -> Self {
        let mut cs = Self::of(kind);
        let v = Variant {
            bus_lanes: v.bus_lanes
                && kind.allows(Variant {
                    bus_lanes: true,
                    ..v
                }),
            tram_tracks: v.tram_tracks
                && kind.allows(Variant {
                    tram_tracks: true,
                    ..v
                }),
        };
        let lanes = cs.lanes();
        for l in &lanes {
            let per_dir = lanes.iter().filter(|o| o.dir == l.dir).count();
            let k = cs.element_index_of(l);
            if v.tram_tracks && l.index as usize == per_dir - 1 {
                // Tracks share the innermost lane of each direction.
                cs.elements[k].lane = LaneKind::CarTramShared;
            } else if v.bus_lanes && l.index == 0 && per_dir >= 2 {
                cs.elements[k].lane = LaneKind::Bus;
            }
        }
        cs
    }

    /// Index into `elements` of a lane returned by [`Self::lanes`].
    fn element_index_of(&self, l: &LaneSpec) -> usize {
        self.offsets()
            .iter()
            .position(|(_, o)| (*o - l.offset_m).abs() < 1e-4)
            .unwrap_or(0)
    }

    /// Driving lanes with direction, index from the right in travel direction, kind and offset.
    #[must_use]
    pub fn lanes(&self) -> Vec<LaneSpec> {
        let offs = self.offsets();
        let mut fwd: Vec<(Element, f32)> = offs
            .iter()
            .copied()
            .filter(|(e, _)| e.kind == ElementKind::LaneForward)
            .collect();
        let mut bwd: Vec<(Element, f32)> = offs
            .iter()
            .copied()
            .filter(|(e, _)| e.kind == ElementKind::LaneBackward)
            .collect();
        // Rightmost first: forward lanes have the most negative offset on the right, backward
        // lanes (travelling against the curve) the most positive.
        fwd.sort_by(|a, b| a.1.total_cmp(&b.1));
        bwd.sort_by(|a, b| b.1.total_cmp(&a.1));
        let spec = |dir, (i, (e, o)): (usize, &(Element, f32))| LaneSpec {
            dir,
            index: i as u8,
            kind: e.lane,
            offset_m: *o,
            width_m: e.width_m,
        };
        fwd.iter()
            .enumerate()
            .map(|x| spec(Dir::Forward, x))
            .chain(bwd.iter().enumerate().map(|x| spec(Dir::Backward, x)))
            .collect()
    }

    /// Half width of the paved carriageway (driving lanes and medians, without sidewalks or
    /// verges): where kerbs are.
    #[must_use]
    pub fn carriageway_half_m(&self) -> f32 {
        let outer: f32 = self
            .elements
            .iter()
            .filter(|e| matches!(e.kind, ElementKind::Sidewalk | ElementKind::Verge))
            .map(|e| e.width_m)
            .sum();
        0.5 * (self.width_m() - outer)
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
    fn lanes_count_offsets_per_cross_section() {
        use Dir::{Backward, Forward};
        // (kind, variant) -> lanes (dir, index, kind, offset)
        let check = |kind, v: Variant, want: &[(Dir, u8, LaneKind, f32)]| {
            let cs = CrossSection::with(kind, v);
            let got: Vec<(Dir, u8, LaneKind, f32)> = cs
                .lanes()
                .iter()
                .map(|l| (l.dir, l.index, l.kind, l.offset_m))
                .collect();
            assert_eq!(got, want, "{kind:?} {v:?}");
            // Widths and the carriageway are unchanged by variants.
            assert_eq!(cs.width_m(), CrossSection::of(kind).width_m());
        };
        let none = Variant::default();
        let bus = Variant {
            bus_lanes: true,
            tram_tracks: false,
        };
        let tram = Variant {
            bus_lanes: false,
            tram_tracks: true,
        };
        let both = Variant {
            bus_lanes: true,
            tram_tracks: true,
        };
        use LaneKind::{Bus, Car, CarTramShared as Shared, Tram};
        check(
            RoadKind::Dirt,
            none,
            &[(Forward, 0, Car, -1.25), (Backward, 0, Car, 1.25)],
        );
        check(
            RoadKind::Street,
            none,
            &[(Forward, 0, Car, -2.0), (Backward, 0, Car, 2.0)],
        );
        // Bus lanes need two lanes per direction: ignored on streets.
        check(
            RoadKind::Street,
            bus,
            &[(Forward, 0, Car, -2.0), (Backward, 0, Car, 2.0)],
        );
        check(
            RoadKind::Street,
            tram,
            &[(Forward, 0, Shared, -2.0), (Backward, 0, Shared, 2.0)],
        );
        check(
            RoadKind::OneWayStreet,
            none,
            &[(Forward, 0, Car, -2.0), (Forward, 1, Car, 2.0)],
        );
        check(
            RoadKind::OneWayStreet,
            bus,
            &[(Forward, 0, Bus, -2.0), (Forward, 1, Car, 2.0)],
        );
        check(
            RoadKind::OneWayStreet,
            tram,
            &[(Forward, 0, Car, -2.0), (Forward, 1, Car, 2.0)],
        );
        let av = [
            (Forward, 0, Car, -7.0),
            (Forward, 1, Car, -3.0),
            (Backward, 0, Car, 7.0),
            (Backward, 1, Car, 3.0),
        ];
        check(RoadKind::Avenue, none, &av);
        check(
            RoadKind::Avenue,
            both,
            &[
                (Forward, 0, Bus, -7.0),
                (Forward, 1, Shared, -3.0),
                (Backward, 0, Bus, 7.0),
                (Backward, 1, Shared, 3.0),
            ],
        );
        check(
            RoadKind::OneWayAvenue,
            bus,
            &[
                (Forward, 0, Bus, -4.0),
                (Forward, 1, Car, 0.0),
                (Forward, 2, Car, 4.0),
            ],
        );
        check(
            RoadKind::TramTrack,
            none,
            &[(Forward, 0, Tram, -1.5), (Backward, 0, Tram, 1.5)],
        );
        assert!(!RoadKind::TramTrack.allows(bus) && !RoadKind::Dirt.allows(tram));
        assert_eq!(CrossSection::of(RoadKind::Avenue).carriageway_half_m(), 9.0);
        assert_eq!(CrossSection::of(RoadKind::TramTrack).width_m(), 8.0);
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
