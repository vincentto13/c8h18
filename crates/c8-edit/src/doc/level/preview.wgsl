// 3D preview: the game's level mesh, coloured exactly like the game: palette index from the
// terrain texture, lit through the colour table's light rows (LUT[index, row]).
struct U {
    vp: mat4x4<f32>,
    eye: vec4<f32>,
    // x: haze start, y: haze end (cells), z: 1 = haze on
    haze: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var tex: texture_2d_array<u32>;
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
    @location(2) @interpolate(flat) param: u32,
    @location(3) dist: f32,
};

@vertex
fn vs(v: VIn) -> VOut {
    var o: VOut;
    o.clip = u.vp * vec4<f32>(v.pos, 1.0);
    o.uv = v.uv;
    o.light = v.light;
    o.param = v.param;
    o.dist = distance(v.pos, u.eye.xyz);
    return o;
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    let t = vec2<i32>(clamp(floor(i.uv), vec2<f32>(0.0), vec2<f32>(63.0)));
    let idx = textureLoad(tex, t, i32(i.param & 255u), 0).r;
    var row = clamp(floor(i.light), 0.0, 63.0);
    if (u.haze.z > 0.5) {
        // fade the light row towards row 0, the haze colour
        let f = clamp((i.dist - u.haze.x) / (u.haze.y - u.haze.x), 0.0, 1.0);
        row = floor(mix(row, 0.0, f));
    }
    return textureLoad(lut, vec2<i32>(i32(idx), i32(row)), 0);
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
    // pull lines slightly towards the camera so that they win against the ground
    o.clip.z = o.clip.z - 0.0005 * o.clip.w;
    o.colour = v.colour;
    return o;
}

@fragment
fn fs_line(i: LOut) -> @location(0) vec4<f32> {
    return i.colour;
}
