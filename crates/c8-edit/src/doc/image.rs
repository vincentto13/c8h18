//! Pixel editor for every palette-image resource: terrain textures, skies, screens, model texture
//! atlases, billboards (TMAPS), sprite sheets and fonts. A document is a list of frames (one
//! texture, glyph, sprite or picture each); the format decides how frames are read and written.
use super::history::{Command, History};
use super::pixels::{self, Frame, Quantizer, Rgb};
use super::Document;
use crate::resources::Kind;
use eframe::egui::{self, Color32, Key, Pos2, Rect, Sense, Stroke, TextureHandle, Vec2};
use c8_data::sprites::SpriteSheet;
use c8_data::tmaps::{TmapImage, Tmaps};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Frame clipboard, shared by all image documents.
static CLIPBOARD: Mutex<Option<Frame>> = Mutex::new(None);

/// Files the port does not load (the 320×200 sets and unused sheets).
const UNUSED_BY_PORT: [&str; 12] = ["olfnt0-0", "osfnt0-0", "pfont0-0", "panel0-0", "logo0-0", "track0-0", "oscr0-0", "onet0-0", "hspr0-0", "mspr0-0", "point0-0", "tex0-1"];

enum Format {
    /// frames of w × h stored one after another (terrain textures, sky, screens)
    Raw,
    /// one raw image 256 wide; the TAB of texture rectangles is kept as it is
    Atlas { tab: Vec<u8>, rects: Vec<[u32; 4]> },
    /// RLE sprite sheet or font, DAT + TAB
    Sheet,
    /// TMAPS.DAT + TAB: per-image sprite mode and animation group
    Tmaps { modes: Vec<u8>, groups: Vec<u16> },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Pencil,
    Eraser,
    Fill,
    Pick,
}

pub struct ImageDoc {
    path: PathBuf,
    rel: String,
    kind: Kind,
    /// lower-case file stem, e.g. "olfnt0-1"
    stem: String,
    format: Format,
    font: bool,
    mask_font: bool,
    frames: Vec<Frame>,
    history: History<Vec<Frame>>,
    sel: usize,
    tool: Tool,
    colour: u8,
    brush: i32,
    zoom: f32,
    fit_pending: bool,
    fit_avail: Vec2,
    grid: bool,
    pal: Rgb,
    tex: Vec<Option<TextureHandle>>,
    /// frame before the current stroke
    stroke: Option<Frame>,
    last: Option<(i32, i32)>,
    new_size: (u32, u32),
    text: String,
    text_tex: Option<(String, u64, TextureHandle)>,
    /// bumped on every change, for caches
    generation: u64,
    message: String,
    test: bool,
}

fn find_tab(dat: &Path) -> Option<PathBuf> {
    let stem = dat.file_stem()?.to_string_lossy().to_ascii_lowercase();
    std::fs::read_dir(dat.parent()?).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.file_stem().is_some_and(|s| s.to_string_lossy().to_ascii_lowercase() == stem) && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("tab"))
    })
}

/// The TAB written next to a DAT, matching the DAT extension's case.
fn tab_path(dat: &Path) -> PathBuf {
    let upper = dat.extension().is_some_and(|e| e.to_string_lossy().chars().any(|c| c.is_ascii_uppercase()));
    dat.with_extension(if upper { "TAB" } else { "tab" })
}

/// The number after "0-" in names like TEXTU0-3.
fn set_number(stem: &str) -> Option<usize> {
    stem.rsplit_once("0-").and_then(|(_, n)| n.parse().ok())
}

impl ImageDoc {
    pub fn open(path: PathBuf, kind: Kind, rel: String) -> Result<ImageDoc, String> {
        let raw = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut data = c8_data::rnc::maybe_unpack(raw).map_err(|e| format!("{rel}: {e}"))?;
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let read_tab = || -> Result<Vec<u8>, String> {
            let t = find_tab(&path).ok_or_else(|| format!("{rel}: the .TAB file next to it is missing"))?;
            std::fs::read(&t).map_err(|e| format!("{}: {e}", t.display()))
        };
        let raw_frames = |data: &mut Vec<u8>, w: u32, h: u32, count: Option<usize>| -> Vec<Frame> {
            let size = (w * h) as usize;
            let n = count.unwrap_or((data.len() / size).max(1));
            data.resize(n * size, 0);
            data.chunks(size).map(|c| Frame { w, h, pix: c.to_vec(), mask: vec![true; size] }).collect()
        };
        let (format, frames) = match kind {
            Kind::TerrainTextures => (Format::Raw, raw_frames(&mut data, 64, 64, Some(256))),
            Kind::Sky => (Format::Raw, raw_frames(&mut data, 256, 256, Some(1))),
            Kind::Image => {
                let (w, h) = match stem.as_str() {
                    "puzzle" => (112, 96),
                    _ if data.len() >= 307200 => (640, 480),
                    _ => (320, 200),
                };
                (Format::Raw, raw_frames(&mut data, w, h, Some(1)))
            }
            Kind::ModelTextures => {
                let tab = read_tab().unwrap_or_default();
                let rects = tab.chunks_exact(6).map(|e| {
                    let o = u32::from_le_bytes([e[0], e[1], e[2], e[3]]);
                    [o % 256, o / 256, e[4] as u32, e[5] as u32]
                });
                let rects = rects.collect();
                let h = (data.len() / 256).max(1) as u32;
                (Format::Atlas { tab, rects }, raw_frames(&mut data, 256, h, Some(1)))
            }
            Kind::Sprites | Kind::Font => (Format::Sheet, SpriteSheet::decode(&data, &read_tab()?).sprites),
            Kind::Billboards => {
                let t = Tmaps::decode(&data, &read_tab()?)?;
                let frames = t.images.iter().map(|i| Frame { w: i.w, h: i.h, pix: i.pix.clone(), mask: vec![true; i.pix.len()] }).collect();
                (Format::Tmaps { modes: t.images.iter().map(|i| i.mode).collect(), groups: t.groups }, frames)
            }
            _ => return Err(format!("{rel}: not an image resource")),
        };
        let font = kind == Kind::Font;
        let mask_font = font && (stem.starts_with("olfnt") || stem.starts_with("osfnt"));
        let pal = if mask_font { pixels::mask_palette() } else { pixels::game_palette() };
        let sel = if font { 1 + b'A' as usize } else { 0 }.min(frames.len().saturating_sub(1));
        let n = frames.len();
        let mut d = ImageDoc {
            path,
            rel,
            kind,
            stem,
            format,
            font,
            mask_font,
            frames,
            history: History::default(),
            sel,
            tool: Tool::Pencil,
            colour: if mask_font { 1 } else { 15 },
            brush: 1,
            zoom: 8.0,
            fit_pending: true,
            fit_avail: Vec2::ZERO,
            grid: true,
            pal,
            tex: vec![None; n],
            stroke: None,
            last: None,
            new_size: (0, 0),
            text: "C8H18".into(),
            text_tex: None,
            generation: 0,
            message: String::new(),
            test: false,
        };
        d.select(sel);
        Ok(d)
    }

