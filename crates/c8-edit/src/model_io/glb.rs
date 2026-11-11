//! glTF 2.0 binary (`.glb`) export and import. See the module above for axes, winding and the
//! material naming.
//!
//! Export: one node and mesh named after the model, one primitive per material, positions and
//! (textured materials) UVs with u16 indices, unlit materials (`KHR_materials_unlit`) with the
//! palette colour or the atlas sheet as an embedded PNG sampled with nearest filtering. Vertices
//! are shared inside a primitive where position and UV agree. Import reads any glTF with
//! triangle primitives (node transforms applied), `.glb` or `.gltf` with embedded buffers.
use super::{groups, to_scene, FaceIn, Look, MatIn, MatKey, Soup};
use base64::Engine;
use c8_data::model::Model;
use serde_json::{json, Value};

const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const FLOAT: u32 = 5126;
const UNSIGNED_SHORT: u32 = 5123;
const NEAREST: u32 = 9728;
const CLAMP_TO_EDGE: u32 = 33071;

struct Bin {
    data: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Bin {
    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        while !self.data.len().is_multiple_of(4) {
            self.data.push(0);
        }
        let mut v = json!({ "buffer": 0, "byteOffset": self.data.len(), "byteLength": bytes.len() });
        if let Some(t) = target {
            v["target"] = json!(t);
        }
        self.data.extend_from_slice(bytes);
        self.views.push(v);
        self.views.len() - 1
    }
    fn accessor(&mut self, view: usize, component: u32, count: usize, ty: &str, minmax: Option<(Vec<f32>, Vec<f32>)>) -> usize {
        let mut a = json!({ "bufferView": view, "componentType": component, "count": count, "type": ty });
        if let Some((lo, hi)) = minmax {
            a["min"] = json!(lo);
            a["max"] = json!(hi);
        }
        self.accessors.push(a);
        self.accessors.len() - 1
    }
}

