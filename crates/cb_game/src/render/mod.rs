//! World rendering (ADR-0049, ADR-0050): the world material for procedural meshes (20-byte
//! vertices, derivative flat normals), the terrain material (smooth normals, baked material
//! colours) and the water material, all with the same focus-distance fog; mesh conversion from
//! `cb_geometry::MeshData`; lighting keyframes and the sky dome.

pub mod lighting;

use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::mesh::{
    Indices, MeshVertexAttribute, MeshVertexBufferLayoutRef, PrimitiveTopology,
    VertexAttributeValues,
};
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError, VertexFormat,
};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::ShaderRef;
use cb_geometry::MeshData;

/// Vertex colour: unorm8x4, rgb = linear albedo, a = baked AO.
pub const ATTRIBUTE_WORLD_COLOR: MeshVertexAttribute =
    MeshVertexAttribute::new("WorldColor", 0x5EED_0001, VertexFormat::Unorm8x4);
/// Packed u32: slot (bits 0–15) | surface kind (16–19) | id (20–31).
pub const ATTRIBUTE_PACKED: MeshVertexAttribute =
    MeshVertexAttribute::new("WorldPacked", 0x5EED_0002, VertexFormat::Uint32);

/// Focus-distance fog parameters (shared by all world materials).
#[derive(Clone, Copy, Debug, ShaderType, Reflect, Default, PartialEq)]
pub struct FocusFog {
    /// xyz focus point (m), w = clear radius (m).
    pub focus: Vec4,
    /// rgb fog luminance (pre-exposure), w = density (1/m).
    pub color: Vec4,
}

/// World material extension.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct WorldExt {
    /// Per-object parameters (rgb tint).
    #[storage(100, read_only, visibility(vertex, fragment))]
    pub slots: Handle<ShaderBuffer>,
    /// Fog.
    #[uniform(101)]
    pub fog: FocusFog,
}

impl MaterialExtension for WorldExt {
    fn vertex_shader() -> ShaderRef {
        "embedded://cb_game/render/shaders/world.wesl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "embedded://cb_game/render/shaders/world.wesl".into()
    }
    fn prepass_vertex_shader() -> ShaderRef {
        "embedded://cb_game/render/shaders/world_prepass.wesl".into()
    }
    fn prepass_fragment_shader() -> ShaderRef {
        "embedded://cb_game/render/shaders/world_prepass.wesl".into()
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
        Ok(())
    }
}

/// Terrain material extension (fragment only; standard vertex layout with normals and colours).
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct TerrainExt {
    /// Fog.
    #[uniform(100)]
    pub fog: FocusFog,
}

impl MaterialExtension for TerrainExt {
    fn fragment_shader() -> ShaderRef {
        "embedded://cb_game/render/shaders/terrain.wesl".into()
    }
}

/// Water colours.
#[derive(Clone, Copy, Debug, ShaderType, Reflect, Default, PartialEq)]
pub struct WaterParams {
    /// Shallow colour (linear).
    pub shallow: Vec4,
    /// Deep colour (linear).
    pub deep: Vec4,
    /// Foam colour (linear).
    pub foam: Vec4,
    /// Sky colour for fresnel (pre-exposure luminance), w = strength.
    pub sky: Vec4,
}

/// Water material extension.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct WaterExt {
    /// Fog.
    #[uniform(100)]
    pub fog: FocusFog,
    /// Colours.
    #[uniform(101)]
    pub water: WaterParams,
}

impl MaterialExtension for WaterExt {
    fn fragment_shader() -> ShaderRef {
        "embedded://cb_game/render/shaders/water.wesl".into()
    }
}

/// World material.
pub type WorldMaterial = ExtendedMaterial<StandardMaterial, WorldExt>;
/// Terrain material.
pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExt>;
/// Water material.
pub type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterExt>;

/// Shared material handles.
#[derive(Resource, Clone, Debug)]
pub struct WorldMaterials {
    /// Procedural objects.
    pub world: Handle<WorldMaterial>,
    /// Terrain.
    pub terrain: Handle<TerrainMaterial>,
    /// Water.
    pub water: Handle<WaterMaterial>,
}

/// Current fog parameters (written by lighting and the camera, copied into the materials).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct FogState(pub FocusFog);

/// sRGB byte → linear byte table (the shader reads Unorm8 as linear).
#[must_use]
pub fn srgb_to_linear_lut() -> [u8; 256] {
    let mut t = [0u8; 256];
    for (i, v) in t.iter_mut().enumerate() {
        let s = i as f32 / 255.0;
        let l = if s <= 0.040_45 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        };
        *v = (l * 255.0).round() as u8;
    }
    t
}

