//! Colour table editor (`DATA/TABLE0-N.DAT`, spec 01 §5): shows the light rows, the blend table
//! and the flat colours, and regenerates them from the palette (after palette edits) and from
//! the track's terrain textures.
use super::history::{Command, History};
use super::pixels::{palette_rgb, vga8, Rgb};
use super::Document;
use crate::resources::Kind;
use eframe::egui::{self, Color32, RichText, Sense, TextureHandle, Vec2};
use c8_data::images::{ColourTable, Palette, SIZE};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq)]
enum View {
    Light,
    Blend,
    Flat,
}

struct TableEdit {
    label: String,
    before: Vec<u8>,
    after: Vec<u8>,
}

impl Command<Vec<u8>> for TableEdit {
    fn apply(&mut self, t: &mut Vec<u8>) {
        t.clone_from(&self.after);
    }
    fn revert(&mut self, t: &mut Vec<u8>) {
        t.clone_from(&self.before);
    }
    fn label(&self) -> String {
        self.label.clone()
    }
}

pub struct TableDoc {
    path: PathBuf,
    rel: String,
    /// texture set / track index (TABLE0-N → track N + 1)
    set: usize,
    data: Vec<u8>,
    history: History<Vec<u8>>,
    pal: Palette,
    rgb: Rgb,
    textures: Option<Vec<u8>>,
    view: View,
    haze: [u8; 3],
    sample_tex: usize,
    image: Option<(View, u64, TextureHandle)>,
    lit: Option<(usize, u64, TextureHandle)>,
    /// bumped on every change, to rebuild the images
    generation: u64,
    message: String,
    test: bool,
}

impl TableDoc {
    pub fn open(path: PathBuf, rel: String) -> Result<TableDoc, String> {
        let raw = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut data = c8_data::rnc::maybe_unpack(raw).map_err(|e| format!("{rel}: {e}"))?;
        if data.len() != SIZE {
            return Err(format!("{rel}: {} bytes, a colour table has {SIZE}", data.len()));
        }
        data.truncate(SIZE);
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let set = stem.rsplit_once("0-").and_then(|(_, n)| n.parse().ok()).unwrap_or(0usize).min(5);
        let pal = Palette::load_or_builtin();
        let haze = ColourTable::from_bytes(&data).haze(&pal);
        let textures = c8_data::images::load_terrain_textures(set).ok();
        Ok(TableDoc { path, rel, set, data, history: History::default(), rgb: palette_rgb(&pal), pal, textures, view: View::Light, haze, sample_tex: 1, image: None, lit: None, generation: 0, message: String::new(), test: false })
    }

    fn change(&mut self, label: &str, f: impl FnOnce(&mut ColourTable, &Palette, Option<&[u8]>)) {
        let mut t = ColourTable::from_bytes(&self.data);
        f(&mut t, &self.pal, self.textures.as_deref());
        let changed = t.data.iter().zip(&self.data).filter(|(a, b)| a != b).count();
        self.message = format!("{label}: {changed} of {SIZE} entries changed");
        if changed > 0 {
            self.history.run(Box::new(TableEdit { label: label.into(), before: self.data.clone(), after: t.data }), &mut self.data);
            self.generation += 1;
        }
    }

    fn colour(&self, i: u8) -> Color32 {
        let c = self.rgb[i as usize];
        Color32::from_rgb(c[0], c[1], c[2])
    }

    fn view_image(&self) -> egui::ColorImage {
        match self.view {
            View::Light => {
                let px = self.data[..16384].iter().map(|&i| self.colour(i)).collect();
                egui::ColorImage::new([256, 64], px)
            }
            View::Blend => {
                let px = self.data[16384..81920].iter().map(|&i| self.colour(i)).collect();
                egui::ColorImage::new([256, 256], px)
            }
            View::Flat => {
                // 4 variants of 512 entries, 32 per row
                let px = self.data[81920..].iter().map(|&i| self.colour(i)).collect();
                egui::ColorImage::new([32, 64], px)
            }
        }
    }

    /// The sample texture at light rows 0, 4, ... 60, 63 side by side.
    fn lit_image(&self) -> Option<egui::ColorImage> {
        let tex = self.textures.as_ref()?.get(self.sample_tex * 4096..(self.sample_tex + 1) * 4096)?;
        let rows: Vec<usize> = (0..64).step_by(4).chain([63]).collect();
        let w = rows.len() * 64;
        let mut px = vec![Color32::BLACK; w * 64];
        for (k, &row) in rows.iter().enumerate() {
            for y in 0..64 {
                for x in 0..64 {
                    let i = self.data[row * 256 + tex[y * 64 + x] as usize];
                    px[y * w + k * 64 + x] = self.colour(i);
                }
            }
        }
        Some(egui::ColorImage::new([w, 64], px))
    }
}

