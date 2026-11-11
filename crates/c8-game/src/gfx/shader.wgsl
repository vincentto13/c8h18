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
    opts: vec4<f32>,     // texture filtering (1/0), 0, 0, 0
    // port addition, true-colour sky (spec 08 §6.1); all zero without one
    sun: vec4<f32>,      // world direction towards the sun, sun present (1/0)
    sun_col: vec4<f32>,  // sun colour 0..1, angular radius (radians)
    tsky: vec4<f32>,     // repeat, top, bottom (radians), azimuth of the left edge (radians)
    tsky2: vec4<f32>,    // true sky (1/0), horizon blend (radians), glare strength (0 = off), 0
    glare: vec4<f32>,    // sun position (uv), edge fade 0..1 (0 = no glare), sun radius in pixels
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
// TEXTURE FILTERING: colour copies of the textures and the light rows as colour transforms
@group(0) @binding(10) var sky_true: texture_2d<f32>;        // true-colour panorama (port addition)
@group(0) @binding(9) var terrain_rgb: texture_2d_array<f32>; // 64x64x256, mipmapped
@group(0) @binding(12) var light_fit: texture_2d<f32>;        // 64x2: row r = (a at y 0, b at y 1)
@group(0) @binding(13) var aniso_samp: sampler;               // trilinear, anisotropic, clamp

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

// Light row (fractional, 0..63) applied to a colour through the fitted per-row transform, blending
// the two neighbouring rows (used for distant, minified terrain only).
fn light_rgb(rgb: vec3<f32>, row: f32) -> vec3<f32> {
    let r = clamp(row, 0.0, 63.0);
    let r0 = i32(floor(r));
    let r1 = min(r0 + 1, 63);
    let f = r - f32(r0);
    let a = mix(textureLoad(light_fit, vec2<i32>(r0, 0), 0).rgb, textureLoad(light_fit, vec2<i32>(r1, 0), 0).rgb, f);
    let b = mix(textureLoad(light_fit, vec2<i32>(r0, 1), 0).rgb, textureLoad(light_fit, vec2<i32>(r1, 1), 0).rgb, f);
    return clamp(a * rgb + b, vec3<f32>(0.0), vec3<f32>(1.0));
}

// Palette index lit exactly through the light table, blending the two neighbouring rows.
fn lit(c: u32, row: f32) -> vec3<f32> {
    let r = clamp(row, 0.0, 63.0);
    let r0 = i32(floor(r));
    let r1 = min(r0 + 1, 63);
    return mix(pal(light(r0, c)).rgb, pal(light(r1, c)).rgb, r - f32(r0));
}

// Bilinear filtering of an indexed texture: the four nearest texels are lit exactly, then blended.
fn terrain_bilinear(uv: vec2<f32>, layer: i32, row: f32) -> vec3<f32> {
    let p = uv - 0.5;
    let i0 = vec2<i32>(floor(p));
    let f = fract(p);
    var c: array<vec3<f32>, 4>;
    let offs = array<vec2<i32>, 4>(vec2<i32>(0, 0), vec2<i32>(1, 0), vec2<i32>(0, 1), vec2<i32>(1, 1));
    for (var k = 0; k < 4; k++) {
        let q = clamp(i0 + offs[k], vec2<i32>(0), vec2<i32>(63));
        c[k] = lit(textureLoad(terrain_tex, q, layer, 0).r, row);
    }
    return mix(mix(c[0], c[1], f.x), mix(c[2], c[3], f.x), f.y);
}

