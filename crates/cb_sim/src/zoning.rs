//! Plots and lots (DATA_MODEL §5, ALGORITHMS §9–§10, ADR-0064). A plot keeps its frontage as the
//! world polyline of the road edge it was drawn on (incl. a junction corner fillet), its depth,
//! and the persisted subdivision state (pre-repair split points, removed splits, interval
//! count); its polygon and lots are derived. New roads mark nearby plots dirty; `EditSet::Zoning`
//! re-clips them and re-subdivides with the previous state as hint so lot IDs stay stable.

use bevy::ecs::component::Component;
use bevy::ecs::message::Message;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use cb_core::hash::{Domain, derive_seed};
use cb_core::ids::{LotId, PlotId, SegmentId};
use cb_geometry::boolean::f64ops;
use cb_geometry::d2::{self, Poly};
use cb_geometry::frontage::{Frontage, Part};
use cb_geometry::rect_fit::{self, EdgeKind, FitParams, RectSpec};
use cb_geometry::subdivide::{self, Hint};
use glam::{DVec2, Vec2};
use smallvec::SmallVec;

use crate::catalog::{Catalog, ZoneKind};
use crate::commands::{CreatePlot, FrontageSpan, ValidationError};
use crate::config::WorldSeed;
use crate::ids::Ids;
use crate::roads::{self, RoadNode, RoadSegment};

/// Minimum frontage length of a plot (m).
pub const MIN_FRONTAGE_M: f32 = 8.0;
/// Plot area bounds (m²).
pub const PLOT_AREA_M2: (f32, f32) = (100.0, 40_000.0);
/// Corner joining tolerance (m).
const JOIN_TOL_M: f32 = 1.0;

/// A plot.
#[derive(Component, Clone, Debug)]
pub struct Plot {
    pub(crate) id: PlotId,
    pub(crate) zone: ZoneKind,
    pub(crate) frontage: Vec<Vec2>,
    pub(crate) depth_m: f32,
    pub(crate) polygon: Vec<Vec2>,
    pub(crate) dn: i8,
    pub(crate) segments: SmallVec<[SegmentId; 2]>,
    pub(crate) splits: Vec<Vec2>,
    pub(crate) merged: Vec<Vec2>,
    pub(crate) intervals: usize,
    pub(crate) lots: Vec<LotId>,
    pub(crate) seed: u64,
}

impl Plot {
    /// Stable id.
    #[must_use]
    pub fn id(&self) -> PlotId {
        self.id
    }
    /// Zone.
    #[must_use]
    pub fn zone(&self) -> ZoneKind {
        self.zone
    }
    /// Frontage polyline (plot on its left).
    #[must_use]
    pub fn frontage(&self) -> &[Vec2] {
        &self.frontage
    }
    /// Depth (m).
    #[must_use]
    pub fn depth_m(&self) -> f32 {
        self.depth_m
    }
    /// Current (clipped) polygon, counter-clockwise.
    #[must_use]
    pub fn polygon(&self) -> &[Vec2] {
        &self.polygon
    }
    /// Lots in frontage order.
    #[must_use]
    pub fn lots(&self) -> &[LotId] {
        &self.lots
    }
    /// Lot count offset from automatic.
    #[must_use]
    pub fn dn(&self) -> i8 {
        self.dn
    }
    /// Segments the frontage was drawn on.
    #[must_use]
    pub fn segments(&self) -> &[SegmentId] {
        &self.segments
    }
}

/// A lot.
#[derive(Component, Clone, Debug)]
pub struct Lot {
    pub(crate) id: LotId,
    pub(crate) plot: PlotId,
    pub(crate) polygon: Vec<Vec2>,
    pub(crate) area_m2: f32,
    pub(crate) front: (Vec2, Vec2),
    pub(crate) frontage_len_m: f32,
    pub(crate) corner: bool,
    pub(crate) edges: Vec<EdgeKind>,
    pub(crate) footprint: Option<[Vec2; 4]>,
    pub(crate) building: Option<cb_core::ids::BuildingId>,
    pub(crate) created_tick: u64,
}

