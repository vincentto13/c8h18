//! Intro movie player (spec chapters 10, 01 §13). PUBLIC API IS FIXED.
use crate::audio::{Audio, MODE_ANY, MODE_ONCE};

pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 200;
/// Frames shown (the ring frame after them never is).
pub const FRAMES: usize = 548;

/// The sound script: (frame, op, argument), in table order (spec 10 §6; 46 records).
pub const SCRIPT: [(u16, u8, i32); 46] = [
    (0, b'A', 8), (0, b'E', 1),
    (1, b'R', 8),
    (13, b'M', 4),
    (50, b'S', 11),
    (51, b'S', 6),
    (64, b'S', 11),
    (96, b'T', 8), (96, b'R', 13),
    (104, b'S', 2),
    (107, b'S', 1),
    (113, b'T', 2), (113, b'S', 2),
    (127, b'T', 2),
    (129, b'T', 1), (129, b'S', 3),
    (138, b'T', 3), (138, b'S', 9),
    (140, b'T', 13), (140, b'R', 14),
    (141, b'S', 5),
    (162, b'T', 14), (162, b'R', 13),
    (170, b'S', 7),
    (174, b'T', 7), (174, b'S', 4),
    (192, b'T', 13),
    (205, b'S', 11),
    (234, b'R', 13),
    (257, b'S', 3),
    (270, b'T', 3), (270, b'S', 9),
    (275, b'S', 11),
    (277, b'T', 13),
    (300, b'S', 12),
    (330, b'T', 12),
    (349, b'S', 4),
    (355, b'S', 11),
    (404, b'R', 8),
    (460, b'S', 3),
    (470, b'T', 3), (470, b'S', 9),
    (477, b'S', 11),
    (480, b'T', 8),
    (532, b'X', 0), (532, b'S', 10),
];

/// Owner id used for the intro's samples.
const OWNER: u32 = 0;

/// Headerless FLC stream decoder (spec 01 §13).
pub struct Flc {
    data: Vec<u8>,
    /// Byte offsets of the frame chunks (0xF1FA), in order.
    frames: Vec<usize>,
    pub width: usize,
    pub height: usize,
    pub frame_count: usize,
}

/// What a decoded frame carried.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameInfo {
    pub palette: bool,
    pub picture: bool,
}

