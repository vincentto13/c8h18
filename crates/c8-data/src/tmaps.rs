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
    let mut t = Tmaps::load()?;
    t.images.truncate(48);
    Ok(t.images)
}

/// The whole billboard container, with the animation group of each image.
#[derive(Clone, Default)]
pub struct Tmaps {
    pub images: Vec<TmapImage>,
    pub groups: Vec<u16>,
}

impl Tmaps {
    pub fn load() -> Result<Tmaps, String> {
        Self::decode(&read_raw("data/tmaps.dat")?, &read_raw("data/tmaps.tab")?)
    }
    pub fn decode(dat: &[u8], tab: &[u8]) -> Result<Tmaps, String> {
        let n = tab.len() / 10;
        let mut t = Tmaps::default();
        for i in 0..n.saturating_sub(1) {
            let off = u32le(tab, i * 10 + 4) as usize;
            let next = u32le(tab, (i + 1) * 10 + 4) as usize;
            if off > next || off > dat.len() {
                return Err(format!("tmaps: bad offset of image {i}"));
            }
            let img = maybe_unpack(dat[off..next.min(dat.len())].to_vec())?;
            if img.len() < 6 {
                return Err(format!("tmaps: image {i} too short"));
            }
            let w = u16le(&img, 2) as u32;
            let h = u16le(&img, 4) as u32;
            let mut pix = img[6..].to_vec();
            pix.resize((w * h) as usize, 0);
            t.images.push(TmapImage { mode: img[1], w, h, pix });
            t.groups.push(u16le(tab, i * 10 + 8));
        }
        Ok(t)
    }
    /// Encode as (DAT, TAB) with unpacked blobs (the loader uses a blob without the RNC signature as
    /// stored, spec 01 §6). The 8 header bytes are the port's own (`C8H18TEX`); nothing reads them.
    pub fn encode(&self) -> (Vec<u8>, Vec<u8>) {
        let mut dat = b"C8H18TEX".to_vec();
        let mut tab = Vec::new();
        for (i, img) in self.images.iter().enumerate() {
            let size = 6 + img.w * img.h;
            tab.extend_from_slice(&size.to_le_bytes());
            tab.extend_from_slice(&(dat.len() as u32).to_le_bytes());
            tab.extend_from_slice(&self.groups.get(i).copied().unwrap_or(0).to_le_bytes());
            dat.extend_from_slice(&[0, img.mode]);
            dat.extend_from_slice(&(img.w as u16).to_le_bytes());
            dat.extend_from_slice(&(img.h as u16).to_le_bytes());
            let mut pix = img.pix.clone();
            pix.resize((img.w * img.h) as usize, 0);
            dat.extend_from_slice(&pix);
        }
        tab.extend_from_slice(&0u32.to_le_bytes());
        tab.extend_from_slice(&(dat.len() as u32).to_le_bytes());
        tab.extend_from_slice(&[0, 0]);
        (dat, tab)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmaps_round_trip() {
        crate::files::set_data_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources").into());
        let Ok(a) = Tmaps::load() else { return };
        assert_eq!(a.images.len(), 48);
        let (dat, tab) = a.encode();
        let b = Tmaps::decode(&dat, &tab).unwrap();
        assert_eq!(a.groups, b.groups);
        for (x, y) in a.images.iter().zip(&b.images) {
            assert_eq!((x.mode, x.w, x.h, &x.pix), (y.mode, y.w, y.h, &y.pix));
        }
    }
}