impl Lot {
    /// Stable id.
    #[must_use]
    pub fn id(&self) -> LotId {
        self.id
    }
    /// Owning plot.
    #[must_use]
    pub fn plot(&self) -> PlotId {
        self.plot
    }
    /// Polygon (counter-clockwise).
    #[must_use]
    pub fn polygon(&self) -> &[Vec2] {
        &self.polygon
    }
    /// Area (m²).
    #[must_use]
    pub fn area_m2(&self) -> f32 {
        self.area_m2
    }
    /// Frontage interval ends (world points at s0 and s1).
    #[must_use]
    pub fn front(&self) -> (Vec2, Vec2) {
        self.front
    }
    /// Frontage length (m).
    #[must_use]
    pub fn frontage_len_m(&self) -> f32 {
        self.frontage_len_m
    }
    /// Corner lot.
    #[must_use]
    pub fn corner(&self) -> bool {
        self.corner
    }
    /// Edge classes (`edge k` = vertex k → k+1).
    #[must_use]
    pub fn edges(&self) -> &[EdgeKind] {
        &self.edges
    }
    /// Fitted house footprint (residential), if one fits.
    #[must_use]
    pub fn footprint(&self) -> Option<[Vec2; 4]> {
        self.footprint
    }
    /// Building on the lot.
    #[must_use]
    pub fn building(&self) -> Option<cb_core::ids::BuildingId> {
        self.building
    }
}

/// Plot/lot changes for view sync.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoningChanged {
    /// A plot was created or re-subdivided.
    Plot(PlotId),
    /// A plot was removed.
    PlotRemoved(PlotId),
    /// A lot was created or changed.
    Lot(LotId),
    /// A lot was removed.
    LotRemoved(LotId),
}

/// Plots to re-clip in `EditSet::Zoning`.
#[derive(Resource, Default, Debug)]
pub struct DirtyPlots(pub(crate) Vec<PlotId>);

fn to64(p: &[Vec2]) -> Poly {
    p.iter().map(|v| v.as_dvec2()).collect()
}

/// Frontage polyline from spans (two spans are joined around the junction corner between them).
fn frontage_polyline(
    world: &World,
    spans: &[FrontageSpan],
) -> Result<(Vec<Vec2>, SmallVec<[SegmentId; 2]>), ValidationError> {
    if spans.is_empty() || spans.len() > 2 {
        return Err(ValidationError::new("plot-invalid-frontage"));
    }
    let mut parts: Vec<Vec<Vec2>> = Vec::new();
    let mut segs: SmallVec<[SegmentId; 2]> = SmallVec::new();
    for sp in spans {
        let seg = roads::segment(world, sp.segment)
            .ok_or(ValidationError::new("plot-invalid-frontage"))?;
        let len = seg.geom().length();
        let (c0, c1) = seg.cuts();
        // Plot on the left of the walking direction: left side walks forward, right side back.
        let (mut s0, mut s1) = (sp.s0.clamp(c0, len - c1), sp.s1.clamp(c0, len - c1));
        if sp.left == (s0 > s1) {
            std::mem::swap(&mut s0, &mut s1);
        }
        if (s1 - s0).abs() < 0.5 {
            return Err(ValidationError::new("plot-invalid-frontage"));
        }
        parts.push(roads::edge_polyline(seg, sp.left, s0, s1));
        segs.push(sp.segment);
    }
    let mut out = parts[0].clone();
    if parts.len() == 2 {
        let (end_a, start_b) = (*out.last().unwrap_or(&Vec2::ZERO), parts[1][0]);
        let corner = find_corner(world, end_a, start_b)
            .ok_or(ValidationError::at("plot-invalid-corner", end_a))?;
        out.pop();
        out.extend(corner);
        out.extend(parts[1].iter().skip(1));
    }
    Ok((out, segs))
}

/// The junction corner whose outer kerb runs from near `a` to near `b` (either direction).
fn find_corner(world: &World, a: Vec2, b: Vec2) -> Option<Vec<Vec2>> {
    let ids = world.get_resource::<Ids>()?;
    for (_, e) in ids.nodes.iter() {
        let Some(j) = world.get::<RoadNode>(e).and_then(RoadNode::junction) else {
            continue;
        };
        for c in &j.corners {
            let pts: Vec<Vec2> = c.outer.iter().map(|v| Vec2::new(v.x, v.z)).collect();
            let (f, l) = (pts[0], pts[pts.len() - 1]);
            if f.distance(b) < JOIN_TOL_M && l.distance(a) < JOIN_TOL_M {
                return Some(pts.into_iter().rev().collect());
            }
            if f.distance(a) < JOIN_TOL_M && l.distance(b) < JOIN_TOL_M {
                return Some(pts);
            }
        }
    }
    None
}

