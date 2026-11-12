//! Track checks (what the game needs from a level: checkpoints, start area, waypoints, links,
//! groups, columns) and the AI test, which runs the game's headless computer-only race on the
//! edited level and reports laps and stuck points.
use super::objects::Sel;
use super::LevelDoc;
use eframe::egui::{self, Color32};
use c8_data::level::{Level, MAX_ENTITIES};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver};

pub(super) struct Issue {
    pub error: bool,
    pub text: String,
    pub at: Option<(f32, f32)>,
    pub sel: Option<Sel>,
}

fn issue(error: bool, text: String) -> Issue {
    Issue { error, text, at: None, sel: None }
}

/// Start-tile texture: the starting grid uses these cells inside the start area.
const START_TILE: u16 = 122;

pub(super) fn check(l: &Level) -> Vec<Issue> {
    let mut out = Vec::new();
    let by_index: HashMap<usize, &c8_data::level::Entity> = l.entities.iter().map(|e| (e.index, e)).collect();
    let at_e = |e: &c8_data::level::Entity| Some((e.x as f32 + 0.5, e.y as f32 + 0.5));

    // checkpoints
    let mut cps: Vec<&c8_data::level::Entity> = l.entities.iter().filter(|e| e.kind == 1).collect();
    cps.sort_by_key(|e| e.value);
    if cps.is_empty() {
        out.push(issue(true, "no checkpoints: laps cannot be counted".into()));
    } else {
        if !cps.iter().any(|e| e.value == 0) {
            out.push(issue(true, "no start/finish checkpoint (value 0)".into()));
        }
        let mut seen = HashSet::new();
        for e in &cps {
            if !seen.insert(e.value) {
                out.push(Issue { error: false, text: format!("two checkpoints with order {}", e.value), at: at_e(e), sel: Some(Sel::Entity(e.index)) });
            }
            if e.sub != 5 {
                out.push(Issue { error: false, text: format!("checkpoint {} has sub-type {} (the game uses 5)", e.index, e.sub), at: at_e(e), sel: Some(Sel::Entity(e.index)) });
            }
        }
        let max = cps.last().map_or(0, |e| e.value);
        let missing: Vec<String> = (0..max).filter(|v| !seen.contains(v)).map(|v| v.to_string()).collect();
        if !missing.is_empty() {
            out.push(issue(false, format!("checkpoint orders missing: {}", missing.join(", "))));
        }
    }

    // regions
    let starts: Vec<_> = l.regions.iter().filter(|r| r.kind == 4).collect();
    match starts.len() {
        0 => out.push(issue(true, "no start area: crafts have no starting grid".into())),
        1 => {
            let r = starts[0];
            let mut tiles = 0;
            for y in (r.cy - r.hh).floor() as i32..=(r.cy + r.hh).ceil() as i32 {
                for x in (r.cx - r.hw).floor() as i32..=(r.cx + r.hw).ceil() as i32 {
                    let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
                    if (cx - r.cx).abs() <= r.hw && (cy - r.cy).abs() <= r.hh && l.floor_tex(x, y) == START_TILE {
                        tiles += 1;
                    }
                }
            }
            if tiles < 8 {
                out.push(Issue { error: false, text: format!("start area has {tiles} start tiles (texture {START_TILE}); 8 crafts need 8, otherwise every cell of the area is used"), at: Some((r.cx, r.cy)), sel: Some(Sel::Region(r.slot)) });
            }
        }
        _ => out.push(issue(false, format!("{} start areas: the game uses the last one", starts.len()))),
    }
    for (k, name) in [(1, "shield"), (2, "fuel"), (3, "ammo")] {
        if !l.regions.iter().any(|r| r.kind == k) {
            out.push(issue(false, format!("no {name} charger")));
        }
    }

    // links
    for e in &l.entities {
        if e.link == 0 {
            continue;
        }
        match by_index.get(&(e.link as usize)) {
            None => out.push(Issue { error: true, text: format!("{} {} links to missing entity {}", super::edit::entity_name(e), e.index, e.link), at: at_e(e), sel: Some(Sel::Entity(e.index)) }),
            Some(t) if e.kind == 9 && t.kind != 9 => out.push(Issue { error: false, text: format!("{} {} links to a {} (expected a wall/waypoint point)", super::edit::entity_name(e), e.index, super::edit::entity_name(t)), at: at_e(e), sel: Some(Sel::Entity(e.index)) }),
            _ => {}
        }
    }
    for e in l.entities.iter().filter(|e| e.kind == 2 && (15..=19).contains(&e.sub) && e.link == 0) {
        out.push(Issue { error: true, text: format!("{} {} has no source area (link)", super::edit::entity_name(e), e.index), at: at_e(e), sel: Some(Sel::Entity(e.index)) });
    }

    // waypoints, as the game builds them: positions joined by the first link from each (port
    // addition: a waypoint is a position and a level, so the two branches of a crossing stay apart)
    type Wp = (i32, i32, Option<u32>);
    let wp = |e: &c8_data::level::Entity| -> Wp { (e.x, e.y, e.level.map(f32::to_bits)) };
    let mut child: HashMap<Wp, Wp> = HashMap::new();
    let mut points: HashMap<Wp, usize> = HashMap::new();
    for e in l.entities.iter().filter(|e| e.kind == 9 && e.sub != 0 && e.link != 0) {
        let Some(n) = by_index.get(&(e.link as usize)) else { continue };
        points.entry(wp(e)).or_insert(e.index);
        points.entry(wp(n)).or_insert(n.index);
        if wp(e) != wp(n) {
            child.entry(wp(e)).or_insert(wp(n));
        }
    }
    if l.has_addition() {
        crossing_checks(l, &points, &mut out);
    }
    if points.is_empty() {
        out.push(issue(true, "no waypoints: computer players cannot drive".into()));
    } else {
        if points.len() > 200 {
            out.push(issue(true, format!("{} waypoint positions; the game keeps only the first 200", points.len())));
        }
        let mut ends: Vec<_> = points.iter().filter(|(p, _)| !child.contains_key(p)).collect();
        ends.sort_by_key(|(p, _)| (p.0, p.1, p.2));
        for (p, &i) in ends {
            out.push(Issue { error: false, text: format!("waypoint at {}, {} leads nowhere: computer players stop there", p.0, p.1), at: Some((p.0 as f32 + 0.5, p.1 as f32 + 0.5)), sel: Some(Sel::Entity(i)) });
        }
    }

    // groups and triggers
    let targets: HashSet<u16> = l.entities.iter().filter(|e| e.kind == 8).map(|e| e.target_group).collect();
    let mut groups: HashMap<u16, usize> = HashMap::new();
    for e in &l.entities {
        *groups.entry(e.group).or_default() += 1;
    }
    let mut gs: Vec<_> = groups.iter().filter(|(g, _)| **g != 1 && !targets.contains(g)).collect();
    gs.sort();
    for (g, n) in gs {
        out.push(issue(false, format!("group {g} is never activated by a trigger: its {n} entities never appear")));
    }
    for e in l.entities.iter().filter(|e| e.kind == 8 && !groups.contains_key(&e.target_group)) {
        out.push(Issue { error: false, text: format!("trigger {} activates group {}, which has no entities", e.index, e.target_group), at: at_e(e), sel: Some(Sel::Entity(e.index)) });
    }
    if !l.entities.iter().any(|e| e.kind == 10) {
        out.push(issue(false, "no recovery trucks".into()));
    }
    if l.entities.len() >= MAX_ENTITIES - 1 {
        out.push(issue(true, "all entity records are in use".into()));
    }

    // columns
    let mut reported = HashSet::new();
    for (i, c) in l.cells.iter().enumerate() {
        if c.tex >= 0 {
            continue;
        }
        let id = -(c.tex as i32) as usize;
        if !reported.insert(id) {
            continue;
        }
        let at = Some(((i % 256) as f32 + 0.5, (i / 256) as f32 + 0.5));
        match l.columns.get(id) {
            None => out.push(Issue { error: true, text: format!("cells use column {id}, which does not exist"), at, sel: None }),
            Some(col) => {
                if col.mask == 0 {
                    out.push(Issue { error: false, text: format!("column {id} has no solid level"), at, sel: None });
                }
                for k in 0..8 {
                    let b = col.blocks[k];
                    if col.mask & (1 << k) != 0 && (b <= 0 || b as usize >= l.blocks.len()) {
                        out.push(Issue { error: true, text: format!("column {id} level {k} is solid but has no block"), at, sel: None });
                    }
                }
            }
        }
    }
    out.sort_by_key(|i| !i.error);
    out
}

