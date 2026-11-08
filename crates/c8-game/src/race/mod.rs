//! The race (spec chapters 04-08).
pub mod ai;
pub mod camera;
pub mod flight;
pub mod hud;
pub mod math;
pub mod puzzle;
pub mod render;
pub mod sim;
pub mod sound;
pub mod terrain;
pub mod track;
pub mod types;
pub mod weapons;

use crate::ctx::Ctx;
use crate::data::images::{load_sky, load_terrain_textures, ColourTable};
use crate::data::level::Level;
use crate::frontend::{PauseMenu, PauseOut};
use crate::gfx::canvas::Canvas;
use crate::gfx::{Gfx, Scene};
use crate::input::KeyCode;
use camera::{RaceCamera, ViewData};
use flight::HumanIn;
use crate::frontend::keys::Action;
use crate::pad::Btn;
use hud::Hud;
use math::*;
use sim::*;
use std::rc::Rc;
use types::{PlayerResult, RaceResult, RaceSetup, POINTS};

/// Game speed in percent of the design rate of 20 ticks per second (default 90 %).
pub static SPEED_PCT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(90);

/// Length of one simulation tick in real seconds. The simulation always advances in whole
/// fixed ticks (independent of the frame rate); drawing interpolates between them.
pub fn tick_secs() -> f64 {
    0.05 * 100.0 / SPEED_PCT.load(std::sync::atomic::Ordering::Relaxed).clamp(25, 400) as f64
}
const COUNTDOWN: u32 = 60;

pub struct Race {
    pub setup: RaceSetup,
    pub w: World,
    cam: RaceCamera,
    hud: Hud,
    acc: f64,
    fe_acc: f64,
    countdown: u32,
    paused: Option<PauseMenu>,
    human: Option<usize>,
    end_requested: bool,
    end_now: bool,
    quit_game: bool,
    table: Rc<ColourTable>,
    shading: bool,
    mapping: bool,
    big_prev_finished: bool,
    lang: usize,
    stereo: bool,
    /// camera window action chosen by keys (port: camera keys work in every race)
    view_action: u8,
    lit: usize,
    frame_dt: f64,
    cheat: bool,
    puzzle_keys: Vec<KeyCode>,
    bindings: crate::frontend::Bindings,
}

impl Race {
    pub fn new(setup: RaceSetup, ctx: &mut Ctx, gfx: &mut Gfx) -> Result<Race, String> {
        let t = setup.track.clamp(1, 6);
        let lvl = Level::load(t)?;
        let table = Rc::new(ColourTable::load(t - 1)?);
        gfx.set_colour_table(&table.data);
        gfx.set_terrain_textures(&load_terrain_textures(t - 1)?);
        gfx.set_sky(&load_sky(t - 1)?);
        gfx.set_palette(&ctx.assets.palette.vga);
        let mut w = World::new(lvl, ctx.assets.rings.clone());
        w.demo = setup.demo;
        // crafts on the grid
        let cells = w.track.start_cells.clone();
        let heading = w.track.start_heading;
        let n_humans = setup.players.iter().filter(|p| p.human).count();
        let mut human = None;
        for (slot, p) in setup.players.iter().enumerate().take(8) {
            let mut c = Craft::default();
            c.active = true;
            c.slot = slot;
            c.human = p.human;
            if p.human && human.is_none() {
                human = Some(slot);
            }
            c.name = p.name.clone();
            c.team = p.team;
            c.kind = p.craft;
            c.difficulty = p.difficulty;
            if !p.human {
                let rank = slot.saturating_sub(n_humans).min(7);
                c.personality = PERSONALITY[rank];
            }
            let (cx, cy) = cells.get(slot).copied().unwrap_or((128 + slot as i32, 80));
            c.pos = [cx as f32 + 0.5, cy as f32 + 0.5, 0.0];
            c.yaw = heading;
            c.view = [heading, 0.0, 0.0];
            c.prev_view = c.view;
            c.race_laps = setup.laps + 1;
            c.place = slot as u8 + 1;
            c.target_cp = 0;
            w.init_craft(&mut c);
            c.inv = 100;
            if p.human && std::env::var_os("C8H18_AUTOPILOT").is_some() {
                c.autopilot_locked = true;
            }
            w.crafts.push(c);
        }
        w.viewed = human.unwrap_or(0);
        let shading = ctx.cfg.shading();
        let mapping = ctx.cfg.texture_mapping();
        let mesh = terrain::build_mesh(&w.lvl, &terrain::MeshOpts { shading, flat: if mapping { None } else { Some(&table) } });
        gfx.set_terrain_mesh(&mesh);
        w.mesh_dirty = false;
        let mut cam = RaceCamera::new();
        cam.follow = w.viewed;
        let view_action = 7 + ctx.cfg.view_slot() as u8;
        ctx.audio.stop_all();
        Ok(Race {
            setup,
            w,
            cam,
            hud: Hud::new(),
            acc: 0.0,
            fe_acc: 0.0,
            countdown: 0,
            paused: None,
            human,
            end_requested: false,
            end_now: false,
            quit_game: false,
            table,
            shading,
            mapping,
            big_prev_finished: false,
            lang: ctx.cfg.language(),
            stereo: ctx.cfg.stereo(),
            view_action: if (7..=10).contains(&view_action) { view_action } else { 7 },
            lit: 0,
            frame_dt: 0.0,
            cheat: ctx.cfg.cheat(),
            puzzle_keys: Vec::new(),
            bindings: crate::frontend::load_bindings(),
        })
    }

