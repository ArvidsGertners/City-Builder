//! M1 placeholder scene: ground, a few flat-shaded blocks and trees, sun and sky. Proves the
//! render pipeline and gives the screenshot suite something to look at; replaced in M2.

use bevy::prelude::*;

/// Marker for the world camera.
#[derive(Component)]
pub struct WorldCamera;

/// Camera placement as focus + distance + angles (screenshot suites use the same form).
#[derive(Clone, Copy, PartialEq, Debug, serde::Deserialize)]
pub struct CameraPose {
    /// Point looked at (m).
    pub focus: [f32; 3],
    /// Distance from the focus (m).
    pub distance: f32,
    /// Elevation above the horizon (degrees).
    pub pitch_deg: f32,
    /// Heading around +Y (degrees).
    pub yaw_deg: f32,
}

impl CameraPose {
    /// The camera transform.
    #[must_use]
    pub fn transform(&self) -> Transform {
        let (p, y) = (self.pitch_deg.to_radians(), self.yaw_deg.to_radians());
        let focus = Vec3::from(self.focus);
        let offset = Vec3::new(p.cos() * y.sin(), p.sin(), p.cos() * y.cos()) * self.distance;
        Transform::from_translation(focus + offset).looking_at(focus, Vec3::Y)
    }
}

impl Default for CameraPose {
    fn default() -> Self {
        Self {
            focus: [0.0, 0.0, 0.0],
            distance: 70.0,
            pitch_deg: 32.0,
            yaw_deg: 35.0,
        }
    }
}

fn flat(mesh: impl Into<Mesh>) -> Mesh {
    let mut m: Mesh = mesh.into();
    m.duplicate_vertices();
    m.compute_flat_normals();
    m
}

/// Spawns the scene.
pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mat = |materials: &mut Assets<StandardMaterial>, srgb: [f32; 3], rough: f32| {
        materials.add(StandardMaterial {
            base_color: Color::srgb(srgb[0], srgb[1], srgb[2]),
            perceptual_roughness: rough,
            ..default()
        })
    };
    commands.insert_resource(ClearColor(Color::srgb(0.62, 0.76, 0.88)));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.85, 0.9, 1.0),
        brightness: 900.0,
        ..default()
    });

    commands.spawn((
        Camera3d::default(),
        CameraPose::default().transform(),
        WorldCamera,
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 9_000.0,
            shadow_maps_enabled: true,
            color: Color::srgb(1.0, 0.95, 0.86),
            ..default()
        },
        Transform::from_xyz(40.0, 60.0, 25.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    let grass = mat(&mut materials, [0.48, 0.62, 0.36], 0.95);
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(400.0, 400.0))),
        MeshMaterial3d(grass),
    ));
    let road = mat(&mut materials, [0.36, 0.36, 0.38], 0.9);
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(80.0, 0.1, 7.0))),
        MeshMaterial3d(road),
        Transform::from_xyz(0.0, 0.05, 0.0),
    ));

    let walls = [
        [0.93, 0.87, 0.76],
        [0.86, 0.72, 0.6],
        [0.95, 0.93, 0.88],
        [0.78, 0.82, 0.86],
    ];
    let roof = mat(&mut materials, [0.66, 0.3, 0.24], 0.8);
    let roof_mesh = meshes.add(flat(Extrusion::new(
        Triangle2d::new(
            // Base 10 m over a 9.4 m deep house, 8.8 m long over an 8 m front: 0.3–0.4 m eaves.
            Vec2::new(-5.0, 0.0),
            Vec2::new(5.0, 0.0),
            Vec2::new(0.0, 3.2),
        ),
        8.8,
    )));
    for (i, x) in [-28.0_f32, -16.0, -4.0, 8.0, 20.0].into_iter().enumerate() {
        for side in [-1.0_f32, 1.0] {
            let h = 5.0 + ((i as f32) * 1.7 + side).rem_euclid(3.0);
            let wall = mat(
                &mut materials,
                walls[(i + usize::from(side > 0.0)) % walls.len()],
                0.85,
            );
            let z = side * 11.0;
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(8.0, h, 9.4))),
                MeshMaterial3d(wall),
                Transform::from_xyz(x, h / 2.0, z),
            ));
            commands.spawn((
                Mesh3d(roof_mesh.clone()),
                MeshMaterial3d(roof.clone()),
                Transform::from_xyz(x, h, z)
                    .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)),
            ));
        }
    }

    let leaves = mat(&mut materials, [0.3, 0.5, 0.28], 0.9);
    let trunk = mat(&mut materials, [0.42, 0.3, 0.2], 0.9);
    let crown = meshes.add(flat(
        Sphere::new(2.4)
            .mesh()
            .ico(1)
            .unwrap_or_else(|_| Sphere::new(2.4).mesh().uv(8, 6)),
    ));
    let stem = meshes.add(Cylinder::new(0.3, 2.4));
    for (x, z) in [
        (-34.0, 20.0),
        (-22.0, 24.0),
        (30.0, -22.0),
        (34.0, 18.0),
        (-10.0, -26.0),
        (14.0, 26.0),
    ] {
        commands.spawn((
            Mesh3d(stem.clone()),
            MeshMaterial3d(trunk.clone()),
            Transform::from_xyz(x, 1.2, z),
        ));
        commands.spawn((
            Mesh3d(crown.clone()),
            MeshMaterial3d(leaves.clone()),
            Transform::from_xyz(x, 4.2, z),
        ));
    }
}
