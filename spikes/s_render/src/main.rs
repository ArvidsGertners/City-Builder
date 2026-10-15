//! Spike S-Render (PROGRESS.md Phase S): WorldMaterial with the ADR-0049 20-byte vertex layout,
//! derivative flat normals in forward + prepass, ShaderBuffer slot lookup, CSM shadows, SSAO,
//! SMAA, bloom, TonyMcMapface, focus fog, and render scale via offscreen composite or
//! `MainPassResolutionOverride`. Throwaway code: not promoted into `crates/` as-is.
//!
//! Usage: s_render [--scale-mode native|composite|override] [--render-scale F]
//!                 [--view lit|normals] [--out FILE.png] [--frames N] [--size WxH]

use bevy::anti_alias::smaa::Smaa;
use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{Hdr, ImageRenderTarget, MainPassResolutionOverride, RenderTarget, ScalingMode};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{CascadeShadowConfigBuilder, NotShadowCaster};
use bevy::mesh::{Indices, MeshVertexAttribute, MeshVertexBufferLayoutRef, PrimitiveTopology, VertexAttributeValues};
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
    ScreenSpaceAmbientOcclusion,
};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError, TextureFormat,
    VertexFormat,
};
use bevy::render::storage::ShaderBuffer;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::shader::ShaderRef;
use bevy::window::WindowResolution;
use bevy::shader::{Shader, ShaderCache, ShaderCacheError, ShaderCacheSource, ShaderDefVal, ValidateShader};
use std::sync::Mutex;

mod picking;

/// Pipeline variants seen by `specialize`: (shader, defs, stage).
static VARIANTS: Mutex<Vec<(AssetId<Shader>, Vec<ShaderDefVal>, &'static str)>> = Mutex::new(Vec::new());
/// Per composed module: backend translation results.
static RESULTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Vertex color: unorm8x4, rgb = albedo (sRGB-encoded bytes, linearized on the CPU before
/// packing), a = baked AO.
const ATTRIBUTE_WORLD_COLOR: MeshVertexAttribute =
    MeshVertexAttribute::new("WorldColor", 0x5EED_0001, VertexFormat::Unorm8x4);
/// Packed u32: slot (low 16 bits) | surface flags (high 16 bits).
const ATTRIBUTE_PACKED: MeshVertexAttribute =
    MeshVertexAttribute::new("WorldPacked", 0x5EED_0002, VertexFormat::Uint32);

type WorldMaterial = ExtendedMaterial<StandardMaterial, WorldExt>;

#[derive(Clone, Copy, Debug, ShaderType, Reflect, Default)]
struct FocusFog {
    focus: Vec4,
    color: Vec4,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
struct WorldExt {
    #[storage(100, read_only, visibility(vertex, fragment))]
    slots: Handle<ShaderBuffer>,
    #[uniform(101)]
    fog: FocusFog,
}

impl MaterialExtension for WorldExt {
    fn vertex_shader() -> ShaderRef {
        "embedded://s_render/world.wesl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "embedded://s_render/world.wesl".into()
    }
    fn prepass_vertex_shader() -> ShaderRef {
        "embedded://s_render/world_prepass.wesl".into()
    }
    fn prepass_fragment_shader() -> ShaderRef {
        "embedded://s_render/world_prepass.wesl".into()
    }

    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            ATTRIBUTE_WORLD_COLOR.at_shader_location(5),
            ATTRIBUTE_PACKED.at_shader_location(8),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        descriptor.vertex.shader_defs.push("VERTEX_COLORS".into());
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.shader_defs.push("VERTEX_COLORS".into());
        }
        if let Ok(mut v) = VARIANTS.lock() {
            v.push((descriptor.vertex.shader.id(), descriptor.vertex.shader_defs.clone(), "vertex"));
            if let Some(f) = descriptor.fragment.as_ref() {
                v.push((f.shader.id(), f.shader_defs.clone(), "fragment"));
            }
        }
        Ok(())
    }
}

/// Debug material showing the prepass normal buffer.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
struct ShowNormals {}

impl Material for ShowNormals {
    fn fragment_shader() -> ShaderRef {
        "embedded://s_render/show_normals.wesl".into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum ScaleMode {
    Native,
    Composite,
    Override,
}

#[derive(Resource, Clone, Debug)]
struct Args {
    scale_mode: ScaleMode,
    render_scale: f32,
    normals_view: bool,
    validate: bool,
    fog_density: f32,
    fog_r0: f32,
    out: Option<String>,
    frames: u32,
    size: (u32, u32),
}

fn parse_args() -> Args {
    let mut a = Args {
        scale_mode: ScaleMode::Native,
        render_scale: 1.0,
        normals_view: false,
        validate: false,
        fog_density: 0.004,
        fog_r0: 60.0,
        out: None,
        frames: 40,
        size: (1600, 900),
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let v = args.get(i + 1).cloned().unwrap_or_default();
        match args[i].as_str() {
            "--scale-mode" => {
                a.scale_mode = match v.as_str() {
                    "composite" => ScaleMode::Composite,
                    "override" => ScaleMode::Override,
                    _ => ScaleMode::Native,
                };
                i += 1;
            }
            "--render-scale" => {
                a.render_scale = v.parse().unwrap_or(1.0);
                i += 1;
            }
            "--view" => {
                a.normals_view = v == "normals";
                i += 1;
            }
            "--validate-shaders" => {
                a.validate = true;
            }
            "--fog-r0" => {
                a.fog_r0 = v.parse().unwrap_or(60.0);
                i += 1;
            }
            "--fog-density" => {
                a.fog_density = v.parse().unwrap_or(0.004);
                i += 1;
            }
            "--out" => {
                a.out = Some(v);
                i += 1;
            }
            "--frames" => {
                a.frames = v.parse().unwrap_or(40);
                i += 1;
            }
            "--size" => {
                if let Some((w, h)) = v.split_once('x') {
                    a.size = (w.parse().unwrap_or(1600), h.parse().unwrap_or(900));
                }
                i += 1;
            }
            other => eprintln!("unknown arg {other}"),
        }
        i += 1;
    }
    a
}

fn main() {
    let args = parse_args();
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "S-Render".into(),
            resolution: WindowResolution::new(args.size.0, args.size.1),
            resizable: false,
            ..default()
        }),
        ..default()
    }))
    .add_plugins((
        MaterialPlugin::<WorldMaterial>::default(),
        MaterialPlugin::<ShowNormals>::default(),
    ))
    .insert_resource(args)
    .insert_resource(ClearColor(Color::srgb(0.62, 0.76, 0.88)))
    .add_systems(Startup, setup)
    .add_systems(Last, (capture, validate_shaders));
    embedded_asset!(app, "world.wesl");
    embedded_asset!(app, "world_prepass.wesl");
    embedded_asset!(app, "show_normals.wesl");
    app.run();
}

