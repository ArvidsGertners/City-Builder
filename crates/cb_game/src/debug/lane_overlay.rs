//! Lane graph debug overlay (ARCHITECTURE §3 `gizmo_overlays`, PROGRESS M3-10): lanes coloured
//! by kind with a direction arrow, connectors coloured by movement, conflict zones as circles.
//! Always compiled because screenshot suites use it; the F5 toggle is a `dev` tool. Developer
//! facing (no localization).

use bevy::prelude::*;
use cb_network::LaneKind;
use cb_network::connectors::ConnectorKind;
use cb_network::lane_graph::LaneTurn;
use cb_sim::lanes::LaneNetwork;

use crate::app::GameSet;

/// Lift above the road surface so lines are not hidden by the asphalt (m).
const LIFT_M: f32 = 0.25;

/// Which debug overlays are on.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct LaneOverlay(pub bool);

/// Lane overlay plugin.
pub struct LaneOverlayPlugin;

impl Plugin for LaneOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LaneOverlay>()
            .add_systems(Update, draw.in_set(GameSet::Presentation));
        #[cfg(feature = "dev")]
        app.add_systems(PreUpdate, toggle);
    }
}

#[cfg(feature = "dev")]
fn toggle(keys: Res<ButtonInput<KeyCode>>, mut on: ResMut<LaneOverlay>) {
    if keys.just_pressed(KeyCode::F5) {
        on.0 = !on.0;
    }
}

fn lane_color(k: LaneKind) -> Color {
    match k {
        LaneKind::Car => Color::srgb(0.25, 0.75, 1.0),
        LaneKind::Bus => Color::srgb(1.0, 0.35, 0.3),
        LaneKind::Tram => Color::srgb(1.0, 0.85, 0.2),
        LaneKind::CarTramShared => Color::srgb(0.6, 1.0, 0.4),
    }
}

fn turn_color(t: LaneTurn, kind: ConnectorKind) -> Color {
    if kind == ConnectorKind::Tram {
        return Color::srgb(1.0, 0.85, 0.2);
    }
    match t {
        LaneTurn::Straight => Color::srgb(0.95, 0.95, 0.95),
        LaneTurn::Left => Color::srgb(1.0, 0.55, 0.1),
        LaneTurn::Right => Color::srgb(0.3, 0.9, 0.35),
        LaneTurn::UTurn => Color::srgb(0.8, 0.4, 1.0),
    }
}

fn draw(mut gizmos: Gizmos, on: Res<LaneOverlay>, lanes: Option<Res<LaneNetwork>>) {
    let (true, Some(lanes)) = (on.0, lanes) else {
        return;
    };
    let g = lanes.graph();
    let lift = |p: Vec3| p + Vec3::Y * LIFT_M;
    for (_, l) in g.lanes() {
        let c = lane_color(l.spec.kind);
        gizmos.linestrip(l.path.points.iter().map(|p| lift(*p)), c);
        // Direction arrow over the last metres.
        let len = l.path.length();
        if len > 4.0 {
            let (a, b) = (l.path.point_at(len - 4.0), l.path.point_at(len - 0.5));
            gizmos.arrow(lift(a), lift(b), c).with_tip_length(1.5);
        }
    }
    for (_, c) in g.connectors() {
        gizmos.linestrip(
            c.points.iter().map(|p| lift(*p)),
            turn_color(c.turn, c.kind),
        );
    }
    for (_, z) in g.zones() {
        gizmos.circle(
            Isometry3d::new(
                lift(z.center) + Vec3::Y * 0.05,
                Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            ),
            z.radius_m,
            Color::srgb(1.0, 0.1, 0.25),
        );
    }
}
