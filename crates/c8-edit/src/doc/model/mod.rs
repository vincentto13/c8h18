//! Model viewer and editor (`OBJECTS/DATA/*.DAT`, spec 01 §8, 08 §8): orbit view drawn exactly
//! like the game (the game's triangle builder, palette, colour table), wireframe, stats, triangle
//! info on hover, per-triangle type / texture / colour edits, and the Blender round trip: export
//! to glTF (.glb) or OBJ, import from .glb / .gltf / .obj (see [`crate::model_io`]).
mod view;

use super::history::{Command, History};
use super::Document;
use crate::model_io::{self, Look};
use crate::resources::Kind;
use eframe::egui::{self, Color32, RichText};
use c8_data::model::{is_textured, kind_name, Model};
use std::path::{Path, PathBuf};
use view::{Gpu, LineVertex, Orbit, Show};

struct ModelEdit {
    label: String,
    before: Model,
    after: Model,
}

impl Command<Model> for ModelEdit {
    fn apply(&mut self, t: &mut Model) {
        t.clone_from(&self.after);
    }
    fn revert(&mut self, t: &mut Model) {
        t.clone_from(&self.before);
    }
    fn label(&self) -> String {
        self.label.clone()
    }
}

pub struct ModelDoc {
    path: PathBuf,
    rel: String,
    /// file stem in capitals, e.g. `CAR0-0`
    name: String,
    model: Model,
    history: History<Model>,
    look: Result<Look, String>,
    set: usize,
    cam: Option<Orbit>,
    show: Show,
    gpu: Option<Gpu>,
    /// what the GPU buffers were built from: (model, colour table set)
    uploaded: Option<(Model, usize)>,
    overlay_key: Option<(Option<usize>, Option<usize>, bool, Orbit)>,
    hovered: Option<usize>,
    selected: Option<usize>,
    message: Result<String, String>,
    test: bool,
}

impl ModelDoc {
    pub fn open(path: PathBuf, rel: String) -> Result<ModelDoc, String> {
        let raw = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let model = Model::parse(&raw).map_err(|e| format!("{rel}: {e}"))?;
        let name = path.file_stem().map(|s| s.to_string_lossy().to_uppercase()).unwrap_or_else(|| "MODEL".into());
        Ok(ModelDoc { path, rel, name, model, history: History::default(), look: Look::load(0), set: 0, cam: None, show: Show::default(), gpu: None, uploaded: None, overlay_key: None, hovered: None, selected: None, message: Ok(String::new()), test: false })
    }

    fn set_model(&mut self, label: &str, after: Model) {
        if after != self.model {
            let before = self.model.clone();
            self.history.run(Box::new(ModelEdit { label: label.into(), before, after }), &mut self.model);
        }
    }

    fn export(&mut self, ext: &str) {
        let Ok(look) = &self.look else { return };
        let Some(p) = rfd::FileDialog::new().set_file_name(format!("{}.{ext}", self.name)).add_filter(ext, &[ext]).save_file() else { return };
        self.message = model_io::export_file(&self.model, look, &self.name, &p).map(|f| format!("exported {}", f.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join(", ")));
    }

    /// Import a file, with the current model as the template (header, order, shade bytes).
    fn import_path(&mut self, p: &Path) {
        let Ok(look) = &self.look else { return };
        match model_io::import_file(p, look, Some(&self.model)) {
            Ok(r) => {
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let summary = format!("imported {name}: {} vertices, {} triangles{}\n{}", r.model.verts.len(), r.model.tris.len(), if r.exact { " (unchanged)" } else { "" }, r.notes.join("\n"));
                self.set_model(&format!("import {name}"), r.model);
                self.selected = None;
                self.message = Ok(summary);
            }
            Err(e) => self.message = Err(format!("import failed:\n{e}")),
        }
    }