/// Builds meshes in the ADR-0049 layout: indexed, shared vertices, no normals.
#[derive(Default)]
struct WorldMeshBuilder {
    positions: Vec<[f32; 3]>,
    colors: Vec<[u8; 4]>,
    packed: Vec<u32>,
    indices: Vec<u32>,
}

fn srgb_byte_to_linear_byte(c: u8) -> u8 {
    // Unorm8 is linear in the shader; store linearized albedo so StandardMaterial math holds.
    let s = f32::from(c) / 255.0;
    let l = if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) };
    (l * 255.0).round() as u8
}

impl WorldMeshBuilder {
    fn vertex(&mut self, p: Vec3, rgb: [u8; 3], ao: f32, slot: u16) -> u32 {
        let idx = self.positions.len() as u32;
        self.positions.push(p.to_array());
        self.colors.push([
            srgb_byte_to_linear_byte(rgb[0]),
            srgb_byte_to_linear_byte(rgb[1]),
            srgb_byte_to_linear_byte(rgb[2]),
            (ao.clamp(0.0, 1.0) * 255.0).round() as u8,
        ]);
        self.packed.push(u32::from(slot));
        idx
    }

    fn quad(&mut self, a: u32, b: u32, c: u32, d: u32) {
        // Counter-clockwise when seen from outside.
        self.indices.extend_from_slice(&[a, b, c, a, c, d]);
    }

