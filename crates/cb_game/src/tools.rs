//! Player tools (GAME_DESIGN §12, ALGORITHMS §1, §9): road (straight, curved, freehand), plot
//! (frontage drag, edit handles, sub-plot ±, depth), bulldoze; the toolbar and the cursor label.
//! Tools only emit `PlayerCommand`s; previews call the sim's read-only planning functions so the
//! ghost always agrees with what the command will do.

use bevy::asset::RenderAssetUsages;
use bevy::input::keyboard::Key;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use cb_core::ids::{PlotId, SegmentId};
use cb_geometry::CubicBezier2;
use cb_l10n::{Arg, Key as LKey, format, keys};
use cb_network::{RoadKind, SegmentGeom};
use cb_sim::catalog::{Catalog, ZoneKind};
use cb_sim::commands::{
    BuildRoad, CommandQueue, CommandResult, CreatePlot, Endpoint, FrontageSpan, PlayerCommand,
    ValidationError,
};
use cb_sim::economy::Treasury;
use cb_sim::ids::Ids;
use cb_sim::roads::{RoadNode, RoadPreview, RoadSegment};
use cb_sim::terrain::TerrainRes;
use cb_sim::zoning::Plot;

use crate::camera::{PointerOverUi, ToolOwnsDrag};
use crate::l10n::Localization;
use crate::picking::GroundPick;

/// Node snapping radius (m).
pub const SNAP_NODE_M: f32 = 10.0;
/// Freehand stroke sampling (m).
pub const STROKE_STEP_M: f32 = 4.0;
/// Freehand fit tolerance (m).
pub const STROKE_FIT_TOL_M: f32 = 2.0;
/// Plot handle pick radius (m).
pub const HANDLE_PICK_M: f32 = 4.0;

/// Road drawing modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RoadMode {
    /// Two clicks.
    #[default]
    Straight,
    /// Start, control point, end.
    Curved,
    /// Drag a stroke; fitted with Béziers on release.
    Freehand,
}

/// Road tool state.
#[derive(Clone, Debug, Default)]
pub struct RoadTool {
    /// Road type.
    pub kind: RoadKindSel,
    /// Mode.
    pub mode: RoadMode,
    /// Clicked points (start, control).
    pub points: Vec<Vec2>,
    /// Freehand stroke.
    pub stroke: Vec<Vec2>,
    /// The command the preview is for.
    pub pending: Option<BuildRoad>,
    /// Preview result.
    pub preview: Option<Result<RoadPreview, ValidationError>>,
}

/// Road kind selection (wrapper with a default).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoadKindSel(pub RoadKind);

impl Default for RoadKindSel {
    fn default() -> Self {
        Self(RoadKind::Street)
    }
}

/// A plot-tool drag along a road edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeDrag {
    /// Segment.
    pub segment: SegmentId,
    /// Side.
    pub left: bool,
    /// Start arc length.
    pub s0: f32,
    /// Current arc length.
    pub s1: f32,
}

/// What a plot-tool drag edits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlotDrag {
    /// Drawing a new plot.
    New(EdgeDrag),
    /// Moving one frontage end of the selected plot (`end` = 0 start, 1 end).
    End {
        /// Which end.
        end: usize,
        /// Current arc length.
        s: f32,
    },
    /// Dragging the depth handle.
    Depth(f32),
}

/// Plot tool state.
#[derive(Clone, Debug)]
pub struct PlotTool {
    /// Zone.
    pub zone: ZoneKind,
    /// Depth for new plots (m).
    pub depth_m: f32,
    /// Current drag.
    pub drag: Option<PlotDrag>,
    /// Selected plot (edit handles and ± panel).
    pub selected: Option<PlotId>,
    /// Ghost of the plot being drawn or edited.
    pub preview: Option<Result<Vec<Vec2>, ValidationError>>,
}

impl Default for PlotTool {
    fn default() -> Self {
        Self {
            zone: ZoneKind::Residential,
            depth_m: 30.0,
            drag: None,
            selected: None,
            preview: None,
        }
    }
}

/// The active tool.
#[derive(Resource, Clone, Debug, Default)]
pub enum ActiveTool {
    /// Camera only.
    #[default]
    None,
    /// Road tool.
    Road(Box<RoadTool>),
    /// Plot tool.
    Plot(PlotTool),
    /// Bulldozer.
    Bulldoze,
}

/// Zoning view toggle (plots and lot lines; always on with the plot tool).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ZoningView(pub bool);

/// Whether the game UI is drawn (screenshot shots without UI turn it off).
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiVisible(pub bool);

