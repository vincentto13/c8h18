// Model viewer: the game's model triangles (c8-render vertices), coloured like the game:
// palette index from the atlas (textured types) or the triangle (flat types), through the colour
// table's light row (LUT[index, row]). Optional directional shading to read the shape.
struct U {
    vp: mat4x4<f32>,
    // xyz: direction towards the light, w: 1 = shading on
    light: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var atlas: texture_2d<u32>;
@group(0) @binding(2) var lut: texture_2d<f32>;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) light: f32,
    @location(3) kind: u32,
    @location(4) param: u32,
};
struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) light: f32,
    @location(2) @interpolate(flat) kind: u32,
    @location(3) @interpolate(flat) param: u32,
    @location(4) wpos: vec3<f32>,
};

@vertex
fn vs(v: VIn) -> VOut {
    var o: VOut;
    o.clip = u.vp * vec4<f32>(v.pos, 1.0);
    o.uv = v.uv;
    o.light = v.light;
    o.kind = v.kind;
    o.param = v.param;
    o.wpos = v.pos;
    return o;
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    let n = normalize(cross(dpdx(i.wpos), dpdy(i.wpos)));
    var idx = i.param & 255u;
    if ((i.kind & 255u) == 1u) {
        let mx = vec2<i32>(textureDimensions(atlas)) - 1;
        idx = textureLoad(atlas, clamp(vec2<i32>(floor(i.uv)), vec2<i32>(0), mx), 0).r;
    }
    let row = i32(clamp(floor(i.light), 0.0, 63.0));
    var c = textureLoad(lut, vec2<i32>(i32(idx), row), 0);
    if (u.light.w > 0.5) {
        c = vec4<f32>(c.rgb * (0.45 + 0.55 * abs(dot(n, u.light.xyz))), 1.0);
    }
    return c;
}

struct LIn {
    @location(0) pos: vec3<f32>,
    @location(1) colour: vec4<f32>,
};
struct LOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) colour: vec4<f32>,
};

@vertex
fn vs_line(v: LIn) -> LOut {
    var o: LOut;
    o.clip = u.vp * vec4<f32>(v.pos, 1.0);
    // pull lines slightly towards the camera so that they win against their own faces
    o.clip.z = o.clip.z - 0.002 * o.clip.w;
    o.colour = v.colour;
    return o;
}

@fragment
fn fs_line(i: LOut) -> @location(0) vec4<f32> {
    return i.colour;
}
