//! "BULLFROG OBJECT DATA" 3D models (spec 01 §8).
use super::files::{i16le, read_raw, u16le, u32le};

#[derive(Clone, Copy, Debug)]
pub struct Tri {
    pub kind: u8,
    pub v: [u16; 3],
    /// flat colour or texture atlas entry
    pub col: u8,
    pub uv: [(u8, u8); 3],
}

#[derive(Clone, Default)]
pub struct Model {
    pub tris: Vec<Tri>,
    /// file coordinates (x, y, z), 1/2048 cell
    pub verts: Vec<[i16; 3]>,
    pub shade: Vec<u8>,
    pub flags: u8,
}

impl Model {
    pub fn load(rel: &str) -> Result<Model, String> {
        let d = read_raw(rel)?;
        Self::parse(&d).map_err(|e| format!("{rel}: {e}"))
    }
    pub fn parse(d: &[u8]) -> Result<Model, String> {
        if d.len() < 0x3a || !d[..36].windows(20).any(|w| w == b"BULLFROG OBJECT DATA") {
            return Err("not an object file".into());
        }
        let tri_size = u32le(d, 0x18) as usize;
        let flags = d[0x24];
        let frames = d[0x25].max(1) as usize;
        let ntri = u16le(d, 0x26) as usize;
        let nvert = u16le(d, 0x28) as usize;
        let mut p = 0x3a;
        if flags & 2 != 0 {
            p += frames * 8;
        }
        let tri_end = p + tri_size;
        let mut tris = Vec::with_capacity(ntri);
        while p < tri_end && tris.len() < ntri {
            let kind = d[p];
            let big = matches!(kind, 2 | 3 | 5 | 6);
            let mut t = Tri { kind, v: [d[p + 2] as u16, d[p + 3] as u16, d[p + 4] as u16], col: d[p + 5], uv: [(0, 0); 3] };
            if big {
                for k in 0..3 {
                    t.uv[k] = (d[p + 6 + 2 * k], d[p + 7 + 2 * k]);
                }
                p += 12;
            } else {
                p += 6;
            }
            tris.push(t);
        }
        p = tri_end;
        let mut verts = Vec::with_capacity(nvert);
        for i in 0..nvert {
            let o = p + i * 6;
            verts.push([i16le(d, o), i16le(d, o + 2), i16le(d, o + 4)]);
        }
        p += nvert * 6;
        let shade = d.get(p..p + nvert).map(|s| s.to_vec()).unwrap_or_else(|| vec![64; nvert]);
        tris.retain(|t| (t.v.iter().all(|&v| (v as usize) < nvert)));
        Ok(Model { tris, verts, shade, flags })
    }
}

/// Model file names in load order (index = model number).
pub fn model_files() -> Vec<String> {
    let mut v = Vec::new();
    for c in 0..8 { v.push(format!("CAR0-{c}")); }
    v.push("MARSH0-0".into());
    v.push("SIGN0-0".into());
    for stem in ["TANK", "JUGGA", "JET", "BIKE", "SKIM"] {
        for c in 0..8 { v.push(format!("{stem}0-{c}")); }
    }
    for t in 0..6 { v.push(format!("TRACK0-{t}")); }
    v.push("RECOV0-0".into());
    v.push("CONE0-0".into());
    v.push("CONE0-1".into());
    for b in 0..3 { v.push(format!("BAREL0-{b}")); }
    v
}

pub fn load_all_models() -> Result<Vec<Model>, String> {
    model_files().iter().map(|n| Model::load(&format!("objects/data/{n}.dat"))).collect()
}

/// Model number for craft type and colour.
pub fn craft_model(kind: usize, colour: usize) -> usize {
    let c = colour & 7;
    match kind {
        0 => c,
        1 => 10 + c,
        2 => 18 + c,
        3 => 26 + c,
        4 => 34 + c,
        _ => 42 + c,
    }
}
pub const MODEL_TRACK0: usize = 50;
pub const MODEL_RECOV: usize = 56;
pub const MODEL_CONE0: usize = 57;
pub const MODEL_BAREL0: usize = 59;