impl Default for UiVisible {
    fn default() -> Self {
        Self(true)
    }
}

/// Last rejection shown as a toast.
#[derive(Resource, Clone, Debug, Default)]
pub struct Toast {
    text: Option<(String, f32)>,
}

/// Tools plugin.
pub struct ToolsPlugin;

impl Plugin for ToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveTool>()
            .init_resource::<ZoningView>()
            .init_resource::<Toast>()
            .init_resource::<UiVisible>()
            .add_systems(Startup, setup_ghost)
            .add_systems(
                Update,
                (
                    tool_keys,
                    tool_input,
                    tool_preview,
                    update_ghost,
                    draw_gizmos,
                )
                    .chain()
                    .in_set(crate::app::GameSet::Input)
                    .after(crate::camera::CameraSystems)
                    .after(crate::picking::PickingSystems),
            )
            .add_systems(
                Update,
                collect_results.in_set(crate::app::GameSet::Presentation),
            )
            .add_systems(EguiPrimaryContextPass, toolbar_ui);
    }
}

fn tool_keys(
    keys: Res<ButtonInput<KeyCode>>,
    logical: Res<ButtonInput<Key>>,
    mut tool: ResMut<ActiveTool>,
    mut zoning: ResMut<ZoningView>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        match &mut *tool {
            ActiveTool::Road(r) if !r.points.is_empty() || !r.stroke.is_empty() => {
                r.points.clear();
                r.stroke.clear();
                r.preview = None;
            }
            ActiveTool::Plot(p) if p.drag.is_some() || p.selected.is_some() => {
                p.drag = None;
                p.selected = None;
                p.preview = None;
            }
            _ => *tool = ActiveTool::None,
        }
    }
    // Mnemonic shortcuts bind logical keys (CLAUDE.md platform rules).
    let ch = |c: &str| logical.just_pressed(Key::Character(c.into()));
    if ch("r") {
        *tool = ActiveTool::Road(Box::default());
    } else if ch("z") {
        *tool = ActiveTool::Plot(PlotTool::default());
    } else if ch("b") {
        *tool = ActiveTool::Bulldoze;
    } else if ch("v") {
        zoning.0 = !zoning.0;
    }
}

/// Nearest node within the snap radius, else the nearest point on a segment centreline within
/// its half width, else the point itself.
fn snap(p: Vec2, nodes: &Query<&RoadNode>, segs: &Query<&RoadSegment>) -> Vec2 {
    let mut best: Option<(f32, Vec2)> = None;
    for n in nodes {
        let q = Vec2::new(n.pos().x, n.pos().z);
        let d = q.distance(p);
        if d < SNAP_NODE_M && best.is_none_or(|b| d < b.0) {
            best = Some((d, q));
        }
    }
    if let Some((_, q)) = best {
        return q;
    }
    for s in segs {
        let g = s.geom();
        let (sp, d) = g.project(p);
        if d.abs() < g.half_width_m + 1.0 && sp > 0.5 && sp < g.length() - 0.5 {
            return g.point_at(sp);
        }
    }
    p
}

/// The road edge under `p`: segment, side, arc length.
fn edge_under(p: Vec2, segs: &Query<&RoadSegment>) -> Option<(SegmentId, bool, f32)> {
    let mut best: Option<(f32, SegmentId, bool, f32)> = None;
    for s in segs {
        let g = s.geom();
        let (sp, d) = g.project(p);
        let off = d.abs() - g.half_width_m;
        if (-1.5..=8.0).contains(&off)
            && sp >= 0.0
            && sp <= g.length()
            && best.is_none_or(|b| off.abs() < b.0)
        {
            best = Some((off.abs(), s.id(), d > 0.0, sp));
        }
    }
    best.map(|b| (b.1, b.2, b.3))
}

fn span_of(d: &EdgeDrag) -> FrontageSpan {
    FrontageSpan {
        segment: d.segment,
        left: d.left,
        s0: d.s0,
        s1: d.s1,
    }
}

