//! Simulation state: crafts, trucks, things, triggers; level activation.
use super::math::*;
use super::track::Track;
use crate::data::level::Level;

pub const P_SPARE: u8 = 1;
pub const P_FAST: u8 = 2;
pub const P_STEADY: u8 = 4;
pub const P_NOCATCH: u8 = 8;
pub const PERSONALITY: [u8; 8] = [
    P_SPARE | P_FAST,
    P_STEADY | P_NOCATCH,
    P_SPARE | P_FAST,
    P_SPARE | P_STEADY,
    P_SPARE,
    P_FAST | P_STEADY,
    P_SPARE | P_NOCATCH,
    0,
];

// craft actions (spec 05 §4)
pub const A_INIT: u8 = 0;
pub const A_RACING: u8 = 1;
pub const A_TUMBLE: u8 = 9;
pub const A_DROP: u8 = 17;
pub const A_EXPLODING: u8 = 19;
pub const A_CALL: u8 = 20;
pub const A_TRUCK_COMING: u8 = 22;
pub const A_REPAIR: u8 = 23;
pub const A_REPAIRED: u8 = 24;
pub const A_RESET: u8 = 25;

#[derive(Clone, Default, Debug)]
pub struct Craft {
    pub active: bool,
    pub slot: usize,
    pub human: bool,
    pub name: String,
    pub team: u8,
    pub kind: u8,
    pub difficulty: u8,
    pub personality: u8,
    // motion
    pub pos: [f32; 3],
    pub prev_pos: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub turn_rate: f32,
    pub view: [f32; 3],
    pub prev_view: [f32; 3],
    pub throttle: f32,
    pub brake_flag: bool,
    pub momentum: [f32; 2],
    pub friction: f32,
    pub eff: i32,
    pub airborne: bool,
    pub sensors: [(f32, f32); 4],
    pub sensor_floor: [f32; 4],
    pub push: [f32; 2],
    pub vert_disp: f32,
    pub measured_vel: f32,
    pub behind: i32,
    pub last_move: [f32; 3],
    // resources
    pub fuel: i32,
    pub shield: i32,
    pub ammo: i32,
    /// minigun, missile, booster
    pub upgrades: [i32; 3],
    pub action: u8,
    pub action_count: i32,
    pub time_slice: u8,
    pub reposition: bool,
    // inputs
    pub speed_in: f32,
    pub steer_in: f32,
    pub prev_steer: f32,
    pub trig_gun: bool,
    pub trig_missile: bool,
    pub trig_boost: bool,
    // weapons
    pub heat: i32,
    pub gun_restriction: i32,
    pub barrel: bool,
    pub reload: i32,
    pub missile_side: bool,
    pub boost_charge: i32,
    pub boost_restriction: i32,
    pub booster_active: bool,
    pub booster_burn: i32,
    pub booster_strength: i32,
    pub target: Option<usize>,
    pub lock: i32,
    pub incoming_missile: Option<f32>,
    // affects for the next post-process
    pub aff_damage: i32,
    pub aff_explosion: bool,
    pub aff_bullet: bool,
    pub aff_missile: bool,
    pub aff_bump: bool,
    pub aff_contact: bool,
    pub aff_booster: bool,
    pub aff_invincible: bool,
    /// shield, fuel, ammo chargers
    pub aff_charger: [bool; 3],
    pub aff_collect: Vec<u8>,
    pub hitter: Option<usize>,
    pub contact_phase: u8,
    pub charging: [bool; 3],
    pub inv: i32,
    pub invis: i32,
    pub killer: Option<usize>,
    pub kills: i32,
    pub deaths: i32,
    pub bullet_counter: i32,
    pub missile_counter: i32,
    pub hits_tick: [i32; 8],
    pub hits_run: [i32; 8],
    pub hits_max: [i32; 8],
    pub shake: i32,
    pub warn: [i32; 3],
    pub fuel_empty: i32,
    // race
    pub lap: u32,
    pub race_laps: u32,
    pub target_cp: usize,
    pub last_cp: usize,
    pub prev_cp: usize,
    pub cp_dist: f32,
    pub lap_time: i32,
    pub race_time: i32,
    pub finish_time: i32,
    pub best_lap: Option<i32>,
    pub laps_list: Vec<i32>,
    pub finish_place: u8,
    pub hold: i32,
    pub place: u8,
    pub started: bool,
    pub autopilot_locked: bool,
    pub lap_event: bool,
    pub kill_event: bool,
    pub finished_event: bool,
    // stats
    pub bullets_fired: i32,
    pub bullets_hit: i32,
    pub rockets_launched: i32,
    pub rockets_hit: i32,
    pub bonus_score: i32,
    // supercar
    pub sc_armed: bool,
    pub sc_next: bool,
    pub sc_kills: i32,
    pub sc_level: i32,
    pub supercar: i32,
    pub sc_saved: [i32; 3],
    pub sc_awarded: bool,
    // spin bonus
    pub spin_phase: u8,
    pub spin_sum: f32,
    pub spin_last_yaw: f32,
    pub spin_bits: u8,
    /// 0 none, 3 = 720!, 4 = 360!, 5 = 180! (big message ids) for this tick
    pub spin_msg: u8,
    // destruction
    pub tumble_rate: [f32; 3],
    pub tumble: i32,
    pub destroyed: bool,
    pub truck: Option<usize>,
    pub recovery_wait: i32,
    // autopilot
    pub ap_first: bool,
    pub waypoint: usize,
    pub last_wp: usize,
    pub stuck_count: i32,
    pub stuck_pos: [f32; 2],
    pub evade1: i32,
    pub evade2: i32,
    pub brake_count: i32,
    pub hold_fire: bool,
    pub fire_at_will: bool,
    /// fuel, repair, ammo
    pub wants: [bool; 3],
    pub targeted_close: bool,
    pub close_to_target: bool,
}

