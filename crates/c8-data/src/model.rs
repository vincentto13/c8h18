//! "BULLFROG OBJECT DATA" 3D models (spec 01 §8): reader and writer.
//!
//! [`Model::to_bytes`] writes the file back byte for byte: the header bytes the game never reads
//! (the 16 run-time pointer bytes at 0x2a, the zero u32 at 0x14), the unused byte 1 of every
//! triangle, the frame table and any further animation frames are kept as read. Only files the
//! reader repairs do not round-trip: triangles with an out-of-range vertex index are dropped on
//! load (none in the shipped files).
use super::files::{i16le, read_raw, u16le, u32le};

pub const SIGNATURE: &[u8; 20] = b"BULLFROG OBJECT DATA";
/// Header size; the frame table (if any) and the triangle block follow.
pub const HEADER: usize = 0x3a;
/// The triangle's vertex indices are bytes, so a model can use at most 256 vertices (the loader's
/// own limit is "< 500").
pub const MAX_VERTS: usize = 256;
/// Texture atlas entries in `TEX0-0.TAB` (spec 01 §7).
pub const ATLAS_ENTRIES: usize = 51;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Tri {
    pub kind: u8,
    /// byte 1, unused (0 in every shipped file)
    pub pad: u8,
    pub v: [u16; 3],
    /// flat colour or texture atlas entry
    pub col: u8,
    pub uv: [(u8, u8); 3],
}

impl Tri {
    /// Types 2, 3, 5, 6 are textured 12-byte records; the rest are 6-byte flat records.
    pub fn textured(&self) -> bool {
        is_textured(self.kind)
    }
}

pub fn is_textured(kind: u8) -> bool {
    matches!(kind, 2 | 3 | 5 | 6)
}

/// What a triangle type draws (spec 01 §8).
pub fn kind_name(kind: u8) -> &'static str {
    match kind {
        0 => "flat, unlit",
        4 => "flat, vertex shade",
        2 | 3 => "textured, unlit",
        5 | 6 => "textured, light row 32",
        _ => "unknown (drawn flat)",
    }
}

#[derive(Clone, Debug)]
pub struct Model {
    pub tris: Vec<Tri>,
    /// file coordinates (x, y, z), 1/2048 cell; frame 0
    pub verts: Vec<[i16; 3]>,
    /// per-vertex shade bytes of frame 0
    pub shade: Vec<u8>,
    pub flags: u8,
    /// the header as read; the size and count fields (0x18..0x2a) are recomputed on write
    pub header: [u8; HEADER],
    /// frame table (flags bit 1 only), as read
    pub frame_table: Vec<u8>,
    /// vertex and shade blocks of frames 1.. (flags bit 1; no shipped file has more than one frame)
    pub more_frames: Vec<(Vec<[i16; 3]>, Vec<u8>)>,
    /// bytes after the last frame (none in the shipped files)
    pub trailing: Vec<u8>,
}

impl Default for Model {
    fn default() -> Self {
        Model { tris: Vec::new(), verts: Vec::new(), shade: Vec::new(), flags: 5, header: default_header(), frame_table: Vec::new(), more_frames: Vec::new(), trailing: Vec::new() }
    }
}

/// Equal content: the header's size and count fields (recomputed on write) are not compared.
impl PartialEq for Model {
    fn eq(&self, o: &Model) -> bool {
        let fixed = |h: &[u8; HEADER]| {
            let mut h = *h;
            h[0x18..0x24].fill(0);
            h[0x26..0x2a].fill(0);
            h
        };
        self.tris == o.tris && self.verts == o.verts && self.shade == o.shade && self.flags == o.flags && fixed(&self.header) == fixed(&o.header) && self.frame_table == o.frame_table && self.more_frames == o.more_frames && self.trailing == o.trailing
    }
}
impl Eq for Model {}