    /// Does this format have transparent pixels?
    fn transparency(&self) -> bool {
        matches!(self.format, Format::Sheet)
    }
    /// Can frames change size?
    fn resizable(&self) -> bool {
        matches!(self.format, Format::Sheet | Format::Tmaps { .. })
    }
    /// Billboards treat colour 0 as transparent.
    fn zero_clear(&self) -> bool {
        matches!(self.format, Format::Tmaps { .. })
    }
    fn colours(&self) -> Vec<u8> {
        if self.mask_font { vec![0, 1] } else { (0..=255).collect() }
    }

    fn select(&mut self, i: usize) {
        self.sel = i.min(self.frames.len().saturating_sub(1));
        if let Some(f) = self.frames.get(self.sel) {
            self.new_size = (f.w, f.h);
        }
        self.fit_pending = true;
    }

    fn changed(&mut self, i: Option<usize>) {
        self.generation += 1;
        match i {
            Some(i) => self.tex[i] = None,
            None => self.tex.iter_mut().for_each(|t| *t = None),
        }
        if let Some(f) = self.frames.get(self.sel) {
            self.new_size = (f.w, f.h);
        }
    }

    fn texture(&mut self, ctx: &egui::Context, i: usize) -> Option<egui::TextureId> {
        let f = self.frames.get(i)?;
        if f.w == 0 || f.h == 0 {
            return None;
        }
        if self.tex[i].is_none() {
            let img = pixels::frame_image(f, &self.pal, self.zero_clear());
            self.tex[i] = Some(ctx.load_texture(format!("{}#{i}", self.rel), img, egui::TextureOptions::NEAREST));
        }
        self.tex[i].as_ref().map(|t| t.id())
    }

    /// Replace frame `i` as one undo step.
    fn set_frame(&mut self, i: usize, after: Frame, label: &str) {
        let before = self.frames[i].clone();
        if before == after {
            return;
        }
        self.history.run(Box::new(FrameEdit { i, before, after, label: label.into() }), &mut self.frames);
        self.changed(Some(i));
    }

    fn end_stroke(&mut self) {
        self.last = None;
        if let Some(before) = self.stroke.take() {
            let after = self.frames[self.sel].clone();
            self.frames[self.sel] = before;
            let label = match self.tool {
                Tool::Pencil => "draw",
                Tool::Eraser => "erase",
                Tool::Fill => "fill",
                Tool::Pick => "pick",
            };
            self.set_frame(self.sel, after, label);
        }
    }

    fn paint(&mut self, p: (i32, i32), erase: bool) {
        let from = self.last.unwrap_or(p);
        let (b, transparency, colour) = (self.brush, self.transparency(), self.colour);
        let f = &mut self.frames[self.sel];
        for (x, y) in pixels::line(from, p) {
            for dy in 0..b {
                for dx in 0..b {
                    let (px, py) = (x + dx - b / 2, y + dy - b / 2);
                    if px >= 0 && py >= 0 && (px as u32) < f.w && (py as u32) < f.h {
                        let i = (py as u32 * f.w + px as u32) as usize;
                        if erase && transparency {
                            f.mask[i] = false;
                        } else {
                            f.pix[i] = if erase { 0 } else { colour };
                            f.mask[i] = true;
                        }
                    }
                }
            }
        }
        self.last = Some(p);
        self.changed(Some(self.sel));
    }

    fn pick(&mut self, p: (i32, i32)) {
        let f = &self.frames[self.sel];
        let i = (p.1 as u32 * f.w + p.0 as u32) as usize;
        if f.mask[i] {
            self.colour = f.pix[i];
            if self.tool == Tool::Eraser || self.tool == Tool::Pick {
                self.tool = Tool::Pencil;
            }
        } else {
            self.tool = Tool::Eraser;
        }
    }

