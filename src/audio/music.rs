//! XMIDI → OPL3 driver: sequences an XMIDI tune at 120 Hz and allocates its notes to the 18
//! two-operator voices of the emulated OPL3 using the GTL timbre bank (`SAMPLE.OPL`).
use super::opl3::{self, Opl3};
use super::xmidi::{Ev, Sequence};
use std::collections::HashMap;

/// One GTL timbre (two-operator).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timbre {
    pub transpose: i8,
    /// Modulator registers 0x20, 0x40, 0x60, 0x80, 0xE0.
    pub modr: [u8; 5],
    /// Register 0xC0 (feedback / connection).
    pub fbc: u8,
    pub car: [u8; 5],
}

/// The Global Timbre Library keyed by (bank, patch).
#[derive(Clone, Debug, Default)]
pub struct TimbreBank {
    pub map: HashMap<(u8, u8), Timbre>,
}

impl TimbreBank {
    pub fn parse(d: &[u8]) -> Result<TimbreBank, String> {
        let mut map = HashMap::new();
        let mut p = 0;
        loop {
            if p + 2 > d.len() {
                return Err("GTL header not terminated".into());
            }
            let (patch, bank) = (d[p], d[p + 1]);
            if patch == 0xff && bank == 0xff {
                break;
            }
            if p + 6 > d.len() {
                return Err("GTL header truncated".into());
            }
            let off = u32::from_le_bytes([d[p + 2], d[p + 3], d[p + 4], d[p + 5]]) as usize;
            p += 6;
            if off + 14 > d.len() {
                continue;
            }
            let t = &d[off..off + 14];
            let len = u16::from_le_bytes([t[0], t[1]]);
            if len < 14 {
                continue;
            }
            map.insert(
                (bank, patch),
                Timbre {
                    transpose: t[2] as i8,
                    modr: [t[3], t[4], t[5], t[6], t[7]],
                    fbc: t[8],
                    car: [t[9], t[10], t[11], t[12], t[13]],
                },
            );
        }
        Ok(TimbreBank { map })
    }
}

#[derive(Clone, Copy)]
struct Channel {
    program: u8,
    bank: u8,
    volume: u8,
    expression: u8,
    pan: u8,
    bend: u16,
    sustain: bool,
}

impl Default for Channel {
    fn default() -> Self {
        Channel { program: 0, bank: 0, volume: 100, expression: 127, pan: 64, bend: 0x2000, sustain: false }
    }
}

#[derive(Clone, Copy, Default)]
struct Voice {
    keyed: bool,
    /// Duration ran out while the sustain pedal was down.
    held: bool,
    ch: u8,
    /// MIDI note as sent (identifies the note).
    note: u8,
    /// Note the OPL plays (after transpose / percussion mapping).
    play_note: i32,
    vel: u8,
    remaining: u32,
    timbre: Option<Timbre>,
    /// Allocation stamp (older = smaller).
    stamp: u64,
    block_fnum: u16,
}

pub struct MusicPlayer {
    opl: Opl3,
    rate: u32,
    bank: TimbreBank,
    seq: Sequence,
    pos: usize,
    wait: u32,
    loops: Vec<(usize, u8)>,
    channels: [Channel; 16],
    voices: [Voice; opl3::CHANNELS],
    stamp: u64,
    tick_acc: u32,
    stereo: bool,
    /// Loop the whole tune at its end (port deviation: race and intro tunes loop).
    pub looping: bool,
    finished: bool,
}

const MAX_EVENTS_PER_TICK: usize = 4096;

impl MusicPlayer {
    pub fn new(seq: Sequence, bank: TimbreBank, rate: u32) -> MusicPlayer {
        let mut opl = Opl3::new(rate);
        // Silence every operator and set both outputs on.
        for ch in 0..opl3::CHANNELS {
            let (b, c) = Opl3::chan_base(ch);
            let (m, k) = opl3::op_offsets(c);
            opl.write(b | (0x40 + m), 0x3f);
            opl.write(b | (0x40 + k), 0x3f);
            opl.write(b | (0xb0 + c as u16), 0);
            opl.write(b | (0xc0 + c as u16), 0x30);
        }
        let wait = seq.events.first().map(|e| e.delta).unwrap_or(0);
        MusicPlayer {
            opl,
            rate: rate.max(1),
            bank,
            seq,
            pos: 0,
            wait,
            loops: Vec::new(),
            channels: [Channel::default(); 16],
            voices: [Voice::default(); opl3::CHANNELS],
            stamp: 0,
            tick_acc: 0,
            stereo: true,
            looping: true,
            finished: false,
        }
    }

    pub fn finished(&self) -> bool {
        self.finished
    }