#[allow(clippy::too_many_arguments, reason = "Bevy system parameters")]
fn tool_input(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    pick: Res<GroundPick>,
    over_ui: Res<PointerOverUi>,
    mut tool: ResMut<ActiveTool>,
    mut owns: ResMut<ToolOwnsDrag>,
    mut queue: ResMut<CommandQueue>,
    nodes: Query<&RoadNode>,
    segs: Query<&RoadSegment>,
    plots: Query<&Plot>,
    ids: Option<Res<Ids>>,
) {
    owns.0 = matches!(&*tool, ActiveTool::Plot(_))
        || matches!(&*tool, ActiveTool::Road(r) if r.mode == RoadMode::Freehand);
    if over_ui.0 {
        return;
    }
    let Some(p3) = pick.point else { return };
    let p = Vec2::new(p3.x, p3.z);
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    match &mut *tool {
        ActiveTool::None => {}
        ActiveTool::Road(r) => {
            if buttons.just_pressed(MouseButton::Right) {
                r.points.clear();
                r.stroke.clear();
                r.preview = None;
                r.pending = None;
                return;
            }
            let q = snap(p, &nodes, &segs);
            match r.mode {
                RoadMode::Straight | RoadMode::Curved => {
                    let need = if r.mode == RoadMode::Straight { 1 } else { 2 };
                    r.pending = if r.points.len() >= need {
                        let curve = if r.mode == RoadMode::Straight {
                            CubicBezier2::straight(r.points[0], q)
                        } else {
                            CubicBezier2::from_quadratic(r.points[0], r.points[1], q)
                        };
                        Some(BuildRoad {
                            kind: r.kind.0,
                            curve,
                            start: Endpoint::Free,
                            end: Endpoint::Free,
                        })
                    } else if r.points.len() == 1 {
                        Some(BuildRoad {
                            kind: r.kind.0,
                            curve: CubicBezier2::straight(r.points[0], q),
                            start: Endpoint::Free,
                            end: Endpoint::Free,
                        })
                    } else {
                        None
                    };
                    if buttons.just_pressed(MouseButton::Left) && !alt {
                        if r.points.len() >= need {
                            if let (Some(cmd), Some(Ok(_))) =
                                (r.pending.clone(), r.preview.as_ref())
                            {
                                queue.push(PlayerCommand::BuildRoad(cmd));
                                // Continue from the end point.
                                r.points = vec![q];
                                r.preview = None;
                            }
                        } else {
                            r.points.push(q);
                        }
                    }
                }
                RoadMode::Freehand => {
                    if buttons.pressed(MouseButton::Left) && !alt {
                        if r.stroke
                            .last()
                            .is_none_or(|l| l.distance(p) >= STROKE_STEP_M)
                        {
                            r.stroke.push(if r.stroke.is_empty() { q } else { p });
                        }
                    } else if !r.stroke.is_empty() {
                        let mut pts = std::mem::take(&mut r.stroke);
                        if let Some(last) = pts.last_mut() {
                            *last = q;
                        }
                        if pts.len() >= 2 {
                            for c in cb_geometry::curve_fit::fit(&pts, STROKE_FIT_TOL_M) {
                                queue.push(PlayerCommand::BuildRoad(BuildRoad {
                                    kind: r.kind.0,
                                    curve: c,
                                    start: Endpoint::Free,
                                    end: Endpoint::Free,
                                }));
                            }
                        }
                        r.preview = None;
                    }
                }
            }
        }
        ActiveTool::Plot(pt) => {
            let sel_plot = pt
                .selected
                .and_then(|id| ids.as_ref().and_then(|i| i.plots.get(id)))
                .and_then(|e| plots.get(e).ok());
            if buttons.just_pressed(MouseButton::Left) && !alt {
                // 1. Handles of the selected plot.
                if let Some(plot) = sel_plot
                    && let Some(h) = handles(plot)
                {
                    if h.start.distance(p) < HANDLE_PICK_M {
                        pt.drag = Some(PlotDrag::End { end: 0, s: 0.0 });
                    } else if h.end.distance(p) < HANDLE_PICK_M {
                        pt.drag = Some(PlotDrag::End { end: 1, s: 0.0 });
                    } else if h.depth.distance(p) < HANDLE_PICK_M {
                        pt.drag = Some(PlotDrag::Depth(plot.depth_m()));
                    }
                }
                if pt.drag.is_none() {
                    // 2. A road edge starts a new plot; 3. a click inside a plot selects it.
                    if let Some((segment, left, s)) = edge_under(p, &segs) {
                        pt.drag = Some(PlotDrag::New(EdgeDrag {
                            segment,
                            left,
                            s0: s,
                            s1: s,
                        }));
                        pt.selected = None;
                    } else {
                        pt.selected = ids.as_ref().and_then(|i| {
                            i.plots.iter().find_map(|(id, e)| {
                                plots
                                    .get(e)
                                    .ok()
                                    .filter(|pl| cb_geometry::polygon::contains(pl.polygon(), p))
                                    .map(|_| id)
                            })
                        });
                    }
                }
            }
            match &mut pt.drag {
                Some(PlotDrag::New(d)) => {
                    if let Some(seg) = ids
                        .as_ref()
                        .and_then(|i| i.segments.get(d.segment))
                        .and_then(|e| segs.get(e).ok())
                    {
                        d.s1 = seg.geom().project(p).0;
                    }
                }
                Some(PlotDrag::End { end, s }) => {
                    if let Some(plot) = sel_plot
                        && let Some(sp) =
                            plot.spans()
                                .get(if *end == 0 { 0 } else { plot.spans().len() - 1 })
                        && let Some(seg) = ids
                            .as_ref()
                            .and_then(|i| i.segments.get(sp.segment))
                            .and_then(|e| segs.get(e).ok())
                    {
                        *s = seg.geom().project(p).0;
                    }
                }
                Some(PlotDrag::Depth(dep)) => {
                    if let Some(plot) = sel_plot
                        && let Some(h) = handles(plot)
                    {
                        *dep = (plot.depth_m() + (p - h.depth).dot(h.normal)).clamp(10.0, 80.0);
                    }
                }
                None => {}
            }
            if buttons.just_released(MouseButton::Left) {
                let ok = matches!(pt.preview, Some(Ok(_)));
                match pt.drag.take() {
                    Some(PlotDrag::New(d)) if ok => {
                        queue.push(PlayerCommand::CreatePlot(CreatePlot {
                            zone: pt.zone,
                            frontage: vec![span_of(&d)],
                            depth_m: pt.depth_m,
                            dn: 0,
                        }));
                    }
                    Some(drag @ (PlotDrag::End { .. } | PlotDrag::Depth(_))) if ok => {
                        if let (Some(id), Some(plot)) = (pt.selected, sel_plot) {
                            let (spans, depth) = edited(plot, drag);
                            queue.push(PlayerCommand::EditPlot {
                                plot: id,
                                frontage: spans,
                                depth_m: depth,
                            });
                        }
                    }
                    _ => {}
                }
                pt.preview = None;
            }
        }
        ActiveTool::Bulldoze => {
            if buttons.just_pressed(MouseButton::Left) && !alt {
                // A bridge deck under the cursor wins over the ground below it.
                if let Some((sid, _)) = pick.deck {
                    queue.push(PlayerCommand::RemoveSegment { segment: sid });
                    return;
                }
                let seg = segs.iter().find(|s| {
                    s.geom().project(p).1.abs() < s.geom().half_width_m && {
                        let sp = s.geom().project(p).0;
                        sp > 0.0 && sp < s.geom().length()
                    }
                });
                if let Some(s) = seg {
                    queue.push(PlayerCommand::RemoveSegment { segment: s.id() });
                } else if let Some(id) = ids.as_ref().and_then(|i| {
                    i.plots.iter().find_map(|(id, e)| {
                        plots
                            .get(e)
                            .ok()
                            .filter(|pl| cb_geometry::polygon::contains(pl.polygon(), p))
                            .map(|_| id)
                    })
                }) {
                    queue.push(PlayerCommand::DeletePlot { plot: id });
                }
            }
        }
    }
}

