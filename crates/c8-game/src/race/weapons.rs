//! Targeting, weapons, explosions, effect things, collectables (spec 06).
use super::math::*;
use super::sim::*;

const CRAFT_HS: f32 = 32.0 * U;

fn overlap(a: [f32; 3], ha: f32, b: [f32; 3], hb: f32) -> bool {
    (a[0] - b[0]).abs() < ha + hb && (a[1] - b[1]).abs() < ha + hb && (a[2] - b[2]).abs() < ha + hb
}

impl World {
    /// Targeting step (spec 06 §2).
    pub fn targeting(&mut self, c: &mut Craft, _i: usize) {
        let mut best: Option<(f32, usize)> = None;
        for (j, o) in self.crafts.iter().enumerate() {
            if !o.active || j == c.slot {
                continue;
            }
            let d = dist3(c.pos, o.pos);
            if d >= 15.0 || d == 0.0 {
                continue;
            }
            let b = heading(o.pos[0] - c.pos[0], o.pos[1] - c.pos[1]);
            if diff(b, c.yaw).abs() >= 5461.0 {
                continue;
            }
            let e = atan2_raw(c.pos[2] - o.pos[2], dist2(c.pos, o.pos));
            if diff(e, c.pitch).abs() >= 5461.0 {
                continue;
            }
            if best.map(|b| d < b.0).unwrap_or(true) {
                best = Some((d, j));
            }
        }
        let res = best.map(|b| b.1);
        if res != c.target {
            c.target = res;
            c.lock = 0;
        }
        if let Some(t) = c.target {
            let o = &self.crafts[t];
            let drop = (c.personality & P_SPARE != 0 && !c.human && o.shield < 2000) || o.inv > 0 || o.shield < 1 || !o.racing();
            if drop {
                c.lock = 0;
                c.target = None;
                return;
            }
            c.lock = (c.lock + c.upgrades[1] + 1).min(100);
        } else {
            c.lock = 0;
        }
    }

    fn aim(&self, c: &Craft) -> (f32, f32) {
        if let Some(t) = c.target.and_then(|t| self.crafts.get(t)) {
            (heading(t.pos[0] - c.pos[0], t.pos[1] - c.pos[1]), atan2_raw(c.pos[2] - t.pos[2], dist2(c.pos, t.pos)))
        } else {
            (c.view[0], c.view[1])
        }
    }

    /// Booster weapon (charging / release), run before the craft update.
    pub fn booster_weapon(&mut self, i: usize) {
        let c = &self.crafts[i];
        if !c.active {
            return;
        }
        let resources = c.ammo >= 6 && c.fuel >= 6;
        let (pos, airborne) = (c.pos, c.airborne);
        let c = &mut self.crafts[i];
        if !c.trig_boost || c.boost_restriction != 0 || c.boost_charge >= 100 || !resources {
            if c.boost_charge > 0 && !airborne {
                let release = c.boost_restriction == 0;
                if release {
                    c.aff_booster = true;
                    c.booster_strength = c.boost_charge;
                }
                c.boost_charge -= 1;
                c.boost_restriction = if c.boost_charge > 0 { c.boost_restriction + 1 } else { 0 };
                if release {
                    self.stop_sound(8, Some(i));
                    self.sound(9, Some(i), pos, 100);
                }
            } else if c.boost_charge == 0 {
                self.stop_sound(8, Some(i));
            }
        } else {
            c.ammo -= 3;
            c.fuel -= 3;
            c.boost_charge += 2 + c.upgrades[2];
            let rate = 2 * c.boost_charge + 100;
            self.sound(8, Some(i), pos, rate);
        }
        self.crafts[i].trig_boost = false;
    }

