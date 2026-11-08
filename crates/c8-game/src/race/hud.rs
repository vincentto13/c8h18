//! HUD, minimap, target marker, start lights, banners, big messages (spec 08 §11-12, 04 §12-13).
use super::math::Lcg;
use super::sim::*;
use crate::assets::Assets;
use crate::data::strings;
use crate::gfx::canvas::{Blit, Canvas};

type G = (i32, i32, usize, i32);
const FUEL: [G; 17] = [(257,2,17,0),(244,2,16,384),(232,2,15,768),(220,2,14,1152),(207,2,13,1536),(195,2,12,1920),(183,2,11,2304),(171,2,10,2688),(162,2,9,3072),(153,2,8,3456),(143,2,7,3840),(132,2,6,4224),(120,2,5,4608),(108,2,4,5376),(91,2,3,6528),(68,2,2,7680),(41,2,1,8832)];
const VEL: [G; 16] = [(64,13,56,0),(74,13,57,384),(84,13,58,768),(94,13,59,1152),(104,13,60,1536),(115,13,61,1920),(124,13,62,2304),(134,13,63,2688),(144,13,64,3072),(153,13,65,3456),(162,13,66,3840),(171,13,67,4608),(180,13,68,5376),(190,13,69,6528),(209,13,70,7680),(227,13,71,8832)];
const SPEED: [G; 19] = [(60,8,18,0),(71,8,19,526),(81,8,20,1052),(93,8,21,1578),(103,8,22,2104),(114,8,23,2630),(123,8,24,3156),(134,8,25,3682),(144,8,26,4208),(153,8,27,4734),(162,8,28,5260),(171,8,29,5786),(182,8,30,6312),(194,8,31,6838),(207,8,32,7364),(219,8,33,7890),(231,8,34,8416),(242,8,35,8942),(254,8,36,9468)];
const SHIELD: [G; 6] = [(2,2,72,0),(21,2,73,1666),(34,2,74,3332),(38,2,75,4998),(48,2,76,6664),(57,2,77,8330)];
const AMMO: [G; 6] = [(298,2,91,0),(277,2,90,1666),(263,2,89,3332),(253,2,88,4998),(244,2,87,6664),(230,2,86,8330)];
const HEAT: [G; 16] = [(251,14,92,0),(259,14,93,625),(262,14,94,1250),(265,14,95,1875),(268,14,96,2500),(271,14,97,3125),(274,14,98,3750),(277,14,99,4375),(283,14,100,5000),(289,14,101,5625),(293,14,102,6250),(297,14,103,6875),(301,14,104,7500),(305,14,105,8125),(309,14,106,8750),(313,14,107,9375)];

const TEAM_COLOUR: [u8; 8] = [1, 0, 15, 179, 160, 250, 198, 181];

#[derive(Clone, Copy, Default)]
struct Crack {
    x: i32,
    y: i32,
    state: u8,
}

pub struct Messages {
    pub banner_state: u8,
    pub banner_count: i32,
    pub sweep: i32,
    pub banner_id: u16,
    pub priority: i32,
    pub big_state: u8,
    pub big_count: i32,
    pub big_id: u8,
}

pub struct Hud {
    cracks: [Crack; 16],
    seed: Lcg,
    pub msg: Messages,
}

const BANNER_REPEAT: [u8; 34] = [0, 0, 0, 0, 0, 0, 0, 0, 2, 2, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0];

impl Hud {
    pub fn new() -> Hud {
        Hud {
            cracks: [Crack::default(); 16],
            seed: Lcg(1),
            msg: Messages { banner_state: 0, banner_count: 0, sweep: 0, banner_id: 0, priority: 0, big_state: 0, big_count: 0, big_id: 0 },
        }
    }

    /// Banner request (spec 04 §12.1). Returns a sample stop request.
    pub fn banner_request(&mut self, id: u16) -> bool {
        let m = &mut self.msg;
        let repeat = BANNER_REPEAT.get(id as usize).copied().unwrap_or(0);
        let prio = 10;
        if !(prio >= m.priority && (id != m.banner_id || repeat == 1)) {
            return false;
        }
        let mut stop = false;
        match m.banner_state {
            0 => {
                m.banner_state = 1;
                m.sweep = 0;
            }
            1 | 2 => return false,
            3 => {
                m.banner_state = 2;
                m.banner_count = 2;
                stop = true;
            }
            4 => {
                m.banner_state = 2;
                m.banner_count = 40;
                stop = true;
            }
            _ => m.banner_state = 1,
        }
        m.banner_id = id;
        m.priority = prio;
        stop
    }

