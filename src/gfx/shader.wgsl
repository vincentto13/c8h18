// World, sky and composite shaders. Colour comes from palette indices through the light table.

struct Globals {
    eye: vec4<f32>,      // x, height, row, 0
    right: vec4<f32>,
    up: vec4<f32>,
    fwd: vec4<f32>,
    proj: vec4<f32>,     // fx, fy, near, far
    fog: vec4<f32>,      // fog_start, fog_end, cull_radius, textured(1/0)
    sky: vec4<f32>,      // yaw (raw/65536 turns), pitch raw, roll raw, enabled
    screen: vec4<f32>,   // width, height, canvas_w, canvas_h
    misc: vec4<f32>,     // fade 0..1, bg_enabled, fog models (1/0), smoothing (1/0)
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var palette_tex: texture_2d<f32>;    // 256x1 rgba8
@group(0) @binding(2) var light_tex: texture_2d<u32>;      // 256x328 r8uint
@group(0) @binding(3) var terrain_tex: texture_2d_array<u32>; // 64x64x256
@group(0) @binding(4) var atlas_tex: texture_2d<u32>;      // model atlas r8uint
@group(0) @binding(5) var tmap_tex: texture_2d<u32>;       // billboards rg8uint (idx, opaque)
@group(0) @binding(6) var sky_tex: texture_2d<u32>;        // 256x256 r8uint
@group(0) @binding(7) var lin_samp: sampler;               // linear, repeat
@group(0) @binding(8) var sky_rgb: texture_2d<f32>;        // the sky in colour (SMOOTHING)

fn pal(i: u32) -> vec4<f32> {
    return textureLoad(palette_tex, vec2<i32>(i32(i), 0), 0);
}
fn light(row: i32, c: u32) -> u32 {
    return textureLoad(light_tex, vec2<i32>(i32(c), clamp(row, 0, 63)), 0).r;
}

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) lightv: f32,
    @location(3) kind: u32,
    @location(4) param: u32,
};
struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) lightv: f32,
    @location(2) @interpolate(flat) kind: u32,
    @location(3) @interpolate(flat) param: u32,
    @location(4) depth: f32,
    @location(5) hdist: f32,
};

@vertex
fn vs_world(v: VIn) -> VOut {
    let d = v.pos - g.eye.xyz;
    let vx = dot(d, g.right.xyz);
    let vy = dot(d, g.up.xyz);
    let vz = dot(d, g.fwd.xyz);
    let near = g.proj.z;
    let far = g.proj.w;
    var o: VOut;
    o.clip = vec4<f32>(vx * g.proj.x, vy * g.proj.y, (vz - near) * far / (far - near), vz);
    o.uv = v.uv;
    o.lightv = v.lightv;
    o.kind = v.kind;
    o.param = v.param;
    o.depth = vz;
    o.hdist = length(vec2<f32>(d.x, d.z));
    return o;
}

// kind & 0xff: 0 terrain, 1 model atlas, 2 flat colour, 3 billboard
// kind >> 8: fog mode 0 none, 1 terrain, 2 sprite
fn shade(i: VOut) -> vec4<f32> {
    let k = i.kind & 0xffu;
    let fogm = i.kind >> 8u;
    var c: u32 = 0u;
    var alpha = 1.0;
    if (k == 0u) {
        let uv = vec2<i32>(clamp(vec2<i32>(floor(i.uv)), vec2<i32>(0), vec2<i32>(63)));
        c = textureLoad(terrain_tex, uv, i32(i.param), 0).r;
    } else if (k == 1u) {
        c = textureLoad(atlas_tex, vec2<i32>(floor(i.uv)), 0).r;
    } else if (k == 2u) {
        c = i.param & 0xffu;
    } else {
        let t = textureLoad(tmap_tex, vec2<i32>(floor(i.uv)), 0);
        if (t.g == 0u) { discard; }
        c = t.r;
        if ((i.param & 0xffu) != 0u) { alpha = 0.5; }
    }
    var row = i.lightv;
    let fs = g.fog.x;
    let fe = g.fog.y;
    if (fogm == 1u) {
        if (i.hdist > g.fog.z) { discard; }
        if (i.depth > fe) { row = 0.0; }
        else if (i.depth > fs) { row = mix(row, 0.5, (i.depth - fs) / (fe - fs)); }
    } else if (fogm == 2u) {
        if (i.depth >= fe) { row = 0.0; }
        else if (i.depth > fs) { row = min(row, floor((fe - i.depth) * 32.0 / (fe - fs))); }
    } else if (g.misc.z > 0.5) {
        // port: with a longer view distance models fade into the haze like the terrain
        if (i.depth > fe) { row = 0.0; }
        else if (i.depth > fs) { row = mix(row, 0.5, (i.depth - fs) / (fe - fs)); }
    }
    let lc = light(i32(floor(row)), c);
    let col = pal(lc);
    return vec4<f32>(col.rgb, alpha);
}

@fragment
fn fs_world(i: VOut) -> @location(0) vec4<f32> {
    return shade(i);
}

