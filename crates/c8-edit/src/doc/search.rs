//! Search-ring editor (`DATA/SRCH0-0.DAT`, spec 01 §10, 06 §6.6): the 32 × 32 grid of rings
//! around a centre that the big and massive explosions spread their flares and particles over.
//! Paint cells into rings, move start cells, regenerate the circles, and watch the spread.
use super::history::{Command, History};
use super::Document;
use crate::resources::Kind;
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use c8_data::search::{self, ring_of, Rings, BORDER, OUTSIDE, RINGS, SIZE};
use std::path::{Path, PathBuf};

struct GridEdit {
    label: String,
    before: Vec<u8>,
    after: Vec<u8>,
}

impl Command<Vec<u8>> for GridEdit {
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

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Paint,
    Outside,
    Start,
}

#[derive(Clone, Copy, PartialEq)]
enum Spread {
    /// one ring per step
    Rings,
    /// big explosion: 6 ticks, ring n mod 2
    Big,
    /// massive explosion: 3 ticks, rings 2n and 2n + 1
    Massive,
}

impl Spread {
    fn ticks(self) -> usize {
        match self {
            Spread::Rings => RINGS,
            Spread::Big => 6,
            Spread::Massive => 3,
        }
    }
    fn rings(self, n: usize) -> Vec<usize> {
        match self {
            Spread::Rings => vec![n],
            Spread::Big => vec![n % 2],
            Spread::Massive => vec![2 * n, 2 * n + 1],
        }
    }
}

pub struct SearchDoc {
    path: PathBuf,
    rel: String,
    data: Vec<u8>,
    history: History<Vec<u8>>,
    tool: Tool,
    brush: usize,
    /// grid before the current paint stroke
    stroke: Option<Vec<u8>>,
    show_order: bool,
    spread: Spread,
    /// animation start time, if playing
    playing: Option<f64>,
    rate: f32,
    message: String,
    test: bool,
}

/// A colour per ring, cycling through hues so neighbouring rings differ.
fn ring_colour(k: usize) -> Color32 {
    let h = (k as f32 * 0.382).fract();
    let c = egui::ecolor::Hsva::new(h, 0.55, 0.85, 1.0);
    c.into()
}

impl SearchDoc {
    pub fn open(path: PathBuf, rel: String) -> Result<SearchDoc, String> {
        let raw = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut data = c8_data::rnc::maybe_unpack(raw).map_err(|e| format!("{rel}: {e}"))?;
        if data.len() < SIZE * SIZE {
            return Err(format!("{rel}: {} bytes, the search-ring grid has {}", data.len(), SIZE * SIZE));
        }
        data.truncate(SIZE * SIZE);
        Ok(SearchDoc { path, rel, data, history: History::default(), tool: Tool::Paint, brush: 1, stroke: None, show_order: false, spread: Spread::Massive, playing: None, rate: 8.0, message: String::new(), test: false })
    }

    fn commit(&mut self, label: &str, before: Vec<u8>) {
        if before != self.data {
            let after = self.data.clone();
            self.history.run(Box::new(GridEdit { label: label.into(), before, after }), &mut self.data);
        }
    }

    fn set_grid(&mut self, label: &str, g: Vec<u8>) {
        let before = std::mem::replace(&mut self.data, g);
        let changed = before.iter().zip(&self.data).filter(|(a, b)| a != b).count();
        self.message = format!("{label}: {changed} cells changed");
        self.commit(label, before);
    }

    /// Apply the current tool to a cell.
    fn apply_tool(&mut self, i: usize) {
        let (x, y) = (i % SIZE, i / SIZE);
        if x == SIZE - 1 || y == SIZE - 1 {
            return; // the border stays
        }
        match self.tool {
            Tool::Paint => {
                if ring_of(self.data[i]) != Some(self.brush) {
                    self.data[i] = 0x10 + self.brush as u8;
                }
            }
            Tool::Outside => self.data[i] = OUTSIDE,
            Tool::Start => {
                let Some(k) = ring_of(self.data[i]) else { return };
                // the previous start cell of this ring becomes a plain ring cell
                for b in self.data.iter_mut() {
                    if *b as usize == k {
                        *b = 0x10 + k as u8;
                    }
                }
                self.data[i] = k as u8;
            }
        }
    }

    /// Rings lit at the current animation step.
    fn lit(&self, ui: &egui::Ui) -> Option<(usize, Vec<usize>)> {
        let t0 = self.playing?;
        let now = ui.input(|i| i.time);
        let ticks = self.spread.ticks();
        // one pause step after the last tick
        let n = (((now - t0) * self.rate as f64) as usize) % (ticks + 1);
        ui.ctx().request_repaint();
        Some((n, if n < ticks { self.spread.rings(n) } else { vec![] }))
    }
}

