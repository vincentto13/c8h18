//! Raw images, palette, colour tables, textures, sky (spec 01 §2, §4, §5, §7).
use super::files::{load, read_raw, u32le};

#[derive(Clone)]
pub struct Palette {
    /// 6-bit VGA values
    pub vga: [[u8; 3]; 256],
}

impl Palette {
    pub fn load() -> Result<Palette, String> {
        let d = read_raw("data/palet0-0.dat")?;
        Ok(Self::from_bytes(&d))
    }
    /// Port addition: a built-in neutral palette for packages without `DATA/PALET0-0.DAT`:
    /// colour 0 black, 1..31 a grey ramp, then 14 hue ramps of 16 shades (dark to light, the
    /// lightest ones paler); colour 255 is white.
    pub fn builtin() -> Palette {
        let mut vga = [[0u8; 3]; 256];
        for (i, c) in vga.iter_mut().enumerate().take(32) {
            *c = [(i * 63 / 31) as u8; 3];
        }
        for r in 0..14 {
            let hue = r as f32 * 360.0 / 14.0;
            for s in 0..16 {
                let v = (s + 1) as f32 / 16.0;
                let sat = if s < 10 { 0.65 } else { 0.65 - (s - 9) as f32 * 0.08 };
                let (h6, f) = ((hue / 60.0).floor(), hue / 60.0 - (hue / 60.0).floor());
                let (p, q, t) = (v * (1.0 - sat), v * (1.0 - sat * f), v * (1.0 - sat * (1.0 - f)));
                let rgb = match h6 as i32 % 6 {
                    0 => [v, t, p],
                    1 => [q, v, p],
                    2 => [p, v, t],
                    3 => [p, q, v],
                    4 => [t, p, v],
                    _ => [v, p, q],
                };
                vga[32 + r * 16 + s] = rgb.map(|x| (x * 63.0).round().clamp(0.0, 63.0) as u8);
            }
        }
        vga[255] = [63, 63, 63];
        Palette { vga }
    }
    /// `DATA/PALET0-0.DAT`, or the [built-in palette](Self::builtin) after a warning.
    pub fn load_or_builtin() -> Palette {
        match crate::files::read_or_warn("data/palet0-0.dat", "using a built-in neutral palette") {
            Some(d) => Self::from_bytes(&d),
            None => Self::builtin(),
        }
    }
    pub fn from_bytes(d: &[u8]) -> Palette {
        let mut vga = [[0u8; 3]; 256];
        for i in 0..256 {
            for c in 0..3 {
                vga[i][c] = d.get(i * 3 + c).copied().unwrap_or(0) & 63;
            }
        }
        Palette { vga }
    }
    /// Colour cube (spec 08 §13).
    pub fn colour_cube(&self) -> Vec<u8> {
        let mut cube = vec![0u8; 512];
        for r in 0..8i32 {
            for g in 0..8i32 {
                for b in 0..8i32 {
                    let (tr, tg, tb) = (8 * r, 8 * g, 8 * b);
                    let mut best = (i32::MAX, i32::MAX, i32::MAX, 0usize);
                    for (i, c) in self.vga.iter().enumerate() {
                        let (cr, cg, cb) = (c[0] as i32, c[1] as i32, c[2] as i32);
                        let (dr, dg, db) = (cr - tr, cg - tg, cb - tb);
                        let key = (
                            dr * dr + dg * dg + db * db,
                            dr.abs() + dg.abs() + db.abs(),
                            2 * cr * cr + 2 * cg * cg + cb * cb,
                            i,
                        );
                        if key < best {
                            best = key;
                        }
                    }
                    cube[(r * 64 + g * 8 + b) as usize] = best.3 as u8;
                }
            }
        }
        cube
    }
}

/// A raw indexed image.
#[derive(Clone, Default)]
pub struct Image {
    #[allow(dead_code)] // kept to describe the data; not needed by the game yet
    pub w: u32,
    #[allow(dead_code)] // kept to describe the data; not needed by the game yet
    pub h: u32,
    pub pix: Vec<u8>,
}

impl Image {
    pub fn load_raw(rel: &str, w: u32, h: u32) -> Result<Image, String> {
        let mut pix = load(rel)?;
        pix.resize((w * h) as usize, 0);
        Ok(Image { w, h, pix })
    }
}

