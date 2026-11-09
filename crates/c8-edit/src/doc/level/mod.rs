//! Level document: a top-down map of a `LEVEL0-N.DAT` with overlays, a cell inspector, the
//! terrain tools (height, texture, light, selection with copy/paste), texture attributes, the
//! Objects tool (entities and regions), the Blocks tool (columns and blocks), track checks with
//! an AI test, and "Test in game".
mod blocks;
mod check;
mod edit;
mod objects;

use super::history::{Command, History};
use super::Document;
use blocks::ColMode;
use objects::{Drag, Layers, ObjMode, Sel};
use crate::resources::Kind;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, TextureHandle, Vec2};
use c8_data::images::{load_terrain_textures, ColourTable, Palette};
use c8_data::level::{Cell, Level, FLOOR_UV, MAP_H, MAP_W};
use std::path::{Path, PathBuf};

/// Pixels per cell in the map image.
const PX: usize = 4;
/// Thumbnail size in the texture palette.
const THUMB: f32 = 34.0;

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
    Texture,
    Light,
    Select,
    Objects,
    Blocks,
}

#[derive(Clone, Copy, PartialEq)]
enum TexMode {
    Paint,
    Fill,
}

/// The level's texture set: palette, light table and textures, plus small per-orientation
/// thumbnails for the map image.
struct Look {
    set: usize,
    pal: [[u8; 3]; 256],
    table: Option<ColourTable>,
    tex: Vec<u8>,
    /// [texture][orientation 0..7] → PX×PX palette indices
    thumbs: Vec<[[u8; PX * PX]; 8]>,
    /// the 256 textures 16 × 16 in one egui texture, for the texture palette
    atlas: Option<TextureHandle>,
}

impl Look {
    fn load(set: usize) -> Look {
        let pal = Palette::load().map(|p| p.vga.map(|c| c.map(|v| (v as u32 * 255 / 63) as u8))).unwrap_or([[128; 3]; 256]);
        let table = ColourTable::load(set).ok();
        let tex = load_terrain_textures(set).unwrap_or_else(|_| vec![0; 1 << 20]);
        let thumbs = (0..256)
            .map(|t| {
                std::array::from_fn(|o| {
                    let mut th = [0u8; PX * PX];
                    for y in 0..PX {
                        for x in 0..PX {
                            let (u, v) = uv_at(o, (x as f32 + 0.5) / PX as f32, (y as f32 + 0.5) / PX as f32);
                            th[y * PX + x] = tex[t * 4096 + ((v * 63.0) as usize) * 64 + (u * 63.0) as usize];
                        }
                    }
                    th
                })
            })
            .collect();
        Look { set, pal, table, tex, thumbs, atlas: None }
    }
    fn rgb(&self, idx: u8, light_row: usize) -> [u8; 3] {
        let i = match &self.table {
            Some(t) => t.data[light_row.min(63) * 256 + idx as usize],
            None => idx,
        };
        self.pal[i as usize]
    }
    fn atlas(&mut self, ctx: &egui::Context) -> egui::TextureId {
        if self.atlas.is_none() {
            let mut rgba = vec![0u8; 1024 * 1024 * 4];
            for t in 0..256 {
                let (tx, ty) = (t % 16 * 64, t / 16 * 64);
                for y in 0..64 {
                    for x in 0..64 {
                        let c = self.pal[self.tex[t * 4096 + y * 64 + x] as usize];
                        let o = ((ty + y) * 1024 + tx + x) * 4;
                        rgba[o..o + 3].copy_from_slice(&c);
                        rgba[o + 3] = 255;
                    }
                }
            }
            let img = egui::ColorImage::from_rgba_unmultiplied([1024, 1024], &rgba);
            self.atlas = Some(ctx.load_texture(format!("textures-{}", self.set), img, egui::TextureOptions::LINEAR));
        }
        self.atlas.as_ref().unwrap().id()
    }
}

/// Texture coordinate (0..1) at cell position (fx, fy) for floor orientation o.
fn uv_at(o: usize, fx: f32, fy: f32) -> (f32, f32) {
    let c = FLOOR_UV[o & 7];
    let w = [(1.0 - fx) * (1.0 - fy), fx * (1.0 - fy), fx * fy, (1.0 - fx) * fy];
    let u = (0..4).map(|k| w[k] * c[k].0 as f32).sum();
    let v = (0..4).map(|k| w[k] * c[k].1 as f32).sum();
    (u, v)
}

/// Orientation that shows the texture of orientation o turned 90° clockwise / mirrored.
fn orient_transform(o: u8, f: impl Fn((u8, u8)) -> (u8, u8)) -> u8 {
    let c = FLOOR_UV[o as usize & 7].map(f);
    FLOOR_UV.iter().position(|e| *e == c).unwrap_or(0) as u8
}
fn orient_rotate(o: u8) -> u8 {
    // the corner (x, y) shows what corner (y, 1 − x)... i.e. shift the corners by one
    let c = FLOOR_UV[o as usize & 7];
    let r = [c[3], c[0], c[1], c[2]];
    FLOOR_UV.iter().position(|e| *e == r).unwrap_or(0) as u8
}
fn orient_flip(o: u8) -> u8 {
    orient_transform(o, |(u, v)| (1 - u, v))
}