impl Document for SearchDoc {
    fn title(&self) -> String {
        self.rel.clone()
    }
    fn kind(&self) -> Kind {
        Kind::SearchRings
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
    }
    fn redo(&mut self) {
        self.history.redo(&mut self.data);
    }
    fn can_undo(&self) -> bool {
        self.history.can_undo()
    }
    fn can_redo(&self) -> bool {
        self.history.can_redo()
    }
    fn game_files(&self) -> Result<Vec<(String, Vec<u8>)>, String> {
        Ok(vec![("DATA/SRCH0-0.DAT".into(), self.data.clone())])
    }
    fn game_args(&self) -> Vec<String> {
        vec!["--no-intro".into(), "--race".into(), "1".into()]
    }
    fn wants_test(&mut self) -> bool {
        std::mem::take(&mut self.test)
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        let rings = Rings::build(&self.data);
        let missed = rings.unreached(&self.data);
        // walk position of every cell: (ring, index in the ring)
        let mut order = vec![None; SIZE * SIZE];
        for (k, r) in rings.rings.iter().enumerate() {
            for (n, &(i, _)) in r.iter().enumerate() {
                order[i] = Some((k, n));
            }
        }
        let lit = self.lit(ui);

        ui.horizontal(|ui| {
            ui.checkbox(&mut self.show_order, "Walk order").on_hover_text("Number each cell by its position in its ring's walk (the order the game places effects)");
            ui.separator();
            if let Some((n, _)) = &lit {
                ui.label(format!("tick {n}"));
            }
        });
        ui.separator();

        let avail = ui.available_size() - Vec2::new(0.0, 60.0);
        let cell = (avail.x.min(avail.y) / SIZE as f32).floor().clamp(8.0, 28.0);
        let (resp, painter) = ui.allocate_painter(Vec2::splat(cell * SIZE as f32), Sense::click_and_drag());
        let origin = resp.rect.min;
        let rect_of = |i: usize| Rect::from_min_size(origin + Vec2::new((i % SIZE) as f32 * cell, (i / SIZE) as f32 * cell), Vec2::splat(cell));
        for i in 0..SIZE * SIZE {
            let b = self.data[i];
            let r = rect_of(i).shrink(0.5);
            let fill = match ring_of(b) {
                Some(k) => {
                    let c = ring_colour(k);
                    match &lit {
                        Some((_, on)) if on.contains(&k) => Color32::WHITE,
                        Some(_) => c.gamma_multiply(0.35),
                        None => c,
                    }
                }
                None if b == BORDER => Color32::from_gray(24),
                None => Color32::from_gray(48),
            };
            painter.rect_filled(r, 1.0, fill);
            if b < 0x10 {
                // start cell
                painter.circle_filled(r.center(), cell * 0.18, Color32::BLACK);
            }
            if missed.contains(&i) {
                painter.rect_stroke(r.shrink(1.0), 1.0, Stroke::new(2.0, Color32::RED), egui::StrokeKind::Inside);
            }
            if self.show_order && cell >= 14.0 {
                if let Some((_, n)) = order[i] {
                    painter.text(r.center(), egui::Align2::CENTER_CENTER, n.to_string(), egui::FontId::monospace(cell * 0.4), Color32::BLACK);
                }
            }
        }
        // the centre
        let c = rings.centre;
        if c.0 >= 0 && c.1 >= 0 {
            let r = rect_of(c.1 as usize * SIZE + c.0 as usize);
            painter.rect_stroke(r, 1.0, Stroke::new(2.0, Color32::BLACK), egui::StrokeKind::Inside);
        }

        let cell_at = |p: Pos2| {
            let v = (p - origin) / cell;
            (v.x >= 0.0 && v.y >= 0.0 && v.x < SIZE as f32 && v.y < SIZE as f32).then(|| v.y as usize * SIZE + v.x as usize)
        };
        if resp.drag_started() || resp.clicked() {
            self.stroke = Some(self.data.clone());
        }
        if resp.dragged() || resp.clicked() || resp.drag_started() {
            if let Some(i) = resp.interact_pointer_pos().and_then(cell_at) {
                self.apply_tool(i);
            }
        }
        if resp.drag_stopped() || resp.clicked() {
            if let Some(before) = self.stroke.take() {
                let label = match self.tool {
                    Tool::Paint => format!("paint ring {}", self.brush),
                    Tool::Outside => "clear cells".into(),
                    Tool::Start => "move start cell".into(),
                };
                self.commit(&label, before);
            }
        }
        if let Some(i) = resp.hover_pos().and_then(cell_at) {
            let b = self.data[i];
            let (x, y) = ((i % SIZE) as i32, (i / SIZE) as i32);
            let what = match (ring_of(b), order[i]) {
                (Some(k), Some((_, n))) => format!("ring {k}{}, walk step {n} of {}", if b < 0x10 { " (start cell)" } else { "" }, rings.rings[k].len()),
                (Some(k), None) => format!("ring {k}, NOT reached by the walk"),
                _ if b == BORDER => "unused border".into(),
                _ => "outside the rings".into(),
            };
            resp.on_hover_text(format!("cell ({x}, {y}), offset ({}, {}), byte 0x{b:02X}\n{what}", x - c.0, y - c.1));
        }
        ui.label(RichText::new("Each ring is walked from its start cell (black dot, straight above the centre) around its neighbours; the game uses the offsets from the centre cell (outlined). Red outlines: ring cells the walk never reaches, which the game ignores.").small().color(Color32::GRAY));
    }