    fn stats_ui(&self, ui: &mut egui::Ui) {
        let m = &self.model;
        let (lo, hi) = m.bounds();
        let mut kinds = std::collections::BTreeMap::new();
        let mut entries = std::collections::BTreeSet::new();
        let mut colours = std::collections::BTreeSet::new();
        for t in &m.tris {
            *kinds.entry(t.kind).or_insert(0) += 1;
            if t.textured() {
                entries.insert(t.col);
            } else {
                colours.insert(t.col);
            }
        }
        let list = |s: &std::collections::BTreeSet<u8>| s.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ");
        egui::Grid::new("model-stats").num_columns(2).striped(true).show(ui, |ui| {
            ui.label("Vertices");
            ui.label(format!("{} (at most {})", m.verts.len(), c8_data::model::MAX_VERTS));
            ui.end_row();
            ui.label("Triangles");
            ui.label(m.tris.len().to_string());
            ui.end_row();
            for (k, n) in &kinds {
                ui.label(format!("  type {k}"));
                ui.label(format!("{n} · {}", kind_name(*k)));
                ui.end_row();
            }
            ui.label("Texture entries");
            ui.label(if entries.is_empty() { "none".into() } else { list(&entries) });
            ui.end_row();
            ui.label("Flat colours");
            ui.label(if colours.is_empty() { "none".into() } else { list(&colours) });
            ui.end_row();
            ui.label("Bounds x");
            ui.label(format!("{} .. {}", lo[0], hi[0]));
            ui.end_row();
            ui.label("Bounds y (back)");
            ui.label(format!("{} .. {}", lo[1], hi[1]));
            ui.end_row();
            ui.label("Bounds z (up)");
            ui.label(format!("{} .. {}", lo[2], hi[2]));
            ui.end_row();
            let size = |k: usize| (hi[k] as i32 - lo[k] as i32) as f32;
            ui.label("Size");
            ui.label(format!("{:.2} × {:.2} × {:.2} cells", size(0) / 2048.0, size(1) / 2048.0, size(2) / 2048.0)).on_hover_text("width × length × height; 2048 units per map cell, 256 per metre in the exports");
            ui.end_row();
            ui.label("Flags");
            ui.label(format!("{} ({} frame{}{})", m.flags, 1 + m.more_frames.len(), if m.more_frames.is_empty() { "" } else { "s" }, if m.flags & 4 != 0 { ", vertex shade" } else { ", flickering shade" }));
            ui.end_row();
            ui.label("Header n");
            ui.label(m.header_n().to_string()).on_hover_text("u32 at 0x2c, meaning unknown, never read by the game (kept as is)");
            ui.end_row();
        });
        ui.label(RichText::new("A model file holds one model (one animation frame in every shipped file).").small().color(Color32::GRAY));
        for p in m.check() {
            ui.label(RichText::new(p).color(Color32::from_rgb(255, 170, 80)));
        }
    }

    fn tri_text(&self, i: usize) -> String {
        let m = &self.model;
        let t = &m.tris[i];
        let mut s = format!("triangle {i}: type {} ({})\n", t.kind, kind_name(t.kind));
        s += &if t.textured() { format!("atlas entry {}\n", t.col) } else { format!("colour {}\n", t.col) };
        for k in 0..3 {
            let v = t.v[k] as usize;
            let p = m.verts[v];
            s += &format!("v{v}: ({}, {}, {}) shade {}", p[0], p[1], p[2], m.shade.get(v).copied().unwrap_or(0));
            if t.textured() {
                s += &format!("  uv ({}, {})", t.uv[k].0, t.uv[k].1);
            }
            s.push('\n');
        }
        s.pop();
        s
    }

