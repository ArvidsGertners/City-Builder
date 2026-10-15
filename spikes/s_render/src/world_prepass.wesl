// S-Render world material, prepass and shadow pass: same vertex layout, derivative normals into
// the normal prepass (feeds SSAO).
@if(PREPASS_FRAGMENT)
import bevy_pbr::prepass::io::FragmentOutput;

import bevy_pbr::{
    prepass::io::VertexOutput,
    render::{
        mesh_functions,
        mesh_view_bindings::view,
        view_transformations::position_world_to_clip,
    },
};

struct WorldVertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(5) color: vec4<f32>,
    @location(8) packed: u32,
};

@vertex
fn vertex(v: WorldVertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4(v.position, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION) {
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
}
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS)
    out.world_normal = vec3(0.0, 1.0, 0.0);
@if(MOTION_VECTOR_PREPASS)
    out.previous_world_position = out.world_position;
@if(VERTEX_COLORS)
    out.color = v.color;
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = v.instance_index;
    return out;
}

@if(PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    var out: FragmentOutput;
@if(NORMAL_PREPASS) {
    let p = in.world_position.xyz - view.world_position;
    var n = normalize(cross(dpdx(p), dpdy(p)));
    if dot(n, -p) < 0.0 {
        n = -n;
    }
    out.normal = vec4(n * 0.5 + vec3(0.5), 1.0);
}
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION)
    out.frag_depth = in.unclipped_depth;
@if(MOTION_VECTOR_PREPASS)
    out.motion_vector = vec2(0.0);
    return out;
}
