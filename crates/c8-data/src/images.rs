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
        data.resize(83968, 0);
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
