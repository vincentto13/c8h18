//! Computer drivers (spec 07) and the recovery trucks (spec 05 §8).
use super::math::*;
use super::sim::*;

impl World {
    pub fn autopilot(&mut self, c: &mut Craft, i: usize) {
        // 1 first tick
        if !c.ap_first {
            c.ap_first = true;
            c.waypoint = self.track.nearest(c.pos[0], c.pos[1]);
            return;
        }
        // 2 stuck check
        if c.stuck_count == 0 {
            c.stuck_pos = [c.pos[0], c.pos[1]];
        }
        c.stuck_count += 1;
        if c.stuck_count > 50 {
            c.stuck_count = 1;
            if len2([c.pos[0] - c.stuck_pos[0], c.pos[1] - c.stuck_pos[1]]) < 2.0 {
                c.action = A_CALL;
                c.destroyed = false;
                c.stuck_count = 0;
                return;
            }
            c.stuck_pos = [c.pos[0], c.pos[1]];
        }
        // 3
        if c.waypoint == 0 {
            c.waypoint = self.track.absolute_nearest(c.pos[0], c.pos[1]);
            if c.waypoint == 0 {
                return;
            }
        }
        // 4 waypoint progress
        let v = c.measured_vel;
        let dd = if v < 50.0 * U {
            3.0
        } else if v < 100.0 * U {
            4.0
        } else if v < 150.0 * U {
            5.0
        } else {
            6.0
        };
        let w = self.track.wps[c.waypoint];
        if len2([w.x - c.pos[0], w.y - c.pos[1]]) < dd {
            c.hold_fire = w.kind == 8;
            c.fire_at_will = w.kind == 9;
            if w.kind == 7 {
                c.brake_count = 3;
            }
            c.last_wp = c.waypoint;
            c.waypoint = w.child;
            if c.waypoint == 0 {
                c.waypoint = self.track.absolute_nearest(c.pos[0], c.pos[1]);
            }
            if c.wants[0] {
                c.waypoint = self.track.junction(c.waypoint, 2);
            } else if c.wants[1] {
                c.waypoint = self.track.junction(c.waypoint, 4);
            } else if c.wants[2] {
                c.waypoint = self.track.junction(c.waypoint, 3);
            } else if !c.human {
                let s = ((c.pos[0] * 256.0) as i32 + (c.pos[1] * 256.0) as i32) & 3;
                if s == 0 {
                    c.waypoint = self.track.junction(c.waypoint, 6);
                }
            }
        }
        // 5 evasion counters
        if c.evade1 == 0 && c.personality & P_FAST == 0 && c.hits_run.iter().any(|&h| h > 10) {
            c.evade1 = 22;
            c.evade2 = 3;
        }
        if c.evade1 == 0 && c.targeted_close {
            c.evade1 = 23;
            c.evade2 = 1;
        }
        c.targeted_close = false;
        if c.evade1 > 0 {
            c.evade1 -= 1;
        }
        if c.evade2 > 0 {
            c.evade2 -= 1;
        }
        // 6 charger stop
        let want_pad = if c.wants[0] {
            Some(2)
        } else if c.wants[2] {
            Some(3)
        } else if c.wants[1] {
            Some(1)
        } else {
            None
        };
        if let Some(k) = want_pad {
            if self.track.chargers.iter().any(|r| r.kind == k && r.contains_cell(c.pos[0], c.pos[1])) {
                c.speed_in = -26.0;
                c.momentum[0] *= 15.0 / 16.0;
                c.momentum[1] *= 15.0 / 16.0;
                c.steer_in = 0.0;
                if c.fuel >= 10000 {
                    c.wants[0] = false;
                }
                if c.shield >= 10000 {
                    c.wants[1] = false;
                }
                if c.ammo >= 10000 {
                    c.wants[2] = false;
                }
                return;
            }
        }
        // 7 steering and throttle
        let w = self.track.wps[c.waypoint];
        let steer = if c.evade1 < 20 {
            let d = diff(heading(w.x - c.pos[0], w.y - c.pos[1]), c.yaw);
            if d < 0.0 { ((d + 22.0) / 32.0).trunc() } else { (d / 32.0).trunc() }
        } else {
            1294.0
        };
        c.speed_in = if c.evade2 != 0 || c.brake_count > 0 { -26.0 } else { 26.0 };
        if c.brake_count > 0 {
            c.brake_count -= 1;
        }
        let clamped = steer.clamp(-100.0, 100.0);
        let mut si = if clamped >= 0.0 { clamped + (c.prev_steer / 8.0).trunc() } else { clamped - (c.prev_steer / 8.0).trunc() };
        if si.abs() < 1.0 {
            si = 0.0;
        }
        c.steer_in = si;
        c.prev_steer = si;
        // 8 booster
        if c.personality & P_FAST != 0 && !c.human {
            c.upgrades[2] = c.upgrades[2].max(1);
        }
        if c.difficulty != 0 && c.lap > 0 && !c.hold_fire && (c.fire_at_will || clamped < 18.0) {
            c.trig_boost = c.fuel > 0;
        }
        // 9 targeting
        self.targeting(c, i);
        // 10 close target
        c.close_to_target = false;
        if let Some(t) = c.target {
            if dist2(self.crafts[t].pos, c.pos) < 5.0 {
                c.close_to_target = true;
                self.crafts[t].targeted_close = true;
            }
        }
        // 11 weapons
        if c.lap > 0 && c.target.is_some() {
            if c.personality & P_STEADY != 0 || (c.time_slice as u32 % 150) < 80 {
                c.heat = 0;
                c.upgrades[0] = c.difficulty as i32;
                c.trig_gun = true;
            }
            if c.lock > 80 && c.time_slice % 9 == 0 && !c.close_to_target {
                c.upgrades[1] = c.difficulty as i32;
                c.trig_missile = true;
            }
        }
        // 12 charger wishes
        if c.fuel < 3000 {
            c.wants[0] = true;
        }
        if c.shield < 5000 {
            c.wants[1] = true;
        }
        if c.ammo < 3000 {
            c.wants[2] = true;
        }
    }

