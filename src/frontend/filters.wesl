import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;
import bevy_render::color_operations::linear_to_srgb;

// The map drawn at one texel per map pixel, brought to the screen: «Pixel
// della mappa» of the «GRAFICA» page, an option of this version. Even pixels
// (sharp bilinear: nearest pixels with one screen pixel of blending at their
// edges) or smoothed art (xBR level 2, then the same edges).
struct Params {
    // xy: texel coordinate of the screen's top left corner; z: texels per
    // screen pixel; w: the filter, 1 even, 2 smoothed.
    a: vec4<f32>,
    // xy: texels rendered.
    b: vec4<f32>,
}
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var map: texture_2d<f32>;

// The image is sRGB: its texels read linear, and the screen keeps the
// encoded values of the map's compositing (CompositingSpace::Srgb).
fn texel(p: vec2<i32>) -> vec3<f32> {
    let last = vec2<i32>(params.b.xy) - vec2(1);
    return linear_to_srgb(textureLoad(map, clamp(p, vec2(0), last), 0).rgb);
}

// Nearest pixels whose edges blend over one screen pixel; plain bilinear when
// a screen pixel covers more than a texel.
fn sharp(t: vec2<f32>, zoom: f32) -> vec3<f32> {
    let c = t - 0.5;
    let i = floor(c);
    let f = c - i;
    var w = f;
    if zoom >= 1.0 {
        w = clamp((f - 0.5) * zoom + 0.5, vec2(0.0), vec2(1.0));
    }
    let p = vec2<i32>(i);
    let top = mix(texel(p), texel(p + vec2(1, 0)), w.x);
    let bottom = mix(texel(p + vec2(0, 1)), texel(p + vec2(1, 1)), w.x);
    return mix(top, bottom, w.y);
}

const LUMA = vec3(0.2126, 0.7152, 0.0722);
fn luma(c: vec3<f32>) -> f32 {
    return dot(c, LUMA) * 48.0;
}
fn df(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    return abs(a - b);
}
fn c_df(a: vec3<f32>, b: vec3<f32>) -> f32 {
    let d = abs(a - b);
    return d.r + d.g + d.b;
}
fn same(a: vec4<f32>, b: vec4<f32>) -> vec4<bool> {
    return df(a, b) < vec4(15.0);
}
fn distance4(
    a: vec4<f32>,
    b: vec4<f32>,
    c: vec4<f32>,
    d: vec4<f32>,
    e: vec4<f32>,
    f: vec4<f32>,
    g: vec4<f32>,
    h: vec4<f32>,
) -> vec4<f32> {
    return df(a, b) + df(a, c) + df(d, e) + df(d, f) + 4.0 * df(g, h);
}

