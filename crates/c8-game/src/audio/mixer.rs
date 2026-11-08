//! Software sample mixer and the state shared with the audio callback.
use super::music::MusicPlayer;
use super::wav::Sample;
use std::sync::Arc;

pub const VOICES: usize = 16;
/// Gain of a full-volume sample voice (leaves headroom for several voices plus music).
pub const SAMPLE_GAIN: f32 = 0.5;

/// Linear up to 0.8, then a smooth knee towards 1.0.
#[inline]
fn soft_clip(x: f32) -> f32 {
    let a = x.abs();
    if a <= 0.8 { x } else { x.signum() * (0.8 + 0.2 * ((a - 0.8) / 0.2).tanh()) }
}

#[derive(Clone, Default)]
pub struct Voice {
    pub active: bool,
    pub owner: u32,
    pub sample: u16,
    pub data: Option<Arc<Sample>>,
    /// Position in source samples.
    pub pos: f64,
    pub volume: i32,
    pub pan: i32,
    pub rate: i32,
    pub looping: bool,
    /// Ignore the effects volume (intro).
    pub full: bool,
    pub stamp: u64,
}

impl Voice {
    pub fn matches(&self, owner: u32, sample: u16) -> bool {
        self.active && self.owner == owner && self.sample == sample
    }
}

pub struct Shared {
    pub out_rate: u32,
    pub voices: Vec<Voice>,
    pub bank: Vec<Arc<Sample>>,
    pub effects_volume: i32,
    pub music_volume: i32,
    pub stereo: bool,
    pub music: Option<MusicPlayer>,
    pub music_full: bool,
    pub stamp: u64,
}

impl Shared {
    pub fn new(out_rate: u32) -> Shared {
        Shared {
            out_rate: out_rate.max(1),
            voices: vec![Voice::default(); VOICES],
            bank: Vec::new(),
            effects_volume: 0x8000,
            music_volume: 0x8000,
            stereo: true,
            music: None,
            music_full: false,
            stamp: 0,
        }
    }

    pub fn find(&self, owner: u32, sample: u16) -> Option<usize> {
        self.voices.iter().position(|v| v.matches(owner, sample))
    }

    /// A finished voice, or failing that the oldest one-shot voice.
    pub fn free_voice(&self) -> Option<usize> {
        self.voices.iter().position(|v| !v.active).or_else(|| {
            (0..self.voices.len())
                .filter(|&i| !self.voices[i].looping)
                .min_by_key(|&i| self.voices[i].stamp)
        })
    }

    /// Mix into interleaved stereo `out` (overwrites it).
    pub fn mix(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let music_gain = if self.music_full {
            1.0
        } else {
            (((self.music_volume - 1).max(0) >> 8).min(127)) as f32 / 127.0
        };
        let stereo = self.stereo;
        if let Some(m) = self.music.as_mut() {
            m.render(out, music_gain, stereo);
            if m.finished() {
                self.music = None;
            }
        }
        let fx = (self.effects_volume.clamp(0, 0x8000)) as f32 / 32768.0;
        let out_rate = self.out_rate as f64;
        for v in self.voices.iter_mut() {
            if !v.active {
                continue;
            }
            let Some(s) = v.data.clone() else {
                v.active = false;
                continue;
            };
            let len = s.data.len();
            if len == 0 {
                v.active = false;
                continue;
            }
            if v.rate <= 0 {
                continue; // silent until a rate is set
            }
            let vol = (((v.volume - 1).max(0) >> 8).min(127)) as f32 / 127.0 * SAMPLE_GAIN * if v.full { 1.0 } else { fx };
            let p = if stereo { (v.pan.clamp(0, 0xffff) >> 9) as f32 } else { 64.0 };
            let gl = vol * ((127.0 - p) / 64.0).min(1.0);
            let gr = vol * (p / 64.0).min(1.0);
            let step = s.rate as f64 * v.rate as f64 / 100.0 / out_rate;
            let mut pos = v.pos;
            for f in out.chunks_exact_mut(2) {
                if pos >= len as f64 {
                    if v.looping {
                        pos %= len as f64;
                    } else {
                        v.active = false;
                        break;
                    }
                }
                let i = pos as usize;
                let fr = (pos - i as f64) as f32;
                let a = s.data[i];
                let b = if i + 1 < len { s.data[i + 1] } else if v.looping { s.data[0] } else { a };
                let x = a + (b - a) * fr;
                f[0] += x * gl;
                f[1] += x * gr;
                pos += step;
            }
            v.pos = pos;
            if !v.looping && v.pos >= len as f64 {
                v.active = false;
            }
        }
        for x in out.iter_mut() {
            *x = soft_clip(*x);
        }
    }
}
