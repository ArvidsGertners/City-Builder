// Terrain material fragment (ADR-0049: smooth normals, vertex colours baked from material
// weights) with the shared focus fog.
import bevy_pbr::render::{
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

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> fog: FocusFog;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
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
