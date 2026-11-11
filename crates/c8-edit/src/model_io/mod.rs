//! Model exchange with Blender and other 3D tools: glTF 2.0 binary (`.glb`, [`glb`]) and Wavefront
//! OBJ + MTL ([`obj`]).
//!
//! **Axes and units.** glTF and OBJ are right-handed with +Y up; the asset faces +Z. A model file
//! has x right, y backwards and z up (spec 01 §8, 08 §8), so a file vertex (x, y, z) is exported
//! as (−x, z, −y) / 256: the craft stands upright, faces +Z (Blender: −Y, its front view), and
//! 256 file units are one metre (a car is about 4 m, a map cell 8 m). Positions are multiples of
//! 1/256, exact in `f32`, and are rounded back to whole units on import.
//!
//! **Winding.** The game's front faces are clockwise on the screen (spec 08 §8), which in a
//! right-handed space is the reverse of glTF's counter-clockwise front faces, so every triangle
//! (a, b, c) is written as (a, c, b) and read back the same way.
//!
//! **Materials** carry the triangle's type and its texture atlas entry or palette colour in the
//! name (`c8_t5_e12`: type 5, atlas entry 12; `c8_f4_c37`: type 4, colour 37) and in glTF
//! `extras` (`c8_kind`, `c8_tex` / `c8_colour`). Every textured material uses the one atlas
//! sheet (`TEX0-0` with the palette applied, a PNG); UVs point at texel centres of the sheet.
//!
//! **Lossless re-import.** The export also embeds the original file (base64, `extras.c8_model`
//! on the node; a `# c8_model` comment in OBJ). On import, when the triangles are the same
//! (same types, colours, entries, texel UVs and quantised positions, in any order and with any
//! starting corner), that file is used as is: byte-identical. When the geometry changed, the
//! original still provides the vertex order, triangle order and shade bytes of everything that
//! is still there; new vertices take the shade of the nearest old vertex.
pub mod glb;
pub mod obj;

use c8_data::images::{ColourTable, Palette, TexAtlas};
use c8_data::model::{is_textured, Model, Tri, MAX_VERTS};
use std::collections::HashMap;

/// File units per metre in the exported scene.
pub const UNITS_PER_METRE: f32 = 256.0;
/// Shade byte of a new vertex without an old neighbour: light row 64 − 128/4 = 32, unchanged.
pub const DEFAULT_SHADE: u8 = 128;

/// Palette, colour table and model texture atlas: what models are drawn with.
pub struct Look {
    /// 8-bit RGB
    pub pal: [[u8; 3]; 256],
    pub table: Option<ColourTable>,
    pub atlas: TexAtlas,
}

impl Look {
    /// From the data folder: the palette, colour table `set` (the level's, 0..5) and `TEX0-0`.
    pub fn load(set: usize) -> Result<Look, String> {
        let pal = Palette::load()?.vga.map(|c| c.map(|v| (v as u32 * 255 / 63) as u8));
        Ok(Look { pal, table: ColourTable::load(set).ok(), atlas: TexAtlas::load()? })
    }
    /// Colour `idx` through light row `row` (32 = unchanged).
    pub fn rgb(&self, idx: u8, row: usize) -> [u8; 3] {
        let i = match &self.table {
            Some(t) => t.data[row.min(63) * 256 + idx as usize],
            None => idx,
        };
        self.pal[i as usize]
    }
    /// The atlas sheet with the palette applied (light row 32, as textured triangles are drawn).
    pub fn atlas_png(&self) -> Result<Vec<u8>, String> {
        let a = &self.atlas;
        let mut rgb = Vec::with_capacity(a.pix.len() * 3);
        for &i in &a.pix {
            rgb.extend_from_slice(&self.rgb(i, 32));
        }
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 256, a.h);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().map_err(|e| e.to_string())?;
            w.write_image_data(&rgb).map_err(|e| e.to_string())?;
        }
        Ok(out)
    }
    /// UV (0..1, origin top-left) of a triangle corner: the texel centre in the atlas sheet.
    pub fn uv(&self, entry: u8, (u, v): (u8, u8)) -> [f32; 2] {
        let (ex, ey, _, _) = self.atlas.entries.get(entry as usize).copied().unwrap_or((0, 0, 1, 1));
        [(ex as f32 + u as f32 + 0.5) / 256.0, (ey as f32 + v as f32 + 0.5) / self.atlas.h as f32]
    }
    /// Palette colour nearest to an sRGB colour (0..1).
    pub fn nearest(&self, c: [f32; 3]) -> u8 {
        let c = c.map(|v| (v.clamp(0.0, 1.0) * 255.0) as i32);
        (0..256).min_by_key(|&i| (0..3).map(|k| (self.pal[i][k] as i32 - c[k]).pow(2)).sum::<i32>()).unwrap_or(0) as u8
    }
}

