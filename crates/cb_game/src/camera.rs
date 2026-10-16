//! Camera controller (GAME_DESIGN §12, ADR-0052 trackpad parity): an orbit rig around a focus
//! point on the terrain. The logic is a pure step function over an explicit input record so it
//! is unit-tested on every OS; the Bevy system only gathers input.
//!
//! Controls: WASD/arrows pan (physical keys), Q/E rotate (tap snaps to the next 90°, hold rotates
//! continuously), middle-drag or Alt+left-drag pans, right-drag rotates and tilts, mouse wheel
//! (line deltas) zooms, trackpad two-finger scroll (pixel deltas) pans, pinch zooms, two-finger
//! rotate rotates.

use bevy::input::gestures::{PinchGesture, RotationGesture};
use bevy::input::mouse::{AccumulatedMouseMotion, MouseScrollUnit, MouseWheel};
use bevy::prelude::*;

use crate::render::FogState;

/// Distance limits (m).
pub const ZOOM_RANGE_M: (f32, f32) = (25.0, 2400.0);
/// Pitch limits (degrees above the horizon).
pub const PITCH_RANGE_DEG: (f32, f32) = (12.0, 88.0);
/// Minimum camera height above the terrain (m).
pub const MIN_CLEARANCE_M: f32 = 6.0;
/// Q/E press shorter than this is a tap (s).
pub const TAP_S: f32 = 0.25;
/// Continuous rotation speed while Q/E is held (deg/s).
pub const HOLD_ROTATE_DEG_S: f32 = 90.0;
/// Margin between the focus and the map edge (m).
pub const EDGE_MARGIN_M: f32 = 50.0;

/// The orbit rig.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct CameraRig {
    /// Point looked at (m).
    pub focus: Vec3,
    /// Distance from the focus (m).
    pub distance: f32,
    /// Elevation above the horizon (degrees).
    pub pitch_deg: f32,
    /// Heading around +Y (degrees).
    pub yaw_deg: f32,
}

impl Default for CameraRig {
    fn default() -> Self {
        Self {
            focus: Vec3::ZERO,
            distance: 300.0,
            pitch_deg: 45.0,
            yaw_deg: 0.0,
        }
    }
}

impl CameraRig {
    /// Eye position.
    #[must_use]
    pub fn eye(&self) -> Vec3 {
        let (p, y) = (self.pitch_deg.to_radians(), self.yaw_deg.to_radians());
        self.focus + Vec3::new(p.cos() * y.sin(), p.sin(), p.cos() * y.cos()) * self.distance
    }

    /// Camera transform.
    #[must_use]
    pub fn transform(&self) -> Transform {
        Transform::from_translation(self.eye()).looking_at(self.focus, Vec3::Y)
    }

    /// Ground-plane unit vectors (forward = away from the camera, right).
    #[must_use]
    pub fn ground_axes(&self) -> (Vec2, Vec2) {
        let y = self.yaw_deg.to_radians();
        let fwd = Vec2::new(-y.sin(), -y.cos());
        (fwd, Vec2::new(-fwd.y, fwd.x))
    }
}

/// One frame of camera input (logical pixels, lines, seconds).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraInput {
    /// Key pan: x = right, y = forward, each −1..1.
    pub pan_keys: Vec2,
    /// Drag pan in logical pixels (middle drag / Alt+left drag).
    pub drag_pan_px: Vec2,
    /// Trackpad two-finger scroll (pixel deltas) — pans.
    pub scroll_px: Vec2,
    /// Mouse wheel (line deltas) — zooms; positive = in.
    pub zoom_lines: f32,
    /// Pinch: positive = zoom in (fraction).
    pub pinch: f32,
    /// Rotate drag (right drag) in logical pixels: x = yaw, y = pitch.
    pub rotate_px: Vec2,
    /// Two-finger rotate (radians).
    pub twist_rad: f32,
    /// Q/E held: −1 (Q, counter-clockwise), +1 (E), 0.
    pub rotate_key: i8,
}

/// Controller state between frames.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraCtl {
    held_s: f32,
    held_dir: i8,
    yaw_target: Option<f32>,
}

/// World limits for the camera.
pub struct Bounds<'a> {
    /// Half extent of the map (m).
    pub half_extent: f32,
    /// Ground height.
    pub ground: &'a dyn Fn(Vec2) -> f32,
}

fn next_90(yaw: f32, dir: i8) -> f32 {
    let k = yaw / 90.0;
    if dir > 0 {
        (k + 1e-3).floor() * 90.0 + 90.0
    } else {
        (k - 1e-3).ceil() * 90.0 - 90.0
    }
}

