//! Music additions to the MUSIC.DAT editor (port deviation, spec 09 §9): OGG Vorbis
//! replacements per tune with their loop points, WAV export of either kind, and the MIDI
//! conversions (also used by the command line).
//!
//! An OGG for tune n lives at `SOUND/MUSIC/<NAME>.OGG` next to MUSIC.DAT, i.e. in a `MUSIC`
//! folder beside the edited MUSIC.DAT. Imports, loop edits and removals are kept with the
//! document and written when it is saved (and put in the override folder by "Test in game").
use crate::audio;
use eframe::egui::{self, Color32, RichText, Sense, Stroke, Vec2};
use c8_audio::{music, ogg, smf, tune, wav, xmidi};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Find `name` in `dir` ignoring letter case.
fn find_ci(dir: &Path, name: &str) -> Option<PathBuf> {
    let direct = dir.join(name);
    if direct.exists() {
        return Some(direct);
    }
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name)))
}

/// One tune's OGG replacement.
pub struct OggFile {
    /// The file as read or imported (its own loop comments untouched).
    pub bytes: Arc<Vec<u8>>,
    pub info: ogg::OggInfo,
    /// The loop as edited, in frames; `None` = the whole file.
    pub lp: Option<(u64, u64)>,
    /// Where it was read from (`None` = imported, not saved yet).
    pub from: Option<PathBuf>,
    decoded: Option<Arc<ogg::OggTune>>,
    /// Peak envelope for the overview: (bars, (min, max) per bar).
    envelope: Option<(usize, Vec<(f32, f32)>)>,
}

impl OggFile {
    fn new(bytes: Vec<u8>, from: Option<PathBuf>) -> Result<OggFile, String> {
        let info = ogg::probe(&bytes)?;
        if info.frames == 0 {
            return Err("the OGG file has no audio".into());
        }
        let lp = info.tagged.then_some((info.loop_start, info.loop_end));
        Ok(OggFile { bytes: Arc::new(bytes), info, lp, from, decoded: None, envelope: None })
    }

    pub fn seconds(&self) -> f64 {
        self.info.frames as f64 / self.info.rate.max(1) as f64
    }

    /// The loop region in frames (the whole file when untagged).
    pub fn region(&self) -> (u64, u64) {
        self.lp.unwrap_or((0, self.info.frames))
    }

    fn loop_edited(&self) -> bool {
        self.lp != self.info.tagged.then_some((self.info.loop_start, self.info.loop_end))
    }

    /// The bytes to write: the file with the edited loop in its comments.
    pub fn output(&self) -> Result<Vec<u8>, String> {
        if self.loop_edited() { ogg::set_loop_tags(&self.bytes, self.lp) } else { Ok(self.bytes.to_vec()) }
    }

    fn decoded(&mut self) -> Result<Arc<ogg::OggTune>, String> {
        if self.decoded.is_none() {
            self.decoded = Some(Arc::new(ogg::decode(&self.bytes)?));
        }
        let mut t = (**self.decoded.as_ref().unwrap()).clone();
        let (s, e) = self.region();
        (t.loop_start, t.loop_end) = (s.min(t.len()), e.min(t.len()));
        if t.loop_start >= t.loop_end {
            (t.loop_start, t.loop_end) = (0, t.len());
        }
        Ok(Arc::new(t))
    }

    /// A player at `rate` with the edited loop.
    pub fn player(&mut self, rate: u32) -> Result<ogg::OggPlayer, String> {
        Ok(ogg::OggPlayer::new(self.decoded()?, rate))
    }
}

/// The OGG replacements of the five tunes, with pending changes.
#[derive(Default)]
pub struct Tunes {
    pub files: Vec<Option<OggFile>>,
    /// Removed since the last save: delete the OGG beside the saved MUSIC.DAT.
    removed: Vec<bool>,
    pub dirty: bool,
    /// Folder of the open MUSIC.DAT (where its OGGs were looked up).
    dir: Option<PathBuf>,
}

