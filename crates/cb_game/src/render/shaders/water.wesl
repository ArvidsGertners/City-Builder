// Water surface (ART_DIRECTION §2, §5): opaque, colour by depth (shallow #7CC4C0 → deep #3D7FA3),
// a foam band at the shore (#F4F1E6), sky fresnel and soft animated normals. Vertex colour
// carries r = depth / 6 m, g = foam weight.
import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{view, globals},
};

struct FocusFog {
    focus: vec4<f32>,
    color: vec4<f32>,
};

struct WaterParams {
    shallow: vec4<f32>,
    deep: vec4<f32>,
    foam: vec4<f32>,
    // rgb sky colour for fresnel (pre-exposure luminance), w = fresnel strength
    sky: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> fog: FocusFog;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<uniform> water: WaterParams;

fn apply_focus_fog(color: vec3<f32>, world_position: vec3<f32>) -> vec3<f32> {
    let d = max(0.0, length(world_position - fog.focus.xyz) - fog.focus.w);
    let f = 1.0 - exp(-fog.color.w * d);
    return mix(color, fog.color.rgb * view.exposure, f);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let p = in.world_position.xyz;
    let t = globals.time;
    // Soft normals: two slow wave trains.
    let w1 = sin(p.x * 0.21 + p.z * 0.13 + t * 0.9);
    let w2 = sin(p.x * -0.11 + p.z * 0.27 + t * 1.3);
    let n = normalize(vec3(0.035 * w1, 1.0, 0.035 * w2));
    pbr_input.N = n;
    pbr_input.world_normal = n;
    var depth_t = 0.0;
    var foam = 0.0;
@if(VERTEX_COLORS) {
    depth_t = clamp(in.color.r, 0.0, 1.0);
    foam = clamp(in.color.g, 0.0, 1.0);
}
    let shade = mix(water.shallow.rgb, water.deep.rgb, smoothstep(0.0, 1.0, depth_t));
    let foam_wave = 0.75 + 0.25 * sin(t * 1.7 + p.x * 0.6 + p.z * 0.4);
    let base = mix(shade, water.foam.rgb, foam * foam_wave);
    pbr_input.material.base_color = vec4(base, 1.0);
    pbr_input.material.perceptual_roughness = 0.25;
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    // Sky fresnel (Schlick, F0 = 0.02).
    let v = normalize(view.world_position - p);
    let fres = 0.02 + 0.98 * pow(1.0 - max(dot(n, v), 0.0), 5.0);
    out.color = vec4(mix(out.color.rgb, water.sky.rgb * view.exposure, fres * water.sky.w * (1.0 - foam)), 1.0);
    out.color = vec4(apply_focus_fog(out.color.rgb, p), 1.0);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
