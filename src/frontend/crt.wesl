import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

// «Effetto monitor CRT» of the «GRAFICA» page, an option of this version: the
// whole screen as on an old monitor: rounded dark corners, scan lines at the
// interface's pixel rows and an aperture grille. Nothing moves, so the
// pointer still meets what it shows.
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
// x: screen pixels per interface pixel; yz: the screen in pixels.
@group(0) @binding(2) var<uniform> crt: vec4<f32>;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let size = crt.yz;
    let uv = in.uv;
    let c = uv * 2.0 - 1.0;
    var color = textureSampleLevel(source, source_sampler, uv, 0.0).rgb;
    // The glass's rounded corners, a twentieth of the height.
    let radius = size.y * 0.05;
    let edge = abs(in.position.xy - size * 0.5) - (size * 0.5 - radius);
    let outside = length(max(edge, vec2(0.0))) - radius;
    let glass = 1.0 - smoothstep(-1.5, 0.5, outside);
    // A dark gap between the rows of interface pixels.
    let row = uv.y * size.y / max(crt.x, 2.0);
    let scan = 0.78 + 0.22 * cos(fract(row) * 6.2831853);
    // Red, green and blue columns of the grille.
    let column = u32(floor(in.position.x)) % 3u;
    var mask = vec3(0.92);
    if column == 0u {
        mask.r = 1.08;
    } else if column == 1u {
        mask.g = 1.08;
    } else {
        mask.b = 1.08;
    }
    let vignette = 1.0 - 0.18 * dot(c, c);
    color = color * scan * mask * vignette * glass * 1.12;
    return vec4(clamp(color, vec3(0.0), vec3(1.0)), 1.0);
}