impl Tunes {
    /// Look for each tune's OGG beside `music_dat` (`MUSIC/<NAME>.OGG`), then through the
    /// game's file lookup (override folder, data folder).
    pub fn scan(music_dat: &Path) -> Tunes {
        let dir = music_dat.parent().map(Path::to_path_buf);
        let mut files = Vec::new();
        for n in 1..=tune::TUNE_NAMES.len() {
            let name = format!("{}.OGG", tune::tune_name(n).unwrap());
            let local = dir.as_ref().and_then(|d| find_ci(d, "MUSIC")).and_then(|m| find_ci(&m, &name));
            let found = local.or_else(|| tune::find_ogg(n));
            files.push(found.and_then(|p| match std::fs::read(&p).map_err(|e| e.to_string()).and_then(|b| OggFile::new(b, Some(p.clone()))) {
                Ok(f) => Some(f),
                Err(e) => {
                    log::warn!("{}: {e}", p.display());
                    None
                }
            }));
        }
        Tunes { removed: vec![false; files.len()], files, dirty: false, dir }
    }

    pub fn get(&self, i: usize) -> Option<&OggFile> {
        self.files.get(i).and_then(Option::as_ref)
    }

    pub fn get_mut(&mut self, i: usize) -> Option<&mut OggFile> {
        self.files.get_mut(i).and_then(Option::as_mut)
    }

    /// Use an OGG file for tune `i` (0-based).
    pub fn import(&mut self, i: usize, path: &Path) -> Result<String, String> {
        if i >= self.files.len() {
            return Err("only the five tunes can have an OGG".into());
        }
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let f = OggFile::new(bytes, None)?;
        let msg = format!(
            "{}: OGG, {:.1} s, {} Hz, {} channel{}, {}; written to MUSIC/{}.OGG beside MUSIC.DAT when saved",
            tune::TUNE_NAMES[i],
            f.seconds(),
            f.info.rate,
            f.info.channels,
            if f.info.channels == 1 { "" } else { "s" },
            if f.lp.is_some() { "loop points from its comments" } else { "no loop comments: the whole file loops" },
            tune::TUNE_NAMES[i]
        );
        self.files[i] = Some(f);
        self.removed[i] = false;
        self.dirty = true;
        Ok(msg)
    }

    /// Drop tune `i`'s OGG (back to XMIDI).
    pub fn remove(&mut self, i: usize) -> String {
        let Some(f) = self.files.get_mut(i).and_then(Option::take) else { return "no OGG".into() };
        self.removed[i] = true;
        self.dirty = true;
        let in_data = f.from.as_ref().is_some_and(|p| p.starts_with(c8_data::files::data_dir()));
        let local = f.from.as_ref().is_some_and(|p| self.dir.as_ref().is_some_and(|d| p.starts_with(d)));
        if in_data && !local {
            format!("{}: removed here, but the file is in the game data folder ({}); delete it there too", tune::TUNE_NAMES[i], f.from.unwrap().display())
        } else {
            format!("{}: back to XMIDI (the OGG beside MUSIC.DAT is deleted when saved)", tune::TUNE_NAMES[i])
        }
    }

    /// The OGGs for "Test in game": (path relative to the data folder, bytes).
    pub fn game_files(&self) -> Result<Vec<(String, Vec<u8>)>, String> {
        let mut v = Vec::new();
        for (i, f) in self.files.iter().enumerate() {
            if let Some(f) = f {
                v.push((tune::ogg_rel(i + 1).unwrap(), f.output()?));
            }
        }
        Ok(v)
    }

