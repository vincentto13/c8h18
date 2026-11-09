//! Structural edits (entities, regions, columns, blocks, texture attributes; with any cell changes
//! they cause) as snapshot commands, plus the names of the entity types (spec 01 §9.4).
use super::LevelDoc;
use crate::doc::history::Command;
use c8_data::level::{Block, Cell, Column, Entity, Level, Region};

/// The parts of a level a structural edit can change, besides cells.
#[derive(Clone, PartialEq)]
pub(super) struct Structure {
    columns: Vec<Column>,
    blocks: Vec<Block>,
    entities: Vec<Entity>,
    regions: Vec<Region>,
    tex_attr: [u16; 256],
}

impl Structure {
    fn of(l: &Level) -> Structure {
        Structure { columns: l.columns.clone(), blocks: l.blocks.clone(), entities: l.entities.clone(), regions: l.regions.clone(), tex_attr: l.tex_attr }
    }
    fn put(&self, l: &mut Level) {
        l.columns.clone_from(&self.columns);
        l.blocks.clone_from(&self.blocks);
        l.entities.clone_from(&self.entities);
        l.regions.clone_from(&self.regions);
        l.tex_attr = self.tex_attr;
    }
}

/// State before an edit in progress.
pub(super) struct Pending {
    label: String,
    before: Structure,
    cells: Vec<Cell>,
}

struct StructEdit {
    label: String,
    before: Structure,
    after: Structure,
    cells: Vec<(usize, Cell, Cell)>,
}

impl Command<Level> for StructEdit {
    fn apply(&mut self, l: &mut Level) {
        self.after.put(l);
        for &(i, _, new) in &self.cells {
            l.cells[i] = new;
        }
    }
    fn revert(&mut self, l: &mut Level) {
        self.before.put(l);
        for &(i, old, _) in &self.cells {
            l.cells[i] = old;
        }
    }
    fn label(&self) -> String {
        self.label.clone()
    }
}

impl LevelDoc {
    /// Start an edit (or continue the one in progress): changes to the level from now on become
    /// one undo step at [`Self::finish_edit`].
    pub(super) fn begin_edit(&mut self, label: &str) {
        if let Some(p) = &mut self.pending {
            p.label = label.into();
            return;
        }
        self.pending = Some(Pending { label: label.into(), before: Structure::of(&self.level), cells: self.level.cells.clone() });
    }

    pub(super) fn finish_edit(&mut self) {
        let Some(p) = self.pending.take() else { return };
        let after = Structure::of(&self.level);
        let cells: Vec<(usize, Cell, Cell)> = p.cells.iter().zip(&self.level.cells).enumerate().filter(|(_, (a, b))| a != b).map(|(i, (a, b))| (i, *a, *b)).collect();
        if after == p.before && cells.is_empty() {
            return;
        }
        let mut c: Box<dyn Command<Level>> = Box::new(StructEdit { label: p.label, before: p.before, after, cells });
        // already applied: take it back so that History applies it once
        c.revert(&mut self.level);
        self.history.run(c, &mut self.level);
        self.level.update_extent();
        self.touch();
    }

    /// Abandon the edit in progress, restoring the level as it was at its start.
    pub(super) fn cancel_edit(&mut self) {
        if let Some(p) = self.pending.take() {
            p.before.put(&mut self.level);
            self.level.cells = p.cells;
            self.level.update_extent();
            self.touch();
        }
    }

    /// One complete edit.
    pub(super) fn edit(&mut self, label: &str, f: impl FnOnce(&mut Level)) {
        self.finish_edit();
        self.begin_edit(label);
        f(&mut self.level);
        self.finish_edit();
    }

    /// Finish a widget edit when the pointer is released and no text field has focus.
    pub(super) fn settle_edit(&mut self, ctx: &eframe::egui::Context) {
        if self.pending.is_some() && !ctx.input(|i| i.pointer.any_down()) && ctx.memory(|m| m.focused().is_none()) {
            self.finish_edit();
        }
    }
}

/// Name of an entity type.
pub fn type_name(kind: u8) -> &'static str {
    match kind {
        1 => "Checkpoint",
        2 => "Effect / morph",
        3 => "Scenery / camera",
        4 => "Shot",
        5 => "Pickup",
        8 => "Trigger",
        9 => "Wall / waypoint",
        10 => "Recovery truck",
        _ => "Unknown",
    }
}

