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
    /// Anything the driver ignores (aftertouch, sysex, meta).
    Other,
    /// End of track (meta 0x2F).
    End,
}

#[derive(Clone, Copy, Debug)]
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
                    p += len;
                    if ty == 0x2f { Ev::End } else { Ev::Other }
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