    /// Write the OGGs beside a MUSIC.DAT saved at `music_dat` (and delete removed ones there).
    pub fn save(&mut self, music_dat: &Path) -> Result<(), String> {
        let dir = music_dat.parent().ok_or("no folder")?.to_path_buf();
        let mdir = find_ci(&dir, "MUSIC").unwrap_or_else(|| dir.join("MUSIC"));
        for i in 0..self.files.len() {
            let name = format!("{}.OGG", tune::TUNE_NAMES[i]);
            let existing = find_ci(&mdir, &name);
            if let Some(f) = self.files[i].as_mut() {
                let target = existing.unwrap_or_else(|| mdir.join(&name));
                if f.from.as_ref() != Some(&target) || f.loop_edited() {
                    std::fs::create_dir_all(&mdir).map_err(|e| format!("{}: {e}", mdir.display()))?;
                    let out = f.output()?;
                    std::fs::write(&target, &out).map_err(|e| format!("{}: {e}", target.display()))?;
                    *f = OggFile::new(out, Some(target))?;
                }
            } else if self.removed[i] {
                if let Some(p) = existing {
                    std::fs::remove_file(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                }
            }
            self.removed[i] = false;
        }
        self.dir = Some(dir);
        self.dirty = false;
        Ok(())
    }

    /// The OGG panel of the side bar for tune `i`: facts, loop editing, previews, remove.
    /// Returns a status message.
    pub fn ui(&mut self, ui: &mut egui::Ui, i: usize, tag: &str) -> Option<String> {
        let mut msg = None;
        let mut changed = false;
        let mut remove = false;
        {
            let Some(f) = self.get_mut(i) else { return None };
            let rate = f.info.rate.max(1) as f64;
            ui.label(RichText::new("OGG Vorbis replacement").strong().color(Color32::from_rgb(120, 200, 140)));
            ui.label(format!("{:.1} s, {} Hz, {} ch", f.seconds(), f.info.rate, f.info.channels));
            ui.label(RichText::new(match &f.from {
                Some(p) => p.display().to_string(),
                None => "imported, not saved yet".into(),
            }).small().color(Color32::GRAY));
            envelope(ui, f, tag);
            let total = f.info.frames;
            let mut on = f.lp.is_some();
            if ui.checkbox(&mut on, "Loop points").on_hover_text("Off: the whole file loops. On: LOOPSTART / LOOPLENGTH comments, in samples").changed() {
                f.lp = on.then_some(if f.info.tagged { (f.info.loop_start, f.info.loop_end) } else { (0, total) });
                changed = true;
            }
            if let Some((mut s, mut e)) = f.lp {
                egui::Grid::new("oggloop").num_columns(3).show(ui, |ui| {
                    ui.label("start");
                    changed |= ui.add(egui::DragValue::new(&mut s).range(0..=total.saturating_sub(1)).speed(rate / 100.0)).changed();
                    ui.label(format!("{:.3} s", s as f64 / rate));
                    ui.end_row();
                    ui.label("end");
                    changed |= ui.add(egui::DragValue::new(&mut e).range(1..=total).speed(rate / 100.0)).changed();
                    ui.label(format!("{:.3} s", e as f64 / rate));
                    ui.end_row();
                });
                if e <= s {
                    e = (s + 1).min(total);
                }
                f.lp = Some((s, e));
                ui.label(RichText::new(format!("LOOPSTART={s} LOOPLENGTH={}", e - s)).small().monospace().color(Color32::GRAY));
            }
            ui.horizontal(|ui| {
                if ui.button("Play loop seam").on_hover_text("Play from 3 s before the loop end, to hear the jump back").clicked() {
                    let (_, e) = f.region();
                    let r = audio::rate().and_then(|out| {
                        let mut p = f.player(out)?;
                        let at = (e as f64 / rate - 3.0).max(0.0);
                        p.seek(at);
                        audio::play_music_from(tag, p, at)
                    });
                    if let Err(e) = r {
                        msg = Some(e);
                    }
                }
                if ui.button("Remove OGG").on_hover_text("Back to the XMIDI tune").clicked() {
                    remove = true;
                }
            });
        }
        if changed {
            self.dirty = true;
            if let Some(f) = self.get_mut(i) {
                f.envelope = None;
            }
        }
        if remove {
            audio::stop();
            msg = Some(self.remove(i));
        }
        msg
    }
}

/// Overview of the OGG with the loop region shaded and the play position.
fn envelope(ui: &mut egui::Ui, f: &mut OggFile, tag: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 60.0), Sense::hover());
    let bars = rect.width().max(1.0) as usize;
    if f.envelope.as_ref().is_none_or(|(n, _)| *n != bars) {
        if let Ok(t) = f.decoded() {
            let n = t.len() as usize;
            let env = (0..bars)
                .map(|x| {
                    let (a, b) = (x * n / bars, ((x + 1) * n / bars).max(x * n / bars + 1).min(n));
                    t.frames[a * 2..b * 2].iter().fold((1f32, -1f32), |(l, h), &v| (l.min(v), h.max(v)))
                })
                .collect();
            f.envelope = Some((bars, env));
        }
    }
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 3.0, Color32::from_rgb(20, 24, 32));
    let total = f.info.frames.max(1) as f32;
    let (s, e) = f.region();
    let x = |fr: u64| rect.left() + fr as f32 / total * rect.width();
    p.rect_filled(egui::Rect::from_x_y_ranges(x(s)..=x(e), rect.y_range()), 0.0, Color32::from_rgba_unmultiplied(80, 160, 90, 40));
    if let Some((_, env)) = &f.envelope {
        for (i, (lo, hi)) in env.iter().enumerate() {
            let px = rect.left() + i as f32 + 0.5;
            p.line_segment([egui::pos2(px, rect.center().y - hi * rect.height() / 2.0), egui::pos2(px, rect.center().y - lo * rect.height() / 2.0)], Stroke::new(1.0, Color32::from_rgb(110, 190, 250)));
        }
    }
    for fr in [s, e] {
        p.line_segment([egui::pos2(x(fr), rect.top()), egui::pos2(x(fr), rect.bottom())], Stroke::new(1.0, Color32::from_rgb(120, 220, 140)));
    }
    if let Some((t, secs)) = audio::playing() {
        if t == tag {
            // the preview loops: fold the played time into the loop region
            let rate = f.info.rate.max(1) as f64;
            let mut fr = secs * rate;
            if fr >= e as f64 && e > s {
                fr = s as f64 + (fr - e as f64) % (e - s) as f64;
            }
            let px = x(fr as u64);
            p.line_segment([egui::pos2(px, rect.top()), egui::pos2(px, rect.bottom())], Stroke::new(1.5, Color32::WHITE));
            ui.ctx().request_repaint();
        }
    }
}

