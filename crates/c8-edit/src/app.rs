//! The editor window: menu bar, resource browser (left), document tabs (centre), inspector (right).
use crate::doc::{self, Document};
use crate::resources::{self, Entry, Kind};
use eframe::egui::{self, Color32, Key, KeyboardShortcut, Modifiers};
use std::path::{Path, PathBuf};

const DATA_KEY: &str = "data_dir";
const MCP_KEY: &str = "mcp";
/// Default port of the live MCP endpoint (another free port is used when it is taken).
const MCP_PORT: u16 = 47_811;

/// Live MCP settings, remembered between runs: on/off, port and token (so a client configured
/// once keeps working).
#[derive(Clone)]
struct McpPrefs {
    enabled: bool,
    port: u16,
    token: String,
}

impl McpPrefs {
    fn load(s: Option<&dyn eframe::Storage>) -> McpPrefs {
        let v = s.and_then(|s| s.get_string(MCP_KEY)).unwrap_or_default();
        let mut it = v.split(' ');
        let enabled = it.next() != Some("off");
        let port = it.next().and_then(|p| p.parse().ok()).unwrap_or(MCP_PORT);
        // debug automation: HIEDIT_MCP_TOKEN / HIEDIT_MCP_PORT fix the token and port
        let token = std::env::var("HIEDIT_MCP_TOKEN").ok().or_else(|| it.next().filter(|t| t.len() >= 16).map(str::to_string)).unwrap_or_else(crate::mcp::server::new_token);
        let port = std::env::var("HIEDIT_MCP_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(port);
        McpPrefs { enabled, port, token }
    }
    fn save(&self) -> String {
        format!("{} {} {}", if self.enabled { "on" } else { "off" }, self.port, self.token)
    }
}

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
    mcp_prefs: McpPrefs,
    mcp: Option<crate::mcp::server::Live>,
    mcp_error: Option<String>,
    show_mcp: bool,
    ctx: egui::Context,
}

impl EditorApp {
    pub fn new(cc: &eframe::CreationContext<'_>, data: Option<PathBuf>, open: Option<PathBuf>) -> EditorApp {
        if let Some(rs) = &cc.wgpu_render_state {
            crate::gpu::set(rs.clone());
        }
        let remembered = cc.storage.and_then(|s| s.get_string(DATA_KEY)).map(PathBuf::from);
        let mcp_prefs = McpPrefs::load(cc.storage);
        let mut app = EditorApp {
            data_dir: None,
            entries: Vec::new(),
            filter: String::new(),
            docs: Vec::new(),
            active: 0,
            status: String::new(),
            confirm_close: None,
            shot: std::env::var("HIEDIT_SHOT").ok().map(|p| (p, 0)),
            mcp_prefs,
            mcp: None,
            mcp_error: None,
            show_mcp: std::env::var("HIEDIT_MCP_PANEL").is_ok(),
            ctx: cc.egui_ctx.clone(),
        };
        // debug screenshots do not start the endpoint
        if app.mcp_prefs.enabled && (app.shot.is_none() || std::env::var("HIEDIT_MCP_TOKEN").is_ok()) {
            app.start_mcp();
        }
        let candidates = [data, remembered, Some(PathBuf::from("resources"))];
        if let Some(d) = candidates.into_iter().flatten().find(|d| resources::is_data_dir(d)) {
            app.set_data_dir(d);
        }
        if let Some(p) = open {
            app.open_path(p);
        }
        app
    }

    fn start_mcp(&mut self) {
        let ctx = self.ctx.clone();
        match crate::mcp::server::start_live(self.mcp_prefs.port, self.mcp_prefs.token.clone(), std::sync::Arc::new(move || ctx.request_repaint())) {
            Ok(live) => {
                self.mcp_prefs.port = live.port;
                self.mcp = Some(live);
                self.mcp_error = None;
            }
            Err(e) => self.mcp_error = Some(e),
        }
    }

    fn stop_mcp(&mut self) {
        self.mcp = None;
    }

    /// Answer the agent's requests (on this, the UI thread, where the documents live).
    fn serve_mcp(&mut self) {
        let Some(live) = &self.mcp else { return };
        let reqs: Vec<_> = live.rx.try_iter().collect();
        for r in reqs {
            crate::mcp::server::handle(self, r);
        }
    }

