//! Blocks tool: paint and erase block columns on cells, edit column definitions (which levels are
//! solid, which block sits on each) and block definitions (face textures and orientations).
use super::LevelDoc;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Vec2};
use c8_data::level::{Block, Column, MAP_W, MAX_BLOCKS, MAX_COLUMNS};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum ColMode {
    Select,
    Paint,
    Erase,
}

const FACES: [&str; 6] = ["north", "east", "south", "west", "top", "bottom"];
/// `face_sel` value meaning "the column's floor texture"
const FLOOR: usize = 6;

impl LevelDoc {
    /// Cells that use column id.
    fn column_users(&self, id: usize) -> usize {
        self.level.cells.iter().filter(|c| c.tex < 0 && -(c.tex as i32) as usize == id).count()
    }

    fn free_column(&self) -> Option<usize> {
        let mut used = vec![false; MAX_COLUMNS];
        for c in self.level.cells.iter().filter(|c| c.tex < 0) {
            used[(-(c.tex as i32) as usize).min(MAX_COLUMNS - 1)] = true;
        }
        (1..self.level.columns.len().min(MAX_COLUMNS)).find(|&i| !used[i] && self.level.columns[i] == Column::default())
    }

    fn free_block(&self) -> Option<usize> {
        let mut used = vec![false; MAX_BLOCKS];
        for c in &self.level.columns {
            for &b in &c.blocks {
                if b > 0 {
                    used[(b as usize).min(MAX_BLOCKS - 1)] = true;
                }
            }
        }
        (1..self.level.blocks.len().min(MAX_BLOCKS)).find(|&i| !used[i] && self.level.blocks[i] == Block::default())
    }

    pub(super) fn blocks_input(&mut self, resp: &egui::Response, down: bool, alt: bool) {
        let Some((x, y)) = self.hover else {
            if !down {
                self.end_stroke("columns");
            }
            return;
        };
        let pick = |s: &Self| {
            let t = s.level.cell(x, y).tex;
            (t < 0).then(|| -(t as i32) as usize)
        };
        if alt || self.col_mode == ColMode::Select {
            if resp.clicked() {
                self.col_sel = pick(self);
                if let Some(id) = self.col_sel {
                    self.blk_sel = self.level.columns.get(id).and_then(|c| c.blocks.iter().find(|&&b| b > 0)).map(|&b| b as usize);
                }
            }
            return;
        }
        if !down {
            self.end_stroke(if self.col_mode == ColMode::Paint { "paint column" } else { "erase column" });
            return;
        }
        let cells = self.brush_cells(x, y);
        match self.col_mode {
            ColMode::Paint => {
                let Some(id) = self.col_sel else {
                    self.message = "select a column first (click one on the map, or New column)".into();
                    return;
                };
                for (i, _) in cells {
                    self.edit_cell(i, |c| c.tex = -(id as i16));
                }
            }
            ColMode::Erase => {
                for (i, _) in cells {
                    let t = self.level.cells[i].tex;
                    if t < 0 {
                        let floor = self.level.columns.get(-(t as i32) as usize).map_or(0, |c| c.floor_tex.max(0));
                        self.edit_cell(i, |c| c.tex = floor);
                    }
                }
            }
            ColMode::Select => {}
        }
    }