    pub fn big_request(&mut self, id: u8) {
        self.msg.big_state = 2;
        self.msg.big_count = 50;
        self.msg.big_id = id;
    }

    /// Per-frame step (once per race tick here). Returns (play sample?, stop sample?) for the banner's WARNING loop.
    pub fn step(&mut self, paused: bool) -> (Option<u16>, bool) {
        let m = &mut self.msg;
        let sample = if (2..=4).contains(&m.banner_id) { Some(17u16) } else { None };
        let mut play = None;
        let mut stop = false;
        match m.banner_state {
            1 => {
                m.sweep += 1;
                if m.sweep > 5 {
                    m.sweep = 5;
                    m.banner_state = 2;
                    m.banner_count = 10;
                }
            }
            2 => {
                play = sample;
                if !paused {
                    m.banner_count -= 1;
                }
                if m.banner_count < 0 {
                    let rep = BANNER_REPEAT.get(m.banner_id as usize).copied().unwrap_or(0);
                    m.banner_state = if rep == 2 { 4 } else { 3 };
                    m.banner_count = if rep == 2 { 4 } else { 40 };
                }
            }
            3 => {
                play = sample;
                m.banner_count -= 1;
                if m.banner_count < 0 {
                    m.banner_state = 4;
                    m.banner_count = 4;
                }
            }
            4 => {
                play = sample;
                m.priority = 0;
                if !paused {
                    m.banner_count -= 1;
                }
                if m.banner_count < 0 {
                    m.banner_state = 5;
                    m.banner_count = 5;
                }
            }
            5 => {
                stop = true;
                m.sweep -= 1;
                if m.sweep < 0 {
                    m.banner_state = 0;
                    m.banner_id = 0;
                }
            }
            _ => {}
        }
        if m.big_state == 2 && !paused {
            m.big_count -= 1;
            if m.big_count < 0 {
                m.big_state = 0;
            }
        }
        (play, stop)
    }

    fn gauge(fg: &mut Canvas, a: &Assets, g: &[G], value: i32, le: bool, blend: bool) {
        let v = value.min(10000);
        for &(x, y, s, th) in g {
            let ok = if le { th <= v } else { th < v };
            if !ok {
                break;
            }
            fg.sprite(a.panel.get(s), x * 2, y * 2, if blend { Blit::Blend } else { Blit::Solid });
        }
    }

