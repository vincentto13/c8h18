//! Level document: a top-down map of a `LEVEL0-N.DAT` with overlays, a cell inspector and the
//! first editing tool (terrain height brush). More tools follow the plan in docs/editor-plan.md.
use super::history::{Command, History};
use super::Document;
use crate::resources::Kind;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, TextureHandle, Vec2};
use c8_data::images::{load_terrain_textures, ColourTable, Palette};
use c8_data::level::{Level, MAP_H, MAP_W};
use std::path::{Path, PathBuf};

/// Pixels per cell in the map image.
const PX: usize = 4;

#[derive(Clone, Copy, PartialEq)]
enum Base {
    Textures,
    Height,
    Light,
}

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Inspect,
    Height,
}

/// Texture colours of the level's set, for drawing the map: per texture a PX×PX thumbnail.
struct Look {
    set: usize,
    pal: [[u8; 3]; 256],
    table: Option<ColourTable>,
    /// 256 thumbnails of PX×PX palette indices
    thumbs: Vec<[u8; PX * PX]>,
}

impl Look {
    fn load(set: usize) -> Look {
        let pal = Palette::load().map(|p| p.vga.map(|c| c.map(|v| (v as u32 * 255 / 63) as u8))).unwrap_or([[128; 3]; 256]);
        let table = ColourTable::load(set).ok();
        let tex = load_terrain_textures(set).unwrap_or_else(|_| vec![0; 1 << 20]);
        let step = 64 / PX;
        let thumbs = (0..256)
            .map(|t| {
                let mut th = [0u8; PX * PX];
                for y in 0..PX {
                    for x in 0..PX {
                        th[y * PX + x] = tex[t * 4096 + (y * step + step / 2) * 64 + x * step + step / 2];
                    }
                }
                th
            })
            .collect();
        Look { set, pal, table, thumbs }
    }
    fn rgb(&self, idx: u8, light_row: usize) -> [u8; 3] {
        let i = match &self.table {
            Some(t) => t.data[light_row.min(63) * 256 + idx as usize],
            None => idx,
        };
        self.pal[i as usize]
    }
}

pub struct LevelDoc {
    path: PathBuf,
    rel: String,
    level: Level,
    history: History<Level>,
    look: Look,
    image: Option<TextureHandle>,
    image_dirty: bool,
    base: Base,
    show_columns: bool,
    show_entities: bool,
    show_regions: bool,
    show_grid: bool,
    tool: Tool,
    brush_radius: i32,
    brush_step: f32,
    zoom: f32,
    pan: Vec2,
    hover: Option<(i32, i32)>,
    /// corners changed by the current brush stroke (one undo step per stroke)
    stroke: Option<HeightEdit>,
    /// the view has been fitted to the track once
    fitted: bool,
}

impl LevelDoc {
    pub fn open(path: PathBuf, rel: String) -> Result<LevelDoc, String> {
        let level = Level::read_file(&path)?;
        // LEVEL0-N uses texture set and colour table N-1
        let name = path.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let set = name.strip_prefix("level0-").and_then(|n| n.parse::<usize>().ok()).map(|n| n.saturating_sub(1)).unwrap_or(0).min(5);
        Ok(LevelDoc {
            path,
            rel,
            level,
            history: History::default(),
            look: Look::load(set),
            image: None,
            image_dirty: true,
            base: Base::Textures,
            show_columns: true,
            show_entities: true,
            show_regions: true,
            show_grid: false,
            tool: Tool::Inspect,
            brush_radius: 2,
            brush_step: 0.25,
            zoom: 3.0,
            pan: Vec2::ZERO,
            hover: None,
            stroke: None,
            fitted: false,
        })
    }