    fn human_input(&self, ctx: &Ctx) -> HumanIn {
        let b = &self.bindings;
        let d = |ks: &[KeyCode]| ks.iter().any(|&k| ctx.input.down(k));
        let p = ctx.pad;
        // gamepad (port addition): stick / D-pad drive like the arrow keys; with analog steering the
        // stick steers proportionally and only the D-pad steers fully
        let analog = crate::pad::analog_steering();
        let (pad_left, pad_right) = if analog { (p.dpad[2], p.dpad[3]) } else { (p.down(Btn::Left), p.down(Btn::Right)) };
        HumanIn {
            accel: d(&b.accelerate) || p.down(Btn::Up) || p.action_down(Action::Accelerate),
            brake: d(&b.brake) || p.down(Btn::Down) || p.action_down(Action::Brake),
            left: d(&b.left) || pad_left || p.action_down(Action::Left),
            right: d(&b.right) || pad_right || p.action_down(Action::Right),
            gun: d(&b.fire) || p.action_down(Action::Fire),
            missile: d(&b.missile) || p.action_down(Action::Missile),
            boost: d(&b.booster) || p.action_down(Action::Booster),
            steer: if analog { p.steer } else { 0.0 },
        }
    }

    fn tune(&self) -> usize {
        match self.setup.track {
            1 | 2 => 1,
            3 | 4 => 2,
            5 | 6 => 3,
            _ => 5,
        }
    }

