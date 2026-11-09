//! Sound and music bank editor (`SOUND/SOUND.DAT`, `SOUND/MUSIC.DAT`, spec 01 §11): lists the
//! banks, format slots and items, plays samples and tunes, exports them, and replaces samples
//! from WAV files and tunes from XMIDI files.
use super::history::{Command, History};
use super::Document;
use crate::audio;
use crate::resources::Kind;
use eframe::egui::{self, Color32, RichText, Sense, Stroke, Vec2};
use c8_audio::container::{Container, Entry, MUSIC_SLOT, MUSIC_SLOTS, SOUND_SLOT, SOUND_SLOTS};
use c8_audio::{music, wav, xmidi};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const SOUND_FORMATS: [&str; 6] = ["16-bit 44 kHz", "16-bit 22 kHz", "16-bit 11 kHz", "8-bit 22 kHz (used by the port)", "8-bit 11 kHz", "8-bit 11 kHz, reduced set"];
const MUSIC_FORMATS: [&str; 4] = ["General MIDI (.GEN)", "Roland MT-32 (.ROL)", "FM / OPL (.XMI, used by the port)", "Wavetable (.WTB)"];

/// What a sample of bank 0 is used for (spec 09).
fn sample_use(n: usize) -> &'static str {
    match n {
        1 => "engine of the viewed craft (loop)",
        2 => "engines of the other crafts (loop)",
        3 => "minigun shot",
        4 => "minigun overheated",
        5 => "missile launch",
        6 => "explosion",
        7 => "charging, refill; front-end box morph",
        8 => "booster charging",
        9 => "booster fired",
        10..=12 => "bullet ricochet",
        13 => "damage, craft contact",
        14 => "collectable taken",
        15 => "homing missile on you",
        16 | 22..=24 | 30..=32 => "silent placeholder",
        17 => "low fuel / shield / ammo warning",
        18 => "not used",
        19 => "craft contact (loop)",
        20 => "front end: cursor, carousel",
        21 => "front end: select",
        25 => "countdown light 1 and 2",
        26 => "countdown light 3",
        27 => "finish 3rd, 180! bonus",
        28 => "finish 2nd, 360! bonus",
        29 => "finish 1st, 720!, SUPER CAR",
        33 => "typewriter",
        34 => "typewriter return",
        35 => "front-end box opens",
        _ => "",
    }
}

fn tune_use(n: usize) -> &'static str {
    match n {
        1 => "race, tracks 1–2",
        2 => "race, tracks 3–4",
        3 => "race, tracks 5–6",
        4 => "intro movie",
        5 => "unused (tracks above 6)",
        _ => "",
    }
}

/// Replace items: (bank, slot, index, before, after) for each.
struct ItemEdit {
    label: String,
    items: Vec<(usize, usize, usize, Entry, Entry)>,
}

impl Command<Container> for ItemEdit {
    fn apply(&mut self, c: &mut Container) {
        for (b, s, i, _, after) in &self.items {
            if let Some(Some(slot)) = c.banks.get_mut(*b).and_then(|x| x.get_mut(*s)) {
                slot.items[*i] = after.clone();
            }
        }
    }
    fn revert(&mut self, c: &mut Container) {
        for (b, s, i, before, _) in &self.items {
            if let Some(Some(slot)) = c.banks.get_mut(*b).and_then(|x| x.get_mut(*s)) {
                slot.items[*i] = before.clone();
            }
        }
    }
    fn label(&self) -> String {
        self.label.clone()
    }
}

pub struct SoundDoc {
    path: PathBuf,
    rel: String,
    music: bool,
    c: Container,
    history: History<Container>,
    bank: usize,
    slot: usize,
    sel: usize,
    /// replace a sample in all 8-bit slots (3, 4, 5) at once
    all_slots: bool,
    timbres: Option<music::TimbreBank>,
    /// decoded selected sample: (bank, slot, item, generation) → data
    wave: Option<((usize, usize, usize, u64), Arc<wav::Sample>)>,
    /// lengths in seconds of the listed items: (bank, slot, generation) → lengths
    lengths: Option<((usize, usize, u64), Vec<f64>)>,
    generation: u64,
    message: String,
    test: bool,
}

