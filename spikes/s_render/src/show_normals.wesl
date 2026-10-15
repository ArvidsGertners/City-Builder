// Debug view: shows the prepass normal buffer (what SSAO sees) as color.
import bevy_pbr::{
    render::forward_io::VertexOutput,
    prepass::utils,
};

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let n = utils::prepass_normal(mesh.position, 0u);
    return vec4(n * 0.5 + vec3(0.5), 1.0);
}
