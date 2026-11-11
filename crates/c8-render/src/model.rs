//! Convert a model into world-space triangles (spec 08 §8); shared by the game's
//! renderer and the editor's model viewer.
use crate::{Vertex, K_ATLAS, K_FLAT};
use c8_data::images::TexAtlas;
use c8_data::model::Model;

/// Raw 16-bit angle (65536 per turn) in radians.
pub fn ang(raw: f32) -> f32 {
    raw / 65536.0 * std::f32::consts::TAU
}

/// 3x3 rotation, rows = world axes (x, height, row); applied to model-space (mx, mh, mr).
pub type Mat3 = [[f32; 3]; 3];

pub fn mat_mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    r
}

/// Orientation of a craft with yaw, pitch (positive nose down), roll (positive left side up),
/// all raw 16-bit units. Model space: x right, h up, r backwards (+row when heading north).
pub fn orient(yaw: f32, pitch: f32, roll: f32) -> Mat3 {
    let a = ang;
    let (sy, cy) = a(yaw).sin_cos();
    let (sp, cp) = a(pitch).sin_cos();
    let (sr, cr) = a(roll).sin_cos();
    // roll about the forward axis (model -r): left side up => x axis tilts down on the right
    let rz: Mat3 = [[cr, sr, 0.0], [-sr, cr, 0.0], [0.0, 0.0, 1.0]];
    // pitch nose-down: forward (-r) tips towards -h
    let rx: Mat3 = [[1.0, 0.0, 0.0], [0.0, cp, sp], [0.0, -sp, cp]];
    // yaw clockwise from above: heading east => model forward (-r) maps to +x
    let ry: Mat3 = [[cy, 0.0, -sy], [0.0, 1.0, 0.0], [sy, 0.0, cy]];
    mat_mul(&ry, &mat_mul(&rx, &rz))
}

/// Append a model's triangles. `pos` = world (x, height, row). File vertex (x, y, z) -> model (x, z, y)
/// in 1/2048 cells (file z up, file y along +row). Front faces are clockwise on screen; the GPU
/// pipeline does not cull, back faces are hidden by the depth buffer.
pub fn push_model(out: &mut Vec<Vertex>, m: &Model, atlas: &TexAtlas, pos: [f32; 3], rot: &Mat3, scale: f32, flicker: u32) {
    let s = scale / 2048.0;
    let xf = |v: [i16; 3]| -> [f32; 3] {
        let mv = [v[0] as f32 * s, v[2] as f32 * s, v[1] as f32 * s];
        [
            pos[0] + rot[0][0] * mv[0] + rot[0][1] * mv[1] + rot[0][2] * mv[2],
            pos[1] + rot[1][0] * mv[0] + rot[1][1] * mv[1] + rot[1][2] * mv[2],
            pos[2] + rot[2][0] * mv[0] + rot[2][1] * mv[1] + rot[2][2] * mv[2],
        ]
    };
    for t in &m.tris {
        let p = [xf(m.verts[t.v[0] as usize]), xf(m.verts[t.v[1] as usize]), xf(m.verts[t.v[2] as usize])];
        match t.kind {
            2 | 3 | 5 | 6 => {
                let (ex, ey, _, _) = atlas.entries.get(t.col as usize).copied().unwrap_or((0, 0, 1, 1));
                let light = 32.0;
                for k in 0..3 {
                    let (u, v) = t.uv[k];
                    out.push(Vertex { pos: p[k], uv: [ex as f32 + u as f32 + 0.5, ey as f32 + v as f32 + 0.5], light, kind: K_ATLAS, param: 0 });
                }
            }
            4 => {
                let l = if m.flags & 4 != 0 {
                    let sum: u32 = t.v.iter().map(|&v| (m.shade[v as usize] >> 2) as u32).sum();
                    64.0 - (sum / 3) as f32
                } else {
                    (16 + (flicker % 16)) as f32
                };
                for k in 0..3 {
                    out.push(Vertex { pos: p[k], uv: [0.0, 0.0], light: l, kind: K_FLAT, param: t.col as u32 });
                }
            }
            _ => {
                for k in 0..3 {
                    out.push(Vertex { pos: p[k], uv: [0.0, 0.0], light: 32.0, kind: K_FLAT, param: t.col as u32 });
                }
            }
        }
    }
}