    fn selected_ui(&mut self, ui: &mut egui::Ui) {
        let Some(i) = self.selected.filter(|&i| i < self.model.tris.len()) else {
            ui.label(RichText::new("Click a triangle to select it.").small().color(Color32::GRAY));
            return;
        };
        let mut t = self.model.tris[i];
        ui.label(RichText::new(self.tri_text(i)).monospace().small());
        ui.horizontal(|ui| {
            ui.label("Type");
            egui::ComboBox::from_id_salt("tri-kind").selected_text(format!("{} · {}", t.kind, kind_name(t.kind))).show_ui(ui, |ui| {
                for k in [0u8, 2, 3, 4, 5, 6] {
                    ui.selectable_value(&mut t.kind, k, format!("{k} · {}", kind_name(k)));
                }
            });
        });
        let entries = self.look.as_ref().map(|l| l.atlas.entries.len()).unwrap_or(51).max(1);
        ui.horizontal(|ui| {
            if is_textured(t.kind) {
                ui.label("Atlas entry");
                ui.add(egui::DragValue::new(&mut t.col).range(0..=entries - 1));
            } else {
                ui.label("Colour");
                ui.add(egui::DragValue::new(&mut t.col));
                if let Ok(l) = &self.look {
                    let c = l.rgb(t.col, 32);
                    let (r, _) = ui.allocate_exact_size(egui::Vec2::new(18.0, 18.0), egui::Sense::hover());
                    ui.painter().rect_filled(r, 3.0, Color32::from_rgb(c[0], c[1], c[2]));
                }
            }
        });
        if is_textured(t.kind) {
            for k in 0..3 {
                ui.horizontal(|ui| {
                    ui.label(format!("uv {k}"));
                    ui.add(egui::DragValue::new(&mut t.uv[k].0));
                    ui.add(egui::DragValue::new(&mut t.uv[k].1));
                });
            }
        }
        if ui.button("Flip facing").on_hover_text("Reverse the corner order: front and back swap").clicked() {
            t.v.swap(1, 2);
            t.uv.swap(1, 2);
        }
        if t != self.model.tris[i] {
            let mut m = self.model.clone();
            m.tris[i] = t;
            self.set_model(&format!("edit triangle {i}"), m);
        }
        if ui.button("Delete triangle").clicked() {
            let mut m = self.model.clone();
            m.tris.remove(i);
            self.set_model(&format!("delete triangle {i}"), m);
            self.selected = None;
        }
    }

    /// Lines on top: axes at the origin and the hovered / selected triangle.
    fn overlay(&self, cam: &Orbit) -> Vec<LineVertex> {
        let m = &self.model;
        let mut out = Vec::new();
        let mut seg = |a: [f32; 3], b: [f32; 3], c: [f32; 4]| {
            out.push(LineVertex { pos: a, colour: c });
            out.push(LineVertex { pos: b, colour: c });
        };
        if self.show.axes {
            let l = cam.dist * 0.15;
            // x right (red), up (green), forward = −row (blue)
            seg([0.0; 3], [l, 0.0, 0.0], view::linear([230, 60, 60], 0.9));
            seg([0.0; 3], [0.0, l, 0.0], view::linear([60, 210, 60], 0.9));
            seg([0.0; 3], [0.0, 0.0, -l], view::linear([70, 120, 255], 0.9));
        }
        for (i, c) in [(self.hovered, [255, 255, 255]), (self.selected, [255, 200, 0])] {
            let Some(t) = i.and_then(|i| m.tris.get(i)) else { continue };
            for k in 0..3 {
                seg(view::world(m.verts[t.v[k] as usize]), view::world(m.verts[t.v[(k + 1) % 3] as usize]), view::linear(c, 1.0));
            }
        }
        out
    }
}