/// The header of a new model: signature, flags 5, one frame, and the 16 run-time bytes as the
/// shipped files have them (with n = 2, the cone's value; the game never reads it).
pub fn default_header() -> [u8; HEADER] {
    let mut h = [0u8; HEADER];
    h[..20].copy_from_slice(SIGNATURE);
    h[0x24] = 5;
    h[0x25] = 1;
    h[0x2a..0x3a].copy_from_slice(&[0x4f, 0x00, 2, 0, 0, 0, 0x7f, 0x1f, 0x00, 0x00, 0x7f, 0x12, 0x00, 0x00, 0xd9, 0xdd]);
    h
}

impl Model {
    pub fn load(rel: &str) -> Result<Model, String> {
        let d = read_raw(rel)?;
        Self::parse(&d).map_err(|e| format!("{rel}: {e}"))
    }

    pub fn parse(d: &[u8]) -> Result<Model, String> {
        if d.len() < HEADER || !d[..36].windows(20).any(|w| w == SIGNATURE) {
            return Err("not an object file".into());
        }
        let short = |what: &str| format!("file too short for its {what}");
        let tri_size = u32le(d, 0x18) as usize;
        let flags = d[0x24];
        let frames = d[0x25].max(1) as usize;
        let ntri = u16le(d, 0x26) as usize;
        let nvert = u16le(d, 0x28) as usize;
        let mut header = [0u8; HEADER];
        header.copy_from_slice(&d[..HEADER]);
        let mut p = HEADER;
        let mut frame_table = Vec::new();
        if flags & 2 != 0 {
            frame_table = d.get(p..p + frames * 8).ok_or_else(|| short("frame table"))?.to_vec();
            p += frames * 8;
        }
        let tri_end = p + tri_size;
        if tri_end > d.len() {
            return Err(short("triangles"));
        }
        let mut tris = Vec::with_capacity(ntri);
        while p < tri_end && tris.len() < ntri {
            let kind = d[p];
            let len = if is_textured(kind) { 12 } else { 6 };
            let r = d.get(p..p + len).filter(|_| p + len <= tri_end).ok_or_else(|| short("triangles"))?;
            let mut t = Tri { kind, pad: r[1], v: [r[2] as u16, r[3] as u16, r[4] as u16], col: r[5], uv: [(0, 0); 3] };
            if len == 12 {
                for k in 0..3 {
                    t.uv[k] = (r[6 + 2 * k], r[7 + 2 * k]);
                }
            }
            p += len;
            tris.push(t);
        }
        p = tri_end;
        let read_frame = |p: &mut usize, full: bool| -> Result<(Vec<[i16; 3]>, Vec<u8>), String> {
            let vb = d.get(*p..*p + nvert * 6).ok_or_else(|| short("vertices"))?;
            let verts = (0..nvert).map(|i| [i16le(vb, i * 6), i16le(vb, i * 6 + 2), i16le(vb, i * 6 + 4)]).collect();
            *p += nvert * 6;
            let shade = match d.get(*p..*p + nvert) {
                Some(s) => s.to_vec(),
                None if !full => vec![64; nvert],
                None => return Err(short("shade bytes")),
            };
            *p = (*p + nvert).min(d.len());
            Ok((verts, shade))
        };
        // frame 0 tolerates a missing shade block (as the game's reader always did)
        let (verts, shade) = read_frame(&mut p, false)?;
        let mut more_frames = Vec::new();
        if flags & 2 != 0 {
            for _ in 1..frames {
                more_frames.push(read_frame(&mut p, true)?);
            }
        }
        let trailing = d[p.min(d.len())..].to_vec();
        tris.retain(|t| t.v.iter().all(|&v| (v as usize) < nvert));
        Ok(Model { tris, verts, shade, flags, header, frame_table, more_frames, trailing })
    }

