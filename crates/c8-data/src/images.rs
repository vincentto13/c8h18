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
}

/// Search rings (spec 01 §10).
pub fn load_search_rings() -> Result<Vec<Vec<(i32, i32)>>, String> {
    let g = load("data/srch0-0.dat")?;
    let mut grid: Vec<i8> = g.iter().map(|&b| b as i8).collect();
    grid.resize(1024, -2);
    let dirs = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];
    let mut rings = Vec::new();
    let mut centre = (15i32, 15i32);
    for k in 0..16i32 {
        let Some(start) = grid.iter().position(|&b| b as i32 == k) else { break };
        let (mut cx, mut cy) = ((start % 32) as i32, (start / 32) as i32);
        if k == 0 {
            centre = (cx, cy);
        }
        let mut ring = vec![(cx - centre.0, cy - centre.1)];
        grid[start] = -1;
        let mut base = 0usize;
        'outer: loop {
            for d in [2usize, 0, 1, 3, 4, 5, 6, 7] {
                let (dx, dy) = dirs[(d + base) & 7];
                let (nx, ny) = (cx + dx, cy + dy);
                if nx < 0 || ny < 0 || nx >= 32 || ny >= 32 {
                    continue;
                }
                let idx = (ny * 32 + nx) as usize;
                let b = grid[idx];
                if b >= 0 && (b as i32 & 15) == k {
                    base = (base + d) & 6;
                    ring.push((nx - centre.0, ny - centre.1));
                    grid[idx] = -1;
                    cx = nx;
                    cy = ny;
                    continue 'outer;
                }
            }
            break;
        }
        rings.push(ring);
    }
    Ok(rings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn original(rel: &str) -> Option<Vec<u8>> {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources").join(rel);
        crate::rnc::maybe_unpack(std::fs::read(p).ok()?).ok()
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
