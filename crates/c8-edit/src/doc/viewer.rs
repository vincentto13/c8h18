//! Viewer for resource kinds that have no editor yet: file facts plus a preview where the format
//! is simple (palettes, terrain textures, skies, language files). Editors replace it kind by kind.
use super::Document;
use crate::resources::Kind;
use eframe::egui::{self, Color32, TextureHandle};
use c8_data::images::Palette;
use std::path::{Path, PathBuf};

pub struct Viewer {
    path: PathBuf,
    rel: String,
    kind: Kind,
    size: usize,
    packed: bool,
    data: Vec<u8>,
    text: Option<String>,
    preview: Option<(usize, usize, Vec<u8>)>,
    image: Option<TextureHandle>,
}

fn rgb_palette() -> [[u8; 3]; 256] {
    Palette::load().map(|p| p.vga.map(|c| c.map(|v| (v as u32 * 255 / 63) as u8))).unwrap_or_else(|_| std::array::from_fn(|i| [i as u8; 3]))
}

impl Viewer {
    pub fn open(path: PathBuf, kind: Kind, rel: String) -> Result<Viewer, String> {
        let raw = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let packed = c8_data::rnc::is_rnc(&raw);
        let size = raw.len();
        let data = c8_data::rnc::maybe_unpack(raw).map_err(|e| format!("{rel}: {e}"))?;
        let text = (kind == Kind::Strings).then(|| String::from_utf8_lossy(&data).to_string());
        // palette-index preview: (width, height, indices)
        let preview = match kind {
            Kind::TerrainTextures if data.len() >= 1 << 20 => {
                // 256 textures of 64×64 laid out 16 × 16
                let mut img = vec![0u8; 1024 * 1024];
                for t in 0..256 {
                    let (tx, ty) = (t % 16 * 64, t / 16 * 64);
                    for y in 0..64 {
                        let src = t * 4096 + y * 64;
                        let dst = (ty + y) * 1024 + tx;
                        img[dst..dst + 64].copy_from_slice(&data[src..src + 64]);
                    }
                }
                Some((1024, 1024, img))
            }
            Kind::Sky if data.len() >= 65536 => Some((256, 256, data[..65536].to_vec())),
            Kind::Palette => Some((16, 16, (0..=255u8).collect())),
            _ => None,
        };
        Ok(Viewer { path, rel, kind, size, packed, data, text, preview, image: None })
    }
}

impl Document for Viewer {
    fn title(&self) -> String {
        self.rel.clone()
    }
    fn kind(&self) -> Kind {
        self.kind
    }
    fn path(&self) -> Option<&Path> {
        Some(&self.path)
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        if let Some(t) = &self.text {
            egui::ScrollArea::both().show(ui, |ui| {
                ui.add(egui::Label::new(egui::RichText::new(t).monospace()).extend());
            });
            return;
        }
        if let Some((w, h, idx)) = &self.preview {
            let tex = self.image.get_or_insert_with(|| {
                let pal = if self.kind == Kind::Palette {
                    Palette::from_bytes(&self.data).vga.map(|c| c.map(|v| (v as u32 * 255 / 63) as u8))
                } else {
                    rgb_palette()
                };
                let rgba: Vec<u8> = idx.iter().flat_map(|&i| [pal[i as usize][0], pal[i as usize][1], pal[i as usize][2], 255]).collect();
                ui.ctx().load_texture(self.rel.clone(), egui::ColorImage::from_rgba_unmultiplied([*w, *h], &rgba), egui::TextureOptions::NEAREST)
            });
            egui::ScrollArea::both().show(ui, |ui| {
                let scale = if *w <= 16 { 24.0 } else if *w <= 256 { 3.0 } else { 1.0 };
                ui.image((tex.id(), egui::vec2(*w as f32, *h as f32) * scale));
            });
            return;
        }
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new(format!("{}: no viewer or editor for this resource kind yet", self.kind.label())).color(Color32::GRAY));
        });
    }
    fn side_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("File");
        egui::Grid::new("file").num_columns(2).show(ui, |ui| {
            ui.label("kind");
            ui.label(self.kind.label());
            ui.end_row();
            ui.label("size");
            ui.label(format!("{} bytes{}", self.size, if self.packed { format!(" (RNC, {} unpacked)", self.data.len()) } else { String::new() }));
            ui.end_row();
            ui.label("path");
            ui.label(self.path.display().to_string());
            ui.end_row();
        });
        ui.label("Read-only for now.");
    }
}
