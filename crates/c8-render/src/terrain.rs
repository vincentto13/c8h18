//! Level mesh (spec 08 §5.2): floor quads and block faces as GPU triangles.
use crate::{Vertex, K_FLAT_TERRAIN, K_TERRAIN};
use c8_data::level::{Level, MAP_H, MAP_W};

const E: f32 = 64.0;
/// UV entries (corner order of the quad), 63 stands for 64 - epsilon.
const UV: [[(f32, f32); 4]; 16] = [
    [(0., 0.), (E, 0.), (E, E), (0., E)],
    [(E, 0.), (0., 0.), (0., E), (E, E)],
    [(0., E), (E, E), (E, 0.), (0., 0.)],
    [(E, E), (0., E), (0., 0.), (E, 0.)],
    [(0., 0.), (0., E), (E, E), (E, 0.)],
    [(0., E), (0., 0.), (E, 0.), (E, E)],
    [(E, 0.), (E, E), (0., E), (0., 0.)],
    [(E, E), (E, 0.), (0., 0.), (0., E)],
    [(0., 0.), (E, 0.), (E, E), (0., E)],
    [(E, 0.), (0., 0.), (0., E), (E, E)],
    [(0., E), (E, E), (E, 0.), (0., 0.)],
    [(E, E), (0., E), (0., 0.), (E, 0.)],
    [(0., 0.), (0., E), (E, E), (E, 0.)],
    [(E, 0.), (E, E), (0., E), (0., 0.)],
    [(0., E), (0., 0.), (E, 0.), (E, E)],
    [(E, E), (E, 0.), (0., 0.), (0., E)],
];

const COMPOSE_TOP: [u8; 64] = [
    0, 1, 2, 3, 4, 5, 6, 7, 1, 0, 3, 2, 5, 4, 7, 6, 2, 3, 0, 1, 6, 7, 4, 5, 3, 2, 1, 0, 7, 6, 5, 4, //
    4, 6, 5, 7, 0, 2, 1, 3, 5, 7, 4, 6, 1, 3, 0, 2, 6, 4, 7, 5, 2, 0, 3, 1, 7, 5, 6, 4, 3, 1, 2, 0,
];
const COMPOSE_BOTTOM: [u8; 64] = [
    0, 1, 2, 3, 4, 5, 6, 7, 1, 0, 3, 2, 5, 7, 4, 6, 2, 3, 0, 1, 6, 4, 7, 5, 3, 2, 1, 0, 7, 6, 5, 4, //
    7, 6, 5, 4, 0, 2, 1, 3, 5, 4, 7, 6, 1, 3, 0, 2, 6, 7, 4, 5, 2, 0, 3, 1, 4, 5, 6, 7, 3, 1, 2, 0,
];

fn compose_top(cell_o: u8, face_o: u8) -> usize {
    COMPOSE_TOP[((cell_o as usize * 8) + (face_o as usize & 7)) & 63] as usize
}
fn compose_bottom(cell_o: u8, face_o: u8) -> usize {
    COMPOSE_BOTTOM[((cell_o as usize * 8) + (face_o as usize & 7)) & 63] as usize + 8
}

pub struct MeshOpts<'a> {
    pub shading: bool,
    /// Some(table) = TEXTURE MAPPING off: textures with attribute byte 1 become flat colours
    pub flat: Option<&'a c8_data::images::ColourTable>,
}

/// Texture attribute bytes (spec 08 §5.4): 0 = always textured, 1 = flat when mapping is off.
const TEX_ATTR: [u8; 160] = [
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
    1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, //
    0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, //
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 1,
];

/// Quad: 4 world points (x, height, row), lights per corner, UV entry; `odd` chooses the diagonal.
fn quad(out: &mut Vec<Vertex>, p: [[f32; 3]; 4], l: [f32; 4], uv_entry: usize, tex: u32, odd: bool, opts: &MeshOpts) {
    let uv = UV[uv_entry & 15];
    let tris: [[usize; 3]; 2] = if odd { [[0, 1, 2], [0, 2, 3]] } else { [[0, 1, 3], [1, 2, 3]] };
    for t in tris {
        let avg = (l[t[0]] + l[t[1]] + l[t[2]]) / 3.0;
        if let Some(table) = opts.flat {
            if TEX_ATTR.get(tex as usize).copied().unwrap_or(0) != 0 {
                let (u, v) = uv[t[1]];
                let variant = match (u > 0.0, v > 0.0) {
                    (false, false) => 0,
                    (true, false) => 1,
                    (true, true) => 2,
                    (false, true) => 3,
                };
                let c = table.flat(variant, tex as usize) as u32;
                for &k in &t {
                    out.push(Vertex { pos: p[k], uv: [0.0, 0.0], light: avg, kind: K_FLAT_TERRAIN, param: c });
                }
                continue;
            }
        }
        for &k in &t {
            out.push(Vertex {
                pos: p[k],
                uv: [uv[k].0, uv[k].1],
                light: if opts.shading && opts.flat.is_none() { l[k] } else { avg },
                kind: K_TERRAIN,
                param: tex,
            });
        }
    }
}