const EFFECTS: [&str; 23] = [
    "Explosion (particle)",
    "Explosion medium",
    "Explosion big",
    "Explosion massive",
    "Bullet sparks",
    "Fire (steam strong)",
    "Hole",
    "Smoke",
    "Smoke fire (steam light)",
    "Light (morph source marker)",
    "Earthquake",
    "Tremor",
    "Gas",
    "Altitude",
    "Shade",
    "Move area",
    "Swap area",
    "Copy area",
    "Remove area",
    "Mark area",
    "Sand",
    "Water",
    "Flare",
];
const SCENERY: [&str; 11] = ["Sign post", "Tree", "Lamp post", "Camera static", "Camera follow", "Clone", "Cone 1", "Cone 2", "Barrel 1", "Barrel 2", "Barrel 3"];
const PICKUPS: [&str; 14] = [
    "Invincible",
    "Invisible",
    "Extra shield",
    "Full shield",
    "Double shield",
    "Extra ammo",
    "Full ammo",
    "Double ammo",
    "Extra fuel",
    "Full fuel",
    "Double fuel",
    "Minigun upgrade",
    "Missile upgrade",
    "Booster upgrade",
];
const TRIGGERS: [&str; 4] = ["Boundary", "Timer", "Action", "On effect"];
const LINKS: [&str; 11] = ["Wall", "Waypoint normal", "Waypoint fuel", "Waypoint ammo", "Waypoint shield", "Waypoint (5, unused)", "Waypoint shortcut", "Waypoint brake", "Waypoint hold fire", "Waypoint fire", "Waypoint special"];

/// The sub-types of a type, as (sub-type, name).
pub fn sub_types(kind: u8) -> Vec<(u8, &'static str)> {
    let list: &[&str] = match kind {
        1 => return vec![(5, "Checkpoint")],
        2 => &EFFECTS,
        3 => &SCENERY,
        5 => &PICKUPS,
        8 => &TRIGGERS,
        9 => &LINKS,
        10 => return vec![(9, "Recovery truck")],
        _ => &[],
    };
    list.iter().enumerate().map(|(i, n)| (i as u8, *n)).collect()
}

pub fn entity_name(e: &Entity) -> String {
    match sub_types(e.kind).iter().find(|(s, _)| *s == e.sub) {
        Some((_, n)) => (*n).to_string(),
        None => format!("{} {}", type_name(e.kind), e.sub),
    }
}

/// What the fields mean for this entity: labels for (link, value, width, height, target group);
/// `None` = not used.
pub struct FieldUse {
    pub link: Option<&'static str>,
    pub value: Option<&'static str>,
    pub w: Option<&'static str>,
    pub h: Option<&'static str>,
    pub target: Option<&'static str>,
}

pub fn field_use(e: &Entity) -> FieldUse {
    let none = FieldUse { link: None, value: None, w: None, h: None, target: None };
    match (e.kind, e.sub) {
        (1, _) => FieldUse { value: Some("order (0 = start/finish)"), w: Some("width − 1"), h: Some("height − 1"), ..none },
        (2, 15..=19) => FieldUse { link: Some("source area"), w: Some("width − 1"), h: Some("height − 1"), ..none },
        (8, 0) => FieldUse { target: Some("activates group"), w: Some("half width"), h: Some("half height"), ..none },
        (8, 1) => FieldUse { target: Some("activates group"), value: Some("delay (ticks − 1)"), ..none },
        (8, _) => FieldUse { target: Some("activates group"), ..none },
        (9, 0) => FieldUse { link: Some("wall to"), ..none },
        (9, _) => FieldUse { link: Some("next waypoint"), ..none },
        _ => none,
    }
}

/// Entities the "Add" menu offers: (label, type, sub-type, draws a chain).
pub const TEMPLATES: &[(&str, u8, u8, bool)] = &[
    ("Wall chain", 9, 0, true),
    ("Waypoint chain (normal)", 9, 1, true),
    ("Waypoint chain (shortcut)", 9, 6, true),
    ("Waypoint chain (fuel charger)", 9, 2, true),
    ("Waypoint chain (ammo charger)", 9, 3, true),
    ("Waypoint chain (shield charger)", 9, 4, true),
    ("Waypoint chain (brake)", 9, 7, true),
    ("Waypoint chain (hold fire)", 9, 8, true),
    ("Waypoint chain (fire)", 9, 9, true),
    ("Checkpoint", 1, 5, false),
    ("Recovery truck", 10, 9, false),
    ("Pickup: minigun upgrade", 5, 11, false),
    ("Pickup: missile upgrade", 5, 12, false),
    ("Pickup: booster upgrade", 5, 13, false),
    ("Pickup: extra shield", 5, 2, false),
    ("Pickup: extra ammo", 5, 5, false),
    ("Pickup: extra fuel", 5, 8, false),
    ("Pickup: invincible", 5, 0, false),
    ("Pickup: invisible", 5, 1, false),
    ("Cone", 3, 6, false),
    ("Barrel", 3, 8, false),
    ("Camera (static)", 3, 3, false),
    ("Trigger: boundary", 8, 0, false),
    ("Trigger: timer", 8, 1, false),
    ("Trigger: on effect", 8, 3, false),
    ("Explosion massive", 2, 3, false),
    ("Fire", 2, 5, false),
    ("Swap area (morph)", 2, 16, false),
    ("Light (morph source)", 2, 9, false),
];