    /// Advance by real time dt. Returns the result when the race is over.
    pub fn frame(&mut self, ctx: &mut Ctx, dt: f64) -> Option<RaceResult> {
        self.frame_dt = dt;
        if self.w.mesh_dirty {
            // the app re-uploads via take_mesh(); see draw()
        }
        // pause menu
        if let Some(pm) = &mut self.paused {
            ctx.keys.extend(ctx.pad.menu_keys(true));
            self.fe_acc += dt;
            while self.fe_acc >= 1.0 / 70.0 {
                self.fe_acc -= 1.0 / 70.0;
                match pm.frame(ctx) {
                    PauseOut::Stay => {}
                    PauseOut::Resume => {
                        self.paused = None;
                        self.acc = 0.0;
                        break;
                    }
                }
                ctx.keys.clear();
            }
            return None;
        }
        // keys
        let mut keys = std::mem::take(&mut ctx.keys);
        if !self.setup.demo {
            if let Some(i) = keys.iter().position(|k| k.code == KeyCode::Tab && k.ctrl) {
                keys.remove(i);
                puzzle::toggle(ctx.cfg);
            }
        }
        let puzzle_on = !self.setup.demo && puzzle::visible(ctx.cfg);
        if puzzle_on {
            for k in keys.drain(..) {
                if matches!(k.code, KeyCode::ArrowUp | KeyCode::ArrowDown | KeyCode::ArrowLeft | KeyCode::ArrowRight) {
                    self.puzzle_keys.push(k.code);
                }
            }
        }
        for k in &keys {
            if self.setup.demo {
                if !matches!(k.code, KeyCode::KeyR | KeyCode::KeyC) {
                    return Some(self.results());
                }
                continue;
            }
            match k.code {
                KeyCode::KeyQ if k.right_shift => {
                    self.quit_game = true;
                    let mut r = self.results();
                    r.quit_game = true;
                    return Some(r);
                }
                KeyCode::KeyE if k.right_shift => self.end_requested = true,
                KeyCode::Escape => {
                    self.end_now = true;
                }
                c if self.bindings.pause.contains(&c) => {
                    ctx.audio.stop_all();
                    self.paused = Some(PauseMenu::new());
                    self.fe_acc = 0.0;
                    return None;
                }
                KeyCode::F1 | KeyCode::F2 | KeyCode::F3 | KeyCode::F4 | KeyCode::F5 | KeyCode::F6 | KeyCode::F7 | KeyCode::F8 | KeyCode::F9 if k.alt => {
                    if ctx.cfg.cheat() {
                        self.cheat = true;
                        self.cheat(k.code);
                    }
                }
                KeyCode::F1 => self.view_action = 1,
                KeyCode::F2 => self.view_action = 2,
                KeyCode::F3 => self.view_action = 3,
                KeyCode::F4 => self.view_action = 4,
                KeyCode::F5 => self.view_action = 5,
                KeyCode::F6 => self.view_action = 6,
                KeyCode::F7 => self.view_action = 7,
                KeyCode::F8 => self.view_action = 8,
                KeyCode::F9 => self.view_action = 9,
                KeyCode::F10 => self.view_action = 11,
                c if self.bindings.zoom_out.contains(&c) => self.cam.zoom = (self.cam.zoom + 20.0).min(1000.0),
                c if self.bindings.zoom_in.contains(&c) => self.cam.zoom = (self.cam.zoom - 20.0).max(0.0),
                KeyCode::Digit1 | KeyCode::Digit2 | KeyCode::Digit3 | KeyCode::Digit4 | KeyCode::Digit5 | KeyCode::Digit6 | KeyCode::Digit7 | KeyCode::Digit8 => {
                    let n = match k.code {
                        KeyCode::Digit1 => 0,
                        KeyCode::Digit2 => 1,
                        KeyCode::Digit3 => 2,
                        KeyCode::Digit4 => 3,
                        KeyCode::Digit5 => 4,
                        KeyCode::Digit6 => 5,
                        KeyCode::Digit7 => 6,
                        _ => 7,
                    };
                    if n < self.w.crafts.len() {
                        self.cam.follow = n;
                    }
                }
                _ => {}
            }
        }
        // gamepad (port addition): the bound buttons pause and zoom (defaults Start, RB / LB), Y cycles
        // the views unless bound to an action, Back held 1 s ends the race; in a demo race any button
        // ends it
        let pad = ctx.pad;
        if self.setup.demo {
            if pad.any_pressed() {
                return Some(self.results());
            }
        } else {
            if pad.action_pressed(Action::Pause) {
                ctx.audio.stop_all();
                self.paused = Some(PauseMenu::new());
                self.fe_acc = 0.0;
                return None;
            }
            if pad.pressed(Btn::Y) && !crate::pad::is_bound(Btn::Y) {
                self.view_action = match self.view_action {
                    7 => 8,
                    8 => 9,
                    9 => 10,
                    _ => 7,
                };
            }
            if pad.action_pressed(Action::ZoomOut) {
                self.cam.zoom = (self.cam.zoom + 20.0).min(1000.0);
            }
            if pad.action_pressed(Action::ZoomIn) {
                self.cam.zoom = (self.cam.zoom - 20.0).max(0.0);
            }
            if pad.back_held >= 1.0 {
                self.end_now = true;
            }
        }
        if self.end_now {
            return Some(self.results());
        }
        let input = if puzzle_on { HumanIn::default() } else { self.human_input(ctx) };
        self.acc += dt;
        let mut ticks = 0;
        let tick = tick_secs();
        while self.acc >= tick && ticks < 5 {
            self.acc -= tick;
            ticks += 1;
            if self.countdown < COUNTDOWN {
                self.countdown_tick(ctx);
            } else {
                self.tick(input);
                if puzzle_on {
                    let pk = std::mem::take(&mut self.puzzle_keys);
                    puzzle::step(ctx.cfg, &pk);
                    if ctx.cfg.cheat() {
                        self.cheat = true;
                    }
                }
                self.after_tick(ctx);
                if self.setup.demo && self.w.frame >= 3000 {
                    return Some(self.results());
                }
                // race end (spec 04 §5.2)
                let over = self.w.crafts.iter().filter(|c| c.active).all(|c| c.finish_place > 0 && c.hold == 0);
                if over || self.end_requested {
                    if self.w.frame & 32 == 0 {
                        return Some(self.results());
                    }
                }
            }
        }
        if ticks == 5 {
            self.acc = 0.0;
        }
        None
    }

