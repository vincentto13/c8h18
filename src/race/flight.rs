//! Craft per-tick update (spec 05): action state machine, flight model, post-process, bonuses.
use super::math::*;
use super::sim::*;

#[derive(Clone, Copy, Default, Debug)]
pub struct HumanIn {
    pub accel: bool,
    pub brake: bool,
    pub left: bool,
    pub right: bool,
    pub gun: bool,
    pub missile: bool,
    pub boost: bool,
    /// analog steering -1..1 from a gamepad stick (port addition); used when left/right are off
    pub steer: f32,
}

const SENSOR_REBOUND: [f32; 4] = [50.0, 50.0, 200.0, 200.0];

impl World {
    pub fn init_craft(&mut self, c: &mut Craft) {
        let ground = self.floor(c.pos[0], c.pos[1], 255.0);
        c.pos[2] = ground + 128.0 * U;
        c.prev_pos = c.pos;
        for s in &mut c.sensors {
            *s = (c.pos[2], 0.0);
        }
        c.fuel = 10000;
        c.shield = 10000;
        c.ammo = 10000;
        c.friction = 10.0;
        c.eff = 100;
        c.behind = 100;
        c.inv = 100;
        c.killer = None;
        c.action = A_RACING;
    }

    /// The whole per-tick update of craft i.
    pub fn update_craft(&mut self, i: usize, input: HumanIn) {
        let mut c = std::mem::take(&mut self.crafts[i]);
        if !c.active {
            self.crafts[i] = c;
            return;
        }
        c.prev_pos = c.pos;
        c.prev_view = c.view;
        c.incoming_missile = None;
        c.speed_in = 0.0;
        c.steer_in = 0.0;
        // finished crafts drive on (spec 04 §5.1)
        if c.finish_place > 0 && c.hold > 0 {
            if !c.autopilot_locked {
                c.waypoint = self.track.nearest(c.pos[0], c.pos[1]);
                c.ap_first = false;
            }
            c.autopilot_locked = true;
            c.hold -= 1;
        }
        self.action_machine(&mut c, i, input);
        if !c.reposition {
            self.flight(&mut c, i);
        }
        self.view_data(&mut c);
        self.post_process(&mut c, i);
        self.behind_factor(&mut c);
        self.bonuses(&mut c);
        self.spray(&mut c);
        self.checkpoints(&mut c, i);
        c.time_slice = c.time_slice.wrapping_add(1);
        self.crafts[i] = c;
    }

    fn view_data(&self, c: &mut Craft) {
        let mut v = [c.yaw, c.pitch, c.roll + 4.0 * c.turn_rate];
        if c.tumble > 0 {
            for k in 0..3 {
                v[k] += c.tumble_rate[k] * c.tumble as f32;
            }
        }
        c.view = v;
    }