/// File vertex → exported position (metres, Y up, facing +Z).
pub fn to_scene(v: [i16; 3]) -> [f32; 3] {
    [-(v[0] as f32) / UNITS_PER_METRE, v[2] as f32 / UNITS_PER_METRE, -(v[1] as f32) / UNITS_PER_METRE]
}

/// Exported position → file vertex, rounded to whole units.
pub fn from_scene(p: [f32; 3]) -> Result<[i16; 3], String> {
    let q = |f: f32| -> Result<i16, String> {
        let r = (f * UNITS_PER_METRE).round();
        if !r.is_finite() || r < i16::MIN as f32 || r > i16::MAX as f32 {
            return Err(format!("vertex ({:.3}, {:.3}, {:.3}) m is outside the format's range (±128 m, 16-bit coordinates)", p[0], p[1], p[2]));
        }
        Ok(r as i16)
    };
    Ok([q(-p[0])?, q(-p[2])?, q(p[1])?])
}

pub fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}
pub fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// What a material stands for: a triangle type plus its atlas entry or palette colour.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum MatKey {
    Tex { kind: u8, entry: u8 },
    Flat { kind: u8, col: u8 },
}

impl MatKey {
    pub fn of(t: &Tri) -> MatKey {
        if t.textured() { MatKey::Tex { kind: t.kind, entry: t.col } } else { MatKey::Flat { kind: t.kind, col: t.col } }
    }
    pub fn name(self) -> String {
        match self {
            MatKey::Tex { kind, entry } => format!("c8_t{kind}_e{entry}"),
            MatKey::Flat { kind, col } => format!("c8_f{kind}_c{col}"),
        }
    }
    /// From a material name; Blender's `.001` duplicate suffixes are ignored.
    pub fn parse(name: &str) -> Option<MatKey> {
        let base = name.split('.').next()?;
        let rest = base.strip_prefix("c8_")?;
        let (a, b) = rest.split_once('_')?;
        let (t, kind) = (a.get(..1)?, a.get(1..)?.parse::<u8>().ok()?);
        let (u, val) = (b.get(..1)?, b.get(1..)?.parse::<u8>().ok()?);
        match (t, u) {
            ("t", "e") if is_textured(kind) => Some(MatKey::Tex { kind, entry: val }),
            ("f", "c") if !is_textured(kind) => Some(MatKey::Flat { kind, col: val }),
            _ => None,
        }
    }
    /// From glTF material extras (`c8_kind` plus `c8_tex` or `c8_colour`).
    pub fn from_extras(v: &serde_json::Value) -> Option<MatKey> {
        let kind = u8::try_from(v.get("c8_kind")?.as_u64()?).ok()?;
        if is_textured(kind) {
            Some(MatKey::Tex { kind, entry: u8::try_from(v.get("c8_tex")?.as_u64()?).ok()? })
        } else {
            Some(MatKey::Flat { kind, col: u8::try_from(v.get("c8_colour")?.as_u64()?).ok()? })
        }
    }
    pub fn extras(self) -> serde_json::Value {
        match self {
            MatKey::Tex { kind, entry } => serde_json::json!({ "c8_kind": kind, "c8_tex": entry }),
            MatKey::Flat { kind, col } => serde_json::json!({ "c8_kind": kind, "c8_colour": col }),
        }
    }
    pub fn textured(self) -> bool {
        matches!(self, MatKey::Tex { .. })
    }
}

/// The triangles grouped by material, in order of first use: (material, triangle indices).
pub fn groups(m: &Model) -> Vec<(MatKey, Vec<usize>)> {
    let mut out: Vec<(MatKey, Vec<usize>)> = Vec::new();
    for (i, t) in m.tris.iter().enumerate() {
        let k = MatKey::of(t);
        match out.iter_mut().find(|g| g.0 == k) {
            Some(g) => g.1.push(i),
            None => out.push((k, vec![i])),
        }
    }
    out
}

