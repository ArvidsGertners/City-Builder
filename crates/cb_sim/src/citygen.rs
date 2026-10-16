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

/// A roundabout (ALGORITHMS §1 preset): four one-way quarter arcs of radius `r` around
/// `center`, counter-clockwise in (x, z), plus a street from each ring node outward by `arm`
/// metres (none for `arm` ≤ 0). Returns whether every piece was built.
pub fn roundabout(world: &mut World, center: Vec2, r: f32, arm: f32) -> bool {
    let k = 0.552_284_8 * r;
    let dirs = [Vec2::X, Vec2::Y, -Vec2::X, -Vec2::Y];
    let ep = |w: &World, p: Vec2| node_at(w, p).map_or(Endpoint::Free, Endpoint::Node);
    let tick = world
        .get_resource::<crate::SimClock>()
        .map_or(0, |c| c.tick().0);
    let mut ok = true;
    for q in 0..4 {
        let (a, b) = (dirs[q], dirs[(q + 1) % 4]);
        let (p0, p3) = (center + a * r, center + b * r);
        let cmd = BuildRoad {
            kind: RoadKind::OneWayStreet,
            curve: CubicBezier2::new(p0, p0 + b * k, p3 + a * k, p3),
            start: ep(world, p0),
            end: ep(world, p3),
        };
        ok &= roads::build_road(world, &cmd, tick).is_ok();
    }
    if arm > 0.0 {
        for d in dirs {
            ok &= road(
                world,
                RoadKind::Street,
                center + d * r,
                center + d * (r + arm),
            );
        }
    }
    ok
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

/// A located scene feature: where to look and along which road direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Feature {
    /// Ground position (m).
    pub pos: Vec2,
    /// Road direction at the position (unit).
    pub dir: Vec2,
    /// Size of the feature (cut or fill depth, m; 0 when not applicable).
    pub depth_m: f32,
}

/// The scripted M2 scene features (PROGRESS M2 shot list).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct M2Features {
    /// Middle of the curved street on the ~8 % slope.
    pub slope: Option<Feature>,
    /// Deepest fill embankment of the scripted roads.
    pub fill: Option<Feature>,
    /// Deepest cut of the scripted roads.
    pub cut: Option<Feature>,
    /// End of the street at the river or lake shore.
    pub shore: Option<Feature>,
    /// The corner plot's junction.
    pub corner: Option<Vec2>,
    /// Ground too steep for a street (nothing built; `dir` uphill), for the invalid-ghost shot.
    pub steep: Option<Feature>,
    /// Counts.
    pub report: CityGenReport,
}

/// Terrain probes for the feature search (pre-build ground).
struct Probe<'a>(&'a cb_terrain::Terrain);

impl Probe<'_> {
    fn h(&self, p: Vec2) -> f32 {
        self.0.height_at(p)
    }
    fn wet(&self, p: Vec2) -> bool {
        self.0.water().is_water(p)
    }
    fn half(&self) -> f32 {
        let h = self.0.height();
        0.5 * (h.n() - 1) as f32 * h.spacing_m()
    }
    /// Mean slope over a 60 m patch, `None` when any probe is wet or off the map.
    fn patch_slope(&self, c: Vec2) -> Option<f32> {
        let lim = self.half() - 200.0;
        let mut sum = 0.0;
        for j in -2..=2 {
            for i in -2..=2 {
                let p = c + Vec2::new(i as f32, j as f32) * 15.0;
                if p.x.abs() > lim || p.y.abs() > lim || self.wet(p) {
                    return None;
                }
                sum += self.0.height().slope_at(p);
            }
        }
        Some(sum / 25.0)
    }
    fn uphill(&self, p: Vec2) -> Vec2 {
        let d = 10.0;
        let g = Vec2::new(
            self.h(p + Vec2::X * d) - self.h(p - Vec2::X * d),
            self.h(p + Vec2::Y * d) - self.h(p - Vec2::Y * d),
        );
        g.try_normalize().unwrap_or(Vec2::X)
    }
}