// xBR level 2 (Hyllian): each texel's corners are cut along the edges its
// neighbours draw, at 30, 45 and 60 degrees.
//    A1 B1 C1
// A0  A  B  C C4
// D0  D  E  F F4
// G0  G  H  I I4
//    G5 H5 I5
fn xbr(t: vec2<f32>, zoom: f32) -> vec3<f32> {
    let i = vec2<i32>(floor(t));
    let fp = t - floor(t);
    let A1 = texel(i + vec2(-1, -2));
    let B1 = texel(i + vec2(0, -2));
    let C1 = texel(i + vec2(1, -2));
    let A0 = texel(i + vec2(-2, -1));
    let A = texel(i + vec2(-1, -1));
    let B = texel(i + vec2(0, -1));
    let C = texel(i + vec2(1, -1));
    let C4 = texel(i + vec2(2, -1));
    let D0 = texel(i + vec2(-2, 0));
    let D = texel(i + vec2(-1, 0));
    let E = texel(i);
    let F = texel(i + vec2(1, 0));
    let F4 = texel(i + vec2(2, 0));
    let G0 = texel(i + vec2(-2, 1));
    let G = texel(i + vec2(-1, 1));
    let H = texel(i + vec2(0, 1));
    let I = texel(i + vec2(1, 1));
    let I4 = texel(i + vec2(2, 1));
    let G5 = texel(i + vec2(-1, 2));
    let H5 = texel(i + vec2(0, 2));
    let I5 = texel(i + vec2(1, 2));

    let b = vec4(luma(B), luma(D), luma(H), luma(F));
    let c = vec4(luma(C), luma(A), luma(G), luma(I));
    let e = vec4(luma(E));
    let d = b.yzwx;
    let f = b.wxyz;
    let g = c.zwxy;
    let h = b.zwxy;
    let ii = c.wxyz;
    let i4 = vec4(luma(I4), luma(C1), luma(A0), luma(G5));
    let i5 = vec4(luma(I5), luma(C4), luma(A1), luma(G0));
    let h5 = vec4(luma(H5), luma(F4), luma(B1), luma(D0));
    let f4 = h5.yzwx;

    let Ao = vec4(1.0, -1.0, -1.0, 1.0);
    let Bo = vec4(1.0, 1.0, -1.0, -1.0);
    let Co = vec4(1.5, 0.5, -0.5, 0.5);
    let Ax = vec4(1.0, -1.0, -1.0, 1.0);
    let Bx = vec4(0.5, 2.0, -0.5, -2.0);
    let Cx = vec4(1.0, 1.0, -0.5, 0.0);
    let Ay = vec4(1.0, -1.0, -1.0, 1.0);
    let By = vec4(2.0, 0.5, -2.0, -0.5);
    let Cy = vec4(2.0, 0.0, -1.0, 0.5);
    let Ci = vec4(0.25);

    // The blending across a cut is one screen pixel wide.
    let s = 1.0 / zoom;
    let delta = vec4(s);
    let deltaL = vec4(0.5 * s, s, 0.5 * s, s);
    let deltaU = deltaL.yxwz;

    let fx = Ao * fp.y + Bo * fp.x;
    let fx_left = Ax * fp.y + Bx * fp.x;
    let fx_up = Ay * fp.y + By * fp.x;

    let lv0 = (e != f) & (e != h);
    let lv1 = lv0 & ((!same(f, b) & !same(h, d))
        | (same(e, ii) & !same(f, i4) & !same(h, i5))
        | same(e, g)
        | same(e, c));
    let lv2_left = (e != g) & (d != g);
    let lv2_up = (e != c) & (b != c);

    var fx45i = clamp((fx + delta - Co - Ci) / (2.0 * delta), vec4(0.0), vec4(1.0));
    var fx45 = clamp((fx + delta - Co) / (2.0 * delta), vec4(0.0), vec4(1.0));
    var fx30 = clamp((fx_left + deltaL - Cx) / (2.0 * deltaL), vec4(0.0), vec4(1.0));
    var fx60 = clamp((fx_up + deltaU - Cy) / (2.0 * deltaU), vec4(0.0), vec4(1.0));

    let wd1 = distance4(e, c, g, ii, h5, f4, h, f);
    let wd2 = distance4(h, d, i5, f, i4, b, e, ii);

    let edri = (wd1 <= wd2) & lv0;
    let edr = (wd1 < wd2) & lv1;
    let edr_left = (2.0 * df(f, g) <= df(h, c)) & lv2_left & edr;
    let edr_up = (df(f, g) >= 2.0 * df(h, c)) & lv2_up & edr;

    let zero = vec4(0.0);
    fx45 = select(zero, fx45, edr);
    fx30 = select(zero, fx30, edr_left);
    fx60 = select(zero, fx60, edr_up);
    fx45i = select(zero, fx45i, edri);

    let px = df(e, f) <= df(e, h);
    let maximos = max(max(fx30, fx60), max(fx45, fx45i));

    var res1 = E;
    res1 = mix(res1, select(H, F, px.x), maximos.x);
    res1 = mix(res1, select(B, D, px.z), maximos.z);
    var res2 = E;
    res2 = mix(res2, select(F, B, px.y), maximos.y);
    res2 = mix(res2, select(D, H, px.w), maximos.w);
    return mix(res1, res2, step(c_df(E, res1), c_df(E, res2)));
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let t = params.a.xy + in.position.xy * params.a.z;
    let zoom = 1.0 / params.a.z;
    if params.a.w > 1.5 && zoom >= 1.0 {
        return vec4(xbr(t, zoom), 1.0);
    }
    return vec4(sharp(t, zoom), 1.0);
}