    fn action_machine(&mut self, c: &mut Craft, i: usize, input: HumanIn) {
        match c.action {
            A_INIT => self.init_craft(c),
            A_RACING => {
                // finished crafts drive on (spec 04 §5.1)
                // fuel empty
                if c.fuel <= 0 {
                    if c.fuel_empty == 0 {
                        c.fuel_empty = 200;
                        if c.human {
                            self.banner_req(i, 5);
                        }
                    } else {
                        c.fuel_empty -= 1;
                        if c.fuel_empty == 1 {
                            c.action = A_CALL;
                            c.destroyed = false;
                            if c.human {
                                self.banner_req(i, 30);
                            }
                            return;
                        }
                        if c.fuel_empty == 0 {
                            c.fuel_empty = 200;
                            if c.human {
                                self.banner_req(i, 5);
                            }
                        }
                    }
                } else {
                    c.fuel_empty = 0;
                }
                // damage smoke
                if c.shield > 0 && c.shield <= 3000 {
                    let n = (c.shield / 1000 + 1) as u8;
                    if c.time_slice % n == 0 {
                        self.smoke_fire(c.pos, 0.0);
                    }
                    if c.shield < 500 {
                        self.smoke_fire(c.pos, 20.0 * U);
                    }
                }
                // death
                if c.shield <= 0 {
                    self.explosion(TK::ExplBig, c.pos, c.yaw, c.pitch, false, None);
                    c.action = A_TUMBLE;
                    c.destroyed = true;
                    let b = c.target.and_then(|t| self.crafts.get(t)).map(|t| diff(heading(t.pos[0] - c.pos[0], t.pos[1] - c.pos[1]), c.yaw)).unwrap_or(0.0);
                    let r = b + 65536.0;
                    if c.time_slice % 4 < 2 {
                        c.tumble_rate = [r / 20.0, 0.0, -r / 7.0];
                    } else {
                        c.tumble_rate = [r / 40.0, 0.0, 36.0];
                    }
                    c.tumble = 20;
                    if c.human {
                        self.banner_req(i, 1);
                    }
                    return;
                }
                if !c.started {
                    return;
                }
                if c.autopilot() {
                    self.autopilot(c, i);
                } else {
                    self.targeting(c, i);
                    if input.accel {
                        c.speed_in = 26.0;
                    } else if input.brake {
                        c.speed_in = -26.0;
                    }
                    if input.right {
                        c.steer_in = 100.0;
                    } else if input.left {
                        c.steer_in = -100.0;
                    } else if input.steer != 0.0 {
                        c.steer_in = (input.steer * 100.0).clamp(-100.0, 100.0);
                    }
                    c.trig_gun = input.gun;
                    c.trig_missile = input.missile;
                    c.trig_boost = input.boost && c.fuel > 0;
                }
            }
            A_TUMBLE => {
                c.tumble -= 1;
                if c.tumble < 1 {
                    c.tumble = 0;
                    c.action = A_DROP;
                }
            }
            A_DROP => {
                self.drop_collectables(c);
                c.action = A_EXPLODING;
                c.action_count = 5;
            }
            A_EXPLODING => {
                if c.time_slice % 3 == 0 {
                    self.explosion(TK::ExplMedium, c.pos, c.yaw, c.pitch, false, None);
                }
                self.smoke_fire(c.pos, 0.0);
                c.action_count -= 1;
                if c.action_count < 0 {
                    c.action = A_CALL;
                }
            }
            A_CALL => {
                c.pitch *= 250.0 / 256.0;
                c.roll *= 250.0 / 256.0;
                c.momentum[0] *= 240.0 / 256.0;
                c.momentum[1] *= 240.0 / 256.0;
                c.throttle = 0.0;
                if c.destroyed && c.time_slice % 4 == 0 {
                    self.smoke_fire(c.pos, 0.0);
                }
                // port deviation: no truck in the level -> put back after 100 ticks
                if self.trucks.is_empty() {
                    c.recovery_wait += 1;
                    if c.recovery_wait >= 100 {
                        c.recovery_wait = 0;
                        let wp = self.track.nearest(c.pos[0], c.pos[1]);
                        if wp != 0 {
                            let w = self.track.wps[wp];
                            c.pos = [w.x, w.y, w.floor + 0.5];
                            c.waypoint = wp;
                        }
                        if c.destroyed {
                            c.shield = 10000;
                            c.fuel = 10000;
                            c.ammo = c.ammo.max(10000);
                        } else {
                            c.fuel += 2000;
                        }
                        c.action = A_RESET;
                    }
                }
            }
            A_TRUCK_COMING => {
                c.momentum[0] *= 230.0 / 256.0;
                c.momentum[1] *= 230.0 / 256.0;
                c.throttle = 0.0;
            }
            A_REPAIR => {
                c.pitch *= 250.0 / 256.0;
                c.pitch *= 252.0 / 256.0;
                c.reposition = true;
                c.throttle = 0.0;
                c.measured_vel = 0.0;
                c.momentum = [0.0; 2];
                if c.destroyed {
                    if c.shield < 10000 {
                        c.shield = (c.shield + 1000).min(10000);
                        self.sound(7, Some(i), c.pos, 100);
                    } else if c.fuel < 10000 {
                        c.fuel = (c.fuel + 1000).min(10000);
                        self.sound(7, Some(i), c.pos, 100);
                    } else if c.ammo < 10000 {
                        c.ammo = (c.ammo + 1000).min(10000);
                        self.sound(7, Some(i), c.pos, 100);
                    } else {
                        self.stop_sound(7, Some(i));
                        c.action = A_REPAIRED;
                        c.bullet_counter = 0;
                        c.missile_counter = 0;
                    }
                } else {
                    c.fuel += 2000;
                    c.action = A_REPAIRED;
                }
            }
            A_REPAIRED => {}
            A_RESET => {
                c.killer = None;
                if c.destroyed {
                    for u in &mut c.upgrades {
                        *u = (*u - 1).max(0);
                    }
                    c.inv = 100;
                }
                c.reposition = false;
                c.wants = [false; 3];
                for s in &mut c.sensors {
                    *s = (c.pos[2], 0.0);
                }
                c.momentum = [0.0; 2];
                c.turn_rate = 0.0;
                c.roll = 0.0;
                c.tumble = 0;
                c.destroyed = false;
                c.stuck_count = 0;
                c.fuel_empty = 0;
                c.truck = None;
                c.action = A_RACING;
            }
            _ => c.action = A_RACING,
        }
    }

