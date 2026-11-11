//! OGG Vorbis music (a port addition): decoding with loop points, and the player.
//!
//! Loop points come from the Vorbis comments `LOOPSTART` and `LOOPLENGTH` (or `LOOPEND`), in
//! samples (frames) from the start of the file, keys case-insensitive. `LOOPEND` is the
//! exclusive end: the frame at which playback jumps back to `LOOPSTART`. Without `LOOPSTART`
//! the whole file loops. The decoded audio is kept in memory as stereo floats at the file's
//! own rate and resampled to the output rate while playing (cubic interpolation).
use lewton::inside_ogg::OggStreamReader;
use lewton::samples::InterleavedSamples;
use std::io::Cursor;
use std::sync::Arc;

/// Gain applied to OGG music before the music volume: a full-scale recording then peaks where
/// the FM tunes do (about −6 dBFS), leaving headroom for the sound effects.
pub const OGG_GAIN: f32 = 0.5;

/// A decoded OGG tune.
#[derive(Clone, Debug)]
pub struct OggTune {
    pub rate: u32,
    /// Channels in the file (the audio is always kept as stereo).
    pub channels: u8,
    /// Interleaved L R.
    pub frames: Vec<f32>,
    /// Loop region in frames: [start, end).
    pub loop_start: u64,
    pub loop_end: u64,
    /// The loop came from the file's comments (else it is the whole file).
    pub tagged: bool,
    pub comments: Vec<(String, String)>,
}

impl OggTune {
    pub fn len(&self) -> u64 {
        (self.frames.len() / 2) as u64
    }
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
    pub fn seconds(&self) -> f64 {
        self.len() as f64 / self.rate.max(1) as f64
    }
}

fn comment(comments: &[(String, String)], key: &str) -> Option<u64> {
    comments.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).and_then(|(_, v)| v.trim().parse().ok())
}

/// The loop region from the comments for a file of `total` frames: (start, end, tagged).
pub fn loop_points(comments: &[(String, String)], total: u64) -> (u64, u64, bool) {
    let Some(start) = comment(comments, "LOOPSTART") else { return (0, total, false) };
    let end = match (comment(comments, "LOOPLENGTH"), comment(comments, "LOOPEND")) {
        (Some(len), _) => start.saturating_add(len),
        (None, Some(end)) => end,
        (None, None) => total,
    };
    let end = end.min(total);
    if start >= end { (0, total, false) } else { (start, end, true) }
}

/// Decode a whole OGG Vorbis file.
pub fn decode(bytes: &[u8]) -> Result<OggTune, String> {
    let mut rd = OggStreamReader::new(Cursor::new(bytes)).map_err(|e| format!("not an OGG Vorbis file: {e}"))?;
    let rate = rd.ident_hdr.audio_sample_rate;
    let channels = rd.ident_hdr.audio_channels;
    if rate == 0 || channels == 0 {
        return Err("OGG file without audio".into());
    }
    let comments = rd.comment_hdr.comment_list.clone();
    let n = channels as usize;
    let mut frames = Vec::new();
    loop {
        let pck = match rd.read_dec_packet_generic::<InterleavedSamples<f32>>() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(e) => {
                if frames.is_empty() {
                    return Err(format!("OGG decoding failed: {e}"));
                }
                log::warn!("OGG decoding stopped early: {e}");
                break;
            }
        };
        for f in pck.samples.chunks_exact(n) {
            // Vorbis channel order: 1 mono, 2 L R, 3 L C R, 4 FL FR RL RR, 5 FL C FR RL RR, 6+ like 5 + LFE…
            let (l, r) = match n {
                1 => (f[0], f[0]),
                2 => (f[0], f[1]),
                3 => (f[0] + 0.7 * f[1], f[2] + 0.7 * f[1]),
                4 => (f[0] + 0.7 * f[2], f[1] + 0.7 * f[3]),
                _ => (f[0] + 0.7 * f[1] + 0.7 * f[3], f[2] + 0.7 * f[1] + 0.7 * f[4]),
            };
            frames.push(l);
            frames.push(r);
        }
    }
    // the last page's granule position is the exact length (the decoder may give a little more
    // when the whole stream sits in one page)
    if let Some(g) = last_granule(bytes) {
        if (g as usize) * 2 < frames.len() {
            frames.truncate(g as usize * 2);
        }
    }
    if n > 2 {
        let peak = frames.iter().fold(0f32, |a, x| a.max(x.abs()));
        if peak > 1.0 {
            frames.iter_mut().for_each(|x| *x /= peak);
        }
    }
    let total = (frames.len() / 2) as u64;
    let (loop_start, loop_end, tagged) = loop_points(&comments, total);
    Ok(OggTune { rate, channels, frames, loop_start, loop_end, tagged, comments })
}