    fn mcp_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_mcp;
        let mut renew = false;
        egui::Window::new("MCP (agent access)").open(&mut open).default_width(460.0).show(ctx, |ui| {
            let mut enabled = self.mcp_prefs.enabled;
            if ui.checkbox(&mut enabled, "Let MCP clients (e.g. Claude Code) edit the open levels").changed() {
                self.mcp_prefs.enabled = enabled;
                if enabled { self.start_mcp() } else { self.stop_mcp() }
            }
            if let Some(e) = &self.mcp_error {
                ui.colored_label(Color32::from_rgb(230, 90, 80), e);
            }
            let Some(live) = &self.mcp else { return };
            ui.label(format!("Endpoint: {} (this computer only)", live.url()));
            ui.horizontal(|ui| {
                ui.label("Token:");
                ui.monospace(live.token());
            });
            ui.horizontal(|ui| {
                if ui.button("Copy Claude Code command").on_hover_text(live.claude_command()).clicked() {
                    ctx.copy_text(live.claude_command());
                    self.status = "copied the `claude mcp add` command".into();
                }
                if ui.button("New token").on_hover_text("Clients configured with the old token stop working").clicked() {
                    renew = true;
                }
            });
            let mut paused = live.handler.paused.load(std::sync::atomic::Ordering::Relaxed);
            if ui.checkbox(&mut paused, "Pause agent edits (reading still works)").changed() {
                live.handler.paused.store(paused, std::sync::atomic::Ordering::Relaxed);
            }
            ui.label(egui::RichText::new("Agent edits go into the level's normal undo history, labelled \"MCP: ...\" (Cmd+Z undoes them). Tools work on the active level tab, or the last level opened.").small().color(Color32::GRAY));
            ui.separator();
            ui.label("Activity");
            egui::ScrollArea::vertical().max_height(220.0).stick_to_bottom(true).show(ui, |ui| {
                let lines = live.handler.log.lines();
                if lines.is_empty() {
                    ui.label(egui::RichText::new("no requests yet").color(Color32::GRAY));
                }
                for l in lines {
                    ui.label(egui::RichText::new(l).small().monospace());
                }
            });
        });
        self.show_mcp = open;
        if renew {
            self.mcp_prefs.token = crate::mcp::server::new_token();
            if let Some(l) = &self.mcp {
                l.set_token(self.mcp_prefs.token.clone());
            }
        }
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
        let at = std::env::var("HIEDIT_SHOT_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(30);
        if *frames == at {
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
        self.serve_mcp();
        self.shortcuts(&ctx);
        self.debug_shot(&ctx);
        if self.show_mcp {
            self.mcp_window(&ctx);
        }
        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(d) = self.docs.get(self.active) {
                    ui.label(egui::RichText::new(d.kind().label()).color(Color32::GRAY));
                    ui.separator();
                }
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (text, colour) = match &self.mcp {
                        Some(l) if l.handler.paused.load(std::sync::atomic::Ordering::Relaxed) => (format!("MCP :{} paused", l.port), Color32::from_rgb(230, 180, 60)),
                        Some(l) => (format!("MCP :{}", l.port), Color32::from_rgb(110, 200, 120)),
                        None => ("MCP off".to_string(), Color32::GRAY),
                    };
                    if ui.add(egui::Button::new(egui::RichText::new(text).color(colour)).frame(false)).on_hover_text("Agent access: endpoint, token, activity").clicked() {
                        self.show_mcp = !self.show_mcp;
                    }
                });
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
        storage.set_string(MCP_KEY, self.mcp_prefs.save());
    }
}

/// Live MCP: the agent works on the editor's open documents.
impl crate::mcp::Workspace for EditorApp {
    fn data_dir(&self) -> Option<PathBuf> {
        self.data_dir.clone()
    }
    fn levels(&mut self) -> Vec<(String, PathBuf, bool, bool)> {
        let target = self.default_level();
        self.docs
            .iter_mut()
            .enumerate()
            .filter_map(|(i, d)| {
                let (t, p, dirty) = (d.title(), d.path().map(Path::to_path_buf).unwrap_or_default(), d.dirty());
                d.as_level().map(|_| (t, p, dirty, Some(i) == target))
            })
            .collect()
    }
    fn level(&mut self, name: Option<&str>) -> Result<&mut crate::doc::level::LevelDoc, String> {
        let i = match name {
            None => self.default_level().ok_or("no level is open in the editor: use level_open")?,
            Some(n) => self
                .docs
                .iter_mut()
                .position(|d| (d.title().eq_ignore_ascii_case(n) || d.path().and_then(|p| p.file_name()).is_some_and(|f| f.to_string_lossy().eq_ignore_ascii_case(n))) && d.as_level().is_some())
                .ok_or(format!("no open level named {n}"))?,
        };
        Ok(self.docs[i].as_level().unwrap())
    }
    fn open_level(&mut self, path: PathBuf) -> Result<String, String> {
        if Kind::of(&path.to_string_lossy()) != Kind::Level {
            return Err(format!("{} is not a level file (LEVEL*.DAT)", path.display()));
        }
        let before = self.docs.len();
        self.open_path(path.clone());
        let opened = self.docs.len() > before || self.docs.get(self.active).is_some_and(|d| d.path() == Some(path.as_path()));
        if !opened {
            return Err(self.status.clone());
        }
        Ok(self.docs[self.active].title())
    }
    fn may_write(&self, p: &Path) -> Result<(), String> {
        if self.is_original(p) { Err(format!("{} is in the game data folder, which is never written", p.display())) } else { Ok(()) }
    }
    fn may_read(&self, _: &Path) -> Result<(), String> {
        Ok(())
    }
}

impl EditorApp {
    /// The level MCP tools work on by default: the active tab if it is a level, else the last
    /// level tab.
    fn default_level(&mut self) -> Option<usize> {
        if self.docs.get_mut(self.active).is_some_and(|d| d.as_level().is_some()) {
            return Some(self.active);
        }
        (0..self.docs.len()).rev().find(|&i| self.docs[i].as_level().is_some())
    }
}
