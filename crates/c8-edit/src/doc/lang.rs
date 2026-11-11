//! Language file editor (`lang/<code>.txt`, spec 02 appendix A): every key with the English text
//! beside it, checks for missing entries, markup, glyphs and width, and a preview in the game font.
use super::history::{Command, History};
use super::Document;
use crate::resources::Kind;
use eframe::egui::{self, Color32, RichText, TextureHandle, Vec2};
use c8_data::strings::{self, LangFile};
use c8_data::sprites::Font;
use std::path::{Path, PathBuf};

/// Widest text the 640-pixel screen shows in the large menu font.
const SCREEN_W: i32 = 640;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Level {
    Error,
    Warning,
    Info,
}

struct Issue {
    level: Level,
    text: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Filter {
    All,
    Problems,
    Missing,
    Untranslated,
}

/// The whole file before and after an edit.
struct FileEdit {
    label: String,
    before: LangFile,
    after: LangFile,
}

impl Command<LangFile> for FileEdit {
    fn apply(&mut self, f: &mut LangFile) {
        f.clone_from(&self.after);
    }
    fn revert(&mut self, f: &mut LangFile) {
        f.clone_from(&self.before);
    }
    fn label(&self) -> String {
        self.label.clone()
    }
}

pub struct LangDoc {
    path: PathBuf,
    rel: String,
    code: String,
    file: LangFile,
    /// the English file (the reference), unless this is it
    en: Option<LangFile>,
    history: History<LangFile>,
    /// the file before the edit in progress, and its label
    pending: Option<(String, LangFile)>,
    sel: Option<String>,
    filter: Filter,
    search: String,
    font: Option<Font>,
    preview: Option<(String, TextureHandle)>,
    scroll_to: Option<usize>,
    test: bool,
}

impl LangDoc {
    pub fn open(path: PathBuf, rel: String) -> Result<LangDoc, String> {
        let text = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let file = LangFile::parse(&String::from_utf8_lossy(&text));
        let code = path.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let en = if code == "en" {
            None
        } else {
            let p = path.with_file_name("en.txt");
            let d = std::fs::read(&p).or_else(|_| c8_data::files::read_raw("lang/en.txt").map_err(|e| std::io::Error::other(e)));
            d.ok().map(|d| LangFile::parse(&String::from_utf8_lossy(&d)))
        };
        let font = Font::load("olfnt0-1", true).map_err(|e| c8_data::files::warn_asset("data/olfnt0-1.dat", Some(&e), "no in-game preview")).ok();
        Ok(LangDoc { path, rel, code, file, en, history: History::default(), pending: None, sel: None, filter: Filter::All, search: String::new(), font, preview: None, scroll_to: None, test: false })
    }

    fn reference(&self) -> &LangFile {
        self.en.as_ref().unwrap_or(&self.file)
    }

    /// Keys in English order, then this file's extra keys.
    fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = Vec::new();
        for k in self.reference().keys().into_iter().chain(self.file.keys()) {
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
        keys
    }

    fn width(&self, s: &str) -> Option<i32> {
        let f = self.font.as_ref()?;
        Some(strings::to_cp437(s).iter().map(|&c| f.glyph(c).w as i32).sum())
    }

    fn issues(&self, key: &str) -> Vec<Issue> {
        let mut v = Vec::new();
        let mut add = |level, text: String| v.push(Issue { level, text });
        let en = self.en.as_ref().and_then(|e| e.get(key));
        let text = self.file.get(key);
        let index = strings::table_index(key);
        if index.is_some_and(|n| n >= strings::COUNT) {
            add(Level::Error, format!("string {key} is out of range (000..{:03}): the game refuses the file", strings::COUNT - 1));
        }
        let Some(text) = text else {
            if self.en.is_some() {
                add(Level::Warning, "missing: the game shows the English text".into());
            } else if index.is_some() {
                add(Level::Error, "missing in English: the game shows nothing".into());
            }
            return v;
        };
        if self.en.is_some() && en.is_none() && index.is_none() {
            add(Level::Warning, "not in the English file: the game does not use this key".into());
        }
        let missing = strings::missing_glyphs(text);
        if !missing.is_empty() {
            add(Level::Warning, format!("no glyphs for {}: drawn as ?", missing.iter().collect::<String>()));
        }
        if let Some(en) = en {
            let (a, b) = (strings::markup_codes(en), strings::markup_codes(text));
            if a != b {
                add(Level::Warning, format!("markup differs from English: {} vs {}", a.join(" "), b.join(" ")));
            }
            if text == en && text.chars().any(|c| c.is_alphabetic()) && key != "000" {
                add(Level::Info, "same as English".into());
            }
        }
        if let Some(w) = self.width(text) {
            if w > SCREEN_W {
                add(Level::Info, format!("{w} px wide in the large menu font: wider than the screen ({SCREEN_W}) where that font is used"));
            }
        }
        v
    }

