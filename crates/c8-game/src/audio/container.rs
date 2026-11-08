//! Bank directory shared by SOUND.DAT and MUSIC.DAT (spec 01 §11).
use crate::data::rnc;

fn u16le(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([d[o], d[o + 1]])
}
fn u32le(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

/// SOUND.DAT format slot used by the port: 8-bit 22050 Hz.
pub const SOUND_SLOT: usize = 3;
pub const SOUND_SLOTS: usize = 6;
/// MUSIC.DAT format slot used by the port: FM (.XMI).
pub const MUSIC_SLOT: usize = 2;
pub const MUSIC_SLOTS: usize = 4;

/// One catalogue item: its 8.3 name and its bytes.
pub struct Item {
    #[allow(dead_code)] // kept to describe the data; not needed by the game yet
    pub name: String,
    pub data: Vec<u8>,
}

/// Read one blob (catalogue or data) at `off`, unpacking RNC if present.
fn read_blob(file: &[u8], off: usize, len: usize) -> Result<Vec<u8>, String> {
    if off >= file.len() {
        return Err("blob offset past end of file".into());
    }
    let rest = &file[off..];
    if rnc::is_rnc(rest) && rest.len() >= 18 {
        let packed = u32::from_be_bytes([rest[8], rest[9], rest[10], rest[11]]) as usize;
        let end = (18 + packed).min(rest.len());
        rnc::unpack(&rest[..end])
    } else {
        let end = (off + len).min(file.len());
        Ok(file[off..end].to_vec())
    }
}

/// Load bank `bank` of format slot `slot` from a container with `nslots` slots.
/// Returns the catalogue items (index 0 = item 1).
pub fn load_bank(file: &[u8], nslots: usize, slot: usize, bank: usize) -> Result<Vec<Item>, String> {
    if file.len() < 4 {
        return Err("container too short".into());
    }
    let dir = u32le(file, file.len() - 4) as usize;
    if dir + nslots * 2 > file.len() {
        return Err("bad directory offset".into());
    }
    let counts: Vec<usize> = (0..nslots).map(|s| u16le(file, dir + s * 2) as usize).collect();
    if bank >= counts[slot] {
        return Err(format!("bank {bank} not present in slot {slot}"));
    }
    let rec = dir + nslots * 2 + bank * nslots * 16 + slot * 16;
    if rec + 16 > file.len() {
        return Err("bank record past end".into());
    }
    let cat_off = u32le(file, rec) as i32;
    let data_off = u32le(file, rec + 4) as i32;
    let cat_len = u32le(file, rec + 8) as usize;
    let data_len = u32le(file, rec + 12) as usize;
    if data_off < 0 || cat_off < 0 {
        return Err("slot absent".into());
    }
    let cat = read_blob(file, cat_off as usize, cat_len)?;
    let data = read_blob(file, data_off as usize, data_len)?;
    let mut items = Vec::new();
    let n = cat.len() / 32;
    for i in 1..n {
        let e = &cat[i * 32..i * 32 + 32];
        let name_end = e[..12].iter().position(|&b| b == 0).unwrap_or(12);
        let name = String::from_utf8_lossy(&e[..name_end]).into_owned();
        let off = u32le(e, 0x12) as usize;
        let len = u32le(e, 0x1a) as usize;
        let start = off.min(data.len());
        let end = (off + len).min(data.len());
        items.push(Item { name, data: data[start..end].to_vec() });
    }
    Ok(items)
}