    /// Mix `out.len() / 2` stereo frames into `out` (interleaved L R), scaled by `gain`.
    pub fn render(&mut self, out: &mut [f32], gain: f32, stereo: bool) {
        if stereo != self.stereo {
            self.stereo = stereo;
            for v in 0..opl3::CHANNELS {
                if self.voices[v].timbre.is_some() {
                    self.write_pan(v);
                }
            }
        }
        for f in out.chunks_exact_mut(2) {
            self.tick_acc += 120;
            while self.tick_acc >= self.rate {
                self.tick_acc -= self.rate;
                self.tick();
            }
            let (l, r) = self.opl.frame();
            f[0] += l * gain;
            f[1] += r * gain;
        }
    }

    /// One 1/120 s interval.
    fn tick(&mut self) {
        // Note durations.
        for v in 0..opl3::CHANNELS {
            let voice = &mut self.voices[v];
            if voice.keyed && !voice.held && voice.remaining > 0 {
                voice.remaining -= 1;
                if voice.remaining == 0 {
                    if self.channels[voice.ch as usize].sustain {
                        voice.held = true;
                    } else {
                        self.key_off(v);
                    }
                }
            }
        }
        if self.finished || self.seq.events.is_empty() {
            return;
        }
        let mut guard = 0;
        while self.wait == 0 {
            guard += 1;
            if guard > MAX_EVENTS_PER_TICK {
                break;
            }
            let ev = self.seq.events[self.pos].ev;
            let mut next = self.pos + 1;
            match ev {
                Ev::End => {
                    self.all_notes_off();
                    self.loops.clear();
                    if self.looping {
                        next = 0;
                    } else {
                        self.finished = true;
                        return;
                    }
                }
                Ev::Ctrl { ctrl: 116, val, .. } => {
                    if self.loops.len() < 4 {
                        self.loops.push((self.pos + 1, val));
                    }
                }
                Ev::Ctrl { ctrl: 117, val, .. } => {
                    if val >= 64 {
                        if let Some(top) = self.loops.last_mut() {
                            if top.1 == 0 {
                                next = top.0;
                            } else {
                                top.1 -= 1;
                                if top.1 > 0 {
                                    next = top.0;
                                } else {
                                    self.loops.pop();
                                }
                            }
                        }
                    }
                }
                _ => self.exec(ev),
            }
            self.pos = next.min(self.seq.events.len() - 1);
            self.wait = self.seq.events[self.pos].delta;
        }
        if self.wait > 0 {
            self.wait -= 1;
        }
    }

