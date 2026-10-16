//! Data catalog embedded at compile time (`crates/cb_sim/data/*.ron`): road types, zone
//! parameters and economy basics. Balance numbers live here and in GAME_DESIGN.md (CLAUDE.md).

use std::collections::BTreeMap;

use bevy::ecs::resource::Resource;
use cb_core::Money;
use cb_network::RoadKind;
use serde::{Deserialize, Serialize};

use crate::config::Difficulty;

/// Zone types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ZoneKind {
    /// Housing.
    Residential,
    /// Shops and offices.
    Commercial,
    /// Workshops, factories.
    Industrial,
}

/// One road type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoadType {
    /// Construction cost (cents per metre).
    pub cost_cents_per_m: i64,
    /// Upkeep (units per metre per game day).
    pub upkeep_per_m_day: f32,
    /// Speed limit.
    pub speed_kmh: f32,
    /// Capacity per lane (veh per sim-hour).
    pub capacity_per_lane_per_sim_h: f32,
    /// Maximum grade.
    pub max_grade: f32,
    /// Minimum curve radius (m).
    pub min_radius_m: f32,
    /// Junction corner radius (m).
    pub corner_radius_m: f32,
}

/// Zone subdivision and setback parameters.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ZoneSpec {
    /// Minimum lot frontage (m).
    pub w_min: f32,
    /// Target lot frontage (m).
    pub w_target: f32,
    /// Maximum lot frontage (m).
    pub w_max: f32,
    /// Maximum plot depth (m).
    pub d_max: f32,
    /// Minimum lot area (m²).
    pub a_min: f32,
    /// Front, side, rear setbacks (m).
    pub setback: (f32, f32, f32),
    /// Default plot depth of the plot tool (m).
    pub default_depth_m: f32,
}

impl ZoneSpec {
    /// As subdivision parameters.
    #[must_use]
    pub fn params(&self) -> cb_geometry::subdivide::ZoneParams {
        cb_geometry::subdivide::ZoneParams {
            w_min: f64::from(self.w_min),
            w_target: f64::from(self.w_target),
            w_max: f64::from(self.w_max),
            d_max: f64::from(self.d_max),
            a_min: f64::from(self.a_min),
            setback: [
                f64::from(self.setback.0),
                f64::from(self.setback.1),
                f64::from(self.setback.2),
            ],
        }
    }
}

#[derive(Deserialize)]
struct RoadsFile {
    roads: BTreeMap<RoadKind, RoadType>,
    #[serde(default)]
    variants: VariantCosts,
}

/// Extra construction cost of segment variants (GAME_DESIGN §3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariantCosts {
    /// Bus lanes (cents per metre).
    pub bus_lanes_cents_per_m: i64,
    /// Tram tracks in a road (cents per metre).
    pub tram_tracks_cents_per_m: i64,
}

#[derive(Deserialize)]
struct ZonesFile {
    zones: BTreeMap<ZoneKind, ZoneSpec>,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
struct StartMoney {
    relaxed: i64,
    normal: i64,
    hard: i64,
}

#[derive(Deserialize)]
struct EconomyFile {
    start_money_cents: StartMoney,
    growth_debug_delay_ticks: u64,
}

/// The loaded catalog.
#[derive(Resource, Clone, Debug)]
pub struct Catalog {
    roads: BTreeMap<RoadKind, RoadType>,
    variants: VariantCosts,
    zones: BTreeMap<ZoneKind, ZoneSpec>,
    start_money: StartMoney,
    growth_debug_delay_ticks: u64,
}

/// Catalog parse errors.
#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    /// A RON file failed to parse.
    #[error("catalog {file}: {source}")]
    Parse {
        /// File name.
        file: &'static str,
        /// Parser error.
        source: ron::error::SpannedError,
    },
    /// A required entry is missing.
    #[error("catalog {file}: missing {what}")]
    Missing {
        /// File name.
        file: &'static str,
        /// Entry.
        what: &'static str,
    },
}

const ROADS_RON: &str = include_str!("../data/roads.ron");
const ZONES_RON: &str = include_str!("../data/zones.ron");
const ECONOMY_RON: &str = include_str!("../data/economy.ron");

impl Catalog {
    /// Parses the embedded data.
    ///
    /// # Errors
    /// A parse error or a missing road/zone entry.
    pub fn load() -> Result<Self, CatalogError> {
        let roads: RoadsFile = ron::from_str(ROADS_RON).map_err(|source| CatalogError::Parse {
            file: "roads.ron",
            source,
        })?;
        let zones: ZonesFile = ron::from_str(ZONES_RON).map_err(|source| CatalogError::Parse {
            file: "zones.ron",
            source,
        })?;
        let eco: EconomyFile =
            ron::from_str(ECONOMY_RON).map_err(|source| CatalogError::Parse {
                file: "economy.ron",
                source,
            })?;
        for k in RoadKind::ALL {
            if !roads.roads.contains_key(&k) {
                return Err(CatalogError::Missing {
                    file: "roads.ron",
                    what: "road kind",
                });
            }
        }
        for z in [
            ZoneKind::Residential,
            ZoneKind::Commercial,
            ZoneKind::Industrial,
        ] {
            if !zones.zones.contains_key(&z) {
                return Err(CatalogError::Missing {
                    file: "zones.ron",
                    what: "zone",
                });
            }
        }
        Ok(Self {
            roads: roads.roads,
            variants: roads.variants,
            zones: zones.zones,
            start_money: eco.start_money_cents,
            growth_debug_delay_ticks: eco.growth_debug_delay_ticks,
        })
    }

    /// Road type data (every kind is present after [`Catalog::load`]).
    #[must_use]
    pub fn road(&self, k: RoadKind) -> &RoadType {
        // INVARIANT: `load` verified every RoadKind; the fallback is unreachable.
        self.roads
            .get(&k)
            .unwrap_or_else(|| &self.roads[&RoadKind::Street])
    }

    /// Extra cost per metre of a segment variant (cents).
    #[must_use]
    pub fn variant_cost_cents_per_m(&self, v: cb_network::Variant) -> i64 {
        i64::from(v.bus_lanes) * self.variants.bus_lanes_cents_per_m
            + i64::from(v.tram_tracks) * self.variants.tram_tracks_cents_per_m
    }

    /// Zone data (every zone is present after [`Catalog::load`]).
    #[must_use]
    pub fn zone(&self, z: ZoneKind) -> &ZoneSpec {
        // INVARIANT: `load` verified every ZoneKind.
        self.zones
            .get(&z)
            .unwrap_or_else(|| &self.zones[&ZoneKind::Residential])
    }

    /// Starting money for a difficulty.
    #[must_use]
    pub fn start_money(&self, d: Difficulty) -> Money {
        Money(match d {
            Difficulty::Relaxed => self.start_money.relaxed,
            Difficulty::Normal => self.start_money.normal,
            Difficulty::Hard => self.start_money.hard,
        })
    }

    /// Ticks after which an empty lot grows in `GrowthMode::Debug`.
    #[must_use]
    pub fn growth_debug_delay_ticks(&self) -> u64 {
        self.growth_debug_delay_ticks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_parses_and_matches_game_design() {
        let c = Catalog::load().unwrap();
        assert_eq!(c.road(RoadKind::Street).cost_cents_per_m, 2200);
        assert_eq!(c.zone(ZoneKind::Residential).w_target, 13.0);
        assert_eq!(c.start_money(Difficulty::Normal), Money::units(60_000));
    }
}