/// Port addition: checks of a level with crossings (sidecar data, spec 01 §9.7).
fn crossing_checks(l: &Level, points: &HashMap<(i32, i32, Option<u32>), usize>, out: &mut Vec<Issue>) {
    // a waypoint on or under a deck (blocks floating at least 0.4 above the terrain) needs a level,
    // which says whether the route runs on the deck or under it
    let mut under: Vec<usize> = points
        .iter()
        .filter(|(p, _)| p.2.is_none())
        .filter(|(p, _)| {
            let (x, y) = (p.0 as f32 + 0.5, p.1 as f32 + 0.5);
            l.ceiling_above(x, y, l.terrain_height(x, y) + 0.05).is_some_and(|c| c - l.terrain_height(x, y) >= 0.4)
        })
        .map(|(_, &i)| i)
        .collect();
    under.sort();
    for i in under {
        let e = l.entities.iter().find(|e| e.index == i).unwrap();
        out.push(Issue { error: false, text: format!("waypoint {i} is on or under a deck but has no level (the deck's top or the road's height under it)"), at: Some((e.x as f32 + 0.5, e.y as f32 + 0.5)), sel: Some(Sel::Entity(i)) });
    }
    // flat bases without a column do nothing
    let n = l.cells.iter().filter(|c| c.base.is_some() && c.tex >= 0).count();
    if n > 0 {
        out.push(issue(false, format!("{n} cells have a flat block base but no column")));
    }
    // a wall with a height range on its last point (no segment starts there)
    for e in l.entities.iter().filter(|e| e.kind == 9 && e.sub == 0 && e.link == 0 && e.range.is_some()) {
        if !l.entities.iter().any(|o| o.kind == 9 && o.sub == 0 && o.link as usize == e.index && o.range.is_some()) {
            out.push(Issue { error: false, text: format!("wall point {} has a height range but starts no segment, and the segment ending there has none", e.index), at: Some((e.x as f32 + 0.5, e.y as f32 + 0.5)), sel: Some(Sel::Entity(e.index)) });
        }
    }
}

