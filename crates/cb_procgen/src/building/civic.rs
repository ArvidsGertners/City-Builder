//! Civic buildings, generator set A (ALGORITHMS §11, ART_DIRECTION §5 "silhouettes carry
//! identity first"): police station, fire station, clinic, elementary school, town hall, diesel
//! generator, coal power plant, water pump, sewage outlet and landfill.
//!
//! Each kind has a fixed parcel (`CivicKind::parcel_m`, width along the road × depth) laid out
//! in a parcel frame: origin at the front-left corner on the road edge, `ex` along the road,
//! `ey` into the parcel. The seed mirrors the layout along the road and jitters colours, so two
//! stations of a kind differ but keep their signature silhouette. Volumes are [`Mass`]es (LOD1
//! is the masses alone, which the silhouette test compares); LOD0 adds windows, doors, signs,
//! ground surfaces, fences and parked vehicles.

use cb_core::rng::Pcg32;
use cb_geometry::MeshData;
use glam::{Vec2, Vec3};

use super::foundation::{self, FLOOR_ABOVE_PAD_M, Foundation, FoundationKind};
use super::roofs::{self, RoofKind, RoofSpec};
use crate::paint::{Frame, Painter};
use crate::palette::{self, Rgb, hex};
use crate::surface::Surface;

/// Civic building kinds of generator set A (set B lands with its systems, M6/M10).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum CivicKind {
    /// Police station.
    PoliceStation,
    /// Fire station.
    FireStation,
    /// Health clinic.
    Clinic,
    /// Elementary school.
    ElementarySchool,
    /// Town hall (auto-placed at start).
    TownHall,
    /// Diesel generator.
    DieselGenerator,
    /// Coal power plant.
    CoalPlant,
    /// Water pump (shoreline).
    WaterPump,
    /// Sewage outlet (shoreline).
    SewageOutlet,
    /// Landfill.
    Landfill,
}

impl CivicKind {
    /// Every kind, in a stable order.
    pub const ALL: [Self; 10] = [
        Self::PoliceStation,
        Self::FireStation,
        Self::Clinic,
        Self::ElementarySchool,
        Self::TownHall,
        Self::DieselGenerator,
        Self::CoalPlant,
        Self::WaterPump,
        Self::SewageOutlet,
        Self::Landfill,
    ];

    /// Parcel size `(width along the road, depth)` in metres.
    #[must_use]
    pub const fn parcel_m(self) -> (f32, f32) {
        match self {
            Self::PoliceStation => (26.0, 24.0),
            Self::FireStation => (30.0, 28.0),
            Self::Clinic => (24.0, 22.0),
            Self::ElementarySchool => (48.0, 40.0),
            Self::TownHall => (32.0, 30.0),
            Self::DieselGenerator => (18.0, 14.0),
            Self::CoalPlant => (90.0, 72.0),
            Self::WaterPump => (16.0, 14.0),
            Self::SewageOutlet => (12.0, 12.0),
            Self::Landfill => (64.0, 56.0),
        }
    }

    /// How far a pipe reaches past the back of the parcel (shoreline kinds), m.
    #[must_use]
    pub const fn pipe_reach_m(self) -> f32 {
        match self {
            Self::WaterPump => 12.0,
            Self::SewageOutlet => 10.0,
            _ => 0.0,
        }
    }

    /// LOD0 triangle budget.
    #[must_use]
    pub const fn tri_budget(self) -> usize {
        match self {
            Self::CoalPlant | Self::ElementarySchool | Self::TownHall => 6000,
            _ => 3500,
        }
    }
}

/// Roof of a block mass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Top {
    /// Flat roof, optional parapet.
    Flat {
        /// Parapet height (m), 0 for none.
        parapet: f32,
    },
    /// Hip roof (pyramid on squares) with `tan(pitch)`.
    Hip(f32),
    /// Gable roof: ridge along `u` or `v`, `tan(pitch)`.
    Gable {
        /// Ridge along the parcel's `u` axis.
        ridge_u: bool,
        /// tan(pitch).
        slope: f32,
    },
}

/// Cap of a round mass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cap {
    /// Flat lid.
    Flat,
    /// Cone of this height.
    Cone(f32),
    /// Open top with a dark rim (chimneys, cooling towers).
    Open,
}

/// One volume of a civic building, in parcel coordinates `(u, v)`; heights above the floor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mass {
    /// Box over `[u0, v0, u1, v1]` from `y0` up `h` m.
    Block {
        /// Rectangle.
        r: [f32; 4],
        /// Base height.
        y0: f32,
        /// Wall height.
        h: f32,
        /// Wall colour.
        wall: Rgb,
        /// Roof form.
        top: Top,
        /// Roof colour.
        roof: Rgb,
    },
    /// Round structure (tank, chimney, cooling tower section): radius `r0` at the base, `r1`
    /// at the top.
    Round {
        /// Centre.
        c: Vec2,
        /// Base radius.
        r0: f32,
        /// Top radius.
        r1: f32,
        /// Base height.
        y0: f32,
        /// Height.
        h: f32,
        /// Colour.
        col: Rgb,
        /// Top.
        cap: Cap,
    },
    /// Terraced earth mound over a rectangle (landfill, coal pile).
    Mound {
        /// Base rectangle.
        r: [f32; 4],
        /// Height.
        h: f32,
        /// Terraces.
        steps: u8,
        /// Colour.
        col: Rgb,
    },
    /// Thin square mast `w` wide.
    Mast {
        /// Foot.
        c: Vec2,
        /// Base height.
        y0: f32,
        /// Height.
        h: f32,
        /// Width.
        w: f32,
        /// Colour.
        col: Rgb,
    },
}

/// Layout of one civic building.
#[derive(Clone, Debug, PartialEq)]
pub struct CivicLayout {
    /// Kind.
    pub kind: CivicKind,
    /// Parcel frame (mirrored along the road for odd seeds; faces orient from outward hints).
    pub frame: Frame,
    /// Volumes.
    pub masses: Vec<Mass>,
    /// Body colour (signature table, jittered).
    pub body: Rgb,
    /// Signature accent colour.
    pub accent: Rgb,
    /// Mirrored along the road.
    pub mirrored: bool,
    /// Seed for details.
    pub seed: u64,
}

/// Parcel frame for a building whose front edge is centred on `front_mid` and runs along
/// `along` (the parcel lies to the left of `along`, i.e. along `along` rotated +90°).
#[must_use]
pub fn frame_at(kind: CivicKind, front_mid: Vec2, along: Vec2) -> Frame {
    let ex = along.normalize_or(Vec2::X);
    let ey = ex.perp();
    let (w, _) = kind.parcel_m();
    Frame {
        o: front_mid - ex * (0.5 * w),
        ex,
        ey,
    }
}

const POLICE_BODY: Rgb = hex(0xDADDE2);
const POLICE_BAND: Rgb = hex(0x5872A3);
const FIRE_BODY: Rgb = hex(0xB9554A);
const FIRE_DOORS: Rgb = hex(0xD9C9A8);
const HEALTH_BODY: Rgb = hex(0xF2F0EB);
const HEALTH_CROSS: Rgb = hex(0x4FA38F);
const SCHOOL_BODY: Rgb = hex(0xD3A65B);
const SCHOOL_ROOF: Rgb = hex(0x7D5A44);
const POWER_BODY: Rgb = hex(0x6B6F76);
const POWER_STRIPES: Rgb = hex(0xE2B24A);
const WATER_BODY: Rgb = hex(0x6FA8C7);
const WATER_TANK: Rgb = hex(0xDCE3E8);
const SEWAGE_BODY: Rgb = hex(0x8B9A6B);
const SEWAGE_BASIN: Rgb = hex(0x6E8C8C);
const GARBAGE_BODY: Rgb = hex(0x7E8B5A);
const GARBAGE_ACCENT: Rgb = hex(0xA9A69C);
const HALL_STONE: Rgb = hex(0xE8DCC4);
const HALL_COPPER: Rgb = hex(0x4F6A7A);
const FLAT_ROOF: Rgb = hex(0x7C8288);
const COAL: Rgb = hex(0x2F3236);
const GRAVEL: Rgb = hex(0xA39C8E);
const CONCRETE: Rgb = hex(0xB8B3A8);