    fn worst(&self, key: &str) -> Option<Level> {
        self.issues(key).iter().map(|i| i.level).min()
    }

    fn begin(&mut self, label: &str) {
        if self.pending.is_none() {
            self.pending = Some((label.into(), self.file.clone()));
        }
    }

    fn finish(&mut self) {
        let Some((label, before)) = self.pending.take() else { return };
        if before == self.file {
            return;
        }
        let after = self.file.clone();
        self.file = before;
        self.history.run(Box::new(FileEdit { label, before: self.file.clone(), after }), &mut self.file);
        self.preview = None;
    }

    fn edit(&mut self, label: &str, f: impl FnOnce(&mut LangFile, &[String])) {
        self.finish();
        self.begin(label);
        let order = self.reference().keys();
        f(&mut self.file, &order);
        self.finish();
    }

    fn fill_missing(&mut self) {
        let Some(en) = self.en.clone() else { return };
        let missing: Vec<String> = en.keys().into_iter().filter(|k| self.file.get(k).is_none()).collect();
        self.edit(&format!("add {} missing entries", missing.len()), |f, order| {
            for k in &missing {
                f.set(k, en.get(k).unwrap_or(""), order);
            }
        });
    }

    fn row_visible(&self, key: &str) -> bool {
        let text = self.file.get(key);
        let pass = match self.filter {
            Filter::All => true,
            Filter::Problems => self.worst(key).is_some_and(|l| l <= Level::Warning),
            Filter::Missing => text.is_none(),
            Filter::Untranslated => self.issues(key).iter().any(|i| i.text == "same as English"),
        };
        if !pass {
            return false;
        }
        if self.search.is_empty() {
            return true;
        }
        let q = self.search.to_lowercase();
        let en = self.en.as_ref().and_then(|e| e.get(key)).unwrap_or("");
        key.to_lowercase().contains(&q) || text.unwrap_or("").to_lowercase().contains(&q) || en.to_lowercase().contains(&q)
    }