fn u16le(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([d[o], d[o + 1]])
}
fn u32le(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

impl Flc {
    pub fn new(data: Vec<u8>) -> Result<Flc, String> {
        if data.len() < 12 || u16le(&data, 4) != 0xaf12 {
            return Err("INTRO.DAT: missing 0xAF12 header chunk".into());
        }
        let frame_count = u16le(&data, 6) as usize;
        let width = u16le(&data, 8) as usize;
        let height = u16le(&data, 10) as usize;
        if width != WIDTH || height != HEIGHT {
            return Err(format!("INTRO.DAT: unexpected size {width}x{height}"));
        }
        let mut frames = Vec::new();
        let mut p = (u32le(&data, 0) as usize).max(12);
        while p + 6 <= data.len() {
            let size = u32le(&data, p) as usize;
            let ty = u16le(&data, p + 4);
            if ty != 0xf1fa || size < 16 || p + size > data.len() {
                break;
            }
            frames.push(p);
            p += size;
        }
        if frames.is_empty() {
            return Err("INTRO.DAT: no frames".into());
        }
        Ok(Flc { data, frames, width, height, frame_count })
    }

    /// Number of frame chunks present (including the ring frame).
    pub fn chunks(&self) -> usize {
        self.frames.len()
    }

    /// Decode frame chunk `n` (0-based) onto `pixels` / `palette`.
    pub fn decode(&self, n: usize, pixels: &mut [u8], palette: &mut [[u8; 3]; 256]) -> FrameInfo {
        let mut info = FrameInfo::default();
        let Some(&start) = self.frames.get(n) else { return info };
        let d = &self.data;
        let end = start + u32le(d, start) as usize;
        let count = u16le(d, start + 6) as usize;
        let mut p = start + 16;
        for _ in 0..count {
            if p + 6 > end {
                break;
            }
            let size = u32le(d, p) as usize;
            let ty = u16le(d, p + 4);
            let body_end = (p + size).min(end);
            if size < 6 {
                break;
            }
            let body = &d[p + 6..body_end];
            match ty {
                4 | 11 => {
                    decode_color(body, palette);
                    info.palette = true;
                }
                15 => {
                    self.decode_byte_run(body, pixels);
                    info.picture = true;
                }
                7 => {
                    self.decode_delta_flc(body, pixels);
                    info.picture = true;
                }
                12 => {
                    self.decode_delta_fli(body, pixels);
                    info.picture = true;
                }
                16 => {
                    let n = body.len().min(pixels.len());
                    pixels[..n].copy_from_slice(&body[..n]);
                    info.picture = true;
                }
                _ => {}
            }
            p += size;
        }
        info
    }

    fn decode_byte_run(&self, b: &[u8], px: &mut [u8]) {
        let w = self.width;
        let mut p = 0;
        for y in 0..self.height {
            if p >= b.len() {
                return;
            }
            p += 1; // old packet count
            let row = &mut px[y * w..y * w + w];
            let mut x = 0;
            while x < w && p < b.len() {
                let n = b[p] as i8;
                p += 1;
                if n >= 0 {
                    let v = b.get(p).copied().unwrap_or(0);
                    p += 1;
                    for _ in 0..n {
                        if x < w {
                            row[x] = v;
                            x += 1;
                        }
                    }
                } else {
                    for _ in 0..(-(n as i32)) {
                        if x < w && p < b.len() {
                            row[x] = b[p];
                            x += 1;
                        }
                        p += 1;
                    }
                }
            }
        }
    }

    fn decode_delta_flc(&self, b: &[u8], px: &mut [u8]) {
        let (w, h) = (self.width, self.height);
        if b.len() < 2 {
            return;
        }
        let mut lines = u16le(b, 0) as usize;
        let mut p = 2;
        let mut y = 0usize;
        while lines > 0 && y < h {
            // Opcode words until a packet count.
            let packets;
            loop {
                if p + 2 > b.len() {
                    return;
                }
                let word = u16le(b, p);
                p += 2;
                match word >> 14 {
                    0b11 => y += 65536 - word as usize,
                    0b10 => {
                        if y < h {
                            px[y * w + w - 1] = (word & 0xff) as u8;
                        }
                    }
                    0b00 => {
                        packets = word as usize;
                        break;
                    }
                    _ => {} // 01: undefined, skip
                }
            }
            if y >= h {
                return;
            }
            let row = y * w;
            let mut x = 0usize;
            for _ in 0..packets {
                if p + 2 > b.len() {
                    return;
                }
                x += b[p] as usize;
                let n = b[p + 1] as i8;
                p += 2;
                if n > 0 {
                    for _ in 0..n {
                        if p + 2 > b.len() {
                            return;
                        }
                        if x + 1 < w {
                            px[row + x] = b[p];
                            px[row + x + 1] = b[p + 1];
                        }
                        x += 2;
                        p += 2;
                    }
                } else if n < 0 {
                    if p + 2 > b.len() {
                        return;
                    }
                    let (c0, c1) = (b[p], b[p + 1]);
                    p += 2;
                    for _ in 0..(-(n as i32)) {
                        if x + 1 < w {
                            px[row + x] = c0;
                            px[row + x + 1] = c1;
                        }
                        x += 2;
                    }
                }
            }
            y += 1;
            lines -= 1;
        }
    }

    fn decode_delta_fli(&self, b: &[u8], px: &mut [u8]) {
        let (w, h) = (self.width, self.height);
        if b.len() < 4 {
            return;
        }
        let first = u16le(b, 0) as usize;
        let lines = u16le(b, 2) as usize;
        let mut p = 4;
        for y in first..(first + lines).min(h) {
            if p >= b.len() {
                return;
            }
            let packets = b[p];
            p += 1;
            let mut x = 0usize;
            for _ in 0..packets {
                if p + 2 > b.len() {
                    return;
                }
                x += b[p] as usize;
                let n = b[p + 1] as i8;
                p += 2;
                if n > 0 {
                    for _ in 0..n {
                        if x < w && p < b.len() {
                            px[y * w + x] = b[p];
                        }
                        x += 1;
                        p += 1;
                    }
                } else if n < 0 {
                    let v = b.get(p).copied().unwrap_or(0);
                    p += 1;
                    for _ in 0..(-(n as i32)) {
                        if x < w {
                            px[y * w + x] = v;
                        }
                        x += 1;
                    }
                }
            }
        }
    }
}

/// COLOR_256 / COLOR_64 body: values are 6-bit in this file and stored unchanged.
fn decode_color(b: &[u8], pal: &mut [[u8; 3]; 256]) {
    if b.len() < 2 {
        return;
    }
    let packets = u16le(b, 0) as usize;
    let mut p = 2;
    let mut idx = 0usize;
    for _ in 0..packets {
        if p + 2 > b.len() {
            return;
        }
        idx += b[p] as usize;
        let count = if b[p + 1] == 0 { 256 } else { b[p + 1] as usize };
        p += 2;
        for _ in 0..count {
            if p + 3 > b.len() {
                return;
            }
            if idx < 256 {
                pal[idx] = [b[p] & 0x3f, b[p + 1] & 0x3f, b[p + 2] & 0x3f];
            }
            idx += 1;
            p += 3;
        }
    }
}

pub struct Intro {
    pub pixels: Vec<u8>, // 320x200 indices
    pub palette: [[u8; 3]; 256], // 6-bit values
    done: bool,
    flc: Flc,
    /// Frame currently displayed (0 = not started, 1..=548).
    frame: usize,
    /// Ticks of the 120 Hz timer per frame wait (script op A).
    delay: u32,
    /// Seconds the current frame has been shown.
    elapsed: f64,
    skip_requested: bool,
}

impl Intro {
    /// Load DATA/INTRO.DAT. Err if the file is missing/corrupt (the game then skips the intro).
    pub fn new() -> Result<Intro, String> {
        let data = crate::data::files::read_raw("data/intro.dat")?;
        let flc = Flc::new(data)?;
        if flc.chunks() < FRAMES.min(flc.frame_count) {
            return Err("INTRO.DAT: truncated".into());
        }
        Ok(Intro {
            pixels: vec![0; WIDTH * HEIGHT],
            palette: [[0; 3]; 256],
            done: false,
            flc,
            frame: 0,
            delay: 21,
            elapsed: 0.0,
            skip_requested: false,
        })
    }

    /// Number of frames the movie shows.
    fn last_frame(&self) -> usize {
        self.flc.frame_count.min(FRAMES).min(self.flc.chunks())
    }

    /// Current frame number (1-based; 0 before the first `advance`).
    pub fn frame(&self) -> usize {
        self.frame
    }

    fn run_script(&mut self, frame: usize, audio: &mut Audio) {
        for &(f, op, arg) in SCRIPT.iter() {
            if f as usize != frame {
                continue;
            }
            match op.to_ascii_uppercase() {
                b'A' => self.delay = arg.max(0) as u32,
                b'E' => audio.load_bank(arg as usize),
                b'M' => audio.play_tune(arg as usize, true),
                b'R' => audio.play_full_volume(OWNER, arg as u16, true, MODE_ONCE),
                b'S' => {
                    if arg == 0 {
                        audio.stop_all();
                    } else {
                        audio.play_full_volume(OWNER, arg as u16, false, MODE_ANY);
                    }
                }
                b'T' => {
                    if arg == 0 {
                        audio.stop_all();
                    } else {
                        audio.stop(OWNER, arg as u16);
                    }
                }
                b'X' => audio.stop_music(),
                _ => {}
            }
        }
    }

    /// Go to the next frame: script, then decode. Frames with delay 0 are decoded and passed
    /// over without being shown. Returns false when the movie has run out.
    fn next_frame(&mut self, audio: &mut Audio) -> bool {
        loop {
            if self.frame >= self.last_frame() {
                return false;
            }
            self.frame += 1;
            self.run_script(self.frame, audio);
            self.flc.decode(self.frame - 1, &mut self.pixels, &mut self.palette);
            if self.delay != 0 {
                return true;
            }
        }
    }

    fn frame_time(&self) -> f64 {
        (self.delay as f64 + 1.0) / 120.0
    }

    /// Advance by wall-clock time `dt` seconds; runs the sound script through `audio`.
    /// Returns true when the movie has finished.
    pub fn advance(&mut self, dt: f64, audio: &mut Audio) -> bool {
        if self.done {
            return true;
        }
        if self.frame == 0 {
            self.run_script(0, audio);
            if !self.next_frame(audio) {
                self.done = true;
            }
            self.elapsed = 0.0;
            return self.done;
        }
        self.elapsed += dt.max(0.0);
        let t = self.frame_time();
        if self.elapsed >= t {
            // At most one frame per call: a stall does not fast-forward.
            self.elapsed = (self.elapsed - t).min(t * 0.999);
            if self.skip_requested || !self.next_frame(audio) {
                self.done = true;
            }
        }
        self.done
    }

    /// A key was pressed: end after the current frame.
    pub fn skip(&mut self) {
        self.skip_requested = true;
    }

    /// Called once when leaving the intro: stops samples and music.
    pub fn finish(&mut self, audio: &mut Audio) {
        audio.stop_all();
        audio.stop_music();
    }
}