    fn exec(&mut self, ev: Ev) {
        match ev {
            Ev::Note { ch, note, vel, dur } => {
                if vel > 0 {
                    self.note_on(ch, note, vel, dur.max(1));
                }
            }
            Ev::Program { ch, prog } => self.channels[ch as usize].program = prog,
            Ev::Bend { ch, val } => {
                self.channels[ch as usize].bend = val;
                for v in 0..opl3::CHANNELS {
                    if self.voices[v].keyed && self.voices[v].ch == ch {
                        self.write_freq(v, true);
                    }
                }
            }
            Ev::Ctrl { ch, ctrl, val } => {
                let c = &mut self.channels[ch as usize];
                match ctrl {
                    7 | 11 => {
                        if ctrl == 7 { c.volume = val } else { c.expression = val }
                        for v in 0..opl3::CHANNELS {
                            if self.voices[v].keyed && self.voices[v].ch == ch {
                                self.write_levels(v);
                            }
                        }
                    }
                    10 => {
                        c.pan = val;
                        for v in 0..opl3::CHANNELS {
                            if self.voices[v].keyed && self.voices[v].ch == ch {
                                self.write_pan(v);
                            }
                        }
                    }
                    64 => {
                        c.sustain = val >= 64;
                        if !c.sustain {
                            for v in 0..opl3::CHANNELS {
                                if self.voices[v].keyed && self.voices[v].held && self.voices[v].ch == ch {
                                    self.key_off(v);
                                }
                            }
                        }
                    }
                    114 => c.bank = val,
                    120 | 123 => {
                        for v in 0..opl3::CHANNELS {
                            if self.voices[v].keyed && self.voices[v].ch == ch {
                                self.key_off(v);
                            }
                        }
                    }
                    121 => {
                        c.volume = 100;
                        c.expression = 127;
                        c.pan = 64;
                        c.bend = 0x2000;
                        c.sustain = false;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn note_on(&mut self, ch: u8, note: u8, vel: u8, dur: u32) {
        let c = self.channels[ch as usize];
        let (timbre, play_note) = if ch == 9 {
            match self.bank.map.get(&(127, note)) {
                Some(t) => (*t, t.transpose as i32),
                None => return,
            }
        } else {
            let t = self
                .bank
                .map
                .get(&(c.bank, c.program))
                .or_else(|| self.bank.map.get(&(0, c.program)));
            match t {
                Some(t) => (*t, note as i32 + t.transpose as i32),
                None => return,
            }
        };
        // Same note retriggered on the same channel: reuse its voice.
        let v = (0..opl3::CHANNELS)
            .find(|&v| self.voices[v].keyed && self.voices[v].ch == ch && self.voices[v].note == note)
            .or_else(|| {
                // A free voice, preferring one that already holds this timbre, then the oldest.
                (0..opl3::CHANNELS)
                    .filter(|&v| !self.voices[v].keyed)
                    .min_by_key(|&v| (self.voices[v].timbre != Some(timbre), self.voices[v].stamp))
            })
            .unwrap_or_else(|| (0..opl3::CHANNELS).min_by_key(|&v| self.voices[v].stamp).unwrap());
        if self.voices[v].keyed {
            self.key_off(v);
        }
        self.stamp += 1;
        let reload = self.voices[v].timbre != Some(timbre);
        let voice = &mut self.voices[v];
        voice.keyed = true;
        voice.held = false;
        voice.ch = ch;
        voice.note = note;
        voice.play_note = play_note;
        voice.vel = vel;
        voice.remaining = dur;
        voice.stamp = self.stamp;
        voice.timbre = Some(timbre);
        if reload {
            self.write_timbre(v);
        }
        self.write_levels(v);
        self.write_pan(v);
        self.write_freq(v, true);
    }

    fn key_off(&mut self, v: usize) {
        let voice = &mut self.voices[v];
        voice.keyed = false;
        voice.held = false;
        let (b, c) = Opl3::chan_base(v);
        self.opl.write(b | (0xb0 + c as u16), (voice.block_fnum >> 8) as u8 & 0x1f);
    }

    fn all_notes_off(&mut self) {
        for v in 0..opl3::CHANNELS {
            if self.voices[v].keyed {
                self.key_off(v);
            }
        }
    }

    fn write_timbre(&mut self, v: usize) {
        let Some(t) = self.voices[v].timbre else { return };
        let (b, c) = Opl3::chan_base(v);
        let (m, k) = opl3::op_offsets(c);
        for (i, reg) in [0x20u16, 0x40, 0x60, 0x80, 0xe0].into_iter().enumerate() {
            self.opl.write(b | (reg + m), t.modr[i]);
            self.opl.write(b | (reg + k), t.car[i]);
        }
    }

    /// Total levels from velocity × channel volume × expression (AIL-style linear TL scaling).
    fn write_levels(&mut self, v: usize) {
        let voice = self.voices[v];
        let Some(t) = voice.timbre else { return };
        let c = self.channels[voice.ch as usize];
        let level = voice.vel as u32 * c.volume as u32 * c.expression as u32; // 0..127^3
        let scale = |reg40: u8| -> u8 {
            let tl = (reg40 & 0x3f) as u32;
            let loud = (63 - tl) * level / (127 * 127 * 127);
            (reg40 & 0xc0) | (63 - loud) as u8
        };
        let (b, c) = Opl3::chan_base(v);
        let (m, k) = opl3::op_offsets(c);
        self.opl.write(b | (0x40 + k), scale(t.car[1]));
        let modv = if t.fbc & 1 != 0 { scale(t.modr[1]) } else { t.modr[1] };
        self.opl.write(b | (0x40 + m), modv);
    }

    fn write_pan(&mut self, v: usize) {
        let voice = self.voices[v];
        let Some(t) = voice.timbre else { return };
        let pan = self.channels[voice.ch as usize].pan;
        let bits = if !self.stereo {
            0x30
        } else if pan < 27 {
            0x10
        } else if pan > 100 {
            0x20
        } else {
            0x30
        };
        let (b, c) = Opl3::chan_base(v);
        self.opl.write(b | (0xc0 + c as u16), (t.fbc & 0x0f) | bits);
    }

    fn write_freq(&mut self, v: usize, key: bool) {
        let voice = self.voices[v];
        let bend = self.channels[voice.ch as usize].bend as f64 - 8192.0;
        let semis = voice.play_note as f64 - 69.0 + bend / 8192.0 * 2.0;
        let freq = 440.0 * (semis / 12.0).exp2();
        let mut block = 0u16;
        let mut fnum = freq * (1u32 << 20) as f64 / opl3::NATIVE_RATE;
        while fnum >= 1023.5 && block < 7 {
            block += 1;
            fnum /= 2.0;
        }
        let fnum = (fnum.round() as u16).min(1023);
        let bf = (block << 10) | fnum;
        self.voices[v].block_fnum = bf;
        let (b, c) = Opl3::chan_base(v);
        self.opl.write(b | (0xa0 + c as u16), (bf & 0xff) as u8);
        self.opl.write(b | (0xb0 + c as u16), ((bf >> 8) as u8 & 0x1f) | if key { 0x20 } else { 0 });
    }
}
