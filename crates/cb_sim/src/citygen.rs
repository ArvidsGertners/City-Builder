//! Scripted city generation for benchmarks, screenshot scenes and tests (M2: `--benchmark`
//! harness, ~500 houses). Builds through the same functions as player commands, so the result
//! is a regular, valid city; roads or plots that fail validation (water, steep ground) are
//! skipped and counted.

use bevy::ecs::world::World;
use cb_core::ids::NodeId;
use cb_geometry::CubicBezier2;
use cb_network::RoadKind;
use glam::Vec2;

use crate::catalog::ZoneKind;
use crate::commands::{BuildRoad, CreatePlot, Endpoint, FrontageSpan};
use crate::config::SimConfig;
use crate::ids::Ids;
use crate::roads::{self, RoadNode, RoadSegment};

/// A street grid.
#[derive(Clone, Debug, PartialEq)]
pub struct GridSpec {
    /// Grid centre.
    pub center: Vec2,
    /// Blocks along x and z.
    pub blocks: (u32, u32),
    /// Block size (m, centre to centre).
    pub block_m: Vec2,
    /// Road type of every street.
    pub kind: RoadKind,
    /// Plot depth (m; 0 = no plots).
    pub plot_depth_m: f32,
    /// Grow a house on every lot immediately.
    pub grow: bool,
}

impl Default for GridSpec {
    fn default() -> Self {
        Self {
            center: Vec2::ZERO,
            blocks: (6, 5),
            block_m: Vec2::new(110.0, 80.0),
            kind: RoadKind::Street,
            plot_depth_m: 30.0,
            grow: true,
        }
    }
}

/// What citygen produced.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CityGenReport {
    /// Segments built.
    pub segments: usize,
    /// Roads rejected by validation.
    pub roads_rejected: usize,
    /// Plots created.
    pub plots: usize,
    /// Plots rejected.
    pub plots_rejected: usize,
    /// Houses grown.
    pub buildings: usize,
}

/// Node at a position (within 0.5 m).
#[must_use]
pub fn node_at(world: &World, p: Vec2) -> Option<NodeId> {
    let ids = world.get_resource::<Ids>()?;
    ids.nodes.iter().find_map(|(id, e)| {
        let n = world.get::<RoadNode>(e)?;
        (Vec2::new(n.pos().x, n.pos().z).distance(p) < roads::SNAP_TOL_M).then_some(id)
    })
}

/// Builds a straight road between two points, connecting to nodes that already exist there.
pub fn road(world: &mut World, kind: RoadKind, a: Vec2, b: Vec2) -> bool {
    let ep = |w: &World, p: Vec2| node_at(w, p).map_or(Endpoint::Free, Endpoint::Node);
    let cmd = BuildRoad {
        kind,
        curve: CubicBezier2::straight(a, b),
        start: ep(world, a),
        end: ep(world, b),
    };
    let tick = world
        .get_resource::<crate::SimClock>()
        .map_or(0, |c| c.tick().0);
    roads::build_road(world, &cmd, tick).is_ok()
}

/// Plots on both sides of every segment.
pub fn plots_along_all(world: &mut World, zone: ZoneKind, depth: f32, report: &mut CityGenReport) {
    let segs: Vec<(cb_core::ids::SegmentId, f32)> = world
        .get_resource::<Ids>()
        .map(|ids| {
            ids.segments
                .iter()
                .filter_map(|(id, e)| world.get::<RoadSegment>(e).map(|s| (id, s.geom().length())))
                .collect()
        })
        .unwrap_or_default();
    for (sid, len) in segs {
        for left in [true, false] {
            let (s0, s1) = if left { (0.0, len) } else { (len, 0.0) };
            let c = CreatePlot {
                zone,
                frontage: vec![FrontageSpan {
                    segment: sid,
                    left,
                    s0,
                    s1,
                }],
                depth_m: depth,
                dn: 0,
            };
            match crate::zoning::create_plot(world, &c) {
                Ok(_) => report.plots += 1,
                Err(_) => report.plots_rejected += 1,
            }
        }
    }
}

/// Generates a street grid with plots (and houses) in sandbox mode.
pub fn grid(world: &mut World, spec: &GridSpec) -> CityGenReport {
    let mut report = CityGenReport::default();
    let saved = world.get_resource::<SimConfig>().cloned();
    if let Some(mut c) = world.get_resource_mut::<SimConfig>() {
        c.sandbox = true;
    }
    let (nx, nz) = spec.blocks;
    let origin = spec.center - Vec2::new(nx as f32, nz as f32) * spec.block_m * 0.5;
    let at = |i: u32, j: u32| origin + Vec2::new(i as f32, j as f32) * spec.block_m;
    for j in 0..=nz {
        for i in 0..nx {
            if road(world, spec.kind, at(i, j), at(i + 1, j)) {
                report.segments += 1;
            } else {
                report.roads_rejected += 1;
            }
        }
    }
    for i in 0..=nx {
        for j in 0..nz {
            if road(world, spec.kind, at(i, j), at(i, j + 1)) {
                report.segments += 1;
            } else {
                report.roads_rejected += 1;
            }
        }
    }
    // Updates from road building (plot dirtying) settle before plots are drawn.
    crate::zoning::update_dirty_plots(world);
    if spec.plot_depth_m > 0.0 {
        plots_along_all(world, ZoneKind::Residential, spec.plot_depth_m, &mut report);
    }
    if spec.grow {
        let lots: Vec<cb_core::ids::LotId> = world
            .get_resource::<Ids>()
            .map(|i| i.lots.iter().map(|(id, _)| id).collect())
            .unwrap_or_default();
        for l in lots {
            if crate::buildings::grow_lot(world, l).is_some() {
                report.buildings += 1;
            }
        }
    }
    if let Some(c) = saved {
        world.insert_resource(c);
    }
    report
}
