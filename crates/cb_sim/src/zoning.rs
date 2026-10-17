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
#[derive(Component, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
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
    pub(crate) spans: Vec<FrontageSpan>,
    /// Free outline of a polygon-mode plot (empty for frontage-drag plots).
    pub(crate) outline: Vec<Vec2>,
}

impl Plot {
    /// The frontage spans it was drawn with (for edits).
    #[must_use]
    pub fn spans(&self) -> &[FrontageSpan] {
        &self.spans
    }
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
    /// The player's outline vertices (polygon mode; empty for frontage-drag plots).
    #[must_use]
    pub fn outline(&self) -> &[Vec2] {
        &self.outline
    }
}

/// A lot.
#[derive(Component, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
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
    pub(crate) inset: Vec<Vec2>,
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
    /// Fitted rectangle (residential houses, industrial halls), if one fits.
    #[must_use]
    pub fn footprint(&self) -> Option<[Vec2; 4]> {
        self.footprint
    }
    /// The lot shrunk by the zone's setbacks (empty when nothing is left).
    #[must_use]
    pub fn inset(&self) -> &[Vec2] {
        &self.inset
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
    // Civic parcels (placed buildings own their ground).
    for (_, e) in ids.buildings.iter() {
        if let Some(q) = world
            .get::<crate::buildings::Building>(e)
            .and_then(crate::buildings::Building::parcel)
            && near(&q)
        {
            out.push(to64(&q));
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
        rect: match zone {
            ZoneKind::Residential => Some(RectSpec::HOUSE),
            ZoneKind::Commercial => None,
            ZoneKind::Industrial => Some(RectSpec::HALL),
        },
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
        let inset: Vec<Vec2> = fits[k]
            .inset
            .as_ref()
            .map_or_else(Vec::new, |p| d2::to_f32(p));
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
            inset: inset.clone(),
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

/// Undo/redo: removes a plot entity only (its lots are handled by the caller).
pub(crate) fn restore_remove_plot(world: &mut World, id: PlotId) {
    if let Some(e) = world.resource_mut::<Ids>().plots.remove(id) {
        world.despawn(e);
        world.write_message(ZoningChanged::PlotRemoved(id));
    }
}

/// Undo/redo: removes a lot entity only (its building is handled by the caller).
pub(crate) fn restore_remove_lot(world: &mut World, id: LotId) {
    if let Some(e) = world.resource_mut::<Ids>().lots.remove(id) {
        world.despawn(e);
        world.write_message(ZoningChanged::LotRemoved(id));
    }
}

/// Undo/redo: puts a plot back with its original id.
pub(crate) fn restore_put_plot(world: &mut World, plot: Plot) {
    let id = plot.id;
    match world.get_resource::<Ids>().and_then(|i| i.plots.get(id)) {
        Some(e) => {
            if let Some(mut p) = world.get_mut::<Plot>(e) {
                *p = plot;
            }
        }
        None => {
            let e = world.spawn(plot).id();
            world.resource_mut::<Ids>().plots.insert(id, e);
        }
    }
    world.write_message(ZoningChanged::Plot(id));
}

/// Undo/redo: puts a lot back with its original id.
pub(crate) fn restore_put_lot(world: &mut World, lot: Lot) {
    let id = lot.id;
    match world.get_resource::<Ids>().and_then(|i| i.lots.get(id)) {
        Some(e) => {
            if let Some(mut l) = world.get_mut::<Lot>(e) {
                *l = lot;
            }
        }
        None => {
            let e = world.spawn(lot).id();
            world.resource_mut::<Ids>().lots.insert(id, e);
        }
    }
    world.write_message(ZoningChanged::Lot(id));
}

fn hint_of(p: &Plot, n: usize) -> Option<Hint> {
    (p.intervals > 0).then(|| Hint {
        split_pts: p.splits.iter().map(|v| v.as_dvec2()).collect(),
        n,
        merged_pts: p.merged.iter().map(|v| v.as_dvec2()).collect(),
    })
}

/// Validated plot geometry: frontage polyline, fronted segments, clipped polygon, depth.
type PlotGeometry = (Vec<Vec2>, SmallVec<[SegmentId; 2]>, Poly, f32);

fn plot_geometry(
    world: &World,
    zone: ZoneKind,
    spans: &[FrontageSpan],
    depth_m: f32,
    except: Option<PlotId>,
) -> Result<PlotGeometry, ValidationError> {
    let cat = world
        .get_resource::<Catalog>()
        .ok_or(ValidationError::new("internal-no-catalog"))?;
    let depth = depth_m.clamp(10.0, cat.zone(zone).d_max);
    let (frontage, segments) = frontage_polyline(world, spans)?;
    let flen: f32 = frontage.windows(2).map(|w| w[0].distance(w[1])).sum();
    if flen < MIN_FRONTAGE_M {
        return Err(ValidationError::new("plot-invalid-frontage"));
    }
    let f = Frontage::new(&[Part::Points(to64(&frontage))], f64::from(depth))
        .ok_or(ValidationError::new("plot-invalid-frontage"))?;
    let poly = clip_polygon(world, &f, f64::from(depth), except)
        .ok_or(ValidationError::new("plot-invalid-overlap"))?;
    let area = d2::area(&poly) as f32;
    if !(PLOT_AREA_M2.0..=PLOT_AREA_M2.1).contains(&area) {
        return Err(ValidationError::new("plot-invalid-size"));
    }
    Ok((frontage, segments, poly, depth))
}

/// Read-only preview of a plot (the tool ghost): its clipped polygon.
///
/// # Errors
/// The same reasons as `CreatePlot`.
pub fn preview_plot(world: &World, c: &CreatePlot) -> Result<Vec<Vec2>, ValidationError> {
    plot_geometry(world, c.zone, &c.frontage, c.depth_m, None).map(|g| d2::to_f32(&g.2))
}

/// Read-only preview of an `EditPlot` (the plot itself is excluded from the overlap clip).
///
/// # Errors
/// The same reasons as `EditPlot`.
pub fn preview_edit_plot(
    world: &World,
    id: PlotId,
    spans: &[FrontageSpan],
    depth_m: f32,
) -> Result<Vec<Vec2>, ValidationError> {
    let zone = world
        .get_resource::<Ids>()
        .and_then(|i| i.plots.get(id))
        .and_then(|e| world.get::<Plot>(e))
        .map(|p| p.zone)
        .ok_or(ValidationError::new("plot-invalid-missing"))?;
    plot_geometry(world, zone, spans, depth_m, Some(id)).map(|g| d2::to_f32(&g.2))
}

/// `EditPlot`: new frontage and depth; lots keep their IDs where their frontage intervals
/// overlap the old ones (ADR-0064 edit path).
///
/// # Errors
/// `plot-invalid-missing` or the `CreatePlot` reasons.
pub(crate) fn edit_plot(
    world: &mut World,
    id: PlotId,
    spans: &[FrontageSpan],
    depth_m: f32,
) -> Result<(), ValidationError> {
    let e = world
        .get_resource::<Ids>()
        .and_then(|i| i.plots.get(id))
        .ok_or(ValidationError::new("plot-invalid-missing"))?;
    let old = world
        .get::<Plot>(e)
        .cloned()
        .ok_or(ValidationError::new("plot-invalid-missing"))?;
    let (frontage, segments, poly, depth) =
        plot_geometry(world, old.zone, spans, depth_m, Some(id))?;
    if let Some(mut p) = world.get_mut::<Plot>(e) {
        p.frontage = frontage;
        p.segments = segments;
        p.polygon = d2::to_f32(&poly);
        p.depth_m = depth;
        p.spans = spans.to_vec();
    }
    resubdivide(world, id, hint_of(&old, old.intervals));
    Ok(())
}

/// `CreatePlot`.
///
/// # Errors
/// Invalid frontage, too small/large, or overlapping everything.
pub(crate) fn create_plot(world: &mut World, c: &CreatePlot) -> Result<PlotId, ValidationError> {
    let (frontage, segments, poly, depth) =
        plot_geometry(world, c.zone, &c.frontage, c.depth_m, None)?;
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
        spans: c.frontage.clone(),
        outline: Vec::new(),
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

/// Whether the plot's stored frontage is no longer on its road's edge (width change).
fn edge_moved(world: &World, p: &Plot) -> bool {
    let Some(sp) = p.spans.first() else {
        return false;
    };
    let Some(seg) = roads::segment(world, sp.segment) else {
        return false;
    };
    let n = p.frontage.len();
    if n < 2 {
        return false;
    }
    // A point of the first span's part (corner arcs are not on the segment edge).
    let q = (p.frontage[0] + p.frontage[1]) * 0.5;
    let (_, d) = seg.geom().project(q);
    (d.abs() - seg.geom().half_width_m).abs() > 0.05
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
        // The road edge moved (upgrade/downgrade changed the width): re-derive the frontage from
        // the spans it was drawn with; keep the stored one if that is impossible now.
        let frontage = if edge_moved(world, &p) {
            frontage_polyline(world, &p.spans)
                .map(|(f, _)| f)
                .unwrap_or_else(|_| p.frontage.clone())
        } else {
            p.frontage.clone()
        };
        let moved = frontage.len() != p.frontage.len()
            || frontage
                .iter()
                .zip(&p.frontage)
                .any(|(a, b)| a.distance(*b) > 0.01);
        let Some(f) = Frontage::new(&[Part::Points(to64(&frontage))], depth) else {
            continue;
        };
        let clipped = if p.outline.is_empty() {
            clip_polygon(world, &f, depth, Some(id))
        } else {
            polygon_outline(world, &p.outline)
                .ok()
                .and_then(|o| clip_outline(world, &o.dense, &frontage, Some(id)))
        };
        let Some(poly) = clipped.filter(|q| d2::area(q) as f32 >= PLOT_AREA_M2.0) else {
            // Nothing usable left: the plot goes away with its lots.
            let _ = delete_plot(world, id);
            continue;
        };
        let new = d2::to_f32(&poly);
        let old_area = cb_geometry::polygon::area(&p.polygon);
        if !moved && (cb_geometry::polygon::area(&new) - old_area).abs() < 0.05 {
            continue;
        }
        if let Some(mut pm) = world.get_mut::<Plot>(e) {
            pm.polygon = new;
            pm.frontage = frontage;
        }
        resubdivide(world, id, hint_of(&p, p.intervals));
    }
}

/// Polygon mode: vertices within this distance of a road edge snap onto it (m).
pub const PLOT_SNAP_M: f32 = 1.5;

/// A vertex snapped onto a road edge: segment, side, centreline arc length.
#[derive(Clone, Copy, Debug, PartialEq)]
struct EdgeHit {
    seg: SegmentId,
    left: bool,
    s: f32,
}

/// The road edge within [`PLOT_SNAP_M`] of `p` (nearest), with the snapped point.
fn nearest_road_edge(world: &World, p: Vec2) -> Option<(EdgeHit, Vec2)> {
    let ids = world.get_resource::<Ids>()?;
    let mut best: Option<(f32, EdgeHit, Vec2)> = None;
    for (sid, e) in ids.segments.iter() {
        let Some(seg) = world.get::<RoadSegment>(e) else {
            continue;
        };
        let g = seg.geom();
        let (s, d) = g.centerline.project(p);
        let off = (d.abs() - g.half_width_m).abs();
        if off > PLOT_SNAP_M || best.as_ref().is_some_and(|b| b.0 <= off) {
            continue;
        }
        let len = g.length();
        let (c0, c1) = seg.cuts();
        let s = s.clamp(c0, (len - c1).max(c0));
        let left = d > 0.0;
        let q = roads::edge_polyline(seg, left, s, s)[0];
        if q.distance(p) > PLOT_SNAP_M + 0.05 {
            continue;
        }
        best = Some((off, EdgeHit { seg: sid, left, s }, q));
    }
    best.map(|b| (b.1, b.2))
}

/// A snapped polygon-mode outline.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotOutline {
    /// The player's vertices after snapping (counter-clockwise).
    pub vertices: Vec<Vec2>,
    /// Dense outline: road-following edges expanded along the road edge.
    pub dense: Vec<Vec2>,
    /// Frontage: the longest run of road-edge outline edges, plot on its left.
    pub frontage: Vec<Vec2>,
    /// Segments along the frontage.
    pub segments: SmallVec<[SegmentId; 2]>,
}

/// Snaps polygon-mode vertices: a vertex within [`PLOT_SNAP_M`] of a road edge moves onto it;
/// an edge between two vertices on the same road edge follows the road and becomes frontage.
///
/// # Errors
/// `plot-invalid-size` (fewer than 3 vertices), `plot-invalid-self-intersection`,
/// `plot-invalid-no-frontage`.
pub fn polygon_outline(world: &World, vertices: &[Vec2]) -> Result<PlotOutline, ValidationError> {
    if vertices.len() < 3 {
        return Err(ValidationError::new("plot-invalid-size"));
    }
    let mut v = vertices.to_vec();
    if cb_geometry::polygon::signed_area(&v) < 0.0 {
        v.reverse();
    }
    let hits: Vec<Option<(EdgeHit, Vec2)>> =
        v.iter().map(|p| nearest_road_edge(world, *p)).collect();
    for (p, h) in v.iter_mut().zip(&hits) {
        if let Some((_, q)) = h {
            *p = *q;
        }
    }
    let n = v.len();
    // Dense outline with a road flag per dense edge (edge k = dense[k] → dense[k + 1]).
    let mut dense: Vec<Vec2> = Vec::new();
    let mut on_road: Vec<Option<SegmentId>> = Vec::new();
    for i in 0..n {
        let j = (i + 1) % n;
        dense.push(v[i]);
        match (&hits[i], &hits[j]) {
            (Some((a, _)), Some((b, _)))
                if a.seg == b.seg && a.left == b.left && (a.s - b.s).abs() > 0.1 =>
            {
                let seg = roads::segment(world, a.seg)
                    .ok_or(ValidationError::new("plot-invalid-frontage"))?;
                let pts = roads::edge_polyline(seg, a.left, a.s, b.s);
                for q in pts.iter().skip(1).take(pts.len().saturating_sub(2)) {
                    on_road.push(Some(a.seg));
                    dense.push(*q);
                }
                on_road.push(Some(a.seg));
            }
            _ => on_road.push(None),
        }
    }
    // Drop repeated points (snapping can merge neighbours).
    let mut keep_d: Vec<Vec2> = Vec::with_capacity(dense.len());
    let mut keep_r: Vec<Option<SegmentId>> = Vec::with_capacity(dense.len());
    for (k, p) in dense.iter().enumerate() {
        if keep_d.last().is_some_and(|l: &Vec2| l.distance(*p) < 0.01) {
            if let Some(r) = keep_r.last_mut() {
                *r = on_road[k];
            }
            continue;
        }
        keep_d.push(*p);
        keep_r.push(on_road[k]);
    }
    if keep_d.len() > 3 && keep_d[0].distance(keep_d[keep_d.len() - 1]) < 0.01 {
        keep_d.pop();
        keep_r.pop();
    }
    let (dense, on_road) = (keep_d, keep_r);
    if dense.len() < 3 || !cb_geometry::polygon::is_simple(&dense) {
        return Err(ValidationError::new("plot-invalid-self-intersection"));
    }
    // Longest run of consecutive road edges.
    let m = dense.len();
    let Some(k0) = (0..m).find(|k| on_road[*k].is_none()) else {
        return Err(ValidationError::new("plot-invalid-self-intersection"));
    };
    let mut best: (f32, Vec<Vec2>, SmallVec<[SegmentId; 2]>) = (0.0, Vec::new(), SmallVec::new());
    let mut cur: (f32, Vec<Vec2>, SmallVec<[SegmentId; 2]>) = (0.0, Vec::new(), SmallVec::new());
    for t in 1..=m {
        let k = (k0 + t) % m;
        match on_road[k] {
            Some(seg) => {
                let (a, b) = (dense[k], dense[(k + 1) % m]);
                if cur.1.is_empty() {
                    cur.1.push(a);
                }
                cur.1.push(b);
                cur.0 += a.distance(b);
                if !cur.2.contains(&seg) {
                    cur.2.push(seg);
                }
            }
            None => {
                if cur.0 > best.0 {
                    best = std::mem::take(&mut cur);
                }
                cur = (0.0, Vec::new(), SmallVec::new());
            }
        }
    }
    if cur.0 > best.0 {
        best = cur;
    }
    if best.0 < MIN_FRONTAGE_M {
        return Err(ValidationError::new("plot-invalid-no-frontage"));
    }
    Ok(PlotOutline {
        vertices: v,
        dense,
        frontage: best.1,
        segments: best.2,
    })
}

/// Auto-clip of a polygon outline: minus roads, junctions, other plots and civic parcels; the
/// component that holds the frontage midpoint.
fn clip_outline(
    world: &World,
    dense: &[Vec2],
    frontage: &[Vec2],
    except: Option<PlotId>,
) -> Option<Poly> {
    let (lo, hi) = cb_geometry::polygon::bounds(dense);
    let obs = obstacles(world, lo - Vec2::splat(1.0), hi + Vec2::splat(1.0), except);
    let poly = to64(dense);
    let pieces = if obs.is_empty() {
        vec![poly]
    } else {
        f64ops::difference(&poly, &obs)
    };
    // Probe just inside the frontage midpoint (plot on the left of the frontage).
    let len: f32 = frontage.windows(2).map(|w| w[0].distance(w[1])).sum();
    let mut acc = 0.0;
    let mut probe = None;
    for w in frontage.windows(2) {
        let l = w[0].distance(w[1]);
        if acc + l >= 0.5 * len && l > 1e-4 {
            let t = (0.5 * len - acc) / l;
            let d = (w[1] - w[0]) / l;
            probe = Some(w[0].lerp(w[1], t) + d.perp() * 0.1);
            break;
        }
        acc += l;
    }
    let probe = probe?.as_dvec2();
    pieces
        .into_iter()
        .find(|p| d2::point_in_poly(probe, p))
        .map(|p| d2::simplify(&p, 0.002))
}

/// Validated polygon-mode geometry.
fn polygon_geometry(
    world: &World,
    vertices: &[Vec2],
    except: Option<PlotId>,
) -> Result<(PlotOutline, Poly, f32), ValidationError> {
    let o = polygon_outline(world, vertices)?;
    let poly = clip_outline(world, &o.dense, &o.frontage, except)
        .ok_or(ValidationError::new("plot-invalid-overlap"))?;
    let area = d2::area(&poly) as f32;
    if !(PLOT_AREA_M2.0..=PLOT_AREA_M2.1).contains(&area) {
        return Err(ValidationError::new("plot-invalid-size"));
    }
    let f = cb_geometry::polyline::Polyline2::new(o.frontage.iter().copied());
    let depth = d2::to_f32(&poly)
        .iter()
        .map(|q| f.project(*q).1.abs())
        .fold(10.0f32, f32::max);
    Ok((o, poly, depth))
}

/// Read-only preview of a polygon-mode plot (or edit, with `except`): its clipped polygon.
///
/// # Errors
/// The `CreatePlotPolygon` reasons.
pub fn preview_plot_polygon(
    world: &World,
    vertices: &[Vec2],
    except: Option<PlotId>,
) -> Result<Vec<Vec2>, ValidationError> {
    polygon_geometry(world, vertices, except).map(|g| d2::to_f32(&g.1))
}

/// `CreatePlotPolygon`.
///
/// # Errors
/// `plot-invalid-size`, `-self-intersection`, `-no-frontage`, `-overlap`.
pub(crate) fn create_plot_polygon(
    world: &mut World,
    zone: ZoneKind,
    vertices: &[Vec2],
    dn: i8,
) -> Result<PlotId, ValidationError> {
    let (o, poly, depth) = polygon_geometry(world, vertices, None)?;
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
        zone,
        frontage: o.frontage,
        depth_m: depth,
        polygon: d2::to_f32(&poly),
        dn,
        segments: o.segments,
        splits: Vec::new(),
        merged: Vec::new(),
        intervals: 0,
        lots: Vec::new(),
        seed,
        spans: Vec::new(),
        outline: o.vertices,
    };
    let e = world.spawn(plot).id();
    world.resource_mut::<Ids>().plots.insert(id, e);
    resubdivide(world, id, None);
    Ok(id)
}

/// `EditPlotPolygon`: a new outline (vertex or edge drag); lots keep their IDs where their
/// frontage intervals overlap the old ones. Frontage-drag plots become polygon plots.
///
/// # Errors
/// `plot-invalid-missing` or the `CreatePlotPolygon` reasons.
pub(crate) fn edit_plot_polygon(
    world: &mut World,
    id: PlotId,
    vertices: &[Vec2],
) -> Result<(), ValidationError> {
    let e = world
        .get_resource::<Ids>()
        .and_then(|i| i.plots.get(id))
        .ok_or(ValidationError::new("plot-invalid-missing"))?;
    let old = world
        .get::<Plot>(e)
        .cloned()
        .ok_or(ValidationError::new("plot-invalid-missing"))?;
    let (o, poly, depth) = polygon_geometry(world, vertices, Some(id))?;
    if let Some(mut p) = world.get_mut::<Plot>(e) {
        p.frontage = o.frontage;
        p.segments = o.segments;
        p.polygon = d2::to_f32(&poly);
        p.depth_m = depth;
        p.spans = Vec::new();
        p.outline = o.vertices;
    }
    resubdivide(world, id, hint_of(&old, old.intervals));
    Ok(())
}
