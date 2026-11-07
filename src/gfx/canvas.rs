//! Indexed 2D drawing surface in virtual units (spec 00 §3; 640x480 mode only).
//!
//! Pixel encoding (u16): 0 = transparent (layer below shows through),
//! `OPAQUE | idx` = palette index, `TRANSLUCENT | idx` = blended over the 3D view (approximation of
//! the blend table where the destination is not known on the CPU).
use crate::data::images::ColourTable;
use crate::data::sprites::{Font, Sprite};
use std::rc::Rc;

pub const OPAQUE: u16 = 0x100;
pub const TRANSLUCENT: u16 = 0x200;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Blit {
    Solid,
    /// blend[src][dst]
    Blend,
    /// blend[dst][src]
    BlendRev,
}

pub struct Canvas {
    pub w: i32,
    pub h: i32,
    pub pix: Vec<u16>,
    /// x offset of the 640-wide virtual area (centred when the canvas is wider)
    pub x0: i32,
    pub table: Rc<ColourTable>,
}

impl Canvas {
    pub fn new(w: i32, h: i32, table: Rc<ColourTable>) -> Canvas {
        let w = w.max(640);
        Canvas { w, h, pix: vec![0; (w * h) as usize], x0: (w - 640) / 2, table }
    }
    pub fn resize(&mut self, w: i32, h: i32) {
        let w = w.max(640);
        if w != self.w || h != self.h {
            self.w = w;
            self.h = h;
            self.pix = vec![0; (w * h) as usize];
            self.x0 = (w - 640) / 2;
        }
    }
    pub fn clear(&mut self) {
        self.pix.fill(0);
    }
    pub fn clear_colour(&mut self, c: u8) {
        self.pix.fill(OPAQUE | c as u16);
    }
    /// Absolute pixel write with blending (coordinates are canvas pixels, not offset by x0).
    #[inline]
    pub fn put_abs(&mut self, x: i32, y: i32, c: u8, mode: Blit) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        let i = (y * self.w + x) as usize;
        let dst = self.pix[i];
        self.pix[i] = match mode {
            Blit::Solid => OPAQUE | c as u16,
            Blit::Blend | Blit::BlendRev => {
                if dst & OPAQUE != 0 {
                    let d = (dst & 0xff) as u8;
                    let v = if mode == Blit::Blend { self.table.blend(c, d) } else { self.table.blend(d, c) };
                    OPAQUE | v as u16
                } else {
                    TRANSLUCENT | c as u16
                }
            }
        };
    }
    /// Write in 640-virtual coordinates (offset by x0).
    #[inline]
    pub fn put(&mut self, x: i32, y: i32, c: u8, mode: Blit) {
        self.put_abs(x + self.x0, y, c, mode);
    }
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u8, mode: Blit) {
        for yy in y.max(0)..(y + h).min(self.h) {
            for xx in x..x + w {
                self.put(xx, yy, c, mode);
            }
        }
    }
    /// Raw full-screen image blit (e.g. backdrop), scaled by integer factor, at virtual (x, y).
    pub fn blit_image(&mut self, pix: &[u8], w: i32, h: i32, x: i32, y: i32, scale: i32) {
        for yy in 0..h * scale {
            let sy = yy / scale;
            for xx in 0..w * scale {
                let c = pix[(sy * w + xx / scale) as usize];
                self.put(x + xx, y + yy, c, Blit::Solid);
            }
        }
    }
    /// Sprite blit at virtual (x, y).
    pub fn sprite(&mut self, s: &Sprite, x: i32, y: i32, mode: Blit) {
        for sy in 0..s.h as i32 {
            for sx in 0..s.w as i32 {
                if let Some(c) = s.get(sx as u32, sy as u32) {
                    self.put(x + sx, y + sy, c, mode);
                }
            }
        }
    }
    /// Sprite blit at absolute canvas coordinates (no x0 offset).
    pub fn sprite_abs(&mut self, s: &Sprite, x: i32, y: i32, mode: Blit) {
        let x0 = self.x0;
        self.x0 = 0;
        self.sprite(s, x, y, mode);
        self.x0 = x0;
    }
    /// Sprite blit that is skipped whole when it would leave the 640 virtual screen.
    pub fn sprite_clipped(&mut self, s: &Sprite, x: i32, y: i32, mode: Blit) {
        if x < 0 || y < 0 || x + s.w as i32 > 640 || y + s.h as i32 > self.h {
            return;
        }
        self.sprite(s, x, y, mode);
    }
    /// Mask glyph: pixel 1 -> colour, pixel 0 -> outline (if < 256).
    pub fn mask_sprite(&mut self, s: &Sprite, x: i32, y: i32, colour: u8, outline: Option<u8>, mode: Blit) {
        for sy in 0..s.h as i32 {
            for sx in 0..s.w as i32 {
                if let Some(p) = s.get(sx as u32, sy as u32) {
                    if p != 0 {
                        self.put(x + sx, y + sy, colour, mode);
                    } else if let Some(o) = outline {
                        self.put(x + sx, y + sy, o, mode);
                    }
                }
            }
        }
    }
    /// Width of a string drawn with `spacing` extra advance per glyph.
    pub fn text_width(font: &Font, s: &[u8], spacing: i32) -> i32 {
        s.iter().map(|&c| font.glyph(c).w as i32 + spacing).sum()
    }
    /// Simple text: colour fonts draw their own colours; mask fonts use colour/outline.
    pub fn text(&mut self, font: &Font, s: &[u8], x: i32, y: i32, spacing: i32, colour: u8, outline: Option<u8>) -> i32 {
        let mut cx = x;
        for &c in s {
            let g = font.glyph(c);
            if font.mask_glyphs {
                self.mask_sprite(g, cx, y, colour, outline, Blit::Solid);
            } else {
                self.sprite(g, cx, y, Blit::Solid);
            }
            cx += g.w as i32 + spacing;
        }
        cx
    }
    /// Centred text (x = (640 - width) / 2), glyphs crossing the screen edge skipped.
    pub fn text_centred(&mut self, font: &Font, s: &[u8], y: i32, spacing: i32, colour: u8, outline: Option<u8>) {
        let w = Self::text_width(font, s, spacing);
        let mut cx = (640 - w) >> 1;
        for &c in s {
            let g = font.glyph(c);
            if cx >= 0 && cx + g.w as i32 <= 640 {
                if font.mask_glyphs {
                    self.mask_sprite(g, cx, y, colour, outline, Blit::Solid);
                } else {
                    self.sprite(g, cx, y, Blit::Solid);
                }
            }
            cx += g.w as i32 + spacing;
        }
    }
}
