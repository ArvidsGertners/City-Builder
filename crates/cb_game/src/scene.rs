//! Scene setup: the world camera with the post-processing stack validated in S-Render, and the
//! starting content of a new game. M2 starts with a demo town on the flattest dry site near the
//! map centre (citygen); the new-game flow with an empty map arrives with the menus (M15).

use bevy::anti_alias::smaa::Smaa;
use bevy::camera::{Exposure, Hdr};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::pbr::ScreenSpaceAmbientOcclusion;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use cb_sim::terrain::TerrainRes;

use crate::camera::CameraRig;
pub use crate::camera::WorldCamera;

/// Camera placement as focus + distance + angles (screenshot suites use the same form).
#[derive(Clone, Copy, PartialEq, Debug, serde::Deserialize)]
pub struct CameraPose {
    /// Point looked at (m), relative to the scene anchor in scripted scenes.
    pub focus: [f32; 3],
    /// Distance from the focus (m).
    pub distance: f32,
    /// Elevation above the horizon (degrees).
    pub pitch_deg: f32,
    /// Heading around +Y (degrees).
    pub yaw_deg: f32,
}

impl CameraPose {
    /// As a camera rig.
    #[must_use]
    pub fn rig(&self) -> CameraRig {
        CameraRig {
            focus: Vec3::from(self.focus),
            distance: self.distance,
            pitch_deg: self.pitch_deg,
            yaw_deg: self.yaw_deg,
        }
    }

    /// The camera transform.
    #[must_use]
    pub fn transform(&self) -> Transform {
        self.rig().transform()
    }
}

impl Default for CameraPose {
    fn default() -> Self {
        Self {
            focus: [0.0, 0.0, 0.0],
            distance: 300.0,
            pitch_deg: 45.0,
            yaw_deg: 0.0,
        }
    }
}

/// Where the starting town is (scripted scenes and screenshot suites place shots relative to it).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct SceneAnchor(pub Vec3);

/// A named place in the starting scene (scripted shots frame it); `yaw_deg` looks across the
/// road there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FeatureAnchor {
    /// Ground position.
    pub pos: Vec3,
    /// Camera heading looking across the road (degrees).
    pub yaw_deg: f32,
    /// Road direction (unit, ground plane).
    pub dir: Vec2,
}

/// Named feature anchors of the starting scene (`town`, `map` (centre), `slope`, `fill`, `cut`,
/// `shore`, `corner`, `steep`).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct SceneFeatures(pub std::collections::BTreeMap<String, FeatureAnchor>);

/// Spawns the world camera.
pub fn setup(mut commands: Commands, rig: Res<CameraRig>) {
    commands.spawn((
        Camera3d::default(),
        // Far enough for the 2 400 m zoom over a 4 096 m map and the sky dome.
        Projection::Perspective(PerspectiveProjection {
            far: 20_000.0,
            near: 0.5,
            ..default()
        }),
        Hdr,
        Msaa::Off,
        Tonemapping::TonyMcMapface,
        Bloom::NATURAL,
        Smaa::default(),
        ScreenSpaceAmbientOcclusion::default(),
        Exposure {
            ev100: crate::render::lighting::BASE_EV100,
        },
        rig.transform(),
        WorldCamera,
    ));
}

/// The flattest dry square of `size_m` near the map centre (mean slope over a sample grid).
#[must_use]
pub fn find_site(t: &cb_terrain::Terrain, size_m: f32) -> Vec2 {
    let h = t.height();
    let half = 0.5 * (h.n() - 1) as f32 * h.spacing_m();
    let reach = (half * 0.6).min(1400.0);
    let mut best = (f32::INFINITY, Vec2::ZERO);
    let step = 96.0;
    let n = (2.0 * reach / step) as i32;
    for gz in 0..=n {
        for gx in 0..=n {
            let c = Vec2::new(-reach + gx as f32 * step, -reach + gz as f32 * step);
            let mut cost = 0.0;
            let k = 8;
            for sz in 0..=k {
                for sx in 0..=k {
                    let p = c + Vec2::new(sx as f32 / k as f32 - 0.5, sz as f32 / k as f32 - 0.5)
                        * size_m;
                    if t.water().is_water(p) {
                        cost += 50.0;
                    }
                    cost += h.slope_at(p);
                }
            }
            cost += c.length() / 4000.0;
            if cost < best.0 {
                best = (cost, c);
            }
        }
    }
    best.1
}

/// Marker: start without the demo town (the `empty` benchmark scene).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct NoDemoTown;

/// Builds the demo town and points the camera at it.
pub fn demo_town(world: &mut World) {
    if world.contains_resource::<NoDemoTown>() {
        return;
    }
    let Some(site) = world
        .get_resource::<TerrainRes>()
        .map(|t| find_site(t.terrain(), 420.0))
    else {
        return;
    };
    let spec = cb_sim::citygen::GridSpec {
        center: site,
        blocks: (4, 3),
        ..Default::default()
    };
    let rep = cb_sim::citygen::grid(world, &spec);
    tracing::info!("demo town at {site}: {rep:?}");
    let half = Vec2::new(spec.blocks.0 as f32, spec.blocks.1 as f32) * spec.block_m * 0.5;
    let f = cb_sim::citygen::m2_features(world, site, half);
    tracing::info!("scripted features: {:?}", f.report);
    let terrain = world.get_resource::<TerrainRes>();
    let ground = |p: Vec2| terrain.map_or(0.0, |t| t.terrain().height_at(p));
    let y = ground(site);
    let mut features = SceneFeatures::default();
    let mut put = |name: &str, pos: Vec2, dir: Vec2| {
        // Eye offset (sin yaw, cos yaw) perpendicular to the road.
        let n = dir.perp();
        let yaw_deg = n.x.atan2(n.y).to_degrees();
        features.0.insert(
            name.to_owned(),
            FeatureAnchor {
                pos: Vec3::new(pos.x, ground(pos), pos.y),
                yaw_deg,
                dir,
            },
        );
    };
    put("town", site, Vec2::X);
    put("map", Vec2::ZERO, Vec2::X);
    for (name, feat) in [
        ("slope", f.slope),
        ("fill", f.fill),
        ("cut", f.cut),
        ("shore", f.shore),
        ("steep", f.steep),
    ] {
        if let Some(ft) = feat {
            put(name, ft.pos, ft.dir);
        }
    }
    if let Some(c) = f.corner {
        put("corner", c, Vec2::X);
    }
    world.insert_resource(features);
    world.insert_resource(SceneAnchor(Vec3::new(site.x, y, site.y)));
    if let Some(mut rig) = world.get_resource_mut::<CameraRig>() {
        *rig = CameraRig {
            focus: Vec3::new(site.x, y, site.y),
            distance: 420.0,
            pitch_deg: 42.0,
            yaw_deg: 25.0,
        };
    }
}