/// Polygons that plots may not overlap near `bbox`: road corridors, junctions, other plots.
fn obstacles(world: &World, lo: Vec2, hi: Vec2, except: Option<PlotId>) -> Vec<Poly> {
    let Some(ids) = world.get_resource::<Ids>() else {
        return Vec::new();
    };
    let near = |q: &[Vec2]| {
        let (a, b) = cb_geometry::polygon::bounds(q);
        a.x <= hi.x && lo.x <= b.x && a.y <= hi.y && lo.y <= b.y
    };
    let mut out = Vec::new();
    for (_, e) in ids.segments.iter() {
        if let Some(s) = world.get::<RoadSegment>(e)
            && near(&s.geom().corridor)
        {
            out.push(to64(&s.geom().corridor));
        }
    }
    for (_, e) in ids.nodes.iter() {
        if let Some(j) = world.get::<RoadNode>(e).and_then(RoadNode::junction) {
            let p: Vec<Vec2> = j.outer.iter().map(|v| Vec2::new(v.x, v.z)).collect();
            if near(&p) {
                out.push(to64(&p));
            }
        }
    }
    for (id, e) in ids.plots.iter() {
        if Some(id) != except
            && let Some(p) = world.get::<Plot>(e)
            && near(&p.polygon)
        {
            out.push(to64(&p.polygon));
        }
    }
    out
}

/// Plot polygon: offset band of the frontage minus obstacles, the component holding the
/// frontage midpoint.
fn clip_polygon(world: &World, f: &Frontage, depth: f64, except: Option<PlotId>) -> Option<Poly> {
    let band = f.plot_polygon(depth, 12.0)?;
    let (lo, hi) = d2::bbox(&band);
    let obs = obstacles(
        world,
        lo.as_vec2() - Vec2::splat(1.0),
        hi.as_vec2() + Vec2::splat(1.0),
        except,
    );
    let pieces = if obs.is_empty() {
        vec![band]
    } else {
        f64ops::difference(&band, &obs)
    };
    let mid = 0.5 * f.len();
    let probe = f.point_at(mid) + f.normal_at(mid) * 0.1;
    pieces
        .into_iter()
        .find(|p| d2::point_in_poly(probe, p))
        .map(|p| d2::simplify(&p, 0.002))
}

fn fit_params(cat: &Catalog, zone: ZoneKind) -> FitParams {
    let z = cat.zone(zone).params();
    FitParams {
        setback: z.setback,
        rect: (zone == ZoneKind::Residential).then_some(RectSpec::HOUSE),
    }
}

