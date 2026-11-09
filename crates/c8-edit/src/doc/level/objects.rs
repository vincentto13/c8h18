//! Objects tool: select, move, add and delete entities and map regions, edit their fields, and draw
//! wall and waypoint chains.
use super::edit::{entity_name, field_use, sub_types, type_name, TEMPLATES};
use super::LevelDoc;
use eframe::egui::{self, Color32, Vec2};
use c8_data::level::{Entity, Region, MAP_H, MAP_W};

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Sel {
    /// entity record number
    Entity(usize),
    /// region thing-record slot
    Region(usize),
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum ObjMode {
    Select,
    /// place entities of template i with each click
    Place(usize),
    /// draw a chain of template i; `last` = the previous point
    Chain(usize, Option<usize>),
    /// place a region of this kind
    PlaceRegion(u8),
    /// the next click on an entity sets the selected entity's link
    PickLink,
}

pub(super) enum Drag {
    /// move by cells; offset from the pointer cell to the entity cell
    Entity(usize, (i32, i32)),
    /// move a region; offset from the pointer to the centre
    Region(usize, Vec2),
    Resize(usize),
}

/// Which entities are drawn and can be picked.
pub(super) struct Layers {
    pub walls: bool,
    pub waypoints: bool,
    pub checkpoints: bool,
    pub pickups: bool,
    pub scenery: bool,
    pub effects: bool,
    pub trucks: bool,
}

impl Default for Layers {
    fn default() -> Self {
        Layers { walls: true, waypoints: true, checkpoints: true, pickups: true, scenery: true, effects: true, trucks: true }
    }
}

impl Layers {
    pub fn shows(&self, e: &Entity) -> bool {
        match (e.kind, e.sub) {
            (9, 0) => self.walls,
            (9, _) => self.waypoints,
            (1, _) => self.checkpoints,
            (5, _) => self.pickups,
            (3, _) => self.scenery,
            (2, _) | (8, _) => self.effects,
            (10, _) => self.trucks,
            _ => true,
        }
    }
}

pub(super) const REGION_KINDS: [(u8, &str); 4] = [(4, "Start area"), (1, "Shield charger"), (2, "Fuel charger"), (3, "Ammo charger")];

pub(super) fn region_name(kind: u8) -> &'static str {
    REGION_KINDS.iter().find(|r| r.0 == kind).map_or("Region", |r| r.1)
}

fn snap_half(v: f32) -> f32 {
    (v * 2.0).round() / 2.0
}

impl LevelDoc {
    pub(super) fn entity(&self, index: usize) -> Option<&Entity> {
        self.level.entities.iter().find(|e| e.index == index)
    }
    pub(super) fn entity_mut(&mut self, index: usize) -> Option<&mut Entity> {
        self.level.entities.iter_mut().find(|e| e.index == index)
    }
    fn region_mut(&mut self, slot: usize) -> Option<&mut Region> {
        self.level.regions.iter_mut().find(|r| r.slot == slot)
    }

