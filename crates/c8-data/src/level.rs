//! Level file MAPS/LEVEL0-N.DAT (spec 01 §9): reading and writing.
//!
//! A level is a fixed-size snapshot with areas of unknown use. [`Level`] keeps the bytes it was
//! read from and [`Level::to_bytes`] writes only the fields it understands back into them, so a
//! level that is read and written unchanged is byte-identical.
use super::files::{i16le, load, u16le};
use std::path::Path;

pub const MAP_W: usize = 256;
pub const MAP_H: usize = 160;
/// Size of an unpacked level file.
pub const LEVEL_SIZE: usize = 896140;
const CELLS: usize = 404620;
const COLUMNS: usize = 98012;
const BLOCKS: usize = 124636;
const THINGS: usize = 162604;
const TEX_ATTR: usize = 357668;
/// Entity records (record 0 is never used).
pub const MAX_ENTITIES: usize = 4000;
/// Column and block definitions (record 0 is never used).
pub const MAX_COLUMNS: usize = 1024;
pub const MAX_BLOCKS: usize = 1024;
/// Region records are thing records counted down from the end of the thing table (spec 01 §9.5).
const THING_RECORDS: usize = 1000;
const REGION_SLOTS: usize = 10;

/// Texture corner coordinates (u, v as 0/1) of the floor orientations 0..7 (spec 08 §5.2), for the
/// quad corners (x, y), (x+1, y), (x+1, y+1), (x, y+1).
pub const FLOOR_UV: [[(u8, u8); 4]; 8] = [
    [(0, 0), (1, 0), (1, 1), (0, 1)],
    [(1, 0), (0, 0), (0, 1), (1, 1)],
    [(0, 1), (1, 1), (1, 0), (0, 0)],
    [(1, 1), (0, 1), (0, 0), (1, 0)],
    [(0, 0), (0, 1), (1, 1), (1, 0)],
    [(0, 1), (0, 0), (1, 0), (1, 1)],
    [(1, 0), (1, 1), (0, 1), (0, 0)],
    [(1, 1), (1, 0), (0, 0), (0, 1)],
];

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Cell {
    pub light: i16,
    /// 8.8 corner height
    pub height: u16,
    /// >=0 texture id, <0 = -column index
    pub tex: i16,
    pub orient: u8,
}

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Column {
    pub mask: u16,
    pub floor_tex: i16,
    pub blocks: [i16; 8],
}

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Block {
    /// N, E, S, W, top, bottom
    pub tex: [u8; 6],
    pub orient: [u8; 6],
}

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Entity {
    /// record number 1..3999; other entities refer to it through `link`
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

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Region {
    /// thing record number (999 downwards)
    pub slot: usize,
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
    /// the bytes the level was read from (unknown areas are written back from here)
    raw: Vec<u8>,
}

fn fx88(d: &[u8], o: usize) -> f32 {
    u16le(d, o) as f32 / 256.0
}

impl Level {
    /// Load original level N (1..6) from the data directory.
    pub fn load(track: usize) -> Result<Level, String> {
        let d = load(&format!("maps/level0-{track}.dat"))?;
        Self::from_bytes(&d).map_err(|e| format!("level {track}: {e}"))
    }
    /// Read a level file from any path (RNC-packed or not).
    pub fn read_file(path: &Path) -> Result<Level, String> {
        let d = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let d = super::rnc::maybe_unpack(d).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_bytes(&d).map_err(|e| format!("{}: {e}", path.display()))
    }
    /// Write the level unpacked (the game reads packed and unpacked levels).
    pub fn write_file(&self, path: &Path) -> Result<(), String> {
        let d = self.to_bytes()?;
        std::fs::write(path, d).map_err(|e| format!("{}: {e}", path.display()))
    }
    /// Parse unpacked level bytes.
    pub fn from_bytes(d: &[u8]) -> Result<Level, String> {
        if d.len() != LEVEL_SIZE {
            return Err(format!("bad size {} (expected {LEVEL_SIZE})", d.len()));
        }
        Ok(Self::parse(d))
    }
    /// Parse unpacked level bytes; `d` must be [`LEVEL_SIZE`] long.
    pub fn parse(d: &[u8]) -> Level {
        let mut cells = Vec::with_capacity(MAP_W * MAP_H);
        for i in 0..MAP_W * MAP_H {
            let o = CELLS + i * 12;
            cells.push(Cell { light: i16le(d, o), height: u16le(d, o + 2), tex: i16le(d, o + 4), orient: d[o + 10] >> 4 });
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
                regions.push(Region { slot: r, kind: k, cx: fx88(d, o + 24), cy: fx88(d, o + 26), hw: fx88(d, o + 45), hh: fx88(d, o + 47) });
            }
        }
        let mut tex_attr = [0u16; 256];
        for i in 0..256 {
            tex_attr[i] = u16le(d, 357668 + i * 2);
        }
        let mut lvl = Level { cells, columns, blocks, entities, regions, tex_attr, max_x: 0, max_y: 0, raw: d.to_vec() };
        lvl.update_extent();
        lvl
    }
    /// Recompute the track extent (cells with a "track" texture), as the game's loader does.
    pub fn update_extent(&mut self) {
        let (mut mx, mut my) = (0, 0);
        for y in 0..MAP_H {
            for x in 0..MAP_W {
                let t = self.floor_tex(x as i32, y as i32);
                if self.tex_attr[t as usize & 255] & 4 != 0 {
                    mx = mx.max(x as i32 + 1);
                    my = my.max(y as i32 + 1);
                }
            }
        }
        self.max_x = mx;
        self.max_y = my;
    }

    /// Serialize: the fields above written into the bytes the level was read from.
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        let mut d = self.raw.clone();
        let put16 = |d: &mut [u8], o: usize, v: u16| d[o..o + 2].copy_from_slice(&v.to_le_bytes());
        let fx = |v: f32| -> Result<u16, String> {
            let r = (v * 256.0).round();
            if (0.0..=65535.0).contains(&r) { Ok(r as u16) } else { Err(format!("value {v} out of range")) }
        };
        if self.cells.len() != MAP_W * MAP_H {
            return Err(format!("{} cells (expected {})", self.cells.len(), MAP_W * MAP_H));
        }
        // map cells
        for (i, c) in self.cells.iter().enumerate() {
            let o = CELLS + i * 12;
            put16(&mut d, o, c.light as u16);
            put16(&mut d, o + 2, c.height);
            put16(&mut d, o + 4, c.tex as u16);
            d[o + 10] = (c.orient & 15) << 4 | (d[o + 10] & 15);
            if c.tex < 0 && -(c.tex as i32) as usize >= self.columns.len().min(MAX_COLUMNS) {
                return Err(format!("cell {}, {}: column {} does not exist", i % MAP_W, i / MAP_W, -(c.tex as i32)));
            }
        }
        // columns and blocks (record 0 stays unused)
        if self.columns.len() > MAX_COLUMNS || self.blocks.len() > MAX_BLOCKS {
            return Err(format!("too many columns ({}) or blocks ({})", self.columns.len(), self.blocks.len()));
        }
        for (id, c) in self.columns.iter().enumerate().skip(1) {
            let o = COLUMNS + id * 26;
            put16(&mut d, o, c.mask);
            put16(&mut d, o + 2, c.floor_tex as u16);
            for k in 0..8 {
                put16(&mut d, o + 6 + 2 * k, c.blocks[k] as u16);
            }
        }
        for (id, b) in self.blocks.iter().enumerate().skip(1) {
            let o = BLOCKS + id * 16;
            d[o..o + 6].copy_from_slice(&b.tex);
            d[o + 6..o + 12].copy_from_slice(&b.orient);
        }
        // entities: clear records that no longer exist, then write every entity
        let mut used = vec![false; MAX_ENTITIES];
        for e in &self.entities {
            if e.index == 0 || e.index >= MAX_ENTITIES {
                return Err(format!("entity record {} out of range 1..{}", e.index, MAX_ENTITIES - 1));
            }
            if std::mem::replace(&mut used[e.index], true) {
                return Err(format!("entity record {} used twice", e.index));
            }
            if e.kind == 0 {
                return Err(format!("entity record {} has type 0", e.index));
            }
        }
        for i in 1..MAX_ENTITIES {
            let o = i * 24;
            if !used[i] && d[o] != 0 {
                d[o..o + 24].fill(0);
            }
        }
        for e in &self.entities {
            let o = e.index * 24;
            if self.raw[o] == 0 {
                // a new record: unknown fields as in the shipped levels
                d[o..o + 24].fill(0);
                put16(&mut d, o + 10, 1);
            }
            if !(0..MAP_W as i32).contains(&e.x) || !(0..MAP_H as i32).contains(&e.y) {
                return Err(format!("entity record {} at {}, {} is off the map", e.index, e.x, e.y));
            }
            d[o] = e.kind;
            d[o + 1] = e.sub;
            put16(&mut d, o + 2, e.group);
            put16(&mut d, o + 4, e.target_group);
            put16(&mut d, o + 12, e.link);
            put16(&mut d, o + 14, e.value);
            d[o + 17] = e.x as u8;
            d[o + 19] = e.y as u8;
            put16(&mut d, o + 20, e.w);
            put16(&mut d, o + 22, e.h);
        }
        // regions (thing records at the end of the thing table)
        let template = self.regions_template();
        let lowest = THING_RECORDS - REGION_SLOTS;
        let mut rused = vec![false; THING_RECORDS];
        for r in &self.regions {
            if r.slot < lowest || r.slot >= THING_RECORDS {
                return Err(format!("region record {} out of range {lowest}..{}", r.slot, THING_RECORDS - 1));
            }
            if std::mem::replace(&mut rused[r.slot], true) {
                return Err(format!("region record {} used twice", r.slot));
            }
        }
        for slot in lowest..THING_RECORDS {
            let o = THINGS + slot * 85;
            if !rused[slot] && (1..=4).contains(&d[o]) {
                d[o..o + 85].fill(0);
            }
        }
        for r in &self.regions {
            let o = THINGS + r.slot * 85;
            if !(1..=4).contains(&self.raw[o]) {
                let t = template.ok_or("no region record to copy for a new region")?;
                d.copy_within(t..t + 85, o);
                put16(&mut d, o + 12, 1000 + r.slot as u16);
                put16(&mut d, o + 16, r.slot as u16);
            }
            d[o] = r.kind;
            put16(&mut d, o + 24, fx(r.cx)?);
            put16(&mut d, o + 26, fx(r.cy)?);
            put16(&mut d, o + 45, fx(r.hw)?);
            put16(&mut d, o + 47, fx(r.hh)?);
        }
        // texture attributes
        for (i, a) in self.tex_attr.iter().enumerate() {
            put16(&mut d, TEX_ATTR + i * 2, *a);
        }
        Ok(d)
    }
    /// Offset of an original region record, the pattern for new ones.
    fn regions_template(&self) -> Option<usize> {
        (THING_RECORDS - REGION_SLOTS..THING_RECORDS).rev().map(|s| THINGS + s * 85).find(|&o| (1..=4).contains(&self.raw[o]))
    }
    /// A free entity record number for a new entity.
    pub fn free_entity_index(&self) -> Option<usize> {
        let mut used = vec![false; MAX_ENTITIES];
        for e in &self.entities {
            if e.index < MAX_ENTITIES {
                used[e.index] = true;
            }
        }
        (1..MAX_ENTITIES).find(|&i| !used[i])
    }
    /// A free region record for a new region (regions live in the last 10 thing records).
    pub fn free_region_slot(&self) -> Option<usize> {
        (THING_RECORDS - REGION_SLOTS..THING_RECORDS).rev().find(|&s| !self.regions.iter().any(|r| r.slot == s))
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
    /// Surface for the crafts' hover sensors (spec 05 §3.i, §3.l): the terrain or the top of the
    /// highest block below height z. Unlike [`floor_below`](Self::floor_below), a block the point
    /// is inside does not count, so a sensor pushed into a wall can't climb onto it.
    pub fn surface_below(&self, x: f32, y: f32, z: f32) -> f32 {
        let base = self.terrain_height(x, y);
        let Some(col) = self.column(x.floor() as i32, y.floor() as i32) else { return base };
        let rel = z - base;
        let top = (0..16).filter(|&k| col.mask & (1 << k) != 0 && (k as f32 + 1.0) <= rel + 0.001).map(|k| k as f32 + 1.0).fold(0.0, f32::max);
        base + top
    }
    /// Map collision: inside a solid block or below the terrain.
    pub fn collide(&self, x: f32, y: f32, z: f32) -> bool {
        self.solid_at(x, y, z) || z < self.terrain_height(x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() {
        crate::files::set_data_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources").into());
    }

    /// Reading and writing an original level unchanged gives the same bytes.
    #[test]
    fn round_trip_is_identical() {
        data();
        for track in 1..=6 {
            let raw = load(&format!("maps/level0-{track}.dat")).expect("level");
            let lvl = Level::from_bytes(&raw).expect("parse");
            let out = lvl.to_bytes().expect("write");
            assert!(out == raw, "level {track} differs after a round trip");
        }
    }

    /// Edits survive a write and a re-read.
    #[test]
    fn edits_survive() {
        data();
        let mut lvl = Level::load(1).expect("level");
        let c = lvl.cell_mut(10, 20);
        c.height = 0x1234;
        c.light = 40 << 8;
        c.orient = 5;
        lvl.columns[3].mask = 0b101;
        lvl.blocks[2].tex[4] = 77;
        lvl.tex_attr[9] ^= 4;
        let removed = lvl.entities.remove(0).index;
        let idx = lvl.free_entity_index().unwrap();
        lvl.entities.push(Entity { index: idx, kind: 5, sub: 11, group: 1, x: 30, y: 40, ..Default::default() });
        let slot = lvl.free_region_slot().unwrap();
        lvl.regions.push(Region { slot, kind: 2, cx: 50.5, cy: 60.5, hw: 1.5, hh: 2.5 });
        let back = Level::from_bytes(&lvl.to_bytes().unwrap()).unwrap();
        let c = back.cell(10, 20);
        assert_eq!((c.height, c.light, c.orient), (0x1234, 40 << 8, 5));
        assert_eq!(back.columns[3].mask, 0b101);
        assert_eq!(back.blocks[2].tex[4], 77);
        assert_eq!(back.tex_attr[9], lvl.tex_attr[9]);
        assert!(!back.entities.iter().any(|e| e.index == removed && removed != idx));
        let e = back.entities.iter().find(|e| e.index == idx).unwrap();
        assert_eq!((e.kind, e.sub, e.x, e.y), (5, 11, 30, 40));
        let r = back.regions.iter().find(|r| r.slot == slot).unwrap();
        assert_eq!((r.kind, r.cx, r.cy, r.hw, r.hh), (2, 50.5, 60.5, 1.5, 2.5));
        assert_eq!(back.regions.len(), lvl.regions.len());
    }
}
