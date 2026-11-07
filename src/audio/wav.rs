//! Minimal RIFF WAVE reader: only the `fmt ` and `data` chunks are used (spec 01 §11.1).

/// A decoded mono sample, normalised to -1..1.
pub struct Sample {
    pub name: String,
    pub rate: u32,
    pub data: Vec<f32>,
}

impl Sample {
    pub fn duration(&self) -> f64 {
        if self.rate == 0 { 0.0 } else { self.data.len() as f64 / self.rate as f64 }
    }
}

/// Parse a WAV file. Unknown or truncated data degrades to an empty sample.
pub fn parse(name: &str, d: &[u8]) -> Sample {
    let mut s = Sample { name: name.to_string(), rate: 22050, data: Vec::new() };
    if d.len() < 12 || &d[0..4] != b"RIFF" || &d[8..12] != b"WAVE" {
        return s;
    }
    let mut bits = 8u16;
    let mut channels = 1u16;
    let mut p = 12;
    while p + 8 <= d.len() {
        let id = &d[p..p + 4];
        let len = u32::from_le_bytes([d[p + 4], d[p + 5], d[p + 6], d[p + 7]]) as usize;
        let body = p + 8;
        let end = body.saturating_add(len).min(d.len());
        match id {
            b"fmt " if end >= body + 16 => {
                channels = u16::from_le_bytes([d[body + 2], d[body + 3]]).max(1);
                s.rate = u32::from_le_bytes([d[body + 4], d[body + 5], d[body + 6], d[body + 7]]);
                bits = u16::from_le_bytes([d[body + 14], d[body + 15]]);
            }
            b"data" => {
                let raw = &d[body..end];
                let ch = channels as usize;
                if bits == 16 {
                    s.data = raw
                        .chunks_exact(2 * ch)
                        .map(|f| i16::from_le_bytes([f[0], f[1]]) as f32 / 32768.0)
                        .collect();
                } else {
                    s.data = raw.chunks_exact(ch).map(|f| (f[0] as f32 - 128.0) / 128.0).collect();
                }
                break;
            }
            _ => {}
        }
        p = body.saturating_add(len + (len & 1));
    }
    s
}
