//! Colour tables computed from a palette (port addition): what `DATA/TABLE0-N.DAT` holds
//! (spec 01 §5), for packages without them and for tools that write new ones.
use crate::images::{ColourTable, Nearest, Palette, SIZE};

/// 8-bit RGB to the palette's 6-bit VGA scale (rounded).
pub fn rgb_to_vga(c: [u8; 3]) -> [u8; 3] {
    c.map(|v| ((v as u32 * 63 + 127) / 255) as u8)
}

/// A whole colour table (83968 bytes, spec 01 §5) for `pal` and the haze colour `haze_rgb`
/// (8-bit RGB): the 64 brightness rows (row 32 the identity, rows below it fade towards the
/// haze, row 0 is the haze, rows above it darken; each entry the nearest palette colour), the
/// 256 × 256 blend table and the four flat-colour tables, all grey (the palette colour nearest
/// to mid grey). See [`build_with_textures`] for flat colours from terrain textures.
pub fn build(pal: &Palette, haze_rgb: [u8; 3]) -> Vec<u8> {
    build_with_textures(pal, haze_rgb, None)
}

/// [`build`], with the flat-colour tables set to each terrain texture's average colour when
/// `textures` (a TEXTU file, 256 textures of 64 × 64) is given.
pub fn build_with_textures(pal: &Palette, haze_rgb: [u8; 3], textures: Option<&[u8]>) -> Vec<u8> {
    let mut t = ColourTable { data: vec![0; SIZE] };
    t.set_light(&ColourTable::light_rows(pal, rgb_to_vga(haze_rgb)));
    t.set_blend(&ColourTable::blend_table(pal));
    match textures {
        Some(tex) => t.set_flat(&ColourTable::flat_colours(pal, tex)),
        None => {
            let grey = Nearest::new(pal).get([32, 32, 32]);
            t.data[81920..].fill(grey);
        }
    }
    t.data
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A palette of 256 distinct colours.
    fn distinct() -> Palette {
        Palette { vga: std::array::from_fn(|i| [(i % 8 * 9) as u8, (i / 8 % 8 * 9) as u8, (i / 64 * 21) as u8]) }
    }

    #[test]
    fn layout_and_identity_row() {
        let pal = distinct();
        let t = build(&pal, [255, 255, 255]);
        assert_eq!(t.len(), SIZE);
        for i in 0..256 {
            assert_eq!(t[32 * 256 + i] as usize, i, "row 32 is the identity");
            assert_eq!(t[16384 + i * 256 + i] as usize, i, "blending a colour with itself");
        }
        // row 0 is the haze (white: the brightest palette colour) for every colour
        let white = t[0];
        assert!(t[..256].iter().all(|&c| c == white));
        assert_eq!(pal.vga[white as usize], [63, 63, 63]);
        // row 63 darkens everything to (nearly) black
        assert!(t[63 * 256..64 * 256].iter().all(|&c| pal.vga[c as usize].iter().all(|&v| v <= 9)));
        // flat tables: one grey everywhere
        assert!(t[81920..].iter().all(|&c| c == t[81920]));
    }

    #[test]
    fn flat_colours_from_textures() {
        let pal = distinct();
        let mut tex = vec![0u8; 1 << 20];
        tex[4096..8192].fill(200);
        let t = build_with_textures(&pal, [0, 0, 0], Some(&tex));
        let ct = ColourTable::from_bytes(&t);
        for v in 0..4 {
            assert_eq!(ct.flat(v, 0), 0);
            assert_eq!(ct.flat(v, 1), 200);
        }
    }
}