    fn render(&self) -> egui::ColorImage {
        let (w, h) = (MAP_W * PX, MAP_H * PX);
        let mut rgba = vec![0u8; w * h * 4];
        let lvl = &self.level;
        let max_h = lvl.cells.iter().map(|c| c.height).max().unwrap_or(1).max(1) as f32;
        for cy in 0..MAP_H {
            for cx in 0..MAP_W {
                let c = lvl.cell(cx as i32, cy as i32);
                let light_row = (c.light.max(0) >> 8) as usize;
                // slope shading for the height view: light from the north-west
                let shade = {
                    let d = lvl.corner_h(cx as i32, cy as i32) - lvl.corner_h(cx as i32 + 1, cy as i32 + 1);
                    (1.0 + d * 0.8).clamp(0.4, 1.6)
                };
                let tex = lvl.floor_tex(cx as i32, cy as i32) as usize & 255;
                for py in 0..PX {
                    for px in 0..PX {
                        let col = match self.base {
                            Base::Textures => self.look.rgb(self.look.thumbs[tex][py * PX + px], light_row),
                            Base::Height => {
                                let v = (c.height as f32 / max_h * 200.0 + 30.0) * shade;
                                let v = v.clamp(0.0, 255.0) as u8;
                                [v, v, v]
                            }
                            Base::Light => {
                                let v = (light_row * 4).min(255) as u8;
                                [v, v, (v / 2).saturating_add(60)]
                            }
                        };
                        let o = ((cy * PX + py) * w + cx * PX + px) * 4;
                        rgba[o..o + 3].copy_from_slice(&col);
                        rgba[o + 3] = 255;
                    }
                }
            }
        }
        egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba)
    }

    fn to_screen(&self, origin: Pos2, x: f32, y: f32) -> Pos2 {
        origin + Vec2::new(x, y) * self.zoom - self.pan
    }

    fn brush(&mut self, cx: i32, cy: i32, lower: bool) {
        let r = self.brush_radius;
        let step = (self.brush_step * 256.0) as i32 * if lower { -1 } else { 1 };
        let mut changes = Vec::new();
        for y in cy - r..=cy + r {
            for x in cx - r..=cx + r {
                if x < 0 || y < 0 || x >= MAP_W as i32 || y >= MAP_H as i32 {
                    continue;
                }
                let d2 = ((x - cx).pow(2) + (y - cy).pow(2)) as f32;
                let fall = (1.0 - d2.sqrt() / (r as f32 + 1.0)).max(0.0);
                if fall <= 0.0 {
                    continue;
                }
                let i = y as usize * MAP_W + x as usize;
                let old = self.level.cells[i].height;
                let new = (old as i32 + (step as f32 * fall) as i32).clamp(0, 65535) as u16;
                if new != old {
                    changes.push((i, old, new));
                }
            }
        }
        if changes.is_empty() {
            return;
        }
        // apply now, merge into the stroke; the whole stroke becomes one undo step
        for &(i, _, new) in &changes {
            self.level.cells[i].height = new;
        }
        let st = self.stroke.get_or_insert_with(|| HeightEdit { changes: Vec::new() });
        for (i, old, new) in changes {
            match st.changes.iter_mut().find(|c| c.0 == i) {
                Some(c) => c.2 = new,
                None => st.changes.push((i, old, new)),
            }
        }
        self.image_dirty = true;
    }

    fn end_stroke(&mut self) {
        if let Some(st) = self.stroke.take() {
            // already applied: record without applying again
            let mut c: Box<dyn Command<Level>> = Box::new(st);
            c.revert(&mut self.level);
            self.history.run(c, &mut self.level);
        }
    }

    fn draw_overlays(&self, p: &egui::Painter, origin: Pos2) {
        let z = self.zoom;
        let lvl = &self.level;
        let cell_rect = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(self.to_screen(origin, x, y), Vec2::new(w, h) * z);
        if self.show_columns {
            for cy in 0..MAP_H as i32 {
                for cx in 0..MAP_W as i32 {
                    if let Some(col) = lvl.column(cx, cy) {
                        let n = col.mask.count_ones() as u8;
                        let tunnel = col.mask & 1 == 0 && col.mask != 0;
                        let c = if tunnel { Color32::from_rgba_unmultiplied(80, 160, 255, 90) } else { Color32::from_rgba_unmultiplied(255, 120, 40, 40 + n.min(8) * 15) };
                        p.rect_filled(cell_rect(cx as f32, cy as f32, 1.0, 1.0), 0.0, c);
                    }
                }
            }
        }
        if self.show_regions {
            for r in &lvl.regions {
                let c = match r.kind {
                    1 => Color32::from_rgb(80, 220, 80),
                    2 => Color32::from_rgb(80, 140, 255),
                    3 => Color32::from_rgb(255, 220, 60),
                    _ => Color32::WHITE,
                };
                let rect = Rect::from_min_max(self.to_screen(origin, r.cx - r.hw, r.cy - r.hh), self.to_screen(origin, r.cx + r.hw, r.cy + r.hh));
                p.rect_stroke(rect, 0.0, Stroke::new(2.0, c), egui::StrokeKind::Inside);
            }
        }
        if self.show_entities {
            let pos = |i: usize| lvl.entities.iter().find(|e| e.index == i).map(|e| (e.x as f32 + 0.5, e.y as f32 + 0.5));
            for e in &lvl.entities {
                let (x, y) = (e.x as f32 + 0.5, e.y as f32 + 0.5);
                let s = self.to_screen(origin, x, y);
                match (e.kind, e.sub) {
                    (9, sub) => {
                        if let Some((lx, ly)) = pos(e.link as usize) {
                            let col = if sub == 0 { Color32::from_rgb(255, 60, 60) } else if sub == 6 { Color32::from_rgb(255, 0, 255) } else { Color32::from_rgb(0, 230, 230) };
                            let t = self.to_screen(origin, lx, ly);
                            p.line_segment([s, t], Stroke::new(if sub == 0 { 2.0 } else { 1.5 }, col));
                            if sub != 0 {
                                p.circle_filled(t, 2.0, col);
                            }
                        }
                    }
                    (1, _) => {
                        let r = cell_rect(e.x as f32, e.y as f32, e.w as f32 + 1.0, e.h as f32 + 1.0);
                        p.rect_stroke(r, 0.0, Stroke::new(2.0, Color32::YELLOW), egui::StrokeKind::Inside);
                        p.text(r.center(), egui::Align2::CENTER_CENTER, e.value.to_string(), egui::FontId::proportional(12.0), Color32::YELLOW);
                    }
                    (5, _) => {
                        p.circle_filled(s, (z * 0.35).max(3.0), Color32::from_rgb(60, 255, 60));
                    }
                    (10, _) => {
                        p.circle_filled(s, (z * 0.45).max(4.0), Color32::from_rgb(255, 150, 0));
                    }
                    (8, _) => {
                        p.circle_stroke(s, (z * 0.4).max(3.0), Stroke::new(1.5, Color32::from_rgb(255, 80, 255)));
                    }
                    (3, 3) => {
                        p.circle_stroke(s, 2.5, Stroke::new(1.0, Color32::LIGHT_GRAY));
                    }
                    _ => {
                        p.circle_filled(s, 2.0, Color32::from_rgb(200, 200, 200));
                    }
                }
            }
        }
        if self.show_grid && z >= 6.0 {
            let col = Color32::from_rgba_unmultiplied(0, 0, 0, 60);
            for x in 0..=MAP_W {
                p.line_segment([self.to_screen(origin, x as f32, 0.0), self.to_screen(origin, x as f32, MAP_H as f32)], Stroke::new(1.0, col));
            }
            for y in 0..=MAP_H {
                p.line_segment([self.to_screen(origin, 0.0, y as f32), self.to_screen(origin, MAP_W as f32, y as f32)], Stroke::new(1.0, col));
            }
        }
        if let Some((hx, hy)) = self.hover {
            let r = if self.tool == Tool::Height { self.brush_radius as f32 } else { 0.0 };
            let rect = Rect::from_min_max(self.to_screen(origin, hx as f32 - r, hy as f32 - r), self.to_screen(origin, hx as f32 + 1.0 + r, hy as f32 + 1.0 + r));
            p.rect_stroke(rect, 0.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Outside);
        }
    }
}

