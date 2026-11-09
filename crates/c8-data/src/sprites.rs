//! RLE sprite sheets and fonts: NAME.DAT + NAME.TAB (spec 01 §3).
use super::files::{load, read_raw, u32le};

#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Sprite {
    pub w: u32,
    pub h: u32,
    /// palette indices, row-major; `mask` tells which pixels are opaque
    pub pix: Vec<u8>,
    pub mask: Vec<bool>,
}

impl Sprite {
    /// A fully transparent sprite.
    pub fn new(w: u32, h: u32) -> Sprite {
        Sprite { w, h, pix: vec![0; (w * h) as usize], mask: vec![false; (w * h) as usize] }
    }
    #[inline]
    pub fn get(&self, x: u32, y: u32) -> Option<u8> {
        let i = (y * self.w + x) as usize;
        if self.mask[i] { Some(self.pix[i]) } else { None }
    }
}

#[derive(Clone, Default)]
pub struct SpriteSheet {
    pub sprites: Vec<Sprite>,
}

impl SpriteSheet {
    /// Load `data/<name>.dat` + `.tab`.
    pub fn load(name: &str) -> Result<SpriteSheet, String> {
        let dat = load(&format!("data/{name}.dat"))?;
        let tab = read_raw(&format!("data/{name}.tab"))?;
        Ok(Self::decode(&dat, &tab))
    }
    pub fn decode(dat: &[u8], tab: &[u8]) -> SpriteSheet {
        let n = tab.len() / 6;
        let mut sprites = Vec::with_capacity(n);
        for i in 0..n {
            let off = u32le(tab, i * 6) as usize;
            let w = tab[i * 6 + 4] as u32;
            let h = tab[i * 6 + 5] as u32;
            let mut s = Sprite { w, h, pix: vec![0; (w * h) as usize], mask: vec![false; (w * h) as usize] };
            if w > 0 && h > 0 && off < dat.len() {
                let mut p = off;
                let (mut x, mut y) = (0u32, 0u32);
                while y < h && p < dat.len() {
                    let n = dat[p] as i8;
                    p += 1;
                    if n < 0 {
                        x += (-(n as i32)) as u32;
                    } else if n == 0 {
                        x = 0;
                        y += 1;
                    } else {
                        for _ in 0..n {
                            if p >= dat.len() { break; }
                            if x < w {
                                let idx = (y * w + x) as usize;
                                s.pix[idx] = dat[p];
                                s.mask[idx] = true;
                            }
                            p += 1;
                            x += 1;
                        }
                    }
                }
            }
            sprites.push(s);
        }
        SpriteSheet { sprites }
    }
    /// Encode as (DAT, TAB), unpacked (spec 01 §3). Sizes are clamped to 255. The RLE stream may
    /// differ from the original encoder's, but decodes to the same pixels.
    pub fn encode(&self) -> (Vec<u8>, Vec<u8>) {
        let n = self.sprites.len();
        let mut dat = (n as u16).to_le_bytes().to_vec();
        let mut tab = Vec::with_capacity(n * 6);
        for (i, s) in self.sprites.iter().enumerate() {
            let (w, h) = (s.w.min(255), s.h.min(255));
            if w == 0 || h == 0 {
                let off = if i == 0 { 0 } else { dat.len() as u32 };
                tab.extend_from_slice(&off.to_le_bytes());
                tab.extend_from_slice(&[w as u8, h as u8]);
                continue;
            }
            tab.extend_from_slice(&(dat.len() as u32).to_le_bytes());
            tab.extend_from_slice(&[w as u8, h as u8]);
            for y in 0..h {
                let at = |x: u32| s.get(x, y);
                // the last opaque pixel of the row; the rest needs no bytes
                let end = (0..w).rev().find(|&x| at(x).is_some()).map_or(0, |x| x + 1);
                let mut x = 0;
                while x < end {
                    if at(x).is_none() {
                        let mut k = 0;
                        while x + k < end && at(x + k).is_none() && k < 128 {
                            k += 1;
                        }
                        dat.push((-(k as i32)) as i8 as u8);
                        x += k;
                    } else {
                        let mut k = 0;
                        while x + k < end && k < 127 && at(x + k).is_some() {
                            k += 1;
                        }
                        dat.push(k as u8);
                        dat.extend((x..x + k).map(|x| at(x).unwrap_or(0)));
                        x += k;
                    }
                }
                dat.push(0);
            }
        }
        (dat, tab)
    }
    pub fn get(&self, i: usize) -> &Sprite {
        static EMPTY: Sprite = Sprite { w: 0, h: 0, pix: Vec::new(), mask: Vec::new() };
        self.sprites.get(i).unwrap_or(&EMPTY)
    }
}

/// A font: character c is entry c+1.
#[derive(Clone, Default)]
pub struct Font {
    pub sheet: SpriteSheet,
    /// glyph pixel values are masks (0/1) rather than colours
    pub mask_glyphs: bool,
}

impl Font {
    pub fn load(name: &str, mask_glyphs: bool) -> Result<Font, String> {
        Ok(Font { sheet: SpriteSheet::load(name)?, mask_glyphs })
    }
    pub fn glyph(&self, c: u8) -> &Sprite {
        self.sheet.get(c as usize + 1)
    }
    pub fn space_w(&self) -> u32 {
        self.glyph(b' ').w
    }
    pub fn space_h(&self) -> u32 {
        self.glyph(b' ').h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn same(a: &SpriteSheet, b: &SpriteSheet) -> bool {
        a.sprites.len() == b.sprites.len()
            && a.sprites.iter().zip(&b.sprites).all(|(a, b)| {
                a.w == b.w && a.h == b.h && a.mask == b.mask && a.pix.iter().zip(&a.mask).zip(&b.pix).all(|((p, m), q)| !m || p == q)
            })
    }

    #[test]
    fn sheets_round_trip() {
        crate::files::set_data_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources").into());
        let names = ["hfont0-0", "mfont0-0", "olfnt0-0", "olfnt0-1", "osfnt0-0", "osfnt0-1", "pfont0-0", "pfont0-1", "panel0-0", "panel0-1", "hspr0-0", "mspr0-0", "logo0-0", "logo0-1", "point0-0", "track0-0", "track0-1"];
        for name in names {
            let Ok(a) = SpriteSheet::load(name) else { continue };
            let (dat, tab) = a.encode();
            assert_eq!(tab.len(), a.sprites.len() * 6, "{name}");
            assert!(same(&a, &SpriteSheet::decode(&dat, &tab)), "{name} changed after encoding");
        }
    }
}
