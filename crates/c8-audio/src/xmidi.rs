//! XMIDI (Miles AIL extended MIDI) parser (spec 01 §11.2).
//!
//! Time is counted in 1/120 s intervals; a note-on carries its duration; there are no
//! note-offs and no running status.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ev {
    /// Note-on with its duration in intervals.
    Note { ch: u8, note: u8, vel: u8, dur: u32 },
    Ctrl { ch: u8, ctrl: u8, val: u8 },
    Program { ch: u8, prog: u8 },
    /// 14-bit pitch bend, 0x2000 = centre.
    Bend { ch: u8, val: u16 },
    /// A meta event the driver ignores (tempo, time signature, …) whose data fits in 8 bytes;
    /// kept so that tunes survive a read and write unchanged.
    Meta { ty: u8, len: u8, data: [u8; 8] },
    /// Anything else the driver ignores (aftertouch, sysex, long meta events); not written back.
    Other,
    /// End of track (meta 0x2F).
    End,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct XEvent {
    /// Intervals to wait before this event.
    pub delta: u32,
    pub ev: Ev,
}

#[derive(Clone, Debug, Default)]
pub struct Sequence {
    /// TIMB chunk: (patch, bank) pairs.
    pub timbres: Vec<(u8, u8)>,
    pub events: Vec<XEvent>,
}

impl Sequence {
    /// Total length in intervals (ignoring loops).
    pub fn length(&self) -> u64 {
        self.events.iter().map(|e| e.delta as u64).sum()
    }

    /// The (patch, bank) pairs the events use, for the TIMB chunk: every program on its channel's
    /// bank (controller 114), and every percussion note on channel 10 as (note, 127).
    pub fn used_timbres(&self) -> Vec<(u8, u8)> {
        let mut bank = [0u8; 16];
        let mut prog = [None::<u8>; 16];
        let mut out: Vec<(u8, u8)> = Vec::new();
        let mut add = |t: (u8, u8)| {
            if !out.contains(&t) {
                out.push(t);
            }
        };
        for e in &self.events {
            match e.ev {
                Ev::Ctrl { ch, ctrl: 114, val } => bank[ch as usize] = val,
                Ev::Program { ch, prog: p } => prog[ch as usize] = Some(p),
                Ev::Note { ch: 9, note, .. } => add((note, 127)),
                Ev::Note { ch, .. } => add((prog[ch as usize].unwrap_or(0), bank[ch as usize])),
                _ => {}
            }
        }
        out
    }
}

fn be32(d: &[u8], o: usize) -> usize {
    u32::from_be_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]) as usize
}

/// Walk an IFF chunk list, collecting every FORM XMID's TIMB/EVNT.
fn walk(d: &[u8], mut p: usize, end: usize, out: &mut Vec<Sequence>, cur: &mut Option<Sequence>) {
    while p + 8 <= end {
        let id = &d[p..p + 4];
        let len = be32(d, p + 4);
        let body = p + 8;
        let bend = body.saturating_add(len).min(end);
        match id {
            b"FORM" | b"CAT " | b"LIST" if bend >= body + 4 => {
                let ty = &d[body..body + 4];
                if ty == b"XMID" && id == b"FORM" {
                    let mut seq = Some(Sequence::default());
                    walk(d, body + 4, bend, out, &mut seq);
                    if let Some(s) = seq {
                        out.push(s);
                    }
                } else {
                    walk(d, body + 4, bend, out, cur);
                }
            }
            b"TIMB" => {
                if let Some(s) = cur.as_mut() {
                    if bend >= body + 2 {
                        let n = u16::from_le_bytes([d[body], d[body + 1]]) as usize;
                        for i in 0..n {
                            let o = body + 2 + i * 2;
                            if o + 2 <= bend {
                                s.timbres.push((d[o], d[o + 1]));
                            }
                        }
                    }
                }
            }
            b"EVNT" => {
                if let Some(s) = cur.as_mut() {
                    s.events = parse_events(&d[body..bend]);
                }
            }
            _ => {}
        }
        p = body.saturating_add(len + (len & 1));
    }
}

fn vlq(d: &[u8], p: &mut usize) -> u32 {
    let mut v = 0u32;
    while *p < d.len() {
        let b = d[*p];
        *p += 1;
        v = (v << 7) | (b & 0x7f) as u32;
        if b & 0x80 == 0 {
            break;
        }
    }
    v
}