    /// A gable house: walls share 8 corner vertices, the roof has its own (color split).
    fn house(&mut self, base: Vec3, w: f32, d: f32, h: f32, roof_h: f32, wall: [u8; 3], roof: [u8; 3], slot: u16) {
        let (hw, hd) = (w / 2.0, d / 2.0);
        let corners = [
            Vec3::new(-hw, 0.0, -hd),
            Vec3::new(hw, 0.0, -hd),
            Vec3::new(hw, 0.0, hd),
            Vec3::new(-hw, 0.0, hd),
        ];
        let bottom: Vec<u32> = corners.iter().map(|c| self.vertex(base + *c, wall, 0.82, slot)).collect();
        let top: Vec<u32> = corners
            .iter()
            .map(|c| self.vertex(base + *c + Vec3::Y * h, wall, 1.0, slot))
            .collect();
        for i in 0..4 {
            let j = (i + 1) % 4;
            self.quad(bottom[j], bottom[i], top[i], top[j]);
        }
        // Gable triangles in the wall color (ridge along x).
        let ridge_w = [
            self.vertex(base + Vec3::new(-hw, h + roof_h, 0.0), wall, 1.0, slot),
            self.vertex(base + Vec3::new(hw, h + roof_h, 0.0), wall, 1.0, slot),
        ];
        self.indices.extend_from_slice(&[top[1], ridge_w[1], top[2]]); // +x gable
        self.indices.extend_from_slice(&[top[3], ridge_w[0], top[0]]); // -x gable
        // Roof with 0.4 m eaves.
        let e = 0.4;
        let rv = |b: &mut Self, p: Vec3| b.vertex(base + p, roof, 0.95, slot);
        let r0 = rv(self, Vec3::new(-hw - e, h - 0.25, -hd - e));
        let r1 = rv(self, Vec3::new(hw + e, h - 0.25, -hd - e));
        let r2 = rv(self, Vec3::new(hw + e, h - 0.25, hd + e));
        let r3 = rv(self, Vec3::new(-hw - e, h - 0.25, hd + e));
        let k0 = rv(self, Vec3::new(-hw - e, h + roof_h, 0.0));
        let k1 = rv(self, Vec3::new(hw + e, h + roof_h, 0.0));
        self.quad(r0, k0, k1, r1); // -z slope
        self.quad(r2, k1, k0, r3); // +z slope
        // Undersides so eaves are not see-through from below.
        self.quad(r1, k1, k0, r0);
        self.quad(r3, k0, k1, r2);
    }

    /// A building wall grid subdivided to roughly `target_tris` triangles (memory metric).
    fn detailed_block(&mut self, base: Vec3, size: Vec3, target_tris: usize, color: [u8; 3], slot: u16) {
        // 4 walls + roof, each n×n quads → 5 × 2n² triangles.
        let n = (((target_tris as f32) / 10.0).sqrt().round() as usize).max(1);
        // (origin, u, v) with u × v pointing outward.
        let faces: [(Vec3, Vec3, Vec3); 5] = [
            (Vec3::new(size.x, 0.0, 0.0), -Vec3::X * size.x, Vec3::Y * size.y),
            (Vec3::new(size.x, 0.0, size.z), -Vec3::Z * size.z, Vec3::Y * size.y),
            (Vec3::new(0.0, 0.0, size.z), Vec3::X * size.x, Vec3::Y * size.y),
            (Vec3::new(0.0, 0.0, 0.0), Vec3::Z * size.z, Vec3::Y * size.y),
            (Vec3::new(0.0, size.y, 0.0), Vec3::Z * size.z, Vec3::X * size.x),
        ];
        for (origin, u, v) in faces {
            let start = self.positions.len() as u32;
            for j in 0..=n {
                for i in 0..=n {
                    let p = base + origin + u * (i as f32 / n as f32) + v * (j as f32 / n as f32);
                    // Slight inset pattern so the grid is not coplanar (window recesses).
                    let inset = if i % 2 == 1 && j % 2 == 1 && j < n { 0.15 } else { 0.0 };
                    let normal = u.cross(v).normalize();
                    self.vertex(p - normal * inset, color, if j == 0 { 0.82 } else { 1.0 }, slot);
                }
            }
            let row = (n + 1) as u32;
            for j in 0..n as u32 {
                for i in 0..n as u32 {
                    let a = start + j * row + i;
                    self.quad(a, a + 1, a + row + 1, a + row);
                }
            }
        }
    }

    fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Bytes of GPU vertex + index data (u16 indices when possible).
    fn gpu_bytes(&self) -> usize {
        let index_size = if self.positions.len() <= usize::from(u16::MAX) { 2 } else { 4 };
        self.positions.len() * 20 + self.indices.len() * index_size
    }

    fn build(self) -> Mesh {
        let indices = if self.positions.len() <= usize::from(u16::MAX) {
            Indices::U16(self.indices.iter().map(|&i| i as u16).collect())
        } else {
            Indices::U32(self.indices)
        };
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(ATTRIBUTE_WORLD_COLOR, VertexAttributeValues::Unorm8x4(self.colors))
            .with_inserted_attribute(ATTRIBUTE_PACKED, VertexAttributeValues::Uint32(self.packed))
            .with_inserted_indices(indices)
    }
}

fn camera_transform() -> Transform {
    let (pitch, yaw, dist) = (32.0_f32.to_radians(), 35.0_f32.to_radians(), 70.0);
    let off = Vec3::new(pitch.cos() * yaw.sin(), pitch.sin(), pitch.cos() * yaw.cos()) * dist;
    Transform::from_translation(off).looking_at(Vec3::ZERO, Vec3::Y)
}

fn setup(
    mut commands: Commands,
    args: Res<Args>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut std_materials: ResMut<Assets<StandardMaterial>>,
    mut world_materials: ResMut<Assets<WorldMaterial>>,
    mut normal_materials: ResMut<Assets<ShowNormals>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut images: ResMut<Assets<Image>>,
) {
    // Per-slot tints (a real game indexes per-building params here).
    let slots: Vec<[f32; 4]> = (0..64)
        .map(|i| {
            let t = (i as f32 * 0.618_034).fract();
            [0.92 + 0.08 * t, 0.95 + 0.05 * (1.0 - t), 0.93 + 0.07 * (t * 2.0).fract(), 1.0]
        })
        .collect();
    let slots = buffers.add(ShaderBuffer::from(slots));
    let world_material = world_materials.add(WorldMaterial {
        base: StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.9, ..default() },
        extension: WorldExt {
            slots,
            fog: FocusFog {
                focus: Vec4::new(0.0, 0.0, 0.0, args.fog_r0),
                // Fog luminance ≈ a light-grey surface (albedo 0.6–0.8) under the 10 klx sun.
                color: Vec4::new(5500.0, 6600.0, 7800.0, args.fog_density),
            },
        },
    });

    // Houses along a street, one merged mesh (chunk-style).
    let mut b = WorldMeshBuilder::default();
    let walls = [[0xEE, 0xDF, 0xC2], [0xDB, 0xB8, 0x99], [0xF2, 0xEE, 0xE2], [0xC7, 0xD1, 0xDB]];
    let roofs = [[0xB0, 0x4E, 0x3C], [0x8E, 0x4A, 0x3A], [0x6E, 0x5A, 0x52]];
    let mut slot = 0u16;
    for (i, x) in [-28.0_f32, -16.0, -4.0, 8.0, 20.0].into_iter().enumerate() {
        for side in [-1.0_f32, 1.0] {
            let h = 5.0 + ((i as f32) * 1.7 + side).rem_euclid(3.0);
            b.house(
                Vec3::new(x, 0.0, side * 11.0),
                8.0,
                9.4,
                h,
                3.2,
                walls[(i + usize::from(side > 0.0)) % walls.len()],
                roofs[i % roofs.len()],
                slot,
            );
            slot += 1;
        }
    }
    let houses_tris = b.triangle_count();
    commands.spawn((Mesh3d(meshes.add(b.build())), MeshMaterial3d(world_material.clone())));

    // A ~1k-triangle building for the memory metric.
    let mut d = WorldMeshBuilder::default();
    d.detailed_block(Vec3::new(30.0, 0.0, -30.0), Vec3::new(12.0, 18.0, 10.0), 1000, [0xE6, 0xD5, 0xB8], 40);
    let (d_tris, d_bytes, d_verts) = (d.triangle_count(), d.gpu_bytes(), d.positions.len());
    commands.spawn((Mesh3d(meshes.add(d.build())), MeshMaterial3d(world_material.clone())));
    let per_1k = d_bytes as f64 / (d_tris as f64 / 1000.0) / 1024.0;
    println!(
        "metric=memory tris={d_tris} verts={d_verts} bytes={d_bytes} kib_per_1k_tris={per_1k:.2} (houses chunk: {houses_tris} tris)"
    );

    // Ground (smooth-shaded terrain stays on StandardMaterial with normals).
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(400.0, 400.0))),
        MeshMaterial3d(std_materials.add(StandardMaterial {
            base_color: Color::srgb_u8(0x8D, 0xB0, 0x6A),
            perceptual_roughness: 0.95,
            ..default()
        })),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(80.0, 0.1, 7.0))),
        MeshMaterial3d(std_materials.add(StandardMaterial {
            base_color: Color::srgb_u8(0x6E, 0x6E, 0x72),
            perceptual_roughness: 0.9,
            ..default()
        })),
        Transform::from_xyz(0.0, 0.05, 0.0),
    ));

    // Sun with cascaded shadows.
    commands.spawn((
        DirectionalLight {
            illuminance: 10_000.0,
            shadow_maps_enabled: true,
            color: Color::srgb_u8(0xFF, 0xF3, 0xE2),
            ..default()
        },
        Transform::from_xyz(40.0, 60.0, 25.0).looking_at(Vec3::ZERO, Vec3::Y),
        CascadeShadowConfigBuilder { num_cascades: 3, maximum_distance: 250.0, ..default() }.build(),
    ));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb_u8(0xBF, 0xD0, 0xE2),
        brightness: 500.0,
        ..default()
    });

    // Cameras.
    let (w, h) = args.size;
    let scaled = UVec2::new(
        ((w as f32) * args.render_scale).round().max(1.0) as u32,
        ((h as f32) * args.render_scale).round().max(1.0) as u32,
    );
    let world_cam = (
        Camera3d::default(),
        Hdr,
        Msaa::Off,
        Tonemapping::TonyMcMapface,
        Bloom::NATURAL,
        Smaa::default(),
        ScreenSpaceAmbientOcclusion::default(),
        camera_transform(),
    );
    let cam = match args.scale_mode {
        ScaleMode::Native => commands.spawn(world_cam).id(),
        ScaleMode::Override => commands.spawn((world_cam, MainPassResolutionOverride(scaled))).id(),
        ScaleMode::Composite => {
            let target = images.add(Image::new_target_texture(
                scaled.x,
                scaled.y,
                TextureFormat::Rgba8UnormSrgb,
                None,
            ));
            let cam = commands
                .spawn((world_cam, RenderTarget::Image(ImageRenderTarget::from(target.clone()))))
                .id();
            // Composite: orthographic camera drawing a full-screen quad with the scaled image.
            commands.spawn((
                Camera3d::default(),
                Camera { order: 1, ..default() },
                Projection::Orthographic(OrthographicProjection {
                    scaling_mode: ScalingMode::Fixed { width: 2.0, height: 2.0 },
                    ..OrthographicProjection::default_3d()
                }),
                Tonemapping::None,
                Msaa::Off,
                RenderLayers::layer(1),
                Transform::from_xyz(0.0, 0.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
            ));
            commands.spawn((
                Mesh3d(meshes.add(Rectangle::new(2.0, 2.0))),
                MeshMaterial3d(std_materials.add(StandardMaterial {
                    base_color_texture: Some(target),
                    unlit: true,
                    ..default()
                })),
                RenderLayers::layer(1),
                NotShadowCaster,
            ));
            cam
        }
    };

    if args.normals_view {
        // Quad just in front of the camera, sampling the prepass normal buffer.
        let fov = std::f32::consts::FRAC_PI_4;
        let dist = 0.2;
        let qh = 2.0 * dist * (fov / 2.0).tan() * 1.05;
        let qw = qh * (w as f32 / h as f32);
        commands.entity(cam).with_children(|c| {
            c.spawn((
                Mesh3d(meshes.add(Rectangle::new(qw, qh))),
                MeshMaterial3d(normal_materials.add(ShowNormals {})),
                Transform::from_xyz(0.0, 0.0, -dist),
                NotShadowCaster,
            ));
        });
    }
}