    fn flight(&mut self, c: &mut Craft, i: usize) {
        // a hit memory
        for k in 0..8 {
            if c.hits_tick[k] > 0 {
                c.hits_run[k] += c.hits_tick[k];
            } else {
                c.hits_max[k] = c.hits_max[k].max(c.hits_run[k]);
                c.hits_run[k] = 0;
            }
            c.hits_tick[k] = 0;
        }
        // b friction
        if c.airborne {
            c.friction = 0.0;
        } else if c.time_slice % 2 == 0 {
            let mut t = ((self.lvl.attr(c.pos[0].floor() as i32, c.pos[1].floor() as i32) >> 10) & 15) as f32;
            if t == 0.0 {
                t = 15.0;
            }
            if c.friction < t {
                c.friction += 1.0;
            } else if c.friction > t {
                c.friction -= 1.0;
            }
        }
        // c steering
        if c.steer_in != 0.0 {
            if c.turn_rate == 0.0 || c.turn_rate.signum() != c.steer_in.signum() {
                c.turn_rate *= 54.0 / 64.0;
            }
            c.turn_rate += c.steer_in;
        } else {
            c.turn_rate *= 57.0 / 64.0;
        }
        c.turn_rate = c.turn_rate.clamp(-3640.0, 3640.0);
        c.yaw = wrap(c.yaw + c.turn_rate);
        // d thrust
        if c.shield > 0 {
            if c.speed_in.abs() > 1.0 {
                c.throttle += c.speed_in;
                c.brake_flag = c.speed_in < 0.0;
            }
        } else {
            c.throttle = 0.0;
        }
        c.throttle = c.throttle.clamp(0.0, 240.0);
        if c.fuel <= 0 {
            c.throttle = c.throttle.min(120.0);
        }
        let thrust = along2(c.yaw, c.eff as f32 / 100.0 * c.throttle / 16.0 * U);
        // e slope
        let (cx, cy) = (c.pos[0].floor() as i32, c.pos[1].floor() as i32);
        let h1 = self.lvl.corner_h(cx, cy);
        let h2 = self.lvl.corner_h(cx + 1, cy);
        let h3 = self.lvl.corner_h(cx + 1, cy + 1);
        let h4 = self.lvl.corner_h(cx, cy + 1);
        let slope = [h1 - h2 - h3 + h4, h1 + h2 - h3 - h4];
        // f momentum
        let mut delta = [thrust[0] + slope[0] / 64.0, thrust[1] + slope[1] / 64.0];
        let bf = (c.behind - 100) as f32 / 100.0;
        let catchup = [c.momentum[0] / 4.0 * bf, c.momentum[1] / 4.0 * bf];
        if !c.airborne {
            let v = len2(c.momentum) / 4.0;
            if v < 100.0 * U {
                let boost = (50.0 * U - v).clamp(0.0, 50.0 * U) / (100.0 * U);
                delta[0] *= 1.0 + boost;
                delta[1] *= 1.0 + boost;
            }
            c.momentum[0] += delta[0];
            c.momentum[1] += delta[1];
            if c.aff_booster {
                c.aff_booster = false;
                c.booster_active = true;
                c.booster_burn = 70;
                let a = along2(c.yaw, 150.0 * U * c.booster_strength as f32 / 100.0 * c.eff as f32 / 100.0);
                c.momentum[0] += a[0];
                c.momentum[1] += a[1];
            } else {
                if c.booster_active {
                    if c.booster_burn > 0 {
                        let a = along2(c.yaw, 7.0 * U * c.booster_strength as f32 / 100.0 * c.eff as f32 / 100.0);
                        c.momentum[0] += a[0];
                        c.momentum[1] += a[1];
                        c.booster_burn -= 1;
                    } else {
                        c.booster_active = false;
                        c.boost_charge = 0;
                        // The weapon side only clears its restriction while draining a non-zero
                        // charge, so clearing the charge here must clear the restriction too, or
                        // the booster stays locked for the rest of the race.
                        c.boost_restriction = 0;
                    }
                }
                let fwd = along2(c.yaw, 1.0);
                let f = c.momentum[0] * fwd[0] + c.momentum[1] * fwd[1];
                let side = [c.momentum[0] - f * fwd[0], c.momentum[1] - f * fwd[1]];
                let fr = c.friction * U;
                let kf = 250.0 * U - fr;
                let ks = 243.0 * U - fr;
                c.momentum = [f * fwd[0] * kf + side[0] * ks, f * fwd[1] * kf + side[1] * ks];
                let lost = len2(side) * (1.0 - ks);
                c.momentum[0] += fwd[0] * lost * 0.6;
                c.momentum[1] += fwd[1] * lost * 0.6;
            }
            if c.brake_flag {
                c.momentum[0] *= 234.0 / 256.0;
                c.momentum[1] *= 234.0 / 256.0;
            }
        }
        // g movement delta
        let mut mv = [c.momentum[0] / 4.0 + c.push[0], c.momentum[1] / 4.0 + c.push[1], c.vert_disp];
        c.measured_vel = len2([mv[0], mv[1]]);
        mv[0] += catchup[0];
        mv[1] += catchup[1];
        for k in 0..2 {
            if mv[k] < -250.0 * U {
                mv[k] = -250.0 * U;
            } else if mv[k] >= 251.0 * U {
                mv[k] = 250.0 * U;
            }
        }
        c.push = [0.0; 2];
        // h craft-vs-craft
        self.craft_collision(c, i, &mut mv);
        // i sensors
        self.sensors(c, &mut mv, slope);
        // j walls
        self.walls(c, &mut mv);
        // k overlap stop
        for (j, o) in self.crafts.iter().enumerate() {
            if j == i || !o.solid() {
                continue;
            }
            let nx = c.pos[0] + mv[0];
            let ny = c.pos[1] + mv[1];
            if (nx - o.pos[0]).abs() < 60.0 * U && (ny - o.pos[1]).abs() < 60.0 * U && (c.pos[2] - o.pos[2]).abs() < 100.0 * U {
                mv[0] = 0.0;
                mv[1] = 0.0;
                break;
            }
        }
        // l hover height
        let (nx, ny) = (c.pos[0] + mv[0], c.pos[1] + mv[1]);
        let mut y = (c.sensors[0].0 + c.sensors[1].0) / 2.0;
        let ground = self.lvl.surface_below(nx, ny, y + 0.25);
        if y - ground < 151.0 * U {
            c.airborne = false;
            if y - ground < 100.0 * U {
                y = ground + 100.0 * U;
            }
        } else {
            c.airborne = true;
        }
        mv[2] = y - c.pos[2];
        // m roll
        if c.airborne {
            c.roll *= 15.0 / 16.0;
        } else {
            let d = c.sensor_floor[0] - c.sensor_floor[1];
            if d.abs() < 120.0 * U {
                c.roll += (atan2_raw(d, 120.0 * U) - c.roll) / 8.0;
                c.roll = c.roll.clamp(-4551.0, 4551.0);
            }
        }
        // n pitch
        if c.airborne {
            c.pitch = (c.pitch + 45.0).clamp(-8192.0, 8192.0);
        } else {
            let d = ground - (c.sensor_floor[0] + c.sensor_floor[1]) / 2.0;
            if d.abs() < 60.0 * U {
                c.pitch += (atan2_raw(d, 60.0 * U) - c.pitch) / 4.0;
                c.pitch = c.pitch.clamp(-4551.0, 4551.0);
            }
        }
        // o apply
        mv[0] = mv[0].clamp(-230.0 * U, 230.0 * U);
        mv[1] = mv[1].clamp(-230.0 * U, 230.0 * U);
        mv[2] = mv[2].clamp(-150.0 * U, 150.0 * U);
        c.pos[0] = (c.pos[0] + mv[0]).clamp(0.5, 255.5);
        c.pos[1] = (c.pos[1] + mv[1]).clamp(0.5, 159.5);
        c.pos[2] += mv[2];
        c.vert_disp = mv[2];
        c.last_move = mv;
    }