/// The model as a `.glb` file (`name`: the node, mesh and scene name, e.g. `CAR0-0`).
pub fn write(m: &Model, look: &Look, name: &str) -> Result<Vec<u8>, String> {
    let original = m.to_bytes()?;
    let mut bin = Bin { data: Vec::new(), views: Vec::new(), accessors: Vec::new() };
    let mut materials = Vec::new();
    let mut prims = Vec::new();
    let mut any_tex = false;
    let doubled = super::doubled(m);
    for (key, tris) in groups(m) {
        // vertices (file vertex, texel uv), shared except by doubled triangles
        let mut corners: Vec<(u16, (u8, u8))> = Vec::new();
        let mut shared: std::collections::HashMap<(u16, (u8, u8)), u16> = Default::default();
        let mut indices: Vec<u16> = Vec::new();
        for &ti in &tris {
            let t = &m.tris[ti];
            // the game's clockwise (a, b, c) → counter-clockwise (a, c, b)
            for k in [0, 2, 1] {
                let c = (t.v[k], if key.textured() { t.uv[k] } else { (0, 0) });
                let fresh = |corners: &mut Vec<_>| {
                    corners.push(c);
                    (corners.len() - 1) as u16
                };
                let i = if doubled[ti] { fresh(&mut corners) } else { *shared.entry(c).or_insert_with(|| fresh(&mut corners)) };
                indices.push(i);
            }
        }
        let pos: Vec<[f32; 3]> = corners.iter().map(|c| to_scene(m.verts[c.0 as usize])).collect();
        let mut lo = vec![f32::MAX; 3];
        let mut hi = vec![f32::MIN; 3];
        for p in &pos {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let pv = bin.view(bytemuck::cast_slice(&pos), Some(ARRAY_BUFFER));
        let pa = bin.accessor(pv, FLOAT, pos.len(), "VEC3", Some((lo, hi)));
        let mut attrs = json!({ "POSITION": pa });
        let mut mat = json!({ "name": key.name(), "extras": key.extras(), "extensions": { "KHR_materials_unlit": {} } });
        match key {
            MatKey::Tex { entry, .. } => {
                any_tex = true;
                let uv: Vec<[f32; 2]> = corners.iter().map(|c| look.uv(entry, c.1)).collect();
                let uvv = bin.view(bytemuck::cast_slice(&uv), Some(ARRAY_BUFFER));
                attrs["TEXCOORD_0"] = json!(bin.accessor(uvv, FLOAT, uv.len(), "VEC2", None));
                mat["pbrMetallicRoughness"] = json!({ "baseColorTexture": { "index": 0 }, "metallicFactor": 0.0, "roughnessFactor": 1.0 });
            }
            MatKey::Flat { col, .. } => {
                let c = look.rgb(col, 32).map(super::srgb_to_linear);
                mat["pbrMetallicRoughness"] = json!({ "baseColorFactor": [c[0], c[1], c[2], 1.0], "metallicFactor": 0.0, "roughnessFactor": 1.0 });
            }
        }
        let iv = bin.view(bytemuck::cast_slice(&indices), Some(ELEMENT_ARRAY_BUFFER));
        let ia = bin.accessor(iv, UNSIGNED_SHORT, indices.len(), "SCALAR", None);
        materials.push(mat);
        prims.push(json!({ "attributes": attrs, "indices": ia, "material": materials.len() - 1, "mode": 4 }));
    }
    let mut root = json!({
        "asset": { "version": "2.0", "generator": "c8h18-edit" },
        "extensionsUsed": ["KHR_materials_unlit"],
        "scene": 0,
        "scenes": [{ "name": name, "nodes": [0] }],
        "nodes": [{ "name": name, "mesh": 0, "extras": {
            "c8_model": base64::engine::general_purpose::STANDARD.encode(&original),
            "c8_file": format!("{name}.DAT"),
        } }],
        "meshes": [{ "name": name, "primitives": prims }],
        "materials": materials,
    });
    if any_tex {
        let png = look.atlas_png()?;
        let v = bin.view(&png, None);
        root["images"] = json!([{ "name": "TEX0-0", "bufferView": v, "mimeType": "image/png" }]);
        root["samplers"] = json!([{ "magFilter": NEAREST, "minFilter": NEAREST, "wrapS": CLAMP_TO_EDGE, "wrapT": CLAMP_TO_EDGE }]);
        root["textures"] = json!([{ "sampler": 0, "source": 0, "name": "TEX0-0" }]);
    }
    while !bin.data.len().is_multiple_of(4) {
        bin.data.push(0);
    }
    root["bufferViews"] = Value::Array(bin.views);
    root["accessors"] = Value::Array(bin.accessors);
    root["buffers"] = json!([{ "byteLength": bin.data.len() }]);
    let mut js = serde_json::to_vec(&root).map_err(|e| e.to_string())?;
    while !js.len().is_multiple_of(4) {
        js.push(b' ');
    }
    let total = 12 + 8 + js.len() + 8 + bin.data.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(js.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&js);
    out.extend_from_slice(&(bin.data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin.data);
    Ok(out)
}

type M4 = [[f32; 4]; 4];

fn mul(a: &M4, b: &M4) -> M4 {
    // column-major: r[c][r] = sum_k a[k][r] * b[c][k]
    let mut r = [[0.0; 4]; 4];
    for c in 0..4 {
        for row in 0..4 {
            r[c][row] = (0..4).map(|k| a[k][row] * b[c][k]).sum();
        }
    }
    r
}

fn apply(m: &M4, p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r])
}

fn det3(m: &M4) -> f32 {
    m[0][0] * (m[1][1] * m[2][2] - m[2][1] * m[1][2]) - m[1][0] * (m[0][1] * m[2][2] - m[2][1] * m[0][2]) + m[2][0] * (m[0][1] * m[1][2] - m[1][1] * m[0][2])
}

fn extras_json(e: &gltf::json::Extras) -> Option<Value> {
    e.as_ref().and_then(|r| serde_json::from_str(r.get()).ok())
}

fn c8_model(e: &gltf::json::Extras) -> Option<Vec<u8>> {
    let v = extras_json(e)?;
    base64::engine::general_purpose::STANDARD.decode(v.get("c8_model")?.as_str()?).ok()
}

