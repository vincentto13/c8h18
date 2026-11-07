//! Billboard images TMAPS.DAT/TAB (spec 01 §6).
use super::files::{read_raw, u16le, u32le};
use super::rnc::maybe_unpack;

#[derive(Clone, Default)]
pub struct TmapImage {
    pub mode: u8,
    pub w: u32,
    pub h: u32,
    pub pix: Vec<u8>,
}

pub fn load_tmaps() -> Result<Vec<TmapImage>, String> {
    let dat = read_raw("data/tmaps.dat")?;
    let tab = read_raw("data/tmaps.tab")?;
    let n = tab.len() / 10;
    let mut out = Vec::new();
    for i in 0..n.saturating_sub(1).min(48) {
        let off = u32le(&tab, i * 10 + 4) as usize;
        let next = u32le(&tab, (i + 1) * 10 + 4) as usize;
        let blob = dat[off..next.min(dat.len())].to_vec();
        let img = maybe_unpack(blob)?;
        let mode = img[1];
        let w = u16le(&img, 2) as u32;
        let h = u16le(&img, 4) as u32;
        let mut pix = img[6..].to_vec();
        pix.resize((w * h) as usize, 0);
        out.push(TmapImage { mode, w, h, pix });
    }
    Ok(out)
}