    fn craft_collision(&mut self, c: &mut Craft, i: usize, mv: &mut [f32; 3]) {
        let r = 60.0 * U;
        let s = [c.pos[0], c.pos[1]];
        let d = [mv[0], mv[1]];
        let dl = len2(d);
        let mut best: Option<(f32, usize)> = None;
        for (j, o) in self.crafts.iter().enumerate() {
            if j == i || !o.solid() || (o.pos[2] - c.pos[2]).abs() >= 100.0 * U {
                continue;
            }
            let op = [o.pos[0], o.pos[1]];
            let rel = [op[0] - s[0], op[1] - s[1]];
            let dist = len2(rel);
            if dist == 0.0 {
                mv[0] = 60.0 * U;
                mv[1] = 60.0 * U;
                return;
            }
            let mut hit = None;
            if dist < 2.0 * r - 10.0 * U {
                hit = Some(0.0);
            } else if dl > 0.0 && rel[0] * d[0] + rel[1] * d[1] > 0.0 && dist - 2.0 * r < dl {
                let t = (rel[0] * d[0] + rel[1] * d[1]) / dl;
                let perp2 = (dist * dist - t * t).max(0.0);
                if perp2.sqrt() < 2.0 * r {
                    let back = ((2.0 * r).powi(2) - perp2).max(0.0).sqrt();
                    hit = Some((t - back).max(0.0));
                }
            }
            if let Some(h) = hit {
                if best.map(|b| h < b.0).unwrap_or(true) {
                    best = Some((h, j));
                }
            }
        }
        if let Some((_, j)) = best {
            let o = &self.crafts[j];
            let rel = [o.pos[0] - s[0], o.pos[1] - s[1]];
            let l = len2(rel).max(1e-6);
            let n = [rel[0] / l, rel[1] / l];
            let tan = [n[1], -n[0]];
            let a = [mv[0] / 2.0, mv[1] / 2.0];
            let b = [o.momentum[0] / 8.0, o.momentum[1] / 8.0];
            let p = (a[0] + b[0]) * n[0] + (a[1] + b[1]) * n[1];
            let t = 2.0 * (a[0] * n[1] - a[1] * n[0]);
            mv[0] = p * n[0] + t * tan[0];
            mv[1] = p * n[1] + t * tan[1];
            let push = [p * n[0], p * n[1]];
            if push[0] != 0.0 || push[1] != 0.0 {
                c.aff_contact = true;
                let o = &mut self.crafts[j];
                o.push = push;
                o.aff_contact = true;
            }
        }
    }