/// Cells copied from a selection.
#[derive(Clone)]
struct Clip {
    w: usize,
    h: usize,
    cells: Vec<Cell>,
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
    tex_mode: TexMode,
    paint_tex: u8,
    paint_orient: u8,
    light_step: f32,
    light_smooth: bool,
    bake_azimuth: f32,
    bake_strength: f32,
    bake_ambient: f32,
    zoom: f32,
    pan: Vec2,
    hover: Option<(i32, i32)>,
    /// cells changed by the current stroke (one undo step per stroke)
    stroke: Option<CellEdit>,
    fitted: bool,
    /// selection (x0, y0, x1, y1) inclusive, and the drag start
    selection: Option<(i32, i32, i32, i32)>,
    sel_start: Option<(i32, i32)>,
    clip: Option<Clip>,
    pasting: bool,
    paste_heights: bool,
    paste_textures: bool,
    paste_light: bool,
    message: String,
    test: bool,
    /// structural edit in progress (see edit.rs)
    pending: Option<edit::Pending>,
    /// pointer in map coordinates (cells, fractional)
    hover_f: Option<Vec2>,
    view_size: Vec2,
    obj_sel: Option<Sel>,
    obj_mode: ObjMode,
    drag: Option<Drag>,
    layers: Layers,
    add_template: usize,
    all_fields: bool,
    entity_filter: String,
    col_mode: ColMode,
    col_sel: Option<usize>,
    blk_sel: Option<usize>,
    /// face being given a texture: 0..5 block faces, 6 the column floor
    face_sel: usize,
    /// column level of the selected block
    lvl_sel: usize,
    ai: Option<check::AiState>,
    ai_ticks: u32,
}