impl SoundDoc {
    pub fn open(path: PathBuf, rel: String) -> Result<SoundDoc, String> {
        let file = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let music = path.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("music.dat"));
        let (slots, slot) = if music { (MUSIC_SLOTS, MUSIC_SLOT) } else { (SOUND_SLOTS, SOUND_SLOT) };
        let c = Container::parse(&file, slots).map_err(|e| format!("{rel}: {e}"))?;
        let timbres = if music { c8_data::files::read_raw("sound/sample.opl").ok().and_then(|d| music::TimbreBank::parse(&d).ok()) } else { None };
        Ok(SoundDoc { path, rel, music, c, history: History::default(), bank: 0, slot, sel: 0, all_slots: true, timbres, wave: None, lengths: None, generation: 0, message: String::new(), test: false })
    }

    fn formats(&self) -> &'static [&'static str] {
        if self.music { &MUSIC_FORMATS } else { &SOUND_FORMATS }
    }

    fn items(&self) -> &[Entry] {
        match self.c.banks.get(self.bank).and_then(|b| b.get(self.slot)) {
            Some(Some(s)) => &s.items,
            _ => &[],
        }
    }

    fn tag(&self, i: usize) -> String {
        format!("{}#{}#{}#{}", self.rel, self.bank, self.slot, i)
    }

    fn sample(&mut self, i: usize) -> Option<Arc<wav::Sample>> {
        let key = (self.bank, self.slot, i, self.generation);
        if self.wave.as_ref().is_none_or(|(k, _)| *k != key) {
            let it = self.items().get(i)?;
            let d = c8_data::rnc::maybe_unpack(it.data.clone()).unwrap_or_default();
            self.wave = Some((key, Arc::new(wav::parse(&it.name, &d))));
        }
        self.wave.as_ref().map(|(_, w)| w.clone())
    }

    fn sequence(&self, i: usize) -> Result<xmidi::Sequence, String> {
        let it = self.items().get(i).ok_or("no such item")?;
        xmidi::parse(&it.data)?.into_iter().next().ok_or_else(|| "no sequence".into())
    }

    fn player(&self, i: usize, rate: u32) -> Result<music::MusicPlayer, String> {
        let bank = self.timbres.clone().ok_or("SOUND/SAMPLE.OPL (the FM timbres) not found")?;
        Ok(music::MusicPlayer::new(self.sequence(i)?, bank, rate))
    }

    fn play(&mut self, i: usize) {
        let tag = self.tag(i);
        let r = if self.music {
            audio::rate().and_then(|rate| {
                let mut p = self.player(i, rate)?;
                p.looping = false;
                audio::play_music(&tag, p)
            })
        } else {
            match self.sample(i) {
                Some(s) if !s.data.is_empty() => audio::play_sample(&tag, Arc::new(s.data.clone()), s.rate),
                _ => Err("empty sample".into()),
            }
        };
        if let Err(e) = r {
            self.message = e;
        }
    }

    fn duration(&mut self, i: usize) -> f64 {
        let key = (self.bank, self.slot, self.generation);
        if self.lengths.as_ref().is_none_or(|(k, _)| *k != key) {
            let v = (0..self.items().len())
                .map(|i| {
                    if self.music {
                        self.sequence(i).map(|s| s.length() as f64 / 120.0).unwrap_or(0.0)
                    } else {
                        let it = &self.items()[i];
                        wav::parse(&it.name, &c8_data::rnc::maybe_unpack(it.data.clone()).unwrap_or_default()).duration()
                    }
                })
                .collect();
            self.lengths = Some((key, v));
        }
        self.lengths.as_ref().and_then(|(_, v)| v.get(i).copied()).unwrap_or(0.0)
    }

    /// Replace the selected sample with a WAV file, converted to each slot's format.
    fn import_wav(&mut self, path: &Path) -> Result<String, String> {
        let d = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let slots: Vec<usize> = if self.all_slots && (3..=5).contains(&self.slot) { vec![3, 4, 5] } else { vec![self.slot] };
        let mut items = Vec::new();
        for s in slots {
            let Some(Some(slot)) = self.c.banks.get(self.bank).and_then(|b| b.get(s)) else { continue };
            let Some(old) = slot.items.get(self.sel) else { continue };
            // keep the slot's rate (the old sample's, or the slot's nominal one)
            let old_rate = wav::parse("", &c8_data::rnc::maybe_unpack(old.data.clone()).unwrap_or_default()).rate;
            let rate = if s == 3 { 22050 } else if old_rate > 0 && old_rate != 22050 { old_rate } else { 11025 };
            let pcm = wav::convert_to_u8(&d, rate)?;
            let mut new = old.clone();
            new.data = wav::write_u8(rate, &pcm);
            items.push((self.bank, s, self.sel, old.clone(), new));
        }
        if items.is_empty() {
            return Err("nothing to replace".into());
        }
        let n = items.len();
        let name = self.items().get(self.sel).map(|e| e.name.clone()).unwrap_or_default();
        self.history.run(Box::new(ItemEdit { label: format!("replace {name}"), items }), &mut self.c);
        self.generation += 1;
        Ok(format!("replaced {name} in {n} slot{}", if n == 1 { "" } else { "s" }))
    }

    /// Replace the selected tune with an XMIDI file.
    fn import_xmi(&mut self, path: &Path) -> Result<String, String> {
        let d = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let seqs = xmidi::parse(&d).map_err(|e| format!("not an XMIDI file: {e}"))?;
        if seqs.is_empty() || seqs[0].events.is_empty() {
            return Err("the file has no XMIDI sequence".into());
        }
        let old = self.items().get(self.sel).ok_or("no tune selected")?.clone();
        let mut new = old.clone();
        new.data = d;
        let name = old.name.clone();
        self.history.run(Box::new(ItemEdit { label: format!("replace {name}"), items: vec![(self.bank, self.slot, self.sel, old, new)] }), &mut self.c);
        self.generation += 1;
        Ok(format!("replaced {name}"))
    }

    /// Render the selected tune once through (at most 10 minutes) as a 44.1 kHz stereo WAV.
    fn render_tune(&self, i: usize) -> Result<Vec<u8>, String> {
        const RATE: u32 = 44100;
        let mut p = self.player(i, RATE)?;
        p.looping = false;
        let mut out = Vec::new();
        let mut buf = vec![0f32; 4096];
        while !p.finished() && out.len() < (RATE * 600 * 2) as usize {
            p.render(&mut buf, 1.0, true);
            out.extend_from_slice(&buf);
        }
        Ok(wav::write_i16(RATE, 2, &out))
    }

    fn waveform(&mut self, ui: &mut egui::Ui) {
        let Some(s) = self.sample(self.sel) else { return };
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 120.0), Sense::click());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 3.0, Color32::from_rgb(20, 24, 32));
        p.line_segment([rect.left_center(), rect.right_center()], Stroke::new(1.0, Color32::from_gray(60)));
        let n = s.data.len();
        if n > 0 {
            let cols = rect.width().max(1.0) as usize;
            for x in 0..cols {
                let (a, b) = (x * n / cols, ((x + 1) * n / cols).max(x * n / cols + 1).min(n));
                let (lo, hi) = s.data[a..b].iter().fold((1f32, -1f32), |(l, h), &v| (l.min(v), h.max(v)));
                let px = rect.left() + x as f32 + 0.5;
                p.line_segment([egui::pos2(px, rect.center().y - hi * rect.height() / 2.0), egui::pos2(px, rect.center().y - lo * rect.height() / 2.0)], Stroke::new(1.0, Color32::from_rgb(110, 190, 250)));
            }
            if let Some((tag, t)) = audio::playing() {
                if tag == self.tag(self.sel) {
                    let x = rect.left() + (t / s.duration().max(1e-6)).min(1.0) as f32 * rect.width();
                    p.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], Stroke::new(1.5, Color32::WHITE));
                    ui.ctx().request_repaint();
                }
            }
        }
        if resp.clicked() {
            let sel = self.sel;
            self.play(sel);
        }
        ui.label(RichText::new(format!("{} samples at {} Hz, {:.2} s, 8-bit mono; click to play", n, s.rate, s.duration())).small().color(Color32::GRAY));
    }
}

