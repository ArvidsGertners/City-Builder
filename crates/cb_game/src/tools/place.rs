//! Placement tools (GAME_DESIGN §3, §5–6): civic buildings (`PlaceBuilding`, front snapped to
//! the road edge within 2 m) and free decorations (`PlaceProp`). Previews call the sim's
//! read-only validation so the ghost and the cursor label match what the command will do.

use bevy::prelude::*;
use bevy_egui::egui;
use cb_core::Money;
use cb_l10n::{Key as LKey, keys};
use cb_procgen::building::civic::CivicKind;
use cb_sim::commands::{PlayerCommand, ValidationError};
use cb_sim::props::DecorKind;

/// Valid ghost colour (ART_DIRECTION §6).
const GHOST_VALID: Color = Color::srgb(0.31, 0.70, 0.69);
/// Invalid ghost colour.
const GHOST_INVALID: Color = Color::srgb(0.82, 0.39, 0.23);

/// Civic building placement.
#[derive(Clone, Debug)]
pub struct PlaceTool {
    /// What to place.
    pub kind: CivicKind,
    /// Cursor position on the ground.
    pub pos: Option<Vec2>,
    /// Cost or rejection at the cursor.
    pub preview: Option<Result<Money, ValidationError>>,
    /// Snapped parcel and volume outlines of a valid preview.
    pub ghost: Option<(Vec<Vec2>, Vec<Vec<Vec2>>)>,
}

impl Default for PlaceTool {
    fn default() -> Self {
        Self {
            kind: CivicKind::PoliceStation,
            pos: None,
            preview: None,
            ghost: None,
        }
    }
}

/// Decoration placement.
#[derive(Clone, Debug)]
pub struct DecorTool {
    /// What to place.
    pub kind: DecorKind,
    /// Cursor position on the ground.
    pub pos: Option<Vec2>,
    /// Rotation for the next prop (radians; [ and ] turn it).
    pub yaw: f32,
    /// Cost or rejection at the cursor.
    pub preview: Option<Result<Money, ValidationError>>,
}

impl Default for DecorTool {
    fn default() -> Self {
        Self {
            kind: DecorKind::Tree,
            pos: None,
            yaw: 0.0,
            preview: None,
        }
    }
}

/// Localization key of a civic building kind.
#[must_use]
pub fn civic_key(k: CivicKind) -> LKey {
    match k {
        CivicKind::PoliceStation => keys::CIVIC_POLICE_STATION,
        CivicKind::FireStation => keys::CIVIC_FIRE_STATION,
        CivicKind::Clinic => keys::CIVIC_CLINIC,
        CivicKind::ElementarySchool => keys::CIVIC_ELEMENTARY_SCHOOL,
        CivicKind::TownHall => keys::CIVIC_TOWN_HALL,
        CivicKind::DieselGenerator => keys::CIVIC_DIESEL_GENERATOR,
        CivicKind::CoalPlant => keys::CIVIC_COAL_PLANT,
        CivicKind::WaterPump => keys::CIVIC_WATER_PUMP,
        CivicKind::SewageOutlet => keys::CIVIC_SEWAGE_OUTLET,
        CivicKind::Landfill => keys::CIVIC_LANDFILL,
    }
}

/// Localization key of a decoration kind.
#[must_use]
pub fn decor_key(k: DecorKind) -> LKey {
    match k {
        DecorKind::Tree => keys::DECOR_TREE,
        DecorKind::TreeCluster => keys::DECOR_TREE_CLUSTER,
        DecorKind::Bench => keys::DECOR_BENCH,
        DecorKind::Planter => keys::DECOR_PLANTER,
        DecorKind::Fountain => keys::DECOR_FOUNTAIN,
        DecorKind::FlowerBed => keys::DECOR_FLOWER_BED,
    }
}

/// Click handling of the civic placement tool.
pub(super) fn place_input(
    t: &mut PlaceTool,
    p: Vec2,
    click: bool,
    queue: &mut cb_sim::commands::CommandQueue,
) {
    t.pos = Some(p);
    if click && matches!(t.preview, Some(Ok(_))) {
        queue.push(PlayerCommand::PlaceBuilding {
            kind: t.kind,
            pos: p,
        });
    }
}

/// Click and rotate handling of the decoration tool.
pub(super) fn decor_input(
    t: &mut DecorTool,
    p: Vec2,
    click: bool,
    turn: f32,
    queue: &mut cb_sim::commands::CommandQueue,
) {
    t.pos = Some(p);
    t.yaw = (t.yaw + turn).rem_euclid(std::f32::consts::TAU);
    if click && matches!(t.preview, Some(Ok(_))) {
        queue.push(PlayerCommand::PlaceProp {
            kind: t.kind,
            pos: p,
            yaw: t.yaw,
        });
    }
}

