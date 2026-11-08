//! Race sound dispatcher (spec 09 §3).
use super::math::*;
use super::sim::*;
use crate::audio::{Audio, MODE_ONCE, MODE_RESTART};

pub struct Listener {
    pub craft: usize,
    pub pos: [f32; 3],
    pub yaw: f32,
}

pub fn distance_volume(l: &Listener, p: [f32; 3]) -> i32 {
    let d = dist3(l.pos, p) * 256.0;
    if d < 4608.0 { 1000 + ((4608.0 - d) * 0x7c17 as f32 / 4608.0) as i32 } else { 1000 }
}

pub fn pan(l: &Listener, p: [f32; 3]) -> i32 {
    if dist2(l.pos, p) < 129.0 / 256.0 {
        return 0x8000;
    }
    let a = heading(p[0] - l.pos[0], p[1] - l.pos[1]);
    let d = diff(a, l.yaw);
    let f = if d.abs() <= 16384.0 { d } else { d.signum() * (32768.0 - d.abs()) };
    (0x8000 as f32 + 2.0 * f).clamp(0.0, 65535.0) as i32
}

pub fn dispatch(audio: &mut Audio, l: &Listener, ev: &SoundEv) {
    let viewed = ev.craft == Some(l.craft);
    let s = ev.sample;
    if ev.stop {
        audio.stop(ev.owner, s);
        return;
    }
    let pos = |audio: &mut Audio, scale: i32, looping: bool, mode: u8| {
        let vol = if viewed { 0x7fff } else { distance_volume(l, ev.pos) };
        if !viewed && vol <= 1000 {
            return;
        }
        let pn = if viewed { 0x8000 } else { pan(l, ev.pos) };
        audio.play(ev.owner, s, vol * scale / 0x8000, pn, ev.rate, looping, mode);
    };
    match s {
        1 | 7 | 8 | 15 | 17 => {
            if viewed {
                audio.update(ev.owner, s, 0x7fff, 0x8000, ev.rate, true, MODE_ONCE);
            }
        }
        2 => {}
        3 | 4 => {
            if viewed {
                audio.play(ev.owner, s, 0x7fff, 0x8000, ev.rate, false, MODE_RESTART);
            } else {
                pos(audio, 0x5dc0, false, MODE_RESTART);
            }
        }
        5 | 6 | 10..=13 | 18 => pos(audio, 0x8000, false, MODE_RESTART),
        9 | 22..=32 => {
            if viewed {
                audio.play(ev.owner, s, 0x7fff, 0x8000, ev.rate, false, MODE_ONCE);
            }
        }
        14 => {
            if viewed {
                audio.play(ev.owner, s, 0x7fff, pan(l, ev.pos), ev.rate, false, MODE_RESTART);
            }
        }
        19 => pos(audio, 0x8000, true, MODE_ONCE),
        _ => {}
    }
}

/// Engine loops (spec 09 §3.4), once per tick.
pub fn engines(audio: &mut Audio, l: &Listener, w: &World, stereo: bool) {
    let mut groups = [(0i32, 0i64, 0i32, 0i32); 2]; // volume, pan sum, n, rate
    for (i, c) in w.crafts.iter().enumerate() {
        if !c.active {
            continue;
        }
        let rate = ((c.measured_vel * 256.0).min(180.0) * 300.0 / 110.0) as i32;
        if i == l.craft {
            audio.update(1000 + i as u32, 1, 0x7fff, 0x8000, rate.max(1), true, MODE_ONCE);
            continue;
        }
        let v = distance_volume(l, c.pos);
        if v <= 1000 {
            continue;
        }
        let p = pan(l, c.pos);
        let g = if stereo && p >= 0x8000 { 1 } else { 0 };
        groups[g].0 = (groups[g].0 + v).min(0x7fff);
        groups[g].1 += p as i64;
        groups[g].2 += 1;
        groups[g].3 = rate.max(1);
    }
    for (g, &(v, ps, n, rate)) in groups.iter().enumerate() {
        let owner = 2000 + g as u32;
        if n == 0 {
            audio.stop(owner, 2);
        } else {
            let p = if stereo { (ps / n as i64) as i32 } else { 0x8000 };
            audio.update(owner, 2, v, p, rate, true, MODE_ONCE);
        }
    }
}
