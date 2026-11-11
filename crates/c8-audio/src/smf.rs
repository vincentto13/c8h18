//! Standard MIDI files ⇄ XMIDI sequences.
//!
//! [`to_xmidi`] converts a `.mid` (format 0 or 1, PPQ or SMPTE timing, any tempo map) into an
//! XMIDI [`Sequence`]: note-on/note-off pairs become note-ons with a duration, ticks become
//! 1/120 s intervals through the tempo map, and the loop becomes AIL FOR/NEXT controllers
//! (116 value 0 = forever / 117 value 127). The loop is taken from, in this order:
//! - marker or text events `loopStart` and `loopEnd` (case-insensitive; also `loop_start`,
//!   `loop start`, `[` and `]`);
//! - controller 111 (a common game-MIDI loop-start convention), the loop ending at the end;
//! - otherwise the whole piece loops (FOR at the start, NEXT at the end of the last track).
//!
//! The end of the piece is the latest end-of-track event (so a final bar of silence survives).
//! Bank selects (0, 32) and the AIL-reserved controllers 110..=120 are dropped; aftertouch and
//! system exclusive messages too.
//!
//! [`write`] goes the other way, for MuseScore, LMMS or a DAW: one tick per interval, a tempo
//! that keeps it so, a conductor track (tempo, time signature, `loopStart`/`loopEnd` markers
//! for FOR/NEXT) and one track per MIDI channel.
use super::xmidi::{Ev, Sequence, XEvent};

/// How the converted tune loops.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Loop {
    /// Loop markers, else controller 111, else the whole piece.
    Auto,
    /// No FOR/NEXT (the port still restarts the tune at its end).
    None,
}

/// What [`to_xmidi`] did, for a report.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub tracks: usize,
    pub notes: usize,
    /// Length in seconds.
    pub seconds: f64,
    /// Loop start and end in seconds, if any.
    pub loop_secs: Option<(f64, f64)>,
    /// Where the loop came from.
    pub loop_source: String,
    /// Note-ons that never got a note-off (ended at the end of the piece).
    pub unterminated: usize,
    pub programs: Vec<u8>,
}