fn segments_snapshot(world: &World) -> Vec<cb_core::ids::SegmentId> {
    world
        .get_resource::<Ids>()
        .map(|i| i.segments.iter().map(|(id, _)| id).collect())
        .unwrap_or_default()
}

/// Builds a road and records the deepest cut and fill (against the pre-build ground).
fn road_measured(
    world: &mut World,
    cmd: &BuildRoad,
    cut: &mut Option<Feature>,
    fill: &mut Option<Feature>,
) -> bool {
    let Ok(pv) = roads::preview_road(world, cmd) else {
        return false;
    };
    if let Some(t) = world.get_resource::<crate::terrain::TerrainRes>() {
        let t = t.terrain();
        let len = pv.geom.length();
        let mut s = 4.0;
        while s < len - 4.0 {
            let c = pv.geom.point_at(s);
            let dir = pv.geom.dir_at(s);
            let road_h = pv.profile.height_at(s);
            for off in [-1.0, 0.0, 1.0] {
                let p = c + dir.perp() * (off * pv.geom.half_width_m);
                let d = road_h - t.height_at(p);
                if d > fill.map_or(0.0, |f| f.depth_m) {
                    *fill = Some(Feature {
                        pos: c,
                        dir,
                        depth_m: d,
                    });
                }
                if -d > cut.map_or(0.0, |f| f.depth_m) {
                    *cut = Some(Feature {
                        pos: c,
                        dir,
                        depth_m: -d,
                    });
                }
            }
            s += 2.0;
        }
    }
    let tick = world
        .get_resource::<crate::SimClock>()
        .map_or(0, |c| c.tick().0);
    roads::build_road(world, cmd, tick).is_ok()
}

fn free(kind: RoadKind, curve: CubicBezier2) -> BuildRoad {
    BuildRoad {
        kind,
        curve,
        start: Endpoint::Free,
        end: Endpoint::Free,
    }
}

/// Corner plot in the quadrant between two segments leaving `node` (40 m along each).
fn corner_plot(
    world: &mut World,
    node: Vec2,
    a: cb_core::ids::SegmentId,
    b: cb_core::ids::SegmentId,
) -> bool {
    let geo = |id| {
        world
            .get_resource::<Ids>()
            .and_then(|i| i.segments.get(id))
            .and_then(|e| world.get::<RoadSegment>(e))
            .map(|s| s.geom().clone())
    };
    let (Some(ga), Some(gb)) = (geo(a), geo(b)) else {
        return false;
    };
    // Away-from-node direction and the node-end arc length of each segment.
    let away = |g: &cb_network::SegmentGeom| {
        let len = g.length();
        if g.point_at(0.0).distance(node) < g.point_at(len).distance(node) {
            (g.dir_at(0.0), 0.0, len.min(40.0))
        } else {
            (-g.dir_at(len), len, (len - 40.0).max(0.0))
        }
    };
    let ((ua, na, fa), (ub, nb, fb)) = (away(&ga), away(&gb));
    let cross = ua.perp_dot(ub);
    let inside = |v: Vec2| ua.perp_dot(v) * cross > 0.0 && v.perp_dot(ub) * cross > 0.0;
    let side = |g: &cb_network::SegmentGeom, s: f32| {
        let probe = g.point_at(s) + g.dir_at(s).perp() * 10.0;
        inside(probe - node)
    };
    let (ma, mb) = (0.5 * (na + fa), 0.5 * (nb + fb));
    let c = CreatePlot {
        zone: ZoneKind::Residential,
        frontage: vec![
            FrontageSpan {
                segment: a,
                left: side(&ga, ma),
                s0: fa,
                s1: na,
            },
            FrontageSpan {
                segment: b,
                left: side(&gb, mb),
                s0: nb,
                s1: fb,
            },
        ],
        depth_m: 30.0,
        dn: 0,
    };
    crate::zoning::create_plot(world, &c).is_ok()
}