    fn sensors(&mut self, c: &mut Craft, mv: &mut [f32; 3], _slope: [f32; 2]) {
        let step = |p: [f32; 3], yaw: f32, tilt: f32, d: f32| -> [f32; 3] {
            let a = along(yaw, tilt, d);
            [p[0] + a[0], p[1] + a[1], p[2] + a[2]]
        };
        let p1 = step(c.pos, c.yaw + 16384.0, c.roll, 60.0 * U);
        let fr = step(p1, c.yaw, c.pitch, 60.0 * U);
        let fl = step(fr, c.yaw - 16384.0, -c.roll, 120.0 * U);
        let rl = step(fl, -c.yaw, -c.pitch, 120.0 * U);
        let rr = step(rl, c.yaw + 16384.0, c.roll, 120.0 * U);
        let pts = [fl, fr, rl, rr];
        let behind = c.behind as f32 / 100.0;
        let mut xb = [false; 4];
        let mut rb = [false; 4];
        for k in 0..4 {
            let s = pts[k];
            let (nx, ny) = (s[0] + mv[0], s[1] + mv[1]);
            let h = s[2];
            if self.lvl.solid_at(nx, ny, h) {
                if self.lvl.solid_at(nx, s[1], h) {
                    xb[k] = true;
                }
                if self.lvl.solid_at(s[0], ny, h) {
                    rb[k] = true;
                }
            }
            let (mut zpos, mut zdisp) = c.sensors[k];
            let floor = self.lvl.surface_below(nx, ny, zpos.max(h) + 0.3);
            c.sensor_floor[k] = floor;
            let rest = floor + 50.0 * U;
            if zpos >= rest {
                zpos += zdisp;
            } else {
                if self.lvl.attr(nx.floor() as i32, ny.floor() as i32) & 4 == 0 {
                    let (cx, cy) = (nx.floor() as i32, ny.floor() as i32);
                    let h1 = self.lvl.corner_h(cx, cy);
                    let h2 = self.lvl.corner_h(cx + 1, cy);
                    let h3 = self.lvl.corner_h(cx + 1, cy + 1);
                    let h4 = self.lvl.corner_h(cx, cy + 1);
                    let sl = [h1 - h2 - h3 + h4, h1 + h2 - h3 - h4];
                    if sl[0].abs() >= 501.0 * U {
                        xb[k] = true;
                    }
                    if sl[1].abs() >= 501.0 * U {
                        rb[k] = true;
                    }
                }
                zdisp = ((rest - zpos) * SENSOR_REBOUND[k] / 256.0).min(80.0 * U);
                zpos = rest + zdisp;
            }
            if zpos - rest > 3.0 * U {
                zdisp -= 3.0 * behind * U;
            }
            zdisp = zdisp.max(-70.0 * behind * U);
            if zdisp >= 101.0 * U {
                zdisp = 100.0 * U;
            }
            c.sensors[k] = (zpos, zdisp);
        }
        if xb.iter().any(|&b| b) {
            if mv[0].abs() >= 11.0 * U {
                c.aff_bump = true;
            }
            mv[0] = 0.0;
            c.momentum[0] = -c.momentum[0] * 50.0 / 256.0;
        }
        if rb.iter().any(|&b| b) {
            if mv[1].abs() >= 11.0 * U {
                c.aff_bump = true;
            }
            mv[1] = 0.0;
            c.momentum[1] = -c.momentum[1] * 50.0 / 256.0;
        }
        for k in 0..4 {
            if xb[k] || rb[k] {
                let left = k == 0 || k == 2;
                let a = along2(if left { c.yaw + 16384.0 } else { c.yaw - 16384.0 }, 15.0 * U);
                mv[0] += a[0];
                mv[1] += a[1];
            }
        }
    }

