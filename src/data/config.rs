//! config.dat / save games: one 11449-byte block (spec 01 §12).
use super::files::{i16le, i32le, save_dir, u16le};

pub const CONFIG_SIZE: usize = 0x2cb9;
pub const DEFAULT_LAPS: [u8; 6] = [11, 8, 9, 8, 9, 5];

#[derive(Clone)]
pub struct Config {
    pub raw: Vec<u8>,
    /// config.dat was missing at start (first run)
    pub fresh: bool,
}

fn put_str(d: &mut [u8], o: usize, len: usize, s: &str) {
    for i in 0..len {
        d[o + i] = 0;
    }
    for (i, b) in s.bytes().take(len - 1).enumerate() {
        d[o + i] = b;
    }
}

impl Config {
    /// Fresh config with the port's defaults (spec 01 §12, §16).
    pub fn fresh() -> Config {
        let mut c = Config { raw: vec![0; CONFIG_SIZE], fresh: true };
        c.raw[0x22] = 1; // player active
        c.set_i32(0x097c, 0x7800);
        c.set_i32(0x0980, 0x7800);
        c.set_i32(0x098c, 4);
        c.set_i32(0x0990, 1); // port: stereo on
        for row in 0..50 {
            let o = 0x0994 + row * 39;
            c.set_i32(o, 0);
            c.raw[o + 4] = 15;
            put_str(&mut c.raw, o + 5, 32, "C8H18");
            c.raw[o + 37] = (row % 8) as u8;
            c.raw[o + 38] = (row % 6) as u8;
        }
        for t in 0..6 {
            let o = 0x29aa + 74 * t;
            // port: records start at 0 = "no record"
            c.raw[o + 72] = DEFAULT_LAPS[t];
            c.raw[o + 73] = 15;
        }
        c.apply_detail_defaults();
        c
    }
    fn apply_detail_defaults(&mut self) {
        self.set_i32(0x2996, 1);
        self.set_i32(0x299a, 1);
        self.set_i32(0x299e, 2);
        self.set_i32(0x29a2, 3);
        self.set_i32(0x29a6, 1);
    }
    pub fn load() -> Config {
        let p = save_dir().join("config.dat");
        match std::fs::read(&p) {
            Ok(d) if d.len() == CONFIG_SIZE => Config { raw: d, fresh: false },
            _ => Config::fresh(),
        }
    }
    pub fn save(&mut self) {
        self.set_cheat(false);
        let _ = std::fs::write(save_dir().join("config.dat"), &self.raw);
    }
    pub fn slot_path(slot: usize) -> std::path::PathBuf {
        save_dir().join(format!("save{slot:02}.dat"))
    }
    /// Slot name for the LOAD/SAVE lists; None = EMPTY.
    pub fn slot_name(slot: usize) -> Option<String> {
        let d = std::fs::read(Self::slot_path(slot)).ok()?;
        if d.len() != CONFIG_SIZE {
            return None;
        }
        Some(cstr(&d[0..32]))
    }
    pub fn load_slot(slot: usize) -> Option<Config> {
        let d = std::fs::read(Self::slot_path(slot)).ok()?;
        (d.len() == CONFIG_SIZE).then(|| Config { raw: d, fresh: false })
    }
    pub fn save_slot(&self, slot: usize) {
        let _ = std::fs::write(Self::slot_path(slot), &self.raw);
    }

    // ---- raw access ----
    pub fn u16(&self, o: usize) -> u16 { u16le(&self.raw, o) }
    pub fn i16(&self, o: usize) -> i16 { i16le(&self.raw, o) }
    pub fn i32(&self, o: usize) -> i32 { i32le(&self.raw, o) }
    pub fn set_u16(&mut self, o: usize, v: u16) { self.raw[o..o + 2].copy_from_slice(&v.to_le_bytes()); }
    pub fn set_i16(&mut self, o: usize, v: i16) { self.raw[o..o + 2].copy_from_slice(&v.to_le_bytes()); }
    pub fn set_i32(&mut self, o: usize, v: i32) { self.raw[o..o + 4].copy_from_slice(&v.to_le_bytes()); }
    pub fn str_at(&self, o: usize, len: usize) -> String { cstr(&self.raw[o..o + len]) }
    pub fn set_str_at(&mut self, o: usize, len: usize, s: &str) { put_str(&mut self.raw, o, len, s); }