    fn cheat(&mut self, k: KeyCode) {
        let Some(h) = self.human else { return };
        match k {
            KeyCode::F1 => self.w.crafts[h].shield = -1,
            KeyCode::F2 => {
                for (i, c) in self.w.crafts.iter_mut().enumerate() {
                    if i != h && c.racing() {
                        c.shield = -1;
                    }
                }
            }
            KeyCode::F3 => {
                let c = &mut self.w.crafts[h];
                c.fuel = 20000;
                c.ammo = 20000;
                c.shield = 20000;
            }
            KeyCode::F4 => self.w.crafts[h].lap += 1,
            KeyCode::F5 => self.w.crafts[h].upgrades = [3; 3],
            KeyCode::F6 => {
                let c = &mut self.w.crafts[h];
                c.fuel = 10000;
                c.ammo = 10000;
                c.shield = 10000;
                c.upgrades = [3; 3];
            }
            _ => {}
        }
    }

    fn countdown_tick(&mut self, ctx: &mut Ctx) {
        self.countdown += 1;
        for c in &mut self.w.crafts {
            c.prev_pos = c.pos;
            c.prev_view = c.view;
        }
        let snd = |ctx: &mut Ctx, s: u16| ctx.audio.play(0, s, 0x7fff, 0x8000, 100, false, crate::audio::MODE_ONCE);
        match self.countdown {
            20 => {
                self.lit = 1;
                snd(ctx, 25);
            }
            40 => {
                self.lit = 2;
                snd(ctx, 25);
            }
            COUNTDOWN => {
                self.lit = 3;
                snd(ctx, 26);
                ctx.audio.play_tune(self.tune(), false);
                for c in &mut self.w.crafts {
                    c.started = true;
                }
            }
            _ => {}
        }
    }

