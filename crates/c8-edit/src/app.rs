//! The editor window: menu bar, resource browser (left), document tabs (centre), inspector (right).
use crate::doc::{self, Document};
use crate::resources::{self, Entry, Kind};
use eframe::egui::{self, Color32, Key, KeyboardShortcut, Modifiers};
use std::path::{Path, PathBuf};

const DATA_KEY: &str = "data_dir";

pub struct EditorApp {
    data_dir: Option<PathBuf>,
    entries: Vec<Entry>,
    filter: String,
    docs: Vec<Box<dyn Document>>,
    active: usize,
    status: String,
    /// tab waiting for "discard changes?"
    confirm_close: Option<usize>,
    /// debug: `HIEDIT_SHOT=<png>` saves a screenshot after a few frames and quits
    shot: Option<(String, u32)>,
}

impl EditorApp {
    pub fn new(cc: &eframe::CreationContext<'_>, data: Option<PathBuf>, open: Option<PathBuf>) -> EditorApp {
        if let Some(rs) = &cc.wgpu_render_state {
            crate::gpu::set(rs.clone());
        }
        let remembered = cc.storage.and_then(|s| s.get_string(DATA_KEY)).map(PathBuf::from);
        let mut app = EditorApp { data_dir: None, entries: Vec::new(), filter: String::new(), docs: Vec::new(), active: 0, status: String::new(), confirm_close: None, shot: std::env::var("HIEDIT_SHOT").ok().map(|p| (p, 0)) };
        let candidates = [data, remembered, Some(PathBuf::from("resources"))];
        if let Some(d) = candidates.into_iter().flatten().find(|d| resources::is_data_dir(d)) {
            app.set_data_dir(d);
        }
        if let Some(p) = open {
            app.open_path(p);
        }
        app
    }

    fn set_data_dir(&mut self, d: PathBuf) {
        if !resources::is_data_dir(&d) {
            self.status = format!("{} is not a game data folder (no DATA/PALET0-0.DAT and MAPS/)", d.display());
            if let Some(old) = &self.data_dir {
                c8_data::files::set_data_dir(old.clone());
            }
            return;
        }
        c8_data::files::set_data_dir(d.clone());
        self.entries = resources::scan(&d);
        self.status = format!("data folder {}: {} files", d.display(), self.entries.len());
        self.data_dir = Some(d);
    }

    fn open_path(&mut self, path: PathBuf) {
        if let Some(i) = self.docs.iter().position(|d| d.path() == Some(path.as_path())) {
            self.active = i;
            return;
        }
        let rel = self
            .data_dir
            .as_ref()
            .and_then(|d| path.strip_prefix(d).ok())
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|| path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
        let kind = Kind::of(&rel);
        match doc::open(path, kind, rel.clone()) {
            Ok(d) => {
                self.docs.push(d);
                self.active = self.docs.len() - 1;
                self.status = format!("opened {rel}");
            }
            Err(e) => self.status = format!("cannot open {rel}: {e}"),
        }
    }

    /// Files inside the game data folder are the originals: saving them asks for a new name.
    fn is_original(&self, p: &Path) -> bool {
        self.data_dir.as_ref().is_some_and(|d| p.starts_with(d))
    }

    fn save(&mut self, save_as: bool) {
        let Some(d) = self.docs.get(self.active) else { return };
        if !d.can_save() {
            self.status = format!("{} cannot be saved yet", d.title());
            return;
        }
        let current = d.path().map(Path::to_path_buf);
        let target = match current {
            Some(p) if !save_as && !self.is_original(&p) => Some(p),
            p => {
                let mut dlg = rfd::FileDialog::new();
                if let Some(name) = p.as_ref().and_then(|p| p.file_name()) {
                    dlg = dlg.set_file_name(name.to_string_lossy());
                }
                dlg.save_file()
            }
        };
        let Some(target) = target else { return };
        let d = &mut self.docs[self.active];
        self.status = match d.save_to(&target) {
            Ok(()) => format!("saved {}", target.display()),
            Err(e) => format!("save failed: {e}"),
        };
    }