struct HeightEdit {
    /// (cell index, old height, new height)
    changes: Vec<(usize, u16, u16)>,
}

impl Command<Level> for HeightEdit {
    fn apply(&mut self, l: &mut Level) {
        for &(i, _, new) in &self.changes {
            l.cells[i].height = new;
        }
    }
    fn revert(&mut self, l: &mut Level) {
        for &(i, old, _) in &self.changes {
            l.cells[i].height = old;
        }
    }
    fn label(&self) -> String {
        format!("height brush ({} corners)", self.changes.len())
    }
}

impl Document for LevelDoc {
    fn title(&self) -> String {
        self.rel.clone()
    }
    fn kind(&self) -> Kind {
        Kind::Level
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
        self.level.write_file(path)?;
        self.path = path.to_path_buf();
        self.rel = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        self.history.mark_saved();
        Ok(())
    }
    fn undo(&mut self) {
        self.history.undo(&mut self.level);
        self.image_dirty = true;
    }
    fn redo(&mut self) {
        self.history.redo(&mut self.level);
        self.image_dirty = true;
    }
    fn can_undo(&self) -> bool {
        self.history.can_undo()
    }
    fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        if self.image_dirty || self.image.is_none() {
            let img = self.render();
            match &mut self.image {
                Some(t) => t.set(img, egui::TextureOptions::NEAREST),
                None => self.image = Some(ui.ctx().load_texture("level-map", img, egui::TextureOptions::NEAREST)),
            }
            self.image_dirty = false;
        }
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        painter.rect_filled(rect, 0.0, Color32::from_gray(24));
        // zoom about the pointer (pinch, ctrl+wheel), pan with wheel / right or middle drag
        let (zoom_delta, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
        if resp.hovered() {
            if let Some(ptr) = resp.hover_pos() {
                if zoom_delta != 1.0 {
                    let before = (ptr - rect.min + self.pan) / self.zoom;
                    self.zoom = (self.zoom * zoom_delta).clamp(1.0, 64.0);
                    self.pan = before * self.zoom - (ptr - rect.min);
                } else {
                    self.pan -= scroll;
                }
            }
        }
        if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
            self.pan -= resp.drag_delta();
        }
        if !self.fitted {
            // fit the track (plus a margin) into the view
            let (w, h) = ((self.level.max_x.max(16) + 4) as f32, (self.level.max_y.max(16) + 4) as f32);
            self.zoom = (rect.width() / w).min(rect.height() / h).clamp(1.0, 64.0);
            self.pan = Vec2::new(-2.0, -2.0) * self.zoom;
            self.fitted = true;
        }
        let origin = rect.min;
        let map = Rect::from_min_size(self.to_screen(origin, 0.0, 0.0), Vec2::new(MAP_W as f32, MAP_H as f32) * self.zoom);
        let painter = painter.with_clip_rect(rect);
        if let Some(t) = &self.image {
            painter.image(t.id(), map, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        self.hover = resp.hover_pos().map(|p| (p - origin + self.pan) / self.zoom).filter(|c| c.x >= 0.0 && c.y >= 0.0 && c.x < MAP_W as f32 && c.y < MAP_H as f32).map(|c| (c.x as i32, c.y as i32));
        // tools
        if self.tool == Tool::Height {
            let primary = resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_down());
            if primary {
                if let Some((x, y)) = self.hover {
                    let lower = ui.input(|i| i.modifiers.shift || i.modifiers.alt);
                    self.brush(x, y, lower);
                }
            } else {
                self.end_stroke();
            }
        }
        self.draw_overlays(&painter, origin);
    }