pub fn build_mesh(lvl: &Level, opts: &MeshOpts) -> Vec<Vertex> {
    let mut out = Vec::with_capacity(MAP_W * MAP_H * 6 + 100_000);
    let light = |x: i32, y: i32| lvl.cell(x, y).light as f32 / 256.0;
    // a neighbour's block hides a side only when both stand on the same base (port addition: a
    // flat block base, spec 01 §9.7)
    let has_block = |x: i32, y: i32, k: usize, base: Option<u16>| -> bool {
        if x < 0 || y < 0 || x >= MAP_W as i32 || y >= MAP_H as i32 {
            return false;
        }
        lvl.cell(x, y).base == base && lvl.column(x, y).map(|c| c.mask & (1 << k) != 0).unwrap_or(false)
    };
    for y in 0..MAP_H as i32 {
        for x in 0..MAP_W as i32 {
            let c = lvl.cell(x, y);
            let h = [lvl.corner_h(x, y), lvl.corner_h(x + 1, y), lvl.corner_h(x + 1, y + 1), lvl.corner_h(x, y + 1)];
            let xy = [(x, y), (x + 1, y), (x + 1, y + 1), (x, y + 1)];
            let l = [light(x, y), light(x + 1, y), light(x + 1, y + 1), light(x, y + 1)];
            let odd = (x + y) & 1 != 0;
            let pt = |k: usize, dz: f32| [xy[k].0 as f32, h[k] + dz, xy[k].1 as f32];
            // block corners: on the terrain corners, or all on the cell's flat base (port addition)
            let flat = c.base.map(|b| b as f32 / 256.0);
            let bp = |k: usize, dz: f32| match flat {
                Some(b) => [xy[k].0 as f32, b + dz, xy[k].1 as f32],
                None => pt(k, dz),
            };
            match lvl.column(x, y).copied() {
                None => {
                    quad(&mut out, [pt(0, 0.), pt(1, 0.), pt(2, 0.), pt(3, 0.)], l, c.orient as usize, (c.tex as u32) & 255, odd, opts);
                }
                Some(col) => {
                    if col.mask & 1 == 0 || flat.is_some() {
                        let tex = col.floor_tex.max(0) as u32 & 255;
                        quad(&mut out, [pt(0, 0.), pt(1, 0.), pt(2, 0.), pt(3, 0.)], l, c.orient as usize, tex, odd, opts);
                    }
                    for k in 0..8usize {
                        if col.mask & (1 << k) == 0 {
                            continue;
                        }
                        let bid = col.blocks[k].max(0) as usize;
                        let b = lvl.blocks.get(bid).copied().unwrap_or_default();
                        let (z0, z1) = (k as f32, k as f32 + 1.0);
                        // top
                        if col.mask & (1 << (k + 1)) == 0 {
                            quad(&mut out, [bp(0, z1), bp(1, z1), bp(2, z1), bp(3, z1)], l, compose_top(c.orient, b.orient[4]), b.tex[4] as u32, odd, opts);
                        }
                        // bottom
                        if (k > 0 && col.mask & (1 << (k - 1)) == 0) || (k == 0 && flat.is_some()) {
                            quad(&mut out, [bp(3, z0), bp(2, z0), bp(1, z0), bp(0, z0)], l, compose_bottom(c.orient, b.orient[5]), b.tex[5] as u32, odd, opts);
                        }
                        // sides, seen from outside: top-left, top-right, bottom-right, bottom-left
                        // north (row y edge, corners 0,1), seen from the north: left = east (1), right = west (0)
                        if !has_block(x, y - 1, k, c.base) {
                            quad(&mut out, [bp(1, z1), bp(0, z1), bp(0, z0), bp(1, z0)], [l[1], l[0], l[0], l[1]], 0, b.tex[0] as u32, false, opts);
                        }
                        // east (x+1 edge, corners 1,2), seen from the east: left = south (2), right = north (1)
                        if !has_block(x + 1, y, k, c.base) {
                            quad(&mut out, [bp(2, z1), bp(1, z1), bp(1, z0), bp(2, z0)], [l[2], l[1], l[1], l[2]], 0, b.tex[1] as u32, false, opts);
                        }
                        // south (row y+1 edge, corners 3,2), seen from the south: left = west (3), right = east (2)
                        if !has_block(x, y + 1, k, c.base) {
                            quad(&mut out, [bp(3, z1), bp(2, z1), bp(2, z0), bp(3, z0)], [l[3], l[2], l[2], l[3]], compose_top(0, b.orient[2]), b.tex[2] as u32, false, opts);
                        }
                        // west (x edge, corners 0,3), seen from the west: left = north (0), right = south (3)
                        if !has_block(x - 1, y, k, c.base) {
                            quad(&mut out, [bp(0, z1), bp(3, z1), bp(3, z0), bp(0, z0)], [l[0], l[3], l[3], l[0]], 0, b.tex[3] as u32, false, opts);
                        }
                    }
                }
            }
        }
    }
    out
}
