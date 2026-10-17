import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;

// Placement grid, an addition of this version: the edges of the 32-pixel
// cells, a line of `look.w` screen pixels, a little darker near the cursor,
// and the cells the building cannot cover tinted red. `look.xy` is the map
// point under the cursor, `look.z` 1 when there is one. `tone` holds the
// line's alpha, its extra near the cursor and the alpha of a refused cell.
// The map composites in sRGB (SRGB_OUTPUT), so colours are screen values.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> look: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var<uniform> tone: vec4<f32>;
// One texel per cell of the map: red where the building cannot stand.
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var marks: texture_2d<f32>;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let p = mesh.world_position.xy;
    let cell = p / 32.0;
    // Screen pixels from the nearest edge. The window is half open and as
    // wide as the line, so an edge between two pixels still takes one.
    let s = (cell - round(cell)) / fwidth(cell);
    let low = -0.5 * look.w - 0.25;
    let high = 0.5 * look.w - 0.25;
    let on = (s.x >= low && s.x < high) || (s.y >= low && s.y < high);
    let size = vec2<i32>(textureDimensions(marks));
    let texel = clamp(vec2<i32>(floor(vec2(cell.x, -cell.y))), vec2(0), size - 1);
    let refused = textureLoad(marks, texel, 0).r > 0.5;
    let near = look.z * (1.0 - smoothstep(96.0, 320.0, distance(p, look.xy)));
    if on {
        let alpha = tone.x + tone.y * near;
        if refused {
            return vec4(0.3, 0.0, 0.0, min(alpha + 0.1, 1.0));
        }
        return vec4(0.0, 0.0, 0.0, alpha);
    }
    if refused {
        // Diagonal stripes over a faint fill, so that a refused cell reads
        // apart from the browns of the terrain itself.
        let u = p.x + p.y;
        let gap = abs(fract(u / 8.0) - 0.5) * 8.0 / fwidth(u);
        let fill = tone.z * (0.75 + 0.25 * near);
        let alpha = select(0.6 * fill, min(1.8 * fill, 1.0), gap < 0.5 * look.w + 0.25);
        return vec4(0.95, 0.12, 0.08, alpha);
    }
    return vec4(0.0);
}