impl LevelDoc {
    pub fn open(path: PathBuf, rel: String) -> Result<LevelDoc, String> {
        let level = Level::read_file(&path)?;
        // LEVEL0-N uses texture set and colour table N-1
        let name = path.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let set = name.strip_prefix("level0-").and_then(|n| n.parse::<usize>().ok()).map(|n| n.saturating_sub(1)).unwrap_or(0).min(5);
        let mut d = LevelDoc {
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
            // debug: HIEDIT_TOOL=height|texture|light|select starts with that tool (screenshots)
            tool: match std::env::var("HIEDIT_TOOL").as_deref() {
                Ok("height") => Tool::Height,
                Ok("texture") => Tool::Texture,
                Ok("light") => Tool::Light,
                Ok("select") => Tool::Select,
                Ok("objects") => Tool::Objects,
                Ok("blocks") => Tool::Blocks,
                _ => Tool::Inspect,
            },
            brush_radius: 2,
            brush_step: 0.25,
            tex_mode: TexMode::Paint,
            paint_tex: 0,
            paint_orient: 0,
            light_step: 2.0,
            light_smooth: false,
            bake_azimuth: 315.0,
            bake_strength: 12.0,
            bake_ambient: 31.0,
            zoom: 3.0,
            pan: Vec2::ZERO,
            hover: None,
            stroke: None,
            fitted: false,
            selection: None,
            sel_start: None,
            clip: None,
            pasting: false,
            paste_heights: true,
            paste_textures: true,
            paste_light: true,
            message: String::new(),
            test: false,
            pending: None,
            hover_f: None,
            view_size: Vec2::new(800.0, 600.0),
            obj_sel: None,
            obj_mode: ObjMode::Select,
            drag: None,
            layers: Layers::default(),
            add_template: 0,
            all_fields: false,
            entity_filter: String::new(),
            col_mode: ColMode::Select,
            col_sel: None,
            blk_sel: None,
            face_sel: 0,
            lvl_sel: 0,
            ai: None,
            ai_ticks: 4000,
        };
        // debug (screenshots): HIEDIT_ENTITY=<record> / HIEDIT_COLUMN=<id> start with a selection
        let num = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<usize>().ok());
        if let Some(e) = num("HIEDIT_ENTITY") {
            d.obj_sel = Some(Sel::Entity(e));
        }
        if let Some(c) = num("HIEDIT_COLUMN") {
            d.col_sel = Some(c);
            d.blk_sel = d.level.columns.get(c).and_then(|c| c.blocks.iter().find(|&&b| b > 0)).map(|&b| b as usize);
        }
        Ok(d)
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
                let shade = {
                    let d = lvl.corner_h(cx as i32, cy as i32) - lvl.corner_h(cx as i32 + 1, cy as i32 + 1);
                    (1.0 + d * 0.8).clamp(0.4, 1.6)
                };
                let tex = lvl.floor_tex(cx as i32, cy as i32) as usize & 255;
                let thumb = &self.look.thumbs[tex][c.orient as usize & 7];
                for py in 0..PX {
                    for px in 0..PX {
                        let col = match self.base {
                            Base::Textures => self.look.rgb(thumb[py * PX + px], light_row),
                            Base::Height => {
                                let v = ((c.height as f32 / max_h * 200.0 + 30.0) * shade).clamp(0.0, 255.0) as u8;
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

    // ---- editing --------------------------------------------------------------------------

    /// Change cell i now and record it in the current stroke.
    fn edit_cell(&mut self, i: usize, f: impl FnOnce(&mut Cell)) {
        let old = self.level.cells[i];
        let mut new = old;
        f(&mut new);
        if new == old {
            return;
        }
        self.level.cells[i] = new;
        let st = self.stroke.get_or_insert_with(|| CellEdit { label: String::new(), changes: Vec::new() });
        match st.changes.iter_mut().find(|c| c.0 == i) {
            Some(c) => c.2 = new,
            None => st.changes.push((i, old, new)),
        }
        self.image_dirty = true;
    }

    /// Finish the stroke: it becomes one undo step.
    fn end_stroke(&mut self, label: &str) {
        if let Some(mut st) = self.stroke.take() {
            if st.changes.is_empty() {
                return;
            }
            st.label = format!("{label} ({} cells)", st.changes.len());
            let mut c: Box<dyn Command<Level>> = Box::new(st);
            // already applied while painting: take it back so that History applies it once
            c.revert(&mut self.level);
            self.history.run(c, &mut self.level);
        }
    }

    /// Cells within the brush with their falloff 0..1.
    fn brush_cells(&self, cx: i32, cy: i32) -> Vec<(usize, f32)> {
        let r = self.brush_radius;
        let mut out = Vec::new();
        for y in cy - r..=cy + r {
            for x in cx - r..=cx + r {
                if x < 0 || y < 0 || x >= MAP_W as i32 || y >= MAP_H as i32 {
                    continue;
                }
                let d = (((x - cx).pow(2) + (y - cy).pow(2)) as f32).sqrt();
                let fall = (1.0 - d / (r as f32 + 1.0)).max(0.0);
                if fall > 0.0 {
                    out.push((y as usize * MAP_W + x as usize, fall));
                }
            }
        }
        out
    }

    fn height_brush(&mut self, cx: i32, cy: i32, lower: bool) {
        let step = self.brush_step * 256.0 * if lower { -1.0 } else { 1.0 };
        for (i, fall) in self.brush_cells(cx, cy) {
            self.edit_cell(i, |c| c.height = (c.height as f32 + step * fall).clamp(0.0, 65535.0) as u16);
        }
    }

    fn texture_brush(&mut self, cx: i32, cy: i32) {
        let (t, o) = (self.paint_tex, self.paint_orient);
        for (i, _) in self.brush_cells(cx, cy) {
            // cells with block columns take their floor from the column (shared by many cells)
            self.edit_cell(i, |c| {
                if c.tex >= 0 {
                    c.tex = t as i16;
                    c.orient = o;
                }
            });
        }
    }

    fn texture_fill(&mut self, x: i32, y: i32) {
        let start = *self.level.cell(x, y);
        if start.tex < 0 {
            self.message = "cells with block columns cannot be filled".into();
            return;
        }
        let mut seen = vec![false; MAP_W * MAP_H];
        let mut todo = vec![(x, y)];
        while let Some((x, y)) = todo.pop() {
            if x < 0 || y < 0 || x >= MAP_W as i32 || y >= MAP_H as i32 {
                continue;
            }
            let i = y as usize * MAP_W + x as usize;
            if seen[i] || self.level.cells[i].tex != start.tex {
                continue;
            }
            seen[i] = true;
            todo.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]);
        }
        let (t, o) = (self.paint_tex, self.paint_orient);
        for (i, _) in seen.iter().enumerate().filter(|(_, s)| **s) {
            self.edit_cell(i, |c| {
                c.tex = t as i16;
                c.orient = o;
            });
        }
        self.end_stroke("texture fill");
    }

    fn light_brush(&mut self, cx: i32, cy: i32, darker: bool) {
        let cells = self.brush_cells(cx, cy);
        if self.light_smooth {
            let avg: Vec<(usize, i16)> = cells
                .iter()
                .map(|&(i, _)| {
                    let (x, y) = ((i % MAP_W) as i32, (i / MAP_W) as i32);
                    let mut s = 0i32;
                    for (dx, dy) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                        s += self.level.cell(x + dx, y + dy).light as i32;
                    }
                    (i, (s / 5) as i16)
                })
                .collect();
            for (i, v) in avg {
                self.edit_cell(i, |c| c.light = v);
            }
        } else {
            let step = self.light_step * 256.0 * if darker { -1.0 } else { 1.0 };
            for (i, fall) in cells {
                self.edit_cell(i, |c| c.light = (c.light as f32 + step * fall).clamp(0.0, 63.0 * 256.0 + 255.0) as i16);
            }
        }
    }

    /// Light every corner in the selection (or the whole map) from its slope: light from the
    /// azimuth, `ambient` on flat ground, ± `strength` rows on slopes.
    fn bake_light(&mut self) {
        let (x0, y0, x1, y1) = self.selection.unwrap_or((0, 0, MAP_W as i32 - 1, MAP_H as i32 - 1));
        let a = self.bake_azimuth.to_radians();
        // map rows grow southwards; azimuth 0 = light from the north, clockwise
        let (lx, ly) = (a.sin(), -a.cos());
        for y in y0..=y1 {
            for x in x0..=x1 {
                let h = |x: i32, y: i32| self.level.corner_h(x, y);
                let (dx, dy) = ((h(x + 1, y) - h(x - 1, y)) / 2.0, (h(x, y + 1) - h(x, y - 1)) / 2.0);
                // a slope facing the light (rising away from it) is brighter
                let facing = -(dx * lx + dy * ly);
                let row = (self.bake_ambient + facing * self.bake_strength).clamp(2.0, 62.0);
                let v = (row * 256.0) as i16 | 0xff;
                self.edit_cell(y as usize * MAP_W + x as usize, |c| c.light = v);
            }
        }
        self.end_stroke("bake light");
    }

    fn copy(&mut self) {
        let Some((x0, y0, x1, y1)) = self.selection else { return };
        let (w, h) = ((x1 - x0 + 1) as usize, (y1 - y0 + 1) as usize);
        let cells = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).map(|(x, y)| *self.level.cell(x0 + x as i32, y0 + y as i32)).collect();
        self.clip = Some(Clip { w, h, cells });
        self.message = format!("copied {w} × {h} cells");
    }

    fn paste_at(&mut self, x0: i32, y0: i32) {
        let Some(clip) = self.clip.clone() else { return };
        let (ph, pt, pl) = (self.paste_heights, self.paste_textures, self.paste_light);
        for y in 0..clip.h {
            for x in 0..clip.w {
                let (tx, ty) = (x0 + x as i32, y0 + y as i32);
                if tx < 0 || ty < 0 || tx >= MAP_W as i32 || ty >= MAP_H as i32 {
                    continue;
                }
                let src = clip.cells[y * clip.w + x];
                self.edit_cell(ty as usize * MAP_W + tx as usize, |c| {
                    if ph {
                        c.height = src.height;
                    }
                    if pt {
                        c.tex = src.tex;
                        c.orient = src.orient;
                    }
                    if pl {
                        c.light = src.light;
                    }
                });
            }
        }
        self.end_stroke("paste");
        self.selection = Some((x0, y0, x0 + clip.w as i32 - 1, y0 + clip.h as i32 - 1));
    }