    // ---- frame operations -----------------------------------------------------------------

    fn flip(&mut self, horizontal: bool) {
        let f = &self.frames[self.sel];
        let (w, h) = (f.w, f.h);
        let n = pixels::transformed(f, w, h, |x, y| if horizontal { (w - 1 - x, y) } else { (x, h - 1 - y) });
        self.set_frame(self.sel, n, if horizontal { "flip horizontally" } else { "flip vertically" });
    }

    fn rotate(&mut self) {
        let f = &self.frames[self.sel];
        if f.w != f.h && !self.resizable() {
            self.message = "only square frames can be rotated here".into();
            return;
        }
        let (w, h) = (f.w, f.h);
        let n = pixels::transformed(f, h, w, |x, y| (y, h - 1 - x));
        self.set_frame(self.sel, n, "rotate");
    }

    fn clear(&mut self) {
        let f = &self.frames[self.sel];
        let mut n = f.clone();
        if self.transparency() {
            n.mask.iter_mut().for_each(|m| *m = false);
        } else {
            n.pix.iter_mut().for_each(|p| *p = self.colour);
        }
        self.set_frame(self.sel, n, "clear");
    }

    fn resize(&mut self, w: u32, h: u32) {
        let f = &self.frames[self.sel];
        let n = pixels::resized(f, w, h, self.transparency(), 0);
        self.set_frame(self.sel, n, "resize");
    }

    fn crop_to_content(&mut self) {
        let f = &self.frames[self.sel];
        match pixels::content_box(f) {
            Some((x, y, w, h)) => {
                let n = pixels::crop(f, x, y, w, h);
                self.set_frame(self.sel, n, "crop");
            }
            None => self.message = "the frame is empty".into(),
        }
    }

