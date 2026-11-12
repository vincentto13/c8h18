//! Level file MAPS/LEVEL0-N.DAT (spec 01 §9): reading and writing.
//!
//! A level is a fixed-size snapshot with areas of unknown use. [`Level`] keeps the bytes it was
//! read from and [`Level::to_bytes`] writes only the fields it understands back into them, so a
//! level that is read and written unchanged is byte-identical.
use super::files::{i16le, load, u16le};
use std::path::{Path, PathBuf};

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
    /// Port addition (level sidecar, spec 01 §9.7): the blocks of this cell's column stand on this
    /// flat height (8.8 cells) instead of on the cell's terrain surface. Not stored in the `.DAT`.
    pub base: Option<u16>,
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
    /// Port addition (level sidecar, spec 01 §9.7): what the entity creates stands on the highest
    /// surface at or below this height (cells) instead of the highest surface of the cell; for
    /// waypoints, the route's height there. Not stored in the `.DAT`.
    pub level: Option<f32>,
    /// Port addition (level sidecar): a wall segment (on its first point) or a checkpoint applies only
    /// to crafts whose height is in `lo..=hi` (cells). Not stored in the `.DAT`.
    pub range: Option<(f32, f32)>,
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
    /// Port addition (level sidecar): a charger works only for crafts whose height is in `lo..=hi`.
    pub range: Option<(f32, f32)>,
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
    /// Port addition: the sidecar `MAPS/LEVEL0-N.TXT` (spec 01 §9.7) is applied when it exists.
    pub fn load(track: usize) -> Result<Level, String> {
        let d = load(&format!("maps/level0-{track}.dat"))?;
        let mut lvl = Self::from_bytes(&d).map_err(|e| format!("level {track}: {e}"))?;
        let side = format!("maps/level0-{track}.txt");
        if super::files::exists(&side) {
            let t = super::files::read_raw(&side).and_then(|b| String::from_utf8(b).map_err(|e| e.to_string()));
            t.and_then(|t| lvl.apply_addition(&t)).map_err(|e| format!("{}: {e}", super::files::asset_name(&side)))?;
        }
        Ok(lvl)
    }
    /// Read a level file from any path (RNC-packed or not).
    /// Its sidecar ([`sidecar_path`](Self::sidecar_path)) is applied when it exists.
    pub fn read_file(path: &Path) -> Result<Level, String> {
        let d = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let d = super::rnc::maybe_unpack(d).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut lvl = Self::from_bytes(&d).map_err(|e| format!("{}: {e}", path.display()))?;
        let side = Self::sidecar_path(path);
        if side.is_file() {
            let t = std::fs::read(&side).map_err(|e| e.to_string()).and_then(|b| String::from_utf8(b).map_err(|e| e.to_string()));
            t.and_then(|t| lvl.apply_addition(&t)).map_err(|e| format!("{}: {e}", side.display()))?;
        }
        Ok(lvl)
    }
    /// Write the level unpacked (the game reads packed and unpacked levels). The port addition goes
    /// to the sidecar next to it; a level without the addition removes a sidecar that is there.
    pub fn write_file(&self, path: &Path) -> Result<(), String> {
        let d = self.to_bytes()?;
        std::fs::write(path, d).map_err(|e| format!("{}: {e}", path.display()))?;
        let side = Self::sidecar_path(path);
        match self.addition_text() {
            Some(t) => std::fs::write(&side, t).map_err(|e| format!("{}: {e}", side.display())),
            None if side.is_file() => std::fs::remove_file(&side).map_err(|e| format!("{}: {e}", side.display())),
            None => Ok(()),
        }
    }
    /// The sidecar of a level file: `LEVEL0-1.DAT` -> `LEVEL0-1.TXT` in the same folder (an existing
    /// file in any letter case, else the extension in the case of the level's).
    pub fn sidecar_path(path: &Path) -> PathBuf {
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let want = format!("{stem}.txt").to_ascii_lowercase();
        let rd = if dir.as_os_str().is_empty() { std::fs::read_dir(".") } else { std::fs::read_dir(&dir) };
        if let Ok(rd) = rd {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().to_ascii_lowercase() == want {
                    return dir.join(e.file_name());
                }
            }
        }
        let lower = path.extension().is_some_and(|e| e.to_string_lossy().chars().any(|c| c.is_ascii_lowercase()));
        dir.join(format!("{stem}.{}", if lower { "txt" } else { "TXT" }))
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
            cells.push(Cell { light: i16le(d, o), height: u16le(d, o + 2), tex: i16le(d, o + 4), orient: d[o + 10] >> 4, base: None });
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
                level: None,
                range: None,
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
                regions.push(Region { slot: r, kind: k, cx: fx88(d, o + 24), cy: fx88(d, o + 26), hw: fx88(d, o + 45), hh: fx88(d, o + 47), range: None });
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
    /// Height the blocks of cell (ix, iy) stand on at point (x, y): the cell's flat base (port
    /// addition) or the terrain surface.
    #[inline]
    fn block_base(&self, ix: i32, iy: i32, x: f32, y: f32) -> (f32, bool) {
        match self.cell(ix, iy).base {
            Some(b) => (b as f32 / 256.0, true),
            None => (self.terrain_height(x, y), false),
        }
    }
    /// Block occupancy: is there a solid block at height z (cells) above cell (x,y)?
    /// Blocks span k..k+1 above the cell's interpolated surface (or its flat base, port addition).
    pub fn solid_at(&self, x: f32, y: f32, z: f32) -> bool {
        let ix = x.floor() as i32;
        let iy = y.floor() as i32;
        if ix < 0 || iy < 0 || ix >= MAP_W as i32 || iy >= MAP_H as i32 {
            return true;
        }
        let Some(col) = self.column(ix, iy) else { return false };
        let (base, _) = self.block_base(ix, iy, x, y);
        let rel = z - base;
        if rel < 0.0 {
            return false;
        }
        let k = rel.floor() as i32;
        k < 16 && col.mask & (1 << k) != 0
    }
    /// Surface below height z: top of the highest block below z, or the terrain.
    pub fn floor_below(&self, x: f32, y: f32, z: f32) -> f32 {
        let ground = self.terrain_height(x, y);
        let ix = x.floor() as i32;
        let iy = y.floor() as i32;
        if let Some(col) = self.column(ix, iy) {
            let (base, flat) = self.block_base(ix, iy, x, y);
            let rel = z - base;
            let mut top = 0.0f32;
            let mut any = false;
            for k in 0..16 {
                if col.mask & (1 << k) != 0 && (k as f32 + 1.0) <= rel + 0.001 {
                    top = k as f32 + 1.0;
                    any = true;
                }
            }
            // if inside a block, its top counts too
            let kin = rel.floor() as i32;
            if rel >= 0.0 && kin < 16 && col.mask & (1 << kin) != 0 {
                top = top.max(kin as f32 + 1.0);
                any = true;
            }
            if flat {
                // a flat base may lie above or below the terrain: the higher of the two surfaces
                return if any { (base + top).max(ground) } else { ground };
            }
            return base + top;
        }
        ground
    }
    /// Surface for the crafts' hover sensors (spec 05 §3.i, §3.l): the terrain or the top of the
    /// highest block below height z. Unlike [`floor_below`](Self::floor_below), a block the point
    /// is inside does not count, so a sensor pushed into a wall can't climb onto it.
    pub fn surface_below(&self, x: f32, y: f32, z: f32) -> f32 {
        let ground = self.terrain_height(x, y);
        let (ix, iy) = (x.floor() as i32, y.floor() as i32);
        let Some(col) = self.column(ix, iy) else { return ground };
        let (base, flat) = self.block_base(ix, iy, x, y);
        let rel = z - base;
        let tops = (0..16).filter(|&k| col.mask & (1 << k) != 0 && (k as f32 + 1.0) <= rel + 0.001).map(|k| k as f32 + 1.0);
        if flat {
            return tops.map(|t| base + t).fold(ground, f32::max);
        }
        base + tops.fold(0.0, f32::max)
    }
    /// Port addition: the underside of the lowest block whose bottom is at or above height z over
    /// point (x, y), if any (a deck above a craft).
    pub fn ceiling_above(&self, x: f32, y: f32, z: f32) -> Option<f32> {
        let (ix, iy) = (x.floor() as i32, y.floor() as i32);
        let col = self.column(ix, iy)?;
        let (base, _) = self.block_base(ix, iy, x, y);
        (0..16).filter(|&k| col.mask & (1 << k) != 0 && base + k as f32 >= z).map(|k| base + k as f32).reduce(f32::min)
    }
    /// Port addition: where an entity's things stand: the highest surface at or below its
    /// `level` when it has one, else the highest surface of the cell (as the original).
    pub fn entity_ground(&self, e: &Entity) -> f32 {
        let (x, y) = (e.x as f32 + 0.5, e.y as f32 + 0.5);
        match e.level {
            Some(h) => self.surface_below(x, y, h),
            None => self.floor_below(x, y, 255.0),
        }
    }
    /// Port addition: does the level use the sidecar data (flat block bases, entity levels or
    /// height ranges, region ranges)?
    pub fn has_addition(&self) -> bool {
        self.cells.iter().any(|c| c.base.is_some())
            || self.entities.iter().any(|e| e.level.is_some() || e.range.is_some())
            || self.regions.iter().any(|r| r.range.is_some())
    }
    /// Port addition: the sidecar text (spec 01 §9.7), or None for a level without the addition.
    pub fn addition_text(&self) -> Option<String> {
        if !self.has_addition() {
            return None;
        }
        use std::fmt::Write;
        let mut out = String::from("# C8H18 level addition (port addition, docs/spec/01-file-formats.md section 9.7)\n");
        // bases: runs along each row, merged with equal runs on the following rows
        let mut runs: Vec<(usize, usize, usize, usize, u16)> = Vec::new();
        for y in 0..MAP_H {
            let mut x = 0;
            while x < MAP_W {
                let Some(b) = self.cells[y * MAP_W + x].base else {
                    x += 1;
                    continue;
                };
                let x0 = x;
                while x + 1 < MAP_W && self.cells[y * MAP_W + x + 1].base == Some(b) {
                    x += 1;
                }
                match runs.iter_mut().find(|r| r.0 == x0 && r.2 == x && r.3 + 1 == y && r.4 == b) {
                    Some(r) => r.3 = y,
                    None => runs.push((x0, y, x, y, b)),
                }
                x += 1;
            }
        }
        for (x0, y0, x1, y1, b) in runs {
            let _ = writeln!(out, "base {x0} {y0} {x1} {y1} {}", b as f32 / 256.0);
        }
        let mut ents: Vec<&Entity> = self.entities.iter().collect();
        ents.sort_by_key(|e| e.index);
        for e in ents {
            if let Some(h) = e.level {
                let _ = writeln!(out, "level {} {h}", e.index);
            }
            if let Some((lo, hi)) = e.range {
                let _ = writeln!(out, "range {} {lo} {hi}", e.index);
            }
        }
        let mut regs: Vec<&Region> = self.regions.iter().collect();
        regs.sort_by_key(|r| r.slot);
        for r in regs {
            if let Some((lo, hi)) = r.range {
                let _ = writeln!(out, "region {} {lo} {hi}", r.slot);
            }
        }
        Some(out)
    }
    /// Port addition: apply a sidecar text (spec 01 §9.7) to the level. Errors name the line.
    pub fn apply_addition(&mut self, text: &str) -> Result<(), String> {
        for (n, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let err = |m: String| format!("line {}: {m}", n + 1);
            let w: Vec<&str> = line.split_whitespace().collect();
            let num = |k: usize| -> Result<f32, String> {
                let v = w.get(k).ok_or_else(|| err(format!("`{}` needs more values", w[0])))?;
                v.parse::<f32>().ok().filter(|f| !f.is_nan()).ok_or_else(|| err(format!("`{v}` is not a number")))
            };
            let int = |k: usize| -> Result<usize, String> {
                let v = w.get(k).ok_or_else(|| err(format!("`{}` needs more values", w[0])))?;
                v.parse::<usize>().map_err(|_| err(format!("`{v}` is not a whole number")))
            };
            let count = |c: usize| if w.len() == c { Ok(()) } else { Err(err(format!("`{}` takes {} values, not {}", w[0], c - 1, w.len() - 1))) };
            let range = |lo: f32, hi: f32| if lo <= hi { Ok((lo, hi)) } else { Err(err(format!("range {lo} .. {hi} is empty"))) };
            match w[0].to_ascii_lowercase().as_str() {
                "base" => {
                    let (x0, y0, x1, y1, h) = if w.len() == 4 {
                        (int(1)?, int(2)?, int(1)?, int(2)?, num(3)?)
                    } else {
                        count(6)?;
                        (int(1)?, int(2)?, int(3)?, int(4)?, num(5)?)
                    };
                    if x0 > x1 || y0 > y1 || x1 >= MAP_W || y1 >= MAP_H {
                        return Err(err(format!("cells {x0},{y0} .. {x1},{y1} are not a rectangle on the map")));
                    }
                    let b = (h * 256.0).round();
                    if !(0.0..=65535.0).contains(&b) {
                        return Err(err(format!("base height {h} is out of range 0..255.99")));
                    }
                    for y in y0..=y1 {
                        for x in x0..=x1 {
                            self.cells[y * MAP_W + x].base = Some(b as u16);
                        }
                    }
                }
                "level" | "range" => {
                    let idx = int(1)?;
                    let v = if w[0].eq_ignore_ascii_case("level") {
                        count(3)?;
                        (num(2)?, 0.0)
                    } else {
                        count(4)?;
                        range(num(2)?, num(3)?)?
                    };
                    let e = self.entities.iter_mut().find(|e| e.index == idx).ok_or_else(|| err(format!("no entity record {idx}")))?;
                    if w[0].eq_ignore_ascii_case("level") {
                        e.level = Some(v.0);
                    } else {
                        e.range = Some(v);
                    }
                }
                "region" => {
                    count(4)?;
                    let slot = int(1)?;
                    let v = range(num(2)?, num(3)?)?;
                    let r = self.regions.iter_mut().find(|r| r.slot == slot).ok_or_else(|| err(format!("no region record {slot}")))?;
                    r.range = Some(v);
                }
                k => return Err(err(format!("unknown keyword `{k}`"))),
            }
        }
        Ok(())
    }
    /// Map collision: inside a solid block or below the terrain.
    pub fn collide(&self, x: f32, y: f32, z: f32) -> bool {
        self.solid_at(x, y, z) || z < self.terrain_height(x, y)
    }
}

