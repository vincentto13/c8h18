//! Wavefront OBJ + MTL export and import (the fallback to glTF). Same axes, units, winding and
//! material names as the glTF export; the vertices are written once each in file order, so the
//! mesh arrives connected. The original file rides along in a `# c8_model <base64>` comment,
//! which survives our own round trip but not a re-export from Blender (Blender drops comments).
use super::{groups, to_scene, FaceIn, Look, MatIn, MatKey, Soup};
use base64::Engine;
use c8_data::model::Model;
use std::fmt::Write as _;
use std::path::Path;

/// (obj text, mtl text). `mtl_name` and `png_name` are the file names the OBJ and MTL refer to.
pub fn write(m: &Model, look: &Look, name: &str, mtl_name: &str, png_name: &str) -> Result<(String, String), String> {
    let original = m.to_bytes()?;
    let mut o = String::new();
    let mut mtl = String::new();
    let _ = writeln!(o, "# c8h18-edit model {name}: 1 unit = 1 m = 256 file units, +Y up, front faces +Z");
    let _ = writeln!(o, "# materials: c8_t<type>_e<atlas entry> textured, c8_f<type>_c<palette colour> flat");
    let _ = writeln!(o, "# c8_model {}", base64::engine::general_purpose::STANDARD.encode(&original));
    let _ = writeln!(o, "mtllib {mtl_name}");
    let _ = writeln!(o, "o {name}");
    for v in &m.verts {
        let p = to_scene(*v);
        let _ = writeln!(o, "v {} {} {}", p[0], p[1], p[2]);
    }
    let _ = writeln!(mtl, "# c8h18-edit model {name}");
    let mut vt: Vec<(u8, (u8, u8))> = Vec::new();
    // doubled triangles get copies of their vertices, after the model's own
    let doubled = super::doubled(m);
    let mut extra: Vec<u16> = Vec::new();
    let mut vi = |ti: usize, v: u16| -> usize {
        if doubled[ti] {
            extra.push(v);
            m.verts.len() + extra.len()
        } else {
            v as usize + 1
        }
    };
    let mut faces = String::new();
    for (key, tris) in groups(m) {
        let _ = writeln!(faces, "usemtl {}", key.name());
        let _ = writeln!(mtl, "\nnewmtl {}", key.name());
        match key {
            MatKey::Tex { entry, .. } => {
                let _ = writeln!(mtl, "Kd 1 1 1\nillum 0\nmap_Kd {png_name}");
                for &ti in &tris {
                    let t = &m.tris[ti];
                    faces.push('f');
                    for k in [0, 2, 1] {
                        let c = (entry, t.uv[k]);
                        let i = vt.iter().position(|x| *x == c).unwrap_or_else(|| {
                            vt.push(c);
                            vt.len() - 1
                        });
                        let _ = write!(faces, " {}/{}", vi(ti, t.v[k]), i + 1);
                    }
                    faces.push('\n');
                }
            }
            MatKey::Flat { col, .. } => {
                let c = look.rgb(col, 32).map(super::srgb_to_linear);
                let _ = writeln!(mtl, "Kd {} {} {}\nillum 0", c[0], c[1], c[2]);
                for &ti in &tris {
                    let t = &m.tris[ti];
                    let _ = writeln!(faces, "f {} {} {}", vi(ti, t.v[0]), vi(ti, t.v[2]), vi(ti, t.v[1]));
                }
            }
        }
    }
    for &v in &extra {
        let p = to_scene(m.verts[v as usize]);
        let _ = writeln!(o, "v {} {} {}", p[0], p[1], p[2]);
    }
    for (entry, uv) in &vt {
        let [u, v] = look.uv(*entry, *uv);
        // OBJ's v runs up from the bottom of the image
        let _ = writeln!(o, "vt {} {}", u, 1.0 - v);
    }
    o.push_str(&faces);
    Ok((o, mtl))
}

/// The MTL file named by the OBJ's `mtllib`, if it is next to it.
pub fn mtl_next_to(path: &Path, text: &str) -> Option<String> {
    let name = text.lines().find_map(|l| l.trim().strip_prefix("mtllib "))?.trim();
    std::fs::read_to_string(path.with_file_name(name)).ok()
}

