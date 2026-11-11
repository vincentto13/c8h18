//! Tunes by number, and the player for either kind (spec 09 §5, port deviation §9).
//!
//! A tune is the XMIDI item of MUSIC.DAT (slot 2) played on the emulated OPL3, unless an OGG
//! Vorbis file named after the tune exists: `SOUND/MUSIC/<NAME>.OGG` (e.g.
//! `SOUND/MUSIC/TGAME1.OGG`), looked up like every data file (override folder first, any
//! letter case). Then the OGG plays instead, looping by its `LOOPSTART`/`LOOPLENGTH` comments
//! (see [`crate::ogg`]).
use crate::music::{MusicPlayer, TimbreBank};
use crate::ogg::{OGG_GAIN, OggPlayer};
use crate::xmidi::{Ev, Sequence};
use std::path::PathBuf;

/// Tune names by tune number − 1 (MUSIC.DAT order).
pub const TUNE_NAMES: [&str; 5] = ["TGAME1", "TGAME2", "TGAME4", "TINTRO2", "TGAME3"];

/// The name of tune `tune` (1-based).
pub fn tune_name(tune: usize) -> Option<&'static str> {
    tune.checked_sub(1).and_then(|i| TUNE_NAMES.get(i)).copied()
}

/// Path of tune `tune`'s OGG replacement relative to the data folder, e.g. `SOUND/MUSIC/TGAME1.OGG`.
pub fn ogg_rel(tune: usize) -> Option<String> {
    tune_name(tune).map(|n| format!("SOUND/MUSIC/{n}.OGG"))
}

/// The OGG replacement of tune `tune`, if there is one (override folder first, then the data folder).
pub fn find_ogg(tune: usize) -> Option<PathBuf> {
    c8_data::files::resolve(&ogg_rel(tune)?).filter(|p| p.is_file())
}

/// A playing tune: FM (XMIDI on the OPL3) or OGG.
pub enum TunePlayer {
    Fm(Box<MusicPlayer>),
    Ogg(OggPlayer),
}

impl TunePlayer {
    /// Mix `out.len() / 2` stereo frames into `out` (interleaved L R), scaled by `gain`.
    pub fn render(&mut self, out: &mut [f32], gain: f32, stereo: bool) {
        match self {
            TunePlayer::Fm(p) => p.render(out, gain, stereo),
            TunePlayer::Ogg(p) => p.render(out, gain * OGG_GAIN, stereo),
        }
    }

    pub fn finished(&self) -> bool {
        match self {
            TunePlayer::Fm(p) => p.finished(),
            TunePlayer::Ogg(p) => p.finished(),
        }
    }

    /// Loop the tune (FM: restart at the end; OGG: repeat the loop region). On by default.
    pub fn set_looping(&mut self, on: bool) {
        match self {
            TunePlayer::Fm(p) => p.looping = on,
            TunePlayer::Ogg(p) => p.looping = on,
        }
    }

    pub fn is_ogg(&self) -> bool {
        matches!(self, TunePlayer::Ogg(_))
    }

    /// Render offline: at most `max_secs`, stopping earlier when a non-looping tune ends.
    /// Interleaved stereo at `rate` (the rate the player was made for).
    pub fn render_offline(&mut self, rate: u32, max_secs: f64) -> Vec<f32> {
        let total = (rate as f64 * max_secs) as usize * 2;
        let mut out = Vec::with_capacity(total.min(rate as usize * 2 * 600));
        let mut buf = vec![0f32; 4096];
        while !self.finished() && out.len() < total {
            buf.fill(0.0);
            self.render(&mut buf, 1.0, true);
            let n = buf.len().min(total - out.len());
            out.extend_from_slice(&buf[..n]);
        }
        out
    }
}

impl From<MusicPlayer> for TunePlayer {
    fn from(p: MusicPlayer) -> Self {
        TunePlayer::Fm(Box::new(p))
    }
}

impl From<OggPlayer> for TunePlayer {
    fn from(p: OggPlayer) -> Self {
        TunePlayer::Ogg(p)
    }
}