    /// One simulation tick in the order of spec 05 §3 / port notes.
    fn tick(&mut self, input: HumanIn) {
        // Cockpit shake decays (count × 5 >> 3) while the cockpit view runs; done per tick so the
        // decay rate does not depend on the display frame rate.
        if self.view_action == 7 {
            let f = self.cam.follow.min(self.w.crafts.len() - 1);
            let c = &mut self.w.crafts[f];
            c.shake = c.shake * 5 >> 3;
        }
        let w = &mut self.w;
        w.frame += 1;
        w.sounds.clear();
        w.banner_reqs.clear();
        Self::placings(w);
        let n = w.crafts.len();
        for i in 0..n {
            w.booster_weapon(i);
        }
        for i in 0..n {
            let inp = if Some(i) == self.human && !self.setup.demo { input } else { HumanIn::default() };
            w.update_craft(i, inp);
        }
        w.update_trucks();
        for i in 0..n {
            w.guns(i);
        }
        w.update_things();
        w.chargers();
        w.triggers();
        w.step_morphs();
        if w.lap_timer {
            for c in w.crafts.iter_mut().filter(|c| c.active) {
                c.lap_time = (c.lap_time + 1).min(9999);
                c.race_time += 1;
            }
        }
    }

    fn placings(w: &mut World) {
        let ncp = w.track.checkpoints.len().max(1) as i64;
        let mut order: Vec<usize> = (0..w.crafts.len()).filter(|&i| w.crafts[i].active).collect();
        let prog = |c: &Craft| -> i64 {
            let tv = w.track.checkpoints.get(c.target_cp).map(|cp| cp.value as i64).unwrap_or(0);
            let tv = if tv == 0 && c.lap > 0 { ncp } else { tv };
            c.lap as i64 * ncp + tv
        };
        order.sort_by(|&a, &b| {
            let (ca, cb) = (&w.crafts[a], &w.crafts[b]);
            let fa = if ca.finish_place > 0 { ca.finish_place as i32 } else { 99 };
            let fb = if cb.finish_place > 0 { cb.finish_place as i32 } else { 99 };
            fa.cmp(&fb)
                .then(prog(cb).cmp(&prog(ca)))
                .then(ca.cp_dist.partial_cmp(&cb.cp_dist).unwrap_or(std::cmp::Ordering::Equal))
                .then(a.cmp(&b))
        });
        for (k, &i) in order.iter().enumerate() {
            w.crafts[i].place = k as u8 + 1;
        }
    }

    /// Sounds, banners and big messages after a tick.
    fn after_tick(&mut self, ctx: &mut Ctx) {
        let lc = self.cam.follow.min(self.w.crafts.len().saturating_sub(1));
        let c = &self.w.crafts[lc];
        let l = sound::Listener { craft: lc, pos: c.pos, yaw: c.yaw };
        let evs = std::mem::take(&mut self.w.sounds);
        for ev in &evs {
            sound::dispatch(ctx.audio, &l, ev);
        }
        sound::engines(ctx.audio, &l, &self.w, self.stereo);
        // lock-on warning
        let viewed_owner = lc as u32 + 1;
        if c.incoming_missile.is_some() {
            ctx.audio.update(viewed_owner, 15, 0x7fff, 0x8000, 100, true, crate::audio::MODE_ONCE);
        } else {
            ctx.audio.stop(viewed_owner, 15);
        }
        // messages for the viewed human
        let Some(h) = self.human else { return };
        if self.setup.demo {
            return;
        }
        let reqs = std::mem::take(&mut self.w.banner_reqs);
        let c = &self.w.crafts[h];
        let quiet = c.finish_place != 0 || c.killer.is_some();
        if !quiet {
            for (ci, id) in reqs {
                if ci == h && self.hud.banner_request(id) {
                    ctx.audio.stop(h as u32 + 1, 17);
                }
            }
        }
        // big messages
        let c = &mut self.w.crafts[h];
        let mut big = None;
        if c.finished_event {
            c.finished_event = false;
            big = Some((1u8, match c.finish_place {
                1 => Some(29u16),
                2 => Some(28),
                3 => Some(27),
                _ => None,
            }));
        }
        if c.sc_awarded {
            big = Some((2, Some(29)));
        }
        match c.spin_msg {
            3 => big = Some((3, Some(29))),
            4 => big = Some((4, Some(28))),
            5 => big = Some((5, Some(27))),
            _ => {}
        }
        if let Some((id, s)) = big {
            self.hud.big_request(id);
            if let Some(s) = s {
                ctx.audio.play(0, s, 0x7fff, 0x8000, 100, false, crate::audio::MODE_ONCE);
            }
        }
        let _ = self.big_prev_finished;
        let c = &self.w.crafts[h];
        if c.finish_place == 0 && c.killer.is_none() {
            let (play, stop) = self.hud.step(false);
            if let Some(s) = play {
                ctx.audio.update(h as u32 + 1, s, 0x7fff, 0x8000, 100, true, crate::audio::MODE_ONCE);
            }
            if stop {
                ctx.audio.stop(h as u32 + 1, 17);
            }
        } else {
            ctx.audio.stop(h as u32 + 1, 17);
            self.hud.step(false);
        }
    }