/// Read an OBJ (with its MTL, for the colours and textures of foreign materials).
pub fn read(text: &str, mtl: Option<&str>) -> Result<Soup, String> {
    // material name → (colour sRGB, textured)
    let mut lib: std::collections::HashMap<String, (Option<[f32; 3]>, bool)> = Default::default();
    if let Some(mtl) = mtl {
        let mut cur = String::new();
        for l in mtl.lines() {
            let mut w = l.split_whitespace();
            match w.next() {
                Some("newmtl") => {
                    cur = w.collect::<Vec<_>>().join(" ");
                    lib.entry(cur.clone()).or_default();
                }
                Some("Kd") => {
                    let c: Vec<f32> = w.filter_map(|v| v.parse().ok()).collect();
                    if c.len() >= 3 {
                        lib.entry(cur.clone()).or_default().0 = Some([c[0], c[1], c[2]].map(super::linear_to_srgb));
                    }
                }
                Some("map_Kd") => lib.entry(cur.clone()).or_default().1 = true,
                _ => {}
            }
        }
    }
    let mut soup = Soup::default();
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut mat = None;
    let mut polys = 0;
    for (ln, l) in text.lines().enumerate() {
        let l = l.trim();
        if let Some(b) = l.strip_prefix("# c8_model ") {
            soup.original = base64::engine::general_purpose::STANDARD.decode(b.trim()).ok();
            continue;
        }
        let mut w = l.split_whitespace();
        let bad = |what: &str| format!("line {}: {what}", ln + 1);
        match w.next() {
            Some("v") => {
                let c: Vec<f32> = w.take(3).map(|v| v.parse().map_err(|_| bad("bad vertex"))).collect::<Result<_, _>>()?;
                if c.len() < 3 {
                    return Err(bad("vertex needs x y z"));
                }
                pos.push([c[0], c[1], c[2]]);
            }
            Some("vt") => {
                let c: Vec<f32> = w.take(2).map(|v| v.parse().map_err(|_| bad("bad texture coordinate"))).collect::<Result<_, _>>()?;
                uvs.push([c.first().copied().unwrap_or(0.0), 1.0 - c.get(1).copied().unwrap_or(0.0)]);
            }
            Some("usemtl") => {
                let name = w.collect::<Vec<_>>().join(" ");
                let i = soup.mats.iter().position(|m| m.name == name).unwrap_or_else(|| {
                    let (colour, textured) = lib.get(&name).copied().unwrap_or((None, false));
                    soup.mats.push(MatIn { key: MatKey::parse(&name), name, colour, textured });
                    soup.mats.len() - 1
                });
                mat = Some(i);
            }
            Some("f") => {
                let resolve = |s: &str, n: usize| -> Result<Option<usize>, String> {
                    if s.is_empty() {
                        return Ok(None);
                    }
                    let i: i64 = s.parse().map_err(|_| bad("bad face index"))?;
                    let r = if i < 0 { n as i64 + i } else { i - 1 };
                    if r < 0 || r as usize >= n {
                        return Err(bad("face index out of range"));
                    }
                    Ok(Some(r as usize))
                };
                let mut corners = Vec::new();
                for c in w {
                    let mut parts = c.split('/');
                    let v = resolve(parts.next().unwrap_or(""), pos.len())?.ok_or_else(|| bad("face corner without a vertex"))?;
                    let t = resolve(parts.next().unwrap_or(""), uvs.len())?;
                    corners.push((v, t));
                }
                if corners.len() < 3 {
                    return Err(bad("face with fewer than 3 corners"));
                }
                if corners.len() > 3 {
                    polys += 1;
                }
                let m = match mat {
                    Some(m) => m,
                    None => {
                        soup.mats.push(MatIn { name: "(none)".into(), ..Default::default() });
                        mat = Some(soup.mats.len() - 1);
                        soup.mats.len() - 1
                    }
                };
                // fan triangulation
                for k in 1..corners.len() - 1 {
                    let tri = [corners[0], corners[k], corners[k + 1]];
                    let uv = match tri.map(|c| c.1) {
                        [Some(a), Some(b), Some(c)] => Some([uvs[a], uvs[b], uvs[c]]),
                        _ => None,
                    };
                    soup.faces.push(FaceIn { mat: m, pos: tri.map(|c| pos[c.0]), uv });
                }
            }
            _ => {}
        }
    }
    soup.points = pos;
    if polys > 0 {
        soup.notes.push(format!("{polys} polygons with more than 3 corners were split into triangles (fan)"));
    }
    Ok(soup)
}

#[cfg(test)]
mod tests {
    use super::super::{build, canonical, Look};
    use super::*;

    #[test]
    fn obj_round_trip_is_byte_exact_and_lossless_without_the_comment() {
        let Some(dir) = super::super::tests::resources() else { return };
        let look = Look::load(0).unwrap();
        for name in ["CAR0-0", "SIGN0-0", "RECOV0-0", "TRACK0-5", "BAREL0-1"] {
            let raw = std::fs::read(dir.join(format!("OBJECTS/DATA/{name}.DAT"))).unwrap();
            let m = Model::parse(&raw).unwrap();
            let (o, mtl) = write(&m, &look, name, "x.mtl", "x.png").unwrap();
            let soup = read(&o, Some(&mtl)).unwrap();
            let back = build(&soup, &look, None).unwrap();
            assert!(back.exact, "{name}");
            assert_eq!(back.model.to_bytes().unwrap(), raw, "{name}");
            // without the embedded original: the same triangles and vertex order
            let stripped: String = o.lines().filter(|l| !l.starts_with("# c8_model")).map(|l| format!("{l}\n")).collect();
            let back = build(&read(&stripped, Some(&mtl)).unwrap(), &look, None).unwrap();
            assert_eq!(canonical(&back.model), canonical(&m), "{name}");
            // vertices keep the file order; the ones at the same position as an earlier one merge
            let mut first = Vec::new();
            for v in &m.verts {
                if !first.contains(v) {
                    first.push(*v);
                }
            }
            assert_eq!(back.model.verts, first, "{name}");
        }
    }

    #[test]
    fn obj_quads_and_negative_indices() {
        let Some(_) = super::super::tests::resources() else { return };
        let look = Look::load(0).unwrap();
        let o = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nusemtl c8_f0_c15\nf -4 -3 -2 -1\n";
        let soup = read(o, None).unwrap();
        assert_eq!(soup.faces.len(), 2);
        let m = build(&soup, &look, None).unwrap().model;
        assert_eq!(m.tris.len(), 2);
        assert!(m.tris.iter().all(|t| t.kind == 0 && t.col == 15));
        assert!(read("f 1 2 3\n", None).is_err());
    }
}