/// Civic preview at the cursor: cost or rejection, and the snapped ghost.
pub(super) fn place_preview(
    world: &World,
    t: &PlaceTool,
) -> (
    Option<Result<Money, ValidationError>>,
    Option<(Vec<Vec2>, Vec<Vec<Vec2>>)>,
) {
    let Some(p) = t.pos else { return (None, None) };
    match cb_sim::buildings::preview_place_civic(world, t.kind, p) {
        Ok(pl) => (Some(Ok(pl.cost)), Some((pl.parcel, pl.outlines))),
        Err(e) => (Some(Err(e)), None),
    }
}

/// Decoration preview at the cursor.
pub(super) fn decor_preview(
    world: &World,
    t: &DecorTool,
) -> Option<Result<Money, ValidationError>> {
    t.pos
        .map(|p| cb_sim::props::preview_place_prop(world, t.kind, p))
}

/// Ghost outlines on the ground.
pub(super) fn place_gizmos(gizmos: &mut Gizmos, t: &PlaceTool, ground: &dyn Fn(Vec2) -> f32) {
    let loop3 = |poly: &[Vec2], lift: f32| -> Vec<Vec3> {
        let mut v: Vec<Vec3> = poly
            .iter()
            .map(|q| Vec3::new(q.x, ground(*q) + lift, q.y))
            .collect();
        if let Some(f) = v.first().copied() {
            v.push(f);
        }
        v
    };
    match (&t.ghost, t.pos) {
        (Some((parcel, outlines)), _) => {
            gizmos.linestrip(loop3(parcel, 0.3), GHOST_VALID);
            for o in outlines {
                gizmos.linestrip(loop3(o, 0.6), GHOST_VALID.with_alpha(0.8));
            }
        }
        (None, Some(p)) => {
            // Invalid: the parcel footprint at the cursor, axis-aligned.
            let (w, d) = t.kind.parcel_m();
            let q = [
                p + Vec2::new(-0.5 * w, 0.0),
                p + Vec2::new(0.5 * w, 0.0),
                p + Vec2::new(0.5 * w, d),
                p + Vec2::new(-0.5 * w, d),
            ];
            gizmos.linestrip(loop3(&q, 0.3), GHOST_INVALID);
        }
        _ => {}
    }
}

/// Decoration marker: a circle of the footprint (valid) or a cross (invalid).
pub(super) fn decor_gizmos(gizmos: &mut Gizmos, t: &DecorTool, ground: &dyn Fn(Vec2) -> f32) {
    let Some(p) = t.pos else { return };
    let r = cb_sim::props::spec(t.kind).footprint_r_m.max(0.8);
    let c = Vec3::new(p.x, ground(p) + 0.3, p.y);
    let ok = matches!(t.preview, Some(Ok(_)));
    let col = if ok { GHOST_VALID } else { GHOST_INVALID };
    gizmos.circle(
        Isometry3d::new(c, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
        r,
        col,
    );
    let (s, co) = cb_core::math::sin_cos(t.yaw);
    gizmos.line(c, c + Vec3::new(co, 0.0, s) * (r + 0.6), col);
    // Land value reach.
    let reach = cb_sim::props::spec(t.kind).radius_m;
    gizmos.circle(
        Isometry3d::new(c, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
        reach,
        col.with_alpha(0.35),
    );
}

/// Kind picker of the civic tool.
pub(super) fn place_toolbar(ui: &mut egui::Ui, t: &mut PlaceTool, l: &cb_l10n::L10n) {
    ui.horizontal_wrapped(|ui| {
        for k in CivicKind::ALL {
            if ui
                .selectable_label(t.kind == k, l.tr(civic_key(k)))
                .clicked()
            {
                t.kind = k;
                t.preview = None;
                t.ghost = None;
            }
        }
    });
}

/// Kind picker of the decoration tool.
pub(super) fn decor_toolbar(ui: &mut egui::Ui, t: &mut DecorTool, l: &cb_l10n::L10n) {
    ui.horizontal(|ui| {
        for k in DecorKind::ALL {
            if ui
                .selectable_label(t.kind == k, l.tr(decor_key(k)))
                .clicked()
            {
                t.kind = k;
                t.preview = None;
            }
        }
    });
}
