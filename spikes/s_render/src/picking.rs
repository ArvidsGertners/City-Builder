//! Cursor ray in logical pixels (ADR-0053): the ray depends only on the logical cursor position,
//! the logical window size and the camera — never on the OS scale factor or the render scale.

use bevy::math::{Mat4, Vec2, Vec3};

/// A ray with a unit direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray3d {
    pub origin: Vec3,
    pub direction: Vec3,
}

/// World-space ray through `cursor_logical` (origin top-left, y down).
#[must_use]
pub fn cursor_ray(cursor_logical: Vec2, window_logical: Vec2, world_from_view: Mat4, clip_from_view: Mat4) -> Option<Ray3d> {
    if window_logical.x <= 0.0 || window_logical.y <= 0.0 {
        return None;
    }
    let ndc = Vec2::new(
        cursor_logical.x / window_logical.x * 2.0 - 1.0,
        1.0 - cursor_logical.y / window_logical.y * 2.0,
    );
    let world_from_clip = world_from_view * clip_from_view.inverse();
    // Bevy uses reverse-Z: near plane at depth 1, far towards 0.
    let near = world_from_clip.project_point3(ndc.extend(1.0));
    let far = world_from_clip.project_point3(ndc.extend(1e-4));
    let dir = (far - near).normalize_or_zero();
    (dir != Vec3::ZERO).then_some(Ray3d { origin: near, direction: dir })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::{Quat, Vec4Swizzles};

    /// What an implementation sees at a given OS scale factor and render scale: physical cursor
    /// and render-target pixels. A correct pipeline converts back to logical before `cursor_ray`.
    fn ray_via_pixels(logical: Vec2, window: Vec2, scale_factor: f32, render_scale: f32, w: Mat4, p: Mat4) -> Ray3d {
        let physical_cursor = logical * scale_factor;
        let target = (window * scale_factor * render_scale).round();
        let in_target = physical_cursor * render_scale;
        let back_to_logical = in_target / target * window;
        cursor_ray(back_to_logical, window, w, p).unwrap()
    }

    #[test]
    fn cursor_ray_dpi_render_scale() {
        let window = Vec2::new(1470.0, 956.0); // MacBook Air 13" logical size
        let world_from_view = Mat4::from_rotation_translation(
            Quat::from_euler(bevy::math::EulerRot::YXZ, 0.6, -0.55, 0.0),
            Vec3::new(30.0, 40.0, 50.0),
        );
        let proj = Mat4::perspective_infinite_reverse_rh(std::f32::consts::FRAC_PI_4, window.x / window.y, 0.1);
        let cursors = [Vec2::new(0.0, 0.0), window / 2.0, Vec2::new(1234.5, 77.25), window];
        for cursor in cursors {
            let reference = cursor_ray(cursor, window, world_from_view, proj).unwrap();
            for sf in [1.0, 2.0] {
                for rs in [1.0, 2.0 / 3.0] {
                    let r = ray_via_pixels(cursor, window, sf, rs, world_from_view, proj);
                    let ang = r.direction.dot(reference.direction).clamp(-1.0, 1.0).acos();
                    assert!(ang < 2e-3, "cursor {cursor} sf {sf} rs {rs}: {ang} rad");
                    assert!((r.origin - reference.origin).length() < 1e-3);
                }
            }
        }
        // Centre ray is the camera forward axis.
        let centre = cursor_ray(window / 2.0, window, world_from_view, proj).unwrap();
        let forward = (world_from_view * bevy::math::Vec4::new(0.0, 0.0, -1.0, 0.0)).xyz().normalize();
        assert!(centre.direction.dot(forward) > 0.99999);
        // Sanity: feeding physical pixels as if logical is wrong at scale factor 2.
        let wrong = cursor_ray(cursors[2] * 2.0, window, world_from_view, proj).unwrap();
        let right = cursor_ray(cursors[2], window, world_from_view, proj).unwrap();
        assert!(wrong.direction.dot(right.direction) < 0.999);
    }
}
