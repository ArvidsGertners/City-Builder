// S-Render world material, main pass (ADR-0049): 20-byte vertices (position, unorm8x4 color with
// baked AO in alpha, packed u32 slot|flags) and flat normals from screen-space derivatives.
import bevy_pbr::render::{
    mesh_functions,
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
};

struct WorldVertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(5) color: vec4<f32>,
    @location(8) packed: u32,
};

struct FocusFog {
    // xyz focus point (m), w = clear radius r0 (m)
    focus: vec4<f32>,
    // rgb fog luminance in the same pre-exposure units as lit surfaces (≈ albedo × sun lux),
    // w = density (1/m)
    color: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<storage, read> slots: array<vec4<f32>>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<uniform> fog: FocusFog;

@vertex
fn vertex(v: WorldVertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4(v.position, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
    // Replaced per fragment by the derivative normal.
    out.world_normal = vec3(0.0, 1.0, 0.0);
    let tint = slots[v.packed & 0xffffu];
    out.color = vec4(v.color.rgb * v.color.a * tint.rgb, 1.0);
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = v.instance_index;
    return out;
}

/// Face normal from screen-space derivatives of the camera-relative position, oriented to the
/// camera (independent of the API's screen-space handedness).
fn flat_normal(world_position: vec3<f32>) -> vec3<f32> {
    let p = world_position - view.world_position;
    var n = normalize(cross(dpdx(p), dpdy(p)));
    if dot(n, -p) < 0.0 {
        n = -n;
    }
    return n;
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

    // Focus-distance fog (ADR-0050): 1 - exp(-density * max(0, |p - focus| - r0)).
    let d = max(0.0, length(in.world_position.xyz - fog.focus.xyz) - fog.focus.w);
    let f = 1.0 - exp(-fog.color.w * d);
    out.color = vec4(mix(out.color.rgb, fog.color.rgb * view.exposure, f), out.color.a);

    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
