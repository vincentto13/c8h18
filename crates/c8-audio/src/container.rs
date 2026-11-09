//! Bank directory shared by SOUND.DAT and MUSIC.DAT (spec 01 §11).
use c8_data::rnc;

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

/// A catalogue item with its full 32-byte entry (fields other than name, offset and length are
/// kept as read).
#[derive(Clone, PartialEq, Debug)]
pub struct Entry {
    pub name: String,
    pub data: Vec<u8>,
    pub raw: [u8; 32],
}

/// One bank in one format slot: the catalogue header (entry 0) and the items.
#[derive(Clone, PartialEq, Debug)]
pub struct Slot {
    pub header: [u8; 32],
    pub items: Vec<Entry>,
}

/// A whole container, for editing: `banks[bank][slot]`, `None` = absent.
#[derive(Clone, PartialEq, Debug)]
pub struct Container {
    pub slots: usize,
    pub banks: Vec<Vec<Option<Slot>>>,
}

impl Container {
    /// Read every bank of every slot. Blobs are unpacked; item bytes are kept as stored.
    pub fn parse(file: &[u8], nslots: usize) -> Result<Container, String> {
        if file.len() < 4 {
            return Err("container too short".into());
        }
        let dir = u32le(file, file.len() - 4) as usize;
        if dir + nslots * 2 > file.len() {
            return Err("bad directory offset".into());
        }
        let counts: Vec<usize> = (0..nslots).map(|s| u16le(file, dir + s * 2) as usize).collect();
        let nbanks = counts.iter().copied().max().unwrap_or(0);
        let mut banks = Vec::new();
        for bank in 0..nbanks {
            let mut slots = Vec::new();
            for slot in 0..nslots {
                let rec = dir + nslots * 2 + bank * nslots * 16 + slot * 16;
                if bank >= counts[slot] || rec + 16 > file.len() {
                    slots.push(None);
                    continue;
                }
                let cat_off = u32le(file, rec) as i32;
                let data_off = u32le(file, rec + 4) as i32;
                if cat_off < 0 || data_off < 0 {
                    slots.push(None);
                    continue;
                }
                let cat = read_blob(file, cat_off as usize, u32le(file, rec + 8) as usize)?;
                let data = read_blob(file, data_off as usize, u32le(file, rec + 12) as usize)?;
                if cat.len() < 32 {
                    return Err(format!("bank {bank} slot {slot}: catalogue too short"));
                }
                let mut header = [0u8; 32];
                header.copy_from_slice(&cat[..32]);
                let mut items = Vec::new();
                for e in cat[32..].chunks_exact(32) {
                    let name_end = e[..12].iter().position(|&b| b == 0).unwrap_or(12);
                    let off = u32le(e, 0x12) as usize;
                    let len = u32le(e, 0x1a) as usize;
                    let (start, end) = (off.min(data.len()), (off + len).min(data.len()));
                    let mut raw = [0u8; 32];
                    raw.copy_from_slice(e);
                    items.push(Entry { name: String::from_utf8_lossy(&e[..name_end]).into_owned(), data: data[start..end].to_vec(), raw });
                }
                slots.push(Some(Slot { header, items }));
            }
            banks.push(slots);
        }
        Ok(Container { slots: nslots, banks })
    }

    /// Write the container, unpacked: catalogue and data blobs bank by bank, then the directory.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut recs = Vec::new();
        for bank in &self.banks {
            for slot in 0..self.slots {
                let Some(Some(s)) = bank.get(slot) else {
                    recs.push([-1i32, -1, 0, 0]);
                    continue;
                };
                let mut data = Vec::new();
                let mut cat = s.header.to_vec();
                for it in &s.items {
                    let mut e = it.raw;
                    e[..12].fill(0);
                    let n = it.name.as_bytes();
                    e[..n.len().min(12)].copy_from_slice(&n[..n.len().min(12)]);
                    e[0x12..0x16].copy_from_slice(&(data.len() as u32).to_le_bytes());
                    e[0x1a..0x1e].copy_from_slice(&(it.data.len() as u32).to_le_bytes());
                    cat.extend_from_slice(&e);
                    data.extend_from_slice(&it.data);
                    while data.len() % 4 != 0 {
                        data.push(0);
                    }
                }
                cat[0x1a..0x1e].copy_from_slice(&(data.len() as u32).to_le_bytes());
                let cat_off = out.len();
                out.extend_from_slice(&cat);
                let data_off = out.len();
                out.extend_from_slice(&data);
                recs.push([cat_off as i32, data_off as i32, cat.len() as i32, data.len() as i32]);
            }
        }
        let dir = out.len();
        for slot in 0..self.slots {
            let count = self.banks.iter().rposition(|b| matches!(b.get(slot), Some(Some(_)))).map_or(0, |i| i + 1);
            out.extend_from_slice(&(count as u16).to_le_bytes());
        }
        for r in recs {
            for v in r {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out.extend_from_slice(&(dir as u32).to_le_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn original(name: &str) -> Option<Vec<u8>> {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/SOUND").join(name);
        std::fs::read(p).ok()
    }

    #[test]
    fn containers_round_trip() {
        for (name, slots) in [("SOUND.DAT", SOUND_SLOTS), ("MUSIC.DAT", MUSIC_SLOTS)] {
            let Some(file) = original(name) else { continue };
            let c = Container::parse(&file, slots).unwrap();
            let again = Container::parse(&c.encode(), slots).unwrap();
            assert_eq!(c.banks.len(), again.banks.len());
            for (b, (x, y)) in c.banks.iter().zip(&again.banks).enumerate() {
                for (s, (x, y)) in x.iter().zip(y).enumerate() {
                    let (Some(x), Some(y)) = (x, y) else {
                        assert_eq!(x.is_some(), y.is_some(), "{name} bank {b} slot {s}");
                        continue;
                    };
                    assert_eq!(x.items.len(), y.items.len());
                    for (i, j) in x.items.iter().zip(&y.items) {
                        assert_eq!((&i.name, &i.data), (&j.name, &j.data));
                    }
                    // the game's loader sees the same items
                    let items = load_bank(&c.encode(), slots, s, b).unwrap();
                    assert_eq!(items.len(), x.items.len());
                }
            }
        }
    }
}