/// Granule position of the last page of the stream (a page with the end-of-stream flag).
fn last_granule(d: &[u8]) -> Option<u64> {
    let mut p = d.len().checked_sub(27)?;
    loop {
        if &d[p..p + 4] == b"OggS" && d[p + 4] == 0 && d[p + 5] & 4 != 0 {
            let g = u64::from_le_bytes(d[p + 6..p + 14].try_into().ok()?);
            return (g != u64::MAX).then_some(g);
        }
        p = p.checked_sub(1)?;
    }
}

/// What [`probe`] finds without decoding the audio.
#[derive(Clone, Debug)]
pub struct OggInfo {
    pub rate: u32,
    pub channels: u8,
    /// Length in frames (from the last page's granule position).
    pub frames: u64,
    pub comments: Vec<(String, String)>,
    /// Loop region [start, end) and whether it came from the comments.
    pub loop_start: u64,
    pub loop_end: u64,
    pub tagged: bool,
}

/// Read the stream facts, comments and loop points without decoding the audio.
pub fn probe(bytes: &[u8]) -> Result<OggInfo, String> {
    let rd = OggStreamReader::new(Cursor::new(bytes)).map_err(|e| format!("not an OGG Vorbis file: {e}"))?;
    let frames = last_granule(bytes).unwrap_or(0);
    let comments = rd.comment_hdr.comment_list.clone();
    let (loop_start, loop_end, tagged) = loop_points(&comments, frames);
    Ok(OggInfo { rate: rd.ident_hdr.audio_sample_rate, channels: rd.ident_hdr.audio_channels, frames, comments, loop_start, loop_end, tagged })
}

/// The file with its loop comments replaced: `Some((start, end))` writes `LOOPSTART` and
/// `LOOPLENGTH` (frames), `None` removes every loop comment (the whole file loops). The audio
/// packets and pages are copied unchanged.
pub fn set_loop_tags(bytes: &[u8], lp: Option<(u64, u64)>) -> Result<Vec<u8>, String> {
    use ogg::reading::PacketReader;
    use ogg::writing::{PacketWriteEndInfo, PacketWriter};
    let mut rd = PacketReader::new(Cursor::new(bytes));
    let mut out = PacketWriter::new(Vec::with_capacity(bytes.len() + 64));
    let mut index = 0;
    let mut serial = None;
    while let Some(p) = rd.read_packet().map_err(|e| format!("OGG: {e}"))? {
        if serial.is_some_and(|s| s != p.stream_serial()) {
            return Err("OGG files with several streams are not supported".into());
        }
        serial = Some(p.stream_serial());
        let mut data = p.data.clone();
        if index == 1 {
            data = rewrite_comments(&data, lp)?;
        }
        // the setup header (packet 2) must end its page, as the identification header does
        let info = if p.last_in_stream() {
            PacketWriteEndInfo::EndStream
        } else if p.last_in_page() || index <= 2 {
            PacketWriteEndInfo::EndPage
        } else {
            PacketWriteEndInfo::NormalPacket
        };
        let gp = if index <= 2 { 0 } else { p.absgp_page() };
        out.write_packet(data.into_boxed_slice(), p.stream_serial(), info, gp).map_err(|e| e.to_string())?;
        index += 1;
    }
    if index < 3 {
        return Err("OGG file without Vorbis headers".into());
    }
    Ok(out.into_inner())
}