fn parse_events(d: &[u8]) -> Vec<XEvent> {
    let mut evs = Vec::new();
    let mut p = 0;
    let mut delta = 0u32;
    while p < d.len() {
        let b = d[p];
        if b < 0x80 {
            delta += b as u32;
            p += 1;
            continue;
        }
        p += 1;
        let ch = b & 0x0f;
        let arg = |p: usize| -> u8 { if p < d.len() { d[p] & 0x7f } else { 0 } };
        let ev = match b & 0xf0 {
            0x80 => {
                p += 2;
                Ev::Other
            }
            0x90 => {
                let note = arg(p);
                let vel = arg(p + 1);
                p += 2;
                let dur = vlq(d, &mut p);
                Ev::Note { ch, note, vel, dur }
            }
            0xa0 => {
                p += 2;
                Ev::Other
            }
            0xb0 => {
                let e = Ev::Ctrl { ch, ctrl: arg(p), val: arg(p + 1) };
                p += 2;
                e
            }
            0xc0 => {
                let e = Ev::Program { ch, prog: arg(p) };
                p += 1;
                e
            }
            0xd0 => {
                p += 1;
                Ev::Other
            }
            0xe0 => {
                let e = Ev::Bend { ch, val: arg(p) as u16 | (arg(p + 1) as u16) << 7 };
                p += 2;
                e
            }
            _ => match b {
                0xff => {
                    let ty = arg(p);
                    p += 1;
                    let len = vlq(d, &mut p) as usize;
                    let start = p.min(d.len());
                    let body = &d[start..(start + len).min(d.len())];
                    p += len;
                    if ty == 0x2f {
                        Ev::End
                    } else if len <= 8 && body.len() == len {
                        let mut data = [0u8; 8];
                        data[..len].copy_from_slice(body);
                        Ev::Meta { ty, len: len as u8, data }
                    } else {
                        Ev::Other
                    }
                }
                0xf0 | 0xf7 => {
                    let len = vlq(d, &mut p) as usize;
                    p += len;
                    Ev::Other
                }
                _ => Ev::Other,
            },
        };
        evs.push(XEvent { delta, ev });
        delta = 0;
        if ev == Ev::End {
            break;
        }
    }
    if delta > 0 || evs.last().map(|e| e.ev) != Some(Ev::End) {
        evs.push(XEvent { delta, ev: Ev::End });
    }
    evs
}