impl Craft {
    pub fn solid(&self) -> bool {
        self.active && matches!(self.action, A_INIT | A_RACING | A_CALL | A_TRUCK_COMING)
    }
    pub fn racing(&self) -> bool {
        self.active && self.action == A_RACING
    }
    pub fn autopilot(&self) -> bool {
        !self.human || self.autopilot_locked
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TK {
    Bullet,
    Spark,
    Missile,
    /// explosion particle (member 0); fast = flare trail particle
    Particle { fast: bool },
    ExplMedium,
    ExplBig,
    ExplMassive,
    Flare,
    Fire,
    Smoke,
    SmokeFire,
    Sand,
    Water,
    /// collectable member; respawn timer when hidden; thrown life (None = level)
    Collect { member: u8 },
    /// scenery sub-type (3 camera, 6/7 cone, 8..10 barrel, 0 sign, 1 tree, 2 lamp)
    Scenery { sub: u8 },
}

#[derive(Clone, Debug)]
pub struct Thing {
    pub kind: TK,
    pub alive: bool,
    pub pos: [f32; 3],
    pub prev_pos: [f32; 3],
    pub disp: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub speed: f32,
    pub rise: f32,
    pub life: i32,
    pub sprite: i32,
    pub owner: Option<usize>,
    pub damage: i32,
    pub first: bool,
    pub homing: bool,
    pub target: Option<usize>,
    pub upgrade: i32,
    pub hidden: i32,
    pub thrown: bool,
    pub seed: Lcg,
    pub age: i32,
}

impl Thing {
    pub fn new(kind: TK, pos: [f32; 3], seed: u32) -> Thing {
        Thing {
            kind,
            alive: true,
            pos,
            prev_pos: pos,
            disp: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            speed: 0.0,
            rise: 0.0,
            life: 0,
            sprite: -1,
            owner: None,
            damage: 0,
            first: true,
            homing: false,
            target: None,
            upgrade: 0,
            hidden: 0,
            thrown: false,
            seed: Lcg(seed),
            age: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TruckState {
    Idle,
    Approach,
    Descend,
    Carry,
    Lower,
    Release,
}

#[derive(Clone, Debug)]
pub struct Truck {
    pub pos: [f32; 3],
    pub prev_pos: [f32; 3],
    pub yaw: f32,
    pub prev_yaw: f32,
    pub state: TruckState,
    pub count: i32,
    pub craft: Option<usize>,
    pub drop_wp: usize,
}

#[derive(Clone, Debug)]
pub struct Trigger {
    pub sub: u8,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub target_group: u16,
    pub timer: i32,
    pub absorb: i32,
    pub alive: bool,
    /// per craft/truck: was inside the ring last tick
    pub inside: Vec<bool>,
}

#[derive(Clone, Debug)]
pub struct Morph {
    pub cells: Vec<((i32, i32), f32, f32)>,
    pub t: i32,
    pub n: i32,
}

pub struct World {
    pub lvl: Level,
    pub track: Track,
    pub crafts: Vec<Craft>,
    pub trucks: Vec<Truck>,
    pub things: Vec<Thing>,
    pub triggers: Vec<Trigger>,
    pub morphs: Vec<Morph>,
    pub mesh_dirty: bool,
    pub rings: Vec<Vec<(i32, i32)>>,
    pub frame: u32,
    pub lap_timer: bool,
    pub seed: Lcg,
    pub thing_seed: u32,
    pub demo: bool,
    pub sounds: Vec<SoundEv>,
    pub viewed: usize,
    /// banner requests (craft, message id)
    pub banner_reqs: Vec<(usize, u16)>,
}

/// Sound event raised by the simulation (played by the race's sound dispatcher).
#[derive(Clone, Copy, Debug)]
pub struct SoundEv {
    pub sample: u16,
    /// source craft slot (owner), or None for things
    pub craft: Option<usize>,
    pub pos: [f32; 3],
    pub rate: i32,
    pub stop: bool,
    pub owner: u32,
}

impl World {
    pub fn new(lvl: Level, rings: Vec<Vec<(i32, i32)>>) -> World {
        let track = Track::build(&lvl);
        let mut w = World {
            lvl,
            track,
            crafts: Vec::new(),
            trucks: Vec::new(),
            things: Vec::new(),
            triggers: Vec::new(),
            morphs: Vec::new(),
            mesh_dirty: true,
            rings,
            frame: 0,
            lap_timer: false,
            seed: Lcg(12345),
            thing_seed: 1,
            demo: false,
            sounds: Vec::new(),
            viewed: 0,
            banner_reqs: Vec::new(),
        };
        w.activate_group(1);
        w
    }

    pub fn spawn(&mut self, mut t: Thing) -> usize {
        self.thing_seed = self.thing_seed.wrapping_add(1);
        t.seed = Lcg(self.thing_seed.wrapping_mul(2654435761));
        if let Some(i) = self.things.iter().position(|x| !x.alive) {
            self.things[i] = t;
            i
        } else {
            self.things.push(t);
            self.things.len() - 1
        }
    }

    pub fn banner_req(&mut self, craft: usize, id: u16) {
        self.banner_reqs.push((craft, id));
    }

    pub fn sound(&mut self, sample: u16, craft: Option<usize>, pos: [f32; 3], rate: i32) {
        let owner = craft.map(|c| c as u32 + 1).unwrap_or(100);
        self.sounds.push(SoundEv { sample, craft, pos, rate, stop: false, owner });
    }
    pub fn stop_sound(&mut self, sample: u16, craft: Option<usize>) {
        let owner = craft.map(|c| c as u32 + 1).unwrap_or(100);
        self.sounds.push(SoundEv { sample, craft, pos: [0.0; 3], rate: 100, stop: true, owner });
    }

    /// Floor (terrain or block top) under a point.
    pub fn floor(&self, x: f32, y: f32, z: f32) -> f32 {
        self.lvl.floor_below(x, y, z)
    }

    /// Create all entities of group g (spec 01 §9.4).
    pub fn activate_group(&mut self, g: u16) {
        let ents: Vec<_> = self.lvl.entities.iter().filter(|e| e.group == g).copied().collect();
        for e in ents {
            let (cx, cy) = (e.x as f32 + 0.5, e.y as f32 + 0.5);
            let ground = self.lvl.floor_below(cx, cy, 255.0);
            let pos = [cx, cy, ground];
            match e.kind {
                2 => {
                    let kind = match e.sub {
                        0 => Some(TK::Particle { fast: false }),
                        1 => Some(TK::ExplMedium),
                        2 => Some(TK::ExplBig),
                        3 => Some(TK::ExplMassive),
                        5 => Some(TK::Fire),
                        7 => Some(TK::Smoke),
                        8 => Some(TK::SmokeFire),
                        _ => None,
                    };
                    if let Some(k) = kind {
                        let mut t = Thing::new(k, pos, 0);
                        match k {
                            TK::Particle { .. } => {
                                t.life = 12;
                                t.sprite = 139;
                                t.damage = 200;
                            }
                            TK::ExplMedium => {
                                t.life = 3;
                                t.damage = 1500;
                            }
                            TK::ExplBig => {
                                t.life = 6;
                                t.damage = 10000;
                            }
                            TK::ExplMassive => {
                                t.life = 3;
                                t.damage = 10000;
                            }
                            TK::Fire => {
                                t.life = 15;
                                t.sprite = 11;
                            }
                            TK::SmokeFire => {
                                t.life = 22;
                                t.sprite = 5;
                            }
                            _ => t.life = 6,
                        }
                        self.spawn(t);
                    }
                    if e.sub == 16 {
                        self.swap_area(&e);
                    }
                }
                3 => {
                    let mut t = Thing::new(TK::Scenery { sub: e.sub }, pos, 0);
                    t.life = i32::MAX;
                    self.spawn(t);
                }
                5 => {
                    let mut t = Thing::new(TK::Collect { member: e.sub }, pos, 0);
                    t.life = i32::MAX;
                    t.sprite = collect_sprite(e.sub);
                    self.spawn(t);
                }
                8 => self.triggers.push(Trigger {
                    sub: e.sub,
                    x: e.x,
                    y: e.y,
                    w: e.w as i32,
                    h: e.h as i32,
                    target_group: e.target_group,
                    timer: e.value as i32 + 1,
                    absorb: 5000,
                    alive: true,
                    inside: Vec::new(),
                }),
                10 => {
                    let p = [cx, cy, ground + 4.0];
                    self.trucks.push(Truck { pos: p, prev_pos: p, yaw: 0.0, prev_yaw: 0.0, state: TruckState::Idle, count: 0, craft: None, drop_wp: 0 });
                }
                _ => {}
            }
        }
    }

    fn swap_area(&mut self, e: &crate::data::level::Entity) {
        let Some(src) = self.lvl.entities.iter().find(|s| s.index == e.link as usize).copied() else { return };
        let mut m = Morph { cells: Vec::new(), t: 0, n: 32 };
        for dy in 0..=e.h as i32 {
            for dx in 0..=e.w as i32 {
                let a = (e.x + dx, e.y + dy);
                let b = (src.x + dx, src.y + dy);
                let ca = *self.lvl.cell(a.0, a.1);
                let cb = *self.lvl.cell(b.0, b.1);
                {
                    let c = self.lvl.cell_mut(a.0, a.1);
                    c.light = cb.light;
                    c.tex = cb.tex;
                    c.orient = cb.orient;
                }
                {
                    let c = self.lvl.cell_mut(b.0, b.1);
                    c.light = ca.light;
                    c.tex = ca.tex;
                    c.orient = ca.orient;
                }
                m.cells.push((a, ca.height as f32, cb.height as f32));
                m.cells.push((b, cb.height as f32, ca.height as f32));
            }
        }
        self.morphs.push(m);
        self.mesh_dirty = true;
    }

    pub fn step_morphs(&mut self) {
        let mut any = false;
        for m in &mut self.morphs {
            m.t += 1;
            let k = (m.t as f32 / m.n as f32).min(1.0);
            for &((x, y), from, to) in &m.cells {
                self.lvl.cell_mut(x, y).height = (from + (to - from) * k).round() as u16;
            }
            any = true;
        }
        self.morphs.retain(|m| m.t < m.n);
        if any && (self.frame % 4 == 0 || self.morphs.is_empty()) {
            self.mesh_dirty = true;
        }
    }
}

/// Collectable base sprite id by member (spec 06 §9).
pub fn collect_sprite(member: u8) -> i32 {
    match member {
        0 => 118,
        1 => -1,
        2 => 94,
        3 => 98,
        4 => 102,
        5 => 82,
        6 => 86,
        7 => 90,
        8 => 70,
        9 => 74,
        10 => 78,
        11 => 106,
        12 => 110,
        13 => 114,
        _ => -1,
    }
}