// ---------------- fullscreen passes ----------------
struct FOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> FOut {
    var p = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    var o: FOut;
    o.clip = vec4<f32>(p[vi], 0.0, 1.0);
    o.uv = vec2<f32>((p[vi].x + 1.0) * 0.5, (1.0 - p[vi].y) * 0.5);
    return o;
}

const PI: f32 = 3.14159265;

// Sky (spec 08 §6), generalised to any aspect: 256 texels span the width of a 4:3 view of this height.
@fragment
fn fs_sky(i: FOut) -> @location(0) vec4<f32> {
    if (g.sky.w < 0.5) {
        return vec4<f32>(pal(light(0, 255u)).rgb, 1.0);
    }
    let W = g.screen.x;
    let H = g.screen.y;
    let ref_w = H * 4.0 / 3.0;
    let px = i.uv * vec2<f32>(W, H);
    let roll = g.sky.z / 65536.0 * 2.0 * PI;
    let pitch = g.sky.y / 65536.0 * 2.0 * PI;
    let s = sin(roll);
    let c = cos(roll);
    let sp = sin(pitch) * 64.0 * (ref_w / 320.0);
    let hx = W * 0.5 - s * sp;
    let hy = H * 0.5 - c * sp;
    let scale = 256.0 / ref_w;
    let dx = px.x - hx;
    let dy = px.y - hy;
    let u = g.sky.x * 256.0 * 2.0 + (c * dx - s * dy) * scale;
    let v = (s * dx + c * dy) * scale;
    if (g.misc.w > 0.5) {
        // SMOOTHING: bilinear filtering of the sky in colour (texel i spans [i, i+1) like below)
        return vec4<f32>(textureSampleLevel(sky_rgb, lin_samp, vec2<f32>(u, v) / 256.0, 0.0).rgb, 1.0);
    }
    let tu = i32(floor(u)) & 255;
    let tv = i32(floor(v)) & 255;
    let t = textureLoad(sky_tex, vec2<i32>(tu, tv), 0).r;
    return vec4<f32>(pal(t).rgb, 1.0);
}

@group(1) @binding(0) var scene_tex: texture_2d<f32>;
@group(1) @binding(1) var bg_tex: texture_2d<u32>;
@group(1) @binding(2) var fg_tex: texture_2d<u32>;

// A canvas pixel as premultiplied colour: opaque a = 1, translucent a = 0.5, transparent a = 0.
fn canvas_pm(t: texture_2d<u32>, p: vec2<i32>) -> vec4<f32> {
    let q = clamp(p, vec2<i32>(0), vec2<i32>(i32(g.screen.z) - 1, i32(g.screen.w) - 1));
    let v = textureLoad(t, q, 0).r;
    if ((v & 0x100u) != 0u) { return vec4<f32>(pal(v & 0xffu).rgb, 1.0); }
    if ((v & 0x200u) != 0u) { return vec4<f32>(pal(v & 0xffu).rgb * 0.5, 0.5); }
    return vec4<f32>(0.0);
}

// SMOOTHING: sharp-bilinear scaling of the canvas. Pixels stay crisp; only the one screen pixel
// where two canvas pixels meet is blended, so uneven nearest-neighbour widths disappear.
fn canvas_sharp(t: texture_2d<u32>, uv: vec2<f32>) -> vec4<f32> {
    let p = uv * vec2<f32>(g.screen.z, g.screen.w) - 0.5;
    let i0 = vec2<i32>(floor(p));
    let scale = max(g.screen.y / g.screen.w, 1.0);
    let f = clamp((fract(p) - 0.5) * scale + 0.5, vec2<f32>(0.0), vec2<f32>(1.0));
    let a = mix(canvas_pm(t, i0), canvas_pm(t, i0 + vec2<i32>(1, 0)), f.x);
    let b = mix(canvas_pm(t, i0 + vec2<i32>(0, 1)), canvas_pm(t, i0 + vec2<i32>(1, 1)), f.x);
    return mix(a, b, f.y);
}

fn canvas_layer(t: texture_2d<u32>, uv: vec2<f32>) -> vec4<f32> {
    if (g.misc.w > 0.5) { return canvas_sharp(t, uv); }
    return canvas_pm(t, vec2<i32>(floor(uv * vec2<f32>(g.screen.z, g.screen.w))));
}

@fragment
fn fs_composite(i: FOut) -> @location(0) vec4<f32> {
    var col = vec3<f32>(0.0);
    if (g.misc.y > 0.5) {
        let b = canvas_layer(bg_tex, i.uv);
        col = col * (1.0 - b.a) + b.rgb;
    }
    let sp = vec2<i32>(floor(i.uv * vec2<f32>(textureDimensions(scene_tex))));
    let s = textureLoad(scene_tex, sp, 0);
    col = mix(col, s.rgb, s.a);
    let f = canvas_layer(fg_tex, i.uv);
    col = col * (1.0 - f.a) + f.rgb;
    return vec4<f32>(col * g.misc.x, 1.0);
}