/// Converts procedural mesh data (sRGB colours) into a GPU mesh in the world vertex layout.
#[must_use]
pub fn world_mesh(m: &MeshData) -> Mesh {
    let lut = srgb_to_linear_lut();
    let colors: Vec<[u8; 4]> = m
        .colors
        .iter()
        .map(|c| {
            [
                lut[c[0] as usize],
                lut[c[1] as usize],
                lut[c[2] as usize],
                c[3],
            ]
        })
        .collect();
    let indices = if m.positions.len() <= usize::from(u16::MAX) {
        Indices::U16(m.indices.iter().map(|&i| i as u16).collect())
    } else {
        Indices::U32(m.indices.clone())
    };
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, m.positions.clone())
    .with_inserted_attribute(
        ATTRIBUTE_WORLD_COLOR,
        VertexAttributeValues::Unorm8x4(colors),
    )
    .with_inserted_attribute(
        ATTRIBUTE_PACKED,
        VertexAttributeValues::Uint32(m.packed.clone()),
    )
    .with_inserted_indices(indices)
}

/// Render plugin.
pub struct WorldRenderPlugin;

impl Plugin for WorldRenderPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/world.wesl");
        embedded_asset!(app, "shaders/world_prepass.wesl");
        embedded_asset!(app, "shaders/terrain.wesl");
        embedded_asset!(app, "shaders/water.wesl");
        app.add_plugins((
            MaterialPlugin::<WorldMaterial>::default(),
            MaterialPlugin::<TerrainMaterial>::default(),
            MaterialPlugin::<WaterMaterial>::default(),
        ))
        .init_resource::<FogState>()
        .add_systems(Startup, create_materials)
        .add_systems(PostUpdate, sync_fog)
        .add_plugins(lighting::LightingPlugin);
    }
}

fn linear(c: Color) -> Vec4 {
    let l = c.to_linear();
    Vec4::new(l.red, l.green, l.blue, 1.0)
}

fn create_materials(
    mut commands: Commands,
    mut world: ResMut<Assets<WorldMaterial>>,
    mut terrain: ResMut<Assets<TerrainMaterial>>,
    mut water: ResMut<Assets<WaterMaterial>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    // Slot 0 = neutral; per-object slots (selection, condition) arrive with the slot allocator.
    let slots = buffers.add(ShaderBuffer::from(vec![[1.0f32, 1.0, 1.0, 1.0]; 1]));
    let fog = FocusFog::default();
    let base = |rough: f32| StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: rough,
        ..default()
    };
    let w = world.add(WorldMaterial {
        base: base(0.9),
        extension: WorldExt { slots, fog },
    });
    let t = terrain.add(TerrainMaterial {
        base: base(0.95),
        extension: TerrainExt { fog },
    });
    let wa = water.add(WaterMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.25,
            reflectance: 0.3,
            ..default()
        },
        extension: WaterExt {
            fog,
            water: WaterParams {
                shallow: linear(Color::srgb_u8(0x7C, 0xC4, 0xC0)),
                deep: linear(Color::srgb_u8(0x3D, 0x7F, 0xA3)),
                foam: linear(Color::srgb_u8(0xF4, 0xF1, 0xE6)),
                sky: Vec4::new(6000.0, 7600.0, 9000.0, 0.6),
            },
        },
    });
    commands.insert_resource(WorldMaterials {
        world: w,
        terrain: t,
        water: wa,
    });
}

/// Copies the fog state into every material when it changes.
fn sync_fog(
    fog: Res<FogState>,
    handles: Option<Res<WorldMaterials>>,
    mut world: ResMut<Assets<WorldMaterial>>,
    mut terrain: ResMut<Assets<TerrainMaterial>>,
    mut water: ResMut<Assets<WaterMaterial>>,
) {
    let Some(h) = handles else { return };
    if !fog.is_changed() && !h.is_added() {
        return;
    }
    if let Some(mut m) = world.get_mut(&h.world) {
        m.extension.fog = fog.0;
    }
    if let Some(mut m) = terrain.get_mut(&h.terrain) {
        m.extension.fog = fog.0;
    }
    if let Some(mut m) = water.get_mut(&h.water) {
        m.extension.fog = fog.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lut_endpoints_and_midtone() {
        let l = srgb_to_linear_lut();
        assert_eq!((l[0], l[255]), (0, 255));
        assert_eq!(l[188], 128, "sRGB 188 ≈ linear 0.5");
    }
}