    /// Classification and results (spec 04 §5.3, §6).
    fn results(&mut self) -> RaceResult {
        let cheat = self.cheat;
        let w = &mut self.w;
        let human_finished = self.human.map(|h| w.crafts[h].finish_place > 0).unwrap_or(false);
        if human_finished {
            let base = w
                .crafts
                .iter()
                .filter(|c| c.active && c.finish_place > 0)
                .max_by_key(|c| c.finish_place)
                .map(|c| c.race_time.max(c.finish_time))
                .unwrap_or(0);
            let base = w.crafts.iter().filter(|c| c.active && c.finish_place > 0).max_by_key(|c| c.finish_place).map(|c| c.finish_time).unwrap_or(base);
            let mut seed = Lcg(base as u32);
            let mut t = base;
            for place in 1..=8u8 {
                for c in w.crafts.iter_mut() {
                    if c.active && c.finish_place == 0 && c.place == place {
                        c.finish_place = place;
                        let s = seed.next();
                        c.finish_time = t;
                        t += (s % 250) as i32;
                    }
                }
            }
        }
        let mut players = Vec::new();
        for c in &w.crafts {
            let acc = if c.rockets_launched != 0 && c.bullets_fired != 0 {
                ((c.bullets_hit + c.rockets_hit * 10) * 100) / (c.rockets_launched * 10 + c.bullets_fired)
            } else {
                0
            };
            let place = if c.finish_place > 0 { c.finish_place as i32 } else { 8 };
            let mut gf = c.bonus_score.clamp(0, 3000);
            gf += acc * 20;
            gf += ((c.kills - c.deaths) * 1000).clamp(-10000, 10000);
            gf += (8 - place) * 700;
            gf = gf.clamp(0, 19999);
            let mut assess = ((19999 - gf) / 1000 - (3 - c.difficulty as i32)).clamp(0, 19) as u8;
            if c.human && cheat {
                assess = 20;
            }
            players.push(PlayerResult {
                name: c.name.clone(),
                team: c.team,
                craft: c.kind,
                human: c.human,
                finish_place: if human_finished { c.finish_place } else { 0 },
                finish_time: c.finish_time,
                best_lap: c.best_lap.unwrap_or(0),
                laps: self.setup.laps,
                kills: c.kills,
                deaths: c.deaths,
                hit_accuracy: acc,
                assessment: assess,
                points: if human_finished && c.finish_place > 0 { POINTS[(c.finish_place as usize - 1).min(7)] } else { 0 },
            });
        }
        RaceResult {
            demo: self.setup.demo,
            championship: self.setup.championship,
            track: self.setup.track,
            quit_game: self.quit_game,
            classified: human_finished && !self.setup.demo,
            players,
        }
    }


    /// Rebuild the terrain mesh if the level changed (morphs).
    fn refresh_mesh(&mut self, gfx: &mut Gfx) {
        if self.w.mesh_dirty {
            let flat = if self.mapping { None } else { Some(&*self.table) };
            let mesh = terrain::build_mesh(&self.w.lvl, &terrain::MeshOpts { shading: self.shading, flat });
            gfx.set_terrain_mesh(&mesh);
            self.w.mesh_dirty = false;
        }
    }