    /// The visible entity nearest to map point p, within picking distance.
    fn hit_entity(&self, p: Vec2) -> Option<usize> {
        let r = (7.0 / self.zoom).max(0.6);
        self.level
            .entities
            .iter()
            .filter(|e| self.layers.shows(e))
            .map(|e| (e.index, (Vec2::new(e.x as f32 + 0.5, e.y as f32 + 0.5) - p).length()))
            .filter(|&(_, d)| d <= r)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    fn hit_region(&self, p: Vec2) -> Option<usize> {
        self.level.regions.iter().filter(|r| (p.x - r.cx).abs() <= r.hw && (p.y - r.cy).abs() <= r.hh).min_by(|a, b| (a.hw * a.hh).total_cmp(&(b.hw * b.hh))).map(|r| r.slot)
    }

    /// Is p on a corner handle of the selected region?
    fn hit_region_corner(&self, p: Vec2) -> Option<usize> {
        let Some(Sel::Region(s)) = self.obj_sel else { return None };
        let r = self.level.regions.iter().find(|r| r.slot == s)?;
        let tol = (6.0 / self.zoom).max(0.35);
        let near = |a: f32, b: f32| (a - b).abs() <= tol;
        ((near(p.x, r.cx - r.hw) || near(p.x, r.cx + r.hw)) && (near(p.y, r.cy - r.hh) || near(p.y, r.cy + r.hh))).then_some(s)
    }

    /// Add an entity at a cell (inside an edit); returns its record number.
    pub(super) fn add_entity(&mut self, kind: u8, sub: u8, x: i32, y: i32) -> Option<usize> {
        let Some(index) = self.level.free_entity_index() else {
            self.message = "all 3999 entity records are in use".into();
            return None;
        };
        let mut e = Entity { index, kind, sub, group: 1, x: x.clamp(0, MAP_W as i32 - 1), y: y.clamp(0, MAP_H as i32 - 1), ..Default::default() };
        match (kind, sub) {
            (1, _) => {
                e.value = self.level.entities.iter().filter(|e| e.kind == 1).map(|e| e.value + 1).max().unwrap_or(0);
                e.h = 4;
            }
            (8, 0) => (e.w, e.h) = (2, 2),
            (2, 15..=19) => (e.w, e.h) = (1, 1),
            _ => {}
        }
        self.level.entities.push(e);
        Some(index)
    }

    pub(super) fn delete_selected(&mut self) {
        match self.obj_sel {
            Some(Sel::Entity(i)) => {
                let name = self.entity(i).map(entity_name).unwrap_or_default();
                self.edit(&format!("delete {name} {i}"), |l| {
                    l.entities.retain(|e| e.index != i);
                    for e in l.entities.iter_mut().filter(|e| e.link as usize == i) {
                        e.link = 0;
                    }
                });
            }
            Some(Sel::Region(s)) => self.edit("delete region", |l| l.regions.retain(|r| r.slot != s)),
            None => return,
        }
        self.obj_sel = None;
    }

    pub(super) fn duplicate_selected(&mut self) {
        let Some(Sel::Entity(i)) = self.obj_sel else { return };
        let Some(src) = self.entity(i).copied() else { return };
        self.finish_edit();
        self.begin_edit(&format!("duplicate {} {i}", entity_name(&src)));
        if let Some(n) = self.add_entity(src.kind, src.sub, src.x + 1, src.y) {
            if let Some(e) = self.entity_mut(n) {
                *e = Entity { index: n, x: e.x, y: e.y, ..src };
            }
            self.obj_sel = Some(Sel::Entity(n));
        }
        self.finish_edit();
    }

    /// End the chain being drawn (or any placing mode).
    pub(super) fn cancel_mode(&mut self) {
        self.finish_edit();
        self.obj_mode = ObjMode::Select;
    }

    /// Pointer handling of the Objects tool.
    pub(super) fn objects_input(&mut self, resp: &egui::Response, press: Option<Vec2>) {
        let Some(p) = self.hover_f else {
            if resp.drag_stopped() {
                self.drag = None;
                self.finish_edit();
            }
            return;
        };
        let cell = (p.x.floor() as i32, p.y.floor() as i32);
        match self.obj_mode {
            ObjMode::Place(t) => {
                if resp.clicked() {
                    let (label, kind, sub, _) = TEMPLATES[t];
                    self.finish_edit();
                    self.begin_edit(&format!("add {label}"));
                    if let Some(i) = self.add_entity(kind, sub, cell.0, cell.1) {
                        self.obj_sel = Some(Sel::Entity(i));
                    }
                    self.finish_edit();
                }
            }
            ObjMode::Chain(t, last) => {
                if resp.clicked() {
                    let (label, kind, sub, _) = TEMPLATES[t];
                    let hit = self.hit_entity(p).filter(|&i| self.entity(i).is_some_and(|e| e.kind == 9));
                    self.begin_edit(&format!("draw {label}"));
                    match (hit, last) {
                        (Some(h), Some(l)) if h != l => {
                            // connect to an existing point and finish the chain
                            if let Some(e) = self.entity_mut(l) {
                                e.link = h as u16;
                            }
                            self.obj_sel = Some(Sel::Entity(h));
                            self.cancel_mode();
                        }
                        (Some(h), None) => {
                            // continue from an existing point
                            self.obj_mode = ObjMode::Chain(t, Some(h));
                            self.obj_sel = Some(Sel::Entity(h));
                        }
                        _ => {
                            if let Some(n) = self.add_entity(kind, sub, cell.0, cell.1) {
                                if let Some(e) = last.and_then(|l| self.entity_mut(l)) {
                                    e.link = n as u16;
                                }
                                self.obj_mode = ObjMode::Chain(t, Some(n));
                                self.obj_sel = Some(Sel::Entity(n));
                            }
                        }
                    }
                }
            }
            ObjMode::PlaceRegion(kind) => {
                if resp.clicked() {
                    match self.level.free_region_slot() {
                        Some(slot) => {
                            let r = Region { slot, kind, cx: cell.0 as f32 + 0.5, cy: cell.1 as f32 + 0.5, hw: 1.5, hh: 1.5 };
                            self.edit(&format!("add {}", region_name(kind)), |l| l.regions.push(r));
                            self.obj_sel = Some(Sel::Region(slot));
                        }
                        None => self.message = "all 10 region records are in use".into(),
                    }
                    self.obj_mode = ObjMode::Select;
                }
            }
            ObjMode::PickLink => {
                if resp.clicked() {
                    if let (Some(Sel::Entity(s)), Some(h)) = (self.obj_sel, self.hit_entity(p)) {
                        self.edit(&format!("link {s} to {h}"), |l| {
                            if let Some(e) = l.entities.iter_mut().find(|e| e.index == s) {
                                e.link = h as u16;
                            }
                        });
                    }
                    self.obj_mode = ObjMode::Select;
                }
            }
            ObjMode::Select => {
                if resp.drag_started_by(egui::PointerButton::Primary) {
                    let p0 = press.unwrap_or(p);
                    let c0 = (p0.x.floor() as i32, p0.y.floor() as i32);
                    if let Some(s) = self.hit_region_corner(p0) {
                        self.drag = Some(Drag::Resize(s));
                        self.begin_edit("resize region");
                    } else if let Some(i) = self.hit_entity(p0) {
                        let e = self.entity(i).copied().unwrap_or_default();
                        self.obj_sel = Some(Sel::Entity(i));
                        self.drag = Some(Drag::Entity(i, (e.x - c0.0, e.y - c0.1)));
                        self.begin_edit(&format!("move {} {i}", entity_name(&e)));
                    } else if let Some(s) = self.hit_region(p0) {
                        let r = self.level.regions.iter().find(|r| r.slot == s).copied().unwrap_or_default();
                        self.obj_sel = Some(Sel::Region(s));
                        self.drag = Some(Drag::Region(s, Vec2::new(r.cx, r.cy) - p0));
                        self.begin_edit(&format!("move {}", region_name(r.kind)));
                    }
                }
                match self.drag {
                    Some(Drag::Entity(i, (ox, oy))) => {
                        let (x, y) = ((cell.0 + ox).clamp(0, MAP_W as i32 - 1), (cell.1 + oy).clamp(0, MAP_H as i32 - 1));
                        if let Some(e) = self.entity_mut(i) {
                            (e.x, e.y) = (x, y);
                        }
                    }
                    Some(Drag::Region(s, off)) => {
                        let c = p + off;
                        if let Some(r) = self.region_mut(s) {
                            (r.cx, r.cy) = (snap_half(c.x).clamp(0.5, MAP_W as f32 - 0.5), snap_half(c.y).clamp(0.5, MAP_H as f32 - 0.5));
                        }
                    }
                    Some(Drag::Resize(s)) => {
                        if let Some(r) = self.region_mut(s) {
                            r.hw = snap_half((p.x - r.cx).abs()).max(0.5);
                            r.hh = snap_half((p.y - r.cy).abs()).max(0.5);
                        }
                    }
                    None => {}
                }
                if resp.drag_stopped() {
                    self.drag = None;
                    self.finish_edit();
                }
                if resp.clicked() {
                    self.obj_sel = self.hit_entity(p).map(Sel::Entity).or_else(|| self.hit_region(p).map(Sel::Region));
                }
            }
        }
    }

    /// Centre the view on a map point (next frame).
    pub(super) fn centre_on(&mut self, x: f32, y: f32) {
        if self.zoom < 6.0 {
            self.zoom = 8.0;
        }
        self.pan = Vec2::new(x, y) * self.zoom - self.view_size / 2.0;
    }

    pub(super) fn objects_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(self.obj_mode == ObjMode::Select, "Select / move").clicked() {
                self.cancel_mode();
            }
            let mut t = match self.obj_mode {
                ObjMode::Place(t) | ObjMode::Chain(t, _) => t,
                _ => self.add_template,
            };
            egui::ComboBox::from_id_salt("add-template").selected_text(TEMPLATES[t].0).width(170.0).show_ui(ui, |ui| {
                for (i, tpl) in TEMPLATES.iter().enumerate() {
                    ui.selectable_value(&mut t, i, tpl.0);
                }
            });
            if t != self.add_template {
                self.add_template = t;
                self.start_adding();
            }
            let adding = matches!(self.obj_mode, ObjMode::Place(_) | ObjMode::Chain(..));
            if ui.selectable_label(adding, "Add").on_hover_text("click on the map to add (A)").clicked() {
                self.start_adding();
            }
        });
        match self.obj_mode {
            ObjMode::Select => ui.label("Click to select, drag to move (regions: drag a corner to resize). Delete removes, Cmd+D duplicates."),
            ObjMode::Place(t) => ui.label(format!("Click on the map to add: {}. Escape stops.", TEMPLATES[t].0)),
            ObjMode::Chain(..) => ui.label("Click to add points, each linked from the previous one. Click an existing point to join it (and finish). Escape finishes."),
            ObjMode::PlaceRegion(k) => ui.label(format!("Click on the map to place the {}.", region_name(k))),
            ObjMode::PickLink => ui.label("Click the entity to link to. Escape cancels."),
        };
        ui.horizontal_wrapped(|ui| {
            ui.label("Regions:");
            for (k, name) in REGION_KINDS {
                if ui.small_button(format!("+ {name}")).clicked() {
                    self.finish_edit();
                    self.obj_mode = ObjMode::PlaceRegion(k);
                }
            }
        });
        ui.collapsing("Layers", |ui| {
            ui.horizontal_wrapped(|ui| {
                let l = &mut self.layers;
                ui.checkbox(&mut l.walls, "walls");
                ui.checkbox(&mut l.waypoints, "waypoints");
                ui.checkbox(&mut l.checkpoints, "checkpoints");
                ui.checkbox(&mut l.pickups, "pickups");
                ui.checkbox(&mut l.scenery, "scenery, cameras");
                ui.checkbox(&mut l.effects, "effects, triggers");
                ui.checkbox(&mut l.trucks, "trucks");
            });
        });
        ui.separator();
        match self.obj_sel {
            Some(Sel::Entity(i)) => self.entity_inspector(ui, i),
            Some(Sel::Region(s)) => self.region_inspector(ui, s),
            None => {
                ui.label("Nothing selected.");
            }
        }
        self.entity_list(ui);
    }

    pub(super) fn start_adding(&mut self) {
        self.finish_edit();
        let t = self.add_template;
        self.obj_mode = if TEMPLATES[t].3 { ObjMode::Chain(t, None) } else { ObjMode::Place(t) };
    }

    fn entity_inspector(&mut self, ui: &mut egui::Ui, i: usize) {
        let Some(orig) = self.entity(i).copied() else {
            self.obj_sel = None;
            return;
        };
        let mut e = orig;
        ui.heading(format!("{} (record {i})", entity_name(&e)));
        let mut go_to = None;
        egui::Grid::new("entity").num_columns(2).show(ui, |ui| {
            ui.label("type");
            egui::ComboBox::from_id_salt("etype").selected_text(format!("{} {}", e.kind, type_name(e.kind))).show_ui(ui, |ui| {
                for k in [1u8, 2, 3, 5, 8, 9, 10] {
                    if ui.selectable_label(e.kind == k, format!("{k} {}", type_name(k))).clicked() && e.kind != k {
                        e.kind = k;
                        e.sub = sub_types(k).first().map_or(0, |s| s.0);
                    }
                }
            });
            ui.end_row();
            ui.label("sub-type");
            egui::ComboBox::from_id_salt("esub").selected_text(format!("{} {}", e.sub, entity_name(&e))).show_ui(ui, |ui| {
                for (s, name) in sub_types(e.kind) {
                    ui.selectable_value(&mut e.sub, s, format!("{s} {name}"));
                }
            });
            ui.end_row();
            ui.label("position");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut e.x).range(0..=MAP_W as i32 - 1).prefix("x "));
                ui.add(egui::DragValue::new(&mut e.y).range(0..=MAP_H as i32 - 1).prefix("y "));
            });
            ui.end_row();
            ui.label("group").on_hover_text("created when this group is activated; group 1 at the start");
            ui.add(egui::DragValue::new(&mut e.group));
            ui.end_row();
            let u = field_use(&e);
            let all = self.all_fields;
            if let Some(name) = u.link.or(all.then_some("link")) {
                ui.label(name);
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut e.link).range(0..=3999));
                    if ui.small_button("pick").on_hover_text("click the target on the map").clicked() {
                        self.obj_mode = ObjMode::PickLink;
                    }
                    if e.link != 0 && ui.small_button("go").clicked() {
                        go_to = Some(e.link as usize);
                    }
                });
                ui.end_row();
            }
            if let Some(name) = u.target.or(all.then_some("target group")) {
                ui.label(name);
                ui.add(egui::DragValue::new(&mut e.target_group));
                ui.end_row();
            }
            if let Some(name) = u.value.or(all.then_some("value")) {
                ui.label(name);
                ui.add(egui::DragValue::new(&mut e.value));
                ui.end_row();
            }
            if let Some(name) = u.w.or(all.then_some("width")) {
                ui.label(name);
                ui.add(egui::DragValue::new(&mut e.w).range(0..=255));
                ui.end_row();
            }
            if let Some(name) = u.h.or(all.then_some("height")) {
                ui.label(name);
                ui.add(egui::DragValue::new(&mut e.h).range(0..=255));
                ui.end_row();
            }
        });
        ui.checkbox(&mut self.all_fields, "show all fields");
        if e != orig {
            self.begin_edit(&format!("edit {} {i}", entity_name(&orig)));
            if let Some(t) = self.entity_mut(i) {
                *t = e;
            }
        }
        let from: Vec<usize> = self.level.entities.iter().filter(|o| o.link as usize == i).map(|o| o.index).collect();
        if !from.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label("linked from");
                for f in from.iter().take(8) {
                    if ui.small_button(f.to_string()).clicked() {
                        go_to = Some(*f);
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            if ui.button("Delete").clicked() {
                self.delete_selected();
            }
            if ui.button("Duplicate").clicked() {
                self.duplicate_selected();
            }
            if e.kind == 9 && ui.button("Continue chain").on_hover_text("add points linked from this one").clicked() {
                let t = TEMPLATES.iter().position(|t| t.1 == 9 && t.2 == e.sub).unwrap_or(0);
                self.finish_edit();
                self.obj_mode = ObjMode::Chain(t, Some(i));
            }
        });
        if let Some(t) = go_to.and_then(|t| self.entity(t).copied()) {
            self.obj_sel = Some(Sel::Entity(t.index));
            self.centre_on(t.x as f32 + 0.5, t.y as f32 + 0.5);
        }
    }

    fn region_inspector(&mut self, ui: &mut egui::Ui, s: usize) {
        let Some(orig) = self.level.regions.iter().find(|r| r.slot == s).copied() else {
            self.obj_sel = None;
            return;
        };
        let mut r = orig;
        ui.heading(format!("{} (record {s})", region_name(r.kind)));
        egui::Grid::new("region").num_columns(2).show(ui, |ui| {
            ui.label("kind");
            egui::ComboBox::from_id_salt("rkind").selected_text(region_name(r.kind)).show_ui(ui, |ui| {
                for (k, name) in REGION_KINDS {
                    ui.selectable_value(&mut r.kind, k, name);
                }
            });
            ui.end_row();
            ui.label("centre");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut r.cx).speed(0.5).range(0.0..=MAP_W as f32).fixed_decimals(1));
                ui.add(egui::DragValue::new(&mut r.cy).speed(0.5).range(0.0..=MAP_H as f32).fixed_decimals(1));
            });
            ui.end_row();
            ui.label("half size");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut r.hw).speed(0.5).range(0.5..=64.0).fixed_decimals(1));
                ui.add(egui::DragValue::new(&mut r.hh).speed(0.5).range(0.5..=64.0).fixed_decimals(1));
            });
            ui.end_row();
        });
        if r.kind == 4 {
            ui.label(egui::RichText::new("The starting grid uses the start-tile cells (texture 122) inside the area, front row nearest the start/finish checkpoint.").small().color(Color32::GRAY));
        }
        if r != orig {
            self.begin_edit(&format!("edit {}", region_name(orig.kind)));
            if let Some(t) = self.region_mut(s) {
                *t = r;
            }
        }
        if ui.button("Delete").clicked() {
            self.delete_selected();
        }
    }

    fn entity_list(&mut self, ui: &mut egui::Ui) {
        let mut pick = None;
        egui::CollapsingHeader::new(format!("All entities ({})", self.level.entities.len())).id_salt("entity-list").show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.entity_filter).hint_text("filter by name or record"));
            let f = self.entity_filter.to_ascii_lowercase();
            let rows: Vec<(usize, String)> = self
                .level
                .entities
                .iter()
                .map(|e| (e.index, format!("{:4} {} ({}, {}) g{}", e.index, entity_name(e), e.x, e.y, e.group)))
                .filter(|(_, s)| f.is_empty() || s.to_ascii_lowercase().contains(&f))
                .collect();
            let h = ui.text_style_height(&egui::TextStyle::Body);
            egui::ScrollArea::vertical().id_salt("entity-rows").max_height(220.0).show_rows(ui, h, rows.len(), |ui, range| {
                for (i, s) in &rows[range] {
                    if ui.selectable_label(self.obj_sel == Some(Sel::Entity(*i)), egui::RichText::new(s).monospace().small()).clicked() {
                        pick = Some(*i);
                    }
                }
            });
        });
        if let Some(e) = pick.and_then(|i| self.entity(i).copied()) {
            self.obj_sel = Some(Sel::Entity(e.index));
            self.centre_on(e.x as f32 + 0.5, e.y as f32 + 0.5);
        }
    }
}
