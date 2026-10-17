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
    /// Highest building level of the zone.
    pub max_level: u8,
    /// Smallest lot area (m²) for each level (index = level − 1).
    pub level_min_area_m2: [f32; 5],
    /// Households (R) or jobs (C/I) per building.
    pub capacity: CapacitySpec,
}

/// Units a zoned building offers: households for residential, jobs for commercial/industrial.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapacitySpec {
    /// Floor area per unit (m²).
    pub m2_per_unit: f32,
    /// Clamp of the unit count per level (index = level − 1).
    pub range: [(u16, u16); 5],
    /// Share of jobs by required education (0, 1, 2) per level.
    pub job_edu: [[f32; 3]; 5],
}

impl CapacitySpec {
    /// Units for a building of `level` with `floor_area_m2` of floor space.
    #[must_use]
    pub fn units(&self, level: u8, floor_area_m2: f32) -> u16 {
        let (lo, hi) = self.range[usize::from(level.clamp(1, 5)) - 1];
        let n = (floor_area_m2 / self.m2_per_unit.max(1.0)).round();
        // Clamp in f32 first: the value is a small non-negative count.
        n.clamp(f32::from(lo), f32::from(hi)) as u16
    }

    /// Jobs split by required education; the remainder of rounding goes to the most
    /// common requirement so the parts sum to `jobs`.
    #[must_use]
    pub fn split_jobs(&self, level: u8, jobs: u16) -> [u16; 3] {
        let w = self.job_edu[usize::from(level.clamp(1, 5)) - 1];
        let mut out = [0u16; 3];
        for k in 0..3 {
            out[k] = (f32::from(jobs) * w[k]).floor() as u16;
        }
        let rest = jobs.saturating_sub(out.iter().sum());
        let top = (0..3)
            .max_by(|a, b| w[*a].total_cmp(&w[*b]).then(b.cmp(a)))
            .unwrap_or(0);
        out[top] += rest;
        out
    }
}

/// Citizen parameters (GAME_DESIGN §10, ALGORITHMS §16), `data/citizens.ron`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CitizensSpec {
    /// Household size 1–5 weights.
    pub household_size_weights: [f32; 5],
    /// Education weights of arriving adults (none, secondary, higher).
    pub adult_education_weights: [f32; 3],
    /// Probability that the second member is an adult.
    pub second_adult_p: f32,
    /// Probability that a small household is retired.
    pub senior_household_p: f32,
    /// Wealth weights (Low, Mid, High) by home level.
    pub wealth_by_level: [[f32; 3]; 5],
    /// Cars per household by wealth.
    pub cars_by_wealth: [f32; 3],
    /// Immigration base rate (households per game hour at R = 1).
    pub immigration_base_per_game_h: f32,
    /// Population that doubles the immigration rate.
    pub immigration_pop_scale: f32,
    /// Straight-line job search radius (m).
    pub job_search_radius_m: f32,
}

impl ZoneSpec {
    /// Highest level a lot of `area_m2` may hold.
    #[must_use]
    pub fn level_cap(&self, area_m2: f32) -> u8 {
        (1..=self.max_level.clamp(1, 5))
            .rev()
            .find(|l| area_m2 >= self.level_min_area_m2[usize::from(*l) - 1])
            .unwrap_or(1)
    }
}

/// Construction time (GAME_DESIGN §4).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConstructionSpec {
    /// Time for a level-1 building (game hours).
    pub base_game_h: f32,
    /// Extra time per level above 1 (game hours).
    pub per_level_game_h: f32,
}

impl ConstructionSpec {
    /// Construction duration of a level (game hours).
    #[must_use]
    pub fn duration_game_h(&self, level: u8) -> f32 {
        self.base_game_h + self.per_level_game_h * f32::from(level.saturating_sub(1))
    }
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
    #[serde(default)]
    bridge: BridgeCosts,
    #[serde(default)]
    decor: DecorCosts,
}