    fn set_tex_attr(&mut self, t: usize, v: u16) {
        let old = self.level.tex_attr[t];
        if old != v {
            self.history.run(Box::new(TexAttrEdit { tex: t, old, new: v }), &mut self.level);
            self.level.update_extent();
        }
    }

    /// Save to a temporary file and start the game on it.
    // ---- drawing ---------------------------------------------------------------------------

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
                let selected = self.obj_sel == Some(Sel::Region(r.slot));
                p.rect_stroke(rect, 0.0, Stroke::new(if selected { 3.5 } else { 2.0 }, c), egui::StrokeKind::Inside);
                if selected {
                    for k in [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()] {
                        p.rect_filled(Rect::from_center_size(k, Vec2::splat(7.0)), 1.0, Color32::WHITE);
                    }
                }
                if z >= 4.0 {
                    p.text(rect.left_top() + Vec2::new(3.0, 2.0), egui::Align2::LEFT_TOP, objects::region_name(r.kind), egui::FontId::proportional(11.0), c);
                }
            }
        }
        if self.show_entities {
            let pos = |i: usize| lvl.entities.iter().find(|e| e.index == i).map(|e| (e.x as f32 + 0.5, e.y as f32 + 0.5));
            for e in lvl.entities.iter().filter(|e| self.layers.shows(e)) {
                let s = self.to_screen(origin, e.x as f32 + 0.5, e.y as f32 + 0.5);
                // entities of later groups appear only when a trigger activates them
                let a = if e.group == 1 { 255 } else { 130 };
                let rgb = |r, g, b| Color32::from_rgba_unmultiplied(r, g, b, a);
                match (e.kind, e.sub) {
                    (9, sub) => {
                        if let Some((lx, ly)) = pos(e.link as usize) {
                            let col = if sub == 0 { rgb(255, 60, 60) } else if sub == 6 { rgb(255, 0, 255) } else { rgb(0, 230, 230) };
                            let t = self.to_screen(origin, lx, ly);
                            p.line_segment([s, t], Stroke::new(if sub == 0 { 2.0 } else { 1.5 }, col));
                            if sub != 0 {
                                p.circle_filled(t, 2.0, col);
                            }
                        }
                        if self.tool == Tool::Objects && z >= 6.0 {
                            p.circle_filled(s, 2.0, if sub == 0 { rgb(255, 120, 120) } else { rgb(150, 255, 255) });
                        }
                    }
                    (1, _) => {
                        // the port's checkpoint box starts at the entity cell's centre
                        let r = cell_rect(e.x as f32 + 0.5, e.y as f32 + 0.5, e.w as f32 + 1.0, e.h as f32 + 1.0);
                        p.rect_stroke(r, 0.0, Stroke::new(2.0, rgb(255, 255, 0)), egui::StrokeKind::Inside);
                        p.text(r.center(), egui::Align2::CENTER_CENTER, e.value.to_string(), egui::FontId::proportional(12.0), rgb(255, 255, 0));
                    }
                    (5, _) => {
                        p.circle_filled(s, (z * 0.35).max(3.0), rgb(60, 255, 60));
                    }
                    (10, _) => {
                        p.circle_filled(s, (z * 0.45).max(4.0), rgb(255, 150, 0));
                    }
                    (8, sub) => {
                        p.circle_stroke(s, (z * 0.4).max(3.0), Stroke::new(1.5, rgb(255, 80, 255)));
                        if sub == 0 && self.obj_sel == Some(Sel::Entity(e.index)) {
                            let r = Rect::from_min_max(self.to_screen(origin, e.x as f32 + 0.5 - e.w as f32, e.y as f32 + 0.5 - e.h as f32), self.to_screen(origin, e.x as f32 + 0.5 + e.w as f32, e.y as f32 + 0.5 + e.h as f32));
                            p.rect_stroke(r, 0.0, Stroke::new(1.0, rgb(255, 80, 255)), egui::StrokeKind::Middle);
                        }
                    }
                    (2, 15..=19) => {
                        let r = cell_rect(e.x as f32, e.y as f32, e.w as f32 + 1.0, e.h as f32 + 1.0);
                        p.rect_stroke(r, 0.0, Stroke::new(1.0, rgb(255, 160, 60)), egui::StrokeKind::Inside);
                        if let Some((lx, ly)) = pos(e.link as usize) {
                            let r2 = cell_rect(lx - 0.5, ly - 0.5, e.w as f32 + 1.0, e.h as f32 + 1.0);
                            p.rect_stroke(r2, 0.0, Stroke::new(1.0, rgb(255, 200, 120)), egui::StrokeKind::Inside);
                            p.line_segment([r.center(), r2.center()], Stroke::new(1.0, rgb(255, 160, 60)));
                        }
                    }
                    (3, 3) => {
                        p.circle_stroke(s, 2.5, Stroke::new(1.0, Color32::LIGHT_GRAY));
                    }
                    _ => {
                        p.circle_filled(s, 2.0, rgb(200, 200, 200));
                    }
                }
            }
            // the selected entity and its link
            if let Some(Sel::Entity(i)) = self.obj_sel {
                if let Some(e) = lvl.entities.iter().find(|e| e.index == i) {
                    let s = self.to_screen(origin, e.x as f32 + 0.5, e.y as f32 + 0.5);
                    p.circle_stroke(s, (z * 0.6).max(6.0), Stroke::new(2.0, Color32::WHITE));
                    if let Some((lx, ly)) = pos(e.link as usize) {
                        p.line_segment([s, self.to_screen(origin, lx, ly)], Stroke::new(3.0, Color32::from_rgba_unmultiplied(255, 255, 255, 160)));
                    }
                }
            }
            // the chain being drawn
            if let (ObjMode::Chain(_, Some(last)), Some(h)) = (self.obj_mode, self.hover) {
                if let Some((lx, ly)) = pos(last) {
                    p.line_segment([self.to_screen(origin, lx, ly), self.to_screen(origin, h.0 as f32 + 0.5, h.1 as f32 + 0.5)], Stroke::new(1.5, Color32::from_rgba_unmultiplied(255, 255, 255, 180)));
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
        if let Some((x0, y0, x1, y1)) = self.selection {
            let r = Rect::from_min_max(self.to_screen(origin, x0 as f32, y0 as f32), self.to_screen(origin, x1 as f32 + 1.0, y1 as f32 + 1.0));
            p.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 25));
            p.rect_stroke(r, 0.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Outside);
        }
        if let Some((hx, hy)) = self.hover {
            if self.pasting {
                if let Some(c) = &self.clip {
                    let r = Rect::from_min_size(self.to_screen(origin, hx as f32, hy as f32), Vec2::new(c.w as f32, c.h as f32) * z);
                    p.rect_stroke(r, 0.0, Stroke::new(2.0, Color32::from_rgb(120, 255, 120)), egui::StrokeKind::Outside);
                }
            } else {
                let r = if matches!(self.tool, Tool::Height | Tool::Light) || (self.tool == Tool::Texture && self.tex_mode == TexMode::Paint) { self.brush_radius as f32 } else { 0.0 };
                let rect = Rect::from_min_max(self.to_screen(origin, hx as f32 - r, hy as f32 - r), self.to_screen(origin, hx as f32 + 1.0 + r, hy as f32 + 1.0 + r));
                p.rect_stroke(rect, 0.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Outside);
            }
        }
    }

    /// The level's 256 textures; returns the clicked one.
    fn texture_picker(&mut self, ui: &mut egui::Ui, selected: u8) -> Option<u8> {
        let atlas = self.look.atlas(ui.ctx());
        // 8 per row, sized to the panel (buttons add a few pixels of frame)
        let cols = 8;
        let thumb = ((ui.available_width() - 20.0) / cols as f32 - 8.0).clamp(16.0, THUMB);
        let mut clicked = None;
        egui::ScrollArea::vertical().id_salt("texpal").max_height(260.0).show(ui, |ui| {
            egui::Grid::new("texgrid").spacing([2.0, 2.0]).show(ui, |ui| {
                for t in 0..256usize {
                    let uv = Rect::from_min_size(Pos2::new((t % 16) as f32 / 16.0, (t / 16) as f32 / 16.0), Vec2::splat(1.0 / 16.0));
                    let img = egui::Image::new((atlas, Vec2::splat(thumb))).uv(uv);
                    let resp = ui.add(egui::Button::image(img).selected(t == selected as usize)).on_hover_text(format!("texture {t}, attributes {:#06x}", self.level.tex_attr[t]));
                    if resp.clicked() {
                        clicked = Some(t as u8);
                    }
                    if (t + 1) % cols == 0 {
                        ui.end_row();
                    }
                }
            });
        });
        clicked
    }
}