    fn copy(&mut self) {
        *CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner()) = Some(self.frames[self.sel].clone());
        self.message = format!("copied frame {}", self.sel);
    }

    fn paste(&mut self) {
        let Some(c) = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner()).clone() else { return };
        let f = &self.frames[self.sel];
        let n = if self.resizable() {
            c
        } else {
            // keep the frame size; transparent clipboard pixels keep the old ones
            let mut n = f.clone();
            for y in 0..f.h.min(c.h) {
                for x in 0..f.w.min(c.w) {
                    let s = (y * c.w + x) as usize;
                    if c.mask[s] {
                        n.pix[(y * f.w + x) as usize] = c.pix[s];
                    }
                }
            }
            n
        };
        self.set_frame(self.sel, n, "paste");
    }

    // ---- PNG ----------------------------------------------------------------------------------

    /// Layout of "all frames" PNGs: 16 columns of cells as large as the largest frame.
    fn sheet_layout(&self) -> (u32, u32, u32) {
        let cw = self.frames.iter().map(|f| f.w).max().unwrap_or(1).max(1);
        let ch = self.frames.iter().map(|f| f.h).max().unwrap_or(1).max(1);
        let cols = if self.frames.len() > 1 { 16 } else { 1 };
        (cw, ch, cols)
    }

    pub fn export_frame(&self, path: &Path) -> Result<(), String> {
        let f = &self.frames[self.sel];
        pixels::write_png(path, f.w, f.h, &pixels::frame_rgba(f, &self.pal, self.zero_clear()))
    }

    pub fn import_frame(&mut self, path: &Path) -> Result<(), String> {
        let (w, h, rgba) = pixels::read_png(path)?;
        let mut q = Quantizer::new(&self.pal, self.colours());
        let f = &self.frames[self.sel];
        let mut n = if self.resizable() {
            let max = if matches!(self.format, Format::Sheet) { 255 } else { 1024 };
            pixels::resized(f, w.min(max), h.min(max), self.transparency(), 0)
        } else {
            f.clone()
        };
        if self.transparency() {
            n.mask.iter_mut().for_each(|m| *m = false);
        }
        pixels::paste_rgba(&mut n, w, &rgba, 0, 0, self.transparency(), &mut q);
        self.set_frame(self.sel, n, "import PNG");
        Ok(())
    }

    pub fn export_all(&self, path: &Path) -> Result<(), String> {
        let (cw, ch, cols) = self.sheet_layout();
        let rows = self.frames.len().div_ceil(cols as usize) as u32;
        let (w, h) = (cw * cols, ch * rows);
        let mut out = vec![0u8; (w * h * 4) as usize];
        for (i, f) in self.frames.iter().enumerate() {
            let (ox, oy) = (i as u32 % cols * cw, i as u32 / cols * ch);
            let rgba = pixels::frame_rgba(f, &self.pal, self.zero_clear());
            for y in 0..f.h {
                let s = (y * f.w * 4) as usize;
                let d = (((oy + y) * w + ox) * 4) as usize;
                out[d..d + (f.w * 4) as usize].copy_from_slice(&rgba[s..s + (f.w * 4) as usize]);
            }
        }
        pixels::write_png(path, w, h, &out)
    }

    /// Import a PNG in the layout of [`Self::export_all`]; frames keep their sizes.
    pub fn import_all(&mut self, path: &Path) -> Result<(), String> {
        let (w, _h, rgba) = pixels::read_png(path)?;
        let (cw, ch, cols) = self.sheet_layout();
        if w != cw * cols {
            return Err(format!("the PNG must be {} pixels wide ({cols} columns of {cw}×{ch}), as written by \"Export all\"", cw * cols));
        }
        let mut q = Quantizer::new(&self.pal, self.colours());
        let before = self.frames.clone();
        let mut after = before.clone();
        for (i, f) in after.iter_mut().enumerate() {
            if self.transparency() {
                f.mask.iter_mut().for_each(|m| *m = false);
            }
            pixels::paste_rgba(f, w, &rgba, i as u32 % cols * cw, i as u32 / cols * ch, self.transparency(), &mut q);
        }
        if before != after {
            self.history.run(Box::new(AllFrames { before, after }), &mut self.frames);
            self.changed(None);
        }
        Ok(())
    }

    // ---- writing --------------------------------------------------------------------------

    /// (DAT bytes, TAB bytes if the format has one), unpacked.
    fn encode(&self) -> (Vec<u8>, Option<Vec<u8>>) {
        match &self.format {
            Format::Raw => (self.frames.iter().flat_map(|f| f.pix.iter().copied()).collect(), None),
            Format::Atlas { tab, .. } => (self.frames[0].pix.clone(), (!tab.is_empty()).then(|| tab.clone())),
            Format::Sheet => {
                let (dat, tab) = SpriteSheet { sprites: self.frames.clone() }.encode();
                (dat, Some(tab))
            }
            Format::Tmaps { modes, groups } => {
                let images = self.frames.iter().enumerate().map(|(i, f)| TmapImage { mode: modes.get(i).copied().unwrap_or(0x15), w: f.w, h: f.h, pix: f.pix.clone() });
                let (dat, tab) = Tmaps { images: images.collect(), groups: groups.clone() }.encode();
                (dat, Some(tab))
            }
        }
    }

    fn game_dir(&self) -> &'static str {
        if self.kind == Kind::ModelTextures { "OBJECTS/DATA" } else { "DATA" }
    }

    // ---- drawing ----------------------------------------------------------------------------

    fn frame_label(&self, i: usize) -> String {
        if self.font {
            if i == 0 {
                return "–".into();
            }
            let c = (i - 1) as u8;
            return match pixels::cp437(c) {
                Some(ch) if ch.is_alphanumeric() || ch.is_ascii_graphic() => format!("{ch}"),
                _ => format!("{c:02X}"),
            };
        }
        i.to_string()
    }

    fn frame_strip(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let cell = if self.font || matches!(self.format, Format::Sheet) { 40.0 } else { 56.0 };
        let mut clicked = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(3.0);
                for i in 0..self.frames.len() {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(cell, cell + 12.0), Sense::click());
                    let selected = i == self.sel;
                    let p = ui.painter_at(rect);
                    p.rect_filled(rect, 3.0, if selected { Color32::from_rgb(60, 90, 140) } else { Color32::from_gray(35) });
                    let f = &self.frames[i];
                    let (w, h) = (f.w as f32, f.h as f32);
                    if let Some(id) = self.texture(&ctx, i) {
                        let s = ((cell - 6.0) / w.max(h)).min(if w.max(h) < cell / 2.0 { ((cell - 6.0) / w.max(h)).floor() } else { 99.0 });
                        let r = Rect::from_center_size(Pos2::new(rect.center().x, rect.min.y + cell / 2.0), Vec2::new(w, h) * s);
                        p.image(id, r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                    }
                    p.text(Pos2::new(rect.center().x, rect.max.y - 1.0), egui::Align2::CENTER_BOTTOM, self.frame_label(i), egui::FontId::proportional(10.0), Color32::LIGHT_GRAY);
                    if resp.clicked() {
                        clicked = Some(i);
                    }
                    resp.on_hover_text(self.frame_tooltip(i));
                }
            });
        });
        if let Some(i) = clicked {
            self.end_stroke();
            self.select(i);
        }
    }

    fn frame_tooltip(&self, i: usize) -> String {
        let f = &self.frames[i];
        let what = if self.font && i > 0 {
            let c = (i - 1) as u8;
            format!("character {c} (0x{c:02X}){}", pixels::cp437(c).map(|ch| format!(" '{ch}'")).unwrap_or_default())
        } else {
            format!("frame {i}")
        };
        format!("{what}, {} × {}", f.w, f.h)
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let mut tools = vec![(Tool::Pencil, "Pencil", "B")];
            if self.transparency() {
                tools.push((Tool::Eraser, "Eraser", "E (makes pixels transparent)"));
            }
            tools.push((Tool::Fill, "Fill", "G"));
            tools.push((Tool::Pick, "Pick", "I, or right-click / Alt+click with any tool"));
            for (t, name, key) in tools {
                if ui.selectable_label(self.tool == t, name).on_hover_text(key).clicked() {
                    self.tool = t;
                }
            }
            ui.separator();
            ui.label("brush");
            ui.add(egui::DragValue::new(&mut self.brush).range(1..=16));
            ui.separator();
            ui.label("zoom");
            ui.add(egui::DragValue::new(&mut self.zoom).range(1.0..=64.0).speed(0.2).fixed_decimals(0));
            if ui.button("Fit").clicked() {
                self.fit_pending = true;
            }
            ui.checkbox(&mut self.grid, "grid");
            ui.separator();
            if ui.button("Flip H").clicked() {
                self.flip(true);
            }
            if ui.button("Flip V").clicked() {
                self.flip(false);
            }
            if ui.button("Rotate").on_hover_text("90° clockwise").clicked() {
                self.rotate();
            }
            if ui.button("Clear").clicked() {
                self.clear();
            }
            if ui.button("Copy").on_hover_text("⌘C: copy this frame").clicked() {
                self.copy();
            }
            if ui.button("Paste").on_hover_text("⌘V: paste into this frame").clicked() {
                self.paste();
            }
        });
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let Some(f) = self.frames.get(self.sel) else { return };
        let (w, h) = (f.w, f.h);
        if w == 0 || h == 0 {
            ui.centered_and_justified(|ui| {
                ui.label("This entry is empty. Give it a size in the side panel (Resize) to draw it.");
            });
            return;
        }
        let avail = ui.available_size();
        if self.fit_pending {
            // panels settle during the first frames: refit until the space stops changing
            self.fit_pending = self.fit_avail != avail;
            self.fit_avail = avail;
            self.zoom = ((avail.x - 20.0) / w as f32).min((avail.y - 20.0) / h as f32).floor().clamp(1.0, 64.0);
        }
        let id = self.texture(&ctx, self.sel);
        let size = Vec2::new(w as f32, h as f32) * self.zoom;
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            let (area, resp) = ui.allocate_exact_size(size.max(ui.available_size()), Sense::click_and_drag());
            let img = Rect::from_center_size(area.center(), size);
            let p = ui.painter_at(area);
            if let Some(id) = id {
                p.image(id, img, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            }
            p.rect_stroke(img.expand(1.0), 0.0, Stroke::new(1.0, Color32::from_gray(90)), egui::StrokeKind::Outside);
            let z = self.zoom;
            if self.grid && z >= 6.0 {
                let s = Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 60));
                for x in 1..w {
                    let sx = img.min.x + x as f32 * z;
                    p.line_segment([Pos2::new(sx, img.min.y), Pos2::new(sx, img.max.y)], s);
                }
                for y in 1..h {
                    let sy = img.min.y + y as f32 * z;
                    p.line_segment([Pos2::new(img.min.x, sy), Pos2::new(img.max.x, sy)], s);
                }
            }
            if let Format::Atlas { rects, .. } = &self.format {
                for (i, r) in rects.iter().enumerate() {
                    if r[2] == 0 || r[3] == 0 {
                        continue;
                    }
                    let rr = Rect::from_min_size(img.min + Vec2::new(r[0] as f32, r[1] as f32) * z, Vec2::new(r[2] as f32, r[3] as f32) * z);
                    p.rect_stroke(rr, 0.0, Stroke::new(1.0, Color32::from_rgb(255, 200, 0)), egui::StrokeKind::Inside);
                    p.text(rr.min + Vec2::splat(2.0), egui::Align2::LEFT_TOP, i.to_string(), egui::FontId::monospace(10.0), Color32::YELLOW);
                }
            }
            if self.font {
                // baseline helpers: the glyph box and its advance
                p.text(img.right_top() + Vec2::new(4.0, 0.0), egui::Align2::LEFT_TOP, format!("{w}×{h}"), egui::FontId::proportional(11.0), Color32::GRAY);
            }
            // zoom with pinch / ctrl+wheel around the pointer
            if resp.hovered() {
                let dz = ui.input(|i| i.zoom_delta());
                if dz != 1.0 {
                    self.zoom = (self.zoom * dz).clamp(1.0, 64.0);
                }
            }
            let hover = resp.hover_pos().map(|pos| (((pos.x - img.min.x) / z).floor() as i32, ((pos.y - img.min.y) / z).floor() as i32));
            let inside = hover.filter(|&(x, y)| x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h);
            if let Some((x, y)) = inside {
                let b = if matches!(self.tool, Tool::Pencil | Tool::Eraser) { self.brush } else { 1 };
                let r = Rect::from_min_size(img.min + Vec2::new((x - b / 2) as f32, (y - b / 2) as f32) * z, Vec2::splat(b as f32 * z));
                p.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Outside);
                let f = &self.frames[self.sel];
                let i = (y as u32 * w + x as u32) as usize;
                let v = if f.mask[i] { format!("colour {}", f.pix[i]) } else { "transparent".into() };
                self.message = format!("{x}, {y}: {v}");
            }
            let (primary, secondary, alt) = ui.input(|i| (i.pointer.primary_down(), i.pointer.secondary_down(), i.modifiers.alt));
            let down_here = resp.is_pointer_button_down_on();
            if let (Some(pt), true) = (inside, down_here && (secondary || (primary && alt))) {
                self.pick(pt);
            } else if let (Some(pt), true) = (inside, down_here && primary) {
                let first = self.stroke.is_none();
                if first {
                    self.stroke = Some(self.frames[self.sel].clone());
                }
                match self.tool {
                    Tool::Pencil => self.paint(pt, false),
                    Tool::Eraser => self.paint(pt, true),
                    Tool::Fill if first => {
                        let v = Some(self.colour);
                        pixels::flood_fill(&mut self.frames[self.sel], pt.0 as u32, pt.1 as u32, v);
                        self.changed(Some(self.sel));
                    }
                    Tool::Pick => self.pick(pt),
                    _ => {}
                }
            } else if !primary && self.stroke.is_some() {
                self.end_stroke();
            }
        });
    }

    fn keys(&mut self, ui: &mut egui::Ui) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let (copy, paste) = ui.input(|i| {
            let mut c = false;
            let mut v = false;
            for e in &i.events {
                match e {
                    egui::Event::Copy => c = true,
                    egui::Event::Paste(_) => v = true,
                    _ => {}
                }
            }
            (c, v)
        });
        if copy {
            self.copy();
        }
        if paste {
            self.paste();
        }
        ui.input(|i| {
            if i.modifiers.command {
                return;
            }
            if i.key_pressed(Key::B) {
                self.tool = Tool::Pencil;
            }
            if i.key_pressed(Key::E) && self.transparency() {
                self.tool = Tool::Eraser;
            }
            if i.key_pressed(Key::G) {
                self.tool = Tool::Fill;
            }
            if i.key_pressed(Key::I) {
                self.tool = Tool::Pick;
            }
        });
        let n = self.frames.len();
        let step = ui.input(|i| {
            if i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::CloseBracket) {
                1
            } else if i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::OpenBracket) {
                -1
            } else {
                0
            }
        });
        if step != 0 && n > 0 {
            self.end_stroke();
            self.select((self.sel as i64 + step).rem_euclid(n as i64) as usize);
        }
    }

    fn palette_ui(&mut self, ui: &mut egui::Ui) {
        let colours = self.colours();
        let cols = if colours.len() <= 2 { 2 } else { 16 };
        let s = if cols == 2 { 32.0 } else { ((ui.available_width() - 4.0) / cols as f32).floor().clamp(8.0, 17.0) };
        egui::Grid::new("palette").spacing(Vec2::ZERO).min_col_width(0.0).min_row_height(0.0).show(ui, |ui| {
            for (k, &c) in colours.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(s), Sense::click());
                let rgb = self.pal[c as usize];
                ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
                if c == self.colour && self.tool != Tool::Eraser {
                    ui.painter().rect_stroke(rect, 0.0, Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Inside);
                    ui.painter().rect_stroke(rect.shrink(2.0), 0.0, Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Inside);
                }
                if resp.clicked() {
                    self.colour = c;
                    if self.tool == Tool::Eraser || self.tool == Tool::Pick {
                        self.tool = Tool::Pencil;
                    }
                }
                let name = if self.mask_font { if c == 1 { "1: text colour (any non-zero value)" } else { "0: outline colour" }.to_string() } else { format!("{c}: {}, {}, {}", rgb[0], rgb[1], rgb[2]) };
                resp.on_hover_text(name);
                if (k + 1) % cols == 0 {
                    ui.end_row();
                }
            }
        });
        let rgb = self.pal[self.colour as usize];
        ui.label(format!("colour {} ({}, {}, {})", self.colour, rgb[0], rgb[1], rgb[2]));
    }

    /// The preview text drawn with this font.
    fn text_preview(&mut self, ui: &mut egui::Ui) {
        ui.add(egui::TextEdit::singleline(&mut self.text).hint_text("preview text"));
        let stale = self.text_tex.as_ref().is_none_or(|(t, g, _)| t != &self.text || *g != self.generation);
        if stale {
            let bytes: Vec<u8> = self.text.chars().map(|c| pixels::to_cp437(c).unwrap_or(b'?')).collect();
            let glyph = |c: u8| self.frames.get(c as usize + 1);
            let w: u32 = bytes.iter().filter_map(|&c| glyph(c)).map(|g| g.w).sum::<u32>().max(1);
            let h: u32 = bytes.iter().filter_map(|&c| glyph(c)).map(|g| g.h).max().unwrap_or(1).max(1);
            let bg = Color32::from_rgb(30, 40, 70);
            let mut img = egui::ColorImage::new([w as usize, h as usize], vec![bg; (w * h) as usize]);
            let mut x0 = 0;
            for &c in &bytes {
                let Some(g) = glyph(c) else { continue };
                for y in 0..g.h {
                    for x in 0..g.w {
                        if let Some(v) = g.get(x, y) {
                            let rgb = if self.mask_font { if v != 0 { [255, 230, 90] } else { [20, 20, 20] } } else { self.pal[v as usize] };
                            img.pixels[(y * w + x0 + x) as usize] = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                        }
                    }
                }
                x0 += g.w;
            }
            let t = ui.ctx().load_texture(format!("{}#text", self.rel), img, egui::TextureOptions::NEAREST);
            self.text_tex = Some((self.text.clone(), self.generation, t));
        }
        if let Some((_, _, t)) = &self.text_tex {
            let [w, h] = t.size();
            let s = (ui.available_width() / w as f32).min(3.0).max(0.25);
            let s = if s >= 1.0 { s.floor() } else { s };
            ui.image((t.id(), Vec2::new(w as f32, h as f32) * s));
        }
    }

    fn png_ui(&mut self, ui: &mut egui::Ui) {
        let name = format!("{}-{}.png", self.stem, self.sel);
        ui.horizontal_wrapped(|ui| {
            if ui.button("Export frame…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name(&name).save_file() {
                    self.message = self.export_frame(&p).map(|_| format!("wrote {}", p.display())).unwrap_or_else(|e| e);
                }
            }
            if ui.button("Import frame…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("PNG", &["png"]).pick_file() {
                    self.message = self.import_frame(&p).map(|_| "imported (colours matched to the palette)".into()).unwrap_or_else(|e| e);
                }
            }
            if self.frames.len() > 1 {
                if ui.button("Export all…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name(format!("{}.png", self.stem)).save_file() {
                        self.message = self.export_all(&p).map(|_| format!("wrote {}", p.display())).unwrap_or_else(|e| e);
                    }
                }
                if ui.button("Import all…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("PNG", &["png"]).pick_file() {
                        self.message = self.import_all(&p).map(|_| "imported all frames".into()).unwrap_or_else(|e| e);
                    }
                }
            }
        });
        let (cw, ch, cols) = self.sheet_layout();
        let what = if self.frames.len() > 1 { format!("\"Export all\" writes a grid of {cols} columns of {cw}×{ch} cells; edit it in any paint program and import it back. ") } else { String::new() };
        ui.label(egui::RichText::new(format!("{what}Imported colours are matched to the nearest palette colour{}.", if self.transparency() { "; transparent PNG pixels stay transparent" } else { "" })).small().color(Color32::GRAY));
    }
}