/// Add a `smpl` chunk with one forward loop [start, end) (frames) to a WAV file.
pub fn add_smpl(mut wav: Vec<u8>, rate: u32, start: u64, end: u64) -> Vec<u8> {
    let mut c = Vec::new();
    let u = |c: &mut Vec<u8>, v: u32| c.extend_from_slice(&v.to_le_bytes());
    for v in [0, 0, 1_000_000_000 / rate.max(1), 60, 0, 0, 0, 1, 0] {
        u(&mut c, v);
    }
    for v in [0, 0, start as u32, end.saturating_sub(1) as u32, 0, 0] {
        u(&mut c, v);
    }
    wav.extend_from_slice(b"smpl");
    wav.extend_from_slice(&(c.len() as u32).to_le_bytes());
    wav.extend_from_slice(&c);
    let riff = (wav.len() - 8) as u32;
    wav[4..8].copy_from_slice(&riff.to_le_bytes());
    wav
}

/// The OGG of tune `i` as a 16-bit WAV with its loop (decoded, at the file's rate).
pub fn ogg_wav(f: &mut OggFile) -> Result<(Vec<u8>, String), String> {
    let t = f.decoded()?;
    let w = add_smpl(wav::write_i16(t.rate, 2, &t.frames), t.rate, t.loop_start, t.loop_end);
    Ok((w, format!("{:.1} s at {} Hz, loop {}..{}", t.seconds(), t.rate, t.loop_start, t.loop_end)))
}

/// Rate of XMIDI renderings: a multiple of 120, so loop points fall on whole samples.
pub const RENDER_RATE: u32 = 48000;