const POLICE_MAIN: [f32; 4] = [4.0, 6.0, 13.0, 16.0];
const POLICE_WING: [f32; 4] = [13.0, 6.0, 23.0, 13.0];
const FIRE_BAY: [f32; 4] = [2.0, 9.0, 20.0, 22.0];
const FIRE_OFFICE: [f32; 4] = [20.0, 9.0, 28.0, 16.0];
const FIRE_TOWER: [f32; 4] = [22.5, 17.0, 27.5, 22.0];
const CLINIC_PAVILION: [f32; 4] = [4.0, 10.0, 20.0, 16.0];
const CLINIC_PAVILION_H: f32 = 3.8;
const CLINIC_WARDS: [f32; 4] = [9.0, 11.0, 15.0, 20.0];
const SCHOOL_MAIN: [f32; 4] = [12.0, 6.0, 38.0, 15.0];
const SCHOOL_WING: [f32; 4] = [29.0, 15.0, 38.0, 24.0];
const SCHOOL_GYM: [f32; 4] = [4.0, 24.0, 18.0, 36.0];
const HALL_MAIN: [f32; 4] = [3.0, 12.0, 29.0, 19.0];
const HALL_WING_L: [f32; 4] = [3.0, 7.0, 8.0, 12.0];
const HALL_WING_R: [f32; 4] = [24.0, 7.0, 29.0, 12.0];
const HALL_TOWER: [f32; 4] = [13.0, 6.0, 19.0, 12.0];
const PUMP_HOUSE: [f32; 4] = [2.5, 4.0, 9.5, 9.5];

fn block(r: [f32; 4], h: f32, wall: Rgb, top: Top, roof: Rgb) -> Mass {
    Mass::Block {
        r,
        y0: 0.0,
        h,
        wall,
        top,
        roof,
    }
}

const FLAT: Top = Top::Flat { parapet: 0.5 };

/// Plans a civic building of `kind` in the parcel `frame`.
#[must_use]
pub fn layout(kind: CivicKind, frame: Frame, seed: u64) -> CivicLayout {
    let mut rng = Pcg32::from_seed(seed);
    let mirrored = rng.chance(0.5);
    let (w, _) = kind.parcel_m();
    let frame = if mirrored {
        Frame {
            o: frame.at(w, 0.0),
            ex: -frame.ex,
            ey: frame.ey,
        }
    } else {
        frame
    };
    let (body, accent) = match kind {
        CivicKind::PoliceStation => (POLICE_BODY, POLICE_BAND),
        CivicKind::FireStation => (FIRE_BODY, FIRE_DOORS),
        CivicKind::Clinic => (HEALTH_BODY, HEALTH_CROSS),
        CivicKind::ElementarySchool => (SCHOOL_BODY, SCHOOL_ROOF),
        CivicKind::TownHall => (HALL_STONE, HALL_COPPER),
        CivicKind::DieselGenerator | CivicKind::CoalPlant => (POWER_BODY, POWER_STRIPES),
        CivicKind::WaterPump => (WATER_BODY, WATER_TANK),
        CivicKind::SewageOutlet => (SEWAGE_BODY, SEWAGE_BASIN),
        CivicKind::Landfill => (GARBAGE_BODY, GARBAGE_ACCENT),
    };
    let body = body.building_variant(&mut rng);
    let masses = match kind {
        CivicKind::PoliceStation => vec![
            // Slim four-storey office block, low garage wing, tall radio mast.
            block(POLICE_MAIN, 14.0, body, FLAT, FLAT_ROOF),
            block(POLICE_WING, 3.8, body.scale(0.95), FLAT, FLAT_ROOF),
            Mass::Mast {
                c: Vec2::new(11.0, 13.5),
                y0: 14.0,
                h: 16.0,
                w: 0.45,
                col: hex(0x8A9099),
            },
        ],
        CivicKind::FireStation => vec![
            // Low flat apparatus bay behind a deep apron, office wing, tall hose tower.
            block(FIRE_BAY, 5.6, body, FLAT, FLAT_ROOF),
            block(FIRE_OFFICE, 7.0, body.scale(0.93), FLAT, FLAT_ROOF),
            block(FIRE_TOWER, 21.0, body, Top::Hip(1.3), hex(0x5E6670)),
        ],
        CivicKind::Clinic => vec![
            // Single-storey pavilion with a slim three-storey ward block rising from it; the
            // cantilevered canopy and the cross sign are part of the silhouette.
            block(CLINIC_PAVILION, CLINIC_PAVILION_H, body, FLAT, FLAT_ROOF),
            block(CLINIC_WARDS, 14.2, body.scale(0.97), FLAT, FLAT_ROOF),
            Mass::Block {
                r: [7.5, 4.5, 16.5, 10.0],
                y0: 3.2,
                h: 0.35,
                wall: body,
                top: Top::Flat { parapet: 0.0 },
                roof: body.scale(0.9),
            },
        ],
        CivicKind::ElementarySchool => vec![
            // L of hipped wings (three-storey main, two-storey side), low gym hall behind, cupola.
            block(SCHOOL_MAIN, 10.8, body, Top::Hip(0.75), accent),
            block(SCHOOL_WING, 7.2, body, Top::Hip(0.75), accent),
            block(
                SCHOOL_GYM,
                6.4,
                body.scale(0.94),
                Top::Gable {
                    ridge_u: true,
                    slope: 0.18,
                },
                hex(0x6F716C),
            ),
            Mass::Block {
                r: [24.0, 9.5, 26.0, 11.5],
                y0: 10.8 + 4.5 * 0.75,
                h: 2.2,
                wall: palette::WINDOW_FRAME,
                top: Top::Hip(1.4),
                roof: accent,
            },
        ],
        CivicKind::TownHall => vec![
            // U plan under hipped copper roofs: a shallow main range and two short forward
            // wings framing the plaza; a tall clock tower with a spire on the axis.
            block(HALL_MAIN, 8.4, body, Top::Hip(0.8), accent),
            block(HALL_WING_L, 6.6, body, Top::Hip(0.8), accent),
            block(HALL_WING_R, 6.6, body, Top::Hip(0.8), accent),
            block(HALL_TOWER, 27.0, body, Top::Hip(2.6), accent),
        ],
        CivicKind::DieselGenerator => vec![
            block(
                [3.0, 4.0, 13.0, 10.0],
                3.6,
                body,
                Top::Flat { parapet: 0.0 },
                FLAT_ROOF,
            ),
            Mass::Round {
                c: Vec2::new(5.0, 11.2),
                r0: 0.4,
                r1: 0.35,
                y0: 0.0,
                h: 9.0,
                col: hex(0x5D6266),
                cap: Cap::Open,
            },
            Mass::Round {
                c: Vec2::new(8.0, 11.2),
                r0: 0.4,
                r1: 0.35,
                y0: 0.0,
                h: 9.0,
                col: hex(0x5D6266),
                cap: Cap::Open,
            },
        ],
        CivicKind::CoalPlant => vec![
            // Boiler house, turbine hall, tapered chimney, two cooling towers, coal pile.
            block([30.0, 18.0, 52.0, 44.0], 32.0, body, FLAT, FLAT_ROOF),
            block(
                [8.0, 22.0, 30.0, 40.0],
                15.0,
                body.scale(1.08),
                Top::Gable {
                    ridge_u: true,
                    slope: 0.2,
                },
                FLAT_ROOF,
            ),
            Mass::Round {
                c: Vec2::new(58.0, 50.0),
                r0: 3.4,
                r1: 2.2,
                y0: 0.0,
                h: 76.0,
                col: hex(0xA9A69C),
                cap: Cap::Open,
            },
            Mass::Round {
                c: Vec2::new(76.0, 20.0),
                r0: 11.0,
                r1: 7.4,
                y0: 0.0,
                h: 24.0,
                col: hex(0xC9C6BE),
                cap: Cap::Flat,
            },
            Mass::Round {
                c: Vec2::new(76.0, 20.0),
                r0: 7.4,
                r1: 8.4,
                y0: 24.0,
                h: 10.0,
                col: hex(0xC9C6BE),
                cap: Cap::Open,
            },
            Mass::Round {
                c: Vec2::new(76.0, 46.0),
                r0: 11.0,
                r1: 7.4,
                y0: 0.0,
                h: 24.0,
                col: hex(0xC9C6BE),
                cap: Cap::Flat,
            },
            Mass::Round {
                c: Vec2::new(76.0, 46.0),
                r0: 7.4,
                r1: 8.4,
                y0: 24.0,
                h: 10.0,
                col: hex(0xC9C6BE),
                cap: Cap::Open,
            },
            Mass::Mound {
                r: [8.0, 48.0, 28.0, 68.0],
                h: 6.0,
                steps: 2,
                col: COAL,
            },
        ],
        CivicKind::WaterPump => vec![
            block(
                PUMP_HOUSE,
                3.2,
                body,
                Top::Gable {
                    ridge_u: true,
                    slope: 0.45,
                },
                hex(0x5E6670),
            ),
            Mass::Round {
                c: Vec2::new(12.6, 9.0),
                r0: 2.2,
                r1: 2.2,
                y0: 0.0,
                h: 11.0,
                col: accent,
                cap: Cap::Cone(1.2),
            },
        ],
        CivicKind::SewageOutlet => vec![
            block(
                [1.5, 2.5, 5.5, 6.0],
                3.0,
                body,
                Top::Flat { parapet: 0.0 },
                FLAT_ROOF,
            ),
            Mass::Round {
                c: Vec2::new(7.8, 7.6),
                r0: 3.2,
                r1: 3.2,
                y0: 0.0,
                h: 1.4,
                col: CONCRETE,
                cap: Cap::Open,
            },
        ],
        CivicKind::Landfill => vec![
            Mass::Mound {
                r: [8.0, 14.0, 58.0, 52.0],
                h: 9.0,
                steps: 3,
                col: body,
            },
            block(
                [3.0, 3.0, 9.0, 7.0],
                3.0,
                accent,
                Top::Flat { parapet: 0.0 },
                FLAT_ROOF,
            ),
        ],
    };
    CivicLayout {
        kind,
        frame,
        masses,
        body,
        accent,
        mirrored,
        seed: rng.next_u64(),
    }
}