impl CameraCtl {
    /// Applies one frame of input to the rig.
    pub fn step(&mut self, rig: &mut CameraRig, input: &CameraInput, dt: f32, bounds: &Bounds<'_>) {
        // Rotation keys: tap → snap to the next 90°, hold → continuous.
        if input.rotate_key != 0 {
            if self.held_dir != input.rotate_key {
                self.held_dir = input.rotate_key;
                self.held_s = 0.0;
            }
            self.held_s += dt;
            if self.held_s > TAP_S {
                self.yaw_target = None;
                rig.yaw_deg += f32::from(input.rotate_key) * HOLD_ROTATE_DEG_S * dt;
            }
        } else {
            if self.held_dir != 0 && self.held_s <= TAP_S {
                let from = self.yaw_target.unwrap_or(rig.yaw_deg);
                self.yaw_target = Some(next_90(from, self.held_dir));
            }
            self.held_dir = 0;
            self.held_s = 0.0;
        }
        if let Some(t) = self.yaw_target {
            let step = 360.0 * dt;
            let d = t - rig.yaw_deg;
            if d.abs() <= step {
                rig.yaw_deg = t;
                self.yaw_target = None;
            } else {
                rig.yaw_deg += step * d.signum();
            }
        }
        rig.yaw_deg += input.rotate_px.x * 0.25 + input.twist_rad.to_degrees();
        rig.pitch_deg =
            (rig.pitch_deg + input.rotate_px.y * 0.2).clamp(PITCH_RANGE_DEG.0, PITCH_RANGE_DEG.1);
        // Zoom.
        let zoom = 1.15f32.powf(-input.zoom_lines) * (1.0 - input.pinch).clamp(0.2, 5.0);
        rig.distance = (rig.distance * zoom).clamp(ZOOM_RANGE_M.0, ZOOM_RANGE_M.1);
        // Pan relative to the view heading, scaled with distance.
        let (fwd, right) = rig.ground_axes();
        let keys = (right * input.pan_keys.x + fwd * input.pan_keys.y) * rig.distance * 0.9 * dt;
        let px = input.drag_pan_px + input.scroll_px;
        let per_px = rig.distance * 0.0016;
        let drag = (-right * px.x + fwd * px.y) * per_px;
        let mut f = Vec2::new(rig.focus.x, rig.focus.z) + keys + drag;
        let lim = (bounds.half_extent - EDGE_MARGIN_M).max(0.0);
        f = f.clamp(Vec2::splat(-lim), Vec2::splat(lim));
        rig.focus = Vec3::new(f.x, (bounds.ground)(f), f.y);
        // Never below the terrain: tilt up until the eye clears it.
        for _ in 0..90 {
            let e = rig.eye();
            if e.y >= (bounds.ground)(Vec2::new(e.x, e.z)) + MIN_CLEARANCE_M
                || rig.pitch_deg >= PITCH_RANGE_DEG.1
            {
                break;
            }
            rig.pitch_deg = (rig.pitch_deg + 1.0).min(PITCH_RANGE_DEG.1);
        }
    }
}

/// What a left-button drag controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragTarget {
    /// The pointer is over UI.
    Ui,
    /// The active tool.
    Tool,
    /// Camera pan (Alt held or no tool).
    Camera,
}

/// Left-drag precedence: UI, then Alt → camera, then the active tool, else camera.
#[must_use]
pub fn left_drag_target(over_ui: bool, alt: bool, tool_active: bool) -> DragTarget {
    if over_ui {
        DragTarget::Ui
    } else if alt || !tool_active {
        DragTarget::Camera
    } else {
        DragTarget::Tool
    }
}

/// Splits wheel events: pixel units (trackpads) pan, line units (mouse wheels) zoom.
#[must_use]
pub fn split_scroll(events: &[(MouseScrollUnit, Vec2)]) -> (Vec2, f32) {
    let mut px = Vec2::ZERO;
    let mut lines = 0.0;
    for (unit, d) in events {
        match unit {
            MouseScrollUnit::Pixel => px += *d,
            MouseScrollUnit::Line => lines += d.y,
        }
    }
    (px, lines)
}

/// Whether a tool currently owns left drags (set by the tool plugin).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ToolOwnsDrag(pub bool);

/// Whether egui wants the pointer this frame (set by the UI plugin).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointerOverUi(pub bool);