/// Triangles that Blender would delete if they shared vertices: the same three corner positions
/// as an earlier one (the back of a two-sided face, as on SIGN0-0), or one vertex used twice (five
/// zero-area triangles of MARSH0-0). The exports give them vertices of their own.
pub fn doubled(m: &Model) -> Vec<bool> {
    let mut seen = std::collections::HashSet::new();
    m.tris
        .iter()
        .map(|t| {
            let mut p = t.v.map(|v| m.verts[v as usize]);
            p.sort();
            !seen.insert(p) || t.v[0] == t.v[1] || t.v[1] == t.v[2] || t.v[0] == t.v[2]
        })
        .collect()
}

/// A material as read from an imported file.
#[derive(Clone, Debug, Default)]
pub struct MatIn {
    pub name: String,
    /// from extras or the name
    pub key: Option<MatKey>,
    /// base colour, sRGB 0..1
    pub colour: Option<[f32; 3]>,
    pub textured: bool,
}

/// A triangle as read from an imported file: scene positions, counter-clockwise front face.
#[derive(Clone, Debug)]
pub struct FaceIn {
    pub mat: usize,
    pub pos: [[f32; 3]; 3],
    /// UVs, origin top-left (glTF convention)
    pub uv: Option<[[f32; 2]; 3]>,
}

/// An imported triangle soup.
#[derive(Default, Debug)]
pub struct Soup {
    pub mats: Vec<MatIn>,
    pub faces: Vec<FaceIn>,
    /// the original model file embedded by our export, if it survived
    pub original: Option<Vec<u8>>,
    /// things skipped while reading (non-triangle primitives etc.)
    pub notes: Vec<String>,
    /// the file's own vertex list, if it has one (OBJ): new vertices are numbered in this order
    pub points: Vec<[f32; 3]>,
}

impl Soup {
    pub fn original_model(&self) -> Option<Model> {
        self.original.as_ref().and_then(|d| Model::parse(d).ok())
    }
}

/// Result of an import.
pub struct Imported {
    pub model: Model,
    /// the embedded or given original was used unchanged (byte-identical save)
    pub exact: bool,
    pub notes: Vec<String>,
}

/// A triangle independent of vertex numbering: type, colour/entry and (position, texel UV) of
/// the corners, rotated so the smallest corner comes first (the winding is kept).
type TriKey = (u8, u8, [([i16; 3], (u8, u8)); 3]);

fn tri_key(kind: u8, col: u8, c: [([i16; 3], (u8, u8)); 3]) -> TriKey {
    let r = (0..3).min_by_key(|&k| c[k]).unwrap_or(0);
    (kind, col, [c[r], c[(r + 1) % 3], c[(r + 2) % 3]])
}

fn model_key(m: &Model, t: &Tri) -> TriKey {
    tri_key(t.kind, t.col, std::array::from_fn(|k| (m.verts[t.v[k] as usize], if t.textured() { t.uv[k] } else { (0, 0) })))
}

/// The model's triangles as a sorted list of numbering-independent keys.
pub fn canonical(m: &Model) -> Vec<TriKey> {
    let mut v: Vec<TriKey> = m.tris.iter().map(|t| model_key(m, t)).collect();
    v.sort();
    v
}

/// Pick the atlas entry for texel coordinates without a known entry: one whose rectangle holds
/// the centroid and whose corner every coordinate reaches with 0..255 (the smallest such entry).
fn pick_entry(atlas: &TexAtlas, t: &[(i32, i32); 3]) -> Option<u8> {
    let cx = (t[0].0 + t[1].0 + t[2].0) as f32 / 3.0;
    let cy = (t[0].1 + t[1].1 + t[2].1) as f32 / 3.0;
    let reach = |x: u32, y: u32| t.iter().all(|&(u, v)| (0..=255).contains(&(u - x as i32)) && (0..=255).contains(&(v - y as i32)));
    atlas
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            let (x, y, w, h) = **e;
            reach(x, y) && cx >= x as f32 - 0.5 && cy >= y as f32 - 0.5 && cx <= (x + w) as f32 + 0.5 && cy <= (y + h) as f32 + 0.5
        })
        .min_by_key(|(_, e)| e.2 * e.3)
        .map(|(i, _)| i as u8)
}