/// Edit handles of a plot: frontage start, frontage end, depth handle (+ inward normal).
pub struct Handles {
    /// Frontage start.
    pub start: Vec2,
    /// Frontage end.
    pub end: Vec2,
    /// Depth handle (middle of the back).
    pub depth: Vec2,
    /// Inward normal at the frontage middle.
    pub normal: Vec2,
}

/// Handles of a plot.
#[must_use]
pub fn handles(plot: &Plot) -> Option<Handles> {
    let f = plot.frontage();
    let (start, end) = (*f.first()?, *f.last()?);
    let total: f32 = f.windows(2).map(|w| w[0].distance(w[1])).sum();
    let mut acc = 0.0;
    for w in f.windows(2) {
        let l = w[0].distance(w[1]);
        if acc + l >= 0.5 * total && l > 0.0 {
            let m = w[0].lerp(w[1], (0.5 * total - acc) / l);
            let d = (w[1] - w[0]) / l;
            let normal = Vec2::new(-d.y, d.x);
            return Some(Handles {
                start,
                end,
                depth: m + normal * plot.depth_m(),
                normal,
            });
        }
        acc += l;
    }
    None
}

/// Spans and depth after an edit drag.
fn edited(plot: &Plot, drag: PlotDrag) -> (Vec<FrontageSpan>, f32) {
    let mut spans = plot.spans().to_vec();
    let mut depth = plot.depth_m();
    match drag {
        PlotDrag::End { end: 0, s } => {
            if let Some(sp) = spans.first_mut() {
                sp.s0 = s;
            }
        }
        PlotDrag::End { s, .. } => {
            if let Some(sp) = spans.last_mut() {
                sp.s1 = s;
            }
        }
        PlotDrag::Depth(d) => depth = d,
        PlotDrag::New(_) => {}
    }
    (spans, depth)
}

