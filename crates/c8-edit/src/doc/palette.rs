//! Palette editor (`DATA/PALET0-0.DAT`, 256 colours of 6-bit VGA values).
use super::history::{Command, History};
use super::pixels::vga8;
use super::Document;
use crate::resources::Kind;
use eframe::egui::{self, Color32, Sense, Stroke, Vec2};
use std::path::{Path, PathBuf};

type Vga = [[u8; 3]; 256];

pub struct PaletteDoc {
    path: PathBuf,
    rel: String,
    vga: Vga,
    history: History<Vga>,
    sel: usize,
    /// the selected colour's value before the current edit; the edit becomes one undo step
    orig: Option<[u8; 3]>,
    test: bool,
}

impl PaletteDoc {
    pub fn open(path: PathBuf, rel: String) -> Result<PaletteDoc, String> {
        let d = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let vga = c8_data::images::Palette::from_bytes(&d).vga;
        Ok(PaletteDoc { path, rel, vga, history: History::default(), sel: 0, orig: None, test: false })
    }
    fn set(&mut self, i: usize, c: [u8; 3]) {
        let before = self.vga[i];
        if before != c {
            self.history.run(Box::new(ColourEdit { i, before, after: c }), &mut self.vga);
        }
    }
    /// Record the edit in progress as one undo step.
    fn commit(&mut self) {
        if let Some(o) = self.orig.take() {
            let c = self.vga[self.sel];
            self.vga[self.sel] = o;
            self.set(self.sel, c);
        }
    }
    fn bytes(&self) -> Vec<u8> {
        self.vga.iter().flatten().copied().collect()
    }
}

struct ColourEdit {
    i: usize,
    before: [u8; 3],
    after: [u8; 3],
}

impl Command<Vga> for ColourEdit {
    fn apply(&mut self, v: &mut Vga) {
        v[self.i] = self.after;
    }
    fn revert(&mut self, v: &mut Vga) {
        v[self.i] = self.before;
    }
    fn label(&self) -> String {
        format!("colour {}", self.i)
    }
}

fn rgb8(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(vga8(c[0]), vga8(c[1]), vga8(c[2]))
}

impl Document for PaletteDoc {
    fn title(&self) -> String {
        self.rel.clone()
    }
    fn kind(&self) -> Kind {
        Kind::Palette
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
        std::fs::write(path, self.bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
        self.path = path.to_path_buf();
        self.history.mark_saved();
        Ok(())
    }
    fn undo(&mut self) {
        self.commit();
        self.history.undo(&mut self.vga);
    }
    fn redo(&mut self) {
        self.history.redo(&mut self.vga);
    }
    fn can_undo(&self) -> bool {
        self.history.can_undo()
    }
    fn can_redo(&self) -> bool {
        self.history.can_redo()
    }
    fn game_files(&self) -> Result<Vec<(String, Vec<u8>)>, String> {
        Ok(vec![("DATA/PALET0-0.DAT".into(), self.bytes())])
    }
    fn game_args(&self) -> Vec<String> {
        vec!["--no-intro".into()]
    }
    fn wants_test(&mut self) -> bool {
        std::mem::take(&mut self.test)
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        let s = ((ui.available_width().min(ui.available_height()) - 20.0) / 16.0).floor().clamp(12.0, 48.0);
        egui::Grid::new("palette-grid").spacing(Vec2::splat(2.0)).show(ui, |ui| {
            for i in 0..256 {
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(s), Sense::click());
                ui.painter().rect_filled(rect, 2.0, rgb8(self.vga[i]));
                if i == self.sel {
                    ui.painter().rect_stroke(rect, 2.0, Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Outside);
                }
                if resp.clicked() {
                    self.commit();
                    self.sel = i;
                }
                let c = self.vga[i];
                resp.on_hover_text(format!("{i}: {} {} {} (VGA 0..63)", c[0], c[1], c[2]));
                if i % 16 == 15 {
                    ui.end_row();
                }
            }
        });
    }

    fn side_ui(&mut self, ui: &mut egui::Ui) {
        if ui.button("Test in game").clicked() {
            self.test = true;
        }
        ui.separator();
        ui.heading(format!("Colour {}", self.sel));
        let mut c = self.vga[self.sel];
        let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::hover());
        ui.painter().rect_filled(r, 4.0, rgb8(c));
        let mut changed = false;
        for (k, name) in ["red", "green", "blue"].iter().enumerate() {
            changed |= ui.add(egui::Slider::new(&mut c[k], 0..=63).text(*name)).changed();
        }
        let mut rgb = [vga8(c[0]), vga8(c[1]), vga8(c[2])];
        if egui::color_picker::color_edit_button_srgb(ui, &mut rgb).changed() {
            c = rgb.map(|v| v >> 2);
            changed = true;
        }
        if changed {
            self.orig.get_or_insert(self.vga[self.sel]);
            self.vga[self.sel] = c;
        } else if self.orig.is_some() && !ui.input(|i| i.pointer.any_down()) {
            self.commit();
        }
        ui.separator();
        ui.label(egui::RichText::new("The game keeps 6 bits per channel (0..63). Terrain lighting and translucency come from the colour tables (TABLE0-N), which are built for the original palette and are not recalculated here, so large changes look wrong in races. The menus and sprites use the palette directly.").small().color(Color32::GRAY));
        ui.collapsing("History", |ui| {
            for l in self.history.labels().collect::<Vec<_>>().iter().rev().take(30) {
                ui.label(egui::RichText::new(l).small());
            }
        });
    }
}