    /// Minigun and missile launcher, run after all crafts moved.
    pub fn guns(&mut self, i: usize) {
        if !self.crafts[i].active {
            return;
        }
        let (yaw, pitch) = self.aim(&self.crafts[i]);
        // minigun
        let c = &mut self.crafts[i];
        let mut fire = false;
        if c.trig_gun {
            if c.heat < 100 {
                fire = true;
            } else {
                if c.gun_restriction == 0 {
                    fire = true;
                    c.gun_restriction = 10;
                } else {
                    c.gun_restriction -= 1;
                    let p = c.pos;
                    self.stop_sound(3, Some(i));
                    self.sound(4, Some(i), p, 100);
                }
                self.crafts[i].heat = 100;
            }
        } else {
            c.heat = (c.heat - [10, 17, 25, 33][c.upgrades[0].clamp(0, 3) as usize]).max(0);
        }
        let c = &mut self.crafts[i];
        if fire {
            c.barrel = !c.barrel;
            let side = if c.barrel { 16384.0 } else { -16384.0 };
            let o = along2(c.yaw + side, 32.0 * U);
            let pos = [c.pos[0] + o[0], c.pos[1] + o[1], c.pos[2] - 8.0 * U];
            c.heat += 2;
            c.bullets_fired += 1;
            let upg = c.upgrades[0];
            let mut t = Thing::new(TK::Bullet, pos, 0);
            t.yaw = yaw;
            t.pitch = pitch;
            t.owner = Some(i);
            t.life = 128;
            t.upgrade = upg;
            self.spawn(t);
            self.sound(3, Some(i), pos, 100);
        }
        self.crafts[i].trig_gun = false;
        // missiles
        let c = &mut self.crafts[i];
        if c.reload > 0 {
            c.reload -= 1;
        } else if c.trig_missile && c.ammo > 1665 {
            c.ammo -= 1666;
            let count = if c.upgrades[1] >= 3 { 2 } else { 1 };
            let (lock, target, upg) = (c.lock, c.target, c.upgrades[1]);
            for _ in 0..count {
                let c = &mut self.crafts[i];
                c.rockets_launched += 1;
                c.missile_side = !c.missile_side;
                let side = if c.missile_side { 16384.0 } else { -16384.0 };
                let o = along2(c.yaw + side, 80.0 * U);
                let pos = [c.pos[0] + o[0], c.pos[1] + o[1], c.pos[2] - 8.0 * U];
                let mut t = Thing::new(TK::Missile, pos, 0);
                t.yaw = yaw;
                t.pitch = pitch;
                t.owner = Some(i);
                t.life = 113;
                t.sprite = 152;
                t.target = target;
                t.upgrade = upg;
                let idx = self.spawn(t);
                let r = self.things[idx].seed.below(100) as i32;
                self.things[idx].homing = r < lock;
            }
            self.crafts[i].reload = 12;
            let p = self.crafts[i].pos;
            self.sound(5, Some(i), p, 100);
        }
        self.crafts[i].trig_missile = false;
    }

    pub fn smoke_fire(&mut self, pos: [f32; 3], dz: f32) {
        let mut t = Thing::new(TK::SmokeFire, [pos[0], pos[1], pos[2] + dz], 0);
        t.life = 22;
        t.sprite = 5;
        t.first = false;
        self.spawn(t);
    }

    /// Create an explosion thing. `damage` false = made without damage (dying crafts).
    pub fn explosion(&mut self, kind: TK, pos: [f32; 3], yaw: f32, pitch: f32, damage: bool, owner: Option<usize>) {
        let mut t = Thing::new(kind, pos, 0);
        t.yaw = yaw;
        t.pitch = pitch;
        t.owner = owner;
        t.life = match kind {
            TK::ExplMedium => 3,
            TK::ExplBig => 6,
            _ => 3,
        };
        t.damage = if damage {
            match kind {
                TK::ExplMedium => 1500,
                _ => 10000,
            }
        } else {
            0
        };
        self.spawn(t);
    }

    fn particle(&mut self, pos: [f32; 3], yaw: f32, pitch: f32, fast: bool, moving: bool) {
        let mut t = Thing::new(TK::Particle { fast }, pos, 0);
        t.yaw = yaw;
        t.pitch = pitch;
        t.life = 12;
        t.sprite = if fast { 140 } else { 139 };
        t.first = false;
        if moving {
            t.disp = along(yaw, pitch, 150.0 * U);
        }
        self.spawn(t);
    }