    // ---- named fields ----
    pub fn set_slot_title(&mut self, s: &str) { self.set_str_at(0, 32, s) }
    pub fn cheat(&self) -> bool { self.raw[0x22] & 4 != 0 }
    pub fn set_cheat(&mut self, on: bool) { if on { self.raw[0x22] |= 4 } else { self.raw[0x22] &= !4 } }
    pub fn toggle_debug_overlay(&mut self) { let v = self.u16(0x36) ^ 2; self.set_u16(0x36, v) }
    pub fn view_slot(&self) -> u16 { self.u16(0x0949).min(3) }
    pub fn team(&self) -> u8 { self.raw[0x094b] & 7 }
    pub fn set_team(&mut self, v: u8) { self.raw[0x094b] = v & 7 }
    pub fn craft(&self) -> u8 { self.raw[0x094c] % 6 }
    pub fn set_craft(&mut self, v: u8) { self.raw[0x094c] = v % 6 }
    pub fn name(&self) -> String { self.str_at(0x094f, 9) }
    pub fn set_name(&mut self, s: &str) { self.set_str_at(0x094f, 9, s) }
    pub fn difficulty(&self) -> u8 { self.raw[0x0977].min(3) }
    pub fn set_difficulty(&mut self, v: u8) { self.raw[0x0977] = v.min(3) }
    pub fn music_volume(&self) -> i32 { self.i32(0x097c).clamp(0, 0x8000) }
    pub fn set_music_volume(&mut self, v: i32) { self.set_i32(0x097c, v.clamp(0, 0x8000)) }
    pub fn effects_volume(&self) -> i32 { self.i32(0x0980).clamp(0, 0x8000) }
    pub fn set_effects_volume(&mut self, v: i32) { self.set_i32(0x0980, v.clamp(0, 0x8000)) }
    pub fn stereo(&self) -> bool { self.i32(0x0990) != 0 }
    pub fn shading(&self) -> bool { self.i32(0x2996) != 0 }
    pub fn set_shading(&mut self, v: bool) { self.set_i32(0x2996, v as i32) }
    pub fn sky(&self) -> bool { self.i32(0x299a) != 0 }
    pub fn set_sky(&mut self, v: bool) { self.set_i32(0x299a, v as i32) }
    pub fn set_near_distance(&mut self, v: i32) { self.set_i32(0x299e, v) }
    pub fn texture_level(&self) -> i32 { self.i32(0x29a2).clamp(0, 4) }
    pub fn set_texture_level(&mut self, v: i32) { self.set_i32(0x29a2, v.clamp(0, 4)) }
    pub fn texture_mapping(&self) -> bool { self.i32(0x29a6) != 0 }
    pub fn set_texture_mapping(&mut self, v: bool) { self.set_i32(0x29a6, v as i32) }
    /// Championship race number 0..5 = selected track - 1.
    pub fn race_number(&self) -> u16 { self.u16(0x2c76).min(5) }
    pub fn set_race_number(&mut self, v: u16) { self.set_u16(0x2c76, v.min(5)) }
    pub fn language(&self) -> usize { (self.u16(0x2c7a) as usize).min(4) }
    pub fn set_language(&mut self, v: usize) { self.set_u16(0x2c7a, v.min(4) as u16) }
    pub fn last_slot(&self) -> usize { (self.u16(0x2c7c) as usize).min(4) }
    pub fn set_last_slot(&mut self, v: usize) { self.set_u16(0x2c7c, v as u16) }
    pub fn champ_points(&self, slot: usize) -> i16 { self.i16(0x2c7e + 2 * slot) }
    pub fn set_champ_points(&mut self, slot: usize, v: i16) { self.set_i16(0x2c7e + 2 * slot, v) }

    // track records (t = 0..5)
    pub fn track_laps(&self, t: usize) -> u8 { self.raw[0x29aa + 74 * t + 72].clamp(1, 50) }
    pub fn set_track_laps(&mut self, t: usize, v: u8) { self.raw[0x29aa + 74 * t + 72] = v.clamp(1, 50) }
    pub fn best_lap(&self, t: usize) -> (i32, String) { let o = 0x29aa + 74 * t; (self.i32(o), self.str_at(o + 4, 32)) }
    pub fn set_best_lap(&mut self, t: usize, ticks: i32, name: &str) { let o = 0x29aa + 74 * t; self.set_i32(o, ticks); self.set_str_at(o + 4, 32, name) }
    pub fn best_race(&self, t: usize) -> (i32, String) { let o = 0x29aa + 74 * t; (self.i32(o + 36), self.str_at(o + 40, 32)) }
    pub fn set_best_race(&mut self, t: usize, ticks: i32, name: &str) { let o = 0x29aa + 74 * t; self.set_i32(o + 36, ticks); self.set_str_at(o + 40, 32, name) }

    // high scores (row 0..49): (score, assessment, name, team, craft)
    pub fn hiscore(&self, row: usize) -> (i32, u8, String, u8, u8) {
        let o = 0x0994 + row * 39;
        (self.i32(o), self.raw[o + 4], self.str_at(o + 5, 32), self.raw[o + 37], self.raw[o + 38])
    }
    pub fn set_hiscore(&mut self, row: usize, e: &(i32, u8, String, u8, u8)) {
        let o = 0x0994 + row * 39;
        self.set_i32(o, e.0);
        self.raw[o + 4] = e.1;
        self.set_str_at(o + 5, 32, &e.2);
        self.raw[o + 37] = e.3;
        self.raw[o + 38] = e.4;
    }

    // sliding puzzle state (0x2b66..)
    pub const PUZZLE: usize = 0x2b66;
}

pub fn cstr(d: &[u8]) -> String {
    let end = d.iter().position(|&b| b == 0).unwrap_or(d.len());
    d[..end].iter().map(|&b| b as char).collect()
}
