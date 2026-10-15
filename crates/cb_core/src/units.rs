//! Units and coordinate conventions (ADR-0033).
//!
//! World space is meters, Y-up, right-handed; the ground plane is XZ and 2D geometry uses
//! `Vec2(x, z)`. The map spans `[-MAP_HALF_M, MAP_HALF_M]` on both axes. Yaw is the angle of
//! `Quat::from_rotation_y(yaw)`; forward is `-Z`.

use crate::math;
use glam::{Vec2, Vec3};

/// Map edge length in meters (4 × 4 km).
pub const MAP_SIZE_M: f32 = 4096.0;
/// Half the map edge length.
pub const MAP_HALF_M: f32 = MAP_SIZE_M / 2.0;

/// Projects a world position onto the ground plane as `Vec2(x, z)`.
#[inline]
#[must_use]
pub fn to_xz(p: Vec3) -> Vec2 {
    Vec2::new(p.x, p.z)
}

/// Lifts a ground-plane point `(x, z)` to 3D at height `y`.
#[inline]
#[must_use]
pub fn from_xz(p: Vec2, y: f32) -> Vec3 {
    Vec3::new(p.x, y, p.y)
}

/// Yaw (radians) whose forward vector (`-Z` rotated about +Y) points along `dir` in XZ.
#[inline]
#[must_use]
pub fn yaw_from_dir(dir: Vec2) -> f32 {
    // Rotating -Z by yaw about +Y gives (-sin yaw, -cos yaw) in (x, z).
    math::atan2(-dir.x, -dir.y)
}

/// Unit ground-plane direction `(x, z)` of the forward vector for `yaw`.
#[inline]
#[must_use]
pub fn dir_from_yaw(yaw: f32) -> Vec2 {
    let (s, c) = math::sin_cos(yaw);
    Vec2::new(-s, -c)
}

/// km/h → m/s.
#[inline]
#[must_use]
pub fn kmh_to_mps(kmh: f32) -> f32 {
    kmh / 3.6
}

/// m/s → km/h.
#[inline]
#[must_use]
pub fn mps_to_kmh(mps: f32) -> f32 {
    mps * 3.6
}

/// Whether a ground point lies inside the map bounds (inclusive).
#[inline]
#[must_use]
pub fn in_map(p: Vec2) -> bool {
    p.x.abs() <= MAP_HALF_M && p.y.abs() <= MAP_HALF_M
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xz_roundtrip() {
        let p = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(from_xz(to_xz(p), p.y), p);
    }

    #[test]
    fn yaw_conventions_match_bevy_forward() {
        // yaw 0 faces -Z.
        assert!((dir_from_yaw(0.0) - Vec2::new(0.0, -1.0)).length() < 1e-6);
        // +90° about +Y turns -Z into -X.
        assert!((dir_from_yaw(math::PI / 2.0) - Vec2::new(-1.0, 0.0)).length() < 1e-6);
        for yaw in [-2.5_f32, -1.0, 0.3, 1.7, 3.0] {
            let back = yaw_from_dir(dir_from_yaw(yaw));
            assert!((back - yaw).abs() < 1e-5, "{yaw} -> {back}");
        }
    }

    #[test]
    fn speed_conversions() {
        assert!((kmh_to_mps(36.0) - 10.0).abs() < 1e-6);
        assert!((mps_to_kmh(10.0) - 36.0).abs() < 1e-5);
    }
}