    fn menu(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Game data folder…").clicked() {
                    if let Some(d) = rfd::FileDialog::new().pick_folder() {
                        self.set_data_dir(d);
                    }
                }
                if ui.button("Open file…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_file() {
                        self.open_path(p);
                    }
                }
                ui.separator();
                if ui.add_enabled(!self.docs.is_empty(), egui::Button::new("Save").shortcut_text("⌘S")).clicked() {
                    self.save(false);
                }
                if ui.add_enabled(!self.docs.is_empty(), egui::Button::new("Save as…").shortcut_text("⇧⌘S")).clicked() {
                    self.save(true);
                }
                if ui.add_enabled(!self.docs.is_empty(), egui::Button::new("Close").shortcut_text("⌘W")).clicked() {
                    self.request_close(self.active);
                }
            });
            ui.menu_button("Game", |ui| {
                if ui.add_enabled(!self.docs.is_empty(), egui::Button::new("Test in game").shortcut_text("⌘R")).on_hover_text("Start the game with the open edited files in place of the originals").clicked() {
                    self.test_in_game();
                }
            });
            ui.menu_button("Edit", |ui| {
                let (u, r) = self.docs.get(self.active).map(|d| (d.can_undo(), d.can_redo())).unwrap_or((false, false));
                if ui.add_enabled(u, egui::Button::new("Undo").shortcut_text("⌘Z")).clicked() {
                    self.docs[self.active].undo();
                }
                if ui.add_enabled(r, egui::Button::new("Redo").shortcut_text("⇧⌘Z")).clicked() {
                    self.docs[self.active].redo();
                }
            });
        });
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = Modifiers::COMMAND;
        let sc = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)));
        // check the shifted variants first, they include the plain ones
        if sc(cmd | Modifiers::SHIFT, Key::S) {
            self.save(true);
        } else if sc(cmd, Key::S) {
            self.save(false);
        }
        if sc(cmd | Modifiers::SHIFT, Key::Z) {
            if let Some(d) = self.docs.get_mut(self.active) {
                d.redo();
            }
        } else if sc(cmd, Key::Z) {
            if let Some(d) = self.docs.get_mut(self.active) {
                d.undo();
            }
        }
        if sc(cmd, Key::R) && !self.docs.is_empty() {
            self.test_in_game();
        }
        if sc(cmd, Key::W) && !self.docs.is_empty() {
            self.request_close(self.active);
        }
    }

    fn test_in_game(&mut self) {
        self.status = crate::launch::test_in_game(&self.docs, self.active).unwrap_or_else(|e| e);
    }

    fn request_close(&mut self, i: usize) {
        if self.docs.get(i).is_some_and(|d| d.dirty()) {
            self.confirm_close = Some(i);
        } else {
            self.close(i);
        }
    }

    fn close(&mut self, i: usize) {
        if i < self.docs.len() {
            self.docs.remove(i);
            self.active = self.active.min(self.docs.len().saturating_sub(1));
        }
    }

    fn debug_shot(&mut self, ctx: &egui::Context) {
        let Some((path, frames)) = &mut self.shot else { return };
        *frames += 1;
        ctx.request_repaint();
        if *frames == 30 {
            let path = path.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
                let f = std::fs::File::create(&path).expect("screenshot file");
                let mut enc = png::Encoder::new(std::io::BufWriter::new(f), img.size[0] as u32, img.size[1] as u32);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                enc.write_header().and_then(|mut w| w.write_image_data(&bytes)).expect("png");
                std::process::exit(0);
            })));
        }
    }

    fn browser(&mut self, ui: &mut egui::Ui) {
        ui.heading("Resources");
        match &self.data_dir {
            Some(d) => {
                ui.label(egui::RichText::new(d.display().to_string()).small().color(Color32::GRAY));
            }
            None => {
                ui.label("Choose the game data folder (the one with DATA/, MAPS/, OBJECTS/, SOUND/).");
                if ui.button("Choose folder…").clicked() {
                    if let Some(d) = rfd::FileDialog::new().pick_folder() {
                        self.set_data_dir(d);
                    }
                }
                return;
            }
        }
        ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("filter"));
        let filter = self.filter.to_ascii_lowercase();
        let mut open = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut i = 0;
            while i < self.entries.len() {
                let kind = self.entries[i].kind;
                let group: Vec<&Entry> = self.entries[i..].iter().take_while(|e| e.kind == kind).collect();
                let n = group.len();
                let shown: Vec<&&Entry> = group.iter().filter(|e| filter.is_empty() || e.rel.to_ascii_lowercase().contains(&filter)).collect();
                if !shown.is_empty() {
                    egui::CollapsingHeader::new(format!("{} ({})", kind.label(), shown.len())).default_open(kind == Kind::Level || !filter.is_empty()).show(ui, |ui| {
                        for e in shown {
                            if ui.selectable_label(false, &e.rel).double_clicked() {
                                open = Some(e.path.clone());
                            }
                        }
                    });
                }
                i += n;
            }
        });
        if let Some(p) = open {
            self.open_path(p);
        }
    }
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.shortcuts(&ctx);
        self.debug_shot(&ctx);
        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(d) = self.docs.get(self.active) {
                    ui.label(egui::RichText::new(d.kind().label()).color(Color32::GRAY));
                    ui.separator();
                }
                ui.label(&self.status);
            });
        });
        egui::Panel::left("browser").default_size(240.0).resizable(true).show(ui, |ui| self.browser(ui));
        if !self.docs.is_empty() {
            egui::Panel::right("inspector").default_size(300.0).size_range(220.0..=480.0).resizable(true).show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.docs[self.active].side_ui(ui));
            });
        }
        egui::CentralPanel::default().show(ui, |ui| {
            if self.docs.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("Double-click a resource on the left to open it.");
                });
                return;
            }
            // tabs
            let mut close = None;
            ui.horizontal_wrapped(|ui| {
                for (i, d) in self.docs.iter().enumerate() {
                    let title = if d.dirty() { format!("● {}", d.title()) } else { d.title() };
                    if ui.selectable_label(i == self.active, title).clicked() {
                        self.active = i;
                    }
                    if ui.small_button("×").clicked() {
                        close = Some(i);
                    }
                    ui.add_space(8.0);
                }
            });
            if let Some(i) = close {
                self.request_close(i);
            }
            ui.separator();
            if let Some(d) = self.docs.get_mut(self.active) {
                d.ui(ui);
            }
        });
        if self.docs.get_mut(self.active).is_some_and(|d| d.wants_test()) {
            self.test_in_game();
        }
        if let Some(i) = self.confirm_close {
            let title = self.docs.get(i).map(|d| d.title()).unwrap_or_default();
            egui::Modal::new(egui::Id::unique("confirm-close")).show(&ctx, |ui| {
                ui.label(format!("{title} has unsaved changes."));
                ui.horizontal(|ui| {
                    if ui.button("Discard").clicked() {
                        self.close(i);
                        self.confirm_close = None;
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm_close = None;
                    }
                });
            });
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if let Some(d) = &self.data_dir {
            storage.set_string(DATA_KEY, d.display().to_string());
        }
    }
}