/// Port addition: a small example level with a crossing (spec 01 §9.7), built in code from an
/// all-zero level. It is the reference for track builders and the levels' tests:
///
/// - the lower road runs west to east along row 39 (rows 38..41 between walls on rows 37 and 42),
///   flat at height 0;
/// - the upper road runs north to south along column 59 (columns 58..61 between walls on columns 57
///   and 62): a ramp from height 0 (corner row 27) up to 2 (corner row 35), a deck of block
///   columns over cells 57..62 × 35..44 on the flat base 1.0 with one block level (underside 1,
///   top 2), and a ramp down from corner row 45 to row 53. Under the deck's end rows (35, 36 and
///   43, 44) the terrain falls from the ramp's top to the lower road, so ramp and deck top meet
///   without a step;
/// - the wall segments that cross the other road carry height ranges (lower road walls up to 1.4,
///   upper road walls from 1.4), the waypoints on and under the deck carry levels, and the
///   checkpoint on the deck counts only crafts above 1.6;
/// - the waypoints form one loop: the lower road, a bend north-east, the upper road over the deck,
///   a bend south-west, and back. Checkpoints 0 (row road at column 40), 1 (on the deck), 2 (south-west
///   bend); a start area on the lower road; no trucks, chargers or textures.
pub fn crossing_example() -> Level {
    let mut l = Level::parse(&vec![0u8; LEVEL_SIZE]);
    // terrain: the upper road's ramps, corners of columns 57..63
    for y in 0..MAP_H as i32 {
        let h = match y {
            27..=35 => 2.0 * (y - 27) as f32 / 8.0,
            36 | 44 => 1.0,
            45..=53 => 2.0 * (53 - y) as f32 / 8.0,
            _ => 0.0,
        };
        for x in 57..=63 {
            l.cell_mut(x, y).height = (h * 256.0) as u16;
        }
    }
    // the deck: one block level on the flat base 1.0
    l.columns[1] = Column { mask: 1, floor_tex: 0, blocks: [1, 0, 0, 0, 0, 0, 0, 0] };
    l.blocks[1] = Block { tex: [5; 6], orient: [0; 6] };
    for y in 35..=44 {
        for x in 57..=62 {
            let c = l.cell_mut(x, y);
            c.tex = -1;
            c.base = Some(256);
        }
    }
    // records are numbered from 1 in the order they are added
    let add = |l: &mut Level, e: Entity| {
        let index = l.entities.len() + 1;
        l.entities.push(Entity { index, ..e });
    };
    // walls: chains of (x, y, range of the segment starting there)
    let low: Option<(f32, f32)> = Some((f32::NEG_INFINITY, 1.4));
    let high: Option<(f32, f32)> = Some((1.4, f32::INFINITY));
    let chains: [[(i32, i32, Option<(f32, f32)>); 4]; 4] = [
        [(50, 37, None), (56, 37, low), (63, 37, None), (69, 37, None)],
        [(50, 42, None), (56, 42, low), (63, 42, None), (69, 42, None)],
        [(57, 25, None), (57, 36, high), (57, 43, None), (57, 55, None)],
        [(62, 25, None), (62, 36, high), (62, 43, None), (62, 55, None)],
    ];
    for chain in chains {
        let first = l.entities.len() + 1;
        for (k, &(x, y, range)) in chain.iter().enumerate() {
            let link = if k + 1 < chain.len() { (first + k + 1) as u16 } else { 0 };
            add(&mut l, Entity { kind: 9, sub: 0, group: 1, x, y, link, range, ..Default::default() });
        }
    }
    // the waypoint loop (x, y, level)
    let route: [(i32, i32, Option<f32>); 26] = [
        (30, 39, None), (42, 39, None), (52, 39, None), (59, 39, Some(0.5)), (66, 39, None), (78, 39, None),
        (88, 34, None), (88, 20, None), (82, 12, None), (70, 10, None), (60, 12, None), (59, 18, None),
        (59, 24, None), (59, 31, None), (59, 39, Some(2.5)), (59, 47, None), (59, 54, None), (59, 62, None),
        (56, 69, None), (48, 72, None), (40, 72, None), (33, 69, None), (30, 62, None), (30, 54, None),
        (30, 47, None), (30, 42, None),
    ];
    let first = l.entities.len() + 1;
    for (k, &(x, y, level)) in route.iter().enumerate() {
        let link = (first + (k + 1) % route.len()) as u16;
        add(&mut l, Entity { kind: 9, sub: 1, group: 1, x, y, link, level, ..Default::default() });
    }
    // checkpoints: the line on the lower road, one on the deck (above 1.6 only), one in the south-west bend
    add(&mut l, Entity { kind: 1, sub: 5, group: 1, value: 0, x: 40, y: 35, w: 0, h: 8, ..Default::default() });
    add(&mut l, Entity { kind: 1, sub: 5, group: 1, value: 1, x: 56, y: 38, w: 6, h: 1, range: Some((1.6, f32::INFINITY)), ..Default::default() });
    add(&mut l, Entity { kind: 1, sub: 5, group: 1, value: 2, x: 36, y: 66, w: 0, h: 8, ..Default::default() });
    // a minigun upgrade under the deck (level 0.5: on the road) and one on the deck (no level: the top)
    add(&mut l, Entity { kind: 5, sub: 11, group: 1, x: 60, y: 39, level: Some(0.5), ..Default::default() });
    add(&mut l, Entity { kind: 5, sub: 11, group: 1, x: 59, y: 40, ..Default::default() });
    l.regions.push(Region { slot: 999, kind: 4, cx: 35.5, cy: 39.5, hw: 2.0, hh: 2.0, range: None });
    l.update_extent();
    l
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
        lvl.regions.push(Region { slot, kind: 2, cx: 50.5, cy: 60.5, hw: 1.5, hh: 2.5, range: None });
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

    /// The port addition survives the sidecar text and a write and re-read; the level file itself
    /// stays as without the addition, and a level without it writes no sidecar.
    #[test]
    fn addition_round_trip() {
        let mut l = crossing_example();
        // an all-zero level has no region record to copy for new ones
        l.regions.clear();
        assert!(l.has_addition());
        let text = l.addition_text().unwrap();
        assert!(text.contains("base 57 35 62 44 1\n"), "{text}");
        // the records quoted in spec 01 §9.7
        for line in ["range 2 -inf 1.4", "range 10 1.4 inf", "level 20 0.5", "level 31 2.5", "range 44 1.6 inf", "level 46 0.5"] {
            assert!(text.contains(&format!("{line}\n")), "{line}: {text}");
        }
        let mut plain = l.clone();
        for c in &mut plain.cells {
            c.base = None;
        }
        for e in &mut plain.entities {
            e.level = None;
            e.range = None;
        }
        assert!(!plain.has_addition() && plain.addition_text().is_none());
        let mut back = plain.clone();
        back.apply_addition(&text).unwrap();
        assert!(back.cells == l.cells && back.entities == l.entities);
        // files: next to the level, removed again when the addition goes
        let dir = std::env::temp_dir().join(format!("c8-level-addition-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("LEVEL0-3.DAT");
        let mut withraw = Level::parse(&l.to_bytes().unwrap());
        withraw.apply_addition(&text).unwrap();
        withraw.write_file(&path).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("LEVEL0-3.TXT")).unwrap(), text);
        let read = Level::read_file(&path).unwrap();
        assert!(read.cells == l.cells && read.entities == l.entities);
        assert_eq!(std::fs::read(&path).unwrap(), plain.to_bytes().unwrap());
        Level::parse(&plain.to_bytes().unwrap()).write_file(&path).unwrap();
        assert!(!dir.join("LEVEL0-3.TXT").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn addition_errors_name_the_line() {
        let mut l = crossing_example();
        assert_eq!(l.apply_addition("# fine\nlevel 5 1\nlevel 3999 1\n").unwrap_err(), "line 3: no entity record 3999");
        assert!(l.apply_addition("range 5 2 1").unwrap_err().contains("line 1"));
        assert!(l.apply_addition("wobble 1").unwrap_err().contains("unknown keyword"));
        assert!(l.apply_addition("base 300 0 1").is_err());
        l.apply_addition("base 3 4 2.5\nregion 999 0 inf").unwrap();
        assert_eq!(l.cell(3, 4).base, Some(640));
        assert_eq!(l.regions[0].range, Some((0.0, f32::INFINITY)));
    }

    /// Surfaces of a deck on a flat base: on it, under it, at its ends.
    #[test]
    fn deck_surfaces() {
        let l = crossing_example();
        // over the lower road: deck top 2, road 0 under it, underside 1
        assert_eq!(l.floor_below(59.5, 39.5, 255.0), 2.0);
        assert_eq!(l.surface_below(59.5, 39.5, 0.7), 0.0);
        assert_eq!(l.surface_below(59.5, 39.5, 2.4), 2.0);
        assert!(!l.solid_at(59.5, 39.5, 0.6) && l.solid_at(59.5, 39.5, 1.5) && !l.solid_at(59.5, 39.5, 2.1));
        assert_eq!(l.ceiling_above(59.5, 39.5, 0.1), Some(1.0));
        assert_eq!(l.ceiling_above(59.5, 39.5, 2.1), None);
        // the ramp's top meets the deck without a step
        assert!((l.terrain_height(59.5, 34.999) - 2.0).abs() < 0.01);
        assert_eq!(l.surface_below(59.5, 35.2, 2.4), 2.0);
        // an entity with a level stands on the surface at or below it
        let e = l.entities.iter().find(|e| e.level == Some(0.5)).unwrap();
        assert_eq!(l.entity_ground(e), 0.0);
        let e = l.entities.iter().find(|e| e.level == Some(2.5)).unwrap();
        assert_eq!(l.entity_ground(e), 2.0);
    }
}