/// The first FOR (controller 116) and its NEXT (117, value ≥ 64), in intervals from the start.
pub fn xmidi_loop(seq: &Sequence) -> Option<(u64, u64)> {
    let mut t = 0u64;
    let mut start = None;
    let mut depth = 0;
    for e in &seq.events {
        t += e.delta as u64;
        match e.ev {
            Ev::Ctrl { ctrl: 116, .. } => {
                if start.is_none() {
                    start = Some(t);
                } else {
                    depth += 1;
                }
            }
            Ev::Ctrl { ctrl: 117, val, .. } if val >= 64 && start.is_some() => {
                if depth == 0 {
                    return start.map(|s| (s, t)).filter(|(s, e)| e > s);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

/// An offline rendering of a tune with its loop, ready to become a looping recording.
pub struct Rendered {
    pub rate: u32,
    /// Interleaved stereo.
    pub frames: Vec<f32>,
    /// Loop region in frames [start, end): playing on from `end` sounds exactly like playing on
    /// from `start`.
    pub loop_frames: (u64, u64),
}

/// Render an XMIDI tune for a looping recording. With a FOR/NEXT loop: the part before the
/// loop and the loop twice; the loop region is the second pass, which starts with the tails of
/// the loop's own end ringing, so the jump back is seamless (the last 10 ms of the region are
/// cross-faded into what precedes its start, which also hides the oscillator phases). Without one: one pass to the end
/// (the port restarts such a tune from silence), at most `max_secs`. Use a rate that is a
/// multiple of 120 (e.g. 48000) for loop points exact to the sample.
pub fn render_xmidi(seq: Sequence, bank: TimbreBank, rate: u32, max_secs: f64) -> Rendered {
    let lp = xmidi_loop(&seq);
    let mut p = MusicPlayer::new(seq, bank, rate);
    let fr = |iv: u64| -> u64 { (iv as u128 * rate as u128 / 120) as u64 };
    match lp {
        Some((s, e)) => {
            let len = e - s;
            let total = fr(e + len).min((max_secs * rate as f64) as u64);
            let mut frames = vec![0f32; total as usize * 2];
            p.render(&mut frames, 1.0, true);
            let start = fr(e).min(total);
            let n = (rate as u64 / 100).min(start).min(total - start) as usize;
            let (a, b) = (start as usize, total as usize);
            for k in 0..n {
                let w = (k as f32 + 1.0) / (n as f32 + 1.0);
                for c in 0..2 {
                    let i = (b - n + k) * 2 + c;
                    frames[i] = frames[i] * (1.0 - w) + frames[(a - n + k) * 2 + c] * w;
                }
            }
            Rendered { rate, frames, loop_frames: (start, total) }
        }
        None => {
            p.looping = false;
            let mut t = TunePlayer::from(p);
            let frames = t.render_offline(rate, max_secs);
            let n = (frames.len() / 2) as u64;
            Rendered { rate, frames, loop_frames: (0, n) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ogg_replaces_xmidi_by_name() {
        assert_eq!(ogg_rel(1).as_deref(), Some("SOUND/MUSIC/TGAME1.OGG"));
        assert_eq!(ogg_rel(3).as_deref(), Some("SOUND/MUSIC/TGAME4.OGG"));
        assert_eq!(ogg_rel(4).as_deref(), Some("SOUND/MUSIC/TINTRO2.OGG"));
        assert_eq!(ogg_rel(0), None);
        assert_eq!(ogg_rel(6), None);
    }

    #[test]
    fn rendered_loop_is_seamless() {
        use crate::music::Timbre;
        use crate::xmidi::XEvent;
        // a two-note loop after a one-note intro; a timbre with a long release that rings over the jump
        let t = Timbre { transpose: 0, modr: [0x21, 0x20, 0xf2, 0x15, 0], fbc: 0x0a, car: [0x21, 0x00, 0xf2, 0x14, 0] };
        let mut bank = TimbreBank::default();
        bank.map.insert((0, 0), t);
        let n = |delta, note| XEvent { delta, ev: Ev::Note { ch: 0, note, vel: 120, dur: 30 } };
        let seq = Sequence {
            timbres: vec![],
            events: vec![
                n(0, 48),
                XEvent { delta: 60, ev: Ev::Ctrl { ch: 0, ctrl: 116, val: 0 } },
                n(0, 55),
                n(60, 60),
                XEvent { delta: 60, ev: Ev::Ctrl { ch: 0, ctrl: 117, val: 127 } },
                XEvent { delta: 0, ev: Ev::End },
            ],
        };
        assert_eq!(xmidi_loop(&seq), Some((60, 180)));
        let r = render_xmidi(seq, bank.clone(), 48000, 60.0);
        assert_eq!(r.loop_frames, (72000, 120000));
        assert_eq!(r.frames.len(), 120000 * 2);
        // the jump from the region's end to its start is no bigger than the largest step nearby
        let (a, b) = (r.loop_frames.0 as usize * 2, r.loop_frames.1 as usize * 2);
        let jump = (r.frames[b - 2] - r.frames[a]).abs();
        let biggest = r.frames[a..a + 960].windows(3).map(|w| (w[2] - w[0]).abs()).fold(0f32, f32::max);
        assert!(jump <= biggest * 1.2 + 1e-4, "jump {jump}, steps up to {biggest}");
        assert!(r.frames[a..b].iter().any(|x| x.abs() > 0.01), "silent");
    }

    /// The music made from scratch (assets/music, written by musicgen) is complete: five FM
    /// tunes in MUSIC.DAT slot 2 with FOR/NEXT loops, and a timbre bank holding every timbre
    /// they use, SAMPLE.AD = SAMPLE.OPL.
    #[test]
    fn new_music_assets_parse() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/music/SOUND");
        let (Ok(dat), Ok(opl)) = (std::fs::read(dir.join("MUSIC.DAT")), std::fs::read(dir.join("SAMPLE.OPL"))) else { return };
        assert_eq!(std::fs::read(dir.join("SAMPLE.AD")).unwrap(), opl);
        let bank = TimbreBank::parse(&opl).unwrap();
        assert!((0..128).all(|p| bank.map.contains_key(&(0, p))), "every General MIDI program");
        let items = crate::container::load_bank(&dat, crate::container::MUSIC_SLOTS, crate::container::MUSIC_SLOT, 0).unwrap();
        let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["TGAME1.XMI", "TGAME2.XMI", "TGAME4.XMI", "TINTRO2.XMI", "TGAME3.XMI"]);
        for it in &items {
            let seq = crate::xmidi::parse(&it.data).unwrap().swap_remove(0);
            let (a, b) = xmidi_loop(&seq).unwrap_or_else(|| panic!("{} has no loop", it.name));
            assert_eq!(b, seq.length(), "{}: the loop ends at the end", it.name);
            assert!(a < b);
            for (p, bk) in &seq.timbres {
                assert!(bank.map.contains_key(&(*bk, *p)), "{}: timbre ({p}, {bk})", it.name);
            }
        }
    }
}