    fn truck_move(&self, t: &mut Truck, target: [f32; 2], mut s: f32, d: f32) {
        let df = diff(heading(target[0] - t.pos[0], target[1] - t.pos[1]), t.yaw);
        if d < 8.0 {
            s = s.min(100.0 * U) * d / 8.0 + 15.0 * U;
        }
        if df > 91.0 {
            s = 0.0;
        }
        t.yaw = wrap(t.yaw + df.clamp(-1820.0, 1820.0));
        let a = along2(t.yaw, s);
        t.pos[0] += a[0];
        t.pos[1] += a[1];
        let g = self.lvl.terrain_height(t.pos[0], t.pos[1]);
        t.pos[2] += (g + 6.0 - t.pos[2]).clamp(-30.0 * U, 40.0 * U);
    }

    fn drop_waypoint(&self, c: &Craft) -> usize {
        let tr = &self.track;
        let mut best = (f32::MAX, 0usize);
        if !tr.checkpoints.is_empty() {
            let last = tr.checkpoints[c.last_cp.min(tr.checkpoints.len() - 1)];
            let next = tr.checkpoints[tr.next_checkpoint(c.last_cp.min(tr.checkpoints.len() - 1))];
            if let Some(mut w) = (1..tr.wps.len()).find(|&k| last.contains(tr.wps[k].x, tr.wps[k].y)) {
                for _ in 0..30 {
                    if w == 0 {
                        break;
                    }
                    let p = tr.wps[w];
                    let d = len2([p.x - c.pos[0], p.y - c.pos[1]]);
                    if d < best.0 && p.child != 0 {
                        best = (d, w);
                    }
                    if next.contains(p.x, p.y) {
                        break;
                    }
                    w = p.child;
                }
            }
        }
        if best.1 == 0 { tr.nearest(c.pos[0], c.pos[1]).max(1).min(tr.wps.len().saturating_sub(1)) } else { best.1 }
    }