/// Colour table (spec 01 §5): 64 light rows, 256x256 blend, 4 flat tables.
#[derive(Clone)]
pub struct ColourTable {
    pub data: Vec<u8>,
}

impl ColourTable {
    pub fn load(n: usize) -> Result<ColourTable, String> {
        let mut data = load(&format!("data/table0-{n}.dat"))?;
        data.resize(SIZE, 0);
        Ok(ColourTable { data })
    }
    #[inline]
    pub fn blend(&self, a: u8, b: u8) -> u8 {
        self.data[16384 + a as usize * 256 + b as usize]
    }
    /// Flat colour table v (0..3), entry = texture id
    pub fn flat(&self, v: usize, tex: usize) -> u8 {
        self.data[81920 + v * 512 + tex.min(511)]
    }
    pub fn from_bytes(d: &[u8]) -> ColourTable {
        let mut data = d.to_vec();
        data.resize(SIZE, 0);
        ColourTable { data }
    }
    /// The haze colour: what light row 0 maps every colour to.
    pub fn haze(&self, pal: &Palette) -> [u8; 3] {
        pal.vga[self.data[0] as usize]
    }

    /// Light rows 0..63 for a palette (6-bit) and haze colour. Rows 0..31 fade a colour towards
    /// the haze by (32 − row)/32, row 32 is the identity, rows 33..63 darken it by (64 − row)/32,
    /// each with integer arithmetic and the nearest palette colour. This reproduces 96 % of the
    /// shipped tables' entries exactly; the rest are near ties.
    pub fn light_rows(pal: &Palette, haze: [u8; 3]) -> Vec<u8> {
        let mut near = Nearest::new(pal);
        let mut out = vec![0u8; 16384];
        for row in 0..64i32 {
            for i in 0..256 {
                let c = pal.vga[i].map(|v| v as i32);
                let t = if row < 32 {
                    std::array::from_fn(|k| c[k] + ((haze[k] as i32 - c[k]) * (32 - row)).div_euclid(32))
                } else if row == 32 {
                    out[row as usize * 256 + i] = i as u8;
                    continue;
                } else {
                    c.map(|v| v * (64 - row) / 32)
                };
                out[row as usize * 256 + i] = near.get(t);
            }
        }
        out
    }

    /// The blend table: `blend[a][b]` = nearest colour to (a + 2b) / 3 (95 % of the shipped
    /// entries exactly).
    pub fn blend_table(pal: &Palette) -> Vec<u8> {
        let mut near = Nearest::new(pal);
        let mut out = vec![0u8; 65536];
        for a in 0..256 {
            for b in 0..256 {
                let (ca, cb) = (pal.vga[a], pal.vga[b]);
                out[a * 256 + b] = if a == b { a as u8 } else { near.get(std::array::from_fn(|k| (ca[k] as i32 + 2 * cb[k] as i32) / 3)) };
            }
        }
        out
    }

    /// Flat colours (texture-mapping off) for a terrain texture set: each texture's average colour.
    /// The shipped tables were tuned by hand (four variants, close to the average); this gives
    /// one value per texture, for all variants.
    pub fn flat_colours(pal: &Palette, textures: &[u8]) -> Vec<u8> {
        let mut near = Nearest::new(pal);
        (0..256)
            .map(|t| {
                let px = textures.get(t * 4096..(t + 1) * 4096).unwrap_or(&[]);
                let mut s = [0u32; 3];
                for &p in px {
                    for k in 0..3 {
                        s[k] += pal.vga[p as usize][k] as u32;
                    }
                }
                let n = px.len().max(1) as u32;
                near.get(s.map(|v| (v / n) as i32))
            })
            .collect()
    }

    /// Replace parts of the table: light rows, blend table, flat colours (texture ids 0..255 of
    /// all four variants).
    pub fn set_light(&mut self, rows: &[u8]) {
        self.data[..16384].copy_from_slice(rows);
    }
    pub fn set_blend(&mut self, b: &[u8]) {
        self.data[16384..81920].copy_from_slice(b);
    }
    pub fn set_flat(&mut self, f: &[u8]) {
        for v in 0..4 {
            self.data[81920 + v * 512..81920 + v * 512 + 256].copy_from_slice(f);
        }
    }
}

