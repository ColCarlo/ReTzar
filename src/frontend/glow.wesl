import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;
@if(SRGB_OUTPUT)
import bevy_render::color_operations::linear_to_srgb;

// Mode 3 of the original painter (0x458740): the picture's channels, already
// scaled by the strength, brighten what lies under them. The pipeline blends
// with `src × (1 − dst) + dst`; transparent texels are black and add nothing.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var texture_sampler: sampler;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    var color = textureSample(texture, texture_sampler, mesh.uv);
@if(SRGB_OUTPUT)
    color = vec4(linear_to_srgb(color.rgb), color.a);
    return vec4(color.rgb, 1.0);
}