fn rewrite_comments(d: &[u8], lp: Option<(u64, u64)>) -> Result<Vec<u8>, String> {
    let bad = || "bad Vorbis comment header".to_string();
    if d.len() < 7 + 8 || &d[..7] != b"\x03vorbis" {
        return Err(bad());
    }
    let u32at = |o: usize| -> Result<usize, String> { d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize).ok_or_else(bad) };
    let mut p = 7;
    let vlen = u32at(p)?;
    let vendor = d.get(p + 4..p + 4 + vlen).ok_or_else(bad)?.to_vec();
    p += 4 + vlen;
    let n = u32at(p)?;
    p += 4;
    let mut list: Vec<Vec<u8>> = Vec::new();
    for _ in 0..n {
        let l = u32at(p)?;
        let c = d.get(p + 4..p + 4 + l).ok_or_else(bad)?.to_vec();
        p += 4 + l;
        let key = c.split(|&b| b == b'=').next().unwrap_or(&[]).to_ascii_uppercase();
        if !matches!(key.as_slice(), b"LOOPSTART" | b"LOOPLENGTH" | b"LOOPEND") {
            list.push(c);
        }
    }
    if let Some((s, e)) = lp {
        list.push(format!("LOOPSTART={s}").into_bytes());
        list.push(format!("LOOPLENGTH={}", e.saturating_sub(s)).into_bytes());
    }
    let mut out = b"\x03vorbis".to_vec();
    out.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    out.extend_from_slice(&vendor);
    out.extend_from_slice(&(list.len() as u32).to_le_bytes());
    for c in &list {
        out.extend_from_slice(&(c.len() as u32).to_le_bytes());
        out.extend_from_slice(c);
    }
    out.push(1); // framing bit
    Ok(out)
}

/// Plays an [`OggTune`] at the output rate, looping its loop region.
pub struct OggPlayer {
    tune: Arc<OggTune>,
    /// Position in source frames.
    pos: f64,
    step: f64,
    pub looping: bool,
    finished: bool,
}

impl OggPlayer {
    pub fn new(tune: Arc<OggTune>, out_rate: u32) -> OggPlayer {
        let step = tune.rate as f64 / out_rate.max(1) as f64;
        OggPlayer { tune, pos: 0.0, step, looping: true, finished: false }
    }

    pub fn tune(&self) -> &Arc<OggTune> {
        &self.tune
    }

    pub fn finished(&self) -> bool {
        self.finished
    }

    /// Jump to `secs` from the start of the file.
    pub fn seek(&mut self, secs: f64) {
        self.pos = (secs * self.tune.rate as f64).clamp(0.0, self.tune.len() as f64);
        self.finished = false;
    }

    /// Position in seconds from the start of the file.
    pub fn position(&self) -> f64 {
        self.pos / self.tune.rate.max(1) as f64
    }

    /// Source frame `i`, following the loop.
    #[inline]
    fn frame(&self, i: i64) -> (f32, f32) {
        let t = &self.tune;
        let mut i = i.max(0) as u64;
        if self.looping && i >= t.loop_end {
            let len = t.loop_end - t.loop_start;
            i = t.loop_start + (i - t.loop_end) % len.max(1);
        }
        if i >= t.len() {
            return (0.0, 0.0);
        }
        let k = i as usize * 2;
        (t.frames[k], t.frames[k + 1])
    }

