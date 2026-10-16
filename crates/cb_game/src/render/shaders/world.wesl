// World material, main pass (ADR-0049): 20-byte vertices (position, unorm8x4 linear colour with
// baked AO in alpha, packed u32 slot | surface kind | id) and flat normals from derivatives.
import bevy_pbr::render::{
    mesh_functions,
    view_transformations::position_world_to_clip,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
};
import bevy_pbr::render::mesh_view_bindings::view;

struct FocusFog {
    // xyz focus point (m), w = clear radius r0 (m)
    focus: vec4<f32>,
    // rgb fog luminance (pre-exposure units ≈ albedo × sun lux), w = density (1/m)
    color: vec4<f32>,
};

/// Face normal from screen-space derivatives of the camera-relative position, oriented to the
/// camera.
fn flat_normal(world_position: vec3<f32>) -> vec3<f32> {
    let p = world_position - view.world_position;
    var n = normalize(cross(dpdx(p), dpdy(p)));
    if dot(n, -p) < 0.0 {
        n = -n;
    }
    return n;
}

/// 1 - exp(-density * max(0, |p - focus| - r0)), blended toward the fog colour.
fn apply_focus_fog(color: vec3<f32>, world_position: vec3<f32>, fog: FocusFog) -> vec3<f32> {
    let d = max(0.0, length(world_position - fog.focus.xyz) - fog.focus.w);
    let f = 1.0 - exp(-fog.color.w * d);
    return mix(color, fog.color.rgb * view.exposure, f);
}

struct WorldVertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(5) color: vec4<f32>,
    @location(8) packed: u32,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<storage, read> slots: array<vec4<f32>>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<uniform> fog: FocusFog;

@vertex
fn vertex(v: WorldVertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4(v.position, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
    out.world_normal = vec3(0.0, 1.0, 0.0);
    let n_slots = arrayLength(&slots);
    let slot = min(v.packed & 0xffffu, n_slots - 1u);
    let tint = slots[slot];
    out.color = vec4(v.color.rgb * v.color.a * tint.rgb, 1.0);
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = v.instance_index;
    return out;
}

@fragment
fn fragment(vertex_output: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var in = vertex_output;
    let n = flat_normal(in.world_position.xyz);
    in.world_normal = n;
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.N = n;
    pbr_input.world_normal = n;
    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color
    );
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = vec4(apply_focus_fog(out.color.rgb, in.world_position.xyz, fog), out.color.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