    fn side_ui(&mut self, ui: &mut egui::Ui) {
        if ui.button("Test in game").on_hover_text("Race on track 1 with this grid (fire a missile or mine to see the explosions)").clicked() {
            self.test = true;
        }
        ui.separator();
        ui.heading("Tool");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tool, Tool::Paint, "Paint");
            ui.selectable_value(&mut self.tool, Tool::Outside, "Clear");
            ui.selectable_value(&mut self.tool, Tool::Start, "Start cell");
        });
        if self.tool == Tool::Paint {
            ui.horizontal(|ui| {
                ui.label("Ring");
                ui.add(egui::DragValue::new(&mut self.brush).range(0..=RINGS - 1));
                let (r, _) = ui.allocate_exact_size(Vec2::new(18.0, 18.0), Sense::hover());
                ui.painter().rect_filled(r, 3.0, ring_colour(self.brush));
            });
        }
        ui.label(RichText::new(match self.tool {
            Tool::Paint => "Click or drag to put cells into the ring.",
            Tool::Outside => "Click or drag to take cells out of the rings.",
            Tool::Start => "Click a ring cell to make it the ring's start cell (ring 0's start is the centre).",
        }).small().color(Color32::GRAY));

        ui.separator();
        ui.heading("Spread");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.spread, Spread::Massive, "Massive").on_hover_text("Massive explosion: 3 ticks, a particle on every cell of rings 2n and 2n+1, 128 units per cell");
            ui.selectable_value(&mut self.spread, Spread::Big, "Big").on_hover_text("Big explosion: 6 ticks, a flare on every cell of ring n mod 2, 180 units per cell");
            ui.selectable_value(&mut self.spread, Spread::Rings, "All rings");
        });
        ui.horizontal(|ui| {
            if self.playing.is_some() {
                if ui.button("Stop").clicked() {
                    self.playing = None;
                }
            } else if ui.button("Play").clicked() {
                self.playing = Some(ui.input(|i| i.time));
            }
            ui.add(egui::Slider::new(&mut self.rate, 1.0..=30.0).text("ticks/s"));
        });

        ui.separator();
        ui.heading("Rings");
        let rings = Rings::build(&self.data);
        egui::Grid::new("rings").striped(true).show(ui, |ui| {
            for k in 0..RINGS {
                let (r, _) = ui.allocate_exact_size(Vec2::new(12.0, 12.0), Sense::hover());
                ui.painter().rect_filled(r, 2.0, ring_colour(k));
                ui.label(format!("{k}"));
                let n = rings.rings.get(k).map_or(0, |r| r.len());
                ui.label(format!("{n} cells"));
                ui.label(RichText::new(match k {
                    0 | 1 => "big + massive",
                    2..=5 => "massive",
                    _ => "unused",
                }).small().color(Color32::GRAY));
                ui.end_row();
            }
        });
        ui.label(RichText::new("The game reads rings 0–5 only; the rest of the grid is never used.").small().color(Color32::GRAY));

        ui.separator();
        let problems = search::check(&self.data);
        if problems.is_empty() {
            ui.label(RichText::new("No problems").color(Color32::LIGHT_GREEN));
        }
        for p in problems {
            ui.label(RichText::new(p).color(Color32::from_rgb(255, 170, 80)));
        }
        if ui.button("Regenerate circles").on_hover_text("Concentric rings by distance (the original differs in 16 hand-placed cells)").clicked() {
            self.set_grid("regenerate circles", search::generate());
        }
        if !self.message.is_empty() {
            ui.label(RichText::new(&self.message).small());
        }
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
    fn paint_regenerate_and_undo() {
        let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources"));
        let Ok(mut d) = SearchDoc::open(root.join("DATA/SRCH0-0.DAT"), "DATA/SRCH0-0.DAT".into()) else { return };
        let orig = d.data.clone();
        // move ring 1's start cell to the cell right of the centre
        d.tool = Tool::Start;
        let before = d.data.clone();
        d.apply_tool(15 * SIZE + 16);
        d.commit("start", before);
        assert_eq!(d.data[15 * SIZE + 16], 1);
        assert_eq!(d.data.iter().filter(|&&b| b == 1).count(), 1);
        d.set_grid("regen", search::generate());
        assert!(d.dirty());
        d.undo();
        d.undo();
        assert_eq!(d.data, orig);
        assert!(!d.dirty());
    }
}