/// Camera plugin.
pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraRig>()
            .init_resource::<CameraCtl>()
            .init_resource::<ToolOwnsDrag>()
            .init_resource::<PointerOverUi>()
            .add_systems(
                Update,
                (gather_and_step, apply_rig)
                    .chain()
                    .in_set(crate::app::GameSet::Input),
            );
    }
}

/// Marker for the world camera.
#[derive(Component)]
pub struct WorldCamera;

/// Disables interactive control (screenshot suites, benchmarks).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct CameraScripted;

#[allow(clippy::too_many_arguments, reason = "Bevy system parameters")]
fn gather_and_step(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    mut pinch: MessageReader<PinchGesture>,
    mut twist: MessageReader<RotationGesture>,
    time: Res<Time>,
    over_ui: Res<PointerOverUi>,
    tool: Res<ToolOwnsDrag>,
    scripted: Option<Res<CameraScripted>>,
    terrain: Option<Res<cb_sim::terrain::TerrainRes>>,
    mut ctl: ResMut<CameraCtl>,
    mut rig: ResMut<CameraRig>,
) {
    let events: Vec<(MouseScrollUnit, Vec2)> = wheel
        .read()
        .map(|e| (e.unit, Vec2::new(e.x, e.y)))
        .collect();
    let pinch: f32 = pinch.read().map(|p| p.0).sum();
    let twist: f32 = twist.read().map(|r| r.0).sum();
    if scripted.is_some() {
        return;
    }
    let axis = |a: KeyCode, b: KeyCode, c: KeyCode, d: KeyCode| {
        f32::from(
            i8::from(keys.pressed(a) || keys.pressed(b))
                - i8::from(keys.pressed(c) || keys.pressed(d)),
        )
    };
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    let (scroll_px, lines) = if over_ui.0 {
        (Vec2::ZERO, 0.0)
    } else {
        split_scroll(&events)
    };
    let mut input = CameraInput {
        pan_keys: Vec2::new(
            axis(
                KeyCode::KeyD,
                KeyCode::ArrowRight,
                KeyCode::KeyA,
                KeyCode::ArrowLeft,
            ),
            axis(
                KeyCode::KeyW,
                KeyCode::ArrowUp,
                KeyCode::KeyS,
                KeyCode::ArrowDown,
            ),
        ),
        scroll_px,
        zoom_lines: lines,
        pinch,
        twist_rad: twist,
        rotate_key: i8::from(keys.pressed(KeyCode::KeyE)) - i8::from(keys.pressed(KeyCode::KeyQ)),
        ..default()
    };
    let delta = motion.delta;
    if !over_ui.0 {
        let left_cam = buttons.pressed(MouseButton::Left)
            && left_drag_target(false, alt, tool.0) == DragTarget::Camera;
        if buttons.pressed(MouseButton::Middle) || left_cam {
            input.drag_pan_px = delta;
        }
        if buttons.pressed(MouseButton::Right) {
            input.rotate_px = delta;
        }
    }
    let ground = |p: Vec2| terrain.as_ref().map_or(0.0, |t| t.terrain().height_at(p));
    let half = terrain.as_ref().map_or(2048.0, |t| {
        0.5 * (t.terrain().height().n() - 1) as f32 * t.terrain().height().spacing_m()
    });
    ctl.step(
        &mut rig,
        &input,
        time.delta_secs(),
        &Bounds {
            half_extent: half,
            ground: &ground,
        },
    );
}