    pub fn draw_gfx(&mut self, ctx: &Ctx, gfx: &mut Gfx, fg: &mut Canvas) -> Option<Scene> {
        self.refresh_mesh(gfx);
        fg.table = self.table.clone();
        fg.clear();
        let alpha = if self.countdown < COUNTDOWN || self.paused.is_some() { 1.0 } else { (self.acc / tick_secs()).clamp(0.0, 1.0) as f32 };
        let ip = render::Interp { alpha };
        let w = &self.w;
        // which camera
        let demo_or_paused = self.setup.demo || self.paused.is_some();
        let mut action = self.view_action;
        let mut follow = self.cam.follow;
        let mut bare = false;
        if demo_or_paused {
            action = 6;
            follow = ((w.frame / 200) % 7 + 1) as usize;
            if follow >= w.crafts.len() {
                follow = 0;
            }
        } else if let Some(h) = self.human {
            let c = &w.crafts[h];
            if !c.racing() || c.autopilot_locked {
                action = 6;
                follow = h;
                bare = true;
            }
        }
        let c = &w.crafts[follow.min(w.crafts.len() - 1)];
        let pos = ip.p(c.prev_pos, c.pos);
        let v = ViewData {
            pos,
            yaw: ip.a(c.prev_view[0], c.view[0]),
            pitch: c.prev_view[1] + (c.view[1] - c.prev_view[1]) * alpha,
            roll: c.prev_view[2] + (c.view[2] - c.prev_view[2]) * alpha,
        };
        let k = 1.0 - (7.0f32 / 8.0).powf((self.frame_dt as f32 / tick_secs() as f32).max(0.0));
        let shake = if action == 7 { Some((c.shake, w.frame & 1 == 1)) } else { None };
        let orbit = [ctx.input.down(KeyCode::ArrowLeft), ctx.input.down(KeyCode::ArrowRight), ctx.input.down(KeyCode::ArrowUp), ctx.input.down(KeyCode::ArrowDown), ctx.input.down(KeyCode::KeyZ), ctx.input.down(KeyCode::KeyX)];
        self.cam.run(w, &v, action, k, shake, orbit);
        let camera = self.cam.eye(w);
        let (sw, shh) = gfx.size();
        let focal = gfx.focal_px(94.0);
        let hidden = if self.cam.hide_followed { Some(follow) } else { None };
        let mut sb = render::SceneBuilder::new(ctx.assets, camera, &gfx.tmap_pos, shh as f32, focal);
        render::build_world(&mut sb, w, &ip, hidden);
        let scene = sb.finish(ctx.cfg.sky());
        // overlays
        let a = ctx.assets;
        if self.countdown < COUNTDOWN {
            Hud::start_lights(fg, a, self.lit);
        } else if !demo_or_paused {
            if let Some(h) = self.human {
                let c = &w.crafts[h];
                if bare {
                    if let Some(kl) = c.killer {
                        let s = format!("{} {}", crate::data::strings::get(self.lang, 174), w.crafts[kl].name);
                        let f = &a.fonts[4];
                        fg.text_centred(f, &crate::data::strings::to_cp437(&s), 240 - f.space_h() as i32 / 2, -6, 0, None);
                    }
                    if c.finish_place == 0 && c.killer.is_none() {
                        self.hud.draw_messages(fg, a, self.lang);
                    }
                } else {
                    if !w.lap_timer {
                        Hud::start_lights(fg, a, 3);
                    } else if c.finish_place == 0 {
                        Hud::minimap(fg, a, w, self.setup.track, h, w.frame);
                        // target marker screen position
                        let ts = c.target.and_then(|t| {
                            let tc = &w.crafts[t];
                            let p = ip.p(tc.prev_pos, tc.pos);
                            let vv = camera.to_view([p[0], p[2], p[1]]);
                            if vv[2] < 200.0 / 2048.0 || vv[2] > 16.0 {
                                return None;
                            }
                            let sx = sw as f32 / 2.0 + vv[0] * focal / vv[2];
                            let sy = shh as f32 / 2.0 - vv[1] * focal / vv[2];
                            if sx < 0.0 || sy < 0.0 || sx >= sw as f32 || sy >= shh as f32 {
                                return None;
                            }
                            Some((sx * fg.w as f32 / sw as f32 - fg.x0 as f32, sy * 480.0 / shh as f32))
                        });
                        self.hud.draw(fg, a, w, h, w.frame, ts, sw as f32, shh as f32);
                    }
                    if c.finish_place == 0 && c.killer.is_none() {
                        self.hud.draw_messages(fg, a, self.lang);
                    }
                }
                Hud::finishers(fg, a, w);
            }
        }
        if !self.setup.demo && puzzle::visible(ctx.cfg) {
            puzzle::draw(ctx.cfg, a, fg);
        }
        if let Some(pm) = &mut self.paused {
            pm.draw(ctx, fg);
        }
        Some(scene)
    }

}