fn atlas_bilinear(uv: vec2<f32>, row: f32) -> vec3<f32> {
    let p = uv - 0.5;
    let i0 = vec2<i32>(floor(p));
    let f = fract(p);
    let mx = vec2<i32>(textureDimensions(atlas_tex)) - 1;
    var c: array<vec3<f32>, 4>;
    let offs = array<vec2<i32>, 4>(vec2<i32>(0, 0), vec2<i32>(1, 0), vec2<i32>(0, 1), vec2<i32>(1, 1));
    for (var k = 0; k < 4; k++) {
        c[k] = lit(textureLoad(atlas_tex, clamp(i0 + offs[k], vec2<i32>(0), mx), 0).r, row);
    }
    return mix(mix(c[0], c[1], f.x), mix(c[2], c[3], f.x), f.y);
}

// Billboards: premultiplied by the opaque flag, so the transparent surround fades out at the edge.
fn tmap_bilinear(uv: vec2<f32>, row: f32) -> vec4<f32> {
    let p = uv - 0.5;
    let i0 = vec2<i32>(floor(p));
    let f = fract(p);
    var c: array<vec4<f32>, 4>;
    let offs = array<vec2<i32>, 4>(vec2<i32>(0, 0), vec2<i32>(1, 0), vec2<i32>(0, 1), vec2<i32>(1, 1));
    for (var k = 0; k < 4; k++) {
        let t = textureLoad(tmap_tex, clamp(i0 + offs[k], vec2<i32>(0), vec2<i32>(1023)), 0);
        if (t.g != 0u) { c[k] = vec4<f32>(lit(t.r, row), 1.0); } else { c[k] = vec4<f32>(0.0); }
    }
    return mix(mix(c[0], c[1], f.x), mix(c[2], c[3], f.x), f.y);
}

