//! Lighting director (ART_DIRECTION §3, ADR-0050): time-of-day keyframes interpolated in Oklab,
//! sun/moon direction, ambient, exposure offset, fog colour and the gradient sky dome. M2 uses a
//! static time (noon, or a debug preset); the day/night cycle drives `LightingState` from M11.

use bevy::asset::RenderAssetUsages;
use bevy::camera::Exposure;
use bevy::light::{CascadeShadowConfigBuilder, NotShadowCaster};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use super::FogState;

/// One keyframe (colours are sRGB hex).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Keyframe {
    /// Hour of day.
    pub hour: f32,
    /// Sun/moon colour.
    pub sun: u32,
    /// Sun intensity relative to noon (pre-exposure).
    pub sun_rel: f32,
    /// Ambient colour.
    pub ambient: u32,
    /// Ambient relative to noon.
    pub amb_rel: f32,
    /// Sky zenith.
    pub zenith: u32,
    /// Sky horizon.
    pub horizon: u32,
    /// Fog colour.
    pub fog: u32,
    /// Exposure offset (EV).
    pub ev: f32,
}

const fn k(
    hour: f32,
    sun: u32,
    sun_rel: f32,
    ambient: u32,
    amb_rel: f32,
    zenith: u32,
    horizon: u32,
    fog: u32,
    ev: f32,
) -> Keyframe {
    Keyframe {
        hour,
        sun,
        sun_rel,
        ambient,
        amb_rel,
        zenith,
        horizon,
        fog,
        ev,
    }
}

/// ART_DIRECTION §3 table.
pub const KEYFRAMES: [Keyframe; 9] = [
    k(
        0.0, 0x9DB2DF, 0.10, 0x3A4F80, 0.14, 0x142039, 0x2E4170, 0x263760, 1.2,
    ),
    k(
        5.0, 0xA9B6E0, 0.08, 0x55618F, 0.16, 0x24325A, 0x8C7FA3, 0x6C6A8E, 1.0,
    ),
    k(
        6.5, 0xFFB27A, 0.45, 0xA88B9A, 0.45, 0x5F7FB8, 0xFFC594, 0xE8B79A, 0.8,
    ),
    k(
        8.5, 0xFFE2BF, 0.85, 0xB5C4D6, 0.75, 0x6F9ED6, 0xCFE0EE, 0xD7E2EA, 0.0,
    ),
    k(
        12.0, 0xFFF3E2, 1.00, 0xBFD0E2, 0.80, 0x6A9FDC, 0xD6E6F2, 0xDCE7EE, 0.0,
    ),
    k(
        16.5, 0xFFE4C2, 0.90, 0xC3C7D3, 0.75, 0x6F9AD0, 0xE3E1DC, 0xE2DED6, 0.0,
    ),
    k(
        19.0, 0xFF9E5E, 0.50, 0xB48A8A, 0.50, 0x4F68A8, 0xFFAE78, 0xE9A882, 0.6,
    ),
    k(
        20.0, 0xC88AA0, 0.12, 0x6A6A98, 0.18, 0x2A3766, 0xB98AA0, 0x7E7398, 0.9,
    ),
    k(
        22.0, 0x9DB2DF, 0.10, 0x3A4F80, 0.14, 0x142039, 0x2E4170, 0x263760, 1.2,
    ),
];

/// Noon sun illuminance (lux) and ambient brightness at `rel = 1` (calibrated in M2).
pub const NOON_SUN_LUX: f32 = 10_000.0;
/// Ambient brightness at `amb_rel = 1`.
pub const NOON_AMBIENT: f32 = 650.0;
/// Fog luminance scale (pre-exposure luminance per unit linear fog colour).
pub const FOG_LUMINANCE: f32 = 7_500.0;

/// Interpolated lighting at one time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mood {
    /// Sun colour.
    pub sun: Color,
    /// Sun relative intensity.
    pub sun_rel: f32,
    /// Ambient colour.
    pub ambient: Color,
    /// Ambient relative intensity.
    pub amb_rel: f32,
    /// Sky zenith.
    pub zenith: Color,
    /// Sky horizon.
    pub horizon: Color,
    /// Fog colour.
    pub fog: Color,
    /// Exposure offset (EV).
    pub ev: f32,
}