/// Subdivides a plot (fresh or with its previous state) and syncs the lot entities.
fn resubdivide(world: &mut World, plot_id: PlotId, hint: Option<Hint>) {
    let Some(pe) = world
        .get_resource::<Ids>()
        .and_then(|i| i.plots.get(plot_id))
    else {
        return;
    };
    let Some(plot) = world.get::<Plot>(pe).cloned() else {
        return;
    };
    let Some(cat) = world.get_resource::<Catalog>().cloned() else {
        return;
    };
    let tick = world
        .get_resource::<crate::SimClock>()
        .map_or(0, |c| c.tick().0);
    let depth = f64::from(plot.depth_m);
    let Some(f) = Frontage::new(&[Part::Points(to64(&plot.frontage))], depth) else {
        return;
    };
    let zp = cat.zone(plot.zone).params();
    let out = subdivide::subdivide(
        &f,
        &to64(&plot.polygon),
        &zp,
        depth,
        i32::from(plot.dn),
        plot.seed,
        hint.as_ref(),
    );
    let fits = rect_fit::fit_lots(&out.lots, &f, &fit_params(&cat, plot.zone));
    // Stable IDs: match new lots to the old frontage intervals.
    let old: Vec<(LotId, (DVec2, DVec2))> = plot
        .lots
        .iter()
        .filter_map(|id| {
            let e = world.get_resource::<Ids>()?.lots.get(*id)?;
            world
                .get::<Lot>(e)
                .map(|l| (*id, (l.front.0.as_dvec2(), l.front.1.as_dvec2())))
        })
        .collect();
    let ends: Vec<(DVec2, DVec2)> = old.iter().map(|o| o.1).collect();
    let matched = subdivide::match_ids(&f, &ends, &out.lots);
    let mut keep: Vec<LotId> = Vec::new();
    let mut new_lots: Vec<LotId> = Vec::new();
    for (k, l) in out.lots.iter().enumerate() {
        let poly = d2::to_f32(&l.poly);
        let footprint = fits[k].footprint.map(|q| q.map(DVec2::as_vec2));
        let mk = |id: LotId, building: Option<cb_core::ids::BuildingId>, created_tick: u64| Lot {
            id,
            plot: plot_id,
            polygon: poly.clone(),
            area_m2: l.area as f32,
            front: (f.point_at(l.s0).as_vec2(), f.point_at(l.s1).as_vec2()),
            frontage_len_m: l.frontage_len as f32,
            corner: l.corner,
            edges: rect_fit::lot_edge_kinds(l, &f),
            footprint,
            building,
            created_tick,
        };
        let id = match matched[k].map(|i| old[i].0) {
            Some(id) => {
                keep.push(id);
                if let Some(e) = world.get_resource::<Ids>().and_then(|i| i.lots.get(id))
                    && let Some(mut cur) = world.get_mut::<Lot>(e)
                {
                    let (building, created) = (cur.building, cur.created_tick);
                    *cur = mk(id, building, created);
                }
                id
            }
            None => {
                let Ok(id) = world.resource_mut::<Ids>().lots.allocate() else {
                    continue;
                };
                let e = world.spawn(mk(id, None, tick)).id();
                world.resource_mut::<Ids>().lots.insert(id, e);
                id
            }
        };
        new_lots.push(id);
        world.write_message(ZoningChanged::Lot(id));
    }
    for (id, _) in &old {
        if !keep.contains(id) {
            remove_lot(world, *id);
        }
    }
    // Buildings whose footprint no longer fits their (changed) lot are removed.
    for id in &keep {
        crate::buildings::check_fit(world, *id);
    }
    if let Some(mut p) = world.get_mut::<Plot>(pe) {
        p.splits = out.splits.iter().map(|s| s.1.as_vec2()).collect();
        p.merged = out.merged_pts.iter().map(|v| v.as_vec2()).collect();
        p.intervals = out.intervals();
        p.lots = new_lots;
    }
    world.write_message(ZoningChanged::Plot(plot_id));
}

fn remove_lot(world: &mut World, id: LotId) {
    let Some(e) = world.resource_mut::<Ids>().lots.remove(id) else {
        return;
    };
    if let Some(b) = world.get::<Lot>(e).and_then(|l| l.building) {
        crate::buildings::remove_building(world, b);
    }
    world.despawn(e);
    world.write_message(ZoningChanged::LotRemoved(id));
}

fn hint_of(p: &Plot, n: usize) -> Option<Hint> {
    (p.intervals > 0).then(|| Hint {
        split_pts: p.splits.iter().map(|v| v.as_dvec2()).collect(),
        n,
        merged_pts: p.merged.iter().map(|v| v.as_dvec2()).collect(),
    })
}

/// `CreatePlot`.
///
/// # Errors
/// Invalid frontage, too small/large, or overlapping everything.
pub(crate) fn create_plot(world: &mut World, c: &CreatePlot) -> Result<PlotId, ValidationError> {
    let cat = world
        .get_resource::<Catalog>()
        .cloned()
        .ok_or(ValidationError::new("internal-no-catalog"))?;
    let spec = *cat.zone(c.zone);
    let depth = c.depth_m.clamp(10.0, spec.d_max);
    let (frontage, segments) = frontage_polyline(world, &c.frontage)?;
    let flen: f32 = frontage.windows(2).map(|w| w[0].distance(w[1])).sum();
    if flen < MIN_FRONTAGE_M {
        return Err(ValidationError::new("plot-invalid-frontage"));
    }
    let f = Frontage::new(&[Part::Points(to64(&frontage))], f64::from(depth))
        .ok_or(ValidationError::new("plot-invalid-frontage"))?;
    let poly = clip_polygon(world, &f, f64::from(depth), None)
        .ok_or(ValidationError::new("plot-invalid-overlap"))?;
    let area = d2::area(&poly) as f32;
    if !(PLOT_AREA_M2.0..=PLOT_AREA_M2.1).contains(&area) {
        return Err(ValidationError::new("plot-invalid-size"));
    }
    let id = world
        .resource_mut::<Ids>()
        .plots
        .allocate()
        .map_err(|_| ValidationError::new("internal-id-space"))?;
    let seed = derive_seed(
        world.get_resource::<WorldSeed>().map_or(0, |s| s.0),
        Domain::Plot,
        u64::from(id.get()),
        0,
    );
    let plot = Plot {
        id,
        zone: c.zone,
        frontage,
        depth_m: depth,
        polygon: d2::to_f32(&poly),
        dn: c.dn,
        segments,
        splits: Vec::new(),
        merged: Vec::new(),
        intervals: 0,
        lots: Vec::new(),
        seed,
    };
    let e = world.spawn(plot).id();
    world.resource_mut::<Ids>().plots.insert(id, e);
    resubdivide(world, id, None);
    Ok(id)
}