fn apply_rig(
    rig: Res<CameraRig>,
    mut cams: Query<&mut Transform, With<WorldCamera>>,
    mut fog: ResMut<FogState>,
) {
    if !rig.is_changed() {
        return;
    }
    for mut t in &mut cams {
        *t = rig.transform();
    }
    // Clear radius grows with zoom so the town stays clear at every distance (ADR-0050).
    let r0 = 150.0 + 0.6 * rig.distance;
    fog.0.focus = rig.focus.extend(r0);
    fog.0.color.w = 0.0007;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(_: Vec2) -> f32 {
        0.0
    }

    fn bounds(g: &dyn Fn(Vec2) -> f32) -> Bounds<'_> {
        Bounds {
            half_extent: 2048.0,
            ground: g,
        }
    }

    #[test]
    fn camera_pan_rotation_relative() {
        let mut ctl = CameraCtl::default();
        let mut rig = CameraRig {
            yaw_deg: 90.0,
            ..Default::default()
        };
        let input = CameraInput {
            pan_keys: Vec2::Y,
            ..Default::default()
        };
        ctl.step(&mut rig, &input, 0.1, &bounds(&flat));
        // Yaw 90°: the camera sits on +x looking toward −x, so "forward" pans toward −x.
        assert!(
            rig.focus.x < -1.0 && rig.focus.z.abs() < 1e-3,
            "{:?}",
            rig.focus
        );
    }

    #[test]
    fn camera_zoom_clamped_25_2400() {
        let mut ctl = CameraCtl::default();
        let mut rig = CameraRig::default();
        ctl.step(
            &mut rig,
            &CameraInput {
                zoom_lines: 100.0,
                ..Default::default()
            },
            0.016,
            &bounds(&flat),
        );
        assert_eq!(rig.distance, 25.0);
        ctl.step(
            &mut rig,
            &CameraInput {
                zoom_lines: -100.0,
                ..Default::default()
            },
            0.016,
            &bounds(&flat),
        );
        assert_eq!(rig.distance, 2400.0);
    }

    #[test]
    fn camera_q_tap_snaps_to_next_90() {
        let mut ctl = CameraCtl::default();
        let mut rig = CameraRig {
            yaw_deg: 30.0,
            ..Default::default()
        };
        let b = bounds(&flat);
        ctl.step(
            &mut rig,
            &CameraInput {
                rotate_key: -1,
                ..Default::default()
            },
            0.1,
            &b,
        );
        for _ in 0..60 {
            ctl.step(&mut rig, &CameraInput::default(), 0.016, &b);
        }
        assert!((rig.yaw_deg - 0.0).abs() < 1e-4, "{}", rig.yaw_deg);
        ctl.step(
            &mut rig,
            &CameraInput {
                rotate_key: 1,
                ..Default::default()
            },
            0.1,
            &b,
        );
        for _ in 0..60 {
            ctl.step(&mut rig, &CameraInput::default(), 0.016, &b);
        }
        assert!((rig.yaw_deg - 90.0).abs() < 1e-4, "{}", rig.yaw_deg);
    }

    #[test]
    fn camera_q_hold_rotates_continuously() {
        let mut ctl = CameraCtl::default();
        let mut rig = CameraRig::default();
        let b = bounds(&flat);
        for _ in 0..100 {
            ctl.step(
                &mut rig,
                &CameraInput {
                    rotate_key: 1,
                    ..Default::default()
                },
                0.01,
                &b,
            );
        }
        // 1 s held: 0.25 s tap window, then 0.75 s at 90°/s.
        assert!((rig.yaw_deg - 67.5).abs() < 1.5, "{}", rig.yaw_deg);
    }

    #[test]
    fn camera_focus_clamped_to_bounds() {
        let mut ctl = CameraCtl::default();
        let mut rig = CameraRig {
            focus: Vec3::new(1990.0, 0.0, 0.0),
            yaw_deg: 270.0,
            ..Default::default()
        };
        for _ in 0..100 {
            ctl.step(
                &mut rig,
                &CameraInput {
                    pan_keys: Vec2::Y,
                    ..Default::default()
                },
                0.1,
                &bounds(&flat),
            );
        }
        assert!(rig.focus.x <= 2048.0 - EDGE_MARGIN_M + 1e-3);
    }

    #[test]
    fn camera_never_below_terrain_plus_6m() {
        let hill = |p: Vec2| if p.x > 20.0 { 80.0 } else { 0.0 };
        let mut ctl = CameraCtl::default();
        let mut rig = CameraRig {
            distance: 200.0,
            pitch_deg: 12.0,
            yaw_deg: 90.0,
            ..Default::default()
        };
        ctl.step(&mut rig, &CameraInput::default(), 0.016, &bounds(&hill));
        let e = rig.eye();
        assert!(
            e.y >= hill(Vec2::new(e.x, e.z)) + MIN_CLEARANCE_M - 1e-3,
            "{e}"
        );
    }

    #[test]
    fn input_tool_drag_precedence() {
        assert_eq!(left_drag_target(true, false, true), DragTarget::Ui);
        assert_eq!(left_drag_target(false, false, true), DragTarget::Tool);
        assert_eq!(left_drag_target(false, true, true), DragTarget::Camera);
        assert_eq!(left_drag_target(false, false, false), DragTarget::Camera);
    }

    #[test]
    fn scroll_unit_policy_pixel_pans_line_zooms() {
        let (px, lines) = split_scroll(&[
            (MouseScrollUnit::Pixel, Vec2::new(3.0, 4.0)),
            (MouseScrollUnit::Line, Vec2::new(0.0, 2.0)),
        ]);
        assert_eq!(px, Vec2::new(3.0, 4.0));
        assert_eq!(lines, 2.0);
    }
}