/// Previews need read access to the whole sim world.
fn tool_preview(world: &mut World) {
    let Some(tool) = world.get_resource::<ActiveTool>().cloned() else {
        return;
    };
    match tool {
        ActiveTool::Road(r) => {
            let preview = r
                .pending
                .as_ref()
                .map(|cmd| cb_sim::roads::preview_road(world, cmd));
            if let Some(mut t) = world.get_resource_mut::<ActiveTool>()
                && let ActiveTool::Road(rt) = &mut *t
            {
                rt.preview = preview;
            }
        }
        ActiveTool::Plot(pt) => {
            let zone = pt.zone;
            let preview = match pt.drag {
                Some(PlotDrag::New(d)) if (d.s1 - d.s0).abs() > 1.0 => {
                    let c = CreatePlot {
                        zone,
                        frontage: vec![span_of(&d)],
                        depth_m: pt.depth_m,
                        dn: 0,
                    };
                    Some(cb_sim::zoning::preview_plot(world, &c))
                }
                Some(drag @ (PlotDrag::End { .. } | PlotDrag::Depth(_))) => pt
                    .selected
                    .and_then(|id| {
                        let e = world.get_resource::<Ids>().and_then(|i| i.plots.get(id))?;
                        world.get::<Plot>(e).cloned().map(|p| (id, p))
                    })
                    .map(|(id, plot)| {
                        let (spans, depth) = edited(&plot, drag);
                        cb_sim::zoning::preview_edit_plot(world, id, &spans, depth)
                    }),
                _ => None,
            };
            if let Some(mut t) = world.get_resource_mut::<ActiveTool>()
                && let ActiveTool::Plot(p) = &mut *t
            {
                p.preview = preview;
            }
        }
        ActiveTool::None | ActiveTool::Bulldoze => {}
    }
}

/// Marker for the road ghost.
#[derive(Component)]
struct Ghost;

fn setup_ghost(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let m = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        fog_enabled: false,
        depth_bias: 50.0,
        cull_mode: None,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        ))),
        MeshMaterial3d(m),
        Transform::default(),
        Visibility::Hidden,
        NotShadowCaster,
        Ghost,
    ));
}

/// Ghost ribbon of a road: valid = translucent off-white, invalid = coral with stripes every 2 m.
fn ghost_mesh(geom: &SegmentGeom, height: &dyn Fn(f32, Vec2) -> f32, valid: bool) -> Mesh {
    let len = geom.length();
    let n = (len / 2.0).ceil().max(1.0) as usize;
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut col: Vec<[f32; 4]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let ok = Color::srgba_u8(0xED, 0xE6, 0xD3, 150).to_linear();
    let bad = Color::srgba_u8(0xD0, 0x64, 0x3B, 170).to_linear();
    let bad2 = Color::srgba_u8(0xE8, 0x9A, 0x7A, 150).to_linear();
    for k in 0..n {
        let s0 = len * k as f32 / n as f32;
        let s1 = len * (k + 1) as f32 / n as f32;
        let c = if valid {
            ok
        } else if k % 2 == 0 {
            bad
        } else {
            bad2
        };
        let base = pos.len() as u32;
        for s in [s0, s1] {
            let pnt = geom.point_at(s);
            let nrm = geom.dir_at(s).perp() * geom.half_width_m;
            for q in [pnt + nrm, pnt - nrm] {
                pos.push([q.x, height(s, q) + 0.25, q.y]);
                col.push([c.red, c.green, c.blue, c.alpha]);
            }
        }
        idx.extend_from_slice(&[base, base + 1, base + 3, base, base + 3, base + 2]);
    }
    let normals = vec![[0.0, 1.0, 0.0]; pos.len()];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
    .with_inserted_indices(Indices::U32(idx))
}