impl Document for SoundDoc {
    fn title(&self) -> String {
        self.rel.clone()
    }
    fn kind(&self) -> Kind {
        Kind::Sound
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
        std::fs::write(path, self.c.encode()).map_err(|e| format!("{}: {e}", path.display()))?;
        self.path = path.to_path_buf();
        self.history.mark_saved();
        Ok(())
    }
    fn undo(&mut self) {
        self.history.undo(&mut self.c);
        self.generation += 1;
    }
    fn redo(&mut self) {
        self.history.redo(&mut self.c);
        self.generation += 1;
    }
    fn can_undo(&self) -> bool {
        self.history.can_undo()
    }
    fn can_redo(&self) -> bool {
        self.history.can_redo()
    }
    fn game_files(&self) -> Result<Vec<(String, Vec<u8>)>, String> {
        let name = if self.music { "SOUND/MUSIC.DAT" } else { "SOUND/SOUND.DAT" };
        Ok(vec![(name.into(), self.c.encode())])
    }
    fn game_args(&self) -> Vec<String> {
        let race = |t: usize| vec!["--no-intro".to_string(), "--race".into(), t.to_string()];
        if self.music {
            match self.sel + 1 {
                2 => race(3),
                3 => race(5),
                4 => vec![], // the intro
                _ => race(1),
            }
        } else if self.bank == 1 {
            vec![]
        } else if matches!(self.sel + 1, 20 | 21 | 33..=35) {
            vec!["--no-intro".into()]
        } else {
            race(1)
        }
    }
    fn wants_test(&mut self) -> bool {
        std::mem::take(&mut self.test)
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Bank");
            for b in 0..self.c.banks.len() {
                let name = match (self.music, b) {
                    (false, 0) => "0: race and front end".to_string(),
                    (false, 1) => "1: intro".to_string(),
                    _ => b.to_string(),
                };
                if ui.selectable_label(self.bank == b, name).clicked() {
                    self.bank = b;
                    self.sel = 0;
                }
            }
            ui.separator();
            ui.label("Format");
            let formats = self.formats();
            egui::ComboBox::from_id_salt("slot").selected_text(formats[self.slot]).width(260.0).show_ui(ui, |ui| {
                for (s, f) in formats.iter().enumerate() {
                    let present = matches!(self.c.banks.get(self.bank).and_then(|b| b.get(s)), Some(Some(_)));
                    let label = if present { f.to_string() } else { format!("{f} (empty)") };
                    ui.add_enabled_ui(present, |ui| ui.selectable_value(&mut self.slot, s, label));
                }
            });
        });
        ui.separator();
        let n = self.items().len();
        if n == 0 {
            ui.label("This bank has nothing in this format.");
            return;
        }
        let playing = audio::playing();
        egui::ScrollArea::vertical().auto_shrink([false, true]).max_height(ui.available_height() - if self.music { 10.0 } else { 160.0 }).show(ui, |ui| {
            egui::Grid::new("items").striped(true).num_columns(6).spacing([14.0, 4.0]).show(ui, |ui| {
                for h in ["#", "", "Name", "Size", "Length", "Use"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for i in 0..n {
                    let (name, size) = (self.items()[i].name.clone(), self.items()[i].data.len());
                    ui.label(format!("{}", i + 1));
                    let is_playing = playing.as_ref().is_some_and(|(t, _)| *t == self.tag(i));
                    if ui.small_button(if is_playing { "Stop" } else { "Play" }).clicked() {
                        self.sel = i;
                        if is_playing {
                            audio::stop();
                        } else {
                            self.play(i);
                        }
                    }
                    if ui.selectable_label(self.sel == i, &name).clicked() {
                        self.sel = i;
                    }
                    ui.label(format!("{size}"));
                    let d = self.duration(i);
                    ui.label(if d >= 60.0 { format!("{}:{:04.1}", (d / 60.0) as u32, d % 60.0) } else { format!("{d:.2} s") });
                    let usage = match (self.music, self.bank) {
                        (true, _) => tune_use(i + 1),
                        (false, 0) => sample_use(i + 1),
                        _ => "intro",
                    };
                    ui.label(RichText::new(usage).color(Color32::GRAY));
                    ui.end_row();
                }
            });
        });
        if playing.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
        if !self.music {
            ui.separator();
            self.waveform(ui);
        }
    }

    fn side_ui(&mut self, ui: &mut egui::Ui) {
        if ui.button("Test in game").on_hover_text("Start the game where this sound or tune is heard").clicked() {
            self.test = true;
        }
        ui.separator();
        let Some(it) = self.items().get(self.sel).cloned() else { return };
        ui.heading(format!("{} {}", self.sel + 1, it.name));
        ui.label(format!("{} bytes, {}", it.data.len(), self.formats()[self.slot]));
        let tag = self.tag(self.sel);
        ui.horizontal(|ui| {
            if ui.button("Play").clicked() {
                let sel = self.sel;
                self.play(sel);
            }
            if ui.button("Stop").clicked() {
                audio::stop();
            }
            if let Some((t, s)) = audio::playing() {
                if t == tag {
                    ui.label(format!("{s:.1} s"));
                }
            }
        });
        ui.separator();
        let stem = it.name.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or(it.name.clone());
        if self.music {
            let ext = it.name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or("xmi".into());
            if ui.button(format!("Export .{ext}…")).on_hover_text("The tune as stored (XMIDI)").clicked() {
                if let Some(p) = rfd::FileDialog::new().set_file_name(&it.name).save_file() {
                    self.message = std::fs::write(&p, &it.data).map(|_| format!("wrote {}", p.display())).unwrap_or_else(|e| e.to_string());
                }
            }
            if ui.button("Export as WAV…").on_hover_text("Rendered once through on the emulated OPL3, 44.1 kHz stereo").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("WAV", &["wav"]).set_file_name(format!("{stem}.wav")).save_file() {
                    self.message = self.render_tune(self.sel).and_then(|w| std::fs::write(&p, w).map_err(|e| e.to_string())).map(|_| format!("wrote {}", p.display())).unwrap_or_else(|e| e);
                }
            }
            if ui.button("Replace from XMIDI…").on_hover_text("An .XMI file (Miles extended MIDI), e.g. exported from another tune").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("XMIDI", &["xmi", "gen", "rol", "wtb"]).pick_file() {
                    self.message = self.import_xmi(&p).unwrap_or_else(|e| e);
                }
            }
            if self.slot != MUSIC_SLOT {
                ui.label(RichText::new("The port plays the FM tunes only; other formats are previewed with the FM timbres and sound different from their intended synthesizer.").small().color(Color32::GRAY));
            }
        } else {
            if ui.button("Export WAV…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("WAV", &["wav"]).set_file_name(format!("{stem}.wav")).save_file() {
                    let d = c8_data::rnc::maybe_unpack(it.data.clone()).unwrap_or_default();
                    self.message = std::fs::write(&p, d).map(|_| format!("wrote {}", p.display())).unwrap_or_else(|e| e.to_string());
                }
            }
            if ui.button("Replace from WAV…").on_hover_text("Any PCM WAV; converted to 8-bit mono at the slot's rate").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("WAV", &["wav"]).pick_file() {
                    self.message = self.import_wav(&p).unwrap_or_else(|e| e);
                }
            }
            if (3..=5).contains(&self.slot) {
                ui.checkbox(&mut self.all_slots, "Replace in all 8-bit formats").on_hover_text("Slots 3, 4 and 5 (22 and 11 kHz); the port plays slot 3");
            }
        }
        if !self.message.is_empty() {
            ui.label(RichText::new(&self.message).small());
        }
        ui.separator();
        ui.label(RichText::new("Saving writes the container unpacked; the game reads packed and unpacked banks.").small().color(Color32::GRAY));
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

    fn res(rel: &str) -> PathBuf {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources")).join(rel)
    }

    #[test]
    fn replace_sample_and_tune() {
        c8_data::files::set_data_dir(res(""));
        let Ok(mut d) = SoundDoc::open(res("SOUND/SOUND.DAT"), "SOUND/SOUND.DAT".into()) else { return };
        assert_eq!(d.items().len(), 35);
        assert!((d.duration(1) - 3.64).abs() < 0.01, "ENGINE1 length");
        // a 0.5 s 44.1 kHz 16-bit stereo tone replaces ENGINE1 in the three 8-bit slots
        let tone: Vec<f32> = (0..22050 * 2).map(|i| ((i / 2) as f32 * 0.05).sin() * 0.5).collect();
        let tmp = std::env::temp_dir().join("hiedit-test-tone.wav");
        std::fs::write(&tmp, wav::write_i16(44100, 2, &tone)).unwrap();
        d.sel = 1;
        let orig = d.c.clone();
        d.import_wav(&tmp).unwrap();
        assert!((d.duration(1) - 0.5).abs() < 0.01);
        let file = d.c.encode();
        // the game's loader sees the new sample in slot 3, at 22050 Hz
        let items = c8_audio::container::load_bank(&file, SOUND_SLOTS, SOUND_SLOT, 0).unwrap();
        let s = wav::parse(&items[1].name, &items[1].data);
        assert_eq!((s.rate, s.data.len()), (22050, 11025));
        let s4 = wav::parse("", &c8_audio::container::load_bank(&file, SOUND_SLOTS, 4, 0).unwrap()[1].data);
        assert_eq!(s4.rate, 11025);
        d.undo();
        assert_eq!(d.c, orig);

        let Ok(mut m) = SoundDoc::open(res("SOUND/MUSIC.DAT"), "SOUND/MUSIC.DAT".into()) else { return };
        assert_eq!(m.items().len(), 5);
        let xmi = m.items()[3].data.clone();
        std::fs::write(&tmp, &xmi).unwrap();
        m.sel = 0;
        m.import_xmi(&tmp).unwrap();
        assert_eq!(m.items()[0].data, xmi);
        assert!(m.import_xmi(&res("SOUND/SAMPLE.OPL")).is_err());
        let _ = std::fs::remove_file(&tmp);
    }
}