// ---- AI test ----------------------------------------------------------------------------------

pub(super) struct AiReport {
    pub summary: String,
    /// (name, laps completed)
    pub laps: Vec<(String, u32)>,
    /// (name, x, y, tick)
    pub stuck: Vec<(String, f32, f32, u32)>,
}

pub(super) enum AiState {
    Running(Receiver<Result<AiReport, String>>),
    Done(Result<AiReport, String>),
}

/// Parse the `--sim` output: craft lines every 100 ticks.
fn parse_sim(out: &str) -> AiReport {
    // the first line that is not a missing-asset warning
    let summary = out.lines().find(|l| !l.starts_with("warning: ")).unwrap_or("").to_string();
    let mut tick = 0u32;
    let mut laps: Vec<(String, u32)> = Vec::new();
    // per craft: last sampled position and how many samples it has not moved
    let mut still: HashMap<String, ((f32, f32), u32, bool)> = HashMap::new();
    let mut stuck = Vec::new();
    for line in out.lines() {
        if let Some(t) = line.strip_prefix("--- tick ") {
            tick = t.split_whitespace().next().and_then(|v| v.parse().ok()).unwrap_or(tick);
            continue;
        }
        let Some((name, rest)) = line.split_once(" pos (") else { continue };
        let name = name.trim().to_string();
        let mut it = rest.split(')').next().unwrap_or("").split(',').map(|v| v.trim().parse::<f32>().unwrap_or(0.0));
        let (x, y) = (it.next().unwrap_or(0.0), it.next().unwrap_or(0.0));
        let lap = rest.split_whitespace().skip_while(|w| *w != "lap").nth(1).and_then(|v| v.parse::<u32>().ok()).unwrap_or(1);
        match laps.iter_mut().find(|l| l.0 == name) {
            Some(l) => l.1 = l.1.max(lap.saturating_sub(1)),
            None => laps.push((name.clone(), lap.saturating_sub(1))),
        }
        let Some(s) = still.get_mut(&name) else {
            still.insert(name, ((x, y), 0, false));
            continue;
        };
        if ((s.0 .0 - x).powi(2) + (s.0 .1 - y).powi(2)).sqrt() < 0.5 {
            s.1 += 1;
            // three samples (300 ticks, 15 s) without moving
            if s.1 == 3 && !s.2 {
                stuck.push((name.clone(), x, y, tick));
                s.2 = true;
            }
        } else {
            *s = ((x, y), 0, false);
        }
    }
    AiReport { summary, laps, stuck }
}