    fn flare(&mut self, pos: [f32; 3]) {
        let mut t = Thing::new(TK::Flare, pos, 0);
        t.first = false;
        let idx = self.spawn(t);
        let th = &mut self.things[idx];
        let speed = (40 + th.seed.below(100)) as f32 * U;
        th.yaw = th.seed.below(65535) as f32;
        th.pitch = -(3640.0 + th.seed.below(12743) as f32);
        th.life = 20;
        th.disp = along(th.yaw, th.pitch, speed);
    }

    /// Apply area damage of an explosion / spark to crafts and "on effect" triggers.
    fn area_damage(&mut self, pos: [f32; 3], hs: f32, amount: i32, owner: Option<usize>, bullet: bool, explosion: bool) {
        for (j, c) in self.crafts.iter_mut().enumerate() {
            if !c.active || Some(j) == owner && bullet {
                continue;
            }
            if overlap(pos, hs, c.pos, CRAFT_HS) {
                c.aff_damage += amount;
                if bullet {
                    c.aff_bullet = true;
                    if let Some(o) = owner {
                        c.hits_tick[o] += 1;
                    }
                }
                if explosion {
                    c.aff_explosion = true;
                    if amount >= 1500 && owner.is_some() {
                        c.aff_missile = true;
                    }
                }
                if owner.is_some() && owner != Some(j) {
                    c.hitter = owner;
                }
            }
        }
        if explosion && amount > 0 {
            for tr in self.triggers.iter_mut().filter(|t| t.alive && t.sub == 3) {
                let (tx, ty) = (tr.x as f32 + 0.5, tr.y as f32 + 0.5);
                if (tx - pos[0]).abs() < hs + 0.5 && (ty - pos[1]).abs() < hs + 0.5 {
                    tr.absorb -= amount;
                }
            }
        }
    }

    /// Process all effect things for one tick.
    pub fn update_things(&mut self) {
        let n = self.things.len();
        for i in 0..n {
            if !self.things[i].alive {
                continue;
            }
            let mut t = std::mem::replace(&mut self.things[i], Thing::new(TK::Smoke, [0.0; 3], 0));
            t.alive = true;
            t.prev_pos = t.pos;
            t.age += 1;
            self.update_thing(&mut t);
            // the slot may have been reused by spawn? spawn only reuses dead slots; placeholder is alive=true
            self.things[i] = t;
        }
    }