    fn preview_ui(&mut self, ui: &mut egui::Ui, key: &str) {
        let Some(font) = &self.font else { return };
        let text = self.file.get(key).or_else(|| self.en.as_ref().and_then(|e| e.get(key))).unwrap_or("").to_string();
        if self.preview.as_ref().is_none_or(|(t, _)| *t != text) {
            let bytes = strings::to_cp437(&text);
            let w = bytes.iter().map(|&c| font.glyph(c).w).sum::<u32>().max(1);
            let h = bytes.iter().map(|&c| font.glyph(c).h).max().unwrap_or(1).max(1);
            let bg = Color32::from_rgb(30, 40, 70);
            let mut img = egui::ColorImage::new([w as usize, h as usize], vec![bg; (w * h) as usize]);
            let mut x0 = 0;
            for &c in &bytes {
                let g = font.glyph(c);
                for y in 0..g.h {
                    for x in 0..g.w {
                        if let Some(v) = g.get(x, y) {
                            img.pixels[(y * w + x0 + x) as usize] = if v != 0 { Color32::WHITE } else { Color32::BLACK };
                        }
                    }
                }
                x0 += g.w;
            }
            let t = ui.ctx().load_texture(format!("{}#preview", self.rel), img, egui::TextureOptions::NEAREST);
            self.preview = Some((text, t));
        }
        if let Some((_, t)) = &self.preview {
            let [w, h] = t.size();
            // the screen is 640 wide: show the text at the same scale as a 640-wide bar
            let s = ui.available_width() / SCREEN_W as f32;
            let (bar, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h as f32 * s + 4.0), egui::Sense::hover());
            ui.painter().rect_filled(bar, 0.0, Color32::from_rgb(15, 20, 35));
            let x = bar.center().x - w as f32 * s / 2.0;
            ui.painter().image(t.id(), egui::Rect::from_min_size(egui::pos2(x, bar.top() + 2.0), Vec2::new(w as f32, h as f32) * s), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            ui.label(RichText::new(format!("{w} of {SCREEN_W} px, large menu font (OLFNT0-1); the bar is the screen width")).small().color(Color32::GRAY));
        }
    }
}

fn level_colour(l: Level) -> Color32 {
    match l {
        Level::Error => Color32::from_rgb(230, 80, 70),
        Level::Warning => Color32::from_rgb(230, 180, 60),
        Level::Info => Color32::from_rgb(120, 160, 220),
    }
}

impl Document for LangDoc {
    fn title(&self) -> String {
        self.rel.clone()
    }
    fn kind(&self) -> Kind {
        Kind::Strings
    }
    fn path(&self) -> Option<&Path> {
        Some(&self.path)
    }
    fn dirty(&self) -> bool {
        self.history.dirty() || self.pending.as_ref().is_some_and(|(_, b)| *b != self.file)
    }
    fn can_save(&self) -> bool {
        true
    }
    fn save_to(&mut self, path: &Path) -> Result<(), String> {
        self.finish();
        std::fs::write(path, self.file.encode()).map_err(|e| format!("{}: {e}", path.display()))?;
        self.path = path.to_path_buf();
        self.history.mark_saved();
        Ok(())
    }
    fn undo(&mut self) {
        self.finish();
        self.history.undo(&mut self.file);
        self.preview = None;
    }
    fn redo(&mut self) {
        self.history.redo(&mut self.file);
        self.preview = None;
    }
    fn can_undo(&self) -> bool {
        self.history.can_undo() || self.pending.is_some()
    }
    fn can_redo(&self) -> bool {
        self.history.can_redo()
    }
    fn game_files(&self) -> Result<Vec<(String, Vec<u8>)>, String> {
        Ok(vec![(format!("lang/{}.txt", self.code), self.file.encode().into_bytes())])
    }
    fn game_args(&self) -> Vec<String> {
        let mut a = vec!["--no-intro".to_string()];
        if strings::CODES.contains(&self.code.as_str()) {
            a.extend(["--lang".into(), self.code.clone()]);
        }
        a
    }
    fn wants_test(&mut self) -> bool {
        std::mem::take(&mut self.test)
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        let keys = self.keys();
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("search keys and text").desired_width(220.0));
            for (f, name) in [(Filter::All, "All"), (Filter::Problems, "Problems"), (Filter::Missing, "Missing"), (Filter::Untranslated, "Same as English")] {
                if f == Filter::Untranslated && self.en.is_none() {
                    continue;
                }
                ui.selectable_value(&mut self.filter, f, name);
            }
        });
        ui.separator();
        let rows: Vec<String> = keys.into_iter().filter(|k| self.row_visible(k)).collect();
        let row_h = 22.0;
        let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
        if let Some(i) = self.scroll_to.take().and_then(|k| rows.iter().position(|r| *r == self.keys()[k])) {
            area = area.vertical_scroll_offset((i as f32 * (row_h + ui.spacing().item_spacing.y) - 60.0).max(0.0));
        }
        let key_w = 140.0;
        let text_w = ((ui.available_width() - key_w - 60.0) / 2.0).max(120.0);
        area.show_rows(ui, row_h, rows.len(), |ui, range| {
            for key in &rows[range] {
                let selected = self.sel.as_deref() == Some(key);
                ui.horizontal(|ui| {
                    ui.set_height(row_h);
                    let worst = self.worst(key);
                    let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
                    if let Some(l) = worst {
                        ui.painter().circle_filled(dot.center(), 4.0, level_colour(l));
                    }
                    let resp = ui.add(egui::Button::selectable(selected, RichText::new(key).monospace()).min_size(Vec2::new(key_w, row_h)));
                    if resp.clicked() {
                        self.sel = Some(key.clone());
                    }
                    if self.en.is_some() {
                        let en = self.en.as_ref().and_then(|e| e.get(key)).unwrap_or("");
                        ui.allocate_ui_with_layout(Vec2::new(text_w, row_h), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.set_width(text_w);
                            ui.add(egui::Label::new(RichText::new(en).color(Color32::GRAY)).truncate()).on_hover_text(en);
                        });
                    }
                    let had = self.file.get(key).map(str::to_string);
                    let mut text = had.clone().unwrap_or_default();
                    let hint = if had.is_none() { "(missing)" } else { "" };
                    let r = ui.add_sized([if self.en.is_some() { text_w } else { text_w * 2.0 }, row_h], egui::TextEdit::singleline(&mut text).hint_text(hint).id_salt(("lang", key.as_str())));
                    if r.gained_focus() {
                        self.sel = Some(key.clone());
                    }
                    if r.changed() {
                        self.begin(&format!("edit {key}"));
                        let order = self.reference().keys();
                        self.file.set(key, &text, &order);
                    }
                });
            }
        });
        if self.pending.is_some() && ui.ctx().memory(|m| m.focused().is_none()) {
            self.finish();
        }
    }

    fn side_ui(&mut self, ui: &mut egui::Ui) {
        if ui.button("Test in game").on_hover_text("Start the game in this language with this file").clicked() {
            self.finish();
            self.test = true;
        }
        ui.separator();
        let keys = self.keys();
        let mut counts = [0usize; 3];
        let mut problems: Vec<(String, Issue)> = Vec::new();
        for k in &keys {
            for i in self.issues(k) {
                counts[i.level as usize] += 1;
                problems.push((k.clone(), i));
            }
        }
        let missing = self.en.as_ref().map(|en| en.keys().iter().filter(|k| self.file.get(k).is_none()).count()).unwrap_or(0);
        ui.heading("Check");
        ui.label(format!("{} entries; {} errors, {} warnings, {} notes", self.file.keys().len(), counts[0], counts[1], counts[2]));
        let bad = self.file.bad_lines();
        if !bad.is_empty() {
            ui.colored_label(level_colour(Level::Error), format!("lines without `=` (the game refuses the file): {}", bad.iter().map(|i| (i + 1).to_string()).collect::<Vec<_>>().join(", ")));
        }
        if missing > 0 && ui.button(format!("Add the {missing} missing entries (English text)")).clicked() {
            self.fill_missing();
        }
        egui::ScrollArea::vertical().id_salt("issues").max_height(220.0).show(ui, |ui| {
            problems.sort_by_key(|(_, i)| i.level);
            for (k, i) in problems.iter().filter(|(_, i)| i.level <= Level::Warning).take(200) {
                let r = ui.add(egui::Label::new(RichText::new(format!("{k}: {}", i.text)).small().color(level_colour(i.level))).sense(egui::Sense::click()));
                if r.clicked() {
                    self.sel = Some(k.clone());
                    self.filter = Filter::All;
                    self.scroll_to = keys.iter().position(|x| x == k);
                }
            }
        });
        ui.separator();
        if let Some(key) = self.sel.clone() {
            ui.heading(format!("Entry {key}"));
            if let Some(n) = strings::table_index(&key) {
                ui.label(RichText::new(format!("original string table entry {n}")).small().color(Color32::GRAY));
            } else {
                ui.label(RichText::new("named entry (port addition)").small().color(Color32::GRAY));
            }
            if let Some(en) = self.en.as_ref().and_then(|e| e.get(&key)) {
                ui.label(format!("English: {en}"));
            }
            for i in self.issues(&key) {
                ui.colored_label(level_colour(i.level), &i.text);
            }
            self.preview_ui(ui, &key);
            if self.file.get(&key).is_some() && self.en.is_some() && ui.button("Remove entry (use English)").clicked() {
                self.edit(&format!("remove {key}"), |f, _| f.remove(&key));
            }
        } else {
            ui.label(RichText::new("Select an entry to see its checks and a preview in the game font.").color(Color32::GRAY));
        }
        ui.separator();
        ui.label(RichText::new("The game reads lang/<code>.txt at start: 000..193 are the original string table, named keys are the port's additions. Missing entries fall back to English. Letters the fonts lack (e.g. Polish ones) are drawn as their base letters.").small().color(Color32::GRAY));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn res(rel: &str) -> PathBuf {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources")).join(rel)
    }

    #[test]
    fn shipped_languages_check_clean_and_edits_undo() {
        c8_data::files::set_data_dir(res(""));
        for code in strings::CODES {
            let d = LangDoc::open(res(&format!("lang/{code}.txt")), format!("lang/{code}.txt")).unwrap();
            for k in d.keys() {
                let errs: Vec<String> = d.issues(&k).into_iter().filter(|i| i.level == Level::Error).map(|i| i.text).collect();
                assert!(errs.is_empty(), "{code} {k}: {errs:?}");
            }
        }
        let mut d = LangDoc::open(res("lang/de.txt"), "lang/de.txt".into()).unwrap();
        let orig = d.file.clone();
        d.edit("remove", |f, _| f.remove("001"));
        assert!(d.issues("001").iter().any(|i| i.text.starts_with("missing")));
        d.fill_missing();
        assert_eq!(d.file.get("001"), d.en.as_ref().unwrap().get("001"));
        d.undo();
        d.undo();
        assert_eq!(d.file, orig);
        assert!(!d.dirty());
    }
}