/// `DeletePlot`.
///
/// # Errors
/// `plot-invalid-missing`.
pub(crate) fn delete_plot(world: &mut World, id: PlotId) -> Result<(), ValidationError> {
    let e = world
        .resource_mut::<Ids>()
        .plots
        .remove(id)
        .ok_or(ValidationError::new("plot-invalid-missing"))?;
    let lots = world
        .get::<Plot>(e)
        .map(|p| p.lots.clone())
        .unwrap_or_default();
    for l in lots {
        remove_lot(world, l);
    }
    world.despawn(e);
    world.write_message(ZoningChanged::PlotRemoved(id));
    Ok(())
}

/// `SetSubdivision`: new lot-count offset; ±1 edits keep the most lot IDs.
///
/// # Errors
/// `plot-invalid-missing`, or `plot-invalid-subdivision` when the count does not fit.
pub(crate) fn set_subdivision(
    world: &mut World,
    id: PlotId,
    dn: i8,
) -> Result<(), ValidationError> {
    let e = world
        .get_resource::<Ids>()
        .and_then(|i| i.plots.get(id))
        .ok_or(ValidationError::new("plot-invalid-missing"))?;
    let p = world
        .get::<Plot>(e)
        .cloned()
        .ok_or(ValidationError::new("plot-invalid-missing"))?;
    let delta = i32::from(dn) - i32::from(p.dn);
    let n = (p.intervals as i32 + delta).max(1) as usize;
    if let Some(mut pm) = world.get_mut::<Plot>(e) {
        pm.dn = dn;
    }
    resubdivide(world, id, hint_of(&p, n));
    Ok(())
}

/// Marks plots whose polygon overlaps `[lo, hi]` for re-clipping.
pub(crate) fn mark_near(world: &mut World, lo: Vec2, hi: Vec2) {
    let Some(ids) = world.get_resource::<Ids>() else {
        return;
    };
    let hits: Vec<PlotId> = ids
        .plots
        .iter()
        .filter(|(_, e)| {
            world.get::<Plot>(*e).is_some_and(|p| {
                let (a, b) = cb_geometry::polygon::bounds(&p.polygon);
                a.x <= hi.x && lo.x <= b.x && a.y <= hi.y && lo.y <= b.y
            })
        })
        .map(|(id, _)| id)
        .collect();
    if let Some(mut d) = world.get_resource_mut::<DirtyPlots>() {
        d.0.extend(hits);
    }
}

/// `EditSet::Zoning`: re-clips dirty plots; changed ones re-subdivide with their hint.
pub(crate) fn update_dirty_plots(world: &mut World) {
    let mut dirty = match world.get_resource_mut::<DirtyPlots>() {
        Some(mut d) if !d.0.is_empty() => std::mem::take(&mut d.0),
        _ => return,
    };
    dirty.sort();
    dirty.dedup();
    for id in dirty {
        let Some(e) = world.get_resource::<Ids>().and_then(|i| i.plots.get(id)) else {
            continue;
        };
        let Some(p) = world.get::<Plot>(e).cloned() else {
            continue;
        };
        let depth = f64::from(p.depth_m);
        let Some(f) = Frontage::new(&[Part::Points(to64(&p.frontage))], depth) else {
            continue;
        };
        let clipped = clip_polygon(world, &f, depth, Some(id));
        let Some(poly) = clipped.filter(|q| d2::area(q) as f32 >= PLOT_AREA_M2.0) else {
            // Nothing usable left: the plot goes away with its lots.
            let _ = delete_plot(world, id);
            continue;
        };
        let new = d2::to_f32(&poly);
        let old_area = cb_geometry::polygon::area(&p.polygon);
        if (cb_geometry::polygon::area(&new) - old_area).abs() < 0.05 {
            continue;
        }
        if let Some(mut pm) = world.get_mut::<Plot>(e) {
            pm.polygon = new;
        }
        resubdivide(world, id, hint_of(&p, p.intervals));
    }
}