#[derive(Default)]
struct CaptureState {
    frames: u32,
    requested: bool,
    exit_in: Option<u32>,
}

fn capture(
    mut commands: Commands,
    args: Res<Args>,
    mut state: Local<CaptureState>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(out) = args.out.clone() else { return };
    state.frames += 1;
    if let Some(n) = state.exit_in.as_mut() {
        if *n == 0 {
            exit.write(AppExit::Success);
        } else {
            *n -= 1;
        }
        return;
    }
    if !state.requested && state.frames >= args.frames {
        state.requested = true;
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(out))
            .observe(|_: On<ScreenshotCaptured>| {});
        state.exit_in = Some(30);
    }
}

/// GPU-free validation: compose every recorded variant with Bevy's own `ShaderCache` (WESL →
/// WGSL), then translate with naga to SPIR-V, MSL and HLSL.
fn validate_shaders(world: &mut World, mut frames: Local<u32>) {
    let validate = world.resource::<Args>().validate;
    if !validate {
        return;
    }
    *frames += 1;
    if *frames != 30 {
        return;
    }
    let mut variants = VARIANTS.lock().map(|v| v.clone()).unwrap_or_default();
    variants.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    variants.dedup_by(|a, b| format!("{a:?}") == format!("{b:?}"));
    let mut cache: ShaderCache<(), ()> = ShaderCache::new((), load_module);
    for (id, shader) in world.resource::<Assets<Shader>>().iter() {
        cache.set_shader(id, shader.clone());
    }
    let globals: Vec<ShaderDefVal> = vec![
        ShaderDefVal::UInt("AVAILABLE_STORAGE_BUFFER_BINDINGS".into(), 8),
        ShaderDefVal::Bool("AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_3".into(), true),
        ShaderDefVal::Bool("AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_6".into(), true),
    ];
    let mut failures = 0;
    for (i, (id, defs, stage)) in variants.iter().enumerate() {
        let mut all = globals.clone();
        all.extend(defs.iter().cloned());
        if let Err(e) = cache.get(i, *id, &all) {
            failures += 1;
            println!("validate: compose FAILED {stage} {id:?}: {e}");
        }
    }
    let results = RESULTS.lock().map(|r| r.clone()).unwrap_or_default();
    let bad: Vec<&String> = results.iter().filter(|r| !r.ends_with("ok")).collect();
    for r in &bad {
        println!("validate: {r}");
    }
    println!(
        "metric=shader_validation variants={} composed_modules={} compose_failures={failures} backend_failures={}",
        variants.len(),
        results.len(),
        bad.len()
    );
    world.write_message(AppExit::Success);
}