/// Convert an imported soup into a model: quantise, map materials and UVs back to triangle types
/// and atlas entries, enforce the format's limits. `like` (else the soup's embedded original)
/// supplies the header, the vertex and triangle order and the shade bytes.
pub fn build(soup: &Soup, look: &Look, like: Option<&Model>) -> Result<Imported, String> {
    let embedded = soup.original_model();
    let original = like.or(embedded.as_ref());
    let mut notes = soup.notes.clone();
    let mut errors = Vec::new();
    let mut fallback = 0usize;
    let mut degenerate = 0usize;
    // (kind, col, corners) in file winding
    let mut tris: Vec<TriKey> = Vec::new();
    let atlas = &look.atlas;
    for (i, f) in soup.faces.iter().enumerate() {
        let mat = soup.mats.get(f.mat).cloned().unwrap_or_default();
        // counter-clockwise (a, b, c) → the game's (a, c, b)
        let order = [0, 2, 1];
        let p = match order.map(|o| from_scene(f.pos[o])) {
            [Ok(a), Ok(b), Ok(c)] => [a, b, c],
            [a, b, c] => {
                errors.push(format!("triangle {i}: {}", [a, b, c].into_iter().find_map(|r| r.err()).unwrap_or_default()));
                continue;
            }
        };
        if p[0] == p[1] || p[1] == p[2] || p[0] == p[2] {
            // kept: the shipped MARSH0-0 has nine of them
            degenerate += 1;
        }
        let key = match mat.key {
            Some(k) => k,
            None => {
                fallback += 1;
                if mat.textured && f.uv.is_some() {
                    MatKey::Tex { kind: 5, entry: 255 }
                } else {
                    MatKey::Flat { kind: 0, col: look.nearest(mat.colour.unwrap_or([0.5; 3])) }
                }
            }
        };
        match key {
            MatKey::Flat { kind, col } => tris.push((kind, col, std::array::from_fn(|k| (p[k], (0, 0))))),
            MatKey::Tex { kind, entry } => {
                let Some(uv) = f.uv else {
                    errors.push(format!("triangle {i} (material {}): textured but has no UVs", mat.name));
                    continue;
                };
                let t: [(i32, i32); 3] = std::array::from_fn(|k| {
                    let [u, v] = uv[order[k]];
                    ((u * 256.0 - 0.5).round() as i32, (v * atlas.h as f32 - 0.5).round() as i32)
                });
                let entry = if (entry as usize) < atlas.entries.len() {
                    entry
                } else if let Some(e) = pick_entry(atlas, &t) {
                    e
                } else {
                    errors.push(format!("triangle {i} (material {}): its UVs ({:?}) are not inside one texture of the atlas", mat.name, t));
                    continue;
                };
                let (ex, ey, _, _) = atlas.entries[entry as usize];
                let mut rel = [(0u8, 0u8); 3];
                let mut ok = true;
                for k in 0..3 {
                    let (u, v) = (t[k].0 - ex as i32, t[k].1 - ey as i32);
                    if !(0..=255).contains(&u) || !(0..=255).contains(&v) {
                        ok = false;
                    }
                    rel[k] = (u.clamp(0, 255) as u8, v.clamp(0, 255) as u8);
                }
                if !ok {
                    errors.push(format!("triangle {i} (material {}): a UV is more than 255 texels right of / below atlas entry {entry}'s corner, or left of / above it", mat.name));
                    continue;
                }
                tris.push((kind, entry, std::array::from_fn(|k| (p[k], rel[k]))));
            }
        }
    }
    if !errors.is_empty() {
        let n = errors.len();
        errors.truncate(8);
        if n > 8 {
            errors.push(format!("… and {} more", n - 8));
        }
        return Err(errors.join("\n"));
    }
    if fallback > 0 {
        notes.push(format!("{fallback} triangles have materials without a c8_ name or extras: textured ones became type 5 (entry from the UVs), the others flat type 0 in the nearest palette colour"));
    }
    if degenerate > 0 {
        notes.push(format!("{degenerate} triangles have no area (two corners at the same point after rounding to file units); kept"));
    }
    if tris.is_empty() {
        return Err("no triangles to import".into());
    }

    let keys: Vec<TriKey> = tris.iter().map(|&(k, c, p)| tri_key(k, c, p)).collect();
    if let Some(o) = original {
        let mut sorted = keys.clone();
        sorted.sort();
        if sorted == canonical(o) {
            notes.push("same triangles as the original: kept its file unchanged".into());
            return Ok(Imported { model: o.clone(), exact: true, notes });
        }
    }

    // vertices: the original's order for the positions still used, then new ones in order of use
    let used: std::collections::HashSet<[i16; 3]> = tris.iter().flat_map(|t| t.2.iter().map(|c| c.0)).collect();
    let mut verts: Vec<[i16; 3]> = Vec::new();
    let mut index: HashMap<[i16; 3], u16> = HashMap::new();
    let mut add = |v: [i16; 3], verts: &mut Vec<[i16; 3]>| {
        index.entry(v).or_insert_with(|| {
            verts.push(v);
            verts.len() as u16 - 1
        });
    };
    if let Some(o) = original {
        for &v in &o.verts {
            if used.contains(&v) {
                add(v, &mut verts);
            }
        }
    }
    for p in soup.points.iter().filter_map(|p| from_scene(*p).ok()) {
        if used.contains(&p) {
            add(p, &mut verts);
        }
    }
    for t in &tris {
        for c in &t.2 {
            add(c.0, &mut verts);
        }
    }
    if verts.len() > MAX_VERTS {
        return Err(format!("{} distinct vertices: triangles address vertices with one byte, so a model can have at most {MAX_VERTS}; simplify the mesh", verts.len()));
    }
    if tris.len() > u16::MAX as usize {
        return Err(format!("{} triangles, at most {} fit the file", tris.len(), u16::MAX));
    }
    let shade: Vec<u8> = verts
        .iter()
        .map(|v| match original {
            Some(o) if !o.verts.is_empty() => {
                let d2 = |a: &[i16; 3]| (0..3).map(|k| (a[k] as i64 - v[k] as i64).pow(2)).sum::<i64>();
                let (i, _) = o.verts.iter().enumerate().min_by_key(|(_, a)| d2(a)).unwrap();
                o.shade.get(i).copied().unwrap_or(DEFAULT_SHADE)
            }
            _ => DEFAULT_SHADE,
        })
        .collect();
    let idx = |p: [i16; 3]| index[&p];

    // triangles: the original's that are still there, in its order and with its first corner,
    // then the new ones in import order
    let mut pool: HashMap<TriKey, Vec<usize>> = HashMap::new();
    for (i, k) in keys.iter().enumerate().rev() {
        pool.entry(*k).or_default().push(i);
    }
    let mut taken = vec![false; tris.len()];
    let mut out = Vec::with_capacity(tris.len());
    let mut kept = 0;
    if let Some(o) = original {
        for t in &o.tris {
            if let Some(i) = pool.get_mut(&model_key(o, t)).and_then(|v| v.pop()) {
                taken[i] = true;
                kept += 1;
                let mut n = *t;
                n.v = t.v.map(|v| idx(o.verts[v as usize]));
                out.push(n);
            }
        }
    }
    for (i, &(kind, col, c)) in tris.iter().enumerate() {
        if !taken[i] {
            out.push(Tri { kind, pad: 0, v: c.map(|c| idx(c.0)), col, uv: if is_textured(kind) { c.map(|c| c.1) } else { [(0, 0); 3] } });
        }
    }
    let mut model = Model { tris: out, verts, shade, ..Model::default() };
    if let Some(o) = original {
        model.flags = o.flags & !2;
        model.header = o.header;
        if !o.more_frames.is_empty() {
            model.header[0x25] = 1;
            notes.push(format!("the original's {} extra animation frames were dropped (the vertices changed)", o.more_frames.len()));
        }
        notes.push(format!("geometry changed: {} of {} triangles and {} of {} vertices match the original", kept, model.tris.len(), model.verts.iter().filter(|v| o.verts.contains(v)).count(), model.verts.len()));
    }
    let problems = model.check();
    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }
    Ok(Imported { model, exact: false, notes })
}