    /// Draw the HUD for craft c (spec 08 §11). `frame` = race frame of the viewer.
    pub fn draw(&mut self, fg: &mut Canvas, a: &Assets, w: &World, ci: usize, frame: u32, target_screen: Option<(f32, f32)>, tw: f32, th: f32) {
        let c = &w.crafts[ci];
        let p = &a.panel;
        // cracked glass
        let n = (c.bullet_counter / 10).clamp(0, 16) as usize;
        let cs = p.get(230);
        let (cw, ch) = (cs.w as i32, cs.h as i32);
        for i in 0..16 {
            if i >= n {
                self.cracks[i].state = 0;
                continue;
            }
            if self.cracks[i].state != 0 {
                continue;
            }
            let x = (self.seed.next() % (640 - cw).max(1) as u32) as i32;
            let y = (self.seed.next() % (480 - ch).max(1) as u32) as i32;
            let close = self.cracks.iter().any(|o| o.state == 2 && (((o.x - x).pow(2) + (o.y - y).pow(2)) as f32).sqrt() < cw as f32);
            self.cracks[i] = Crack { x, y, state: if close { 1 } else { 2 } };
        }
        for k in &self.cracks {
            if k.state == 2 {
                fg.sprite_clipped(cs, k.x, k.y, Blit::Blend);
            }
        }
        let blink = frame & 4 == 0;
        let warn = |v: i32| (1..=4000).contains(&v) && blink;
        Self::gauge(fg, a, &FUEL, c.fuel, false, warn(c.fuel));
        let vel = (c.measured_vel / (200.0 / 256.0) * 10000.0) as i32;
        Self::gauge(fg, a, &VEL, vel, false, false);
        Self::gauge(fg, a, &SPEED, (c.throttle * 10000.0 / 240.0) as i32, false, false);
        if c.boost_charge > 0 {
            let boost: Vec<G> = SPEED.iter().map(|&(x, y, s, t)| (x, y, s + 19, t)).collect();
            Self::gauge(fg, a, &boost, c.boost_charge * 100, false, false);
        }
        Self::gauge(fg, a, &SHIELD, c.shield, true, warn(c.shield));
        Self::gauge(fg, a, &AMMO, c.ammo, true, warn(c.ammo));
        Self::gauge(fg, a, &HEAT, c.heat * 100, false, false);
        // digits and icons
        let crafts = w.crafts.iter().filter(|c| c.active).count() as usize;
        fg.sprite(p.get(108 + c.place.max(1) as usize), 4, 32, Blit::Solid);
        fg.sprite(p.get(138), 56, 76, Blit::Solid);
        fg.sprite(p.get(128 + crafts), 86, 76, Blit::Solid);
        fg.sprite(p.get(228), 554, 68, Blit::Solid);
        let k = c.kills.clamp(0, 99) as usize;
        fg.sprite(p.get(118 + k / 10), 586, 68, Blit::Solid);
        fg.sprite(p.get(118 + k % 10), 612, 68, Blit::Solid);
        let lap = c.lap.max(1).min(c.race_laps.saturating_sub(1).max(1)) as usize;
        let laps = c.race_laps.saturating_sub(1) as usize;
        fg.sprite(p.get(227), 128, 94, Blit::Solid);
        fg.sprite(p.get(139 + (lap / 10) % 10), 178, 96, Blit::Solid);
        fg.sprite(p.get(139 + lap % 10), 192, 96, Blit::Solid);
        fg.sprite(p.get(150), 204, 96, Blit::Solid);
        fg.sprite(p.get(139 + (laps / 10) % 10), 218, 96, Blit::Solid);
        fg.sprite(p.get(139 + laps % 10), 232, 96, Blit::Solid);
        // weapon boxes
        let boxes = [(268usize, 74, [(270usize, 109), (270, 116), (270, 123)]), (271, 132, [(273, 158), (273, 165), (273, 172)]), (274, 181, [(275, 206), (275, 213), (276, 220)])];
        for (wk, (bx, x, pips)) in boxes.iter().enumerate() {
            fg.sprite(p.get(*bx), x * 2, 70, Blit::Solid);
            for (lvl, (s, px)) in pips.iter().enumerate() {
                if (lvl as i32) < c.upgrades[wk] {
                    fg.sprite(p.get(*s), px * 2, 70, Blit::Solid);
                }
            }
        }
        // lap-time list (bottom left, at the canvas edge)
        let digit_h = p.get(139).h as i32;
        let cur = c.lap.max(1) as usize;
        let first = cur.saturating_sub(3).max(1);
        let mut rows: Vec<(usize, i32)> = Vec::new();
        if let Some(best) = c.best_lap {
            if let Some(bi) = c.laps_list.iter().position(|&t| t == best) {
                if bi + 1 < first {
                    rows.push((bi + 1, best));
                }
            }
        }
        for l in first..=cur {
            let t = if l <= c.laps_list.len() { c.laps_list[l - 1] } else { c.lap_time };
            rows.push((l, t));
        }
        let mut y = 480 - rows.len() as i32 * (digit_h + 2) - 2;
        let x0 = -fg.x0 + 4;
        for (l, t) in rows {
            let mut x = x0;
            let tens = (l / 10) % 10;
            let s = p.get(151 + tens);
            if tens != 0 {
                fg.sprite(s, x, y, Blit::Solid);
            }
            x += s.w as i32 - 2;
            let s = p.get(151 + l % 10);
            fg.sprite(s, x, y, Blit::Solid);
            x += s.w as i32 + 4;
            let s = p.get(161);
            fg.sprite(s, x, y, Blit::Solid);
            x += s.w as i32 + 4;
            let t = t.clamp(0, 9999) as usize;
            let digits = [t / 1000, (t / 100) % 10, (t / 10) % 10, t % 10];
            let mut lead = true;
            for (di, d) in digits.iter().enumerate() {
                if *d == 0 && lead && di < 3 {
                    continue;
                }
                lead = false;
                let s = p.get(151 + d);
                fg.sprite(s, x, y, Blit::Solid);
                x += s.w as i32 - 2;
            }
            y += digit_h + 2;
        }
        // target marker
        if c.lock > 0 {
            if let (Some((sx, sy)), Some(t)) = (target_screen, c.target) {
                let _ = (tw, th);
                self.marker(fg, a, w, c, t, sx, sy, frame);
            }
        }
    }