/// Polygon sides of round masses at LOD0 / LOD1.
const ROUND_SIDES: usize = 12;
const ROUND_SIDES_LOD1: usize = 6;

fn ring(c: Vec2, r: f32, n: usize) -> Vec<Vec2> {
    (0..n)
        .map(|k| {
            let a = k as f32 * std::f32::consts::TAU / n as f32;
            let (s, co) = cb_core::math::sin_cos(a);
            c + Vec2::new(co, s) * r
        })
        .collect()
}

fn roof_spec(top: Top) -> Option<RoofSpec> {
    let (kind, slope) = match top {
        Top::Flat { .. } => return None,
        Top::Hip(s) => (RoofKind::Hip, s),
        Top::Gable { ridge_u, slope } => (RoofKind::Gable { ridge_u }, slope),
    };
    Some(RoofSpec {
        kind,
        slope,
        eave_m: 0.4,
        verge_m: 0.3,
        thickness_m: 0.16,
    })
}

impl Mass {
    /// Top height above the floor (roofs and caps included).
    #[must_use]
    pub fn top_m(&self) -> f32 {
        match *self {
            Self::Block { r, y0, h, top, .. } => {
                y0 + h + roof_spec(top).map_or(0.0, |s| roofs::rise(&s, r))
            }
            Self::Round { y0, h, cap, .. } => {
                y0 + h
                    + match cap {
                        Cap::Cone(c) => c,
                        _ => 0.0,
                    }
            }
            Self::Mound { h, .. } => h,
            Self::Mast { y0, h, .. } => y0 + h,
        }
    }

    /// Plan outline in parcel coordinates (`None` for masts and canopies above the ground).
    fn plan(&self) -> Option<Vec<Vec2>> {
        match *self {
            Self::Block { r, y0, .. } if y0 < 0.5 => Some(vec![
                Vec2::new(r[0], r[1]),
                Vec2::new(r[2], r[1]),
                Vec2::new(r[2], r[3]),
                Vec2::new(r[0], r[3]),
            ]),
            Self::Round { c, r0, y0, .. } if y0 < 0.5 => Some(ring(c, r0, ROUND_SIDES_LOD1)),
            Self::Mound { r, .. } => Some(vec![
                Vec2::new(r[0], r[1]),
                Vec2::new(r[2], r[1]),
                Vec2::new(r[2], r[3]),
                Vec2::new(r[0], r[3]),
            ]),
            _ => None,
        }
    }
}

impl CivicLayout {
    /// Parcel quad in world space (counter-clockwise).
    #[must_use]
    pub fn parcel(&self) -> Vec<Vec2> {
        let (w, d) = self.kind.parcel_m();
        cb_geometry::polygon::to_ccw(&self.frame.rect([0.0, 0.0, w, d]))
    }

    /// World outlines of the ground-standing volumes (picking, overlap), counter-clockwise.
    #[must_use]
    pub fn outlines(&self) -> Vec<Vec<Vec2>> {
        self.masses
            .iter()
            .filter_map(Mass::plan)
            .map(|q| {
                let w: Vec<Vec2> = q.iter().map(|p| self.frame.at(p.x, p.y)).collect();
                cb_geometry::polygon::to_ccw(&w)
            })
            .collect()
    }

    /// Height above the floor level (m).
    #[must_use]
    pub fn height_m(&self) -> f32 {
        self.masses.iter().map(Mass::top_m).fold(0.0, f32::max)
    }

    /// Foundation: the whole parcel is levelled (pad, or a terrace on a plinth); no stilts.
    #[must_use]
    pub fn plan_foundation(&self, ground: &dyn Fn(Vec2) -> f32) -> Option<Foundation> {
        let (w, d) = self.kind.parcel_m();
        let mut pts = Vec::with_capacity(9);
        for i in 0..3 {
            for j in 0..3 {
                pts.push(self.frame.at(w * i as f32 / 2.0, d * j as f32 / 2.0));
            }
        }
        let samples: Vec<f32> = pts.iter().map(|p| ground(*p)).collect();
        // Large parcels level a terrace: the plinth limit grows with the parcel (4 % grade).
        let max_dh = (0.04 * w.max(d)).max(foundation::PLINTH_MAX_DH_M);
        foundation::plan_with(&self.parcel(), &samples, false, max_dh)
    }
}

/// Mesh context: painter, frame, floor height.
struct Ctx<'a> {
    p: &'a mut Painter,
    fr: Frame,
    fy: f32,
}