/// Import a `.glb`, `.gltf` (embedded buffers) or `.obj` file by its extension.
pub fn import_file(path: &std::path::Path, look: &Look, like: Option<&Model>) -> Result<Imported, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let soup = match ext.as_str() {
        "glb" | "gltf" => glb::read(&data)?,
        "obj" => {
            let text = String::from_utf8_lossy(&data);
            let mtl = obj::mtl_next_to(path, &text);
            obj::read(&text, mtl.as_deref())?
        }
        _ => return Err(format!("{}: unknown model format (use .glb, .gltf or .obj)", path.display())),
    };
    build(&soup, look, like)
}

/// Export to `.glb` or `.obj` (+ `.mtl` and the atlas `.png` next to it) by the extension.
/// Returns the written files.
pub fn export_file(m: &Model, look: &Look, name: &str, path: &std::path::Path) -> Result<Vec<std::path::PathBuf>, String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let write = |p: &std::path::Path, d: &[u8]| std::fs::write(p, d).map_err(|e| format!("{}: {e}", p.display()));
    match ext.as_str() {
        "glb" => {
            write(path, &glb::write(m, look, name)?)?;
            Ok(vec![path.to_path_buf()])
        }
        "obj" => {
            let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| name.into());
            let (mtl_p, png_p) = (path.with_extension("mtl"), path.with_file_name(format!("{stem}_tex.png")));
            let png_name = png_p.file_name().unwrap().to_string_lossy().to_string();
            let (o, mtl) = obj::write(m, look, name, &format!("{stem}.mtl"), &png_name)?;
            write(path, o.as_bytes())?;
            write(&mtl_p, mtl.as_bytes())?;
            write(&png_p, &look.atlas_png()?)?;
            Ok(vec![path.to_path_buf(), mtl_p, png_p])
        }
        _ => Err(format!("{}: unknown model format (use .glb or .obj)", path.display())),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;

    pub fn resources() -> Option<PathBuf> {
        let p = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources"));
        p.join("OBJECTS/DATA").is_dir().then(|| {
            c8_data::files::set_data_dir(p.clone());
            p
        })
    }
    pub fn model(name: &str) -> Option<(Vec<u8>, Model)> {
        let raw = std::fs::read(resources()?.join(format!("OBJECTS/DATA/{name}.DAT"))).ok()?;
        let m = Model::parse(&raw).ok()?;
        Some((raw, m))
    }

    #[test]
    fn axes_and_names() {
        let v = [1000i16, -512, 256];
        let p = to_scene(v);
        assert_eq!(p, [-1000.0 / 256.0, 1.0, 2.0]);
        assert_eq!(from_scene(p).unwrap(), v);
        assert!(from_scene([200.0, 0.0, 0.0]).is_err());
        for k in [MatKey::Tex { kind: 5, entry: 12 }, MatKey::Flat { kind: 4, col: 255 }, MatKey::Tex { kind: 2, entry: 0 }] {
            assert_eq!(MatKey::parse(&k.name()), Some(k));
            assert_eq!(MatKey::parse(&format!("{}.003", k.name())), Some(k));
            assert_eq!(MatKey::from_extras(&k.extras()), Some(k));
        }
        assert_eq!(MatKey::parse("c8_t4_e1"), None);
        assert_eq!(MatKey::parse("Material"), None);
    }

    #[test]
    fn shipped_models_face_outwards_after_the_winding_flip() {
        // file triangles are clockwise-front; exported counter-clockwise, their normals must
        // point away from the model's centre for most faces
        let Some((_, m)) = model("CAR0-0") else { return };
        let c = m.verts.iter().fold([0f32; 3], |a, v| {
            let p = to_scene(*v);
            [a[0] + p[0], a[1] + p[1], a[2] + p[2]]
        });
        let c = c.map(|x| x / m.verts.len() as f32);
        let mut out = 0;
        for t in &m.tris {
            let [a, b, cc] = [t.v[0], t.v[2], t.v[1]].map(|i| to_scene(m.verts[i as usize]));
            let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [cc[0] - a[0], cc[1] - a[1], cc[2] - a[2]]);
            let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            let mid = [(a[0] + b[0] + cc[0]) / 3.0 - c[0], (a[1] + b[1] + cc[1]) / 3.0 - c[1], (a[2] + b[2] + cc[2]) / 3.0 - c[2]];
            if n[0] * mid[0] + n[1] * mid[1] + n[2] * mid[2] > 0.0 {
                out += 1;
            }
        }
        assert!(out * 10 > m.tris.len() * 8, "{out} of {} face outwards", m.tris.len());
    }

    /// Blender in the loop: every shipped model goes .glb → Blender → .glb → model, once with
    /// Blender's default import and the custom properties exported (byte-exact expected), once
    /// with "Merge Vertices" and without custom properties (same triangles expected), and
    /// .obj → Blender → .obj → model. Needs `blender` on the PATH (or C8_BLENDER); run with
    /// `cargo test -p c8-edit blender -- --ignored`.
    #[test]
    #[ignore]
    fn blender_round_trip() {
        let Some(dir) = resources() else { return };
        let blender = std::env::var("C8_BLENDER").unwrap_or_else(|_| "blender".into());
        let tmp = std::env::temp_dir().join(format!("c8-blender-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let script = tmp.join("rt.py");
        std::fs::write(&script, r#"
import bpy, sys
a = sys.argv[sys.argv.index("--") + 1:]
for i in range(0, len(a), 3):
    src, dst, keep = a[i], a[i + 1], a[i + 2] == "1"
    bpy.ops.wm.read_factory_settings(use_empty=True)
    if src.endswith(".obj"):
        bpy.ops.wm.obj_import(filepath=src)
        bpy.ops.wm.obj_export(filepath=dst, export_materials=True)
    else:
        bpy.ops.import_scene.gltf(filepath=src, merge_vertices=not keep)
        bpy.ops.export_scene.gltf(filepath=dst, export_format='GLB', export_extras=keep)
"#).unwrap();
        let look = Look::load(0).unwrap();
        let names = c8_data::model::model_files();
        let mut args = vec!["-b".to_string(), "--factory-startup".into(), "--python".into(), script.display().to_string(), "--".into()];
        let mut models = Vec::new();
        for name in &names {
            let raw = std::fs::read(dir.join(format!("OBJECTS/DATA/{name}.DAT"))).unwrap();
            let m = Model::parse(&raw).unwrap();
            export_file(&m, &look, name, &tmp.join(format!("{name}.glb"))).unwrap();
            export_file(&m, &look, name, &tmp.join(format!("{name}.obj"))).unwrap();
            for (src, dst, extras) in [("glb", "keep.glb", "1"), ("glb", "plain.glb", "0"), ("obj", "bl.obj", "0")] {
                args.push(tmp.join(format!("{name}.{src}")).display().to_string());
                args.push(tmp.join(format!("{name}.{dst}")).display().to_string());
                args.push(extras.into());
            }
            models.push((name.clone(), raw, m));
        }
        let st = match std::process::Command::new(&blender).args(&args).output() {
            Ok(o) => o,
            Err(e) => {
                eprintln!("no blender ({e}); skipped");
                return;
            }
        };
        assert!(st.status.success(), "{}", String::from_utf8_lossy(&st.stderr));
        // with merged vertices Blender drops zero-area triangles (MARSH0-0 has nine) and the back
        // of a two-sided face (SIGN0-0's flag); the exports keep them apart otherwise
        let flat = |k: &TriKey| k.2[0].0 == k.2[1].0 || k.2[1].0 == k.2[2].0 || k.2[0].0 == k.2[2].0;
        let corners = |k: &TriKey| {
            let mut p = k.2.map(|c| c.0);
            p.sort();
            p
        };
        // merged: leave out two-sided faces (both sides, of which Blender keeps one)
        let real = |m: &Model, orig: &Model, merged: bool| {
            let mut count = HashMap::new();
            for k in canonical(orig) {
                *count.entry(corners(&k)).or_insert(0) += 1;
            }
            let mut c: Vec<_> = canonical(m).into_iter().filter(|k| !(merged && (flat(k) || count.get(&corners(k)).is_some_and(|&n| n > 1)))).collect();
            c.sort();
            c
        };
        for (name, raw, m) in &models {
            let keep = import_file(&tmp.join(format!("{name}.keep.glb")), &look, None).unwrap();
            let plain = import_file(&tmp.join(format!("{name}.plain.glb")), &look, None).unwrap();
            let obj = import_file(&tmp.join(format!("{name}.bl.obj")), &look, None).unwrap();
            assert!(keep.exact, "{name}");
            assert_eq!(&keep.model.to_bytes().unwrap(), raw, "{name}");
            // with the original as a template the OBJ through Blender is exact too
            assert!(import_file(&tmp.join(format!("{name}.bl.obj")), &look, Some(m)).unwrap().exact, "{name}");
            assert_eq!(real(&keep.model, m, false), real(m, m, false), "{name} (glb, custom properties)");
            assert_eq!(real(&plain.model, m, true), real(m, m, true), "{name} (glb, merged, no custom properties)");
            assert_eq!(real(&obj.model, m, false), real(m, m, false), "{name} (obj)");
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