impl Document for TableDoc {
    fn title(&self) -> String {
        self.rel.clone()
    }
    fn kind(&self) -> Kind {
        Kind::ColourTable
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
        std::fs::write(path, &self.data).map_err(|e| format!("{}: {e}", path.display()))?;
        self.path = path.to_path_buf();
        self.history.mark_saved();
        Ok(())
    }
    fn undo(&mut self) {
        self.history.undo(&mut self.data);
        self.generation += 1;
    }
    fn redo(&mut self) {
        self.history.redo(&mut self.data);
        self.generation += 1;
    }
    fn can_undo(&self) -> bool {
        self.history.can_undo()
    }
    fn can_redo(&self) -> bool {
        self.history.can_redo()
    }
    fn game_files(&self) -> Result<Vec<(String, Vec<u8>)>, String> {
        Ok(vec![(format!("DATA/TABLE0-{}.DAT", self.set), self.data.clone())])
    }
    fn game_args(&self) -> Vec<String> {
        vec!["--no-intro".into(), "--race".into(), (self.set + 1).to_string()]
    }
    fn wants_test(&mut self) -> bool {
        std::mem::take(&mut self.test)
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.view, View::Light, "Light rows");
            ui.selectable_value(&mut self.view, View::Blend, "Blend table");
            ui.selectable_value(&mut self.view, View::Flat, "Flat colours");
        });
        ui.separator();
        if self.image.as_ref().is_none_or(|(v, g, _)| *v != self.view || *g != self.generation) {
            let t = ui.ctx().load_texture(format!("{}#view", self.rel), self.view_image(), egui::TextureOptions::NEAREST);
            self.image = Some((self.view, self.generation, t));
        }
        let tex = self.image.as_ref().map(|(_, _, t)| t.id()).unwrap();
        let avail = ui.available_size() - Vec2::new(0.0, if self.view == View::Light { 150.0 } else { 40.0 });
        match self.view {
            View::Light => {
                let size = Vec2::new(avail.x.min(256.0 * 4.0), (avail.y).clamp(64.0, 64.0 * 8.0));
                let r = ui.add(egui::Image::new((tex, size)).sense(Sense::hover()));
                if let Some(p) = r.hover_pos() {
                    let i = ((p.x - r.rect.left()) / size.x * 256.0) as usize;
                    let row = ((p.y - r.rect.top()) / size.y * 64.0) as usize;
                    let o = self.data[(row.min(63)) * 256 + i.min(255)];
                    r.on_hover_text(format!("row {row}: colour {i} → {o}{}", match row {
                        0 => " (haze)",
                        32 => " (identity)",
                        _ => "",
                    }));
                }
                ui.label(RichText::new("Each row remaps the 256 palette colours (left to right) to a brightness: row 0 is the haze at the far edge of the view, rows up to 31 fade towards it, row 32 is unchanged, rows below it darken to black. Terrain light values pick the row.").small().color(Color32::GRAY));
                ui.add_space(6.0);
                if self.textures.is_some() {
                    ui.horizontal(|ui| {
                        ui.label(format!("Texture {} of TEXTU0-{} at rows 0, 4, … 60, 63:", self.sample_tex, self.set));
                        ui.add(egui::DragValue::new(&mut self.sample_tex).range(0..=255));
                    });
                    if self.lit.as_ref().is_none_or(|(t, g, _)| *t != self.sample_tex || *g != self.generation) {
                        if let Some(img) = self.lit_image() {
                            let t = ui.ctx().load_texture(format!("{}#lit", self.rel), img, egui::TextureOptions::NEAREST);
                            self.lit = Some((self.sample_tex, self.generation, t));
                        }
                    }
                    if let Some((_, _, t)) = &self.lit {
                        let [w, h] = t.size();
                        let s = (ui.available_width() / w as f32).min(2.0);
                        ui.image((t.id(), Vec2::new(w as f32, h as f32) * s));
                    }
                }
            }
            View::Blend => {
                let s = avail.x.min(avail.y).clamp(256.0, 1024.0);
                let r = ui.add(egui::Image::new((tex, Vec2::splat(s))).sense(Sense::hover()));
                if let Some(p) = r.hover_pos() {
                    let a = (((p.y - r.rect.top()) / s * 256.0) as usize).min(255);
                    let b = (((p.x - r.rect.left()) / s * 256.0) as usize).min(255);
                    let o = self.data[16384 + a * 256 + b];
                    r.on_hover_text(format!("blend[{a}][{b}] = {o}: sprite colour {a} (row) over screen colour {b} (column)"));
                }
                ui.label(RichText::new("blend[a][b]: translucent sprites, the front-end text box and $G text. Rows are a, columns b; the shipped table is about a third of a plus two thirds of b.").small().color(Color32::GRAY));
            }
            View::Flat => {
                let s = (avail.y / 64.0).clamp(4.0, 12.0);
                let r = ui.add(egui::Image::new((tex, Vec2::new(32.0 * s, 64.0 * s))).sense(Sense::hover()));
                if let Some(p) = r.hover_pos() {
                    let x = (((p.x - r.rect.left()) / s) as usize).min(31);
                    let y = (((p.y - r.rect.top()) / s) as usize).min(63);
                    let (v, t) = (y / 16, (y % 16) * 32 + x);
                    r.on_hover_text(format!("variant {v}, texture {t}: colour {}", self.data[81920 + v * 512 + t]));
                }
                ui.label(RichText::new("Four variants (16 rows each) of one colour per texture id, used for the terrain when texture mapping is off.").small().color(Color32::GRAY));
            }
        }
    }

    fn side_ui(&mut self, ui: &mut egui::Ui) {
        if ui.button("Test in game").on_hover_text(format!("Race on track {}", self.set + 1)).clicked() {
            self.test = true;
        }
        ui.label(RichText::new(format!("Used by track {} (TEXTU0-{}, SKY0-{}); TABLE0-0 also colours the menus.", self.set + 1, self.set, self.set)).small().color(Color32::GRAY));
        ui.separator();
        ui.heading("Regenerate");
        ui.label(RichText::new("After editing the palette, rebuild the tables from it so terrain shading and translucency use the new colours.").small().color(Color32::GRAY));
        ui.horizontal(|ui| {
            ui.label("Haze colour");
            let (r, _) = ui.allocate_exact_size(Vec2::new(28.0, 18.0), Sense::hover());
            ui.painter().rect_filled(r, 3.0, Color32::from_rgb(vga8(self.haze[0]), vga8(self.haze[1]), vga8(self.haze[2])));
        });
        for (k, name) in ["red", "green", "blue"].iter().enumerate() {
            ui.add(egui::Slider::new(&mut self.haze[k], 0..=63).text(*name));
        }
        if ui.button("Use the table's haze").clicked() {
            self.haze = ColourTable::from_bytes(&self.data).haze(&self.pal);
        }
        ui.add_space(4.0);
        let haze = self.haze;
        if ui.button("Light rows (palette + haze)").clicked() {
            self.change("regenerate light rows", |t, p, _| t.set_light(&ColourTable::light_rows(p, haze)));
        }
        if ui.button("Blend table (palette)").clicked() {
            self.change("regenerate blend table", |t, p, _| t.set_blend(&ColourTable::blend_table(p)));
        }
        let have_tex = self.textures.is_some();
        if ui.add_enabled(have_tex, egui::Button::new(format!("Flat colours (TEXTU0-{})", self.set))).on_hover_text("Each texture's average colour, for all four variants").clicked() {
            self.change("regenerate flat colours", |t, p, tex| t.set_flat(&ColourTable::flat_colours(p, tex.unwrap_or(&[]))));
        }
        if ui.button("All of them").clicked() {
            self.change("regenerate colour table", |t, p, tex| {
                t.set_light(&ColourTable::light_rows(p, haze));
                t.set_blend(&ColourTable::blend_table(p));
                if let Some(tex) = tex {
                    t.set_flat(&ColourTable::flat_colours(p, tex));
                }
            });
        }
        if ui.button("Reload palette and textures").on_hover_text("Read DATA/PALET0-0.DAT and the textures again from disk (save the palette first)").clicked() {
            if let Ok(p) = Palette::load() {
                self.rgb = palette_rgb(&p);
                self.pal = p;
            }
            self.textures = c8_data::images::load_terrain_textures(self.set).ok();
            self.generation += 1;
            self.message = "reloaded".into();
        }
        if !self.message.is_empty() {
            ui.label(RichText::new(&self.message).small());
        }
        ui.separator();
        ui.label(RichText::new("The regenerated light rows match 96 % of the shipped entries and the blend table 95 % (the rest are near ties); the shipped flat colours were tuned by hand and differ slightly.").small().color(Color32::GRAY));
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
    fn regenerate_and_undo() {
        let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources"));
        c8_data::files::set_data_dir(root.clone());
        let Ok(mut d) = TableDoc::open(root.join("DATA/TABLE0-2.DAT"), "DATA/TABLE0-2.DAT".into()) else { return };
        let orig = d.data.clone();
        let haze = d.haze;
        d.change("light", |t, p, _| t.set_light(&ColourTable::light_rows(p, haze)));
        assert!(d.dirty());
        // the regenerated rows keep row 32 as the identity and row 0 as the haze
        assert!((0..256).all(|i| d.data[32 * 256 + i] == i as u8));
        assert_eq!(ColourTable::from_bytes(&d.data).haze(&d.pal), d.haze);
        d.change("flat", |t, p, tex| t.set_flat(&ColourTable::flat_colours(p, tex.unwrap())));
        d.undo();
        d.undo();
        assert_eq!(d.data, orig);
        assert!(!d.dirty());
        assert_eq!(d.game_files().unwrap()[0].0, "DATA/TABLE0-2.DAT");
    }
}
