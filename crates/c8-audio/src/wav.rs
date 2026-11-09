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

/// Write 8-bit unsigned mono PCM as a RIFF WAVE file (the game's sample format).
pub fn write_u8(rate: u32, pcm: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(44 + pcm.len() + 1);
    o.extend_from_slice(b"RIFF");
    o.extend_from_slice(&((36 + pcm.len() + (pcm.len() & 1)) as u32).to_le_bytes());
    o.extend_from_slice(b"WAVEfmt ");
    o.extend_from_slice(&16u32.to_le_bytes());
    o.extend_from_slice(&1u16.to_le_bytes()); // PCM
    o.extend_from_slice(&1u16.to_le_bytes()); // mono
    o.extend_from_slice(&rate.to_le_bytes());
    o.extend_from_slice(&rate.to_le_bytes()); // bytes per second
    o.extend_from_slice(&1u16.to_le_bytes()); // block align
    o.extend_from_slice(&8u16.to_le_bytes());
    o.extend_from_slice(b"data");
    o.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    o.extend_from_slice(pcm);
    if pcm.len() & 1 == 1 {
        o.push(0x80);
    }
    o
}

/// Write -1..1 float samples as a 16-bit stereo or mono WAV (for exporting renders).
pub fn write_i16(rate: u32, channels: u16, data: &[f32]) -> Vec<u8> {
    let bytes = data.len() * 2;
    let mut o = Vec::with_capacity(44 + bytes);
    o.extend_from_slice(b"RIFF");
    o.extend_from_slice(&((36 + bytes) as u32).to_le_bytes());
    o.extend_from_slice(b"WAVEfmt ");
    o.extend_from_slice(&16u32.to_le_bytes());
    o.extend_from_slice(&1u16.to_le_bytes());
    o.extend_from_slice(&channels.to_le_bytes());
    o.extend_from_slice(&rate.to_le_bytes());
    o.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    o.extend_from_slice(&(channels * 2).to_le_bytes());
    o.extend_from_slice(&16u16.to_le_bytes());
    o.extend_from_slice(b"data");
    o.extend_from_slice(&(bytes as u32).to_le_bytes());
    for &s in data {
        o.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    o
}

/// Convert any WAV file (8/16-bit, mono/stereo, any rate) to 8-bit unsigned mono at `rate`, the
/// game's format. Stereo is mixed down; the rate is converted by linear interpolation.
pub fn convert_to_u8(d: &[u8], rate: u32) -> Result<Vec<u8>, String> {
    if d.len() < 12 || &d[0..4] != b"RIFF" || &d[8..12] != b"WAVE" {
        return Err("not a WAV file".into());
    }
    let s = parse_any(d)?;
    let out_len = (s.data.len() as u64 * rate as u64 / s.rate.max(1) as u64) as usize;
    let step = s.rate as f64 / rate as f64;
    Ok((0..out_len)
        .map(|i| {
            let p = i as f64 * step;
            let k = p as usize;
            let f = (p - k as f64) as f32;
            let a = s.data.get(k).copied().unwrap_or(0.0);
            let b = s.data.get(k + 1).copied().unwrap_or(a);
            ((a + (b - a) * f) * 128.0 + 128.0).round().clamp(0.0, 255.0) as u8
        })
        .collect())
}

/// Like [`parse`], but mixes all channels down instead of taking the first, and fails on
/// unsupported data instead of returning an empty sample.
fn parse_any(d: &[u8]) -> Result<Sample, String> {
    let mut bits = 0u16;
    let mut channels = 1usize;
    let mut rate = 0u32;
    let mut p = 12;
    while p + 8 <= d.len() {
        let id = &d[p..p + 4];
        let len = u32::from_le_bytes([d[p + 4], d[p + 5], d[p + 6], d[p + 7]]) as usize;
        let body = p + 8;
        let end = body.saturating_add(len).min(d.len());
        match id {
            b"fmt " if end >= body + 16 => {
                let fmt = u16::from_le_bytes([d[body], d[body + 1]]);
                if fmt != 1 && fmt != 0xfffe {
                    return Err(format!("WAV format {fmt} not supported (PCM only)"));
                }
                channels = u16::from_le_bytes([d[body + 2], d[body + 3]]).max(1) as usize;
                rate = u32::from_le_bytes([d[body + 4], d[body + 5], d[body + 6], d[body + 7]]);
                bits = u16::from_le_bytes([d[body + 14], d[body + 15]]);
            }
            b"data" => {
                let raw = &d[body..end];
                let width = match bits {
                    8 => 1,
                    16 => 2,
                    24 => 3,
                    32 => 4,
                    0 => return Err("WAV data before its format".into()),
                    b => return Err(format!("{b}-bit WAV not supported")),
                };
                let frame = |f: &[u8]| -> f32 {
                    let s: f32 = f
                        .chunks_exact(width)
                        .map(|c| match width {
                            1 => (c[0] as f32 - 128.0) / 128.0,
                            2 => i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0,
                            3 => i32::from_le_bytes([0, c[0], c[1], c[2]]) as f32 / 2147483648.0,
                            _ => i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32 / 2147483648.0,
                        })
                        .sum();
                    s / channels as f32
                };
                let data = raw.chunks_exact(width * channels).map(frame).collect();
                return Ok(Sample { name: String::new(), rate: rate.max(1), data });
            }
            _ => {}
        }
        p = body.saturating_add(len + (len & 1));
    }
    Err("WAV file has no data".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_round_trip() {
        let pcm: Vec<u8> = (0..1000).map(|i| (128.0 + 100.0 * (i as f32 * 0.05).sin()) as u8).collect();
        let w = write_u8(22050, &pcm);
        let s = parse("x", &w);
        assert_eq!((s.rate, s.data.len()), (22050, 1000));
        assert_eq!(convert_to_u8(&w, 22050).unwrap(), pcm);
        // stereo 16-bit 44.1 kHz down to 8-bit mono 11025 Hz
        let st: Vec<f32> = (0..4000).flat_map(|i| [(i as f32 * 0.01).sin() * 0.5; 2]).collect();
        let c = convert_to_u8(&write_i16(44100, 2, &st), 11025).unwrap();
        assert_eq!(c.len(), 1000);
    }
}