impl Document for ModelDoc {
    fn title(&self) -> String {
        self.rel.clone()
    }
    fn kind(&self) -> Kind {
        Kind::Model
    }
    fn path(&self) -> Option<&Path> {
        Some(&self.path)
    }
    fn dirty(&self) -> bool {
        self.history.dirty()
    }
    fn can_save(&self) -> bool {
        true
    }
    fn save_to(&mut self, path: &Path) -> Result<(), String> {
        let bytes = self.model.to_bytes()?;
        std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        self.path = path.to_path_buf();
        self.history.mark_saved();
        Ok(())
    }
    fn undo(&mut self) {
        self.history.undo(&mut self.model);
    }
    fn redo(&mut self) {
        self.history.redo(&mut self.model);
    }
    fn can_undo(&self) -> bool {
        self.history.can_undo()
    }
    fn can_redo(&self) -> bool {
        self.history.can_redo()
    }
    fn game_files(&self) -> Result<Vec<(String, Vec<u8>)>, String> {
        Ok(vec![(format!("OBJECTS/DATA/{}.DAT", self.name), self.model.to_bytes()?)])
    }
    fn game_args(&self) -> Vec<String> {
        // the track models turn in the track-selection carousel; everything else is in a race
        if self.name.starts_with("TRACK") {
            vec!["--no-intro".into()]
        } else {
            vec!["--no-intro".into(), "--race".into(), "1".into()]
        }
    }
    fn wants_test(&mut self) -> bool {
        std::mem::take(&mut self.test)
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        let Some(rs) = crate::gpu::state() else {
            ui.centered_and_justified(|ui| ui.label("The 3D view needs the wgpu renderer."));
            return;
        };
        let look = match &self.look {
            Ok(l) => l,
            Err(e) => {
                ui.label(RichText::new(format!("cannot load the palette and model textures: {e}")).color(Color32::from_rgb(255, 110, 90)));
                return;
            }
        };
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.show.faces, "Faces");
            ui.checkbox(&mut self.show.wire, "Wireframe");
            ui.checkbox(&mut self.show.shading, "Shading").on_hover_text("Shade faces by their direction to read the shape (the game draws models without it)");
            ui.checkbox(&mut self.show.axes, "Axes").on_hover_text("At the model origin: red = right, green = up, blue = forward");
            if ui.button("Reset view").clicked() {
                self.cam = None;
            }
        });
        let (device, queue) = (&rs.device, &rs.queue);
        let gpu = self.gpu.get_or_insert_with(|| Gpu::new(device));
        if self.uploaded.as_ref().is_none_or(|(m, s)| *s != self.set || *m != self.model) {
            if self.uploaded.as_ref().is_none_or(|(_, s)| *s != self.set) {
                gpu.upload_look(device, queue, look);
            }
            gpu.upload_model(device, &self.model, look);
            self.uploaded = Some((self.model.clone(), self.set));
            self.overlay_key = None;
        }
        let cam = self.cam.get_or_insert_with(|| Orbit::frame(&self.model));
        let (rect, resp, px) = view::allocate(ui);
        view::camera_input(ui, &resp, cam);
        let cam = *cam;
        let vp = cam.matrix(px[0] as f32 / px[1] as f32);
        self.hovered = resp.hover_pos().filter(|_| !resp.dragged()).and_then(|p| view::pick(&self.model, &vp, rect, p));
        if resp.clicked() {
            self.selected = resp.interact_pointer_pos().and_then(|p| view::pick(&self.model, &vp, rect, p));
        }
        let key = (self.hovered, self.selected, self.show.axes, cam);
        if self.overlay_key != Some(key) {
            let lines = self.overlay(&cam);
            let gpu = self.gpu.as_mut().unwrap();
            gpu.upload_overlay(device, &lines);
            self.overlay_key = Some(key);
        }
        let bg = view::linear([46, 50, 58], 1.0);
        if let Some(id) = self.gpu.as_mut().unwrap().draw(rs, px, &cam, self.show, bg) {
            view::paint(ui, rect, id);
        }
        if let Some(i) = self.hovered {
            resp.on_hover_text(RichText::new(self.tri_text(i)).monospace());
        }
        ui.painter().text(rect.left_bottom() + egui::Vec2::new(6.0, -4.0), egui::Align2::LEFT_BOTTOM, "drag to orbit · right/middle or Shift drag to pan · wheel to zoom · click to select", egui::FontId::proportional(11.0), Color32::from_white_alpha(190));
    }

    fn side_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("Export .glb…").on_hover_text("glTF 2.0 binary for Blender: textures, materials and everything needed to import it back unchanged").clicked() {
                self.export("glb");
            }
            if ui.button("Export .obj…").on_hover_text("Wavefront OBJ + MTL + the texture sheet as PNG").clicked() {
                self.export("obj");
            }
            if ui.button("Import…").on_hover_text("Replace the model with a .glb, .gltf or .obj (undoable); this model supplies the shade bytes and order of what stays").clicked()
                && let Some(p) = rfd::FileDialog::new().add_filter("3D model", &["glb", "gltf", "obj"]).pick_file()
            {
                self.import_path(&p);
            }
        });
        if ui.button("Test in game").on_hover_text(if self.name.starts_with("TRACK") { "Start the game with this model: the track models turn in the track-selection screen" } else { "Start a race on track 1 with this model in place of the original (crafts appear when a racer flies that craft and colour)" }).clicked() {
            self.test = true;
        }
        match &self.message {
            Ok(m) if !m.is_empty() => {
                ui.label(RichText::new(m).small());
            }
            Err(e) => {
                ui.label(RichText::new(e).small().color(Color32::from_rgb(255, 120, 100)));
            }
            _ => {}
        }
        ui.separator();
        let mut set = self.set;
        egui::ComboBox::from_label("Colour table").selected_text(format!("TABLE0-{set} (track {})", set + 1)).show_ui(ui, |ui| {
            for s in 0..6 {
                ui.selectable_value(&mut set, s, format!("TABLE0-{s} (track {})", s + 1));
            }
        });
        if set != self.set {
            match Look::load(set) {
                Ok(l) => {
                    self.look = Ok(l);
                    self.set = set;
                }
                Err(e) => self.message = Err(e),
            }
        }
        ui.separator();
        ui.heading("Model");
        self.stats_ui(ui);
        ui.separator();
        ui.heading("Triangle");
        self.selected_ui(ui);
        ui.collapsing("History", |ui| {
            for l in self.history.labels().collect::<Vec<_>>().iter().rev().take(30) {
                ui.label(RichText::new(l).small());
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_import_undo_save() {
        let Some(root) = crate::model_io::tests::resources() else { return };
        let src = root.join("OBJECTS/DATA/CONE0-0.DAT");
        let mut d = ModelDoc::open(src.clone(), "OBJECTS/DATA/CONE0-0.DAT".into()).unwrap();
        assert_eq!(d.name, "CONE0-0");
        let orig = d.model.clone();
        let tmp = std::env::temp_dir().join(format!("c8-model-doc-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        // export, change the export (drop a triangle via the soup), import back
        let glb = tmp.join("cone.glb");
        model_io::export_file(&d.model, d.look.as_ref().unwrap(), &d.name, &glb).unwrap();
        d.import_path(&glb);
        assert!(!d.dirty(), "an unchanged import is no edit");
        let mut m = d.model.clone();
        m.tris.pop();
        let obj = tmp.join("cone.obj");
        model_io::export_file(&m, d.look.as_ref().unwrap(), &d.name, &obj).unwrap();
        // strip the embedded original: the doc's model is the template
        let text: String = std::fs::read_to_string(&obj).unwrap().lines().filter(|l| !l.starts_with("# c8_model")).map(|l| format!("{l}\n")).collect();
        std::fs::write(&obj, text).unwrap();
        d.import_path(&obj);
        assert!(d.message.is_ok(), "{:?}", d.message);
        assert_eq!(d.model.tris.len(), orig.tris.len() - 1);
        assert_eq!(d.model.shade, orig.shade);
        assert!(d.dirty());
        let files = d.game_files().unwrap();
        assert_eq!(files[0].0, "OBJECTS/DATA/CONE0-0.DAT");
        d.undo();
        assert_eq!(d.model, orig);
        d.redo();
        let out = tmp.join("CONE.DAT");
        d.save_to(&out).unwrap();
        assert!(!d.dirty());
        assert_eq!(Model::parse(&std::fs::read(&out).unwrap()).unwrap(), d.model);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Render CAR0-0 offscreen like the view does (needs a GPU; skipped without one) and check
    /// that the model covers part of the image in more than a couple of colours. Set
    /// C8_MODEL_SHOT=<file.png> to keep the picture.
    #[test]
    fn renders_the_car() {
        let Some(root) = crate::model_io::tests::resources() else { return };
        let Ok((device, queue)) = crate::gpu::device() else { return };
        let d = ModelDoc::open(root.join("OBJECTS/DATA/CAR0-0.DAT"), "CAR0-0.DAT".into()).unwrap();
        let look = d.look.as_ref().unwrap();
        let mut g = Gpu::new(&device);
        g.upload_look(&device, &queue, look);
        g.upload_model(&device, &d.model, look);
        let cam = Orbit::frame(&d.model);
        g.upload_overlay(&device, &d.overlay(&cam));
        let (w, h) = (480, 320);
        let bg = view::linear([46, 50, 58], 1.0);
        let rgba = g.snapshot(&device, &queue, w, h, &cam, Show { wire: true, ..Show::default() }, bg).unwrap();
        let bg8 = [46u8, 50, 58];
        let model_px: Vec<&[u8]> = rgba.chunks(4).filter(|p| p[..3].iter().zip(bg8).any(|(a, b)| a.abs_diff(b) > 3)).collect();
        assert!(model_px.len() > (w * h / 20) as usize, "{} model pixels", model_px.len());
        let colours: std::collections::HashSet<&[u8]> = model_px.iter().copied().collect();
        assert!(colours.len() > 20);
        if let Ok(p) = std::env::var("C8_MODEL_SHOT") {
            let f = std::fs::File::create(p).unwrap();
            let mut e = png::Encoder::new(f, w, h);
            e.set_color(png::ColorType::Rgba);
            e.write_header().unwrap().write_image_data(&rgba).unwrap();
        }
    }
}
