//! RLE sprite sheets and fonts: NAME.DAT + NAME.TAB (spec 01 §3).
use super::files::{load, read_raw, u32le};

#[derive(Clone, Default)]
pub struct Sprite {
    pub w: u32,
    pub h: u32,
    /// palette indices, row-major; `None` pixel = transparent encoded as u16 0xFFFF? -> we use separate mask
    pub pix: Vec<u8>,
    pub mask: Vec<bool>,
}

impl Sprite {
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
    pub fn get(&self, i: usize) -> &Sprite {
        static EMPTY: Sprite = Sprite { w: 0, h: 0, pix: Vec::new(), mask: Vec::new() };
        self.sprites.get(i).unwrap_or(&EMPTY)
    }
    pub fn len(&self) -> usize {
        self.sprites.len()
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