fn update_ghost(
    tool: Res<ActiveTool>,
    terrain: Option<Res<TerrainRes>>,
    mut ghost: Query<(&Mesh3d, &mut Visibility), With<Ghost>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    if !tool.is_changed() {
        return;
    }
    let Ok((mesh, mut vis)) = ghost.single_mut() else {
        return;
    };
    let ActiveTool::Road(r) = &*tool else {
        *vis = Visibility::Hidden;
        return;
    };
    let (Some(cmd), Some(preview)) = (&r.pending, &r.preview) else {
        *vis = Visibility::Hidden;
        return;
    };
    let ground = |q: Vec2| terrain.as_ref().map_or(0.0, |t| t.terrain().height_at(q));
    let m = match preview {
        Ok(pv) => ghost_mesh(&pv.geom, &|s, _| pv.profile.height_at(s), true),
        Err(_) => {
            let geom = SegmentGeom::new(
                cmd.curve,
                cb_network::CrossSection::of(cmd.kind).half_width_m(),
            );
            ghost_mesh(&geom, &|_, q| ground(q), false)
        }
    };
    if let Some(mut mm) = meshes.get_mut(&mesh.0) {
        *mm = m;
    }
    *vis = Visibility::Visible;
}

fn draw_gizmos(
    mut gizmos: Gizmos,
    tool: Res<ActiveTool>,
    pick: Res<GroundPick>,
    terrain: Option<Res<TerrainRes>>,
    ids: Option<Res<Ids>>,
    plots: Query<&Plot>,
) {
    let ground = |q: Vec2| terrain.as_ref().map_or(0.0, |t| t.terrain().height_at(q)) + 0.4;
    let p3 = |q: Vec2| Vec3::new(q.x, ground(q), q.y);
    let teal = Color::srgb_u8(0x4F, 0xB3, 0xB0);
    let coral = Color::srgb_u8(0xD0, 0x64, 0x3B);
    match &*tool {
        ActiveTool::Road(r) => {
            if let Some(Ok(pv)) = &r.preview {
                for edge in [&pv.geom.left, &pv.geom.right] {
                    let pts: Vec<Vec3> = edge.points().iter().map(|q| p3(*q)).collect();
                    gizmos.linestrip(pts, teal);
                }
            }
            if r.mode == RoadMode::Curved && r.points.len() == 2 {
                gizmos.line(p3(r.points[0]), p3(r.points[1]), teal);
            }
            if !r.stroke.is_empty() {
                gizmos.linestrip(r.stroke.iter().map(|q| p3(*q)), teal);
            }
            if let Some(p) = pick.point {
                gizmos.circle(
                    Isometry3d::new(
                        p + Vec3::Y * 0.3,
                        Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                    ),
                    2.0,
                    teal,
                );
            }
        }
        ActiveTool::Plot(pt) => {
            if let Some(prev) = &pt.preview {
                let (poly, c) = match prev {
                    Ok(p) => (p.clone(), teal),
                    Err(_) => (Vec::new(), coral),
                };
                if !poly.is_empty() {
                    let mut pts: Vec<Vec3> = poly.iter().map(|q| p3(*q)).collect();
                    pts.push(pts[0]);
                    gizmos.linestrip(pts, c);
                }
            }
            if let Some(plot) = pt
                .selected
                .and_then(|id| ids.as_ref().and_then(|i| i.plots.get(id)))
                .and_then(|e| plots.get(e).ok())
                && let Some(h) = handles(plot)
            {
                for q in [h.start, h.end, h.depth] {
                    gizmos.sphere(
                        Isometry3d::from_translation(p3(q)),
                        2.0,
                        Color::srgb_u8(0xF4, 0xEF, 0xE6),
                    );
                }
                let mut pts: Vec<Vec3> = plot.polygon().iter().map(|q| p3(*q)).collect();
                if let Some(f) = pts.first().copied() {
                    pts.push(f);
                }
                gizmos.linestrip(pts, teal);
            }
        }
        ActiveTool::None | ActiveTool::Bulldoze => {}
    }
}

fn collect_results(
    mut results: MessageReader<CommandResult>,
    l10n: Res<Localization>,
    mut toast: ResMut<Toast>,
    time: Res<Time>,
) {
    for r in results.read() {
        if let Err(e) = &r.outcome {
            toast.text = Some((l10n.0.tr(LKey(e.key)), time.elapsed_secs() + 4.0));
        }
    }
    if toast
        .text
        .as_ref()
        .is_some_and(|t| t.1 < time.elapsed_secs())
    {
        toast.text = None;
    }
}