/// Render an XMIDI tune for a looping recording (see [`tune::render_xmidi`]) as a 16-bit
/// 48 kHz stereo WAV with a `smpl` loop. The message gives the OGG loop comments to use.
pub fn xmidi_wav(seq: xmidi::Sequence, bank: music::TimbreBank) -> (Vec<u8>, String) {
    let r = tune::render_xmidi(seq, bank, RENDER_RATE, 600.0);
    let (s, e) = r.loop_frames;
    let peak = r.frames.iter().fold(0f32, |a, x| a.max(x.abs()));
    // normalise to −1 dBFS: a recording to replace the tune with is better at full level
    let g = if peak > 0.0 { 0.89 / peak } else { 1.0 };
    let frames: Vec<f32> = r.frames.iter().map(|x| x * g).collect();
    let w = add_smpl(wav::write_i16(r.rate, 2, &frames), r.rate, s, e);
    let msg = format!(
        "{:.1} s at {} Hz (normalised); loop {:.2}..{:.2} s: for an OGG use LOOPSTART={s} LOOPLENGTH={}",
        (frames.len() / 2) as f64 / r.rate as f64,
        r.rate,
        s as f64 / r.rate as f64,
        e as f64 / r.rate as f64,
        e - s
    );
    (w, msg)
}

/// Convert a Standard MIDI file to XMIDI: (XMIDI bytes, report line).
pub fn midi_to_xmi(mid: &[u8], lp: smf::Loop) -> Result<(Vec<u8>, String), String> {
    let (seq, r) = smf::to_xmidi(mid, lp)?;
    let mut msg = format!("{} track{}, {} notes, {:.1} s", r.tracks, if r.tracks == 1 { "" } else { "s" }, r.notes, r.seconds);
    match r.loop_secs {
        Some((s, e)) => msg += &format!(", loop {s:.2}..{e:.2} s ({})", r.loop_source),
        None => msg += ", no loop",
    }
    if r.unterminated > 0 {
        msg += &format!(", {} notes without note-off", r.unterminated);
    }
    if !r.programs.is_empty() {
        msg += &format!(", programs {:?}", r.programs);
    }
    Ok((xmidi::write(&[seq]), msg))
}

/// Convert an XMIDI tune to a Standard MIDI file (its first sequence).
pub fn xmi_to_midi(xmi: &[u8]) -> Result<Vec<u8>, String> {
    let seq = xmidi::parse(xmi)?.swap_remove(0);
    Ok(smf::write(&seq, smf::natural_ppq(&seq), &[]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &[u8] = include_bytes!("../../../c8-audio/tests/data/loop-test.ogg");

    #[test]
    fn import_edit_save_remove() {
        let dir = std::env::temp_dir().join(format!("hiedit-ogg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("SOUND")).unwrap();
        let dat = dir.join("SOUND/MUSIC.DAT");
        let src = dir.join("in.ogg");
        std::fs::write(&src, FIXTURE).unwrap();
        let mut t = Tunes::scan(&dat);
        t.import(2, &src).unwrap();
        assert!(t.dirty);
        assert_eq!(t.get(2).unwrap().region(), (2205, 8820));
        t.get_mut(2).unwrap().lp = Some((100, 9000));
        let files = t.game_files().unwrap();
        assert_eq!(files[0].0, "SOUND/MUSIC/TGAME4.OGG");
        t.save(&dat).unwrap();
        assert!(!t.dirty);
        let written = dir.join("SOUND/MUSIC/TGAME4.OGG");
        let info = ogg::probe(&std::fs::read(&written).unwrap()).unwrap();
        assert_eq!((info.loop_start, info.loop_end, info.tagged), (100, 9000, true));
        // a fresh scan finds it, with the saved loop
        let mut again = Tunes::scan(&dat);
        assert_eq!(again.get(2).unwrap().region(), (100, 9000));
        assert!(again.get(0).is_none());
        let mut p = again.get_mut(2).unwrap().player(44100).unwrap();
        let mut buf = vec![0f32; 2000];
        p.render(&mut buf, 1.0, true);
        assert!(buf.iter().any(|x| x.abs() > 0.1));
        let (w, _) = ogg_wav(again.get_mut(2).unwrap()).unwrap();
        assert_eq!(wav::parse("", &w).rate, 22050);
        // remove: deleted when saved
        again.remove(2);
        again.save(&dat).unwrap();
        assert!(!written.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