impl Ctx<'_> {
    fn bx(&mut self, r: [f32; 4], y0: f32, y1: f32, c: Rgb) {
        let (fr, fy) = (self.fr, self.fy);
        self.p
            .frame_box(&fr, r, fy + y0, fy + y1, c, Some(c.scale(1.05)));
    }

    /// Flat ground cover at `lift` m above the pad.
    fn flat(&mut self, r: [f32; 4], lift: f32, c: Rgb, kind: Surface) {
        let y = self.fy - FLOOR_ABOVE_PAD_M + lift;
        let q = self.fr.rect(r).map(|p| Vec3::new(p.x, y, p.y));
        self.p.face_ao(&q, &[1.0; 4], c, kind, 0, Vec3::Y);
    }

    /// Thin quad on the wall plane `v = v_wall` (front `out = -1` or back `+1`), from `u0` to
    /// `u1` and `y0` to `y1` above the floor.
    #[allow(clippy::too_many_arguments)]
    fn panel_v(
        &mut self,
        v_wall: f32,
        out: f32,
        u: [f32; 2],
        y: [f32; 2],
        c: Rgb,
        kind: Surface,
        id: u32,
    ) {
        let off = 0.04 * out;
        let fy = self.fy;
        let q = [
            self.fr.p3(u[0], v_wall + off, fy + y[0]),
            self.fr.p3(u[1], v_wall + off, fy + y[0]),
            self.fr.p3(u[1], v_wall + off, fy + y[1]),
            self.fr.p3(u[0], v_wall + off, fy + y[1]),
        ];
        let n = self.fr.d3(0.0, out, 0.0);
        self.p.face_ao(&q, &[1.0; 4], c, kind, id, n);
    }

    /// Thin quad on the wall plane `u = u_wall` (left `out = -1` or right `+1`).
    #[allow(clippy::too_many_arguments)]
    fn panel_u(
        &mut self,
        u_wall: f32,
        out: f32,
        v: [f32; 2],
        y: [f32; 2],
        c: Rgb,
        kind: Surface,
        id: u32,
    ) {
        let off = 0.04 * out;
        let fy = self.fy;
        let q = [
            self.fr.p3(u_wall + off, v[0], fy + y[0]),
            self.fr.p3(u_wall + off, v[1], fy + y[0]),
            self.fr.p3(u_wall + off, v[1], fy + y[1]),
            self.fr.p3(u_wall + off, v[0], fy + y[1]),
        ];
        let n = self.fr.d3(out, 0.0, 0.0);
        self.p.face_ao(&q, &[1.0; 4], c, kind, id, n);
    }

    /// Window rows on all four walls of `r`: `floors` rows of `floor_h`, bays of about `bay`
    /// m, window `ww × wh` with the sill at `sill`; walls listed in `skip` (0 front, 1 right,
    /// 2 back, 3 left) stay blank. Windows never cross a wall end.
    #[allow(clippy::too_many_arguments)]
    fn windows(
        &mut self,
        r: [f32; 4],
        floors: u32,
        floor_h: f32,
        bay: f32,
        size: (f32, f32),
        sill: f32,
        skip: &[u8],
        id: &mut u32,
    ) {
        let (ww, wh) = size;
        for wall in 0..4u8 {
            if skip.contains(&wall) {
                continue;
            }
            let len = if wall % 2 == 0 {
                r[2] - r[0]
            } else {
                r[3] - r[1]
            };
            let n = ((len - 0.8) / bay).floor() as i32;
            if n < 1 {
                continue;
            }
            let step = (len - 0.8) / n as f32;
            for f in 0..floors {
                let y0 = f as f32 * floor_h + sill;
                for k in 0..n {
                    let s = 0.4 + step * (k as f32 + 0.5);
                    let (a, b) = (s - 0.5 * ww, s + 0.5 * ww);
                    *id += 1;
                    let c = palette::GLASS;
                    let fc = palette::WINDOW_FRAME;
                    match wall {
                        0 => {
                            self.panel_v(
                                r[1],
                                -1.0,
                                [r[0] + a - 0.1, r[0] + b + 0.1],
                                [y0 - 0.1, y0 + wh + 0.1],
                                fc,
                                Surface::Solid,
                                0,
                            );
                            self.panel_v(
                                r[1] - 0.02,
                                -1.0,
                                [r[0] + a, r[0] + b],
                                [y0, y0 + wh],
                                c,
                                Surface::Window,
                                *id,
                            );
                        }
                        2 => {
                            self.panel_v(
                                r[3],
                                1.0,
                                [r[0] + a - 0.1, r[0] + b + 0.1],
                                [y0 - 0.1, y0 + wh + 0.1],
                                fc,
                                Surface::Solid,
                                0,
                            );
                            self.panel_v(
                                r[3] + 0.02,
                                1.0,
                                [r[0] + a, r[0] + b],
                                [y0, y0 + wh],
                                c,
                                Surface::Window,
                                *id,
                            );
                        }
                        1 => {
                            self.panel_u(
                                r[2],
                                1.0,
                                [r[1] + a - 0.1, r[1] + b + 0.1],
                                [y0 - 0.1, y0 + wh + 0.1],
                                fc,
                                Surface::Solid,
                                0,
                            );
                            self.panel_u(
                                r[2] + 0.02,
                                1.0,
                                [r[1] + a, r[1] + b],
                                [y0, y0 + wh],
                                c,
                                Surface::Window,
                                *id,
                            );
                        }
                        _ => {
                            self.panel_u(
                                r[0],
                                -1.0,
                                [r[1] + a - 0.1, r[1] + b + 0.1],
                                [y0 - 0.1, y0 + wh + 0.1],
                                fc,
                                Surface::Solid,
                                0,
                            );
                            self.panel_u(
                                r[0] - 0.02,
                                -1.0,
                                [r[1] + a, r[1] + b],
                                [y0, y0 + wh],
                                c,
                                Surface::Window,
                                *id,
                            );
                        }
                    }
                }
            }
        }
    }

    /// Band of colour `c` around the walls of `r` between `y0` and `y1`.
    fn band(&mut self, r: [f32; 4], y0: f32, y1: f32, c: Rgb) {
        self.panel_v(r[1], -1.0, [r[0], r[2]], [y0, y1], c, Surface::Solid, 0);
        self.panel_v(r[3], 1.0, [r[0], r[2]], [y0, y1], c, Surface::Solid, 0);
        self.panel_u(r[0], -1.0, [r[1], r[3]], [y0, y1], c, Surface::Solid, 0);
        self.panel_u(r[2], 1.0, [r[1], r[3]], [y0, y1], c, Surface::Solid, 0);
    }

    /// Front door with a frame on the wall `v = v_wall` (facing the street).
    fn door(&mut self, v_wall: f32, u: f32, w: f32, h: f32, c: Rgb) {
        self.panel_v(
            v_wall,
            -1.0,
            [u - 0.5 * w - 0.12, u + 0.5 * w + 0.12],
            [0.0, h + 0.12],
            palette::WINDOW_FRAME,
            Surface::Solid,
            0,
        );
        self.panel_v(
            v_wall - 0.02,
            -1.0,
            [u - 0.5 * w, u + 0.5 * w],
            [0.0, h],
            c,
            Surface::Solid,
            0,
        );
    }

    /// Parked vehicle along `u` (`along_u`) or `v`, centred on `(u, v)`.
    #[allow(clippy::too_many_arguments)]
    fn vehicle(
        &mut self,
        u: f32,
        v: f32,
        len: f32,
        wid: f32,
        h: f32,
        body: Rgb,
        top: Rgb,
        along_u: bool,
    ) {
        let (hl, hw) = (0.5 * len, 0.5 * wid);
        let r = |a: f32, b: f32| {
            if along_u {
                [u - a, v - b, u + a, v + b]
            } else {
                [u - b, v - a, u + b, v + a]
            }
        };
        let base = -FLOOR_ABOVE_PAD_M + 0.25;
        self.bx(r(hl, hw), base, base + 0.55 * h, body);
        self.bx(r(0.62 * hl, 0.9 * hw), base + 0.55 * h, base + h, top);
    }

    /// Simple fence of posts and two rails along a polyline (parcel coordinates).
    fn fence(&mut self, pts: &[Vec2], h: f32, c: Rgb) {
        let base = -FLOOR_ABOVE_PAD_M;
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let len = a.distance(b);
            if len < 0.5 {
                continue;
            }
            let n = (len / 3.0).ceil().max(1.0) as usize;
            for k in 0..=n {
                let q = a + (b - a) * (k as f32 / n as f32);
                self.bx(
                    [q.x - 0.06, q.y - 0.06, q.x + 0.06, q.y + 0.06],
                    base,
                    base + h,
                    c,
                );
            }
            // Rails: thin slabs along the segment.
            let d = (b - a) / len;
            let side = d.perp() * 0.03;
            for y in [0.45 * h, 0.9 * h] {
                let fy = self.fy;
                let q: Vec<Vec2> = [a - side, b - side, b + side, a + side]
                    .iter()
                    .map(|p| self.fr.at(p.x, p.y))
                    .collect();
                self.p.prism(
                    &q,
                    fy + base + y - 0.05,
                    fy + base + y + 0.05,
                    c,
                    Some(c),
                    false,
                );
            }
        }
    }

    /// Flag pole with a flag.
    fn flagpole(&mut self, u: f32, v: f32, flag: Rgb) {
        let base = -FLOOR_ABOVE_PAD_M;
        self.bx(
            [u - 0.06, v - 0.06, u + 0.06, v + 0.06],
            base,
            base + 8.0,
            palette::LAMP_METAL,
        );
        self.panel_v(
            v,
            -1.0,
            [u + 0.06, u + 1.6],
            [base + 6.8, base + 7.8],
            flag,
            Surface::Solid,
            0,
        );
    }
}