/// Read a `.glb` (or `.gltf` with data-URI buffers) into a triangle soup.
pub fn read(data: &[u8]) -> Result<Soup, String> {
    let g = gltf::Gltf::from_slice(data).map_err(|e| format!("not a readable glTF file: {e}"))?;
    let mut buffers: Vec<Vec<u8>> = Vec::new();
    for b in g.buffers() {
        buffers.push(match b.source() {
            gltf::buffer::Source::Bin => g.blob.clone().ok_or("the .glb has no binary chunk")?,
            gltf::buffer::Source::Uri(uri) => match uri.split_once(";base64,") {
                Some((head, d)) if head.starts_with("data:") => base64::engine::general_purpose::STANDARD.decode(d).map_err(|e| format!("buffer {}: {e}", b.index()))?,
                _ => return Err(format!("buffer {} is an external file ({uri}); export as .glb (glTF Binary) instead", b.index())),
            },
        });
    }
    let mut soup = Soup::default();
    // materials, plus a default one at the end for primitives without a material
    for m in g.materials() {
        let name = m.name().unwrap_or("").to_string();
        let key = extras_json(m.extras()).and_then(|v| MatKey::from_extras(&v)).or_else(|| MatKey::parse(&name));
        let pbr = m.pbr_metallic_roughness();
        let c = pbr.base_color_factor();
        soup.mats.push(MatIn { name, key, colour: Some([c[0], c[1], c[2]].map(super::linear_to_srgb)), textured: pbr.base_color_texture().is_some() });
    }
    let default_mat = soup.mats.len();
    soup.mats.push(MatIn { name: "(none)".into(), ..Default::default() });
    if let Some(s) = g.default_scene().or_else(|| g.scenes().next()) {
        soup.original = c8_model(s.extras());
    }
    let ident: M4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
    let mut stack: Vec<(gltf::Node, M4)> = match g.default_scene().or_else(|| g.scenes().next()) {
        Some(s) => s.nodes().map(|n| (n, ident)).collect(),
        None => g.nodes().map(|n| (n, ident)).collect(),
    };
    let mut skipped = 0;
    while let Some((node, parent)) = stack.pop() {
        let world = mul(&parent, &node.transform().matrix());
        if soup.original.is_none() {
            soup.original = c8_model(node.extras());
        }
        for c in node.children() {
            stack.push((c, world));
        }
        let Some(mesh) = node.mesh() else { continue };
        if soup.original.is_none() {
            soup.original = c8_model(mesh.extras());
        }
        let flip = det3(&world) < 0.0;
        for p in mesh.primitives() {
            if p.mode() != gltf::mesh::Mode::Triangles {
                skipped += 1;
                continue;
            }
            let r = p.reader(|b| buffers.get(b.index()).map(|v| v.as_slice()));
            let Some(pos) = r.read_positions() else { continue };
            let pos: Vec<[f32; 3]> = pos.map(|q| apply(&world, q)).collect();
            let uv: Option<Vec<[f32; 2]>> = r.read_tex_coords(0).map(|t| t.into_f32().collect());
            let idx: Vec<u32> = match r.read_indices() {
                Some(i) => i.into_u32().collect(),
                None => (0..pos.len() as u32).collect(),
            };
            let mat = p.material().index().unwrap_or(default_mat);
            for t in idx.as_chunks::<3>().0 {
                let mut t = t.map(|i| i as usize);
                if flip {
                    t.swap(1, 2);
                }
                if t.iter().any(|&i| i >= pos.len()) {
                    return Err("a primitive's index points past its vertices".into());
                }
                soup.faces.push(FaceIn { mat, pos: t.map(|i| pos[i]), uv: uv.as_ref().filter(|u| u.len() >= pos.len()).map(|u| t.map(|i| u[i])) });
            }
        }
    }
    if skipped > 0 {
        soup.notes.push(format!("{skipped} primitives that are not triangle lists were skipped"));
    }
    Ok(soup)
}

#[cfg(test)]
mod tests {
    use super::super::tests::model;
    use super::super::{build, canonical, Look};
    use super::*;

    #[test]
    fn glb_round_trip_is_byte_exact_for_every_shipped_model() {
        let Some(dir) = super::super::tests::resources() else { return };
        let look = Look::load(0).unwrap();
        for name in c8_data::model::model_files() {
            let raw = std::fs::read(dir.join(format!("OBJECTS/DATA/{name}.DAT"))).unwrap();
            let m = Model::parse(&raw).unwrap();
            let glb = write(&m, &look, &name).unwrap();
            let soup = read(&glb).unwrap();
            assert_eq!(soup.faces.len(), m.tris.len(), "{name}");
            let back = build(&soup, &look, None).unwrap();
            assert!(back.exact, "{name}");
            assert_eq!(back.model.to_bytes().unwrap(), raw, "{name}");
        }
    }

