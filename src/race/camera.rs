//! Race camera (spec 08 §3). Runs once per display frame on interpolated craft data.
use super::math::*;
use super::sim::*;
use crate::gfx::Camera;

pub struct RaceCamera {
    pub follow: usize,
    pub zoom: f32,
    pub x: f32,
    pub row: f32,
    pub height: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub distance: f32,
    pub chase_yaw: f32,
    pub chase_pitch: f32,
    pub chase_roll: f32,
    pub chase_dist: f32,
    pub static_count: i32,
    /// followed craft hidden (cockpit)
    pub hide_followed: bool,
    pub initialised: bool,
}

/// Per-frame input of the camera: the followed craft's interpolated view data.
pub struct ViewData {
    pub pos: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
}

impl RaceCamera {
    pub fn new() -> RaceCamera {
        RaceCamera {
            follow: 0,
            zoom: 300.0,
            x: 0.0,
            row: 0.0,
            height: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            distance: 0.0,
            chase_yaw: 0.0,
            chase_pitch: 0.0,
            chase_roll: 0.0,
            chase_dist: 0.0,
            static_count: 0,
            hide_followed: true,
            initialised: false,
        }
    }

    /// Run the camera for one display frame. `k` = easing factor for this frame (1/8 at 20 Hz).
    /// `forced` = Some(action) overrides the window's action for this frame.
    pub fn run(&mut self, w: &World, v: &ViewData, action: u8, k: f32, shake: Option<(i32, bool)>, orbit_keys: [bool; 6]) {
        let ease_a = |cur: &mut f32, target: f32, k: f32| *cur = wrap(*cur + diff(target, *cur) * k);
        let snap = !self.initialised;
        self.initialised = true;
        let kk = if snap { 1.0 } else { k };
        self.x = v.pos[0];
        self.row = v.pos[1];
        self.height = v.pos[2];
        match action {
            1..=5 => {
                self.chase_yaw = v.yaw;
                self.chase_pitch = v.pitch + 2730.0;
                self.chase_roll = v.roll;
                self.chase_dist = 1.0;
                match action {
                    2 => self.chase_yaw += 32768.0,
                    3 => self.chase_yaw += 16384.0,
                    4 => self.chase_yaw -= 16384.0,
                    5 => self.chase_pitch = v.pitch + 16384.0,
                    _ => {}
                }
                ease_a(&mut self.yaw, self.chase_yaw, kk);
                ease_a(&mut self.pitch, self.chase_pitch, kk);
                ease_a(&mut self.roll, self.chase_roll, kk);
                self.distance += (self.chase_dist - self.distance) * kk;
            }
            6 => {
                if !self.level_camera(w, v, kk) {
                    self.cockpit(v, shake);
                }
            }
            8..=10 => {
                let (d, a) = match action {
                    8 => (120.0 * U, 15.0),
                    9 => (240.0 * U, 10.0),
                    _ => (360.0 * U, 5.0),
                };
                self.height += 128.0 * U;
                self.chase_yaw = v.yaw;
                self.chase_roll = v.roll;
                self.chase_pitch = v.pitch + a * DEG;
                self.chase_dist = d;
                let free = probe(w, [self.x, self.row, self.height], self.chase_yaw, self.chase_pitch, -d);
                if free != -d {
                    self.static_count += 1;
                    if self.static_count > 8 {
                        self.chase_dist = (-free - 64.0 * U).max(16.0 * U);
                        self.static_count = 8;
                    }
                } else {
                    self.static_count = 0;
                }
                ease_a(&mut self.yaw, self.chase_yaw, kk);
                ease_a(&mut self.roll, self.chase_roll, kk);
                ease_a(&mut self.pitch, self.chase_pitch, kk);
                self.distance += (self.chase_dist - self.distance) * kk;
            }
            11 => {
                let st = 546.0 * k * 8.0;
                if orbit_keys[0] {
                    self.chase_yaw -= st;
                }
                if orbit_keys[1] {
                    self.chase_yaw += st;
                }
                if orbit_keys[2] {
                    self.chase_pitch -= st;
                }
                if orbit_keys[3] {
                    self.chase_pitch += st;
                }
                if orbit_keys[4] {
                    self.chase_roll -= st;
                }
                if orbit_keys[5] {
                    self.chase_roll += st;
                }
                self.yaw = wrap(self.chase_yaw);
                self.pitch = self.chase_pitch;
                self.roll = self.chase_roll;
                self.distance = self.zoom * 8064.0 / 1000.0 / 256.0 + 0.5;
            }
            _ => self.cockpit(v, shake),
        }
        self.hide_followed = self.distance <= 0.001;
    }