/// Debug: run a race without graphics and print the crafts' progress.
pub fn headless(track: usize, ticks: u32, rings: Vec<Vec<(i32, i32)>>, demo: bool) {
    let lvl = Level::load(track).expect("level");
    let mut w = World::new(lvl, rings);
    w.demo = demo;
    println!("waypoints {} walls {} checkpoints {} start cells {:?} heading {}", w.track.wps.len() - 1, w.track.walls.len(), w.track.checkpoints.len(), &w.track.start_cells[..w.track.start_cells.len().min(8)], w.track.start_heading);
    println!("chargers {:?}", w.track.chargers.iter().map(|r| (r.kind, r.min, r.max)).collect::<Vec<_>>());
    let cells = w.track.start_cells.clone();
    for slot in 0..8 {
        let mut c = Craft::default();
        c.active = true;
        c.slot = slot;
        c.human = false;
        c.name = types::AI_NAMES[slot].into();
        c.difficulty = std::env::var("C8H18_DIFF").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
        c.personality = PERSONALITY[slot];
        let (cx, cy) = cells.get(slot).copied().unwrap_or((128, 80));
        c.pos = [cx as f32 + 0.5, cy as f32 + 0.5, 0.0];
        c.yaw = w.track.start_heading;
        c.race_laps = 4;
        w.init_craft(&mut c);
        c.started = true;
        w.crafts.push(c);
    }
    for t in 0..ticks {
        w.frame += 1;
        Race::placings(&mut w);
        for i in 0..8 {
            w.booster_weapon(i);
        }
        for i in 0..8 {
            w.update_craft(i, HumanIn::default());
        }
        w.update_trucks();
        for i in 0..8 {
            w.guns(i);
        }
        w.update_things();
        w.chargers();
        w.triggers();
        w.step_morphs();
        if w.lap_timer {
            for c in w.crafts.iter_mut() {
                c.lap_time = (c.lap_time + 1).min(9999);
                c.race_time += 1;
            }
        }
        if t % 100 == 0 {
            println!("--- tick {t} things {}", w.things.iter().filter(|t| t.alive).count());
            for c in &w.crafts {
                println!(
                    "{:8} pos ({:6.2},{:6.2},{:5.2}) yaw {:5.0} act {:2} wp {:3} lap {} cp {} place {} fin {} thr {:3.0} v {:.3} sh {} fu {} am {} kills {} shots {}/{}",
                    c.name, c.pos[0], c.pos[1], c.pos[2], c.yaw, c.action, c.waypoint, c.lap, c.target_cp, c.place, c.finish_place, c.throttle, c.measured_vel, c.shield, c.fuel, c.ammo, c.kills, c.bullets_hit, c.bullets_fired
                );
            }
        }
    }
}
