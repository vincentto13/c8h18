//! `$` text markup renderer (spec 02 §2.3) and the typewriter (§2.4).
use crate::assets::Assets;
use crate::audio::{Audio, MODE_ANY, MODE_RESTART};
use crate::gfx::canvas::{Blit, Canvas};

/// Extra advance per glyph in virtual units.
pub const SPACING: i32 = -6;

pub const S_REFUEL: u16 = 7;
pub const S_CHANGE: u16 = 20;
pub const S_SELECT: u16 = 21;
pub const S_PRINTTYP: u16 = 33;
pub const S_PRINTRET: u16 = 34;
pub const S_WINOPEN: u16 = 35;

/// Play a front-end sample: owner 0, centred, full volume (the mixer applies the effects volume).
pub fn sfx(audio: &mut Audio, sample: u16) {
    let mode = if sample == S_PRINTTYP || sample == S_PRINTRET { MODE_RESTART } else { MODE_ANY };
    audio.play(0, sample, 0x7fff, 0x8000, 100, false, mode);
}

/// String n of language `lang` in code page 437.
pub fn st(lang: usize, n: usize) -> Vec<u8> {
    crate::data::strings::to_cp437(crate::data::strings::get(lang, n))
}

/// Builder for markup strings (bytes in code page 437).
#[derive(Default, Clone)]
pub struct Mk(pub Vec<u8>);

impl Mk {
    pub fn new() -> Mk {
        Mk(Vec::new())
    }
    /// Append a code: `$` + letter + optional number + separator space.
    pub fn code(&mut self, c: char, n: Option<i32>) -> &mut Self {
        self.0.push(b'$');
        self.0.push(c as u8);
        if let Some(n) = n {
            self.0.extend_from_slice(n.to_string().as_bytes());
        }
        self.0.push(b' ');
        self
    }
    pub fn c(&mut self, c: char, n: i32) -> &mut Self {
        self.code(c, Some(n))
    }
    pub fn txt(&mut self, s: &[u8]) -> &mut Self {
        self.0.extend_from_slice(s);
        self
    }
    pub fn s(&mut self, s: &str) -> &mut Self {
        self.0.extend_from_slice(&crate::data::strings::to_cp437(s));
        self
    }
    /// Highlight end `$E` (no separator).
    pub fn end_hl(&mut self) -> &mut Self {
        self.0.extend_from_slice(b"$E");
        self
    }
}

enum Tok {
    Code(u8, Option<i32>),
    Glyph(u8),
}

fn tokens(s: &[u8]) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'$' && i + 1 < s.len() && s[i + 1].is_ascii_alphabetic() {
            let letter = s[i + 1].to_ascii_uppercase();
            i += 2;
            let mut j = i;
            if j < s.len() && s[j] == b'-' && j + 1 < s.len() && s[j + 1].is_ascii_digit() {
                j += 1;
            }
            while j < s.len() && s[j].is_ascii_digit() {
                j += 1;
            }
            let n = if j > i { std::str::from_utf8(&s[i..j]).ok().and_then(|t| t.parse().ok()) } else { None };
            i = j;
            // one separator character is swallowed
            if i < s.len() && s[i] == b' ' {
                i += 1;
            }
            out.push(Tok::Code(letter, n));
        } else {
            out.push(Tok::Glyph(s[i]));
            i += 1;
        }
    }
    out
}

/// Number of visible characters (codes excluded).
pub fn visible_len(s: &[u8]) -> usize {
    tokens(s).iter().filter(|t| matches!(t, Tok::Glyph(_))).count()
}

/// Width of plain text in a font slot (with the markup spacing).
pub fn text_w(assets: &Assets, font: usize, s: &[u8]) -> i32 {
    let f = &assets.fonts[font];
    s.iter().map(|&c| f.glyph(c).w as i32 + SPACING).sum()
}

/// Draw markup text, showing at most `limit` visible characters.
pub fn draw(c: &mut Canvas, assets: &Assets, s: &[u8], limit: usize) {
    let toks = tokens(s);
    let mut font = 2usize;
    let mut colour: u8 = 255;
    let mut hl: Option<u8> = None;
    let mut outline: i32 = 300;
    let mut mode = Blit::Solid;
    let mut just = 0;
    let mut need_just = false;
    let (mut x, mut y) = (0i32, 0i32);
    let mut shown = 0usize;
    for (ti, t) in toks.iter().enumerate() {
        match *t {
            Tok::Code(l, n) => {
                let v = n.unwrap_or(0);
                match l {
                    b'C' => colour = n.unwrap_or(-1) as u8,
                    b'F' => hl = Some(v as u8),
                    b'E' => hl = None,
                    b'O' => outline = n.unwrap_or(300),
                    b'G' => mode = match v { 1 => Blit::BlendRev, 2 => Blit::Blend, _ => Blit::Solid },
                    b'J' => {
                        just = v;
                        need_just = true;
                    }
                    b'X' => {
                        x = v;
                        need_just = true;
                    }
                    b'Y' => {
                        y = v;
                        need_just = true;
                    }
                    b'U' => font = (v.clamp(0, 4)) as usize,
                    _ => {}
                }
            }
            Tok::Glyph(g) => {
                if need_just && just != 0 {
                    // measure the run up to the next positioning code
                    let mut f2 = font;
                    let mut w = 0;
                    for t2 in &toks[ti..] {
                        match *t2 {
                            Tok::Code(b'X' | b'Y' | b'J', _) => break,
                            Tok::Code(b'U', n) => f2 = n.unwrap_or(0).clamp(0, 4) as usize,
                            Tok::Glyph(g2) => w += assets.fonts[f2].glyph(g2).w as i32 + SPACING,
                            _ => {}
                        }
                    }
                    x = if just == 1 { (640 - w) / 2 } else { 638 - w };
                }
                need_just = false;
                if shown >= limit {
                    return;
                }
                shown += 1;
                let f = &assets.fonts[font];
                let sp = f.glyph(g);
                if f.mask_glyphs {
                    let col = hl.unwrap_or(colour);
                    let o = if (0..256).contains(&outline) { Some(outline as u8) } else { None };
                    c.mask_sprite(sp, x, y, col, o, mode);
                } else {
                    c.sprite(sp, x, y, mode);
                }
                x += sp.w as i32 + SPACING;
            }
        }
    }
}

/// Typewriter counter (spec 02 §2.4).
#[derive(Default, Clone)]
pub struct Typer {
    pub n: usize,
}

impl Typer {
    pub fn reset(&mut self) {
        self.n = 0;
    }
    /// One frame: Space held shows everything at once.
    pub fn step(&mut self, len: usize, skip: bool, sound: bool, audio: &mut Audio) {
        if skip {
            self.n = self.n.max(len + 4);
            return;
        }
        if self.n <= len + 3 {
            self.n += 1;
            if sound {
                sfx(audio, S_PRINTTYP);
            }
        }
    }
}
