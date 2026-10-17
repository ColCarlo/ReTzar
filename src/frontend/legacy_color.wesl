import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;
@group(0) @binding(0) var source: texture_2d<f32>;
struct Tint {
    // rgb: the clock's tint; a: 1 keeps the original's RGB555 steps, 0 is the
    // true colour of the «GRAFICA» page.
    lighting: vec4<f32>,
    // The light map of the «GRAFICA» page: x how much the lights give back
    // of what the night takes, y the halo; zw turn a screen position into
    // the map's coordinates.
    glow: vec4<f32>,
    // A screen position to map pixels: x scale, yz offsets; w the time in
    // seconds, for the water.
    world: vec4<f32>,
    // x the map's side in cells; y «Nebbia morbida», z «Ambiente vivo» (the
    // glinting water), 1 when on.
    map: vec4<f32>,
}
@group(0) @binding(1) var<uniform> tint: Tint;
@group(0) @binding(2) var lights: texture_2d<f32>;
@group(0) @binding(3) var lights_sampler: sampler;
// r: vertex explored, g: vertex in view.
@group(0) @binding(4) var fog: texture_2d<f32>;
// r: a cell of water.
@group(0) @binding(5) var water: texture_2d<f32>;

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}
fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let top = mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x);
    let bottom = mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x);
    return mix(top, bottom, u.y);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    var pixel = textureLoad(source, vec2<i32>(in.position.xy), 0);
    // The map pixel under this screen pixel, y downwards.
    let at = floor(in.position.xy * tint.world.x + tint.world.yz) + 0.5;
    if tint.map.z > 0.0 {
        // Glinting water, an option of this version: sparkles drift over
        // the cells of water, only where the picture is water-blue.
        let m = textureSampleLevel(water, lights_sampler, at / 32.0 / tint.map.x, 0.0).r;
        let blue = smoothstep(0.0, 0.06, pixel.b - max(pixel.r, pixel.g * 0.85));
        let t = tint.world.w;
        let a = noise(at * 0.31 + vec2(t * 1.3, t * 0.7));
        let b = noise(at * 0.47 - vec2(t * 0.9, t * 1.6));
        let glint = smoothstep(0.78, 0.95, a) * smoothstep(0.78, 0.95, b) * m * blue * 0.85;
        pixel = vec4(pixel.rgb + (vec3(1.0) - pixel.rgb) * glint, pixel.a);
    }
    if tint.map.y > 0.0 {
        // Soft fog, an option of this version: black where nothing is
        // explored, half where the original lays its grey tile, blurred
        // between the cells' corners.
        let uv = (at / 32.0 + 0.5) / (tint.map.x + 1.0);
        let f = textureSampleLevel(fog, lights_sampler, uv, 0.0).rg;
        let seen = smoothstep(0.1, 0.9, f.r);
        let view = smoothstep(0.1, 0.9, f.g);
        pixel = vec4(pixel.rgb * seen * (0.5 + 0.5 * view), pixel.a);
    }
    var lighting = tint.lighting.rgb;
    var halo = vec3(0.0);
    if tint.glow.x > 0.0 || tint.glow.y > 0.0 {
        // Lights and the halo, additions of this version.
        let light = textureSampleLevel(lights, lights_sampler, in.position.xy * tint.glow.zw, 0.0).rgb;
        lighting = lighting + (vec3(1.0) - lighting) * min(light * tint.glow.x, vec3(1.0));
        // The halo grows with the square of the light: fires shine, lamps
        // stay soft.
        halo = light * light * tint.glow.y;
    }
    // True colour, an option of this version: the clock's tint without the
    // five-bit steps.
    if tint.lighting.a < 0.5 {
        let lit = clamp(pixel.rgb * lighting, vec3(0.0), vec3(1.0));
        return vec4(lit + (vec3(1.0) - lit) * min(halo, vec3(1.0)), pixel.a);
    }
    // The source is Rgba8Unorm in CompositingSpace::Srgb: no transfer function.
    // Restore the original RGB555 bit truncation and the decoder's expansion.
    // Restore the map's five-bit compositing before the clock applies its tint.
    // In particular, two overlapping shadows must not retain eight-bit residuals.
    let composite = vec3<u32>(round(clamp(pixel.rgb, vec3(0.0), vec3(1.0)) * 255.0)) >> vec3<u32>(3u);
    let rgb555 = vec3<f32>((composite << vec3<u32>(3u)) | (composite >> vec3<u32>(2u))) / 255.0;
    let lit = clamp(rgb555 * lighting, vec3(0.0), vec3(1.0));
    let glowing = lit + (vec3(1.0) - lit) * min(halo, vec3(1.0));
    let bytes = vec3<u32>(round(glowing * 255.0));
    let five = bytes >> vec3<u32>(3u);
    let expanded = (five << vec3<u32>(3u)) | (five >> vec3<u32>(2u));
    return vec4(vec3<f32>(expanded) / 255.0, pixel.a);
}