fn load_module(_: &(), source: ShaderCacheSource, _: &ValidateShader) -> Result<(), ShaderCacheError> {
    let ShaderCacheSource::Wgsl(wgsl) = source else { return Ok(()) };
    let mut report = String::new();
    let module = naga::front::wgsl::parse_str(&wgsl)
        .map_err(|e| ShaderCacheError::CreateShaderModule(e.emit_to_string(&wgsl)))?;
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module)
        .map_err(|e| ShaderCacheError::CreateShaderModule(format!("{e:?}")))?;
    match naga::back::spv::write_vec(&module, &info, &naga::back::spv::Options::default(), None) {
        Ok(_) => report.push_str("spv ok, "),
        Err(e) => report.push_str(&format!("spv ERR {e}, ")),
    }
    match naga::back::msl::write_string(
        &module,
        &info,
        &naga::back::msl::Options::default(),
        &naga::back::msl::PipelineOptions::default(),
    ) {
        Ok(_) => report.push_str("msl ok, "),
        Err(e) => report.push_str(&format!("msl ERR {e}, ")),
    }
    let mut hlsl = String::new();
    let hlsl_options = naga::back::hlsl::Options::default();
    let hlsl_pipeline = naga::back::hlsl::PipelineOptions::default();
    match naga::back::hlsl::Writer::new(&mut hlsl, &hlsl_options, &hlsl_pipeline).write(&module, &info, None) {
        Ok(_) => report.push_str("hlsl ok"),
        Err(e) => report.push_str(&format!("hlsl ERR {e}")),
    }
    if let Ok(mut r) = RESULTS.lock() {
        r.push(report);
    }
    Ok(())
}