impl LevelDoc {
    pub(super) fn start_ai_test(&mut self) {
        let side = super::sidecar_bytes(&self.level);
        let bytes = match self.level.to_bytes() {
            Ok(b) => b,
            Err(e) => {
                self.ai = Some(AiState::Done(Err(e)));
                return;
            }
        };
        let track = self.look.set + 1;
        let ticks = self.ai_ticks;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let run = || -> Result<AiReport, String> {
                let dir = std::env::temp_dir().join(format!("c8h18-edit-sim-{}", std::process::id()));
                std::fs::create_dir_all(dir.join("MAPS")).map_err(|e| e.to_string())?;
                std::fs::write(dir.join(format!("MAPS/LEVEL0-{track}.DAT")), bytes).map_err(|e| e.to_string())?;
                std::fs::write(dir.join(format!("MAPS/LEVEL0-{track}.TXT")), side).map_err(|e| e.to_string())?;
                let data = c8_data::files::data_dir();
                let data = data.canonicalize().unwrap_or(data);
                let exe = crate::launch::game_exe();
                let out = std::process::Command::new(&exe)
                    .arg("--asset")
                    .arg(&data)
                    .arg("--override")
                    .arg(&dir)
                    .args(["--sim", &track.to_string(), &ticks.to_string()])
                    .output()
                    .map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
                if !out.status.success() {
                    let err = String::from_utf8_lossy(&out.stderr);
                    return Err(format!("the game stopped: {}", err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("")));
                }
                Ok(parse_sim(&String::from_utf8_lossy(&out.stdout)))
            };
            let _ = tx.send(run());
        });
        self.ai = Some(AiState::Running(rx));
    }

    pub(super) fn check_panel(&mut self, ui: &mut egui::Ui) {
        let issues = check(&self.level);
        let errors = issues.iter().filter(|i| i.error).count();
        let title = format!("Check ({errors} error{}, {} warning{})", if errors == 1 { "" } else { "s" }, issues.len() - errors, if issues.len() - errors == 1 { "" } else { "s" });
        let mut go = None;
        egui::CollapsingHeader::new(egui::RichText::new(title).color(if errors > 0 { Color32::from_rgb(255, 110, 90) } else { Color32::LIGHT_GRAY })).id_salt("check").default_open(errors > 0).show(ui, |ui| {
            if issues.is_empty() {
                ui.label("No problems found.");
            }
            egui::ScrollArea::vertical().id_salt("issues").max_height(200.0).show(ui, |ui| {
                for (k, i) in issues.iter().enumerate() {
                    let (mark, col) = if i.error { ("error", Color32::from_rgb(255, 110, 90)) } else { ("warning", Color32::from_rgb(230, 190, 80)) };
                    let text = egui::RichText::new(format!("{mark}: {}", i.text)).small().color(col);
                    if i.at.is_some() || i.sel.is_some() {
                        if ui.link(text).on_hover_text("show on the map").clicked() {
                            go = Some(k);
                        }
                    } else {
                        ui.label(text);
                    }
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                let running = matches!(self.ai, Some(AiState::Running(_)));
                if ui.add_enabled(!running, egui::Button::new(if running { "AI test running…" } else { "Run AI test" })).on_hover_text("race the 8 computer players on this level in the game's headless simulation").clicked() {
                    self.start_ai_test();
                }
                ui.add(egui::DragValue::new(&mut self.ai_ticks).range(500..=20000).speed(100).suffix(" ticks"));
            });
            self.ai_report(ui);
        });
        if let Some(i) = go.map(|k| &issues[k]) {
            if let Some(s) = i.sel {
                self.obj_sel = Some(s);
                if !matches!(self.tool, super::Tool::Objects) && matches!(s, Sel::Entity(_) | Sel::Region(_)) {
                    self.tool = super::Tool::Objects;
                }
            }
            let at = i.at.or_else(|| match i.sel {
                Some(Sel::Entity(e)) => self.entity(e).map(|e| (e.x as f32 + 0.5, e.y as f32 + 0.5)),
                Some(Sel::Region(s)) => self.level.regions.iter().find(|r| r.slot == s).map(|r| (r.cx, r.cy)),
                None => None,
            });
            if let Some((x, y)) = at {
                self.centre_on(x, y);
            }
        }
    }

    fn ai_report(&mut self, ui: &mut egui::Ui) {
        if let Some(AiState::Running(rx)) = &self.ai {
            match rx.try_recv() {
                Ok(r) => self.ai = Some(AiState::Done(r)),
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ui.spinner();
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
                    return;
                }
                Err(_) => self.ai = Some(AiState::Done(Err("the AI test stopped".into()))),
            }
        }
        let mut go = None;
        match &self.ai {
            Some(AiState::Done(Ok(r))) => {
                ui.label(egui::RichText::new(&r.summary).small().color(Color32::GRAY));
                let total: u32 = r.laps.iter().map(|l| l.1).sum();
                ui.label(format!("laps completed in {} ticks: {total} ({})", self.ai_ticks, r.laps.iter().map(|(n, l)| format!("{n} {l}")).collect::<Vec<_>>().join(", ")));
                if r.stuck.is_empty() {
                    ui.label("no craft got stuck");
                } else {
                    ui.label(format!("stuck for 15 s or more ({}; red marks on the map):", r.stuck.len()));
                    for (k, (n, x, y, t)) in r.stuck.iter().enumerate() {
                        if ui.link(egui::RichText::new(format!("{n} at {x:.1}, {y:.1} (tick {t})")).small()).clicked() {
                            go = Some(k);
                        }
                    }
                }
            }
            Some(AiState::Done(Err(e))) => {
                ui.label(egui::RichText::new(e).color(Color32::from_rgb(255, 110, 90)));
            }
            _ => {}
        }
        if let (Some(k), Some(AiState::Done(Ok(r)))) = (go, &self.ai) {
            let (x, y) = (r.stuck[k].1, r.stuck[k].2);
            self.centre_on(x, y);
        }
    }

    /// Red marks where crafts got stuck in the last AI test.
    pub(super) fn draw_ai_marks(&self, p: &egui::Painter, origin: egui::Pos2) {
        let Some(AiState::Done(Ok(r))) = &self.ai else { return };
        for (_, x, y, _) in &r.stuck {
            let c = self.to_screen(origin, *x, *y);
            let s = (self.zoom * 0.6).max(5.0);
            let st = egui::Stroke::new(2.5, Color32::from_rgb(255, 40, 40));
            p.line_segment([c - egui::vec2(s, s), c + egui::vec2(s, s)], st);
            p.line_segment([c - egui::vec2(s, -s), c + egui::vec2(s, -s)], st);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_levels_have_no_errors() {
        c8_data::files::set_data_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources").into());
        for t in 1..=6 {
            let Ok(l) = Level::load(t) else { return };
            let errors: Vec<String> = check(&l).into_iter().filter(|i| i.error).map(|i| i.text).collect();
            assert!(errors.is_empty(), "level {t}: {errors:?}");
        }
    }

    /// The new track 1 in assets/custom (written by the local mapgen generator) round-trips and has
    /// no problems at all.
    #[test]
    fn generated_track_is_clean() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/custom/MAPS/LEVEL0-1.DAT");
        let Ok(raw) = std::fs::read(&p) else { return };
        let l = Level::from_bytes(&raw).expect("parse");
        assert!(l.to_bytes().expect("write") == raw, "assets/custom level differs after a round trip");
        let issues: Vec<String> = check(&l).into_iter().map(|i| format!("{}: {}", if i.error { "error" } else { "warning" }, i.text)).collect();
        assert!(issues.is_empty(), "{issues:?}");
    }

    /// A valid crossing (port addition) gives no errors and no crossing warnings: the two
    /// waypoints that share a cell on two levels are not merged.
    #[test]
    fn crossing_example_is_clean() {
        let l = c8_data::level::crossing_example();
        let issues: Vec<String> = check(&l).into_iter().map(|i| format!("{}: {}", if i.error { "error" } else { "warning" }, i.text)).collect();
        let bad: Vec<&String> = issues.iter().filter(|i| i.starts_with("error") || ["leads nowhere", "deck", "flat block base", "height range"].iter().any(|w| i.contains(w))).collect();
        assert!(bad.is_empty(), "{issues:?}");
        // without the levels, the waypoints on and under the deck are reported
        let mut l = l;
        for e in l.entities.iter_mut().filter(|e| e.kind == 9 && e.sub != 0) {
            e.level = None;
        }
        let issues: Vec<String> = check(&l).into_iter().map(|i| i.text).collect();
        assert!(issues.iter().any(|i| i.contains("on or under a deck")), "{issues:?}");
    }

    #[test]
    fn sim_output_parsing() {
        let out = "waypoints 10\n--- tick 0 things 1\nA        pos (  1.00,  2.00, 3.00) yaw 0 act 1 wp 1 lap 1 cp 0\n--- tick 100 things 1\nA        pos (  1.10,  2.00, 3.00) yaw 0 act 1 wp 1 lap 1 cp 0\n--- tick 200 things 1\nA        pos (  1.10,  2.00, 3.00) yaw 0 act 1 wp 1 lap 2 cp 0\n--- tick 300 things 1\nA        pos (  1.10,  2.10, 3.00) yaw 0 act 1 wp 1 lap 3 cp 0\n";
        let r = parse_sim(out);
        assert_eq!(r.laps, vec![("A".to_string(), 2)]);
        assert_eq!(r.stuck.len(), 1);
        assert_eq!(r.stuck[0].3, 300);
    }
}