    /// A texture thumbnail from the atlas; returns its response.
    fn tex_thumb(&mut self, ui: &mut egui::Ui, t: u8, size: f32, selected: bool) -> egui::Response {
        let atlas = self.look.atlas(ui.ctx());
        let t = t as usize;
        let uv = Rect::from_min_size(Pos2::new((t % 16) as f32 / 16.0, (t / 16) as f32 / 16.0), Vec2::splat(1.0 / 16.0));
        let (r, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
        ui.painter().image(atlas, r, uv, Color32::WHITE);
        if selected {
            ui.painter().rect_stroke(r, 0.0, egui::Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Outside);
        }
        resp
    }

    pub(super) fn blocks_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.col_mode, ColMode::Select, "Select");
            ui.selectable_value(&mut self.col_mode, ColMode::Paint, "Paint");
            ui.selectable_value(&mut self.col_mode, ColMode::Erase, "Erase");
        });
        if self.col_mode != ColMode::Select {
            ui.add(egui::Slider::new(&mut self.brush_radius, 0..=12).text("radius (cells)"));
        }
        ui.label(egui::RichText::new(match self.col_mode {
            ColMode::Select => "Click a cell with a column to edit it. Alt+click picks in any mode.",
            ColMode::Paint => "Drag to put the selected column on cells.",
            ColMode::Erase => "Drag to remove columns (cells get the column's floor texture).",
        }).small().color(Color32::GRAY));
        ui.horizontal(|ui| {
            if ui.button("New column").on_hover_text("an empty column (one solid block level to start with)").clicked() {
                self.new_column(None);
            }
            if let Some(id) = self.col_sel {
                if ui.button("Copy column").on_hover_text("a new column like this one, to change without affecting the cells that use this one").clicked() {
                    self.new_column(Some(id));
                }
            }
        });
        ui.separator();
        if let Some(id) = self.col_sel.filter(|&i| i < self.level.columns.len()) {
            self.column_editor(ui, id);
        } else {
            ui.label("No column selected.");
        }
        if let Some(b) = self.blk_sel.filter(|&b| b > 0 && b < self.level.blocks.len()) {
            ui.separator();
            self.block_editor(ui, b);
        }
        if self.face_sel <= FLOOR && (self.col_sel.is_some() || self.blk_sel.is_some()) {
            ui.separator();
            ui.label(if self.face_sel == FLOOR { "Pick the column's floor texture:".to_string() } else { format!("Pick the {} face texture:", FACES[self.face_sel]) });
            let current = self.selected_face_tex();
            if let Some(t) = self.texture_picker(ui, current) {
                self.set_face_tex(t);
            }
        }
    }

    pub(super) fn new_column(&mut self, from: Option<usize>) {
        let Some(n) = self.free_column() else {
            self.message = "all 1023 columns are in use".into();
            return;
        };
        let col = match from {
            Some(id) => self.level.columns[id],
            None => {
                let b = self.blk_sel.unwrap_or(1) as i16;
                Column { mask: 1, floor_tex: self.paint_tex as i16, blocks: [b, 0, 0, 0, 0, 0, 0, 0] }
            }
        };
        self.edit(&format!("new column {n}"), |l| l.columns[n] = col);
        self.col_sel = Some(n);
        self.col_mode = ColMode::Paint;
    }

    fn selected_face_tex(&self) -> u8 {
        if self.face_sel == FLOOR {
            return self.col_sel.and_then(|c| self.level.columns.get(c)).map_or(0, |c| c.floor_tex.max(0) as u8);
        }
        self.blk_sel.and_then(|b| self.level.blocks.get(b)).map_or(0, |b| b.tex[self.face_sel])
    }

    fn set_face_tex(&mut self, t: u8) {
        if self.face_sel == FLOOR {
            if let Some(c) = self.col_sel {
                self.edit(&format!("column {c} floor texture {t}"), |l| l.columns[c].floor_tex = t as i16);
            }
        } else if let Some(b) = self.blk_sel {
            let f = self.face_sel;
            self.edit(&format!("block {b} {} texture {t}", FACES[f]), |l| l.blocks[b].tex[f] = t);
        }
    }

    fn column_editor(&mut self, ui: &mut egui::Ui, id: usize) {
        let users = self.column_users(id);
        let orig = self.level.columns[id];
        let mut c = orig;
        ui.heading(format!("Column {id}"));
        ui.label(format!("used by {users} cell{}{}", if users == 1 { "" } else { "s" }, if users > 1 { ": changes affect all of them" } else { "" }));
        ui.horizontal(|ui| {
            ui.label("floor texture");
            let floor = c.floor_tex.max(0) as u8;
            if self.tex_thumb(ui, floor, 24.0, self.face_sel == FLOOR).on_hover_text("drawn when level 0 is free (tunnels); click to pick").clicked() {
                self.face_sel = FLOOR;
            }
            ui.add(egui::DragValue::new(&mut c.floor_tex).range(0..=255));
        });
        ui.label(egui::RichText::new("Level k is solid from k to k + 1 cells above the cell's corner heights.").small().color(Color32::GRAY));
        let mut select_block = None;
        egui::Grid::new("column-levels").num_columns(4).show(ui, |ui| {
            for k in (0..8).rev() {
                ui.label(format!("level {k}"));
                let mut solid = c.mask & (1 << k) != 0;
                if ui.checkbox(&mut solid, "solid").changed() {
                    c.mask = if solid { c.mask | 1 << k } else { c.mask & !(1 << k) };
                }
                ui.add(egui::DragValue::new(&mut c.blocks[k]).range(0..=MAX_BLOCKS as i16 - 1).prefix("block "));
                let b = c.blocks[k];
                if b > 0 && ui.selectable_label(self.blk_sel == Some(b as usize), "edit").clicked() {
                    select_block = Some((k, b as usize));
                }
                ui.end_row();
            }
        });
        if c != orig {
            self.begin_edit(&format!("edit column {id}"));
            self.level.columns[id] = c;
            self.touch();
        }
        if let Some((k, b)) = select_block {
            self.blk_sel = Some(b);
            self.lvl_sel = k;
            if self.face_sel == FLOOR {
                self.face_sel = 0;
            }
        }
    }

    fn block_editor(&mut self, ui: &mut egui::Ui, b: usize) {
        let users: usize = self.level.columns.iter().map(|c| c.blocks.iter().filter(|&&x| x as usize == b).count()).sum();
        let orig = self.level.blocks[b];
        let mut blk = orig;
        ui.heading(format!("Block {b}"));
        ui.label(format!("used on {users} column level{}", if users == 1 { "" } else { "s" }));
        let mut pick_face = None;
        egui::Grid::new("block-faces").num_columns(3).show(ui, |ui| {
            for (f, name) in FACES.iter().enumerate() {
                ui.label(*name);
                if self.tex_thumb(ui, blk.tex[f], 24.0, self.face_sel == f).on_hover_text("click, then pick a texture below").clicked() {
                    pick_face = Some(f);
                }
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut blk.tex[f]).prefix("tex "));
                    ui.add(egui::DragValue::new(&mut blk.orient[f]).range(0..=7).prefix("orient "));
                });
                ui.end_row();
            }
        });
        ui.label(egui::RichText::new("The original honours the orientation only on the south, top and bottom faces.").small().color(Color32::GRAY));
        if let Some(f) = pick_face {
            self.face_sel = f;
        }
        if blk != orig {
            self.begin_edit(&format!("edit block {b}"));
            self.level.blocks[b] = blk;
            self.touch();
        }
        if let Some(col) = self.col_sel {
            if users > 1 && ui.button("Copy block for this column level").on_hover_text(format!("a new block like this one on level {} of column {col} only", self.lvl_sel)).clicked() {
                match self.free_block() {
                    Some(n) => {
                        let k = self.lvl_sel;
                        self.edit(&format!("copy block {b} to {n}"), |l| {
                            l.blocks[n] = l.blocks[b];
                            l.columns[col].blocks[k] = n as i16;
                        });
                        self.blk_sel = Some(n);
                    }
                    None => self.message = "all 1023 blocks are in use".into(),
                }
            }
        }
    }

    /// Outline the cells that use the selected column.
    pub(super) fn draw_column_selection(&self, p: &egui::Painter, origin: Pos2) {
        let Some(id) = self.col_sel else { return };
        for (i, c) in self.level.cells.iter().enumerate() {
            if c.tex < 0 && -(c.tex as i32) as usize == id {
                let (x, y) = ((i % MAP_W) as f32, (i / MAP_W) as f32);
                let r = Rect::from_min_max(self.to_screen(origin, x, y), self.to_screen(origin, x + 1.0, y + 1.0));
                p.rect_stroke(r, 0.0, egui::Stroke::new(1.5, Color32::from_rgb(0, 255, 255)), egui::StrokeKind::Inside);
            }
        }
    }
}