    fn marker(&self, fg: &mut Canvas, a: &Assets, w: &World, c: &Craft, ti: usize, vx: f32, vy: f32, frame: u32) {
        let p = &a.panel;
        let b = p.get(258);
        let (bw, bh) = (b.w as i32, b.h as i32);
        let (sx, sy) = (vx as i32, vy as i32);
        let left = sx - bw / 2;
        let top = sy - bh / 2;
        let right = left + bw;
        let bottom = top + bh;
        let t = &w.crafts[ti];
        let name = t.name.to_ascii_uppercase();
        let lock = c.lock;
        let put = |fg: &mut Canvas, s: usize, x: i32, y: i32| fg.sprite_clipped(p.get(s), x, y, Blit::Blend);
        let cube_green = a.cube[48];
        let bar_ok = t.inv == 0 || frame & 4 != 0;
        if lock <= 2 {
            put(fg, 258, left, top);
        } else if lock < 100 {
            let d = (100 - lock) / 4 + 2;
            put(fg, 258, left, top);
            put(fg, 260, left, top - d + 2);
            put(fg, 262, left, bottom + d - p.get(262).h as i32);
            put(fg, 264, left - d, top);
            put(fg, 266, right + d - p.get(266).w as i32, top);
            if frame & 4 != 0 {
                Self::panel_text(fg, a, &name, 232, sx + bw / 2 + 2, top);
            }
            if bar_ok {
                fg.fill_rect(left + 2, sy + bh / 2 + 2, t.shield * (bh - 4) / 10000, 4, cube_green, Blit::Solid);
            }
        } else {
            put(fg, 259, left, top);
            put(fg, 261, left, top - 2);
            put(fg, 263, left, bottom + 2 - p.get(263).h as i32);
            put(fg, 265, left - 2, top);
            put(fg, 267, right + 2 - p.get(267).w as i32, top);
            Self::panel_text(fg, a, &name, 201, sx + bw / 2 + 4, top);
            if bar_ok {
                fg.fill_rect(left + 2, sy + bh / 2 + 2, t.shield * (bh - 2) / 10000, 4, cube_green, Blit::Solid);
            }
        }
    }

    pub fn panel_text(fg: &mut Canvas, a: &Assets, s: &str, base: usize, x: i32, y: i32) {
        let mut cx = x;
        for ch in s.bytes() {
            let ch = ch.to_ascii_uppercase();
            if ch == b' ' {
                cx += a.panel.get(base).w as i32;
            } else if ch.is_ascii_uppercase() {
                let g = a.panel.get(base + (ch - b'A') as usize);
                fg.sprite(g, cx, y, Blit::Solid);
                cx += g.w as i32;
            }
        }
    }

