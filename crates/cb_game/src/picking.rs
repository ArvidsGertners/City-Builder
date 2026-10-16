//! Picking (CLAUDE.md platform rules): the cursor ray from **logical** window coordinates
//! (DPI- and render-scale-independent) and the ground point under the cursor via the terrain's
//! min-max raycast (incl. water surfaces).

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use cb_sim::terrain::TerrainRes;

use crate::camera::WorldCamera;

/// World ray through a cursor at `cursor_logical` (window logical pixels, origin top-left).
#[must_use]
pub fn cursor_ray(
    cursor_logical: Vec2,
    window_logical: Vec2,
    world_from_view: Mat4,
    clip_from_view: Mat4,
) -> Option<Ray3d> {
    if window_logical.x <= 0.0 || window_logical.y <= 0.0 {
        return None;
    }
    let ndc = Vec2::new(
        cursor_logical.x / window_logical.x * 2.0 - 1.0,
        1.0 - cursor_logical.y / window_logical.y * 2.0,
    );
    let world_from_clip = world_from_view * clip_from_view.inverse();
    // Reverse-Z: near plane at depth 1.
    let near = world_from_clip.project_point3(ndc.extend(1.0));
    let far = world_from_clip.project_point3(ndc.extend(1e-4));
    let dir = Dir3::new(far - near).ok()?;
    Some(Ray3d {
        origin: near,
        direction: dir,
    })
}

/// The ground point under the cursor this frame (`None` off the terrain or over UI).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct GroundPick {
    /// Hit point.
    pub point: Option<Vec3>,
    /// Cursor position (logical pixels).
    pub cursor: Option<Vec2>,
}

/// A scripted ground point standing in for the cursor (screenshot suites; no real pointer).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct ScriptedCursor(pub Option<Vec3>);

/// System set of the ground pick (after the camera has moved this frame); tools run after it.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PickingSystems;

/// Picking plugin.
pub struct PickingPlugin;

impl Plugin for PickingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GroundPick>()
            .init_resource::<ScriptedCursor>()
            .configure_sets(
                Update,
                PickingSystems
                    .in_set(crate::app::GameSet::Input)
                    .after(crate::camera::CameraSystems),
            )
            .add_systems(Update, pick_ground.in_set(PickingSystems));
    }
}

fn pick_ground(
    windows: Query<&Window, With<PrimaryWindow>>,
    cams: Query<(&GlobalTransform, &Projection, &Camera), With<WorldCamera>>,
    terrain: Option<Res<TerrainRes>>,
    over_ui: Res<crate::camera::PointerOverUi>,
    scripted: Res<ScriptedCursor>,
    mut pick: ResMut<GroundPick>,
) {
    *pick = GroundPick::default();
    if let Some(p) = scripted.0 {
        pick.point = Some(p);
        if let Ok((gt, _, cam)) = cams.single() {
            pick.cursor = cam.world_to_viewport(gt, p).ok();
        }
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    pick.cursor = Some(cursor);
    if over_ui.0 {
        return;
    }
    let Ok((gt, _proj, cam)) = cams.single() else {
        return;
    };
    let size = Vec2::new(window.width(), window.height());
    let Some(ray) = cursor_ray(cursor, size, gt.to_matrix(), cam.clip_from_view()) else {
        return;
    };
    pick.point = match terrain {
        Some(t) => t
            .terrain()
            .raycast(ray.origin, *ray.direction, 20_000.0)
            .map(|h| h.point),
        None => ray
            .intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y))
            .map(|d| ray.get_point(d)),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pipeline that converts physical cursor and render-target pixels back to logical.
    fn ray_via_pixels(
        logical: Vec2,
        window: Vec2,
        scale_factor: f32,
        render_scale: f32,
        w: Mat4,
        p: Mat4,
    ) -> Ray3d {
        let physical_cursor = logical * scale_factor;
        let target = (window * scale_factor * render_scale).round();
        let in_target = physical_cursor * render_scale;
        let back_to_logical = in_target / target * window;
        cursor_ray(back_to_logical, window, w, p).unwrap()
    }

    #[test]
    fn cursor_ray_dpi_render_scale() {
        let window = Vec2::new(1470.0, 956.0);
        let world_from_view = Mat4::from_rotation_translation(
            Quat::from_euler(EulerRot::YXZ, 0.6, -0.55, 0.0),
            Vec3::new(30.0, 40.0, 50.0),
        );
        let proj = bevy::camera::CameraProjection::get_clip_from_view(&PerspectiveProjection {
            fov: std::f32::consts::FRAC_PI_4,
            aspect_ratio: window.x / window.y,
            near: 0.1,
            ..default()
        });
        for cursor in [Vec2::ZERO, window / 2.0, Vec2::new(1234.5, 77.25), window] {
            let reference = cursor_ray(cursor, window, world_from_view, proj).unwrap();
            for sf in [1.0, 2.0] {
                for rs in [1.0, 2.0 / 3.0] {
                    let r = ray_via_pixels(cursor, window, sf, rs, world_from_view, proj);
                    let ang = r
                        .direction
                        .dot(*reference.direction)
                        .clamp(-1.0, 1.0)
                        .acos();
                    assert!(ang < 2e-3, "cursor {cursor} sf {sf} rs {rs}: {ang} rad");
                }
            }
        }
    }
}