/// Size of a colour table file, unpacked.
pub const SIZE: usize = 83968;

/// Nearest palette colour (squared distance, first index on ties), cached per 6-bit colour.
pub struct Nearest<'a> {
    pal: &'a Palette,
    cache: Vec<u16>,
}

impl<'a> Nearest<'a> {
    pub fn new(pal: &'a Palette) -> Nearest<'a> {
        Nearest { pal, cache: vec![u16::MAX; 1 << 18] }
    }
    pub fn get(&mut self, c: [i32; 3]) -> u8 {
        let c = c.map(|v| v.clamp(0, 63));
        let key = (c[0] << 12 | c[1] << 6 | c[2]) as usize;
        if self.cache[key] == u16::MAX {
            let mut best = (i32::MAX, 0);
            for (i, p) in self.pal.vga.iter().enumerate() {
                let d: i32 = (0..3).map(|k| (p[k] as i32 - c[k]).pow(2)).sum();
                if d < best.0 {
                    best = (d, i);
                }
            }
            self.cache[key] = best.1 as u16;
        }
        self.cache[key] as u8
    }
}

/// Terrain textures: 256 textures of 64x64 (spec 01 §4).
pub fn load_terrain_textures(n: usize) -> Result<Vec<u8>, String> {
    let mut d = load(&format!("data/textu0-{n}.dat"))?;
    d.resize(1048576, 0);
    Ok(d)
}

pub fn load_sky(n: usize) -> Result<Vec<u8>, String> {
    let mut d = load(&format!("data/sky0-{n}.dat"))?;
    d.resize(65536, 0);
    Ok(d)
}

/// Haze colour (8-bit RGB) of the colour tables computed for tracks without `TABLE0-N`, and the
/// horizon colour of the [`gradient_sky`] drawn for tracks without `SKY0-N`.
pub const FALLBACK_HAZE: [u8; 3] = [170, 180, 195];
/// Zenith colour of the fallback sky.
pub const FALLBACK_SKY_TOP: [u8; 3] = [60, 90, 150];

/// Port addition: placeholder terrain textures (a TEXTU file's 256 textures of 64 × 64) for
/// tracks without `TEXTU0-N`: each a grey tile with a darker grid every 16 pixels and a border,
/// its brightness varying mildly with the texture id so neighbouring kinds stay apart.
pub fn placeholder_textures(pal: &Palette) -> Vec<u8> {
    let mut near = Nearest::new(pal);
    let mut out = vec![0u8; 1048576];
    for t in 0..256usize {
        let h = (t as u32).wrapping_mul(2654435761) >> 24;
        let base = 26 + (h % 12) as i32;
        let tint = [(h >> 4 & 1) as i32, 0, (h >> 5 & 1) as i32];
        for y in 0..64usize {
            for x in 0..64usize {
                let edge = x == 0 || y == 0;
                let grid = x % 16 == 0 || y % 16 == 0;
                let noise = (((x * 7 + y * 13 + t * 5) ^ (x * y)) % 3) as i32 - 1;
                let v = if edge { base - 10 } else if grid { base - 5 } else { base + noise };
                out[t * 4096 + y * 64 + x] = near.get(std::array::from_fn(|k| v + tint[k]));
            }
        }
    }
    out
}

/// Port addition: a 256 × 256 sky for tracks without `SKY0-N`: a vertical gradient from `top`
/// (row 0) to `horizon` (row 255, just above the horizon), both 8-bit RGB, ordered-dithered to
/// the palette.
pub fn gradient_sky(pal: &Palette, top: [u8; 3], horizon: [u8; 3]) -> Vec<u8> {
    const BAYER: [[i32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    let mut near = Nearest::new(pal);
    let (a, b) = (crate::tables::rgb_to_vga(top), crate::tables::rgb_to_vga(horizon));
    let mut out = vec![0u8; 65536];
    for y in 0..256usize {
        for x in 0..256usize {
            let d = BAYER[y & 3][x & 3] - 8;
            let c = std::array::from_fn(|k| (a[k] as i32 * 16 * (255 - y as i32) + b[k] as i32 * 16 * y as i32) / 255 + d);
            out[y * 256 + x] = near.get(c.map(|v: i32| (v + 8).div_euclid(16)));
        }
    }
    out
}

/// Model texture atlas TEX0-0 (spec 01 §7).
pub struct TexAtlas {
    #[allow(dead_code)] // kept to describe the data; not needed by the game yet
    pub w: u32,
    pub h: u32,
    pub pix: Vec<u8>,
    /// (x, y, w, h)
    pub entries: Vec<(u32, u32, u32, u32)>,
}

impl TexAtlas {
    pub fn load() -> Result<TexAtlas, String> {
        let pix = read_raw("objects/data/tex0-0.dat")?;
        let tab = read_raw("objects/data/tex0-0.tab")?;
        let h = (pix.len() / 256) as u32;
        let mut entries = Vec::new();
        for i in 0..tab.len() / 6 {
            let o = u32le(&tab, i * 6);
            entries.push((o % 256, o / 256, tab[i * 6 + 4] as u32, tab[i * 6 + 5] as u32));
        }
        Ok(TexAtlas { w: 256, h, pix, entries })
    }
    /// Port addition: a 256 × 256 atlas of one mid grey with no entries, for packages without
    /// TEX0-0 (textured triangles then sample its corner).
    pub fn plain(pal: &Palette) -> TexAtlas {
        TexAtlas { w: 256, h: 256, pix: vec![Nearest::new(pal).get([36, 36, 36]); 65536], entries: Vec::new() }
    }
}

/// Search rings (spec 01 §10), as offsets from the centre in walk order; see [`crate::search`].
pub fn load_search_rings() -> Result<Vec<Vec<(i32, i32)>>, String> {
    Ok(crate::search::Rings::build(&load("data/srch0-0.dat")?).offsets())
}

/// [`load_search_rings`], or the rings of a generated grid ([`crate::search::generate`]) after a
/// warning.
pub fn load_search_rings_or_generate() -> Vec<Vec<(i32, i32)>> {
    let grid = crate::files::load_or_warn("data/srch0-0.dat", "using generated search rings").unwrap_or_else(crate::search::generate);
    crate::search::Rings::build(&grid).offsets()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn original(rel: &str) -> Option<Vec<u8>> {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources").join(rel);
        crate::rnc::maybe_unpack(std::fs::read(p).ok()?).ok()
    }

    #[test]
    fn fallbacks() {
        let pal = Palette::builtin();
        // distinct enough: black, white and a grey ramp
        assert_eq!(pal.vga[0], [0, 0, 0]);
        assert_eq!(pal.vga[255], [63, 63, 63]);
        let distinct: std::collections::HashSet<[u8; 3]> = pal.vga.iter().copied().collect();
        assert!(distinct.len() > 200, "{}", distinct.len());
        let tex = placeholder_textures(&pal);
        assert_eq!(tex.len(), 1 << 20);
        // textures differ from each other and are greyish
        assert_ne!(tex[..4096], tex[4096..8192]);
        for &p in &tex[..4096] {
            let c = pal.vga[p as usize];
            assert!(c.iter().max().unwrap() - c.iter().min().unwrap() <= 8, "{c:?}");
        }
        let sky = gradient_sky(&pal, FALLBACK_SKY_TOP, FALLBACK_HAZE);
        assert_eq!(sky.len(), 65536);
        let lum = |row: usize| sky[row * 256..row * 256 + 256].iter().map(|&p| pal.vga[p as usize].iter().map(|&v| v as u32).sum::<u32>()).sum::<u32>();
        assert!(lum(250) > lum(5), "brighter towards the horizon");
        assert!(!load_search_rings_or_generate().is_empty());
    }

    #[test]
    fn regenerated_tables_match_the_originals() {
        let (Some(pal), Some(t)) = (original("DATA/PALET0-0.DAT"), original("DATA/TABLE0-0.DAT")) else { return };
        let pal = Palette::from_bytes(&pal);
        let t = ColourTable::from_bytes(&t);
        let same = |a: &[u8], b: &[u8]| a.iter().zip(b).filter(|(x, y)| x == y).count() as f64 / a.len() as f64;
        let light = ColourTable::light_rows(&pal, t.haze(&pal));
        assert!(same(&light, &t.data[..16384]) > 0.95);
        let blend = ColourTable::blend_table(&pal);
        assert!(same(&blend, &t.data[16384..81920]) > 0.94);
    }
}