/// Draws a mass at LOD0 (`detailed`) or LOD1.
fn draw_mass(p: &mut Painter, fr: &Frame, fy: f32, m: &Mass, detailed: bool) {
    match *m {
        Mass::Block {
            r,
            y0,
            h,
            wall,
            top,
            roof,
        } => {
            let q = fr.rect(r);
            let (a, b) = (fy + y0, fy + y0 + h);
            match top {
                Top::Flat { parapet } => {
                    p.prism(&q, a, b, wall, Some(roof), detailed);
                    // Canopy slabs above the ground need an underside.
                    if y0 > 0.5 {
                        let pts: Vec<Vec3> = q.iter().map(|v| Vec3::new(v.x, a, v.y)).collect();
                        p.face_ao(
                            &pts,
                            &[0.8; 4],
                            wall.scale(0.8),
                            Surface::Solid,
                            0,
                            -Vec3::Y,
                        );
                    }
                    if parapet > 0.0 && detailed {
                        let t = 0.3;
                        for pr in [
                            [r[0], r[1], r[2], r[1] + t],
                            [r[0], r[3] - t, r[2], r[3]],
                            [r[0], r[1] + t, r[0] + t, r[3] - t],
                            [r[2] - t, r[1] + t, r[2], r[3] - t],
                        ] {
                            p.frame_box(fr, pr, b, b + parapet, wall.scale(0.96), Some(wall));
                        }
                    }
                }
                Top::Hip(_) | Top::Gable { .. } => {
                    p.prism(&q, a, b, wall, None, detailed);
                    if let Some(spec) = roof_spec(top) {
                        roofs::roof(p, fr, r, b, &spec, roof, [wall, wall]);
                    }
                }
            }
        }
        Mass::Round {
            c,
            r0,
            r1,
            y0,
            h,
            col,
            cap,
        } => {
            let n = if detailed {
                ROUND_SIDES
            } else {
                ROUND_SIDES_LOD1
            };
            let lo = ring(fr.at(c.x, c.y), r0, n);
            let hi = ring(fr.at(c.x, c.y), r1, n);
            let (a, b) = (fy + y0, fy + y0 + h);
            let cw = fr.at(c.x, c.y);
            for k in 0..n {
                let j = (k + 1) % n;
                let mid = (lo[k] + lo[j]) * 0.5 - cw;
                let out = Vec3::new(mid.x, (r0 - r1) / h.max(0.1) * mid.length(), mid.y);
                let ao_b = p.contact_ao(a);
                p.face_ao(
                    &[
                        Vec3::new(lo[k].x, a, lo[k].y),
                        Vec3::new(lo[j].x, a, lo[j].y),
                        Vec3::new(hi[j].x, b, hi[j].y),
                        Vec3::new(hi[k].x, b, hi[k].y),
                    ],
                    &[ao_b, ao_b, 1.0, 1.0],
                    col,
                    Surface::Solid,
                    0,
                    out,
                );
            }
            let top: Vec<Vec3> = hi.iter().map(|q| Vec3::new(q.x, b, q.y)).collect();
            match cap {
                Cap::Flat => p.face_ao(
                    &top,
                    &vec![1.0; n],
                    col.scale(0.95),
                    Surface::Roof,
                    0,
                    Vec3::Y,
                ),
                Cap::Open => p.face_ao(
                    &top,
                    &vec![0.35; n],
                    palette::HAZARD[1],
                    Surface::Solid,
                    0,
                    Vec3::Y,
                ),
                Cap::Cone(ch) => {
                    let apex = Vec3::new(cw.x, b + ch, cw.y);
                    for k in 0..n {
                        let j = (k + 1) % n;
                        let mid = (hi[k] + hi[j]) * 0.5 - cw;
                        p.face_ao(
                            &[top[k], top[j], apex],
                            &[1.0; 3],
                            col.scale(0.92),
                            Surface::Roof,
                            0,
                            Vec3::new(mid.x, r1, mid.y),
                        );
                    }
                }
            }
        }
        Mass::Mound { r, h, steps, col } => {
            let steps = steps.max(1);
            let (w, d) = (r[2] - r[0], r[3] - r[1]);
            let shrink = 0.5 * w.min(d) * 0.7 / f32::from(steps);
            let sh = h / f32::from(steps);
            let base = fy - FLOOR_ABOVE_PAD_M - 0.3;
            for s in 0..steps {
                let i0 = shrink * f32::from(s);
                let i1 = i0 + 0.8 * shrink;
                let (y0, y1) = (
                    base + sh * f32::from(s) + 0.3 * f32::from(s.min(1)),
                    base + 0.3 + sh * f32::from(s + 1),
                );
                let lo = [r[0] + i0, r[1] + i0, r[2] - i0, r[3] - i0];
                let hi = [r[0] + i1, r[1] + i1, r[2] - i1, r[3] - i1];
                let ql = fr.rect(lo);
                let qh = fr.rect(hi);
                let ctr = fr.at(0.5 * (r[0] + r[2]), 0.5 * (r[1] + r[3]));
                let shade = col.scale(0.92 + 0.05 * f32::from(s));
                for k in 0..4 {
                    let j = (k + 1) % 4;
                    let mid = (ql[k] + ql[j]) * 0.5 - ctr;
                    p.face_ao(
                        &[
                            Vec3::new(ql[k].x, y0, ql[k].y),
                            Vec3::new(ql[j].x, y0, ql[j].y),
                            Vec3::new(qh[j].x, y1, qh[j].y),
                            Vec3::new(qh[k].x, y1, qh[k].y),
                        ],
                        &[0.9, 0.9, 1.0, 1.0],
                        shade,
                        Surface::Ground,
                        0,
                        Vec3::new(mid.x, 0.8 * mid.length(), mid.y),
                    );
                }
                // Terrace top; the next step's walls stand on it.
                let pts: Vec<Vec3> = qh.iter().map(|q| Vec3::new(q.x, y1, q.y)).collect();
                p.face_ao(
                    &pts,
                    &[1.0; 4],
                    col.scale(1.02 + 0.03 * f32::from(s)),
                    Surface::Ground,
                    0,
                    Vec3::Y,
                );
            }
        }
        Mass::Mast { c, y0, h, w, col } => {
            let hw = 0.5 * w;
            let (a, b) = (fy + y0, fy + y0 + h);
            let q = fr.rect([c.x - hw, c.y - hw, c.x + hw, c.y + hw]);
            p.prism(&q, a, b, col, Some(col), false);
            if detailed {
                // Antenna dishes: two small boxes near the top.
                for (dy, du) in [(0.75, 1.0f32), (0.55, -1.0)] {
                    let y = a + h * dy;
                    let u = c.x + du * (hw + 0.25);
                    p.frame_box(
                        fr,
                        [u - 0.25, c.y - 0.5, u + 0.25, c.y + 0.5],
                        y,
                        y + 1.0,
                        hex(0xE9ECEC),
                        Some(hex(0xE9ECEC)),
                    );
                }
            }
        }
    }
}

/// Builds LOD0 and LOD1 meshes on the foundation.
#[must_use]
pub fn mesh(l: &CivicLayout, f: &Foundation, slot: u16) -> (MeshData, MeshData) {
    let mut p = Painter::new(slot, l.seed);
    let mut lod1 = Painter::new(slot, l.seed ^ 1);
    let fy = f.floor_y;
    p.contact_y = fy;
    lod1.contact_y = fy;
    let fr = l.frame;
    let (w, d) = l.kind.parcel_m();
    let pad_y = fy - FLOOR_ABOVE_PAD_M;
    // Parcel terrace: a skirt down to the lowest ground (a visible retaining wall on plinths).
    let bottom = f
        .pad
        .as_ref()
        .map_or(f.ground_min, |pd| pd.target_h_m.min(f.ground_min))
        - 0.5;
    let parcel = fr.rect([0.0, 0.0, w, d]);
    let skirt = if f.kind == FoundationKind::Plinth {
        palette::STONE
    } else {
        palette::STONE.scale(0.9)
    };
    p.prism(&parcel, bottom, pad_y, skirt, None, false);
    lod1.prism(&parcel, bottom, pad_y, skirt, None, false);
    for m in &l.masses {
        draw_mass(&mut p, &fr, fy, m, true);
        draw_mass(&mut lod1, &fr, fy, m, false);
    }
    let mut c = Ctx { p: &mut p, fr, fy };
    let base_cover = match l.kind {
        CivicKind::DieselGenerator | CivicKind::CoalPlant | CivicKind::SewageOutlet => GRAVEL,
        CivicKind::Landfill => palette::DIRT[1],
        _ => palette::GRASS[0],
    };
    c.flat([0.0, 0.0, w, d], 0.02, base_cover, Surface::Ground);
    let mut id = 0u32;
    let mut rng = Pcg32::from_seed(l.seed ^ 0xC1F1C);
    match l.kind {
        CivicKind::PoliceStation => police(&mut c, l, &mut id),
        CivicKind::FireStation => fire(&mut c, l, &mut id),
        CivicKind::Clinic => clinic(&mut c, l, &mut id),
        CivicKind::ElementarySchool => school(&mut c, l, &mut id),
        CivicKind::TownHall => town_hall(&mut c, l, &mut id),
        CivicKind::DieselGenerator => diesel(&mut c, l),
        CivicKind::CoalPlant => coal(&mut c, l, &mut id),
        CivicKind::WaterPump => pump(&mut c, l, &mut id),
        CivicKind::SewageOutlet => outlet(&mut c, l),
        CivicKind::Landfill => landfill(&mut c, l, &mut rng),
    }
    (p.finish(), lod1.finish())
}