    fn update_thing(&mut self, t: &mut Thing) {
        match t.kind {
            TK::Bullet => {
                for _ in 0..16 {
                    let a = along(t.yaw, t.pitch, 64.0 * U);
                    t.pos = [t.pos[0] + a[0], t.pos[1] + a[1], t.pos[2] + a[2]];
                    let mut hit = self.lvl.collide(t.pos[0], t.pos[1], t.pos[2]);
                    if !hit {
                        for (j, c) in self.crafts.iter().enumerate() {
                            if c.active && Some(j) != t.owner && overlap(t.pos, 32.0 * U, c.pos, CRAFT_HS) {
                                hit = true;
                                break;
                            }
                        }
                        if hit {
                            if let Some(o) = t.owner {
                                self.crafts[o].bullets_hit += 1;
                            }
                        }
                    }
                    if hit {
                        let r = t.seed.r();
                        let rate = 75 + (t.seed.r() % 51) as i32;
                        self.sound(10 + (r % 3) as u16, None, t.pos, rate);
                        let n = if t.upgrade >= 2 { 8 } else { 4 };
                        for k in 0..n {
                            t.seed.r();
                            t.seed.r();
                            let dz = (t.seed.r() % 48) as f32 - 24.0;
                            let mut s = Thing::new(TK::Spark, [t.pos[0], t.pos[1], t.pos[2] + dz * U], 0);
                            s.life = 4;
                            s.sprite = 165;
                            s.owner = t.owner;
                            s.damage = if k == 0 { 60 } else { 0 };
                            self.spawn(s);
                        }
                        t.alive = false;
                        return;
                    }
                    t.life -= 1;
                    if t.life <= 0 {
                        t.alive = false;
                        return;
                    }
                }
            }
            TK::Spark => {
                if t.first {
                    t.first = false;
                    if t.damage > 0 {
                        self.area_damage(t.pos, 1.0, t.damage, t.owner, true, false);
                    }
                }
                t.yaw += 2912.0;
                t.sprite = 165 + (t.age - 1).min(3);
                t.life -= 1;
                if t.life <= 0 {
                    t.alive = false;
                }
            }
            TK::Missile => {
                for _ in 0..4 {
                    if t.homing {
                        if let Some(tc) = t.target.and_then(|x| self.crafts.get(x)).filter(|c| c.active) {
                            let tp = tc.pos;
                            let h = heading(tp[0] - t.pos[0], tp[1] - t.pos[1]);
                            let p = atan2_raw(t.pos[2] - tp[2], dist2(t.pos, tp));
                            t.yaw = wrap(t.yaw + diff(h, t.yaw).clamp(-182.0, 182.0));
                            t.pitch += diff(p, t.pitch).clamp(-182.0, 182.0);
                            let d = dist3(t.pos, tp);
                            let ti = t.target.unwrap();
                            let c = &mut self.crafts[ti];
                            if c.incoming_missile.map(|x| d < x).unwrap_or(true) {
                                c.incoming_missile = Some(d);
                            }
                        }
                    }
                    let a = along(t.yaw, t.pitch, 72.0 * U);
                    t.pos = [t.pos[0] + a[0], t.pos[1] + a[1], t.pos[2] + a[2]];
                    let mut explode = false;
                    if self.lvl.solid_at(t.pos[0], t.pos[1], t.pos[2]) {
                        explode = true;
                    } else {
                        let f = self.lvl.terrain_height(t.pos[0], t.pos[1]);
                        if t.pos[2] < f - 18.0 * U {
                            explode = true;
                        } else if t.pos[2] < f + 8.0 * U {
                            t.pos[2] = f + 8.0 * U;
                        }
                    }
                    if !explode {
                        for (j, c) in self.crafts.iter().enumerate() {
                            if c.active && Some(j) != t.owner && overlap(t.pos, 64.0 * U, c.pos, CRAFT_HS) {
                                explode = true;
                                break;
                            }
                        }
                        if explode {
                            if let Some(o) = t.owner {
                                self.crafts[o].rockets_hit += 1;
                            }
                        }
                    }
                    if !explode && t.seed.below(3) < 2 {
                        let mut s = Thing::new(TK::Smoke, [t.pos[0], t.pos[1], t.pos[2] + 1.0 / 16.0], 0);
                        s.yaw = t.yaw;
                        s.pitch = t.pitch;
                        s.speed = (16 + (t.seed.r() & 15)) as f32 * U;
                        s.life = 6;
                        s.sprite = 60;
                        self.spawn(s);
                    }
                    t.life -= 1;
                    if explode || t.life < 1 {
                        if t.upgrade == 2 {
                            for h in [120.0 * DEG, 0.0, 240.0 * DEG] {
                                let o = along2(h, 2.0);
                                let p = [t.pos[0] + o[0], t.pos[1] + o[1], t.pos[2]];
                                let mut e = Thing::new(TK::ExplMedium, p, 0);
                                e.yaw = t.yaw;
                                e.pitch = t.pitch;
                                e.life = 3;
                                e.damage = 3000;
                                e.owner = t.owner;
                                self.spawn(e);
                            }
                        } else {
                            self.explosion(TK::ExplMedium, t.pos, t.yaw, t.pitch, true, t.owner);
                        }
                        t.alive = false;
                        return;
                    }
                }
            }
            TK::ExplMedium | TK::ExplBig | TK::ExplMassive => {
                if t.first {
                    t.first = false;
                    let hs = match t.kind {
                        TK::ExplMedium => 1.0,
                        TK::ExplBig => 1.5,
                        _ => 2.0,
                    };
                    self.area_damage(t.pos, hs, t.damage, t.owner, false, true);
                    self.sound(6, None, t.pos, 100);
                    t.age = 0;
                }
                let n = t.age;
                match t.kind {
                    TK::ExplMedium => {
                        for _ in 0..6 {
                            let d = (t.seed.r() & 255) as f32 * U;
                            let a = t.seed.r() as f32;
                            t.seed.r();
                            let o = along2(a, d);
                            self.particle([t.pos[0] + o[0], t.pos[1] + o[1], t.pos[2]], t.yaw, t.pitch, false, true);
                        }
                        self.flare(t.pos);
                    }
                    TK::ExplBig => {
                        let ring = self.rings.get((n % 2) as usize).cloned().unwrap_or_default();
                        for (dx, dy) in ring {
                            let x = t.pos[0] + dx as f32 * 180.0 * U;
                            let y = t.pos[1] + dy as f32 * 180.0 * U;
                            let f = self.floor(x, y, t.pos[2] + 1.0);
                            self.flare([x, y, f]);
                        }
                    }
                    _ => {
                        for r in [2 * n, 2 * n + 1] {
                            let ring = self.rings.get(r as usize).cloned().unwrap_or_default();
                            for (dx, dy) in ring {
                                let x = t.pos[0] + dx as f32 * 128.0 * U;
                                let y = t.pos[1] + dy as f32 * 128.0 * U;
                                let f = self.floor(x, y, t.pos[2] + 1.0);
                                self.particle([x, y, f], t.yaw, t.pitch, false, true);
                            }
                        }
                    }
                }
                t.life -= 1;
                if t.life <= 0 {
                    t.alive = false;
                }
            }
            TK::Particle { fast } => {
                if t.first {
                    // level-placed particle
                    t.first = false;
                    if self.lvl.collide(t.pos[0], t.pos[1], t.pos[2]) && self.lvl.solid_at(t.pos[0], t.pos[1], t.pos[2]) {
                        t.alive = false;
                        return;
                    }
                    self.sound(6, None, t.pos, 100);
                    self.area_damage(t.pos, 1.0, t.damage, None, false, true);
                    let r1 = t.seed.below(157) as f32;
                    let r2 = t.seed.below(100) as f32;
                    t.rise = (r2 + 100.0) * ((r1 / 79.0).floor() * 2.0 - 1.0) * U;
                }
                t.life -= 1;
                if fast {
                    t.life -= 1;
                }
                if t.life < 0 {
                    t.alive = false;
                    return;
                }
                if t.disp[0] != 0.0 || t.disp[1] != 0.0 {
                    let target = [t.pos[0] + t.disp[0], t.pos[1] + t.disp[1], t.pos[2] + t.disp[2]];
                    t.disp[0] -= t.disp[0] / 8.0;
                    t.disp[1] -= t.disp[1] / 8.0;
                    if self.lvl.solid_at(target[0], target[1], target[2]) {
                        t.disp[0] = 0.0;
                        t.disp[1] = 0.0;
                    } else {
                        t.pos = target;
                    }
                }
                t.sprite += if fast { 2 } else { 1 };
                t.sprite = t.sprite.min(151);
                t.pos[2] += t.rise;
                let f = self.floor(t.pos[0], t.pos[1], t.pos[2] + 0.1);
                t.pos[2] = t.pos[2].max(f);
                t.rise *= 11.0 / 16.0;
            }
            TK::Flare => {
                t.life -= 1;
                if t.life < 0 {
                    t.alive = false;
                    return;
                }
                let h = len2([t.disp[0], t.disp[1]]);
                if h > 0.0 {
                    let nh = (h - ((h * 256.0 + 15.0) / 16.0).floor() * U).max(0.0);
                    t.disp[0] *= nh / h;
                    t.disp[1] *= nh / h;
                }
                t.disp[2] = (t.disp[2] - 6.0 * U).max(-100.0 * U);
                // our disp[2] is height (+ up): along() gives dh = -sin(pitch) with pitch negative = up
                t.pos = [t.pos[0] + t.disp[0], t.pos[1] + t.disp[1], t.pos[2] + t.disp[2]];
                let mut p = Thing::new(TK::Particle { fast: true }, t.pos, 0);
                p.life = 12;
                p.sprite = 139;
                p.first = false;
                let f = self.floor(t.pos[0], t.pos[1], t.pos[2] + 0.1);
                p.pos[2] = p.pos[2].max(f);
                self.spawn(p);
            }
            TK::Fire => {
                let dz = t.seed.below(31) as f32 * U;
                self.smoke_fire(t.pos, dz);
                t.life -= 1;
                if t.life <= 0 {
                    // level emitters keep emitting (steam)
                    t.life = 15;
                }
            }
            TK::Smoke => {
                if t.first {
                    t.first = false;
                    let a = along(t.yaw, t.pitch, -t.speed);
                    t.disp = a;
                }
                t.pos = [t.pos[0] + t.disp[0], t.pos[1] + t.disp[1], t.pos[2] + t.disp[2]];
                t.sprite = (t.sprite + 1).min(69);
                if self.lvl.solid_at(t.pos[0], t.pos[1], t.pos[2]) {
                    t.alive = false;
                }
                t.life -= 1;
                if t.life <= 0 {
                    t.alive = false;
                }
            }
            TK::SmokeFire => {
                t.pos[2] += t.rise;
                if t.rise < 20.0 * U {
                    t.rise += (16 + t.seed.below(16)) as f32 * U / 16.0;
                }
                if t.age % 2 == 0 {
                    t.sprite = (t.sprite + 1).min(20);
                }
                if self.lvl.collide(t.pos[0], t.pos[1], t.pos[2]) {
                    t.alive = false;
                }
                t.life -= 1;
                if t.life <= 0 {
                    t.alive = false;
                }
            }
            TK::Sand | TK::Water => {
                let a = along(t.yaw, t.pitch, t.speed);
                let np = [t.pos[0] + a[0] + t.disp[0], t.pos[1] + a[1] + t.disp[1], t.pos[2] + a[2] + t.disp[2]];
                for k in 0..3 {
                    t.disp[k] -= t.disp[k] / 64.0;
                }
                if self.lvl.solid_at(np[0], np[1], np[2]) {
                    t.alive = false;
                    return;
                }
                t.pos = np;
                let f = self.floor(t.pos[0], t.pos[1], t.pos[2] + 0.2);
                if t.pos[2] < f {
                    t.pos[2] = f;
                    t.pitch = 0.0;
                } else {
                    t.pos[2] += 5.0 * U;
                }
                t.speed *= 12.0 / 16.0;
                t.sprite += 1;
                t.life -= 1;
                if t.life <= 0 {
                    t.alive = false;
                }
            }
            TK::Collect { member } => {
                if t.hidden > 0 {
                    t.hidden -= 1;
                    return;
                }
                if t.thrown {
                    t.life -= 1;
                    if t.life <= 0 {
                        t.alive = false;
                        return;
                    }
                }
                let base = collect_sprite(member);
                t.sprite = if t.thrown && t.life <= 3 { base + (4 - t.life) } else { base };
                let pos = [t.pos[0], t.pos[1], t.pos[2] + 0.5];
                for j in 0..self.crafts.len() {
                    let c = &self.crafts[j];
                    if !c.racing() || !overlap(pos, 1.0, c.pos, CRAFT_HS) {
                        continue;
                    }
                    let ok = match member {
                        2 | 3 => c.shield < 10000,
                        4 => c.shield < 20000,
                        5 | 6 => c.ammo < 10000,
                        7 => c.ammo < 20000,
                        8 | 9 => c.fuel < 10000,
                        10 => c.fuel < 20000,
                        11 => c.upgrades[0] < 3,
                        12 => c.upgrades[1] < 3,
                        13 => c.upgrades[2] < 3,
                        _ => true,
                    };
                    if ok {
                        let p = c.pos;
                        self.crafts[j].aff_collect.push(member);
                        self.sound(14, Some(j), p, 100);
                        if t.thrown {
                            t.alive = false;
                        } else {
                            t.hidden = 2000;
                        }
                        break;
                    }
                }
            }
            TK::Scenery { sub } => {
                if sub == 6 || sub == 7 {
                    // cones are knocked over by crafts
                    if t.pitch < 16384.0 * 0.95 {
                        if t.rise > 0.0 {
                            t.pitch = (t.pitch + 2000.0).min(16384.0);
                        } else {
                            for c in &self.crafts {
                                if c.active && overlap(t.pos, 0.25, c.pos, CRAFT_HS + 0.1) {
                                    t.rise = 1.0;
                                    t.yaw = heading(t.pos[0] - c.pos[0], t.pos[1] - c.pos[1]);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn drop_collectables(&mut self, c: &Craft) {
        let mut items = vec![2u8, 2, 2, 8, 8, 8, 5, 5, 5];
        for (k, &m) in [11u8, 12, 13].iter().enumerate() {
            if c.upgrades[k] > 0 {
                items.push(m);
            }
        }
        let n = items.len() as f32;
        for (k, m) in items.into_iter().enumerate() {
            let a = k as f32 / n * TURN;
            let o = along2(a, 1.0);
            let (x, y) = ((c.pos[0] + o[0]).clamp(0.5, 255.5), (c.pos[1] + o[1]).clamp(0.5, 159.5));
            let f = self.floor(x, y, c.pos[2] + 0.5);
            let mut t = Thing::new(TK::Collect { member: m }, [x, y, f], 0);
            t.thrown = true;
            t.life = 200;
            t.sprite = collect_sprite(m);
            self.spawn(t);
        }
    }

    /// Chargers: crafts inside a pad get the charger affect.
    pub fn chargers(&mut self) {
        for c in self.crafts.iter_mut().filter(|c| c.active) {
            for r in &self.track.chargers {
                if r.contains_cell(c.pos[0], c.pos[1]) {
                    let k = match r.kind {
                        1 => 0,
                        2 => 1,
                        _ => 2,
                    };
                    c.aff_charger[k] = true;
                }
            }
        }
    }

    /// Triggers (spec 01 §9.4 type 8).
    pub fn triggers(&mut self) {
        let mut fire = Vec::new();
        let positions: Vec<(i32, i32)> = self
            .crafts
            .iter()
            .filter(|c| c.active)
            .map(|c| (c.pos[0].floor() as i32, c.pos[1].floor() as i32))
            .chain(self.trucks.iter().map(|t| (t.pos[0].floor() as i32, t.pos[1].floor() as i32)))
            .collect();
        for (ti, tr) in self.triggers.iter_mut().enumerate() {
            if !tr.alive {
                continue;
            }
            let fired = match tr.sub {
                0 => {
                    if tr.inside.len() != positions.len() {
                        tr.inside = vec![false; positions.len()];
                    }
                    let mut f = false;
                    for (k, &(x, y)) in positions.iter().enumerate() {
                        let dx = (x - tr.x).abs();
                        let dy = (y - tr.y).abs();
                        let on_ring = dx <= tr.w + 1 && dy <= tr.h + 1 && (dx == tr.w + 1 || dy == tr.h + 1);
                        if on_ring && !tr.inside[k] {
                            f = true;
                        }
                        tr.inside[k] = on_ring;
                    }
                    f
                }
                1 => {
                    tr.timer -= 1;
                    tr.timer <= 0
                }
                3 => tr.absorb <= 0,
                _ => false,
            };
            if fired {
                tr.alive = false;
                fire.push((ti, tr.target_group));
            }
        }
        for (_, g) in fire {
            self.activate_group(g);
        }
    }
}