/// Parse an XMIDI file; returns its sequences (usually one).
pub fn parse(d: &[u8]) -> Result<Vec<Sequence>, String> {
    let mut out = Vec::new();
    let mut cur = None;
    walk(d, 0, d.len(), &mut out, &mut cur);
    if out.is_empty() {
        return Err("no XMID sequence".into());
    }
    Ok(out)
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

fn put_chunk(out: &mut Vec<u8>, id: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(id);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
    if body.len() & 1 == 1 {
        out.push(0);
    }
}

/// Encode an event list as an EVNT chunk body. `Other` events are dropped (their delay moves to
/// the next event); an end-of-track event is added when missing.
pub fn write_events(events: &[XEvent]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut carry = 0u32;
    let mut ended = false;
    for e in events {
        let mut delta = carry + e.delta;
        carry = 0;
        if e.ev == Ev::Other {
            carry = delta;
            continue;
        }
        while delta > 0 {
            let b = delta.min(0x7f);
            out.push(b as u8);
            delta -= b;
        }
        match e.ev {
            Ev::Note { ch, note, vel, dur } => {
                out.extend_from_slice(&[0x90 | (ch & 15), note & 0x7f, vel & 0x7f]);
                put_vlq(&mut out, dur);
            }
            Ev::Ctrl { ch, ctrl, val } => out.extend_from_slice(&[0xb0 | (ch & 15), ctrl & 0x7f, val & 0x7f]),
            Ev::Program { ch, prog } => out.extend_from_slice(&[0xc0 | (ch & 15), prog & 0x7f]),
            Ev::Bend { ch, val } => out.extend_from_slice(&[0xe0 | (ch & 15), (val & 0x7f) as u8, (val >> 7 & 0x7f) as u8]),
            Ev::Meta { ty, len, data } => {
                out.extend_from_slice(&[0xff, ty & 0x7f]);
                let len = (len as usize).min(8);
                put_vlq(&mut out, len as u32);
                out.extend_from_slice(&data[..len]);
            }
            Ev::End => {
                out.extend_from_slice(&[0xff, 0x2f, 0x00]);
                ended = true;
                break;
            }
            Ev::Other => {}
        }
    }
    if !ended {
        while carry > 0 {
            let b = carry.min(0x7f);
            out.push(b as u8);
            carry -= b;
        }
        out.extend_from_slice(&[0xff, 0x2f, 0x00]);
    }
    out
}

/// Write an XMIDI file: `FORM XDIR` (`INFO` = sequence count) and `CAT XMID` with one
/// `FORM XMID` (`TIMB`, `EVNT`) per sequence. A sequence with an empty timbre list gets the
/// timbres its events use.
pub fn write(seqs: &[Sequence]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut xdir = b"XDIR".to_vec();
    put_chunk(&mut xdir, b"INFO", &(seqs.len() as u16).to_le_bytes());
    put_chunk(&mut out, b"FORM", &xdir);
    let mut cat = b"XMID".to_vec();
    for s in seqs {
        let timbres = if s.timbres.is_empty() { s.used_timbres() } else { s.timbres.clone() };
        let mut timb = (timbres.len() as u16).to_le_bytes().to_vec();
        for (p, b) in &timbres {
            timb.extend_from_slice(&[*p, *b]);
        }
        let mut form = b"XMID".to_vec();
        put_chunk(&mut form, b"TIMB", &timb);
        put_chunk(&mut form, b"EVNT", &write_events(&s.events));
        put_chunk(&mut cat, b"FORM", &form);
    }
    put_chunk(&mut out, b"CAT ", &cat);
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The shipped FM tunes (MUSIC.DAT slot 2), when the original data is present.
    pub(crate) fn shipped_tunes() -> Vec<(String, Vec<u8>)> {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/SOUND/MUSIC.DAT");
        let Ok(file) = std::fs::read(p) else { return Vec::new() };
        crate::container::load_bank(&file, crate::container::MUSIC_SLOTS, crate::container::MUSIC_SLOT, 0)
            .map(|v| v.into_iter().map(|i| (i.name, i.data)).collect())
            .unwrap_or_default()
    }

    #[test]
    fn shipped_tunes_round_trip() {
        for (name, data) in shipped_tunes() {
            let a = parse(&data).unwrap();
            let written = write(&a);
            let b = parse(&written).unwrap();
            assert_eq!(a.len(), b.len(), "{name}");
            for (x, y) in a.iter().zip(&b) {
                assert_eq!(x.timbres, y.timbres, "{name} timbres");
                assert!(!x.events.iter().any(|e| e.ev == Ev::Other), "{name} has events the writer drops");
                assert_eq!(x.events, y.events, "{name} events");
            }
            // the shipped files carry nothing else: same size up to trailing padding
            assert!(written.len() <= data.len(), "{name}: {} > {}", written.len(), data.len());
        }
    }

    #[test]
    fn long_delays_and_durations() {
        let s = Sequence {
            timbres: vec![],
            events: vec![
                XEvent { delta: 0, ev: Ev::Program { ch: 2, prog: 33 } },
                XEvent { delta: 1000, ev: Ev::Note { ch: 2, note: 40, vel: 100, dur: 20000 } },
                XEvent { delta: 0, ev: Ev::Note { ch: 9, note: 36, vel: 127, dur: 5 } },
                XEvent { delta: 3, ev: Ev::Other },
                XEvent { delta: 4, ev: Ev::Bend { ch: 2, val: 0x2345 } },
                XEvent { delta: 300, ev: Ev::End },
            ],
        };
        let b = &parse(&write(std::slice::from_ref(&s))).unwrap()[0];
        assert_eq!(b.timbres, vec![(33, 0), (36, 127)]);
        let want: Vec<XEvent> = vec![s.events[0], s.events[1], s.events[2], XEvent { delta: 7, ev: s.events[4].ev }, s.events[5]];
        assert_eq!(b.events, want);
        assert_eq!(b.length(), s.length());
    }
}