fn police(c: &mut Ctx, l: &CivicLayout, id: &mut u32) {
    let (main, wing) = (POLICE_MAIN, POLICE_WING);
    c.flat([0.0, 0.0, 26.0, 6.0], 0.04, palette::PAVING, Surface::Paved);
    c.flat(
        [3.0, 17.0, 25.0, 23.5],
        0.04,
        palette::ASPHALT,
        Surface::Paved,
    );
    c.flat(
        [13.0, 13.0, 25.0, 17.0],
        0.04,
        palette::ASPHALT,
        Surface::Paved,
    );
    c.windows(main, 4, 3.5, 2.6, (1.3, 1.5), 1.0, &[], id);
    // Signature: blue bands above the ground floor and on the wing, blue entrance canopy.
    c.band(main, 3.15, 3.75, l.accent);
    c.band(wing, 3.0, 3.8, l.accent);
    c.door(6.0, 7.0, 2.0, 2.4, l.accent.scale(0.8));
    c.bx([5.0, 4.2, 9.0, 6.0], 2.9, 3.15, l.accent);
    for u in [5.2, 8.8] {
        c.bx(
            [u - 0.1, 4.3, u + 0.1, 4.5],
            -FLOOR_ABOVE_PAD_M,
            2.9,
            palette::LAMP_METAL,
        );
    }
    // Garage doors of the wing.
    for u in [16.0, 20.0] {
        c.panel_v(
            6.0,
            -1.0,
            [u - 1.5, u + 1.5],
            [0.0, 3.0],
            hex(0xB8BEC6),
            Surface::Solid,
            0,
        );
    }
    // Parking with stall lines and patrol cars.
    for k in 0..8 {
        let u = 4.0 + 3.0 * k as f32;
        c.flat(
            [u - 0.06, 18.0, u + 0.06, 22.5],
            0.06,
            palette::MARKING,
            Surface::Marking,
        );
    }
    for k in 0..3 {
        let u = 5.5 + 3.0 * k as f32;
        c.vehicle(u, 20.3, 4.4, 1.8, 1.45, hex(0xE9ECEC), hex(0x3D5A8C), false);
    }
    c.flagpole(2.0, 2.0, l.accent);
}

fn fire(c: &mut Ctx, l: &CivicLayout, id: &mut u32) {
    let (bay, office, tower) = (FIRE_BAY, FIRE_OFFICE, FIRE_TOWER);
    c.flat([0.0, 0.0, 30.0, 9.0], 0.04, CONCRETE, Surface::Paved);
    c.flat([20.0, 16.0, 30.0, 27.0], 0.04, CONCRETE, Surface::Paved);
    // Three tall apparatus doors with panel lines and a glazed top row.
    for k in 0..3 {
        let u = bay[0] + (bay[2] - bay[0]) * (k as f32 + 0.5) / 3.0;
        let v = bay[1];
        c.panel_v(
            v,
            -1.0,
            [u - 2.45, u + 2.45],
            [0.0, 4.65],
            palette::WINDOW_FRAME,
            Surface::Solid,
            0,
        );
        c.panel_v(
            v - 0.02,
            -1.0,
            [u - 2.3, u + 2.3],
            [0.0, 4.5],
            l.accent,
            Surface::Solid,
            0,
        );
        for y in [1.1, 2.2, 3.3] {
            c.panel_v(
                v - 0.04,
                -1.0,
                [u - 2.3, u + 2.3],
                [y, y + 0.08],
                l.accent.scale(0.85),
                Surface::Solid,
                0,
            );
        }
        *id += 1;
        c.panel_v(
            v - 0.04,
            -1.0,
            [u - 1.9, u + 1.9],
            [3.6, 4.2],
            palette::GLASS,
            Surface::Window,
            *id,
        );
    }
    c.windows(office, 2, 3.5, 2.8, (1.2, 1.4), 1.0, &[3], id);
    c.door(office[1], 26.0, 1.4, 2.3, l.accent);
    c.band(bay, 4.9, 5.4, palette::WINDOW_FRAME);
    // Hose tower: slit windows up the front, a white band under the cap.
    for y in [6.0, 10.0, 14.0] {
        *id += 1;
        c.panel_v(
            tower[1],
            -1.0,
            [24.5, 25.5],
            [y, y + 1.8],
            palette::GLASS,
            Surface::Window,
            *id,
        );
    }
    c.band(tower, 19.6, 20.3, palette::WINDOW_FRAME);
    // A parked engine on the apron.
    c.vehicle(
        8.0,
        4.4,
        7.5,
        2.4,
        2.9,
        hex(0xC0453A),
        hex(0xC0453A).scale(1.1),
        false,
    );
    c.flagpole(28.0, 1.0, hex(0xC0453A));
}

fn clinic(c: &mut Ctx, l: &CivicLayout, id: &mut u32) {
    let (pav, wards) = (CLINIC_PAVILION, CLINIC_WARDS);
    c.flat(
        [0.0, 0.0, 24.0, 10.0],
        0.04,
        palette::PAVING,
        Surface::Paved,
    );
    c.flat(
        [20.3, 14.5, 23.5, 21.5],
        0.04,
        palette::ASPHALT,
        Surface::Paved,
    );
    // Glass ribbons on the pavilion, punched windows on the ward block.
    for (a, b) in [(4.6, 8.4), (15.6, 19.4)] {
        *id += 1;
        c.panel_v(
            pav[1],
            -1.0,
            [a, b],
            [0.8, 2.6],
            palette::GLASS,
            Surface::Window,
            *id,
        );
        *id += 1;
        c.panel_v(
            pav[3],
            1.0,
            [a, b],
            [0.8, 2.6],
            palette::GLASS,
            Surface::Window,
            *id,
        );
    }
    for f in 0..3 {
        let y = CLINIC_PAVILION_H + 0.9 + 3.4 * f as f32;
        *id += 1;
        c.panel_v(
            wards[1],
            -1.0,
            [9.6, 14.4],
            [y, y + 1.6],
            palette::GLASS,
            Surface::Window,
            *id,
        );
        *id += 1;
        c.panel_v(
            wards[3],
            1.0,
            [9.6, 14.4],
            [y, y + 1.6],
            palette::GLASS,
            Surface::Window,
            *id,
        );
        for (u, out) in [(wards[0], -1.0f32), (wards[2], 1.0)] {
            *id += 1;
            c.panel_u(
                u,
                out,
                [12.0, 19.0],
                [y, y + 1.6],
                palette::GLASS,
                Surface::Window,
                *id,
            );
        }
    }
    // Glass entrance under the canopy, canopy posts.
    *id += 1;
    c.panel_v(
        pav[1],
        -1.0,
        [9.8, 14.2],
        [0.0, 2.8],
        palette::GLASS,
        Surface::Window,
        *id,
    );
    for u in [7.9, 16.1] {
        c.bx(
            [u - 0.12, 4.8, u + 0.12, 5.04],
            -FLOOR_ABOVE_PAD_M,
            3.2,
            palette::WINDOW_FRAME,
        );
    }
    // Signature: green cross on a white panel on top of the ward block's front.
    c.bx([10.4, 11.2, 13.6, 11.5], 14.2, 17.4, palette::WINDOW_FRAME);
    c.panel_v(
        11.2,
        -1.0,
        [11.5, 12.5],
        [14.5, 17.1],
        l.accent,
        Surface::Solid,
        0,
    );
    c.panel_v(
        11.2,
        -1.0,
        [10.7, 13.3],
        [15.3, 16.3],
        l.accent,
        Surface::Solid,
        0,
    );
    c.band(pav, 3.4, 3.8, l.accent);
    // Rooftop units on the pavilion and an ambulance.
    c.bx([5.0, 12.0, 7.0, 14.0], 3.8, 4.7, hex(0xB8BEC6));
    c.bx([17.0, 12.0, 19.0, 14.0], 3.8, 4.7, hex(0xB8BEC6));
    c.vehicle(21.9, 18.0, 5.6, 2.1, 2.4, hex(0xF2F0EB), l.accent, false);
}