struct FrameEdit {
    i: usize,
    before: Frame,
    after: Frame,
    label: String,
}

impl Command<Vec<Frame>> for FrameEdit {
    fn apply(&mut self, f: &mut Vec<Frame>) {
        f[self.i] = self.after.clone();
    }
    fn revert(&mut self, f: &mut Vec<Frame>) {
        f[self.i] = self.before.clone();
    }
    fn label(&self) -> String {
        format!("{} (frame {})", self.label, self.i)
    }
}

struct AllFrames {
    before: Vec<Frame>,
    after: Vec<Frame>,
}

impl Command<Vec<Frame>> for AllFrames {
    fn apply(&mut self, f: &mut Vec<Frame>) {
        f.clone_from(&self.after);
    }
    fn revert(&mut self, f: &mut Vec<Frame>) {
        f.clone_from(&self.before);
    }
    fn label(&self) -> String {
        "import all frames".into()
    }
}

impl Document for ImageDoc {
    fn title(&self) -> String {
        self.rel.clone()
    }
    fn kind(&self) -> Kind {
        self.kind
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
        let (dat, tab) = self.encode();
        std::fs::write(path, &dat).map_err(|e| format!("{}: {e}", path.display()))?;
        if let Some(tab) = tab {
            let t = tab_path(path);
            std::fs::write(&t, &tab).map_err(|e| format!("{}: {e}", t.display()))?;
        }
        self.path = path.to_path_buf();
        self.history.mark_saved();
        Ok(())
    }
    fn undo(&mut self) {
        self.end_stroke();
        self.history.undo(&mut self.frames);
        self.changed(None);
    }
    fn redo(&mut self) {
        self.history.redo(&mut self.frames);
        self.changed(None);
    }
    fn can_undo(&self) -> bool {
        self.history.can_undo()
    }
    fn can_redo(&self) -> bool {
        self.history.can_redo()
    }
    fn game_files(&self) -> Result<Vec<(String, Vec<u8>)>, String> {
        let (dat, tab) = self.encode();
        let name = self.stem.to_ascii_uppercase();
        let mut out = vec![(format!("{}/{name}.DAT", self.game_dir()), dat)];
        if let Some(tab) = tab {
            out.push((format!("{}/{name}.TAB", self.game_dir()), tab));
        }
        Ok(out)
    }
    fn game_args(&self) -> Vec<String> {
        let race = |t: usize| vec!["--no-intro".to_string(), "--race".into(), t.to_string()];
        match self.kind {
            Kind::TerrainTextures | Kind::Sky => race(set_number(&self.stem).unwrap_or(0) + 1),
            Kind::Billboards | Kind::ModelTextures => race(1),
            Kind::Image if self.stem == "title" => vec![],
            _ => vec!["--no-intro".into()],
        }
    }
    fn wants_test(&mut self) -> bool {
        std::mem::take(&mut self.test)
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        self.keys(ui);
        self.toolbar(ui);
        ui.separator();
        if self.frames.len() > 1 {
            let w = if self.font || matches!(self.format, Format::Sheet) { 320.0 } else { 270.0 };
            egui::Panel::left("frames").default_size(w).resizable(true).show(ui, |ui| self.frame_strip(ui));
        }
        egui::CentralPanel::default().show(ui, |ui| self.canvas(ui));
    }