/// The scripted M2 features around a town at `town` with half extent `town_half`: a curved
/// street on a ~8 % slope with a side street and a corner plot, the straight street over the
/// most pronounced crest (a cut) and across the deepest dip (a fill), a street to the nearest
/// shore, plots on all of them and houses on every lot. Sandbox mode; features that find no
/// site or fail validation are left out.
pub fn m2_features(world: &mut World, town: Vec2, town_half: Vec2) -> M2Features {
    let mut out = M2Features::default();
    let saved = world.get_resource::<SimConfig>().cloned();
    if let Some(mut c) = world.get_resource_mut::<SimConfig>() {
        c.sandbox = true;
    }
    let Some(terrain) = world
        .get_resource::<crate::terrain::TerrainRes>()
        .map(|t| t.terrain().clone())
    else {
        return out;
    };
    let probe = Probe(&terrain);
    let before = segments_snapshot(world);
    // Lots of the existing town; every lot created below (incl. the corner plot) grows a house.
    let lots_before: Vec<cb_core::ids::LotId> = world
        .get_resource::<Ids>()
        .map(|i| i.lots.iter().map(|(id, _)| id).collect())
        .unwrap_or_default();

    let keep_out = town_half + Vec2::splat(140.0);
    let outside = |p: Vec2, margin: f32| {
        (p.x - town.x).abs() > keep_out.x + margin || (p.y - town.y).abs() > keep_out.y + margin
    };
    let (mut cut, mut fill) = (None, None);

    // 1. Curved street on the ~8 % slope, a side street downhill and a corner plot.
    let mut best: Option<(f32, Vec2)> = None;
    for gz in -24..=24 {
        for gx in -24..=24 {
            let p = town + Vec2::new(gx as f32, gz as f32) * 40.0;
            if !outside(p, 80.0) {
                continue;
            }
            if let Some(s) = probe.patch_slope(p) {
                let score = (s - 0.08).abs() * 20.0 + p.distance(town) / 1000.0;
                if (0.055..=0.11).contains(&s) && best.is_none_or(|b| score < b.0) {
                    best = Some((score, p));
                }
            }
        }
    }
    if let Some((_, p)) = best {
        let up = probe.uphill(p);
        let t = up.perp();
        let a = p - t * 85.0;
        let b = p + t * 85.0;
        let curve = CubicBezier2::from_quadratic(a, p - up * 36.0, b);
        let mid = curve.eval(0.5);
        if road_measured(world, &free(RoadKind::Street, curve), &mut cut, &mut fill) {
            out.report.segments += 1;
            out.slope = Some(Feature {
                pos: mid,
                dir: t,
                depth_m: 0.0,
            });
            let side_end = mid - up * 75.0;
            let before_side = segments_snapshot(world);
            if road_measured(
                world,
                &free(RoadKind::Street, CubicBezier2::straight(mid, side_end)),
                &mut cut,
                &mut fill,
            ) {
                out.report.segments += 1;
                crate::zoning::update_dirty_plots(world);
                // The side street split the curved street at its middle: corner between the
                // side street and the half toward `a`.
                let new: Vec<_> = segments_snapshot(world)
                    .into_iter()
                    .filter(|s| !before_side.contains(s))
                    .collect();
                let geo_mid = |id| {
                    world
                        .get_resource::<Ids>()
                        .and_then(|i| i.segments.get(id))
                        .and_then(|e| world.get::<RoadSegment>(e))
                        .map(|s| s.geom().point_at(0.5 * s.geom().length()))
                };
                let side = new.iter().copied().find(|id| {
                    geo_mid(*id).is_some_and(|m| m.distance(mid.lerp(side_end, 0.5)) < 5.0)
                });
                let half_a = new.iter().copied().find(|id| {
                    geo_mid(*id)
                        .is_some_and(|m| m.distance(a) < m.distance(b) && m.distance(mid) > 20.0)
                });
                if let (Some(sa), Some(ha)) = (side, half_a)
                    && corner_plot(world, mid, ha, sa)
                {
                    out.report.plots += 1;
                    out.corner = Some(mid);
                }
            }
        }
    }

    // Steep ground for the road tool's "too steep" ghost.
    let mut steep: Option<(f32, Vec2)> = None;
    for gz in -24..=24 {
        for gx in -24..=24 {
            let p = town + Vec2::new(gx as f32, gz as f32) * 40.0;
            if outside(p, 60.0)
                && let Some(s) = probe.patch_slope(p)
                && s > 0.16
                && steep.is_none_or(|b| p.distance(town) < b.0)
            {
                steep = Some((p.distance(town), p));
            }
        }
    }
    out.steep = steep.map(|(_, p)| Feature {
        pos: p,
        dir: probe.uphill(p),
        depth_m: 0.0,
    });

    // 2. Crest (cut) and dip (fill) crossings, apart from each other and the slope street.
    let mut taken: Vec<Vec2> = out.slope.iter().map(|f| f.pos).collect();
    let mut bare: Vec<cb_core::ids::SegmentId> = Vec::new();
    let mut one_sided: Vec<cb_core::ids::SegmentId> = Vec::new();
    for sign in [1.0f32, -1.0] {
        let mut cands: Vec<(f32, Vec2, Vec2)> = Vec::new();
        for gz in -20..=20 {
            for gx in -20..=20 {
                let p = town + Vec2::new(gx as f32, gz as f32) * 45.0;
                if !outside(p, 100.0) || probe.patch_slope(p).is_none_or(|s| s > 0.12) {
                    continue;
                }
                for k in 0..4 {
                    let ang = k as f32 * std::f32::consts::FRAC_PI_4;
                    let d = Vec2::new(cb_core::math::cos(ang), cb_core::math::sin(ang));
                    let (e0, e1) = (p - d * 55.0, p + d * 55.0);
                    if [e0, p, e1].iter().any(|q| probe.wet(*q)) {
                        continue;
                    }
                    let bump = sign * (probe.h(p) - 0.5 * (probe.h(e0) + probe.h(e1)));
                    if bump > 2.5 {
                        cands.push((bump - p.distance(town) / 400.0, p, d));
                    }
                }
            }
        }
        cands.sort_by(|x, y| y.0.total_cmp(&x.0));
        let mut tries = 0;
        for (_, p, d) in cands {
            if tries >= 12 {
                break;
            }
            if taken.iter().any(|q| q.distance(p) < 250.0) {
                continue;
            }
            tries += 1;
            let cmd = free(
                RoadKind::Street,
                CubicBezier2::straight(p - d * 70.0, p + d * 70.0),
            );
            let before_crossing = segments_snapshot(world);
            if road_measured(world, &cmd, &mut cut, &mut fill) {
                out.report.segments += 1;
                taken.push(p);
                // The cut crossing stays bare (its shot looks at the cut face); the fill crossing
                // gets houses on its left side only, so the fill shot sees the embankment in
                // front and houses on plinths or stilts behind it.
                let added = segments_snapshot(world)
                    .into_iter()
                    .filter(|s| !before_crossing.contains(s));
                if sign > 0.0 {
                    bare.extend(added);
                } else {
                    one_sided.extend(added);
                }
                break;
            }
            out.report.roads_rejected += 1;
        }
    }

    // 3. Street to the shore: the shortest street of 80–800 m from an existing node to a point
    //    14 m before the water.
    let nodes: Vec<(NodeId, Vec2)> = world
        .get_resource::<Ids>()
        .map(|ids| {
            ids.nodes
                .iter()
                .filter_map(|(id, e)| {
                    world
                        .get::<RoadNode>(e)
                        .map(|n| (id, Vec2::new(n.pos().x, n.pos().z)))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut shore: Vec<(f32, NodeId, Vec2, Vec2)> = Vec::new();
    for k in 0..72 {
        let ang = k as f32 * std::f32::consts::TAU / 72.0;
        let d = Vec2::new(cb_core::math::cos(ang), cb_core::math::sin(ang));
        let mut r = 60.0;
        while r < 2000.0 {
            if probe.wet(town + d * r) {
                let wet = town + d * r;
                for (nid, np) in &nodes {
                    let to = wet - *np;
                    let len = to.length();
                    let end = wet - to / len.max(1.0) * 14.0;
                    let l = np.distance(end);
                    if (80.0..=800.0).contains(&l) {
                        shore.push((l, *nid, *np, end));
                    }
                }
                break;
            }
            r += 10.0;
        }
    }
    shore.sort_by(|x, y| x.0.total_cmp(&y.0));
    tracing::debug!("shore candidates: {}", shore.len());
    for (_, nid, np, end) in shore.into_iter().take(12) {
        let cmd = BuildRoad {
            kind: RoadKind::Street,
            curve: CubicBezier2::straight(np, end),
            start: Endpoint::Node(nid),
            end: Endpoint::Free,
        };
        if road_measured(world, &cmd, &mut None, &mut None) {
            out.report.segments += 1;
            out.shore = Some(Feature {
                pos: end,
                dir: (end - np).normalize_or_zero(),
                depth_m: 0.0,
            });
            break;
        }
        out.report.roads_rejected += 1;
    }
    out.cut = cut.filter(|c| c.depth_m > 0.5);
    out.fill = fill.filter(|f| f.depth_m > 0.5);

    // 4. Plots on every new segment, houses on every new lot.
    crate::zoning::update_dirty_plots(world);
    let new: Vec<_> = segments_snapshot(world)
        .into_iter()
        .filter(|s| !before.contains(s) && !bare.contains(s))
        .collect();
    for sid in new {
        let Some(len) = world
            .get_resource::<Ids>()
            .and_then(|i| i.segments.get(sid))
            .and_then(|e| world.get::<RoadSegment>(e))
            .map(|s| s.geom().length())
        else {
            continue;
        };
        let sides: &[bool] = if one_sided.contains(&sid) {
            &[true]
        } else {
            &[true, false]
        };
        for &left in sides {
            let (s0, s1) = if left { (0.0, len) } else { (len, 0.0) };
            let c = CreatePlot {
                zone: ZoneKind::Residential,
                frontage: vec![FrontageSpan {
                    segment: sid,
                    left,
                    s0,
                    s1,
                }],
                depth_m: 30.0,
                dn: 0,
            };
            match crate::zoning::create_plot(world, &c) {
                Ok(_) => out.report.plots += 1,
                Err(_) => out.report.plots_rejected += 1,
            }
        }
    }
    let lots: Vec<cb_core::ids::LotId> = world
        .get_resource::<Ids>()
        .map(|i| {
            i.lots
                .iter()
                .map(|(id, _)| id)
                .filter(|id| !lots_before.contains(id))
                .collect()
        })
        .unwrap_or_default();
    for l in lots {
        if crate::buildings::grow_lot(world, l).is_some() {
            out.report.buildings += 1;
        }
    }
    if let Some(c) = saved {
        world.insert_resource(c);
    }
    out
}

/// The scripted M3 scene features (PROGRESS M3 shot list).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct M3Features {
    /// Middle of a street bridge over the river (piers in the water).
    pub river_bridge: Option<Feature>,
    /// Middle of a street bridge over a dry valley with piers taller than 25 m.
    pub valley_bridge: Option<Feature>,
    /// Where a street on the valley floor passes under the valley bridge.
    pub overpass: Option<Feature>,
    /// Middle of a curved, decorated avenue (cobblestone, grass strips, trees, lamps).
    pub decorated: Option<Feature>,
    /// Avenue × street crossing with traffic lights.
    pub lights_x: Option<Feature>,
    /// Y junction with a 30° branch.
    pub y30: Option<Feature>,
    /// Five-way junction.
    pub five_way: Option<Feature>,
    /// A pair of opposite one-way streets and a dirt path, crossed by a street.
    pub one_way: Option<Feature>,
    /// A roundabout with four arms.
    pub roundabout: Option<Feature>,
    /// Counts.
    pub report: CityGenReport,
}

/// Builds the scripted M3 features around the town: a river bridge, a valley bridge with tall
/// piers and a street under it. Sandbox mode; features without a site are left out.
pub fn m3_features(world: &mut World, town: Vec2, town_half: Vec2) -> M3Features {
    let mut out = M3Features::default();
    let saved = world.get_resource::<SimConfig>().cloned();
    if let Some(mut c) = world.get_resource_mut::<SimConfig>() {
        c.sandbox = true;
    }
    let Some(terrain) = world
        .get_resource::<crate::terrain::TerrainRes>()
        .map(|t| t.terrain().clone())
    else {
        return out;
    };
    let probe = Probe(&terrain);
    let lim = probe.half() - 150.0;
    let keep_out = town_half + Vec2::splat(260.0);
    let usable = |p: Vec2| {
        p.x.abs() < lim
            && p.y.abs() < lim
            && ((p.x - town.x).abs() > keep_out.x || (p.y - town.y).abs() > keep_out.y)
    };
    let dirs: Vec<Vec2> = (0..8)
        .map(|k| {
            let a = k as f32 * std::f32::consts::PI / 8.0;
            Vec2::new(cb_core::math::cos(a), cb_core::math::sin(a))
        })
        .collect();
    let tick = world
        .get_resource::<crate::SimClock>()
        .map_or(0, |c| c.tick().0);
    let try_build =
        |world: &mut World, a: Vec2, b: Vec2, need: &dyn Fn(&roads::RoadPreview) -> bool| {
            let cmd = BuildRoad {
                kind: RoadKind::Street,
                curve: CubicBezier2::straight(a, b),
                start: Endpoint::Free,
                end: Endpoint::Free,
            };
            match roads::preview_road(world, &cmd) {
                Ok(pv) if need(&pv) => roads::build_road(world, &cmd, tick).is_ok(),
                _ => false,
            }
        };

    // 1. River bridge: the nearest wet point, crossed where the water is narrowest.
    let mut wet: Vec<(f32, Vec2)> = Vec::new();
    for k in 0..64 {
        let ang = k as f32 * std::f32::consts::TAU / 64.0;
        let d = Vec2::new(cb_core::math::cos(ang), cb_core::math::sin(ang));
        let mut r = 200.0;
        while r < 1800.0 {
            let p = town + d * r;
            if usable(p) && probe.wet(p) {
                wet.push((r, p));
                break;
            }
            r += 20.0;
        }
    }
    wet.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut previews = 0;
    'river: for (_, p) in wet.iter().take(12) {
        let mut cands: Vec<(f32, Vec2, Vec2)> = Vec::new();
        for d in &dirs {
            let extent = |sgn: f32| {
                let mut t = 0.0;
                while t < 160.0 && probe.wet(*p + *d * (sgn * t)) {
                    t += 4.0;
                }
                t
            };
            let (e0, e1) = (extent(-1.0), extent(1.0));
            if e0 >= 160.0 || e1 >= 160.0 {
                continue;
            }
            cands.push((e0 + e1, *d, *p + *d * (0.5 * (e1 - e0))));
        }
        cands.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (w, d, c) in cands.into_iter().take(3) {
            let half = 0.5 * w + 45.0;
            let (a, b) = (c - d * half, c + d * half);
            if !usable(a) || !usable(b) || probe.wet(a) || probe.wet(b) {
                continue;
            }
            previews += 1;
            if try_build(world, a, b, &|pv| {
                !pv.spans.is_empty() && !pv.piers.is_empty()
            }) {
                out.report.segments += 1;
                out.river_bridge = Some(Feature {
                    pos: c,
                    dir: d,
                    depth_m: 0.0,
                });
                break 'river;
            }
            if previews > 30 {
                break 'river;
            }
        }
    }

    // 2. Valley bridge with piers > 25 m: a 240–320 m line whose middle lies 28–45 m below both
    //    ends.
    let mut best: Option<(f32, Vec2, Vec2, f32)> = None;
    for gz in -36..=36 {
        for gx in -36..=36 {
            let c = town + Vec2::new(gx as f32, gz as f32) * 50.0;
            if !usable(c) {
                continue;
            }
            for d in &dirs {
                for half in [120.0f32, 160.0] {
                    let (a, b) = (c - *d * half, c + *d * half);
                    if !usable(a) || !usable(b) || probe.wet(a) || probe.wet(b) {
                        continue;
                    }
                    let ends = probe.h(a).min(probe.h(b));
                    let low = (1..16)
                        .map(|k| probe.h(a.lerp(b, k as f32 / 16.0)))
                        .fold(f32::INFINITY, f32::min);
                    let drop = ends - low;
                    let rise = (probe.h(a) - probe.h(b)).abs();
                    if (28.0..=45.0).contains(&drop) && rise < 10.0 {
                        let score =
                            c.distance(town) / 1000.0 + rise * 0.05 + (drop - 34.0).abs() * 0.02;
                        if best.is_none_or(|bb| score < bb.0) {
                            best = Some((score, c, *d, half));
                        }
                    }
                }
            }
        }
    }
    tracing::debug!("valley bridge site: {best:?}");
    if let Some((_, c, d, half)) = best {
        let (a, b) = (c - d * half, c + d * half);
        let tall = |pv: &roads::RoadPreview| {
            pv.piers
                .iter()
                .any(|p| p.height_m() > cb_network::bridge::TAPERED_PIER_M)
        };
        if try_build(world, a, b, &tall) {
            out.report.segments += 1;
            // The lowest point under the deck.
            let q = (1..40)
                .map(|k| a.lerp(b, k as f32 / 40.0))
                .min_by(|x, y| probe.h(*x).total_cmp(&probe.h(*y)))
                .unwrap_or(c);
            out.valley_bridge = Some(Feature {
                pos: q,
                dir: d,
                depth_m: 0.0,
            });
            // 3. A street along the valley floor under the deck.
            let n = d.perp();
            for half in [100.0, 80.0, 60.0] {
                if try_build(world, q - n * half, q + n * half, &|_| true) {
                    out.report.segments += 1;
                    out.overpass = Some(Feature {
                        pos: q,
                        dir: n,
                        depth_m: 0.0,
                    });
                    break;
                }
                out.report.roads_rejected += 1;
            }
        } else {
            out.report.roads_rejected += 1;
        }
    }
    // 4. A curved avenue on gentle ground with every decoration.
    let mut site: Option<(f32, Vec2)> = None;
    for gz in -24..=24 {
        for gx in -24..=24 {
            let p = town + Vec2::new(gx as f32, gz as f32) * 40.0;
            if !usable(p) {
                continue;
            }
            if let Some(sl) = probe.patch_slope(p)
                && (0.02..=0.06).contains(&sl)
            {
                let score = p.distance(town);
                if site.is_none_or(|b| score < b.0) {
                    site = Some((score, p));
                }
            }
        }
    }
    if let Some((_, p)) = site {
        let up = probe.uphill(p);
        let t = up.perp();
        let curve = CubicBezier2::from_quadratic(p - t * 90.0, p - up * 30.0, p + t * 90.0);
        let cmd = BuildRoad {
            kind: RoadKind::Avenue,
            curve,
            start: Endpoint::Free,
            end: Endpoint::Free,
        };
        let before = segments_snapshot(world);
        if roads::build_road(world, &cmd, tick).is_ok() {
            out.report.segments += 1;
            let style = cb_network::style::SegmentStyle {
                surface: cb_network::style::RoadSurface::Cobblestone,
                grass_strip: [true, true],
                tree_spacing_m: Some(12.0),
                lamp_spacing_m: Some(30.0),
            };
            for sid in segments_snapshot(world)
                .into_iter()
                .filter(|s| !before.contains(s))
            {
                let _ = roads::set_segment_style(world, sid, style);
            }
            out.decorated = Some(Feature {
                pos: curve.eval(0.5),
                dir: t,
                depth_m: 0.0,
            });
        } else {
            out.report.roads_rejected += 1;
        }
    }
    // 5. Junction showcases on flat ground, each on its own site away from the others.
    let mut taken: Vec<Vec2> = [out.river_bridge, out.valley_bridge, out.decorated]
        .iter()
        .flatten()
        .map(|f| f.pos)
        .collect();
    let mut flat: Vec<(f32, Vec2)> = Vec::new();
    for gz in -36..=36 {
        for gx in -36..=36 {
            let p = town + Vec2::new(gx as f32, gz as f32) * 40.0;
            if usable(p)
                && probe.patch_slope(p).is_some_and(|sl| sl <= 0.035)
                && [-120.0f32, 0.0, 120.0]
                    .iter()
                    .flat_map(|dx| [-120.0f32, 0.0, 120.0].map(|dz| p + Vec2::new(*dx, dz)))
                    .all(|q| usable(q) && !probe.wet(q))
            {
                flat.push((p.distance(town), p));
            }
        }
    }
    flat.sort_by(|a, b| a.0.total_cmp(&b.0));
    let site_for =
        |taken: &mut Vec<Vec2>, world: &mut World, build: &dyn Fn(&mut World, Vec2) -> bool| {
            for (_, p) in &flat {
                if taken.iter().any(|t| t.distance(*p) < 380.0) {
                    continue;
                }
                let before = segments_snapshot(world);
                if build(world, *p) {
                    taken.push(*p);
                    return Some(Feature {
                        pos: *p,
                        dir: Vec2::X,
                        depth_m: 0.0,
                    });
                }
                // Undo a partial build.
                for sid in segments_snapshot(world)
                    .into_iter()
                    .filter(|s| !before.contains(s))
                {
                    let _ = roads::remove_segment(world, sid);
                }
                taken.push(*p); // do not retry this neighbourhood
                if taken.len() > 80 {
                    break;
                }
            }
            None
        };
    let st = |world: &mut World, kind: RoadKind, a: Vec2, b: Vec2| road(world, kind, a, b);
    out.lights_x = site_for(&mut taken, world, &|w, p| {
        let ok = st(
            w,
            RoadKind::Avenue,
            p - Vec2::X * 130.0,
            p + Vec2::X * 130.0,
        ) && st(
            w,
            RoadKind::Street,
            p - Vec2::Y * 110.0,
            p + Vec2::Y * 110.0,
        );
        ok && node_at(w, p).is_some_and(|n| {
            roads::set_node_control(w, n, cb_network::control::ControlMode::Lights, &[]).is_ok()
        })
    });
    out.y30 = site_for(&mut taken, world, &|w, p| {
        let (s30, c30) = cb_core::math::sin_cos(cb_core::math::to_radians(30.0));
        st(
            w,
            RoadKind::Street,
            p - Vec2::X * 110.0,
            p + Vec2::X * 110.0,
        ) && st(w, RoadKind::Street, p, p + Vec2::new(c30, s30) * 100.0)
    });
    out.five_way = site_for(&mut taken, world, &|w, p| {
        (0..5).all(|k| {
            let (sn, cs) =
                cb_core::math::sin_cos(cb_core::math::to_radians(18.0 + 72.0 * k as f32));
            st(w, RoadKind::Street, p, p + Vec2::new(cs, sn) * 95.0)
        })
    });
    out.one_way = site_for(&mut taken, world, &|w, p| {
        st(
            w,
            RoadKind::OneWayStreet,
            p + Vec2::new(-130.0, -35.0),
            p + Vec2::new(130.0, -35.0),
        ) && st(
            w,
            RoadKind::OneWayStreet,
            p + Vec2::new(130.0, 35.0),
            p + Vec2::new(-130.0, 35.0),
        ) && st(
            w,
            RoadKind::Dirt,
            p + Vec2::new(-130.0, 105.0),
            p + Vec2::new(130.0, 105.0),
        ) && st(
            w,
            RoadKind::Street,
            p + Vec2::new(0.0, -120.0),
            p + Vec2::new(0.0, 70.0),
        )
    });
    out.roundabout = site_for(&mut taken, world, &|w, p| roundabout(w, p, 30.0, 70.0));
    out.report.segments += [
        out.lights_x,
        out.y30,
        out.five_way,
        out.one_way,
        out.roundabout,
    ]
    .iter()
    .flatten()
    .count();
    if let Some(c) = saved {
        world.insert_resource(c);
    }
    out
}