fn school(c: &mut Ctx, l: &CivicLayout, id: &mut u32) {
    let (main, wing, gym) = (SCHOOL_MAIN, SCHOOL_WING, SCHOOL_GYM);
    c.flat([0.0, 0.0, 48.0, 6.0], 0.04, palette::PAVING, Surface::Paved);
    c.flat(
        [4.0, 15.0, 29.0, 39.0],
        0.04,
        palette::PAVING,
        Surface::Paved,
    );
    // Sports field with white lines.
    let field = [39.0, 18.0, 47.0, 39.0];
    c.flat(field, 0.05, palette::GRASS[2], Surface::Foliage);
    let (t, m) = (0.08, 0.5);
    let [f0, f1, f2, f3] = field;
    for r in [
        [f0 + m, f1 + m, f2 - m, f1 + m + t],
        [f0 + m, f3 - m - t, f2 - m, f3 - m],
        [f0 + m, f1 + m, f0 + m + t, f3 - m],
        [f2 - m - t, f1 + m, f2 - m, f3 - m],
        [f0 + m, 28.5 - 0.5 * t, f2 - m, 28.5 + 0.5 * t],
    ] {
        c.flat(r, 0.07, palette::MARKING, Surface::Marking);
    }
    c.windows(main, 3, 3.6, 3.0, (1.6, 1.7), 0.9, &[], id);
    c.windows(wing, 2, 3.6, 3.0, (1.6, 1.7), 0.9, &[0, 3], id);
    c.windows(gym, 1, 6.4, 3.2, (2.2, 1.2), 4.6, &[], id);
    // Entrance: steps and a framed double door on the axis, a name band above it.
    c.bx(
        [22.5, 4.4, 27.5, 6.0],
        -FLOOR_ABOVE_PAD_M,
        0.0,
        palette::STONE,
    );
    c.door(main[1], 25.0, 2.4, 2.6, hex(0x5F7F8E));
    c.panel_v(
        main[1],
        -1.0,
        [23.4, 26.6],
        [2.8, 3.2],
        palette::WINDOW_FRAME,
        Surface::Solid,
        0,
    );
    c.band(main, 0.0, 0.5, l.body.scale(0.85));
    c.flagpole(8.0, 2.5, palette::C_SIGNS[1]);
}

fn town_hall(c: &mut Ctx, l: &CivicLayout, id: &mut u32) {
    // Plaza with cobbles between the wings, flower beds and two flagpoles.
    c.flat(
        [0.0, 0.0, 32.0, 12.0],
        0.04,
        palette::COBBLESTONE,
        Surface::Paved,
    );
    c.flat(
        [3.0, 20.0, 29.0, 29.0],
        0.04,
        palette::PAVING,
        Surface::Paved,
    );
    for u in [9.0, 19.5] {
        c.bx(
            [u, 1.5, u + 3.5, 3.5],
            -FLOOR_ABOVE_PAD_M,
            0.3,
            palette::STONE,
        );
        c.flat(
            [u + 0.2, 1.7, u + 3.3, 3.3],
            0.32,
            palette::FLOWERS[1],
            Surface::Foliage,
        );
    }
    c.flagpole(10.0, 6.5, hex(0x5872A3));
    c.flagpole(22.0, 6.5, hex(0xD9B45A));
    c.windows(HALL_MAIN, 2, 4.0, 3.2, (1.3, 2.2), 0.9, &[], id);
    for w in [HALL_WING_L, HALL_WING_R] {
        c.windows(w, 2, 3.3, 2.6, (1.2, 1.8), 0.8, &[2], id);
        c.band(w, 0.0, 0.6, l.body.scale(0.85));
    }
    // Cornice and plinth bands.
    c.band(HALL_MAIN, 7.9, 8.4, l.body.scale(0.9));
    c.band(HALL_MAIN, 0.0, 0.6, l.body.scale(0.85));
    let tower = HALL_TOWER;
    c.band(tower, 7.9, 8.4, l.body.scale(0.9));
    // Portico: columns and steps in front of the tower; tall door.
    c.bx(
        [12.0, 4.1, 20.0, 6.0],
        -FLOOR_ABOVE_PAD_M,
        0.0,
        palette::STONE,
    );
    for k in 0..4 {
        let u = 13.4 + 1.73 * k as f32;
        c.bx([u - 0.25, 5.1, u + 0.25, 5.6], 0.0, 4.5, l.body.scale(1.04));
    }
    c.bx([12.8, 4.9, 19.2, 6.0], 4.5, 5.1, l.body);
    c.door(tower[1], 16.0, 2.2, 3.4, hex(0x6E4A35));
    // Clock faces near the top of the tower on all four sides.
    for (wall, out) in [(tower[1], -1.0f32), (tower[3], 1.0)] {
        c.panel_v(
            wall,
            out,
            [14.6, 17.4],
            [23.5, 26.3],
            palette::WINDOW_FRAME,
            Surface::Solid,
            0,
        );
        c.panel_v(
            wall + 0.02 * out,
            out,
            [15.9, 16.1],
            [24.9, 26.0],
            palette::LAMP_METAL,
            Surface::Solid,
            0,
        );
    }
    for (wall, out) in [(tower[0], -1.0f32), (tower[2], 1.0)] {
        c.panel_u(
            wall,
            out,
            [7.6, 10.4],
            [23.5, 26.3],
            palette::WINDOW_FRAME,
            Surface::Solid,
            0,
        );
    }
    for f in 0..5 {
        *id += 1;
        let y = 9.5 + 2.2 * f as f32;
        c.panel_v(
            tower[1],
            -1.0,
            [15.4, 16.6],
            [y, y + 1.3],
            palette::GLASS,
            Surface::Window,
            *id,
        );
    }
}

fn diesel(c: &mut Ctx, l: &CivicLayout) {
    let house = [3.0, 4.0, 13.0, 10.0];
    // Signature: yellow hazard band along the top edge, radiators on the roof.
    c.band(house, 2.9, 3.6, l.accent);
    c.panel_v(
        4.0,
        -1.0,
        [5.0, 8.0],
        [0.0, 2.6],
        hex(0x5D6266),
        Surface::Solid,
        0,
    );
    for u in [4.5, 9.0] {
        c.bx([u, 5.0, u + 3.0, 9.0], 3.6, 4.6, hex(0x5D6266));
        for k in 0..4 {
            let v = 5.3 + k as f32;
            c.bx([u + 0.1, v, u + 2.9, v + 0.2], 4.6, 4.7, hex(0x3C4046));
        }
    }
    // Horizontal fuel tank on saddles.
    for u in [14.2, 16.4] {
        c.bx(
            [u - 0.2, 6.2, u + 0.2, 8.6],
            -FLOOR_ABOVE_PAD_M,
            0.6,
            CONCRETE,
        );
    }
    lying_tank(c, [13.6, 17.0], 7.4, 1.6, 1.0, hex(0xDCE3E8));
    // Stack rims.
    for u in [5.0, 8.0] {
        c.bx([u - 0.45, 10.75, u + 0.45, 11.65], 8.2, 8.6, l.accent);
    }
    c.fence(
        &[
            Vec2::new(0.5, 2.0),
            Vec2::new(0.5, 13.5),
            Vec2::new(17.5, 13.5),
            Vec2::new(17.5, 2.0),
            Vec2::new(11.0, 2.0),
        ],
        1.8,
        hex(0x8A9099),
    );
}

/// Horizontal cylinder along `u` (8-sided) with its axis at height `yc` above the floor.
fn lying_tank(c: &mut Ctx, u: [f32; 2], v: f32, yc: f32, r: f32, col: Rgb) {
    let n = 8;
    let pts: Vec<(f32, f32)> = (0..n)
        .map(|k| {
            let a = (k as f32 + 0.5) * std::f32::consts::TAU / n as f32;
            let (s, co) = cb_core::math::sin_cos(a);
            (v + co * r, yc + s * r)
        })
        .collect();
    let fy = c.fy;
    for k in 0..n {
        let (a, b) = (pts[k], pts[(k + 1) % n]);
        let mid = ((a.0 + b.0) * 0.5 - v, (a.1 + b.1) * 0.5 - yc);
        let q = [
            c.fr.p3(u[0], a.0, fy + a.1),
            c.fr.p3(u[1], a.0, fy + a.1),
            c.fr.p3(u[1], b.0, fy + b.1),
            c.fr.p3(u[0], b.0, fy + b.1),
        ];
        c.p.face_ao(
            &q,
            &[1.0; 4],
            col,
            Surface::Solid,
            0,
            c.fr.d3(0.0, mid.0, mid.1),
        );
    }
    for (uu, s) in [(u[0], -1.0f32), (u[1], 1.0)] {
        let q: Vec<Vec3> = pts.iter().map(|&(vv, y)| c.fr.p3(uu, vv, fy + y)).collect();
        c.p.face_ao(
            &q,
            &vec![1.0; n],
            col.scale(0.95),
            Surface::Solid,
            0,
            c.fr.d3(s, 0.0, 0.0),
        );
    }
}