    fn side_ui(&mut self, ui: &mut egui::Ui) {
        if ui.button("Test in game").on_hover_text("Start the game with this file (and every other open edited file) in place of the original. The originals are not changed.").clicked() {
            self.test = true;
        }
        if UNUSED_BY_PORT.contains(&self.stem.as_str()) {
            ui.label(egui::RichText::new("The game does not load this file (it belongs to the 320×200 set or is unused). Edit the …0-1 file instead where there is one.").small().color(Color32::from_rgb(230, 180, 80)));
        }
        ui.separator();
        ui.heading(if self.font { "Glyph" } else { "Frame" });
        ui.label(self.frame_tooltip(self.sel));
        if let Format::Tmaps { modes, groups } = &self.format {
            ui.label(format!("sprite mode 0x{:02X}, animation group {}", modes.get(self.sel).copied().unwrap_or(0), groups.get(self.sel).copied().unwrap_or(0)));
            ui.label(egui::RichText::new("Colour 0 is transparent.").small().color(Color32::GRAY));
        }
        if self.resizable() {
            let max = if matches!(self.format, Format::Sheet) { 255 } else { 1024 };
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut self.new_size.0).range(0..=max).prefix("w "));
                ui.add(egui::DragValue::new(&mut self.new_size.1).range(0..=max).prefix("h "));
                let f = &self.frames[self.sel];
                if ui.add_enabled(self.new_size != (f.w, f.h), egui::Button::new("Resize")).clicked() {
                    self.resize(self.new_size.0, self.new_size.1);
                }
            });
            if self.transparency() && ui.button("Crop to content").on_hover_text("Shrink the frame to its opaque pixels").clicked() {
                self.crop_to_content();
            }
            if self.font {
                ui.label(egui::RichText::new("A glyph's width is its advance: add or remove transparent columns on the right to change spacing.").small().color(Color32::GRAY));
            }
        }
        ui.separator();
        ui.heading("Colour");
        self.palette_ui(ui);
        if self.font {
            ui.separator();
            ui.heading("Preview");
            self.text_preview(ui);
        }
        ui.separator();
        ui.heading("PNG");
        self.png_ui(ui);
        ui.separator();
        if !self.message.is_empty() {
            ui.label(egui::RichText::new(&self.message).small());
        }
        ui.collapsing("History", |ui| {
            for l in self.history.labels().collect::<Vec<_>>().iter().rev().take(30) {
                ui.label(egui::RichText::new(l).small());
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn res(rel: &str) -> Option<PathBuf> {
        let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources"));
        c8_data::files::set_data_dir(root.clone());
        c8_data::files::resolve(rel)
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("c8-edit-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d.join(name)
    }

    #[test]
    fn font_edit_save_reload() {
        let Some(p) = res("data/olfnt0-1.dat") else { return };
        let mut d = ImageDoc::open(p, Kind::Font, "DATA/OLFNT0-1.DAT".into()).unwrap();
        assert!(d.mask_font && d.frames.len() == 242);
        d.select(1 + b'A' as usize);
        let (w, h) = (d.frames[d.sel].w, d.frames[d.sel].h);
        d.resize(w + 3, h);
        d.stroke = Some(d.frames[d.sel].clone());
        d.paint((w as i32 + 1, 0), false);
        d.end_stroke();
        assert_eq!(d.history.labels().count(), 2);
        let out = tmp("OLFNT0-1.DAT");
        d.save_to(&out).unwrap();
        assert!(tab_path(&out).exists());
        let e = ImageDoc::open(out, Kind::Font, "OLFNT0-1.DAT".into()).unwrap();
        assert_eq!(e.frames, d.frames);
        let g = &e.frames[1 + b'A' as usize];
        assert_eq!((g.w, g.get(w + 1, 0)), (w + 3, Some(1)));
        // undo back to the original glyph
        d.undo();
        d.undo();
        assert_eq!(d.frames[d.sel].w, w);
    }

    #[test]
    fn png_round_trip_and_textures() {
        let Some(p) = res("data/osfnt0-1.dat") else { return };
        let mut d = ImageDoc::open(p, Kind::Font, "DATA/OSFNT0-1.DAT".into()).unwrap();
        let all = tmp("osfnt.png");
        d.export_all(&all).unwrap();
        let before = d.frames.clone();
        d.import_all(&all).unwrap();
        // the game draws every non-zero mask-font pixel the same way
        let norm = |f: &[Frame]| f.iter().map(|g| (g.w, g.h, g.mask.clone(), g.pix.iter().zip(&g.mask).map(|(&p, &m)| m && p != 0).collect::<Vec<_>>())).collect::<Vec<_>>();
        assert_eq!(norm(&d.frames), norm(&before), "export + import changed the glyphs");
        let one = tmp("glyph.png");
        d.select(1 + b'W' as usize);
        d.export_frame(&one).unwrap();
        d.select(1 + b'X' as usize);
        d.import_frame(&one).unwrap();
        assert_eq!(norm(&d.frames[1 + b'X' as usize..][..1]), norm(&before[1 + b'W' as usize..][..1]));

        let Some(p) = res("data/textu0-0.dat") else { return };
        let mut t = ImageDoc::open(p, Kind::TerrainTextures, "DATA/TEXTU0-0.DAT".into()).unwrap();
        assert_eq!(t.frames.len(), 256);
        t.select(5);
        t.colour = 3;
        t.stroke = Some(t.frames[5].clone());
        t.tool = Tool::Fill;
        pixels::flood_fill(&mut t.frames[5], 0, 0, Some(3));
        t.end_stroke();
        let files = t.game_files().unwrap();
        assert_eq!(files[0].0, "DATA/TEXTU0-0.DAT");
        assert_eq!(files[0].1.len(), 1 << 20);
        assert_eq!(files[0].1[5 * 4096], 3);
        assert_eq!(t.game_args(), ["--no-intro", "--race", "1"]);
    }

    #[test]
    fn billboards_and_atlas_round_trip() {
        if let Some(p) = res("data/tmaps.dat") {
            let mut d = ImageDoc::open(p, Kind::Billboards, "DATA/TMAPS.DAT".into()).unwrap();
            d.flip(true);
            let out = tmp("TMAPS.DAT");
            d.save_to(&out).unwrap();
            let e = ImageDoc::open(out, Kind::Billboards, "TMAPS.DAT".into()).unwrap();
            assert_eq!(e.frames, d.frames);
        }
        if let Some(p) = res("objects/data/tex0-0.dat") {
            let d = ImageDoc::open(p.clone(), Kind::ModelTextures, "OBJECTS/DATA/TEX0-0.DAT".into()).unwrap();
            let (dat, tab) = d.encode();
            assert_eq!(dat, std::fs::read(&p).unwrap());
            assert!(tab.is_some_and(|t| t.len() == 51 * 6));
        }
    }
}