fn be16(d: &[u8], o: usize) -> u16 {
    u16::from_be_bytes([d[o], d[o + 1]])
}
fn be32(d: &[u8], o: usize) -> u32 {
    u32::from_be_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

fn vlq(d: &[u8], p: &mut usize) -> Result<u32, String> {
    let mut v = 0u32;
    for _ in 0..4 {
        let b = *d.get(*p).ok_or("truncated variable-length number")?;
        *p += 1;
        v = (v << 7) | (b & 0x7f) as u32;
        if b & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err("variable-length number too long".into())
}

#[derive(Clone, Debug)]
enum Raw {
    On { ch: u8, note: u8, vel: u8 },
    Off { ch: u8, note: u8 },
    Ctrl { ch: u8, ctrl: u8, val: u8 },
    Program { ch: u8, prog: u8 },
    Bend { ch: u8, val: u16 },
    Tempo(u32),
    Meta { ty: u8, data: Vec<u8> },
    EndOfTrack,
}

/// A parsed SMF: events with absolute ticks, in file order per track.
struct Smf {
    /// Ticks per quarter note, or 0 with `smpte_tps` set.
    ppq: u32,
    smpte_tps: f64,
    /// (tick, track, index in track, event)
    events: Vec<(u64, usize, usize, Raw)>,
    tracks: usize,
}

fn parse_smf(d: &[u8]) -> Result<Smf, String> {
    // RIFF RMID wrapper
    let d = if d.len() >= 20 && &d[..4] == b"RIFF" && &d[8..12] == b"RMID" {
        let mut p = 12;
        let mut inner = None;
        while p + 8 <= d.len() {
            let len = u32::from_le_bytes([d[p + 4], d[p + 5], d[p + 6], d[p + 7]]) as usize;
            if &d[p..p + 4] == b"data" {
                inner = Some(&d[p + 8..(p + 8 + len).min(d.len())]);
                break;
            }
            p += 8 + len + (len & 1);
        }
        inner.ok_or("RMID file without a data chunk")?
    } else {
        d
    };
    if d.len() < 14 || &d[..4] != b"MThd" {
        return Err("not a Standard MIDI file (no MThd)".into());
    }
    let hlen = be32(d, 4) as usize;
    let format = be16(d, 8);
    let ntrks = be16(d, 10) as usize;
    let div = be16(d, 12);
    if format > 2 {
        return Err(format!("unknown MIDI file format {format}"));
    }
    let (ppq, smpte_tps) = if div & 0x8000 != 0 {
        let fps = -((div >> 8) as i8) as f64;
        let fps = if fps == 29.0 { 29.97 } else { fps };
        (0, fps * (div & 0xff) as f64)
    } else {
        (div.max(1) as u32, 0.0)
    };
    let mut events = Vec::new();
    let mut p = 8 + hlen;
    let mut track = 0;
    while p + 8 <= d.len() && track < ntrks.max(1) * 4 {
        let len = be32(d, p + 4) as usize;
        let body = p + 8;
        let end = (body + len).min(d.len());
        if &d[p..p + 4] == b"MTrk" {
            parse_track(&d[body..end], track, &mut events)?;
            track += 1;
        }
        p = body + len;
    }
    if track == 0 {
        return Err("no MTrk chunk".into());
    }
    // format 2: the tracks are separate patterns played one after the other
    if format == 2 {
        let mut offset = 0u64;
        for t in 0..track {
            let mut last = 0;
            for e in events.iter_mut().filter(|e| e.1 == t) {
                e.0 += offset;
                last = last.max(e.0);
            }
            offset = last;
        }
    }
    Ok(Smf { ppq, smpte_tps, events, tracks: track })
}

fn parse_track(d: &[u8], track: usize, out: &mut Vec<(u64, usize, usize, Raw)>) -> Result<(), String> {
    let mut p = 0;
    let mut tick = 0u64;
    let mut status = 0u8;
    let mut idx = 0;
    while p < d.len() {
        tick += vlq(d, &mut p)? as u64;
        let mut b = *d.get(p).ok_or("truncated event")?;
        if b & 0x80 != 0 {
            p += 1;
            if b < 0xf0 {
                status = b;
            }
        } else if status != 0 {
            b = status; // running status: b is the first data byte, still at p
        } else {
            return Err("data byte without a status".into());
        }
        let arg = |i: usize| -> Result<u8, String> { d.get(p + i).map(|x| x & 0x7f).ok_or_else(|| "truncated event".to_string()) };
        let ch = b & 0x0f;
        let ev = match b & 0xf0 {
            0x80 => {
                let e = Raw::Off { ch, note: arg(0)? };
                p += 2;
                Some(e)
            }
            0x90 => {
                let (note, vel) = (arg(0)?, arg(1)?);
                p += 2;
                Some(if vel == 0 { Raw::Off { ch, note } } else { Raw::On { ch, note, vel } })
            }
            0xa0 => {
                p += 2;
                None
            }
            0xb0 => {
                let e = Raw::Ctrl { ch, ctrl: arg(0)?, val: arg(1)? };
                p += 2;
                Some(e)
            }
            0xc0 => {
                let e = Raw::Program { ch, prog: arg(0)? };
                p += 1;
                Some(e)
            }
            0xd0 => {
                p += 1;
                None
            }
            0xe0 => {
                let e = Raw::Bend { ch, val: arg(0)? as u16 | (arg(1)? as u16) << 7 };
                p += 2;
                Some(e)
            }
            _ => match b {
                0xff => {
                    let ty = *d.get(p).ok_or("truncated meta event")?;
                    p += 1;
                    let len = vlq(d, &mut p)? as usize;
                    let data = d.get(p..p + len).ok_or("truncated meta event")?.to_vec();
                    p += len;
                    match ty {
                        0x2f => Some(Raw::EndOfTrack),
                        0x51 if len == 3 => Some(Raw::Tempo((data[0] as u32) << 16 | (data[1] as u32) << 8 | data[2] as u32)),
                        _ => Some(Raw::Meta { ty, data }),
                    }
                }
                0xf0 | 0xf7 => {
                    let len = vlq(d, &mut p)? as usize;
                    p += len;
                    None
                }
                _ => return Err(format!("unexpected status byte {b:#04x}")),
            },
        };
        if let Some(ev) = ev {
            let end = matches!(ev, Raw::EndOfTrack);
            out.push((tick, track, idx, ev));
            idx += 1;
            if end {
                break;
            }
        }
    }
    Ok(())
}

fn is_loop_marker(data: &[u8], start: bool) -> bool {
    let t = String::from_utf8_lossy(data).trim().to_ascii_lowercase();
    if start {
        matches!(t.as_str(), "loopstart" | "loop_start" | "loop start" | "[")
    } else {
        matches!(t.as_str(), "loopend" | "loop_end" | "loop end" | "]")
    }
}

/// Convert a Standard MIDI file to an XMIDI sequence (see the module notes).
pub fn to_xmidi(d: &[u8], lp: Loop) -> Result<(Sequence, Report), String> {
    let mut smf = parse_smf(d)?;
    smf.events.sort_by_key(|e| (e.0, e.1, e.2));
    // tempo map: ticks → seconds
    let mut tempos: Vec<(u64, u32)> = vec![(0, 500_000)];
    for (t, _, _, e) in &smf.events {
        if let Raw::Tempo(us) = e {
            if *t == tempos.last().unwrap().0 {
                tempos.last_mut().unwrap().1 = (*us).max(1);
            } else {
                tempos.push((*t, (*us).max(1)));
            }
        }
    }
    let secs = |tick: u64| -> f64 {
        if smf.ppq == 0 {
            return tick as f64 / smf.smpte_tps.max(1.0);
        }
        let mut s = 0.0;
        let mut prev = (0u64, 500_000u32);
        for &(t, us) in &tempos {
            if t >= tick {
                break;
            }
            s += (t - prev.0) as f64 * prev.1 as f64 / smf.ppq as f64 / 1e6;
            prev = (t, us);
        }
        s + (tick - prev.0) as f64 * prev.1 as f64 / smf.ppq as f64 / 1e6
    };
    let iv = |tick: u64| -> u64 { (secs(tick) * 120.0).round() as u64 };

    let end_tick = smf.events.iter().map(|e| e.0).max().unwrap_or(0);
    let mut report = Report { tracks: smf.tracks, ..Default::default() };
    // loop points (ticks)
    let marker = |start: bool| {
        smf.events.iter().find_map(|(t, _, _, e)| match e {
            Raw::Meta { ty: 1 | 6, data } if is_loop_marker(data, start) => Some(*t),
            _ => None,
        })
    };
    let cc111 = smf.events.iter().find_map(|(t, _, _, e)| matches!(e, Raw::Ctrl { ctrl: 111, .. }).then_some(*t));
    let lp_ticks = match lp {
        Loop::None => None,
        Loop::Auto => match (marker(true), marker(false)) {
            (Some(s), e) => {
                report.loop_source = "loopStart/loopEnd markers".into();
                Some((s, e.unwrap_or(end_tick).max(s)))
            }
            (None, Some(e)) => {
                report.loop_source = "loopEnd marker (from the start)".into();
                Some((0, e))
            }
            (None, None) => match cc111 {
                Some(s) => {
                    report.loop_source = "controller 111".into();
                    Some((s, end_tick))
                }
                None => {
                    report.loop_source = "whole piece".into();
                    Some((0, end_tick))
                }
            },
        },
    };

    // timed events: (interval, order, ev); order keeps FOR before and NEXT after same-time notes
    let mut timed: Vec<(u64, u8, usize, Ev)> = Vec::new();
    let mut open: std::collections::HashMap<(u8, u8), std::collections::VecDeque<(usize, u64)>> = Default::default();
    let end_iv = iv(end_tick);
    for (n, (t, _, _, e)) in smf.events.iter().enumerate() {
        let at = iv(*t);
        match e {
            Raw::On { ch, note, vel } => {
                timed.push((at, 1, n, Ev::Note { ch: *ch, note: *note, vel: *vel, dur: 0 }));
                open.entry((*ch, *note)).or_default().push_back((timed.len() - 1, at));
                report.notes += 1;
            }
            Raw::Off { ch, note } => {
                if let Some((i, start)) = open.get_mut(&(*ch, *note)).and_then(|q| q.pop_front()) {
                    if let Ev::Note { dur, .. } = &mut timed[i].3 {
                        *dur = (at.saturating_sub(start)).max(1) as u32;
                    }
                }
            }
            Raw::Ctrl { ch, ctrl, val } => {
                if !matches!(ctrl, 0 | 32 | 110..=120) {
                    timed.push((at, 1, n, Ev::Ctrl { ch: *ch, ctrl: *ctrl, val: *val }));
                }
            }
            Raw::Program { ch, prog } => {
                timed.push((at, 1, n, Ev::Program { ch: *ch, prog: *prog }));
                if !report.programs.contains(prog) && *ch != 9 {
                    report.programs.push(*prog);
                }
            }
            Raw::Bend { ch, val } => timed.push((at, 1, n, Ev::Bend { ch: *ch, val: *val })),
            Raw::Tempo(us) => {
                let data = [(us >> 16) as u8, (us >> 8) as u8, *us as u8, 0, 0, 0, 0, 0];
                timed.push((at, 1, n, Ev::Meta { ty: 0x51, len: 3, data }));
            }
            Raw::Meta { ty, data } if matches!(ty, 0x58 | 0x59) && data.len() <= 8 => {
                let mut b = [0u8; 8];
                b[..data.len()].copy_from_slice(data);
                timed.push((at, 1, n, Ev::Meta { ty: *ty, len: data.len() as u8, data: b }));
            }
            _ => {}
        }
    }
    for q in open.values() {
        for &(i, start) in q {
            report.unterminated += 1;
            if let Ev::Note { dur, .. } = &mut timed[i].3 {
                *dur = end_iv.saturating_sub(start).max(1) as u32;
            }
        }
    }
    if let Some((s, e)) = lp_ticks {
        let (s, e) = (iv(s), iv(e));
        timed.push((s, 0, 0, Ev::Ctrl { ch: 0, ctrl: 116, val: 0 }));
        timed.push((e, 2, usize::MAX, Ev::Ctrl { ch: 0, ctrl: 117, val: 127 }));
        report.loop_secs = Some((s as f64 / 120.0, e as f64 / 120.0));
    }
    timed.sort_by_key(|x| (x.0, x.1, x.2));
    let mut events = Vec::with_capacity(timed.len() + 1);
    let mut now = 0u64;
    for (at, _, _, ev) in timed {
        events.push(XEvent { delta: (at - now) as u32, ev });
        now = at;
    }
    events.push(XEvent { delta: end_iv.saturating_sub(now) as u32, ev: Ev::End });
    report.seconds = end_iv.max(now) as f64 / 120.0;
    let mut seq = Sequence { timbres: Vec::new(), events };
    seq.timbres = seq.used_timbres();
    Ok((seq, report))
}

fn put_vlq(out: &mut Vec<u8>, v: u32) {
    let mut groups = vec![(v & 0x7f) as u8];
    let mut v = v >> 7;
    while v > 0 {
        groups.push((v & 0x7f) as u8 | 0x80);
        v >>= 7;
    }
    out.extend(groups.iter().rev());
}

/// Intervals per quarter note implied by a sequence's tempo event (default 120 BPM → 60).
pub fn natural_ppq(seq: &Sequence) -> u32 {
    seq.events
        .iter()
        .find_map(|e| match e.ev {
            Ev::Meta { ty: 0x51, len: 3, data } => {
                let us = (data[0] as u32) << 16 | (data[1] as u32) << 8 | data[2] as u32;
                Some(((us as f64 * 120.0 / 1e6).round() as u32).max(1))
            }
            _ => None,
        })
        .unwrap_or(60)
}

/// Write a sequence as a format-1 Standard MIDI file with `ppq` ticks per quarter note and one
/// tick per 1/120 s interval (so the tempo is 7200 / `ppq` BPM). `names` labels channel tracks.
/// FOR/NEXT loops are written as `loopStart`/`loopEnd` markers, not unrolled.
pub fn write(seq: &Sequence, ppq: u32, names: &[(u8, &str)]) -> Vec<u8> {
    let ppq = ppq.clamp(1, 0x7fff);
    // absolute times
    let mut abs = Vec::with_capacity(seq.events.len());
    let mut t = 0u64;
    for e in &seq.events {
        t += e.delta as u64;
        abs.push((t, e.ev));
    }
    let end = t;
    let mut conductor: Vec<(u64, u8, Vec<u8>)> = Vec::new();
    let tempo = (ppq as f64 * 1e6 / 120.0).round() as u32;
    conductor.push((0, 0, vec![0xff, 0x51, 3, (tempo >> 16) as u8, (tempo >> 8) as u8, tempo as u8]));
    let has_timesig = abs.iter().any(|(_, e)| matches!(e, Ev::Meta { ty: 0x58, .. }));
    if !has_timesig {
        conductor.push((0, 0, vec![0xff, 0x58, 4, 4, 2, 24, 8]));
    }
    let mut tracks: Vec<Vec<(u64, u8, Vec<u8>)>> = vec![Vec::new(); 16];
    for &(at, ev) in &abs {
        match ev {
            Ev::Note { ch, note, vel, dur } => {
                let c = ch as usize & 15;
                tracks[c].push((at, 2, vec![0x90 | ch, note, vel.max(1)]));
                tracks[c].push((at + dur.max(1) as u64, 0, vec![0x80 | ch, note, 0]));
            }
            Ev::Ctrl { ctrl: 116, .. } => conductor.push((at, 1, [vec![0xff, 6, 9], b"loopStart".to_vec()].concat())),
            Ev::Ctrl { ctrl: 117, val, .. } if val >= 64 => conductor.push((at, 1, [vec![0xff, 6, 7], b"loopEnd".to_vec()].concat())),
            Ev::Ctrl { ch, ctrl, val } if !(110..=120).contains(&ctrl) => tracks[ch as usize & 15].push((at, 1, vec![0xb0 | ch, ctrl, val])),
            Ev::Program { ch, prog } => tracks[ch as usize & 15].push((at, 1, vec![0xc0 | ch, prog])),
            Ev::Bend { ch, val } => tracks[ch as usize & 15].push((at, 1, vec![0xe0 | ch, (val & 0x7f) as u8, (val >> 7 & 0x7f) as u8])),
            Ev::Meta { ty, len, data } if ty != 0x51 && ty != 0x2f => {
                let len = (len as usize).min(8);
                let mut m = vec![0xff, ty];
                put_vlq(&mut m, len as u32);
                m.extend_from_slice(&data[..len]);
                conductor.push((at, 1, m));
            }
            _ => {}
        }
    }
    let mut chunks = vec![conductor];
    for (ch, mut tr) in tracks.into_iter().enumerate() {
        if tr.is_empty() {
            continue;
        }
        if let Some((_, name)) = names.iter().find(|(c, _)| *c as usize == ch) {
            let mut m = vec![0xff, 3];
            put_vlq(&mut m, name.len() as u32);
            m.extend_from_slice(name.as_bytes());
            tr.push((0, 0, m));
        }
        chunks.push(tr);
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"MThd");
    out.extend_from_slice(&6u32.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&(chunks.len() as u16).to_be_bytes());
    out.extend_from_slice(&(ppq as u16).to_be_bytes());
    for mut tr in chunks {
        // note-offs (0) before controls (1) before note-ons (2) at the same tick
        tr.sort_by_key(|e| (e.0, e.1));
        let mut body = Vec::new();
        let mut now = 0u64;
        for (at, _, bytes) in tr {
            put_vlq(&mut body, (at - now) as u32);
            body.extend_from_slice(&bytes);
            now = at;
        }
        put_vlq(&mut body, end.saturating_sub(now) as u32);
        body.extend_from_slice(&[0xff, 0x2f, 0]);
        out.extend_from_slice(b"MTrk");
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(&body);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xmidi;

    /// (absolute interval, event) of every note, controller, program and bend.
    fn timeline(s: &Sequence) -> Vec<(u64, Ev)> {
        let mut t = 0;
        let mut v = Vec::new();
        for e in &s.events {
            t += e.delta as u64;
            if let Ev::Note { ch, note, vel, dur } = e.ev {
                // the player plays a zero duration as one interval, and so does a MIDI file
                v.push((t, Ev::Note { ch, note, vel, dur: dur.max(1) }));
            } else if matches!(e.ev, Ev::Program { .. } | Ev::Bend { .. } | Ev::Ctrl { .. }) {
                v.push((t, e.ev));
            }
        }
        v.sort_by_key(|(t, e)| (*t, format!("{e:?}")));
        v
    }

    #[test]
    fn shipped_tunes_survive_midi() {
        for (name, data) in crate::xmidi::tests::shipped_tunes() {
            let a = xmidi::parse(&data).unwrap().swap_remove(0);
            let mid = write(&a, natural_ppq(&a), &[(0, "zero")]);
            let (b, rep) = to_xmidi(&mid, Loop::Auto).unwrap();
            assert_eq!(rep.loop_source, "loopStart/loopEnd markers", "{name}");
            // the loop controllers come back on channel 0; compare without them
            let strip = |v: Vec<(u64, Ev)>| v.into_iter().filter(|(_, e)| !matches!(e, Ev::Ctrl { ctrl: 116 | 117 | 119, .. })).collect::<Vec<_>>();
            let (x, y) = (strip(timeline(&a)), strip(timeline(&b)));
            if let Some(i) = (0..x.len().min(y.len())).find(|&i| x[i] != y[i]) {
                panic!("{name}: event {i}: {:?} vs {:?}", &x[i..(i + 3).min(x.len())], &y[i..(i + 3).min(y.len())]);
            }
            assert_eq!(x.len(), y.len(), "{name}");
            assert_eq!(a.length(), b.length(), "{name}");
            let lp = |s: &Sequence| {
                let mut t = 0;
                let mut v = vec![];
                for e in &s.events {
                    t += e.delta as u64;
                    if let Ev::Ctrl { ctrl: c @ (116 | 117), .. } = e.ev {
                        v.push((t, c));
                    }
                }
                v
            };
            assert_eq!(lp(&a), lp(&b), "{name} loop");
        }
    }

    /// A hand-made type-0 file: running status, note-on velocity 0 as note-off, a tempo change.
    #[test]
    fn converts_ticks_tempo_and_loop() {
        let mut trk = Vec::new();
        let ev = |trk: &mut Vec<u8>, d: u32, b: &[u8]| {
            put_vlq(trk, d);
            trk.extend_from_slice(b);
        };
        ev(&mut trk, 0, &[0xff, 0x51, 3, 0x07, 0xa1, 0x20]); // 500000 us = 120 BPM
        ev(&mut trk, 0, &[0xc0, 38]);
        ev(&mut trk, 0, &[0x90, 40, 100]); // t=0
        ev(&mut trk, 96, &[40, 0]); // running status, off after a quarter = 0.5 s
        ev(&mut trk, 0, &[0xff, 6, 9]);
        trk.extend_from_slice(b"loopStart");
        ev(&mut trk, 0, &[0xff, 0x51, 3, 0x03, 0xd0, 0x90]); // 250000 us = 240 BPM
        ev(&mut trk, 0, &[0x99, 36, 127]); // drum at 0.5 s
        ev(&mut trk, 96, &[0x89, 36, 0]); // a quarter at 240 BPM = 0.25 s
        ev(&mut trk, 96, &[0xff, 0x2f, 0]); // end at 1.0 s
        let mut f = b"MThd".to_vec();
        f.extend_from_slice(&[0, 0, 0, 6, 0, 0, 0, 1, 0, 96]);
        f.extend_from_slice(b"MTrk");
        f.extend_from_slice(&(trk.len() as u32).to_be_bytes());
        f.extend_from_slice(&trk);
        let (s, rep) = to_xmidi(&f, Loop::Auto).unwrap();
        assert_eq!(rep.notes, 2);
        assert_eq!(rep.loop_secs, Some((0.5, 1.0)));
        assert_eq!(s.timbres, vec![(38, 0), (36, 127)]);
        let tl = timeline(&s);
        assert!(tl.contains(&(0, Ev::Note { ch: 0, note: 40, vel: 100, dur: 60 })));
        assert!(tl.contains(&(60, Ev::Note { ch: 9, note: 36, vel: 127, dur: 30 })));
        assert!(tl.contains(&(60, Ev::Ctrl { ch: 0, ctrl: 116, val: 0 })));
        assert!(tl.contains(&(120, Ev::Ctrl { ch: 0, ctrl: 117, val: 127 })));
        assert_eq!(s.length(), 120);
        // FOR comes before the drum it loops back to
        let pos = |f: &dyn Fn(&Ev) -> bool| s.events.iter().position(|e| f(&e.ev)).unwrap();
        assert!(pos(&|e| matches!(e, Ev::Ctrl { ctrl: 116, .. })) < pos(&|e| matches!(e, Ev::Note { ch: 9, .. })));
        // the XMIDI writer and reader keep it
        assert_eq!(xmidi::parse(&xmidi::write(std::slice::from_ref(&s))).unwrap()[0].events, s.events);
        assert!(to_xmidi(b"MThd", Loop::Auto).is_err());
    }
}