/// Cell changes: (index, old, new).
struct CellEdit {
    label: String,
    changes: Vec<(usize, Cell, Cell)>,
}

impl Command<Level> for CellEdit {
    fn apply(&mut self, l: &mut Level) {
        for &(i, _, new) in &self.changes {
            l.cells[i] = new;
        }
    }
    fn revert(&mut self, l: &mut Level) {
        for &(i, old, _) in &self.changes {
            l.cells[i] = old;
        }
    }
    fn label(&self) -> String {
        self.label.clone()
    }
}

struct TexAttrEdit {
    tex: usize,
    old: u16,
    new: u16,
}

impl Command<Level> for TexAttrEdit {
    fn apply(&mut self, l: &mut Level) {
        l.tex_attr[self.tex] = self.new;
    }
    fn revert(&mut self, l: &mut Level) {
        l.tex_attr[self.tex] = self.old;
    }
    fn label(&self) -> String {
        format!("texture {} attributes {:#06x}", self.tex, self.new)
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
        self.finish_edit();
        self.end_stroke("edit");
        self.history.undo(&mut self.level);
        self.level.update_extent();
        self.image_dirty = true;
    }
    fn redo(&mut self) {
        self.finish_edit();
        self.history.redo(&mut self.level);
        self.level.update_extent();
        self.image_dirty = true;
    }
    fn can_undo(&self) -> bool {
        self.history.can_undo()
    }
    fn can_redo(&self) -> bool {
        self.history.can_redo()
    }
    /// The level replaces the one of the track whose textures it is shown with.
    fn game_files(&self) -> Result<Vec<(String, Vec<u8>)>, String> {
        Ok(vec![(format!("MAPS/LEVEL0-{}.DAT", self.look.set + 1), self.level.to_bytes()?)])
    }
    fn game_args(&self) -> Vec<String> {
        vec!["--no-intro".into(), "--race".into(), (self.look.set + 1).to_string()]
    }
    fn wants_test(&mut self) -> bool {
        std::mem::take(&mut self.test)
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        // keyboard: tools, copy / paste, escape, delete
        if !ui.ctx().egui_wants_keyboard_input() {
            let (keys, copy, paste, delete, dup) = ui.input(|i| {
                let k = |key| i.key_pressed(key) && i.modifiers.is_none();
                (
                    [k(egui::Key::I), k(egui::Key::H), k(egui::Key::T), k(egui::Key::L), k(egui::Key::S), k(egui::Key::O), k(egui::Key::B), k(egui::Key::R), k(egui::Key::F), i.key_pressed(egui::Key::Escape), k(egui::Key::A)],
                    i.events.iter().any(|e| matches!(e, egui::Event::Copy)),
                    i.events.iter().any(|e| matches!(e, egui::Event::Paste(_))),
                    k(egui::Key::Delete) || k(egui::Key::Backspace),
                    i.modifiers.command && i.key_pressed(egui::Key::D),
                )
            });
            let tools = [Tool::Inspect, Tool::Height, Tool::Texture, Tool::Light, Tool::Select, Tool::Objects, Tool::Blocks];
            for (k, t) in keys.iter().zip(tools) {
                if *k && self.tool != t {
                    self.cancel_mode();
                    self.tool = t;
                }
            }
            if keys[7] {
                self.paint_orient = orient_rotate(self.paint_orient);
            }
            if keys[8] {
                self.paint_orient = orient_flip(self.paint_orient);
            }
            if keys[9] {
                if self.obj_mode != ObjMode::Select {
                    self.cancel_mode();
                } else {
                    self.pasting = false;
                    self.selection = None;
                    self.obj_sel = None;
                }
            }
            if keys[10] && self.tool == Tool::Objects {
                self.start_adding();
            }
            if delete && self.tool == Tool::Objects {
                self.delete_selected();
            }
            if dup && self.tool == Tool::Objects {
                self.duplicate_selected();
            }
            if copy {
                self.copy();
            }
            if paste && self.clip.is_some() {
                self.pasting = true;
            }
        }

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
        self.view_size = rect.size();
        painter.rect_filled(rect, 0.0, Color32::from_gray(24));
        if !self.fitted {
            let (w, h) = ((self.level.max_x.max(16) + 4) as f32, (self.level.max_y.max(16) + 4) as f32);
            self.zoom = (rect.width() / w).min(rect.height() / h).clamp(1.0, 64.0);
            self.pan = Vec2::new(-2.0, -2.0) * self.zoom;
            self.fitted = true;
        }
        // zoom about the pointer (pinch, ctrl/cmd + wheel), pan with the wheel or a right/middle drag
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
        let origin = rect.min;
        let map = Rect::from_min_size(self.to_screen(origin, 0.0, 0.0), Vec2::new(MAP_W as f32, MAP_H as f32) * self.zoom);
        let painter = painter.with_clip_rect(rect);
        if let Some(t) = &self.image {
            painter.image(t.id(), map, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        let to_map = |p: Pos2| (p - origin + self.pan) / self.zoom;
        self.hover_f = resp.hover_pos().or_else(|| resp.interact_pointer_pos()).map(to_map).filter(|c| c.x >= 0.0 && c.y >= 0.0 && c.x < MAP_W as f32 && c.y < MAP_H as f32);
        self.hover = self.hover_f.map(|c| (c.x as i32, c.y as i32));
        let press = ui.input(|i| i.pointer.press_origin()).map(to_map);

        // tools (primary button)
        let (primary_down, mods) = ui.input(|i| (i.pointer.primary_down(), i.modifiers));
        let down = resp.is_pointer_button_down_on() && primary_down;
        if self.pasting {
            if resp.clicked() {
                if let Some((x, y)) = self.hover {
                    self.paste_at(x, y);
                }
                self.pasting = false;
            }
        } else {
            match self.tool {
                Tool::Inspect => {}
                Tool::Height => {
                    if down {
                        if let Some((x, y)) = self.hover {
                            self.height_brush(x, y, mods.shift);
                        }
                    } else {
                        self.end_stroke("height brush");
                    }
                }
                Tool::Texture => {
                    if mods.alt && resp.clicked() {
                        // pick texture and orientation from the map
                        if let Some((x, y)) = self.hover {
                            self.paint_tex = self.level.floor_tex(x, y) as u8;
                            self.paint_orient = self.level.cell(x, y).orient & 7;
                        }
                    } else if self.tex_mode == TexMode::Fill {
                        if resp.clicked() {
                            if let Some((x, y)) = self.hover {
                                self.texture_fill(x, y);
                            }
                        }
                    } else if down && !mods.alt {
                        if let Some((x, y)) = self.hover {
                            self.texture_brush(x, y);
                        }
                    } else {
                        self.end_stroke("texture paint");
                    }
                }
                Tool::Light => {
                    if down {
                        if let Some((x, y)) = self.hover {
                            self.light_brush(x, y, mods.shift);
                        }
                    } else {
                        self.end_stroke(if self.light_smooth { "light smooth" } else { "light paint" });
                    }
                }
                Tool::Objects => self.objects_input(&resp, press),
                Tool::Blocks => self.blocks_input(&resp, down, mods.alt),
                Tool::Select => {
                    if resp.drag_started_by(egui::PointerButton::Primary) {
                        self.sel_start = self.hover;
                    }
                    if down {
                        if let (Some((sx, sy)), Some((x, y))) = (self.sel_start, self.hover) {
                            self.selection = Some((sx.min(x), sy.min(y), sx.max(x), sy.max(y)));
                        }
                    } else {
                        self.sel_start = None;
                    }
                    if resp.clicked() {
                        self.selection = self.hover.map(|(x, y)| (x, y, x, y));
                    }
                }
            }
        }
        self.draw_overlays(&painter, origin);
        if self.tool == Tool::Blocks {
            self.draw_column_selection(&painter, origin);
        }
        self.draw_ai_marks(&painter, origin);
        self.settle_edit(ui.ctx());
    }

    fn side_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Test in game").on_hover_text("save to a temporary file and start the game on it").clicked() {
                self.test = true;
            }
        });
        if !self.message.is_empty() {
            ui.label(egui::RichText::new(&self.message).small());
        }
        ui.separator();
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
        egui::ComboBox::from_label("Texture set").selected_text(format!("{set} (track {})", set + 1)).show_ui(ui, |ui| {
            for s in 0..6 {
                ui.selectable_value(&mut set, s, format!("{s} (track {})", s + 1));
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
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.tool, Tool::Inspect, "Inspect (I)");
            ui.selectable_value(&mut self.tool, Tool::Height, "Height (H)");
            ui.selectable_value(&mut self.tool, Tool::Texture, "Texture (T)");
            ui.selectable_value(&mut self.tool, Tool::Light, "Light (L)");
            ui.selectable_value(&mut self.tool, Tool::Select, "Select (S)");
            ui.selectable_value(&mut self.tool, Tool::Objects, "Objects (O)");
            ui.selectable_value(&mut self.tool, Tool::Blocks, "Blocks (B)");
        });
        if self.tool != Tool::Objects && self.obj_mode != ObjMode::Select {
            self.cancel_mode();
        }
        let brush = |ui: &mut egui::Ui, r: &mut i32| {
            ui.add(egui::Slider::new(r, 0..=12).text("radius (cells)"));
        };
        match self.tool {
            Tool::Inspect => {
                ui.label("Point at the map to see a cell.");
            }
            Tool::Objects => self.objects_panel(ui),
            Tool::Blocks => self.blocks_panel(ui),
            Tool::Height => {
                brush(ui, &mut self.brush_radius);
                ui.add(egui::Slider::new(&mut self.brush_step, 0.05..=2.0).text("step (cells)"));
                ui.label("Drag to raise, Shift+drag to lower.");
            }
            Tool::Texture => {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tex_mode, TexMode::Paint, "Paint");
                    ui.selectable_value(&mut self.tex_mode, TexMode::Fill, "Fill");
                });
                if self.tex_mode == TexMode::Paint {
                    brush(ui, &mut self.brush_radius);
                }
                ui.horizontal(|ui| {
                    ui.label(format!("texture {}, orientation {}", self.paint_tex, self.paint_orient));
                    if ui.button("Rotate (R)").on_hover_text("turn 90°").clicked() {
                        self.paint_orient = orient_rotate(self.paint_orient);
                    }
                    if ui.button("Mirror (F)").on_hover_text("mirror left-right").clicked() {
                        self.paint_orient = orient_flip(self.paint_orient);
                    }
                });
                // preview of the brush texture in its orientation
                let atlas = self.look.atlas(ui.ctx());
                let t = self.paint_tex as usize;
                let (r, _) = ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
                let base = Pos2::new((t % 16) as f32 / 16.0, (t / 16) as f32 / 16.0);
                let c = FLOOR_UV[self.paint_orient as usize & 7];
                let mut mesh = egui::Mesh::with_texture(atlas);
                let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()];
                for k in 0..4 {
                    let uv = base + Vec2::new(c[k].0 as f32, c[k].1 as f32) / 16.0;
                    mesh.vertices.push(egui::epaint::Vertex { pos: corners[k], uv, color: Color32::WHITE });
                }
                mesh.indices.extend([0, 1, 2, 0, 2, 3]);
                ui.painter().add(egui::Shape::mesh(mesh));
                ui.label("Alt+click picks from the map; cells with block columns keep their column's floor.");
                if let Some(t) = self.texture_picker(ui, self.paint_tex) {
                    self.paint_tex = t;
                }
                let t = self.paint_tex as usize;
                let mut attr = self.level.tex_attr[t];
                ui.horizontal(|ui| {
                    ui.label(format!("texture {t} attributes"));
                    ui.add(egui::DragValue::new(&mut attr).hexadecimal(4, false, true));
                });
                let mut track = attr & 4 != 0;
                if ui.checkbox(&mut track, "track surface (bit 2: no slope blocking, track extent)").changed() {
                    attr = if track { attr | 4 } else { attr & !4 };
                }
                self.set_tex_attr(t, attr);
            }
            Tool::Light => {
                brush(ui, &mut self.brush_radius);
                ui.checkbox(&mut self.light_smooth, "smooth instead of paint");
                if !self.light_smooth {
                    ui.add(egui::Slider::new(&mut self.light_step, 0.25..=8.0).text("step (rows)"));
                    ui.label("Drag to brighten, Shift+drag to darken (row 32 = unchanged colours).");
                }
                ui.separator();
                ui.label(if self.selection.is_some() { "Bake light into the selection" } else { "Bake light into the whole map" });
                ui.add(egui::Slider::new(&mut self.bake_azimuth, 0.0..=359.0).text("light from (°, 0 = north)"));
                ui.add(egui::Slider::new(&mut self.bake_strength, 0.0..=40.0).text("slope contrast"));
                ui.add(egui::Slider::new(&mut self.bake_ambient, 8.0..=48.0).text("flat ground row"));
                if ui.button("Bake").clicked() {
                    self.bake_light();
                }
            }
            Tool::Select => {
                match self.selection {
                    Some((x0, y0, x1, y1)) => ui.label(format!("{x0}, {y0} – {x1}, {y1} ({} × {})", x1 - x0 + 1, y1 - y0 + 1)),
                    None => ui.label("Drag to select cells."),
                };
                ui.horizontal(|ui| {
                    if ui.add_enabled(self.selection.is_some(), egui::Button::new("Copy (⌘C)")).clicked() {
                        self.copy();
                    }
                    if ui.add_enabled(self.clip.is_some(), egui::Button::new("Paste (⌘V)")).clicked() {
                        self.pasting = true;
                    }
                });
                ui.label("Paste:");
                ui.checkbox(&mut self.paste_heights, "heights");
                ui.checkbox(&mut self.paste_textures, "textures and block columns");
                ui.checkbox(&mut self.paste_light, "light");
                if self.pasting {
                    ui.label("Click on the map to place, Escape to cancel.");
                }
            }
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
        self.check_panel(ui);
        ui.separator();
        ui.heading("Level");
        let used: std::collections::HashSet<i16> = self.level.cells.iter().filter(|c| c.tex < 0).map(|c| c.tex).collect();
        ui.label(format!("{} entities, {} regions, {} columns in use", self.level.entities.len(), self.level.regions.len(), used.len()));
        ui.collapsing("History", |ui| {
            for l in self.history.labels() {
                ui.label(l);
            }
        });
        self.settle_edit(ui.ctx());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orientation_ops() {
        // four rotations come back; two mirrors come back
        for o in 0..8u8 {
            assert_eq!(orient_rotate(orient_rotate(orient_rotate(orient_rotate(o)))), o);
            assert_eq!(orient_flip(orient_flip(o)), o);
            assert_ne!(orient_rotate(o), o);
        }
        // identity texture coordinates
        assert_eq!(uv_at(0, 0.0, 0.0), (0.0, 0.0));
        assert_eq!(uv_at(0, 1.0, 1.0), (1.0, 1.0));
    }

    /// The tools change cells, every stroke is one undo step, and the result saves and reloads.
    #[test]
    fn tools_undo_save() {
        let data = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources"));
        c8_data::files::set_data_dir(data.clone());
        let path = c8_data::files::resolve("maps/level0-1.dat").unwrap();
        let mut d = LevelDoc::open(path, "LEVEL0-1.DAT".into()).unwrap();
        let orig = d.level.cells.clone();
        // a free (column-less) cell
        let (x, y) = (0..MAP_H as i32).flat_map(|y| (0..MAP_W as i32).map(move |x| (x, y))).find(|&(x, y)| x > 20 && y > 20 && d.level.cell(x, y).tex >= 0).unwrap();
        let i = y as usize * MAP_W + x as usize;
        d.paint_tex = 7;
        d.paint_orient = 3;
        d.brush_radius = 0;
        d.texture_brush(x, y);
        d.end_stroke("texture paint");
        assert_eq!((d.level.cells[i].tex, d.level.cells[i].orient), (7, 3));
        d.height_brush(x, y, false);
        d.height_brush(x, y, false);
        d.end_stroke("height brush");
        assert_eq!(d.history.labels().count(), 2, "one undo step per stroke");
        assert!(d.dirty());
        d.undo();
        d.undo();
        assert!(d.level.cells == orig, "undo restores the level");
        d.redo();
        assert_eq!(d.level.cells[i].tex, 7);
        // copy / paste and bake
        d.selection = Some((x, y, x + 1, y + 1));
        d.copy();
        d.paste_at(x + 10, y);
        assert_eq!(d.level.cell(x + 10, y).tex, 7);
        d.selection = Some((x, y, x + 3, y + 3));
        d.bake_light();
        d.texture_fill(x, y);
        // save and reload
        let tmp = std::env::temp_dir().join("c8-edit-test-level.dat");
        d.save_to(&tmp).unwrap();
        assert!(!d.dirty());
        let back = Level::read_file(&tmp).unwrap();
        assert!(back.cells == d.level.cells);
        let _ = std::fs::remove_file(tmp);
    }

    /// Entities, regions, columns and blocks: edits, undo, save and reload.
    #[test]
    fn structure_edits() {
        let data = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources"));
        c8_data::files::set_data_dir(data);
        let Some(path) = c8_data::files::resolve("maps/level0-2.dat") else { return };
        let mut d = LevelDoc::open(path, "LEVEL0-2.DAT".into()).unwrap();
        let n0 = d.level.entities.len();
        // a pickup and a three-point waypoint chain
        d.edit("add", |l| {
            let i = l.free_entity_index().unwrap();
            l.entities.push(c8_data::level::Entity { index: i, kind: 5, sub: 11, group: 1, x: 30, y: 30, ..Default::default() });
        });
        d.begin_edit("chain");
        let a = d.add_entity(9, 1, 40, 40).unwrap();
        let b = d.add_entity(9, 1, 44, 40).unwrap();
        d.entity_mut(a).unwrap().link = b as u16;
        d.finish_edit();
        assert_eq!(d.level.entities.len(), n0 + 3);
        assert_eq!(d.history.labels().count(), 2);
        // delete b: a's link is cleared in the same step
        d.obj_sel = Some(Sel::Entity(b));
        d.delete_selected();
        assert_eq!(d.entity(a).unwrap().link, 0);
        d.undo();
        assert_eq!(d.entity(a).unwrap().link, b as u16);
        // a region and a column
        let slot = d.level.free_region_slot().unwrap();
        d.edit("region", |l| l.regions.push(c8_data::level::Region { slot, kind: 2, cx: 50.5, cy: 50.5, hw: 1.5, hh: 1.5 }));
        d.new_column(None);
        let col = d.col_sel.unwrap();
        d.brush_radius = 0;
        d.hover = Some((60, 60));
        let i = 60 * MAP_W + 60;
        d.edit_cell(i, |c| c.tex = -(col as i16));
        d.end_stroke("paint column");
        assert_eq!(d.level.cells[i].tex, -(col as i16));
        // checks still pass for errors on an edited original; save and reload
        let tmp = std::env::temp_dir().join(format!("c8-edit-structure-{}.dat", std::process::id()));
        d.save_to(&tmp).unwrap();
        let back = Level::read_file(&tmp).unwrap();
        let sorted = |v: &[c8_data::level::Entity]| {
            let mut v = v.to_vec();
            v.sort_by_key(|e| e.index);
            v
        };
        assert_eq!(sorted(&back.entities), sorted(&d.level.entities));
        assert_eq!(back.regions.len(), d.level.regions.len());
        assert_eq!(back.columns[col], d.level.columns[col]);
        assert_eq!(back.cells[i].tex, -(col as i16));
        let _ = std::fs::remove_file(tmp);
        // undo everything
        while d.can_undo() {
            d.undo();
        }
        assert_eq!(d.level.entities.len(), n0);
    }
}