#[allow(clippy::too_many_arguments, reason = "UI state")]
fn toolbar_contents(
    ui: &mut egui::Ui,
    tool: &mut ActiveTool,
    zoning: &mut ZoningView,
    l: &cb_l10n::L10n,
    loc: cb_l10n::Locale,
    treasury: Option<&Treasury>,
    catalog: Option<&Catalog>,
) {
    ui.horizontal(|ui| {
        let is_road = matches!(*tool, ActiveTool::Road(_));
        let is_plot = matches!(*tool, ActiveTool::Plot(_));
        let is_bull = matches!(*tool, ActiveTool::Bulldoze);
        if ui
            .selectable_label(is_road, l.tr(keys::TOOLBAR_ROAD))
            .on_hover_text(l.tr(keys::TOOLTIP_ROAD))
            .clicked()
        {
            *tool = if is_road {
                ActiveTool::None
            } else {
                ActiveTool::Road(Box::default())
            };
        }
        if ui
            .selectable_label(is_plot, l.tr(keys::TOOLBAR_PLOT))
            .on_hover_text(l.tr(keys::TOOLTIP_PLOT))
            .clicked()
        {
            *tool = if is_plot {
                ActiveTool::None
            } else {
                ActiveTool::Plot(PlotTool::default())
            };
        }
        if ui
            .selectable_label(is_bull, l.tr(keys::TOOLBAR_BULLDOZE))
            .on_hover_text(l.tr(keys::TOOLTIP_BULLDOZE))
            .clicked()
        {
            *tool = if is_bull {
                ActiveTool::None
            } else {
                ActiveTool::Bulldoze
            };
        }
        ui.separator();
        if ui
            .selectable_label(zoning.0, l.tr(keys::TOOLBAR_ZONING_VIEW))
            .on_hover_text(l.tr(keys::TOOLTIP_ZONING_VIEW))
            .clicked()
        {
            zoning.0 = !zoning.0;
        }
        if let Some(t) = treasury {
            ui.separator();
            let m = format::money(loc, t.balance());
            ui.strong(l.tr_args(keys::HUD_MONEY, &[("money", Arg::from(m.as_str()))]));
        }
    });
    match tool {
        ActiveTool::Road(r) => {
            ui.horizontal(|ui| {
                for k in [
                    RoadKind::Dirt,
                    RoadKind::Street,
                    RoadKind::OneWayStreet,
                    RoadKind::Avenue,
                    RoadKind::OneWayAvenue,
                ] {
                    if ui
                        .selectable_label(r.kind.0 == k, l.tr(road_kind_key(k)))
                        .clicked()
                    {
                        r.kind = RoadKindSel(k);
                    }
                }
                ui.separator();
                for (m, key) in [
                    (RoadMode::Straight, keys::ROAD_MODE_STRAIGHT),
                    (RoadMode::Curved, keys::ROAD_MODE_CURVED),
                    (RoadMode::Freehand, keys::ROAD_MODE_FREEHAND),
                ] {
                    if ui.selectable_label(r.mode == m, l.tr(key)).clicked() {
                        r.mode = m;
                        r.points.clear();
                        r.stroke.clear();
                    }
                }
            });
        }
        ActiveTool::Plot(pt) => {
            ui.horizontal(|ui| {
                for (z, key) in [
                    (ZoneKind::Residential, keys::ZONE_RESIDENTIAL),
                    (ZoneKind::Commercial, keys::ZONE_COMMERCIAL),
                    (ZoneKind::Industrial, keys::ZONE_INDUSTRIAL),
                ] {
                    if ui.selectable_label(pt.zone == z, l.tr(key)).clicked() {
                        pt.zone = z;
                        pt.depth_m = catalog.map_or(30.0, |c| c.zone(z).default_depth_m);
                    }
                }
            });
        }
        ActiveTool::None | ActiveTool::Bulldoze => {}
    }
}

fn road_kind_key(k: RoadKind) -> LKey {
    match k {
        RoadKind::Dirt => keys::ROAD_KIND_DIRT,
        RoadKind::Street => keys::ROAD_KIND_STREET,
        RoadKind::OneWayStreet => keys::ROAD_KIND_ONE_WAY_STREET,
        RoadKind::Avenue => keys::ROAD_KIND_AVENUE,
        RoadKind::OneWayAvenue => keys::ROAD_KIND_ONE_WAY_AVENUE,
        RoadKind::TramTrack => keys::ROAD_KIND_TRAM_TRACK,
    }
}

