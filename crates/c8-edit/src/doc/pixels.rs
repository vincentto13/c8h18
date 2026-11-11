//! Helpers shared by the pixel editors: palette colours, PNG import/export, colour matching,
//! code page 437 and flood fill. A frame is a [`Sprite`]: palette indices plus an opacity mask
//! (formats without transparency keep the mask all set).
use eframe::egui::{Color32, ColorImage};
use c8_data::images::Palette;
use c8_data::sprites::Sprite;
use std::collections::HashMap;
use std::path::Path;

pub type Frame = Sprite;
pub type Rgb = [[u8; 3]; 256];

/// 6-bit VGA value to 8 bits (spec 01 §2).
pub fn vga8(v: u8) -> u8 {
    (v << 2) | (v >> 4)
}

pub fn palette_rgb(p: &Palette) -> Rgb {
    p.vga.map(|c| c.map(vga8))
}

/// The game palette, or the built-in one (after a warning) if the package has none.
pub fn game_palette() -> Rgb {
    palette_rgb(&Palette::load_or_builtin())
}

/// Display colours of the mask fonts (OLFNT/OSFNT): 0 = outline, anything else = text (the game
/// draws every non-zero pixel in the text colour; a few original glyphs hold stray values).
pub fn mask_palette() -> Rgb {
    let mut p = [[255, 255, 255]; 256];
    p[0] = [0, 0, 0];
    p
}

/// A frame as an egui image; transparent pixels (and colour 0 when `zero_clear`) show as a
/// checkerboard.
pub fn frame_image(f: &Frame, pal: &Rgb, zero_clear: bool) -> ColorImage {
    let (w, h) = (f.w.max(1) as usize, f.h.max(1) as usize);
    let mut px = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let i = y * f.w as usize + x;
            let opaque = (i < f.mask.len() && f.mask[i]) && !(zero_clear && f.pix[i] == 0);
            px.push(if opaque {
                let c = pal[f.pix[i] as usize];
                Color32::from_rgb(c[0], c[1], c[2])
            } else if (x + y) & 1 == 0 {
                Color32::from_gray(96)
            } else {
                Color32::from_gray(120)
            });
        }
    }
    ColorImage::new([w, h], px)
}

/// RGBA bytes of a frame; transparent pixels get alpha 0.
pub fn frame_rgba(f: &Frame, pal: &Rgb, zero_clear: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(f.pix.len() * 4);
    for (i, &p) in f.pix.iter().enumerate() {
        let c = pal[p as usize];
        let a = if f.mask[i] && !(zero_clear && p == 0) { 255 } else { 0 };
        out.extend_from_slice(&[c[0], c[1], c[2], a]);
    }
    out
}

pub fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    let f = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w.max(1), h.max(1));
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().map_err(|e| e.to_string())?;
    let mut data = rgba.to_vec();
    data.resize((w.max(1) * h.max(1) * 4) as usize, 0);
    wr.write_image_data(&data).map_err(|e| e.to_string())
}

/// Read any PNG as (width, height, RGBA).
pub fn read_png(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(f));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut rd = dec.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; rd.output_buffer_size().ok_or("PNG too large")?];
    let info = rd.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width, info.height);
    let n = (w * h) as usize;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..n * 4].to_vec(),
        png::ColorType::Rgb => buf[..n * 3].chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf[..n * 2].chunks(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => buf[..n].iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("indexed PNG was not expanded".into()),
    };
    Ok((w, h, rgba))
}

/// Nearest palette colour, limited to the given indices.
pub struct Quantizer {
    pal: Rgb,
    allowed: Vec<u8>,
    cache: HashMap<[u8; 3], u8>,
}

impl Quantizer {
    pub fn new(pal: &Rgb, allowed: Vec<u8>) -> Quantizer {
        Quantizer { pal: *pal, allowed, cache: HashMap::new() }
    }
    pub fn index(&mut self, c: [u8; 3]) -> u8 {
        let (pal, allowed) = (&self.pal, &self.allowed);
        *self.cache.entry(c).or_insert_with(|| {
            let d = |i: u8| {
                let p = pal[i as usize];
                let (dr, dg, db) = (p[0] as i32 - c[0] as i32, p[1] as i32 - c[1] as i32, p[2] as i32 - c[2] as i32);
                // weighted for perceived brightness
                2 * dr * dr + 4 * dg * dg + 3 * db * db
            };
            allowed.iter().copied().min_by_key(|&i| d(i)).unwrap_or(0)
        })
    }
}

/// Copy an RGBA region into a frame at (0, 0): alpha < 128 becomes transparent when the frame
/// has transparency, otherwise keeps the old pixel.
pub fn paste_rgba(f: &mut Frame, src_w: u32, rgba: &[u8], sx: u32, sy: u32, transparency: bool, q: &mut Quantizer) {
    let src_h = (rgba.len() / 4) as u32 / src_w.max(1);
    for y in 0..f.h {
        for x in 0..f.w {
            let (px, py) = (sx + x, sy + y);
            if px >= src_w || py >= src_h {
                continue;
            }
            let o = ((py * src_w + px) * 4) as usize;
            let i = (y * f.w + x) as usize;
            if rgba[o + 3] < 128 {
                if transparency {
                    f.mask[i] = false;
                }
            } else {
                f.pix[i] = q.index([rgba[o], rgba[o + 1], rgba[o + 2]]);
                f.mask[i] = true;
            }
        }
    }
}