    fn cockpit(&mut self, v: &ViewData, shake: Option<(i32, bool)>) {
        self.yaw = v.yaw;
        self.pitch = v.pitch;
        self.roll = v.roll;
        self.distance = 0.0;
        if let Some((count, odd)) = shake {
            let amount = (count * 64 / 1000) as f32 * if odd { 1.0 } else { -1.0 } * U;
            if amount != 0.0 {
                let a = along(self.yaw + 16384.0, self.pitch, amount);
                self.x += a[0];
                self.row += a[1];
                self.height += a[2];
            }
        }
    }

    fn level_camera(&mut self, w: &World, v: &ViewData, _k: f32) -> bool {
        let mut best: Option<(f32, [f32; 3])> = None;
        for t in &w.things {
            if !t.alive || !matches!(t.kind, TK::Scenery { .. }) {
                continue;
            }
            let p = [t.pos[0], t.pos[1], t.pos[2] + 1.0];
            let d = dist3(p, v.pos);
            if d >= 32.0 || best.map(|b| d >= b.0).unwrap_or(false) {
                continue;
            }
            if visible(w, v.pos, p) {
                best = Some((d, p));
            }
        }
        let Some((d, p)) = best else { return false };
        self.yaw = heading(v.pos[0] - p[0], v.pos[1] - p[1]);
        self.pitch = atan2_raw(p[2] - v.pos[2], dist2(p, v.pos));
        self.roll = 0.0;
        self.distance = self.zoom * d / 1000.0;
        true
    }

    /// Final eye position and the GPU camera.
    pub fn eye(&self, w: &World) -> Camera {
        let mut x = self.x;
        let mut row = self.row;
        let mut h = self.height;
        if self.distance > 0.0 {
            let a = along(self.yaw, self.pitch, -self.distance);
            x += a[0];
            row += a[1];
            h += a[2];
        }
        let t = w.lvl.terrain_height(x.clamp(0.0, 255.9), row.clamp(0.0, 159.9));
        h = h.max(t + 128.0 * U);
        Camera { x, row, height: h, yaw: self.yaw, pitch: self.pitch, roll: self.roll, focal320: 94.0, fixed_43: false }
    }
}

fn visible(w: &World, from: [f32; 3], to: [f32; 3]) -> bool {
    if (from[0] - to[0]).abs() <= 1.0 && (from[1] - to[1]).abs() <= 1.0 {
        return true;
    }
    let d = dist3(from, to) * 256.0;
    let n = ((d + 64.0) as i32 >> 7).max(1);
    for s in 1..n {
        let k = s as f32 / n as f32;
        let p = [from[0] + (to[0] - from[0]) * k, from[1] + (to[1] - from[1]) * k, from[2] + (to[2] - from[2]) * k];
        if w.lvl.collide(p[0], p[1], p[2]) {
            return false;
        }
    }
    true
}

fn probe(w: &World, p: [f32; 3], yaw: f32, pitch: f32, d: f32) -> f32 {
    if d.abs() < 8.0 * U || d.abs() > 10.0 {
        return d;
    }
    let mut k = -8.0 * U;
    while k >= d {
        let a = along(yaw, pitch, k);
        let q = [p[0] + a[0], p[1] + a[1], p[2] + a[2] + 16.0 * U];
        if w.lvl.collide(q[0] - 64.0 * U, q[1] + 64.0 * U, q[2]) || w.lvl.collide(q[0] + 64.0 * U, q[1] + 64.0 * U, q[2]) {
            return k;
        }
        k -= 8.0 * U;
    }
    d
}