    fn walls(&self, c: &mut Craft, mv: &mut [f32; 3]) {
        let mut tries = 5;
        let p = [c.pos[0], c.pos[1]];
        'outer: loop {
            let l = len2([mv[0], mv[1]]);
            if l == 0.0 {
                return;
            }
            let p2 = [p[0] + mv[0], p[1] + mv[1]];
            for w in &self.track.walls {
                let minx = w.a[0].min(w.b[0]) - 1.0;
                let maxx = w.a[0].max(w.b[0]) + 1.0;
                let miny = w.a[1].min(w.b[1]) - 1.0;
                let maxy = w.a[1].max(w.b[1]) + 1.0;
                if p[0] < minx || p[0] > maxx || p[1] < miny || p[1] > maxy {
                    continue;
                }
                if seg_intersect(p, p2, w.a, w.b) {
                    tries -= 1;
                    if tries == 0 {
                        mv[0] = 0.0;
                        mv[1] = 0.0;
                        return;
                    }
                    let md = heading(mv[0], mv[1]);
                    let along_dir = if diff(w.dir, md).abs() <= 16384.0 { w.dir } else { wrap(w.dir + 32768.0) };
                    // slide along the wall, turned slightly away from it
                    let side = diff(md, along_dir);
                    let dir = if side == 0.0 { along_dir } else { along_dir - side.signum() * 30.0 };
                    let a = along2(dir, l * 0.98);
                    mv[0] = a[0];
                    mv[1] = a[1];
                    mv[2] = mv[2].min(0.0).max(mv[2]);
                    let h = if diff(w.dir, c.yaw).abs() <= 16384.0 { w.dir } else { wrap(w.dir + 32768.0) };
                    let err = diff(h, c.yaw);
                    c.yaw = wrap(c.yaw + err / 16.0);
                    continue 'outer;
                }
            }
            return;
        }
    }

    fn post_process(&mut self, c: &mut Craft, i: usize) {
        // 1 fuel
        if c.lap > 0 {
            c.fuel = (c.fuel - (c.throttle as i32 + 39) / 40).max(0);
        } else {
            c.aff_invincible = true;
        }
        if c.aff_invincible {
            c.aff_invincible = false;
            c.inv = c.inv.max(100);
        }
        if c.finish_place > 0 {
            c.inv = 10;
        }
        // 2 counters
        if c.inv > 0 {
            c.inv -= 1;
        }
        if c.invis > 0 {
            c.invis -= 1;
        }
        // 3 damage
        let damaged = c.aff_damage != 0 || c.aff_bump || c.aff_explosion || c.aff_bullet;
        if damaged && c.inv == 0 {
            self.sound(13, Some(i), c.pos, if c.aff_explosion { 240 } else { 100 });
            let dmg = c.aff_damage.clamp(-10000, 10000);
            c.shield = (c.shield - dmg).max(0);
            c.shake = (c.shake + dmg.max(0)).min(1000);
            if c.aff_bullet {
                c.bullet_counter += 1;
            }
            if c.aff_missile {
                c.missile_counter += 1;
            }
        }
        // 4 contact sounds
        if c.aff_contact && c.inv == 0 {
            match c.contact_phase {
                0 => {
                    self.sound(13, Some(i), c.pos, 100);
                    c.contact_phase = 1;
                }
                1 => {
                    self.sound(19, Some(i), c.pos, 60);
                    c.contact_phase = 2;
                }
                _ => {}
            }
        } else if c.contact_phase != 0 {
            c.contact_phase = 0;
            self.stop_sound(19, Some(i));
        }
        // 5 chargers
        let can_charge = c.action == A_RACING || c.action == A_REPAIR;
        let msgs = [(9u16, 12u16), (8, 11), (10, 13)];
        let mut any_charging = false;
        for k in 0..3 {
            let val = match k {
                0 => c.shield,
                1 => c.fuel,
                _ => c.ammo,
            };
            let active = can_charge && c.aff_charger[k] && val < 10000 && (k != 0 || c.shield > 0);
            if active {
                let v = (val + 200).min(10000);
                match k {
                    0 => {
                        c.shield = v;
                        let f = (16 - c.shield * 16 / 10000).max(0);
                        c.bullet_counter = c.bullet_counter * f / 16;
                        c.missile_counter = c.missile_counter * f / 16;
                        if c.action == A_REPAIR {
                            c.inv = c.inv.max(2);
                        }
                    }
                    1 => c.fuel = v,
                    _ => c.ammo = v,
                }
                if !c.charging[k] && c.human {
                    self.banner_req(i, msgs[k].0);
                }
                c.charging[k] = true;
                any_charging = true;
            } else if c.charging[k] {
                c.charging[k] = false;
                if c.human && val >= 10000 {
                    self.banner_req(i, msgs[k].1);
                }
                if k == 0 && c.shield >= 10000 {
                    c.bullet_counter = 0;
                    c.missile_counter = 0;
                }
            }
        }
        if any_charging {
            self.sound(7, Some(i), c.pos, 100);
        } else if c.action != A_REPAIR {
            self.stop_sound(7, Some(i));
        }
        c.aff_charger = [false; 3];
        // 6 collectables
        let coll = std::mem::take(&mut c.aff_collect);
        if c.racing() {
            for m in coll {
                let msg = match m {
                    0 => {
                        c.inv = 100;
                        0
                    }
                    1 => {
                        c.invis = 250;
                        0
                    }
                    2 => {
                        c.shield += 2500;
                        c.bullet_counter = (c.bullet_counter - 10).max(0);
                        15
                    }
                    3 => {
                        c.shield = 10000;
                        c.bullet_counter = 0;
                        c.missile_counter = 0;
                        18
                    }
                    4 => {
                        c.shield = 20000;
                        c.bullet_counter = 0;
                        c.missile_counter = 0;
                        21
                    }
                    5 => {
                        c.ammo += 2500;
                        16
                    }
                    6 => {
                        c.ammo = 10000;
                        19
                    }
                    7 => {
                        c.ammo = 20000;
                        22
                    }
                    8 => {
                        c.fuel += 2500;
                        14
                    }
                    9 => {
                        c.fuel = 10000;
                        17
                    }
                    10 => {
                        c.fuel = 20000;
                        20
                    }
                    11 => {
                        c.upgrades[0] = (c.upgrades[0] + 1).min(3);
                        23
                    }
                    12 => {
                        c.upgrades[1] = (c.upgrades[1] + 1).min(3);
                        24
                    }
                    13 => {
                        c.upgrades[2] = (c.upgrades[2] + 1).min(3);
                        25
                    }
                    _ => 0,
                };
                if msg != 0 && c.human {
                    self.banner_req(i, msg);
                }
            }
        }
        // 7 death check
        if c.racing() && c.shield <= 0 && c.deaths_pending_check() {
            c.deaths += 1;
            if let Some(h) = c.hitter {
                c.killer = Some(h);
                if let Some(k) = self.crafts.get_mut(h) {
                    k.kills += 1;
                    k.kill_event = true;
                }
            }
        }
        // 8 warnings
        let vals = [c.fuel, c.shield, c.ammo];
        for k in 0..3 {
            if vals[k] > 4000 {
                c.warn[k] = 0;
            } else {
                if c.warn[k] < 64 {
                    c.warn[k] += 1;
                }
                if c.warn[k] == 1 && vals[k] > 0 && c.human {
                    self.banner_req(i, [2, 3, 4][k]);
                }
            }
        }
        // 9 clear
        c.aff_damage = 0;
        c.aff_explosion = false;
        c.aff_bullet = false;
        c.aff_missile = false;
        c.aff_bump = false;
        c.aff_contact = false;
        c.hitter = None;
        c.measured_vel *= 1.3;
    }

    fn behind_factor(&self, c: &mut Craft) {
        let place = c.place.max(1) as i32;
        if c.finish_place > 0 {
            c.behind = 130;
        } else {
            let target = if c.human && !self.demo {
                118 + 2 * place + if c.supercar > 0 { 10 } else { 0 }
            } else {
                let fast = c.personality & P_FAST != 0;
                let base = if fast { if place > 3 { 131 } else { 125 } } else { 120 };
                if c.personality & P_NOCATCH != 0 {
                    base
                } else {
                    base + c.difficulty as i32 * 5 / 3 + 4 * place - 4
                }
            };
            if c.behind < target {
                c.behind += 1;
            } else if c.behind > target {
                c.behind -= 1;
            }
        }
        c.eff = (c.shield + 999) * 10 / 10000 + 90;
    }

    fn bonuses(&mut self, c: &mut Craft) {
        // supercar
        c.sc_awarded = false;
        if c.lap_event {
            if c.sc_armed {
                c.sc_next = true;
            } else if c.place == 1 {
                c.sc_armed = true;
            }
        }
        if c.place != 1 {
            c.sc_armed = false;
            c.sc_kills = 0;
        }
        if c.sc_armed && c.kill_event {
            c.sc_kills += 1;
        }
        if c.sc_next {
            c.sc_next = false;
            if c.sc_kills >= c.sc_level {
                c.supercar = 200;
                c.sc_kills = 0;
                c.sc_awarded = true;
                c.sc_saved = c.upgrades;
                c.upgrades = [3; 3];
                c.sc_level += 1;
                c.bonus_score += 1000;
            }
        }
        if c.supercar > 0 {
            c.fuel = 10000;
            c.shield = 10000;
            c.ammo = 10000;
            c.supercar -= 1;
            if c.supercar == 0 {
                c.upgrades = c.sc_saved;
            }
        }
        c.lap_event = false;
        c.kill_event = false;
        // spin bonus
        c.spin_msg = 0;
        if c.airborne {
            let d = diff(c.yaw, c.spin_last_yaw);
            match c.spin_phase {
                0 => {
                    c.spin_sum = 0.0;
                    c.spin_phase = 1;
                }
                1 => {
                    if d < 1.0 {
                        c.spin_sum -= d;
                        c.spin_phase = 3;
                    } else {
                        c.spin_sum += d;
                        c.spin_phase = 2;
                    }
                }
                2 => {
                    if d < 0.0 {
                        c.spin_phase = 0;
                    } else {
                        c.spin_sum += d;
                    }
                }
                _ => {
                    if d < 1.0 {
                        c.spin_sum -= d;
                    } else {
                        c.spin_phase = 0;
                    }
                }
            }
            c.spin_last_yaw = c.yaw;
            if c.spin_sum > 120150.0 {
                c.spin_msg = 3;
            } else if c.spin_sum > 87382.0 {
                c.spin_bits |= 4;
            } else if c.spin_sum > 54614.0 {
                c.spin_bits |= 2;
            } else if c.spin_sum > 27307.0 {
                c.spin_bits |= 1;
            }
        } else {
            c.spin_phase = 0;
            c.spin_last_yaw = c.yaw;
            if c.spin_bits & 8 != 0 {
                c.spin_msg = 3;
                c.bonus_score += 720;
            } else if c.spin_bits & 4 != 0 {
                c.spin_msg = 4;
                c.bonus_score += 540;
            } else if c.spin_bits & 2 != 0 {
                c.spin_msg = 4;
                c.bonus_score += 360;
            } else if c.spin_bits & 1 != 0 {
                c.spin_msg = 5;
                c.bonus_score += 180;
            }
            c.spin_bits = 0;
        }
    }

    fn spray(&mut self, c: &mut Craft) {
        if c.airborne || c.throttle <= 0.0 || !c.racing() {
            return;
        }
        let (x, y) = (c.pos[0] + c.last_move[0], c.pos[1] + c.last_move[1]);
        let a = self.lvl.attr(x.floor() as i32, y.floor() as i32);
        let kind = if a & 1 != 0 {
            TK::Sand
        } else if a & 2 != 0 {
            TK::Water
        } else {
            return;
        };
        for _ in 0..4 {
            let mut t = Thing::new(kind, c.pos, 0);
            let r = self.seed.below(32767) as f32;
            t.yaw = wrap(c.yaw + 16384.0 + r);
            t.pitch = 16384.0;
            t.speed = 100.0 * U;
            t.disp = c.last_move;
            t.life = 12;
            t.sprite = if kind == TK::Sand { 24 } else { 40 };
            t.pos[2] = self.floor(c.pos[0], c.pos[1], c.pos[2]);
            self.spawn(t);
        }
    }

    fn checkpoints(&mut self, c: &mut Craft, i: usize) {
        if self.track.checkpoints.is_empty() || !c.active {
            return;
        }
        let t = c.target_cp.min(self.track.checkpoints.len() - 1);
        if self.track.checkpoints[t].contains(c.pos[0], c.pos[1]) {
            c.prev_cp = c.last_cp;
            c.last_cp = t;
            c.target_cp = self.track.next_checkpoint(t);
            if self.track.checkpoints[t].value == 0 {
                // lap logic
                if c.lap > 0 {
                    c.laps_list.push(c.lap_time);
                    if c.best_lap.map(|b| c.lap_time < b).unwrap_or(true) {
                        c.best_lap = Some(c.lap_time);
                    }
                    c.lap_time = 0;
                }
                c.lap += 1;
                c.lap_event = true;
                self.lap_timer = true;
                if c.lap >= c.race_laps && c.finish_place == 0 {
                    // finished crafts lead the placings, so the current place is the finishing order
                    let done = self.crafts.iter().filter(|o| o.active && o.finish_place > 0).count() as u8;
                    c.finish_place = (done + 1).min(8);
                    c.finish_time = c.race_time;
                    c.hold = 100;
                    c.finished_event = true;
                }
            }
        }
        let tt = c.target_cp.min(self.track.checkpoints.len() - 1);
        c.cp_dist = self.track.checkpoints[tt].dist(c.pos[0], c.pos[1]);
        let _ = i;
    }
}