fn hex(v: u32) -> Oklaba {
    Color::srgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8).into()
}

/// Mood at `hour` (wraps at 24): smoothstep-in-time, Oklab colour interpolation.
#[must_use]
pub fn mood(hour: f32) -> Mood {
    let h = hour.rem_euclid(24.0);
    let n = KEYFRAMES.len();
    let mut i = n - 1;
    for (k, kf) in KEYFRAMES.iter().enumerate() {
        if kf.hour <= h {
            i = k;
        }
    }
    let a = KEYFRAMES[i];
    let b = KEYFRAMES[(i + 1) % n];
    let span = (b.hour - a.hour).rem_euclid(24.0).max(1e-3);
    let t = ((h - a.hour).rem_euclid(24.0) / span).clamp(0.0, 1.0);
    let t = t * t * (3.0 - 2.0 * t);
    let mix = |x: u32, y: u32| Color::from(hex(x).mix(&hex(y), t));
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    Mood {
        sun: mix(a.sun, b.sun),
        sun_rel: lerp(a.sun_rel, b.sun_rel),
        ambient: mix(a.ambient, b.ambient),
        amb_rel: lerp(a.amb_rel, b.amb_rel),
        zenith: mix(a.zenith, b.zenith),
        horizon: mix(a.horizon, b.horizon),
        fog: mix(a.fog, b.fog),
        ev: lerp(a.ev, b.ev),
    }
}

/// Sun (day) or moon (night) direction *toward* the light for `hour`.
#[must_use]
pub fn light_direction(hour: f32) -> Vec3 {
    let h = hour.rem_euclid(24.0);
    let (elev, az) = if (6.0..=19.8).contains(&h) {
        let f = (h - 6.0) / 13.8;
        // Rises east (+x), culminates south (−z) at 55°, sets west.
        (
            ((f * std::f32::consts::PI).sin() * 55.0).max(6.0),
            90.0 + 180.0 * f,
        )
    } else {
        (38.0, 200.0)
    };
    let (e, a) = (elev.to_radians(), az.to_radians());
    Vec3::new(a.sin() * e.cos(), e.sin(), -a.cos() * e.cos()).normalize()
}

/// Time of day used for lighting (lighting only; the sim clock is separate in M2).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct LightingState {
    /// Hour of day 0–24.
    pub hour: f32,
}

impl Default for LightingState {
    fn default() -> Self {
        Self { hour: 12.0 }
    }
}

/// Marker for the sun/moon light.
#[derive(Component)]
pub struct SunLight;

/// Marker for the sky dome.
#[derive(Component)]
pub struct SkyDome;

/// Lighting plugin.
pub struct LightingPlugin;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LightingState>()
            .add_systems(Startup, spawn_lights)
            .add_systems(PostUpdate, apply_lighting);
    }
}

const DOME_R: f32 = 9_000.0;
const DOME_RINGS: usize = 12;
const DOME_SEG: usize = 32;

fn dome_mesh(zenith: Color, horizon: Color) -> Mesh {
    let (z, hz): (Oklaba, Oklaba) = (zenith.into(), horizon.into());
    let mut pos = Vec::new();
    let mut col = Vec::new();
    // Rings from below the horizon (−10°) to the zenith.
    for r in 0..=DOME_RINGS {
        let e = (-10.0 + 100.0 * r as f32 / DOME_RINGS as f32)
            .min(90.0)
            .to_radians();
        let t = (e.sin().max(0.0)).powf(0.6);
        let c = LinearRgba::from(Color::from(hz.mix(&z, t)));
        for s in 0..=DOME_SEG {
            let a = std::f32::consts::TAU * s as f32 / DOME_SEG as f32;
            pos.push([
                DOME_R * e.cos() * a.cos(),
                DOME_R * e.sin(),
                DOME_R * e.cos() * a.sin(),
            ]);
            col.push([c.red, c.green, c.blue, 1.0]);
        }
    }
    let row = (DOME_SEG + 1) as u32;
    let mut idx = Vec::new();
    for r in 0..DOME_RINGS as u32 {
        for s in 0..DOME_SEG as u32 {
            let a = r * row + s;
            // Seen from inside.
            idx.extend_from_slice(&[a, a + row, a + 1, a + 1, a + row, a + row + 1]);
        }
    }
    let normals: Vec<[f32; 3]> = pos
        .iter()
        .map(|p| (-Vec3::from(*p)).normalize_or(Vec3::Y).to_array())
        .collect();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
    .with_inserted_indices(Indices::U32(idx))
}