#[allow(clippy::too_many_arguments, reason = "Bevy system parameters")]
fn toolbar_ui(
    mut contexts: EguiContexts,
    mut tool: ResMut<ActiveTool>,
    mut zoning: ResMut<ZoningView>,
    mut over_ui: ResMut<PointerOverUi>,
    mut queue: ResMut<CommandQueue>,
    l10n: Res<Localization>,
    treasury: Option<Res<Treasury>>,
    pick: Res<GroundPick>,
    toast: Res<Toast>,
    ids: Option<Res<Ids>>,
    plots: Query<&Plot>,
    catalog: Option<Res<Catalog>>,
    visible: Res<UiVisible>,
) {
    if !visible.0 {
        over_ui.0 = false;
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let l = &l10n.0;
    let loc = l.locale();
    egui::Area::new(egui::Id::new("toolbar"))
        .anchor(egui::Align2::LEFT_TOP, egui::vec2(12.0, 12.0))
        .show(ctx, |ui| {
            egui::Frame::window(ui.style()).show(ui, |ui| {
                toolbar_contents(
                    ui,
                    &mut tool,
                    &mut zoning,
                    l,
                    loc,
                    treasury.as_deref(),
                    catalog.as_deref(),
                );
            });
        });
    // Selected plot panel: lot count ±, depth ±, delete.
    if let ActiveTool::Plot(pt) = &mut *tool
        && let Some(id) = pt.selected
        && let Some(plot) = ids
            .as_ref()
            .and_then(|i| i.plots.get(id))
            .and_then(|e| plots.get(e).ok())
    {
        egui::Window::new(l.tr(keys::PLOT_PANEL_TITLE))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 60.0))
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label(l.tr_args(
                    keys::PLOT_LOTS,
                    &[("count", Arg::from(plot.lots().len() as i64))],
                ));
                ui.horizontal(|ui| {
                    if ui
                        .button("−")
                        .on_hover_text(l.tr(keys::PLOT_FEWER_LOTS))
                        .clicked()
                    {
                        queue.push(PlayerCommand::SetSubdivision {
                            plot: id,
                            dn: plot.dn() - 1,
                        });
                    }
                    if ui
                        .button("+")
                        .on_hover_text(l.tr(keys::PLOT_MORE_LOTS))
                        .clicked()
                    {
                        queue.push(PlayerCommand::SetSubdivision {
                            plot: id,
                            dn: plot.dn() + 1,
                        });
                    }
                });
                let depth = format::int(loc, plot.depth_m().round() as i64);
                ui.label(l.tr_args(keys::PLOT_DEPTH, &[("depth", Arg::from(depth.as_str()))]));
                ui.horizontal(|ui| {
                    if ui
                        .button("−")
                        .on_hover_text(l.tr(keys::PLOT_SHALLOWER))
                        .clicked()
                    {
                        queue.push(PlayerCommand::EditPlot {
                            plot: id,
                            frontage: plot.spans().to_vec(),
                            depth_m: plot.depth_m() - 5.0,
                        });
                    }
                    if ui
                        .button("+")
                        .on_hover_text(l.tr(keys::PLOT_DEEPER))
                        .clicked()
                    {
                        queue.push(PlayerCommand::EditPlot {
                            plot: id,
                            frontage: plot.spans().to_vec(),
                            depth_m: plot.depth_m() + 5.0,
                        });
                    }
                });
                if ui.button(l.tr(keys::PLOT_DELETE)).clicked() {
                    queue.push(PlayerCommand::DeletePlot { plot: id });
                    pt.selected = None;
                }
            });
    }
    // Cursor label: invalid reason or cost.
    if let (ActiveTool::Road(r), Some(cursor)) = (&*tool, pick.cursor)
        && let Some(preview) = &r.preview
    {
        let cream = egui::Color32::from_rgb(0xED, 0xE6, 0xD3);
        let cost_line = |cost: cb_core::Money| {
            let c = format::money(loc, cost);
            l.tr_args(keys::TOOL_COST, &[("cost", Arg::from(c.as_str()))])
        };
        // Invalid previews still show what the road would cost.
        let (reason, cost) = match preview {
            Ok(pv) => (None, Some(pv.cost)),
            Err(e) => (
                Some(l.tr(LKey(e.key))),
                r.pending
                    .as_ref()
                    .zip(catalog.as_deref())
                    .map(|(cmd, cat)| {
                        let len = SegmentGeom::new(cmd.curve, 0.0).length();
                        cb_sim::roads::road_cost(cat, cmd.kind, len)
                    }),
            ),
        };
        egui::Area::new(egui::Id::new("tool-label"))
            .fixed_pos(egui::pos2(cursor.x + 18.0, cursor.y + 14.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    if let Some(reason) = reason {
                        ui.colored_label(egui::Color32::from_rgb(0xF2, 0x9A, 0x78), reason);
                    }
                    if let Some(c) = cost {
                        ui.colored_label(cream, cost_line(c));
                    }
                });
            });
    }
    if let Some((text, _)) = &toast.text {
        egui::Area::new(egui::Id::new("toast"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -40.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.colored_label(egui::Color32::from_rgb(0xF2, 0x9A, 0x78), text);
                });
            });
    }
    over_ui.0 = ctx.egui_wants_pointer_input() || ctx.is_pointer_over_egui();
}