// TEXTURE FILTERING path: the same light and haze rules without the integer steps; textures are
// filtered after lighting (exact colours), distant terrain uses the mipmapped colour copy.
fn shade_filtered(i: VOut, duvdx: vec2<f32>, duvdy: vec2<f32>) -> vec4<f32> {
    let k = i.kind & 0xffu;
    let fogm = i.kind >> 8u;
    var row = i.lightv;
    let fs = g.fog.x;
    let fe = g.fog.y;
    if (fogm == 1u) {
        if (i.hdist > g.fog.z) { discard; }
        if (i.depth > fe) { row = 0.0; }
        else if (i.depth > fs) { row = mix(row, 0.5, (i.depth - fs) / (fe - fs)); }
    } else if (fogm == 2u) {
        if (i.depth >= fe) { row = 0.0; }
        else if (i.depth > fs) { row = min(row, (fe - i.depth) * 32.0 / (fe - fs)); }
    } else if (g.misc.z > 0.5) {
        if (i.depth > fe) { row = 0.0; }
        else if (i.depth > fs) { row = mix(row, 0.5, (i.depth - fs) / (fe - fs)); }
    }
    var rgb = vec3<f32>(0.0);
    var alpha = 1.0;
    if (k == 0u) {
        // texels per screen pixel: up to 1 exact bilinear, from 2 the mipmaps, blended in between
        let lod = log2(max(max(length(duvdx), length(duvdy)), 1e-6));
        let w = clamp(lod, 0.0, 1.0);
        if (w < 1.0) {
            rgb = terrain_bilinear(i.uv, i32(i.param), row);
        }
        if (w > 0.0) {
            let far = light_rgb(textureSampleGrad(terrain_rgb, aniso_samp, i.uv / 64.0, i32(i.param), duvdx / 64.0, duvdy / 64.0).rgb, row);
            rgb = mix(rgb, far, w);
        }
    } else if (k == 1u) {
        rgb = atlas_bilinear(i.uv, row);
    } else if (k == 2u) {
        rgb = lit(i.param & 0xffu, row);
    } else {
        let t = tmap_bilinear(i.uv, row);
        if (t.a < 0.5) { discard; }
        rgb = t.rgb / t.a;
        if ((i.param & 0xffu) != 0u) { alpha = 0.5; }
    }
    return vec4<f32>(rgb, alpha);
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
    // derivatives first, in uniform control flow (used by the filtered terrain lookup)
    let duvdx = dpdx(i.uv);
    let duvdy = dpdy(i.uv);
    if (g.opts.x > 0.5) {
        return shade_filtered(i, duvdx, duvdy);
    }
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

// The view ray through a fullscreen uv, in world axes (x, height, row).
fn view_ray(uv: vec2<f32>) -> vec3<f32> {
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    return normalize(g.right.xyz * (ndc.x / g.proj.x) + g.up.xyz * (ndc.y / g.proj.y) + g.fwd.xyz);
}

// Port addition (spec 08 §6.1): the true-colour sky, a cylindrical panorama seen in perspective,
// faded into the light table's haze colour at the horizon, with the sun disk and its corona.
fn true_sky(uv: vec2<f32>) -> vec3<f32> {
    let d = view_ray(uv);
    let el = asin(clamp(d.y, -1.0, 1.0));
    let az = atan2(d.x, -d.z);
    let dims = vec2<f32>(textureDimensions(sky_true));
    let u = fract((az - g.tsky.w) / (2.0 * PI) * g.tsky.x);
    let v = clamp((g.tsky.y - el) / (g.tsky.y - g.tsky.z), 0.5 / dims.y, 1.0 - 0.5 / dims.y);
    var col = textureSampleLevel(sky_true, lin_samp, vec2<f32>(u, v), 0.0).rgb;
    let haze = pal(light(0, 255u)).rgb;
    var t = 0.0;
    if (g.tsky2.y > 0.0) {
        t = smoothstep(0.0, g.tsky2.y, el);
    } else if (el > 0.0) {
        t = 1.0;
    }
    col = mix(haze, col, t);
    if (g.sun.w > 0.5) {
        let r = g.sun_col.w;
        let a = length(d - g.sun.xyz); // chord, the angle for small angles
        let corona = exp(-a / (r * 2.2)) * 0.45 + exp(-a / (r * 9.0)) * 0.10;
        col = col + g.sun_col.rgb * corona * (1.0 - col * 0.5);
        let px = r * g.proj.y * g.screen.y * 0.5; // radius in pixels
        let disk = clamp((r - a) / r * px + 0.5, 0.0, 1.0);
        // the disk: the sun colour pushed to near white, never darker than the corona around it
        col = mix(col, max(col, mix(g.sun_col.rgb, vec3<f32>(1.0), 0.85)), disk);
    }
    return min(col, vec3<f32>(1.0));
}

// Sky (spec 08 §6), generalised to any aspect: 256 texels span the width of a 4:3 view of this height.
@fragment
fn fs_sky(i: FOut) -> @location(0) vec4<f32> {
    if (g.sky.w < 0.5) {
        return vec4<f32>(pal(light(0, 255u)).rgb, 1.0);
    }
    if (g.tsky2.x > 0.5) {
        return vec4<f32>(true_sky(i.uv), 1.0);
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
@group(1) @binding(3) var sun_vis: texture_2d<f32>;           // 1x1: visible part of the sun
// the sun visibility pass reads the scene depth (single or multisampled)
@group(1) @binding(4) var depth_ss: texture_depth_2d;
@group(1) @binding(5) var depth_ms: texture_depth_multisampled_2d;

// Sun visibility (port addition): the share of a small disc around the sun where the depth buffer
// still holds the clear value (only the sky was drawn there). Rendered into a 1x1 target.
const VIS_N: i32 = 8;
// Sample k (0..VIS_N²) of the disc around the sun in pixels; w = 0 for grid points outside the disc.
fn vis_point(k: i32) -> vec3<i32> {
    let size = vec2<i32>(i32(g.screen.x), i32(g.screen.y));
    let c = g.glare.xy * g.screen.xy;
    let rad = max(g.glare.w * 1.6, 3.0);
    let o = (vec2<f32>(f32(k % VIS_N), f32(k / VIS_N)) + 0.5) / f32(VIS_N) * 2.0 - 1.0;
    let p = clamp(vec2<i32>(floor(c + o * rad)), vec2<i32>(0), size - 1);
    return vec3<i32>(p, select(0, 1, dot(o, o) <= 1.0));
}
@fragment
fn fs_sunvis(i: FOut) -> @location(0) vec4<f32> {
    var seen = 0.0;
    var total = 0.0;
    for (var k = 0; k < VIS_N * VIS_N; k++) {
        let p = vis_point(k);
        if (p.z == 0) { continue; }
        total += 1.0;
        if (textureLoad(depth_ss, p.xy, 0) >= 1.0) { seen += 1.0; }
    }
    return vec4<f32>(seen / max(total, 1.0), 0.0, 0.0, 1.0);
}
@fragment
fn fs_sunvis_ms(i: FOut) -> @location(0) vec4<f32> {
    var seen = 0.0;
    var total = 0.0;
    for (var k = 0; k < VIS_N * VIS_N; k++) {
        let p = vis_point(k);
        if (p.z == 0) { continue; }
        total += 1.0;
        if (textureLoad(depth_ms, p.xy, 0) >= 1.0) { seen += 1.0; }
    }
    return vec4<f32>(seen / max(total, 1.0), 0.0, 0.0, 1.0);
}

// Sun glare (port addition): bloom around the sun, a veil towards it and a chain of lens-flare
// ghosts along the line from the sun through the screen centre, scaled by the sun's visibility
// and faded out towards the screen edge. Additive, on the 3D view only (below the HUD).
fn soft_disc(p: vec2<f32>, c: vec2<f32>, r: f32) -> f32 {
    let d = length(p - c) / r;
    return clamp(1.0 - d * d, 0.0, 1.0);
}
fn sun_glare(uv: vec2<f32>) -> vec3<f32> {
    let vis = textureLoad(sun_vis, vec2<i32>(0, 0), 0).r * g.glare.z;
    if (vis <= 0.001) { return vec3<f32>(0.0); }
    let asp = vec2<f32>(g.screen.x / g.screen.y, 1.0);
    let p = uv * asp;
    let s = g.glare.xy * asp;
    let ctr = vec2<f32>(0.5) * asp;
    let r = length(p - s);
    // bloom (tight and wide) and a warm veil that grows towards the sun
    var add = g.sun_col.rgb * (exp(-r * 40.0) * 0.40 + exp(-r * 9.0) * 0.10 + exp(-r * 2.2) * 0.05 + 0.01);
    // ghosts: position along sun -> centre, radius, intensity, tint
    let axis = ctr - s;
    add += vec3<f32>(1.0, 0.75, 0.45) * soft_disc(p, s + axis * 0.45, 0.035) * 0.10;
    add += vec3<f32>(0.55, 1.0, 0.6) * soft_disc(p, s + axis * 0.8, 0.06) * 0.06;
    add += vec3<f32>(0.6, 0.7, 1.0) * soft_disc(p, s + axis * 1.3, 0.11) * 0.05;
    add += vec3<f32>(1.0, 0.6, 0.85) * soft_disc(p, s + axis * 1.7, 0.025) * 0.10;
    add += vec3<f32>(0.75, 0.85, 1.0) * soft_disc(p, s + axis * 2.15, 0.16) * 0.035;
    // a faint ring around the centre
    let ring = abs(length(p - (s + axis * 1.0)) - 0.28);
    add += g.sun_col.rgb * clamp(1.0 - ring / 0.012, 0.0, 1.0) * 0.025;
    return add * vis * g.tsky2.z;
}

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
    if (g.glare.z > 0.0) {
        col = min(col + sun_glare(i.uv) * s.a, vec3<f32>(1.0));
    }
    let f = canvas_layer(fg_tex, i.uv);
    col = col * (1.0 - f.a) + f.rgb;
    return vec4<f32>(col * g.misc.x, 1.0);
}