    /// Mix `out.len() / 2` stereo frames into `out` (interleaved L R), scaled by `gain`.
    pub fn render(&mut self, out: &mut [f32], gain: f32, stereo: bool) {
        let t = self.tune.clone();
        if t.is_empty() || self.finished {
            self.finished = true;
            return;
        }
        let (ls, le) = (t.loop_start as f64, t.loop_end as f64);
        for f in out.chunks_exact_mut(2) {
            if self.looping && self.pos >= le {
                self.pos = ls + (self.pos - le) % (le - ls).max(1.0);
            }
            if !self.looping && self.pos >= t.len() as f64 {
                self.finished = true;
                break;
            }
            let i = self.pos.floor() as i64;
            let x = (self.pos - i as f64) as f32;
            let (a, b, c, d) = (self.frame(i - 1), self.frame(i), self.frame(i + 1), self.frame(i + 2));
            // Catmull-Rom
            let cr = |p0: f32, p1: f32, p2: f32, p3: f32| {
                p1 + 0.5 * x * (p2 - p0 + x * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3 + x * (3.0 * (p1 - p2) + p3 - p0)))
            };
            let l = cr(a.0, b.0, c.0, d.0);
            let r = cr(a.1, b.1, c.1, d.1);
            if stereo {
                f[0] += l * gain;
                f[1] += r * gain;
            } else {
                let m = (l + r) * 0.5 * gain;
                f[0] += m;
                f[1] += m;
            }
            self.pos += self.step;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny OGG made for these tests (sox, from a generated tone): 22050 Hz stereo, 11025
    /// frames (0.5 s), a 440 Hz tone on the left and 660 Hz on the right, with the comments
    /// LOOPSTART=2205 and LOOPLENGTH=6615 and TITLE=hio test tone.
    const FIXTURE: &[u8] = include_bytes!("../tests/data/loop-test.ogg");

    #[test]
    fn decodes_fixture_with_loop_points() {
        let t = decode(FIXTURE).unwrap();
        assert_eq!((t.rate, t.channels), (22050, 2));
        assert_eq!(t.len(), 11025);
        assert_eq!((t.loop_start, t.loop_end, t.tagged), (2205, 2205 + 6615, true));
        let info = probe(FIXTURE).unwrap();
        assert_eq!((info.rate, info.channels, info.frames, info.loop_start, info.loop_end), (22050, 2, 11025, 2205, 8820));
        let rms = |ch: usize| (t.frames.iter().skip(ch).step_by(2).map(|x| x * x).sum::<f32>() / t.len() as f32).sqrt();
        assert!(rms(0) > 0.2 && rms(1) > 0.2, "{} {}", rms(0), rms(1));
        // left and right are different tones: count zero crossings over the file
        let zc = |ch: usize| t.frames.iter().skip(ch).step_by(2).collect::<Vec<_>>().windows(2).filter(|w| (*w[0] < 0.0) != (*w[1] < 0.0)).count();
        assert!((zc(0) as i32 - 440).abs() < 6, "{}", zc(0));
        assert!((zc(1) as i32 - 660).abs() < 6, "{}", zc(1));
    }

    #[test]
    fn loop_comment_rules() {
        let c = |v: &[(&str, &str)]| v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
        assert_eq!(loop_points(&c(&[]), 1000), (0, 1000, false));
        assert_eq!(loop_points(&c(&[("loopstart", "100"), ("LoopLength", "300")]), 1000), (100, 400, true));
        assert_eq!(loop_points(&c(&[("LOOPSTART", "100"), ("LOOPEND", "900")]), 1000), (100, 900, true));
        assert_eq!(loop_points(&c(&[("LOOPSTART", "100")]), 1000), (100, 1000, true));
        assert_eq!(loop_points(&c(&[("LOOPSTART", "100"), ("LOOPLENGTH", "5000")]), 1000), (100, 1000, true));
        assert_eq!(loop_points(&c(&[("LOOPSTART", "2000")]), 1000), (0, 1000, false));
    }

    #[test]
    fn rewrite_loop_tags() {
        let new = set_loop_tags(FIXTURE, Some((100, 5100))).unwrap();
        let t = decode(&new).unwrap();
        assert_eq!((t.loop_start, t.loop_end, t.tagged), (100, 5100, true));
        assert!(t.comments.iter().any(|(k, v)| k.eq_ignore_ascii_case("title") && v == "hio test tone"));
        assert_eq!(t.frames, decode(FIXTURE).unwrap().frames);
        let none = decode(&set_loop_tags(&new, None).unwrap()).unwrap();
        assert_eq!((none.loop_start, none.loop_end, none.tagged), (0, 11025, false));
    }

    #[test]
    fn player_loops_and_resamples() {
        let t = Arc::new(decode(FIXTURE).unwrap());
        // at 44100 Hz the 0.5 s file lasts 22050 output frames; play 2 s, looping the region
        let mut p = OggPlayer::new(t.clone(), 44100);
        let mut out = vec![0f32; 44100 * 2 * 2];
        p.render(&mut out, 1.0, true);
        assert!(!p.finished());
        // after the first pass through the loop end, the position stays inside the loop
        let pos = p.position() * 22050.0;
        assert!(pos >= t.loop_start as f64 && pos < t.loop_end as f64, "{pos}");
        // no silent gap anywhere (a 440 Hz tone never stays near zero for 2 ms)
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        for w in left.chunks(88) {
            assert!(w.iter().any(|x| x.abs() > 0.05));
        }
        // without looping the player finishes after the file
        let mut p = OggPlayer::new(t, 44100);
        p.looping = false;
        let mut out = vec![0f32; 30000 * 2];
        p.render(&mut out, 1.0, false);
        assert!(p.finished());
        assert!(out[2 * 22100..].iter().all(|&x| x == 0.0));
        assert_eq!(out[1000], out[1001], "mono output");
    }
}