    /// Minimap at the bottom right of the canvas (spec 08 §11.6).
    pub fn minimap(fg: &mut Canvas, a: &Assets, w: &World, track: usize, viewed: usize, frame: u32) {
        let pieces: &[(usize, i32, i32)] = match track {
            1 => &[(1, 0, 0), (2, 6, 120)],
            2 => &[(3, 0, 0), (4, 120, 5), (5, 74, 120)],
            3 => &[(6, 0, 0)],
            4 => &[(7, 0, 0), (8, 28, 120)],
            5 => &[(9, 0, 0), (10, 18, 120)],
            _ => &[(11, 0, 0)],
        };
        let t = &a.track;
        let first = t.get(pieces[0].0);
        let last = t.get(pieces[pieces.len() - 1].0);
        let mut x0 = fg.w - fg.x0 - first.w as i32 - 4;
        if track == 2 {
            x0 -= t.get(pieces[1].0).w as i32;
        }
        let last_h = if pieces.len() > 1 { last.h as i32 } else { 0 };
        let y0 = 480 - (first.h as i32 + last_h) - 4;
        for &(e, dx, dy) in pieces {
            fg.sprite(t.get(e), x0 + dx, y0 + dy, Blit::Solid);
        }
        let leader = w.crafts.iter().position(|c| c.active && c.place == 1);
        for (i, c) in w.crafts.iter().enumerate() {
            if !c.active {
                continue;
            }
            let col = TEAM_COLOUR[c.team as usize & 7];
            let (x, y) = (x0 + c.pos[0] as i32, y0 + c.pos[1] as i32);
            if i == viewed && frame & 4 != 0 {
                fg.fill_rect(x - 5, y - 5, 10, 10, col, Blit::BlendRev);
            }
            if i == viewed || Some(i) == leader {
                fg.fill_rect(x - 3, y - 3, 6, 6, col, Blit::Solid);
            } else {
                fg.fill_rect(x, y, 4, 4, col, Blit::Solid);
            }
        }
    }

    pub fn start_lights(fg: &mut Canvas, a: &Assets, lit: usize) {
        let ids = [if lit == 1 { 282 } else { 281 }, if lit == 2 { 284 } else { 283 }, if lit == 3 { 286 } else { 285 }];
        let (sa, sb) = (a.panel.get(ids[0]), a.panel.get(ids[1]));
        let mut x = (2 * sa.w as i32 + sb.w as i32 - 16) / 3;
        for id in ids {
            let s = a.panel.get(id);
            fg.sprite(s, x, 12, Blit::Solid);
            x += s.w as i32 + 8;
        }
    }

    /// Banner, big message (spec 08 §12).
    pub fn draw_messages(&self, fg: &mut Canvas, a: &Assets, lang: usize) {
        let m = &self.msg;
        let y = 480 - 40;
        let p = &a.panel;
        if m.banner_state != 0 {
            let cap_l = p.get(198);
            let cap_r = p.get(199);
            let fill = p.get(200);
            if m.banner_state == 1 || m.banner_state == 5 {
                let t = 30 * (m.sweep.clamp(0, 5) + 1);
                let mut x = 320 - cap_l.w as i32 - t;
                while x < 320 + t && fill.w > 0 {
                    fg.sprite(fill, x, y, Blit::Solid);
                    x += fill.w as i32;
                }
                fg.sprite(cap_l, 320 - cap_l.w as i32 - t, y, Blit::Solid);
                fg.sprite(cap_r, 320 + t, y, Blit::Solid);
            } else {
                let t = 180;
                fg.sprite(cap_l, 320 - cap_l.w as i32 - t, y, Blit::Solid);
                fg.sprite(cap_r, 320 + t, y, Blit::Solid);
                let s = strings::to_cp437(strings::get(lang, m.banner_id as usize));
                let f = &a.fonts[2];
                let ty = y + (cap_l.h as i32 - f.space_h() as i32) / 2;
                fg.text_centred(f, &s, ty, -6, 1, Some(0));
            }
        }
        if m.big_state == 2 && (m.big_count >> 2) & 1 == 1 {
            let n = match m.big_id {
                1 => 172,
                2 => 189,
                3 => 191,
                4 => 192,
                5 => 193,
                _ => 173,
            };
            let s = strings::to_cp437(strings::get(lang, n));
            let f = &a.fonts[4];
            fg.text_centred(f, &s, 240 - f.space_h() as i32 / 2, -6, 0, None);
        }
    }

    pub fn finishers(fg: &mut Canvas, a: &Assets, w: &World) {
        let mut y = 120;
        let step = a.panel.get(139).h as i32 + 2;
        let x0 = -fg.x0 + 2;
        for place in 1..=8u8 {
            for c in w.crafts.iter().filter(|c| c.active && c.finish_place == place) {
                let d = a.panel.get(151 + place as usize);
                fg.sprite(d, x0, y, Blit::Solid);
                Self::panel_text(fg, a, &c.name, 201, x0 + d.w as i32 + 8, y + 4);
                y += step;
            }
        }
    }
}