    #[test]
    fn glb_without_the_embedded_original_keeps_every_triangle() {
        let Some((_, m)) = model("RECOV0-0") else { return };
        let look = Look::load(0).unwrap();
        let mut soup = read(&write(&m, &look, "RECOV0-0").unwrap()).unwrap();
        soup.original = None;
        let back = build(&soup, &look, None).unwrap();
        assert!(!back.exact);
        assert_eq!(canonical(&back.model), canonical(&m));
        // vertices at the same position merge (RECOV0-0 has three such pairs)
        let distinct = |v: &[[i16; 3]]| v.iter().collect::<std::collections::BTreeSet<_>>().len();
        assert_eq!(back.model.verts.len(), distinct(&m.verts));
        // with the original as a template, order and shade come back too
        let back = build(&soup, &look, Some(&m)).unwrap();
        assert!(back.exact);
    }

    #[test]
    fn edited_geometry_keeps_what_it_can() {
        let Some((_, m)) = model("CONE0-0") else { return };
        let look = Look::load(0).unwrap();
        let mut soup = read(&write(&m, &look, "CONE0-0").unwrap()).unwrap();
        // drop one triangle and move the whole thing up by one unit (1/256 m)
        soup.faces.pop();
        let back = build(&soup, &look, None).unwrap();
        assert!(!back.exact);
        assert_eq!(back.model.tris.len(), m.tris.len() - 1);
        assert_eq!(back.model.tris[..], m.tris[..m.tris.len() - 1]);
        for f in &mut soup.faces {
            for p in &mut f.pos {
                p[1] += 1.0 / 256.0;
            }
        }
        let back = build(&soup, &look, None).unwrap();
        assert_eq!(back.model.tris.len(), m.tris.len() - 1);
        for (v, s) in back.model.verts.iter().zip(&back.model.shade) {
            // every vertex is an old one moved up, and keeps that one's shade byte
            let old = m.verts.iter().position(|o| [o[0], o[1], o[2] + 1] == *v).expect("moved vertex");
            assert_eq!(*s, m.shade[old]);
        }
    }

    #[test]
    fn foreign_materials_and_limits() {
        let Some(_) = super::super::tests::resources() else { return };
        let look = Look::load(0).unwrap();
        // a red flat material without hio naming, and a textured one at atlas entry 3's corner
        let (ex, ey, _, _) = look.atlas.entries[3];
        let uv = |x: u32, y: u32| [(x as f32 + 0.5) / 256.0, (y as f32 + 0.5) / look.atlas.h as f32];
        let soup = Soup {
            mats: vec![MatIn { name: "Red".into(), colour: Some([1.0, 0.0, 0.0]), ..Default::default() }, MatIn { name: "Paint".into(), textured: true, ..Default::default() }],
            faces: vec![
                FaceIn { mat: 0, pos: [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], uv: None },
                FaceIn { mat: 1, pos: [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], uv: Some([uv(ex, ey), uv(ex + 4, ey), uv(ex, ey + 4)]) },
            ],
            ..Default::default()
        };
        let r = build(&soup, &look, None).unwrap();
        let m = &r.model;
        assert_eq!(m.tris[0].kind, 0);
        let red = look.pal[m.tris[0].col as usize];
        assert!(red[0] > 150 && red[1] < 80 && red[2] < 80, "{red:?}");
        assert_eq!((m.tris[1].kind, m.tris[1].col), (5, 3));
        assert_eq!(m.tris[1].uv, [(0, 0), (0, 4), (4, 0)]); // winding flipped
        assert!(!r.notes.is_empty());
        // too many vertices
        let mut big = Soup { mats: vec![MatIn::default()], ..Default::default() };
        for i in 0..100 {
            let x = i as f32;
            big.faces.push(FaceIn { mat: 0, pos: [[x, 0.0, 0.0], [x, 1.0, 0.0], [x, 0.0, 1.0]], uv: None });
        }
        let e = build(&big, &look, None).err().unwrap();
        assert!(e.contains("300 distinct vertices"), "{e}");
        // out of range
        big.faces.truncate(1);
        big.faces[0].pos[0] = [500.0, 0.0, 0.0];
        assert!(build(&big, &look, None).err().unwrap().contains("±128 m"));
    }
}