impl Craft {
    /// The death is counted once: when the shield first runs out while racing (action switches next tick).
    pub fn deaths_pending_check(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::level::Level;

    /// Drive straight into block walls (all four directions) and count how often the craft ends up
    /// on top of the blocks instead of being stopped.
    fn wall_climbs(track: usize) -> (usize, usize) {
        let lvl = Level::load(track).expect("level");
        let mut w = World::new(lvl, Vec::new());
        let mut cases = Vec::new();
        for y in 4..156 {
            for x in 4..252 {
                let Some(col) = w.lvl.column(x, y) else { continue };
                if col.mask & 3 != 3 {
                    continue; // want a wall at least two blocks high from the ground
                }
                for (dx, dy) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                    // approach from 3 cells away, through three free cells
                    let free = (1..=3).all(|k| w.lvl.column(x - dx * k, y - dy * k).is_none());
                    if !free {
                        continue;
                    }
                    let h0 = w.lvl.terrain_height(x as f32 + 0.5 - dx as f32 * 2.0, y as f32 + 0.5 - dy as f32 * 2.0);
                    let h1 = w.lvl.terrain_height(x as f32 + 0.5, y as f32 + 0.5);
                    if (h0 - h1).abs() > 0.3 {
                        continue;
                    }
                    cases.push((x, y, dx, dy));
                }
            }
        }
        cases.truncate(200);
        let mut climbs = 0;
        for &(x, y, dx, dy) in &cases {
            w.crafts.clear();
            let mut c = Craft::default();
            c.active = true;
            c.human = true;
            c.pos = [x as f32 + 0.5 - dx as f32 * 3.0, y as f32 + 0.5 - dy as f32 * 3.0, 0.0];
            c.yaw = heading(dx as f32, dy as f32);
            w.init_craft(&mut c);
            c.started = true;
            c.lap = 1;
            w.crafts.push(c);
            let input = HumanIn { accel: true, ..Default::default() };
            for t in 0..80 {
                let mut inp = input;
                // wiggle a little, as a player does against a wall
                inp.left = (t / 6) % 3 == 1;
                inp.right = (t / 6) % 3 == 2;
                w.update_craft(0, inp);
            }
            let c = &w.crafts[0];
            let (cx, cy) = (c.pos[0].floor() as i32, c.pos[1].floor() as i32);
            if w.lvl.column(cx, cy).is_some() && c.pos[2] > w.lvl.terrain_height(c.pos[0], c.pos[1]) + 1.0 {
                climbs += 1;
            }
        }
        (climbs, cases.len())
    }

    #[test]
    fn crafts_do_not_climb_block_walls() {
        crate::data::files::set_data_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/resources").into());
        let mut report = Vec::new();
        for track in 1..=6 {
            let (c, n) = wall_climbs(track);
            report.push(format!("track {track}: {c} of {n}"));
        }
        println!("{}", report.join("\n"));
        assert!(report.iter().all(|r| r.contains(": 0 of")), "{}", report.join("\n"));
    }
}