/// Resize keeping the top-left corner; new pixels are transparent (or `fill` when opaque).
pub fn resized(f: &Frame, w: u32, h: u32, transparency: bool, fill: u8) -> Frame {
    let mut n = Frame { w, h, pix: vec![fill; (w * h) as usize], mask: vec![!transparency; (w * h) as usize] };
    for y in 0..h.min(f.h) {
        for x in 0..w.min(f.w) {
            n.pix[(y * w + x) as usize] = f.pix[(y * f.w + x) as usize];
            n.mask[(y * w + x) as usize] = f.mask[(y * f.w + x) as usize];
        }
    }
    n
}

/// Bounding box (x, y, w, h) of the opaque pixels.
pub fn content_box(f: &Frame) -> Option<(u32, u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for y in 0..f.h {
        for x in 0..f.w {
            if f.mask[(y * f.w + x) as usize] {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            }
        }
    }
    (x0 != u32::MAX).then(|| (x0, y0, x1 - x0 + 1, y1 - y0 + 1))
}

pub fn crop(f: &Frame, x0: u32, y0: u32, w: u32, h: u32) -> Frame {
    let mut n = Frame::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let (s, d) = (((y0 + y) * f.w + x0 + x) as usize, (y * w + x) as usize);
            n.pix[d] = f.pix[s];
            n.mask[d] = f.mask[s];
        }
    }
    n
}

/// Pixel transform: the new frame is (nw, nh) and pixel (x, y) comes from `src(x, y)`.
pub fn transformed(f: &Frame, nw: u32, nh: u32, src: impl Fn(u32, u32) -> (u32, u32)) -> Frame {
    let mut n = Frame::new(nw, nh);
    for y in 0..nh {
        for x in 0..nw {
            let (sx, sy) = src(x, y);
            let (s, d) = ((sy * f.w + sx) as usize, (y * nw + x) as usize);
            n.pix[d] = f.pix[s];
            n.mask[d] = f.mask[s];
        }
    }
    n
}

/// Flood fill (4-connected) of the area with the same pixel as (x, y).
pub fn flood_fill(f: &mut Frame, x: u32, y: u32, value: Option<u8>) {
    let at = |f: &Frame, i: usize| if f.mask[i] { Some(f.pix[i]) } else { None };
    let start = (y * f.w + x) as usize;
    let old = at(f, start);
    if old == value {
        return;
    }
    let mut stack = vec![(x, y)];
    while let Some((x, y)) = stack.pop() {
        let i = (y * f.w + x) as usize;
        if at(f, i) != old {
            continue;
        }
        match value {
            Some(v) => (f.pix[i], f.mask[i]) = (v, true),
            None => f.mask[i] = false,
        }
        if x > 0 {
            stack.push((x - 1, y));
        }
        if y > 0 {
            stack.push((x, y - 1));
        }
        if x + 1 < f.w {
            stack.push((x + 1, y));
        }
        if y + 1 < f.h {
            stack.push((x, y + 1));
        }
    }
}

/// Points of a line, both ends included.
pub fn line(a: (i32, i32), b: (i32, i32)) -> Vec<(i32, i32)> {
    let (dx, dy) = ((b.0 - a.0).abs(), -(b.1 - a.1).abs());
    let (sx, sy) = ((b.0 - a.0).signum(), (b.1 - a.1).signum());
    let (mut x, mut y, mut err) = (a.0, a.1, dx + dy);
    let mut out = vec![(x, y)];
    while (x, y) != b {
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
        out.push((x, y));
    }
    out
}

const CP437_HIGH: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}";

/// The character of a code page 437 byte (control codes 0..31 have no printable form here).
pub fn cp437(c: u8) -> Option<char> {
    match c {
        0x20..=0x7E => Some(c as char),
        0x80..=0xFF => CP437_HIGH.chars().nth(c as usize - 0x80),
        _ => None,
    }
}

pub fn to_cp437(ch: char) -> Option<u8> {
    if (' '..='~').contains(&ch) {
        return Some(ch as u8);
    }
    CP437_HIGH.chars().position(|c| c == ch).map(|i| 0x80 + i as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cp437_table() {
        assert_eq!(CP437_HIGH.chars().count(), 128);
        assert_eq!(cp437(0x8E), Some('Ä'));
        assert_eq!(to_cp437('ü'), Some(0x81));
        assert_eq!(to_cp437('A'), Some(b'A'));
    }

    #[test]
    fn fill_and_line() {
        let mut f = Frame::new(4, 3);
        flood_fill(&mut f, 0, 0, Some(7));
        assert!(f.mask.iter().all(|&m| m) && f.pix.iter().all(|&p| p == 7));
        assert_eq!(line((0, 0), (3, 1)).len(), 4);
        let r = resized(&f, 5, 2, true, 0);
        assert_eq!((r.w, r.h, r.mask[4], r.mask[3]), (5, 2, false, true));
        assert_eq!(content_box(&r), Some((0, 0, 4, 2)));
    }
}
