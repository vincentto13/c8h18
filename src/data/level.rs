//! Level file MAPS/LEVEL0-N.DAT (spec 01 §9).
use super::files::{i16le, load, u16le};

pub const MAP_W: usize = 256;
pub const MAP_H: usize = 160;
const CELLS: usize = 404620;

#[derive(Clone, Copy, Default, Debug)]
pub struct Cell {
    pub light: i16,
    /// 8.8 corner height
    pub height: u16,
    /// >=0 texture id, <0 = -column index
    pub tex: i16,
    pub orient: u8,
    pub raw: [u8; 12],
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Column {
    pub mask: u16,
    pub floor_tex: i16,
    pub blocks: [i16; 8],
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Block {
    /// N, E, S, W, top, bottom
    pub tex: [u8; 6],
    pub orient: [u8; 6],
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Entity {
    pub index: usize,
    pub kind: u8,
    pub sub: u8,
    pub group: u16,
    pub target_group: u16,
    pub link: u16,
    pub value: u16,
    /// cell coordinates (whole cells)
    pub x: i32,
    pub y: i32,
    pub w: u16,
    pub h: u16,
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Region {
    /// 1 shield, 2 fuel, 3 ammo, 4 start
    pub kind: u8,
    pub cx: f32,
    pub cy: f32,
    pub hw: f32,
    pub hh: f32,
}

#[derive(Clone)]
pub struct Level {
    pub cells: Vec<Cell>,
    pub columns: Vec<Column>,
    pub blocks: Vec<Block>,
    pub entities: Vec<Entity>,
    pub regions: Vec<Region>,
    pub tex_attr: [u16; 256],
    pub max_x: i32,
    pub max_y: i32,
}

fn fx88(d: &[u8], o: usize) -> f32 {
    u16le(d, o) as f32 / 256.0
}

impl Level {
    pub fn load(track: usize) -> Result<Level, String> {
        let d = load(&format!("maps/level0-{track}.dat"))?;
        if d.len() != 896140 {
            return Err(format!("level {track}: bad size {}", d.len()));
        }
        Ok(Self::parse(&d))
    }
    pub fn parse(d: &[u8]) -> Level {
        let mut cells = Vec::with_capacity(MAP_W * MAP_H);
        for i in 0..MAP_W * MAP_H {
            let o = CELLS + i * 12;
            let mut raw = [0u8; 12];
            raw.copy_from_slice(&d[o..o + 12]);
            cells.push(Cell { light: i16le(d, o), height: u16le(d, o + 2), tex: i16le(d, o + 4), orient: d[o + 10] >> 4, raw });
        }
        let mut columns = vec![Column::default(); 1024];
        for id in 1..1024 {
            let o = 98012 + id * 26;
            if d[o] == 0 && d[o + 1] == 0 {
                // "first byte 0 = empty" (mask low byte); keep high mask bits anyway
            }
            let mut blocks = [0i16; 8];
            for k in 0..8 {
                blocks[k] = i16le(d, o + 6 + 2 * k);
            }
            columns[id] = Column { mask: u16le(d, o), floor_tex: i16le(d, o + 2), blocks };
        }
        let mut blocks = vec![Block::default(); 1024];
        for id in 1..1024 {
            let o = 124636 + id * 16;
            let mut b = Block::default();
            b.tex.copy_from_slice(&d[o..o + 6]);
            b.orient.copy_from_slice(&d[o + 6..o + 12]);
            blocks[id] = b;
        }
        let mut entities = Vec::new();
        for i in 1..4000 {
            let o = i * 24;
            if d[o] == 0 {
                continue;
            }
            entities.push(Entity {
                index: i,
                kind: d[o],
                sub: d[o + 1],
                group: u16le(d, o + 2),
                target_group: u16le(d, o + 4),
                link: u16le(d, o + 12),
                value: u16le(d, o + 14),
                x: d[o + 17] as i32,
                y: d[o + 19] as i32,
                w: u16le(d, o + 20),
                h: u16le(d, o + 22),
            });
        }
        let mut regions = Vec::new();
        for r in (0..1000).rev() {
            let o = 162604 + r * 85;
            let k = d[o];
            if k == 0 {
                if r < 990 { break; }
                continue;
            }
            if (1..=4).contains(&k) {
                regions.push(Region { kind: k, cx: fx88(d, o + 24), cy: fx88(d, o + 26), hw: fx88(d, o + 45), hh: fx88(d, o + 47) });
            }
        }
        let mut tex_attr = [0u16; 256];
        for i in 0..256 {
            tex_attr[i] = u16le(d, 357668 + i * 2);
        }
        let mut lvl = Level { cells, columns, blocks, entities, regions, tex_attr, max_x: 0, max_y: 0 };
        for y in 0..MAP_H {
            for x in 0..MAP_W {
                let t = lvl.floor_tex(x as i32, y as i32);
                if lvl.tex_attr[t as usize & 255] & 4 != 0 {
                    lvl.max_x = lvl.max_x.max(x as i32 + 1);
                    lvl.max_y = lvl.max_y.max(y as i32 + 1);
                }
            }
        }
        lvl
    }
    #[inline]
    pub fn cell(&self, x: i32, y: i32) -> &Cell {
        let x = x.clamp(0, MAP_W as i32 - 1) as usize;
        let y = y.clamp(0, MAP_H as i32 - 1) as usize;
        &self.cells[y * MAP_W + x]
    }
    #[inline]
    pub fn cell_mut(&mut self, x: i32, y: i32) -> &mut Cell {
        let x = x.clamp(0, MAP_W as i32 - 1) as usize;
        let y = y.clamp(0, MAP_H as i32 - 1) as usize;
        &mut self.cells[y * MAP_W + x]
    }
    /// Corner height in cells.
    #[inline]
    pub fn corner_h(&self, x: i32, y: i32) -> f32 {
        self.cell(x, y).height as f32 / 256.0
    }
    pub fn column(&self, x: i32, y: i32) -> Option<&Column> {
        let t = self.cell(x, y).tex;
        if t < 0 {
            self.columns.get((-(t as i32)) as usize)
        } else {
            None
        }
    }
    /// Floor texture id of the cell.
    pub fn floor_tex(&self, x: i32, y: i32) -> u16 {
        let t = self.cell(x, y).tex;
        if t >= 0 {
            t as u16
        } else {
            self.column(x, y).map(|c| c.floor_tex.max(0) as u16).unwrap_or(0)
        }
    }
    pub fn attr(&self, x: i32, y: i32) -> u16 {
        self.tex_attr[(self.floor_tex(x, y) & 255) as usize]
    }
    /// Interpolated terrain surface height (columns ignored), in cells.
    pub fn terrain_height(&self, x: f32, y: f32) -> f32 {
        let ix = x.floor() as i32;
        let iy = y.floor() as i32;
        let fx = x - ix as f32;
        let fy = y - iy as f32;
        let h1 = self.corner_h(ix, iy);
        let h2 = self.corner_h(ix + 1, iy);
        let h3 = self.corner_h(ix + 1, iy + 1);
        let h4 = self.corner_h(ix, iy + 1);
        // match the triangle split: odd cells 0-2 diagonal, even 1-3
        if (ix + iy) & 1 != 0 {
            // diagonal from corner0 (0,0) to corner2 (1,1)
            if fx >= fy {
                h1 + (h2 - h1) * fx + (h3 - h2) * fy
            } else {
                h1 + (h3 - h4) * fx + (h4 - h1) * fy
            }
        } else {
            // diagonal corner1 (1,0) to corner3 (0,1)
            if fx + fy <= 1.0 {
                h1 + (h2 - h1) * fx + (h4 - h1) * fy
            } else {
                h3 + (h4 - h3) * (1.0 - fx) + (h2 - h3) * (1.0 - fy)
            }
        }
    }
    /// Block occupancy: is there a solid block at height z (cells) above cell (x,y)?
    /// Blocks span k..k+1 above the cell's interpolated surface.
    pub fn solid_at(&self, x: f32, y: f32, z: f32) -> bool {
        let ix = x.floor() as i32;
        let iy = y.floor() as i32;
        if ix < 0 || iy < 0 || ix >= MAP_W as i32 || iy >= MAP_H as i32 {
            return true;
        }
        let Some(col) = self.column(ix, iy) else { return false };
        let base = self.terrain_height(x, y);
        let rel = z - base;
        if rel < 0.0 {
            return false;
        }
        let k = rel.floor() as i32;
        k < 16 && col.mask & (1 << k) != 0
    }
    /// Surface below height z: top of the highest block below z, or the terrain.
    pub fn floor_below(&self, x: f32, y: f32, z: f32) -> f32 {
        let base = self.terrain_height(x, y);
        let ix = x.floor() as i32;
        let iy = y.floor() as i32;
        if let Some(col) = self.column(ix, iy) {
            let rel = z - base;
            let mut top = 0.0f32;
            for k in 0..16 {
                if col.mask & (1 << k) != 0 && (k as f32 + 1.0) <= rel + 0.001 {
                    top = k as f32 + 1.0;
                }
            }
            // if inside a block, its top counts too
            let kin = rel.floor() as i32;
            if rel >= 0.0 && kin < 16 && col.mask & (1 << kin) != 0 {
                top = top.max(kin as f32 + 1.0);
            }
            return base + top;
        }
        base
    }
    /// Map collision: inside a solid block or below the terrain.
    pub fn collide(&self, x: f32, y: f32, z: f32) -> bool {
        self.solid_at(x, y, z) || z < self.terrain_height(x, y)
    }
}