    /// Problems that keep the model from being written or loaded by the game.
    pub fn check(&self) -> Vec<String> {
        let mut out = Vec::new();
        let n = self.verts.len();
        if n > MAX_VERTS {
            out.push(format!("{n} vertices: a triangle addresses its vertices with one byte, so at most {MAX_VERTS} are possible"));
        }
        if self.tris.len() > u16::MAX as usize {
            out.push(format!("{} triangles, at most {} fit the triangle count", self.tris.len(), u16::MAX));
        }
        if self.shade.len() != n {
            out.push(format!("{} shade bytes for {n} vertices", self.shade.len()));
        }
        for (f, (v, s)) in self.more_frames.iter().enumerate() {
            if v.len() != n || s.len() != n {
                out.push(format!("frame {}: {} vertices and {} shade bytes, frame 0 has {n}", f + 1, v.len(), s.len()));
            }
        }
        if self.more_frames.len() + 1 > 255 {
            out.push("more than 255 frames".into());
        }
        if let Some((i, t)) = self.tris.iter().enumerate().find(|(_, t)| t.v.iter().any(|&v| v as usize >= n || v > 255)) {
            out.push(format!("triangle {i} uses vertex {} of {n}", t.v.iter().max().unwrap()));
        }
        out
    }

    /// The file, byte for byte as the game reads it.
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        let problems = self.check();
        if !problems.is_empty() {
            return Err(problems.join("; "));
        }
        let mut tri = Vec::new();
        for t in &self.tris {
            tri.extend_from_slice(&[t.kind, t.pad, t.v[0] as u8, t.v[1] as u8, t.v[2] as u8, t.col]);
            if t.textured() {
                for (u, v) in t.uv {
                    tri.extend_from_slice(&[u, v]);
                }
            }
        }
        let n = self.verts.len();
        let frames = 1 + self.more_frames.len();
        let mut d = self.header.to_vec();
        if !d[..36].windows(20).any(|w| w == SIGNATURE) {
            d[..20].copy_from_slice(SIGNATURE);
        }
        d[0x18..0x1c].copy_from_slice(&(tri.len() as u32).to_le_bytes());
        d[0x1c..0x20].copy_from_slice(&((n * 6) as u32).to_le_bytes());
        d[0x20..0x24].copy_from_slice(&(n as u32).to_le_bytes());
        d[0x24] = self.flags;
        // a single-frame file keeps its stored count (0 reads as 1)
        if frames > 1 || d[0x25] > 1 {
            d[0x25] = frames as u8;
        }
        d[0x26..0x28].copy_from_slice(&(self.tris.len() as u16).to_le_bytes());
        d[0x28..0x2a].copy_from_slice(&(n as u16).to_le_bytes());
        if self.flags & 2 != 0 {
            let mut ft = self.frame_table.clone();
            ft.resize(d[0x25].max(1) as usize * 8, 0);
            d.extend_from_slice(&ft);
        }
        d.extend_from_slice(&tri);
        let mut put = |v: &[[i16; 3]], s: &[u8]| {
            for p in v {
                for c in p {
                    d.extend_from_slice(&c.to_le_bytes());
                }
            }
            d.extend_from_slice(s);
        };
        put(&self.verts, &self.shade);
        for (v, s) in &self.more_frames {
            put(v, s);
        }
        d.extend_from_slice(&self.trailing);
        Ok(d)
    }

    /// Bounding box of frame 0 in file units: (min, max).
    pub fn bounds(&self) -> ([i16; 3], [i16; 3]) {
        if self.verts.is_empty() {
            return ([0; 3], [0; 3]);
        }
        let mut lo = [i16::MAX; 3];
        let mut hi = [i16::MIN; 3];
        for v in &self.verts {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
        (lo, hi)
    }

    /// The header's n at 0x2c (meaning unknown, never read by the game).
    pub fn header_n(&self) -> u32 {
        u32le(&self.header, 0x2c)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn objects() -> Option<std::path::PathBuf> {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/OBJECTS/DATA");
        p.is_dir().then_some(p)
    }

    #[test]
    fn every_shipped_model_round_trips_byte_exact() {
        let Some(dir) = objects() else { return };
        let mut n = 0;
        for name in model_files() {
            let raw = std::fs::read(dir.join(format!("{name}.DAT"))).unwrap();
            let m = Model::parse(&raw).unwrap();
            assert_eq!(m.to_bytes().unwrap(), raw, "{name}");
            n += 1;
        }
        assert_eq!(n, 62);
    }

    /// The replacement models and atlas in `assets/models` (made by the local modelgen tool): all 62
    /// model files are there (crafts, props, recovery truck, track previews), every one passes
    /// `check`, round-trips byte-exact and keeps its UVs inside its atlas entries. A model missing
    /// from the folder would fall back to the original, so it must fit the new atlas too.
    #[test]
    fn generated_models_are_valid() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/models/OBJECTS/DATA");
        let Ok(tab) = std::fs::read(dir.join("TEX0-0.TAB")) else { return };
        let entries: Vec<(u32, u32)> = tab.chunks(6).map(|e| (e[4] as u32, e[5] as u32)).collect();
        assert_eq!(entries.len(), ATLAS_ENTRIES);
        assert_eq!(std::fs::read(dir.join("TEX0-0.DAT")).unwrap().len(), 256 * 768);
        let fits = |m: &Model, name: &str| {
            for t in m.tris.iter().filter(|t| t.textured()) {
                let (w, h) = entries[t.col as usize];
                assert!(t.uv.iter().all(|&(u, v)| (u as u32) < w && (v as u32) < h), "{name}: UVs {:?} outside entry {} ({w} x {h})", t.uv, t.col);
            }
        };
        let mut n = 0;
        for name in model_files() {
            let Ok(raw) = std::fs::read(dir.join(format!("{name}.DAT"))) else {
                if let Some(o) = objects() {
                    fits(&Model::parse(&std::fs::read(o.join(format!("{name}.DAT"))).unwrap()).unwrap(), &name);
                }
                continue;
            };
            let m = Model::parse(&raw).unwrap();
            assert!(m.check().is_empty(), "{name}: {:?}", m.check());
            assert_eq!(m.to_bytes().unwrap(), raw, "{name}");
            fits(&m, &name);
            n += 1;
        }
        assert_eq!(n, model_files().len(), "models in assets/models");
    }

    #[test]
    fn writer_recomputes_counts_and_keeps_extra_frames() {
        let mut m = Model { verts: vec![[0, 0, 0], [100, 0, 0], [0, 100, 0], [0, 0, 100]], shade: vec![10, 20, 30, 40], ..Default::default() };
        m.tris.push(Tri { kind: 5, v: [0, 1, 2], col: 3, uv: [(0, 0), (10, 0), (0, 10)], ..Default::default() });
        m.tris.push(Tri { kind: 0, v: [0, 2, 3], col: 77, ..Default::default() });
        let d = m.to_bytes().unwrap();
        assert_eq!(d.len(), HEADER + 12 + 6 + 4 * 7);
        let back = Model::parse(&d).unwrap();
        assert_eq!(back, m);
        // two frames
        m.flags |= 2;
        m.more_frames.push((vec![[1, 2, 3]; 4], vec![1; 4]));
        let d = m.to_bytes().unwrap();
        let back = Model::parse(&d).unwrap();
        assert_eq!(back.more_frames, m.more_frames);
        assert_eq!(back.frame_table.len(), 16);
        assert_eq!(back.to_bytes().unwrap(), d);
    }

    #[test]
    fn limits_are_reported() {
        let mut m = Model { verts: vec![[0; 3]; 300], shade: vec![0; 300], ..Default::default() };
        m.tris.push(Tri { kind: 0, v: [0, 1, 299], ..Default::default() });
        let e = m.to_bytes().unwrap_err();
        assert!(e.contains("256"), "{e}");
        assert!(Model::parse(b"BULLFROG OBJECT DATA").is_err());
        let mut short = Model::default().to_bytes().unwrap();
        short[0x18] = 50; // triangle block beyond the end
        assert!(Model::parse(&short).is_err());
    }
}
