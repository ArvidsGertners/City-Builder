# ADR-0033: Coordinate system and units

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M1

## Context

Geometry, terrain, rendering, picking and traffic exchange positions and orientations;
mismatched conventions cause sign bugs that are hard to see in a rotating 3D camera.

## Decision

- World: meters, **Y-up, right-handed** (Bevy convention); ground plane XZ; the map spans
  `[-2048, 2048]` on X and Z with the origin at the map center (f32 precision ≥ 0.25 mm).
- 2D geometry in pure crates uses `Vec2(x, z)`; conversions `to_xz(Vec3) -> Vec2` and
  `from_xz(Vec2, y) -> Vec3` live in `cb_core::units` only.
- **Orientation of 2D polygons** is defined in the abstract `(x, z)` frame with the standard
  2D cross product `a.x * b.y − a.y * b.x` (where `.y` holds z). "CCW" means positive
  signed area in that frame; all polygons are stored CCW. No code may infer orientation
  from how a shape looks on screen.
- **Yaw**: an object's rotation is `Quat::from_rotation_y(yaw)`; its forward vector is
  `rotation * Vec3::NEG_Z` (Bevy forward). Helpers `yaw_from_dir(Vec2)` / `dir_from_yaw(f32)`
  in `cb_core::units` are the only conversions.
- Angles in radians internally; degrees only in catalog/UI. Speeds m/s internally, km/h in
  UI/catalog. Sim time in seconds (`f32` per tick), game time `GameTime(i64 ms)`.
  Money `i64` cents. Power kW, water m³/h, garbage kg.

## Consequences

- One documented convention, enforced by unit tests on the conversion helpers.