fn coal(c: &mut Ctx, l: &CivicLayout, id: &mut u32) {
    c.flat(
        [0.0, 0.0, 90.0, 8.0],
        0.04,
        palette::ASPHALT,
        Surface::Paved,
    );
    // Hazard bands near the chimney top.
    let fy = c.fy;
    for y0 in [64.0, 70.0] {
        let poly = ring(c.fr.at(58.0, 50.0), 2.55, ROUND_SIDES);
        c.p.prism(&poly, fy + y0, fy + y0 + 2.5, l.accent, None, false);
    }
    // Window bands on the turbine hall and boiler house.
    for (v, out) in [(22.0, -1.0f32), (40.0, 1.0)] {
        *id += 1;
        c.panel_v(
            v,
            out,
            [9.0, 29.0],
            [8.0, 12.5],
            palette::GLASS,
            Surface::Window,
            *id,
        );
    }
    for y in [6.0, 14.0, 22.0] {
        *id += 1;
        c.panel_v(
            18.0,
            -1.0,
            [32.0, 50.0],
            [y, y + 3.0],
            palette::GLASS,
            Surface::Window,
            *id,
        );
    }
    c.panel_v(
        18.0,
        -1.0,
        [37.0, 45.0],
        [0.0, 6.0],
        hex(0x5D6266),
        Surface::Solid,
        0,
    );
    // Conveyor from the coal pile up into the boiler house.
    beam(
        c,
        Vec3::new(24.0, 2.0, 50.0),
        Vec3::new(36.0, 26.0, 44.0),
        1.6,
        hex(0x8A8C86),
    );
    // Switchyard: transformers and gantries.
    c.flat(
        [8.0, 6.0, 28.0, 18.0],
        0.05,
        GRAVEL.scale(0.95),
        Surface::Ground,
    );
    for k in 0..3 {
        let u = 10.0 + 6.0 * k as f32;
        c.bx([u, 9.0, u + 3.0, 12.0], 0.0, 3.0, hex(0x6E7F5A));
        c.bx(
            [u + 1.3, 13.0, u + 1.7, 13.4],
            0.0,
            10.0,
            palette::LAMP_METAL,
        );
    }
    c.bx([10.0, 13.0, 26.0, 13.4], 9.6, 10.0, palette::LAMP_METAL);
    // Coal wagon-loader and a dozer on the pile.
    c.vehicle(
        18.0,
        60.0,
        4.0,
        2.4,
        2.2,
        hex(0xD9B45A),
        hex(0xD9B45A),
        true,
    );
}

/// Box beam between two parcel-space points `(u, y, v)` (y above the floor), `w` wide.
fn beam(c: &mut Ctx, a: Vec3, b: Vec3, w: f32, col: Rgb) {
    let pa = c.fr.p3(a.x, a.z, c.fy + a.y);
    let pb = c.fr.p3(b.x, b.z, c.fy + b.y);
    let d = (pb - pa).normalize_or_zero();
    let side = d.cross(Vec3::Y).normalize_or(Vec3::X) * (0.5 * w);
    let up = side.cross(d).normalize_or(Vec3::Y) * (0.5 * w);
    let corners = |p: Vec3| [p - side - up, p + side - up, p + side + up, p - side + up];
    let (ca, cb) = (corners(pa), corners(pb));
    for k in 0..4 {
        let j = (k + 1) % 4;
        let mid = (ca[k] + ca[j]) * 0.5 - pa;
        c.p.face_ao(
            &[ca[k], ca[j], cb[j], cb[k]],
            &[1.0; 4],
            col,
            Surface::Solid,
            0,
            mid,
        );
    }
    // Support legs every 8 m.
    let n = (pa.distance(pb) / 8.0).floor() as usize;
    for k in 1..=n {
        let t = k as f32 / (n + 1) as f32;
        let p = a + (b - a) * t;
        c.bx(
            [p.x - 0.3, p.z - 0.3, p.x + 0.3, p.z + 0.3],
            -FLOOR_ABOVE_PAD_M,
            p.y - 0.5 * w,
            palette::LAMP_METAL,
        );
    }
}

fn pump(c: &mut Ctx, l: &CivicLayout, id: &mut u32) {
    let house = PUMP_HOUSE;
    c.flat([4.5, 0.0, 7.5, 4.0], 0.04, palette::PAVING, Surface::Paved);
    c.windows(house, 1, 3.2, 2.3, (0.9, 1.0), 1.3, &[0], id);
    c.door(house[1], 6.0, 1.5, 2.3, hex(0x3E6E8E));
    c.band(house, 0.0, 0.5, palette::STONE);
    // Intake pipe out past the back of the parcel into the water, with an intake head.
    let reach = l.kind.pipe_reach_m();
    let (_, d) = l.kind.parcel_m();
    c.bx([5.6, house[3], 6.4, d + reach], 0.1, 0.9, hex(0x5D6266));
    c.bx([5.0, d + reach - 1.2, 7.0, d + reach], -3.0, 1.4, CONCRETE);
    // Tank: ladder and a blue band near the top.
    let fy = c.fy;
    let poly = ring(c.fr.at(12.6, 9.0), 2.25, ROUND_SIDES);
    c.p.prism(&poly, fy + 9.6, fy + 10.2, WATER_BODY, None, false);
    c.bx([12.4, 6.6, 12.8, 6.8], 0.0, 11.0, palette::LAMP_METAL);
    c.fence(
        &[
            Vec2::new(10.0, 0.5),
            Vec2::new(15.5, 0.5),
            Vec2::new(15.5, 13.5),
            Vec2::new(0.5, 13.5),
            Vec2::new(0.5, 0.5),
            Vec2::new(4.0, 0.5),
        ],
        1.8,
        hex(0x8A9099),
    );
}

fn outlet(c: &mut Ctx, l: &CivicLayout) {
    // Water surface inside the basin and the outfall pipe to the shore.
    let fy = c.fy;
    let surf = ring(c.fr.at(7.8, 7.6), 2.95, ROUND_SIDES);
    let pts: Vec<Vec3> = surf.iter().map(|q| Vec3::new(q.x, fy + 1.1, q.y)).collect();
    c.p.face_ao(
        &pts,
        &[1.0; ROUND_SIDES],
        l.accent,
        Surface::Paved,
        0,
        Vec3::Y,
    );
    let reach = l.kind.pipe_reach_m();
    let (_, d) = l.kind.parcel_m();
    c.bx([7.4, 10.6, 8.2, d + reach], 0.1, 0.9, hex(0x5D6266));
    c.bx([6.2, d + reach - 0.8, 9.4, d + reach], -2.0, 1.6, CONCRETE);
    c.panel_v(
        2.5,
        -1.0,
        [2.8, 4.2],
        [0.0, 2.2],
        hex(0x5D6266),
        Surface::Solid,
        0,
    );
    c.band([1.5, 2.5, 5.5, 6.0], 2.6, 3.0, l.accent);
    c.fence(
        &[
            Vec2::new(6.0, 0.5),
            Vec2::new(11.5, 0.5),
            Vec2::new(11.5, 11.5),
            Vec2::new(0.5, 11.5),
            Vec2::new(0.5, 0.5),
        ],
        1.6,
        hex(0x8A9099),
    );
}

fn landfill(c: &mut Ctx, l: &CivicLayout, rng: &mut Pcg32) {
    c.flat([10.0, 0.0, 22.0, 14.0], 0.04, CONCRETE, Surface::Paved);
    c.flat(
        [12.0, 2.0, 20.0, 6.0],
        0.06,
        palette::LAMP_METAL,
        Surface::Paved,
    );
    c.panel_v(
        3.0,
        -1.0,
        [4.5, 7.5],
        [1.0, 2.2],
        palette::GLASS,
        Surface::Window,
        1,
    );
    // Debris scattered on the top terrace.
    let top_y = 9.0 - FLOOR_ABOVE_PAD_M;
    let colours = [
        GARBAGE_ACCENT,
        hex(0xD9C9A8),
        hex(0x5F7F8E),
        hex(0x9B6B4D),
        hex(0xE9ECEC),
    ];
    for _ in 0..18 {
        let u = 26.0 + 14.0 * rng.next_f32();
        let v = 28.0 + 10.0 * rng.next_f32();
        let s = 0.6 + 1.2 * rng.next_f32();
        let col = palette::pick(rng, &colours);
        c.bx(
            [u, v, u + s, v + 0.8 * s],
            top_y - 0.3,
            top_y + 0.4 * s,
            col,
        );
    }
    // Garbage trucks at the weighbridge and on the ramp.
    c.vehicle(16.0, 4.0, 7.0, 2.4, 3.0, hex(0x6E8B4E), hex(0xE9ECEC), true);
    c.vehicle(48.0, 9.0, 7.0, 2.4, 3.0, hex(0x6E8B4E), hex(0xE9ECEC), true);
    c.fence(
        &[
            Vec2::new(24.0, 0.5),
            Vec2::new(63.5, 0.5),
            Vec2::new(63.5, 55.5),
            Vec2::new(0.5, 55.5),
            Vec2::new(0.5, 0.5),
            Vec2::new(8.0, 0.5),
        ],
        2.0,
        l.accent.scale(0.8),
    );
}