fn spawn_lights(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        DirectionalLight {
            illuminance: NOON_SUN_LUX,
            shadow_maps_enabled: true,
            color: Color::WHITE,
            ..default()
        },
        Transform::default(),
        CascadeShadowConfigBuilder {
            num_cascades: 4,
            first_cascade_far_bound: 60.0,
            maximum_distance: 900.0,
            ..default()
        }
        .build(),
        SunLight,
    ));
    let m = mood(12.0);
    commands.spawn((
        Mesh3d(meshes.add(dome_mesh(m.zenith, m.horizon))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            fog_enabled: false,
            cull_mode: None,
            ..default()
        })),
        Transform::default(),
        NotShadowCaster,
        SkyDome,
    ));
}

fn apply_lighting(
    state: Res<LightingState>,
    mut last: Local<Option<f32>>,
    mut sun: Query<
        (&mut DirectionalLight, &mut Transform),
        (With<SunLight>, Without<SkyDome>, Without<Camera3d>),
    >,
    mut dome: Query<(&Mesh3d, &mut Transform), (With<SkyDome>, Without<Camera3d>)>,
    cams: Query<&Transform, (With<Camera3d>, Without<SkyDome>)>,
    mut cam_exposure: Query<&mut Exposure, With<Camera3d>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut clear: ResMut<ClearColor>,
    mut fog: ResMut<FogState>,
) {
    // The dome follows the camera every frame.
    if let Some(ct) = cams.iter().next() {
        for (_, mut t) in &mut dome {
            t.translation = ct.translation;
        }
    }
    if *last == Some(state.hour) {
        return;
    }
    *last = Some(state.hour);
    let m = mood(state.hour);
    let dir = light_direction(state.hour);
    for (mut l, mut t) in &mut sun {
        l.color = m.sun;
        l.illuminance = NOON_SUN_LUX * m.sun_rel;
        *t = Transform::from_translation(dir).looking_at(Vec3::ZERO, Vec3::Y);
    }
    ambient.color = m.ambient;
    ambient.brightness = NOON_AMBIENT * m.amb_rel;
    clear.0 = m.horizon;
    for mut e in &mut cam_exposure {
        e.ev100 = Exposure::default().ev100 - m.ev;
    }
    for (mesh, _) in &dome {
        if let Some(mut mm) = meshes.get_mut(&mesh.0) {
            *mm = dome_mesh(m.zenith, m.horizon);
        }
    }
    let f = m.fog.to_linear();
    fog.0.color = Vec4::new(
        f.red * FOG_LUMINANCE,
        f.green * FOG_LUMINANCE,
        f.blue * FOG_LUMINANCE,
        fog.0.color.w,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyframes_hit_exactly_and_wrap() {
        let noon = mood(12.0);
        assert!((noon.sun_rel - 1.0).abs() < 1e-5 && (noon.amb_rel - 0.8).abs() < 1e-5);
        let late = mood(23.0);
        assert!(
            (late.sun_rel - 0.10).abs() < 1e-5,
            "22:00 → 24:00 night plateau"
        );
        assert!(light_direction(12.0).y > 0.8);
        assert!(light_direction(23.0).y > 0.5, "moon above the horizon");
        let morning = light_direction(8.5);
        assert!(morning.x > 0.3, "morning light from the east");
    }
}