    fn side_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("View");
        let base_before = self.base;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.base, Base::Textures, "Textures");
            ui.selectable_value(&mut self.base, Base::Height, "Height");
            ui.selectable_value(&mut self.base, Base::Light, "Light");
        });
        ui.checkbox(&mut self.show_columns, "Block columns (blue = tunnel)");
        ui.checkbox(&mut self.show_entities, "Entities, walls, waypoints");
        ui.checkbox(&mut self.show_regions, "Chargers and start area");
        ui.checkbox(&mut self.show_grid, "Grid (when zoomed in)");
        let mut set = self.look.set;
        egui::ComboBox::from_label("Texture set").selected_text(format!("{set}")).show_ui(ui, |ui| {
            for s in 0..6 {
                ui.selectable_value(&mut set, s, format!("{s}"));
            }
        });
        if set != self.look.set {
            self.look = Look::load(set);
            self.image_dirty = true;
        }
        if self.base != base_before {
            self.image_dirty = true;
        }

        ui.separator();
        ui.heading("Tool");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tool, Tool::Inspect, "Inspect");
            ui.selectable_value(&mut self.tool, Tool::Height, "Height brush");
        });
        if self.tool == Tool::Height {
            ui.add(egui::Slider::new(&mut self.brush_radius, 0..=12).text("radius (cells)"));
            ui.add(egui::Slider::new(&mut self.brush_step, 0.05..=2.0).text("step (cells)"));
            ui.label("Drag to raise, Shift+drag to lower.");
        }

        ui.separator();
        ui.heading("Cell");
        if let Some((x, y)) = self.hover {
            let c = *self.level.cell(x, y);
            egui::Grid::new("cell").num_columns(2).show(ui, |ui| {
                ui.label("position");
                ui.label(format!("{x}, {y}"));
                ui.end_row();
                ui.label("height");
                ui.label(format!("{:.3} cells", c.height as f32 / 256.0));
                ui.end_row();
                ui.label("light row");
                ui.label(format!("{}", c.light >> 8));
                ui.end_row();
                ui.label("texture");
                ui.label(format!("{} (orientation {})", self.level.floor_tex(x, y), c.orient));
                ui.end_row();
                ui.label("attributes");
                ui.label(format!("{:#06x}", self.level.attr(x, y)));
                ui.end_row();
                if let Some(col) = self.level.column(x, y) {
                    ui.label("column");
                    ui.label(format!("{} mask {:016b}", -(c.tex as i32), col.mask));
                    ui.end_row();
                }
                for e in self.level.entities.iter().filter(|e| e.x == x && e.y == y) {
                    ui.label(format!("entity {}", e.index));
                    ui.label(format!("type {} / {}, group {}, link {}, value {}", e.kind, e.sub, e.group, e.link, e.value));
                    ui.end_row();
                }
            });
        } else {
            ui.label("Point at the map.");
        }

        ui.separator();
        ui.heading("Level");
        ui.label(format!(
            "{} entities, {} regions, {} columns in use",
            self.level.entities.len(),
            self.level.regions.len(),
            {
                let mut used = std::collections::HashSet::new();
                for c in &self.level.cells {
                    if c.tex < 0 {
                        used.insert(c.tex);
                    }
                }
                used.len()
            }
        ));
        ui.collapsing("History", |ui| {
            for l in self.history.labels() {
                ui.label(l);
            }
        });
    }
}