    pub fn update_trucks(&mut self) {
        for ti in 0..self.trucks.len() {
            let mut t = self.trucks[ti].clone();
            t.prev_pos = t.pos;
            t.prev_yaw = t.yaw;
            match t.state {
                TruckState::Idle => {
                    let served: Vec<usize> = self.trucks.iter().filter_map(|x| x.craft).collect();
                    if let Some(ci) = (0..self.crafts.len()).find(|&k| self.crafts[k].active && self.crafts[k].action == A_CALL && !served.contains(&k)) {
                        self.crafts[ci].action = A_TRUCK_COMING;
                        self.crafts[ci].truck = Some(ti);
                        t.craft = Some(ci);
                        t.state = TruckState::Approach;
                    }
                }
                TruckState::Approach => {
                    let ci = t.craft.unwrap();
                    let cp = self.crafts[ci].pos;
                    let d = dist2(t.pos, cp);
                    if d < 6.0 {
                        t.state = TruckState::Descend;
                        t.count = 20;
                    } else {
                        self.truck_move(&mut t, [cp[0], cp[1]], 220.0 * U, d);
                    }
                }
                TruckState::Descend => {
                    let ci = t.craft.unwrap();
                    let cp = self.crafts[ci].pos;
                    let goal = [cp[0], cp[1], cp[2] + 200.0 * U];
                    for k in 0..3 {
                        t.pos[k] += (goal[k] - t.pos[k]) / t.count as f32;
                    }
                    t.count -= 1;
                    if t.count <= 0 {
                        let c = &self.crafts[ci];
                        t.drop_wp = self.drop_waypoint(c);
                        self.crafts[ci].action = A_REPAIR;
                        self.crafts[ci].reposition = true;
                        t.state = TruckState::Carry;
                        t.count = 0;
                    }
                }
                TruckState::Carry => {
                    let ci = t.craft.unwrap();
                    let w = self.track.wps[t.drop_wp];
                    if t.count == 0 {
                        let d = len2([w.x - t.pos[0], w.y - t.pos[1]]);
                        if d < 4.0 {
                            t.count = 1;
                        } else {
                            self.truck_move(&mut t, [w.x, w.y], 150.0 * U, d);
                            let c = &mut self.crafts[ci];
                            c.pos = [t.pos[0], t.pos[1], t.pos[2] - 200.0 * U];
                        }
                    } else {
                        let busy = self.trucks.iter().enumerate().any(|(k, o)| {
                            k != ti && o.drop_wp == t.drop_wp && ((o.state == TruckState::Carry && o.count != 0) || o.state == TruckState::Lower || o.state == TruckState::Release)
                        });
                        if busy && w.child != 0 {
                            t.drop_wp = w.child;
                            t.count = 0;
                        } else {
                            t.state = TruckState::Lower;
                            t.count = 14;
                        }
                    }
                    self.turn_carried(&t, ci);
                }
                TruckState::Lower => {
                    let ci = t.craft.unwrap();
                    let w = self.track.wps[t.drop_wp];
                    let goal = [w.x, w.y, w.floor + 400.0 * U];
                    for k in 0..3 {
                        t.pos[k] += (goal[k] - t.pos[k]) / t.count as f32;
                    }
                    self.crafts[ci].pos = [t.pos[0], t.pos[1], t.pos[2] - 200.0 * U];
                    t.count -= 1;
                    if t.count <= 2 {
                        t.state = TruckState::Release;
                        t.count = 0;
                    }
                    self.turn_carried(&t, ci);
                }
                TruckState::Release => match t.count {
                    0 => {
                        let ci = t.craft.unwrap();
                        self.turn_carried(&t, ci);
                        if self.crafts[ci].action == A_REPAIRED {
                            t.count = 1;
                        }
                    }
                    1 => {
                        let ci = t.craft.unwrap();
                        let c = &mut self.crafts[ci];
                        c.waypoint = t.drop_wp;
                        c.ap_first = true;
                        c.action = A_RESET;
                        t.count = 2;
                        t.craft = None;
                    }
                    _ => {
                        let g = self.lvl.terrain_height(t.pos[0], t.pos[1]);
                        if t.pos[2] > g + 5.0 {
                            t.state = TruckState::Idle;
                            t.count = 0;
                        } else {
                            t.pos[2] += 25.0 * U;
                        }
                    }
                },
            }
            self.trucks[ti] = t;
        }
    }

    fn turn_carried(&mut self, t: &Truck, ci: usize) {
        let w = self.track.wps[t.drop_wp];
        let n = if w.child != 0 { self.track.wps[w.child] } else { w };
        let c = &mut self.crafts[ci];
        let h = heading(n.x - c.pos[0], n.y - c.pos[1]);
        c.yaw = wrap(c.yaw + diff(h, c.yaw).clamp(-10922.0, 10922.0));
        c.view = [c.yaw, c.pitch, c.roll];
    }
}