/// Surface and decoration costs (GAME_DESIGN §3, cents).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecorCosts {
    /// Light asphalt, extra per metre.
    pub light_asphalt_cents_per_m: i64,
    /// Cobblestone, extra per metre.
    pub cobblestone_cents_per_m: i64,
    /// Grass strip per side and metre.
    pub grass_strip_cents_per_m: i64,
    /// Per street tree.
    pub tree_cents: i64,
    /// Per street lamp.
    pub lamp_cents: i64,
}

/// Bridge costs (GAME_DESIGN §3): deck length × `cost_factor` of the road price, plus per pier
/// a base price and a price per metre of height.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BridgeCosts {
    /// Deck price as a multiple of the road price per metre.
    pub cost_factor: f64,
    /// Price per pier (cents).
    pub pier_cents: i64,
    /// Price per metre of pier height (cents).
    pub pier_cents_per_m: i64,
}

impl Default for BridgeCosts {
    fn default() -> Self {
        Self {
            cost_factor: 4.0,
            pier_cents: 120_000,
            pier_cents_per_m: 4_000,
        }
    }
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
    construction: ConstructionSpec,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
struct StartMoney {
    relaxed: i64,
    normal: i64,
    hard: i64,
}

/// Price and running cost of a civic building (GAME_DESIGN §5–6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CivicSpec {
    /// Construction cost (cents).
    pub cost_cents: i64,
    /// Upkeep per game day (cents).
    pub upkeep_cents_per_day: i64,
}

/// Civic building specs by kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
struct CivicFile {
    police_station: CivicSpec,
    fire_station: CivicSpec,
    clinic: CivicSpec,
    elementary_school: CivicSpec,
    town_hall: CivicSpec,
    diesel_generator: CivicSpec,
    coal_plant: CivicSpec,
    water_pump: CivicSpec,
    sewage_outlet: CivicSpec,
    landfill: CivicSpec,
}

#[derive(Deserialize)]
struct EconomyFile {
    start_money_cents: StartMoney,
    growth_debug_delay_ticks: u64,
    tax_default_bp: u16,
    household_income_cents_per_day: [i64; 5],
    wealth_income_factor: [f32; 3],
    wages_cents_per_day: Wages,
    goods: GoodsSpec,
    loans: [LoanTier; 3],
    bankruptcy_days: u32,
}

/// Wages per job and game day by level (cents).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wages {
    /// Commercial L1–L3.
    pub commercial: [i64; 3],
    /// Industrial L1–L3.
    pub industrial: [i64; 3],
}

/// Goods economy parameters (ALGORITHMS §27).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GoodsSpec {
    /// Price of one unit between businesses (cents).
    pub unit_price_cents: i64,
    /// Export price relative to the unit price.
    pub export_factor: f32,
    /// Import cost relative to the unit price.
    pub import_factor: f32,
    /// Commercial revenue per customer unit (cents).
    pub revenue_per_customer_cents: i64,
    /// Units per delivery truck.
    pub units_per_delivery: u32,
    /// Stock capacity per building level (units).
    pub stock_units_per_level: u32,
    /// Macro truck speed until routing (m/s).
    pub macro_speed_mps: f32,
}

/// One loan tier.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoanTier {
    /// Principal (cents).
    pub principal_cents: i64,
    /// Simple interest per game day.
    pub rate_per_day: f32,
}

/// Economy parameters (GAME_DESIGN §9).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EconomySpec {
    /// Default tax rate (basis points).
    pub tax_default_bp: u16,
    /// Household income per day by home level (cents).
    pub household_income_cents_per_day: [i64; 5],
    /// Income factor by wealth.
    pub wealth_income_factor: [f32; 3],
    /// Wages.
    pub wages: Wages,
    /// Goods.
    pub goods: GoodsSpec,
    /// Loan tiers.
    pub loans: [LoanTier; 3],
    /// Days below zero before the emergency loan or bankruptcy.
    pub bankruptcy_days: u32,
}

