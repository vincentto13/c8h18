//! Track and craft selection carousels (spec 02 §9.2).
use crate::assets::Assets;
use crate::data::model::{craft_model, MODEL_TRACK0};
use crate::gfx::model::{mat_mul, push_model, Mat3};
use crate::gfx::{Camera, Scene};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Tracks,
    Crafts,
    /// championship: only this track (0..5)
    ChampTrack(usize),
}

pub struct Carousel {
    pub kind: Kind,
    pub sel: usize,
    /// turn counter: 24..1 positive = Left, negative = Right, 0 = idle
    pub turn: i32,
    pub spins: [i32; 6],
}

const TILT: i32 = -155;

fn rad(a: i32) -> f32 {
    a as f32 * std::f32::consts::TAU / 2048.0
}

impl Carousel {
    pub fn new(kind: Kind, sel: usize) -> Carousel {
        let mut spins = [0; 6];
        if kind == Kind::Crafts {
            for (j, s) in spins.iter_mut().enumerate() {
                *s = (j as i32 + 1) * 341;
            }
        }
        Carousel { kind, sel, turn: 0, spins }
    }
    pub fn turning(&self) -> bool {
        self.turn != 0
    }
    /// Start a turn: right = next item.
    pub fn start_turn(&mut self, right: bool) {
        self.turn = if right { -24 } else { 24 };
    }
    /// One frame; returns true when a turn ended and the selection changed.
    pub fn frame(&mut self) -> bool {
        for (j, s) in self.spins.iter_mut().enumerate() {
            *s -= match self.kind {
                Kind::Tracks => 16 + j as i32,
                Kind::Crafts => 16,
                Kind::ChampTrack(_) => 8,
            };
            *s &= 2047;
        }
        if self.turn != 0 {
            let right = self.turn < 0;
            self.turn -= self.turn.signum();
            if self.turn == 0 {
                self.sel = if right { (self.sel + 1) % 6 } else { (self.sel + 5) % 6 };
                return true;
            }
        }
        false
    }
    fn offset(&self) -> i32 {
        if self.turn == 0 {
            0
        } else {
            let o = 341 * (25 - self.turn.abs()) / 24;
            if self.turn < 0 { -o } else { o }
        }
    }

    /// The 3D scene of the ring. `team` = colour for the craft models.
    pub fn scene(&self, assets: &Assets, team: u8) -> Scene {
        let mut sc = Scene {
            camera: Camera { focal320: 240.0, fixed_43: true, ..Default::default() },
            world: false,
            near: 0.05,
            far: 20.0,
            ..Default::default()
        };
        let mut place = |model: usize, x: f32, y: f32, z: f32, spin: i32| {
            // M = Rx(tilt) · Ry(spin) on carousel vectors (x, y up, z depth)
            let (st, ct) = rad(TILT).sin_cos();
            let (ss, cs) = rad(spin).sin_cos();
            let rx: Mat3 = [[1.0, 0.0, 0.0], [0.0, ct, -st], [0.0, st, ct]];
            let ry: Mat3 = [[cs, 0.0, ss], [0.0, 1.0, 0.0], [-ss, 0.0, cs]];
            let m = mat_mul(&rx, &ry);
            // model space (x, h, r) = file (x, z, y); carousel input (x, z, -y) = P·model, world = P·carousel
            let p = |i: usize| if i == 2 { -1.0 } else { 1.0 };
            let mut rot: Mat3 = [[0.0; 3]; 3];
            for i in 0..3 {
                for j in 0..3 {
                    rot[i][j] = p(i) * m[i][j] * p(j);
                }
            }
            let pos = [x / 2048.0, y / 2048.0, -z / 2048.0];
            if let Some(md) = assets.models.get(model) {
                push_model(&mut sc.opaque, md, &assets.atlas, pos, &rot, 1.0, 0);
            }
        };
        match self.kind {
            Kind::ChampTrack(t) => place(MODEL_TRACK0 + t, 0.0, -80.0, 2140.0, self.spins[0]),
            _ => {
                let zbase = if self.kind == Kind::Tracks { 5995.0 } else { 6995.0 };
                let off = self.offset();
                for i in 0..6 {
                    let a = (i as i32 - self.sel as i32) * 341 + off;
                    let (s, c) = rad(a).sin_cos();
                    let model = if self.kind == Kind::Tracks { MODEL_TRACK0 + i } else { craft_model(i, team as usize) };
                    place(model, s * 1856.0, 350.0 - c * 716.0, zbase - c * 3336.0, self.spins[i]);
                }
            }
        }
        sc
    }
}