/// The loaded catalog.
#[derive(Resource, Clone, Debug)]
pub struct Catalog {
    roads: BTreeMap<RoadKind, RoadType>,
    variants: VariantCosts,
    bridge: BridgeCosts,
    decor: DecorCosts,
    zones: BTreeMap<ZoneKind, ZoneSpec>,
    construction: ConstructionSpec,
    civic: CivicFile,
    start_money: StartMoney,
    growth_debug_delay_ticks: u64,
    citizens: CitizensSpec,
    economy: EconomySpec,
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
const CIVIC_RON: &str = include_str!("../data/civic.ron");
const CITIZENS_RON: &str = include_str!("../data/citizens.ron");

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
        let civic: CivicFile = ron::from_str(CIVIC_RON).map_err(|source| CatalogError::Parse {
            file: "civic.ron",
            source,
        })?;
        let citizens: CitizensSpec =
            ron::from_str(CITIZENS_RON).map_err(|source| CatalogError::Parse {
                file: "citizens.ron",
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
            bridge: roads.bridge,
            decor: roads.decor,
            zones: zones.zones,
            construction: zones.construction,
            civic,
            start_money: eco.start_money_cents,
            growth_debug_delay_ticks: eco.growth_debug_delay_ticks,
            citizens,
            economy: EconomySpec {
                tax_default_bp: eco.tax_default_bp,
                household_income_cents_per_day: eco.household_income_cents_per_day,
                wealth_income_factor: eco.wealth_income_factor,
                wages: eco.wages_cents_per_day,
                goods: eco.goods,
                loans: eco.loans,
                bankruptcy_days: eco.bankruptcy_days,
            },
        })
    }

    /// Citizen parameters.
    #[must_use]
    pub fn citizens(&self) -> &CitizensSpec {
        &self.citizens
    }

    /// Economy parameters.
    #[must_use]
    pub fn economy(&self) -> &EconomySpec {
        &self.economy
    }

    /// Road type data (every kind is present after [`Catalog::load`]).
    #[must_use]
    pub fn road(&self, k: RoadKind) -> &RoadType {
        // INVARIANT: `load` verified every RoadKind; the fallback is unreachable.
        self.roads
            .get(&k)
            .unwrap_or_else(|| &self.roads[&RoadKind::Street])
    }

    /// Surface and decoration costs.
    #[must_use]
    pub fn decor_costs(&self) -> DecorCosts {
        self.decor
    }

    /// Bridge costs.
    #[must_use]
    pub fn bridge_costs(&self) -> BridgeCosts {
        self.bridge
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

    /// Price and upkeep of a civic building.
    #[must_use]
    pub fn civic(&self, k: cb_procgen::building::civic::CivicKind) -> CivicSpec {
        use cb_procgen::building::civic::CivicKind as K;
        let c = &self.civic;
        match k {
            K::PoliceStation => c.police_station,
            K::FireStation => c.fire_station,
            K::Clinic => c.clinic,
            K::ElementarySchool => c.elementary_school,
            K::TownHall => c.town_hall,
            K::DieselGenerator => c.diesel_generator,
            K::CoalPlant => c.coal_plant,
            K::WaterPump => c.water_pump,
            K::SewageOutlet => c.sewage_outlet,
            K::Landfill => c.landfill,
        }
    }

    /// Construction times.
    #[must_use]
    pub fn construction(&self) -> ConstructionSpec {
        self.construction
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
        // Level caps by lot area (GAME_DESIGN §4).
        let r = c.zone(ZoneKind::Residential);
        assert_eq!(
            (r.level_cap(399.0), r.level_cap(400.0), r.level_cap(700.0)),
            (3, 4, 5)
        );
        let com = c.zone(ZoneKind::Commercial);
        assert_eq!((com.level_cap(599.0), com.level_cap(600.0)), (2, 3));
        let i = c.zone(ZoneKind::Industrial);
        assert_eq!((i.level_cap(1999.0), i.level_cap(2000.0)), (2, 3));
        use cb_procgen::building::civic::CivicKind;
        assert_eq!(c.civic(CivicKind::PoliceStation).cost_cents, 900_000);
        assert_eq!(c.civic(CivicKind::TownHall).cost_cents, 0);
    }
}
