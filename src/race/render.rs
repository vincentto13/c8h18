//! Build the GPU scene for a race frame.
use super::math::*;
use super::sim::*;
use crate::assets::Assets;
use crate::data::model::{craft_model, MODEL_BAREL0, MODEL_CONE0, MODEL_RECOV};
use crate::gfx::model::{orient, push_model};
use crate::gfx::{Camera, Scene, Vertex, K_SPRITE};

/// Sprite table record: (tmap image, height in engine units, blend byte).
pub fn sprite_def(id: i32) -> Option<(usize, f32, u32)> {
    let id = id as usize;
    let r = match id {
        0..=3 => (0, [640.0, 640.0, 1600.0, 160.0][id], 0),
        4 => (39, 1440.0, 0),
        5..=20 => (17, 160.0 * (id - 4) as f32, 2),
        21 => (17, 200.0, 2),
        22..=37 => (13, 160.0 * (id - 21) as f32, 2),
        38..=53 => (25, 160.0 * (id - 37) as f32, 2),
        54..=69 => (17, 312.0 + 72.0 * (id - 54) as f32, 2),
        70..=125 => {
            let k = (id - 70) / 4;
            let v = (id - 70) % 4;
            (29 + k, [1280.0, 960.0, 640.0, 320.0][v], [0, 0, 3, 2][v])
        }
        126..=132 => (id - 126, 800.0, 0),
        133..=135 => (id - 126, 800.0, 3),
        136..=138 => (id - 126, 800.0, 2),
        139 => (0, 1200.0, 0),
        140 => (1, 800.0, 0),
        141 => (2, 1200.0, 0),
        142..=145 => (id - 139, 1600.0, 0),
        146..=148 => (id - 139, 1600.0, 3),
        149..=151 => (id - 139, 1600.0, 2),
        152..=158 => (id - 152, 160.0, 0),
        159..=161 => (id - 152, 160.0, 3),
        162..=164 => (id - 152, 160.0, 2),
        165..=168 => (43 + id - 165, 160.0, 0),
        169..=171 => (43 + id - 169, 160.0, 0),
        172 => (46, 160.0, 3),
        173 | 174 => (44 + id - 173, 160.0, 3),
        175..=177 => (46, 160.0, 2),
        178 => (47, 480.0, 2),
        _ => return None,
    };
    Some(r)
}

pub struct Interp {
    pub alpha: f32,
}
impl Interp {
    pub fn p(&self, a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        let t = self.alpha;
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
    }
    pub fn a(&self, a: f32, b: f32) -> f32 {
        a + diff(b, a) * self.alpha
    }
}

struct Billboard {
    depth: f32,
    verts: [Vertex; 6],
}

pub struct SceneBuilder<'a> {
    pub assets: &'a Assets,
    pub cam: Camera,
    pub tmap_pos: &'a [(u32, u32)],
    pub screen_h: f32,
    pub focal_px: f32,
    bills: Vec<Billboard>,
    pub opaque: Vec<Vertex>,
}

impl<'a> SceneBuilder<'a> {
    pub fn new(assets: &'a Assets, cam: Camera, tmap_pos: &'a [(u32, u32)], screen_h: f32, focal_px: f32) -> Self {
        SceneBuilder { assets, cam, tmap_pos, screen_h, focal_px, bills: Vec::new(), opaque: Vec::new() }
    }

    fn in_range(&self, p: [f32; 3]) -> bool {
        let dx = p[0] - self.cam.x;
        let dy = p[1] - self.cam.row;
        dx * dx + dy * dy < 17.5 * 17.5
    }

    /// World point (x, row, height) -> GPU world (x, height, row).
    fn gp(p: [f32; 3]) -> [f32; 3] {
        [p[0], p[2], p[1]]
    }

    pub fn model(&mut self, m: usize, pos: [f32; 3], yaw: f32, pitch: f32, roll: f32, shadow: bool, flicker: u32) {
        if !self.in_range(pos) {
            return;
        }
        if shadow {
            self.shadow(pos);
        }
        let rot = orient(yaw, pitch, roll);
        push_model(&mut self.opaque, &self.assets.models[m], &self.assets.atlas, Self::gp(pos), &rot, 1.0, flicker);
    }

    fn shadow(&mut self, pos: [f32; 3]) {
        let Some((tm, h, blend)) = sprite_def(178) else { return };
        let img = &self.assets.tmaps[tm];
        let ground = pos[2];
        let _ = ground;
        let hh = h / 2048.0;
        let ww = hh * img.w as f32 / img.h as f32;
        let (sy, cy) = rad(self.cam.yaw).sin_cos();
        let right = [cy, 0.0, sy];
        let fwd = [sy, 0.0, -cy];
        let c = Self::gp(pos);
        let (u0, v0) = self.tmap_pos[tm];
        let (u1, v1) = (u0 as f32 + img.w as f32, v0 as f32 + img.h as f32);
        let corner = |a: f32, b: f32| [c[0] + right[0] * a + fwd[0] * b, c[1] + 0.02, c[2] + right[2] * a + fwd[2] * b];
        let p = [corner(-ww / 2.0, hh / 2.0), corner(ww / 2.0, hh / 2.0), corner(ww / 2.0, -hh / 2.0), corner(-ww / 2.0, -hh / 2.0)];
        let uv = [[u0 as f32, v0 as f32], [u1, v0 as f32], [u1, v1], [u0 as f32, v1]];
        let depth = self.cam.to_view(c)[2] - 1.0;
        self.push_bill(depth, p, uv, blend);
    }

    fn push_bill(&mut self, depth: f32, p: [[f32; 3]; 4], uv: [[f32; 2]; 4], blend: u32) {
        let mk = |k: usize| Vertex { pos: p[k], uv: uv[k], light: 32.0, kind: K_SPRITE, param: blend };
        self.bills.push(Billboard { depth, verts: [mk(0), mk(1), mk(2), mk(0), mk(2), mk(3)] });
    }

    /// Upright billboard with its bottom centre at pos.
    pub fn sprite(&mut self, id: i32, pos: [f32; 3], anim: i32) {
        if id < 0 || !self.in_range(pos) {
            return;
        }
        let Some((mut tm, h, blend)) = sprite_def(id) else { return };
        if (13..=28).contains(&tm) {
            tm = tm - (tm - 13) % 4 + (anim.rem_euclid(4) as usize);
        }
        let Some(img) = self.assets.tmaps.get(tm) else { return };
        let c = Self::gp(pos);
        let v = self.cam.to_view(c);
        if v[2] < 200.0 / 2048.0 {
            return;
        }
        let hh = h / 2048.0;
        // size limit (port: scaled with the screen; 64 px at 200 lines of 320x200)
        let px = hh * self.focal_px / v[2];
        if px > 64.0 * self.screen_h / 240.0 * 1.5 {
            return;
        }
        let ww = hh * img.w as f32 / img.h as f32;
        let (r, u, _) = self.cam.basis();
        let corner = |a: f32, b: f32| [c[0] + r[0] * a + u[0] * b, c[1] + r[1] * a + u[1] * b, c[2] + r[2] * a + u[2] * b];
        let p = [corner(-ww / 2.0, hh), corner(ww / 2.0, hh), corner(ww / 2.0, 0.0), corner(-ww / 2.0, 0.0)];
        let (u0, v0) = self.tmap_pos[tm];
        let (u1, v1) = (u0 as f32 + img.w as f32, v0 as f32 + img.h as f32);
        let uv = [[u0 as f32, v0 as f32], [u1, v0 as f32], [u1, v1], [u0 as f32, v1]];
        self.push_bill(v[2] - 64.0 / 2048.0, p, uv, blend);
    }

    pub fn finish(mut self, sky: bool) -> Scene {
        self.bills.sort_by(|a, b| b.depth.partial_cmp(&a.depth).unwrap_or(std::cmp::Ordering::Equal));
        let mut sprites = Vec::with_capacity(self.bills.len() * 6);
        for b in &self.bills {
            sprites.extend_from_slice(&b.verts);
        }
        Scene { camera: self.cam, sky, world: true, opaque: self.opaque, sprites, near: 200.0 / 2048.0, far: 40.0 }
    }
}

/// Add all things of the world to the scene.
pub fn build_world(sb: &mut SceneBuilder, w: &World, ip: &Interp, hidden: Option<usize>) {
    for (i, c) in w.crafts.iter().enumerate() {
        if !c.active || c.invis > 0 || Some(i) == hidden {
            continue;
        }
        let p = ip.p(c.prev_pos, c.pos);
        let v = [ip.a(c.prev_view[0], c.view[0]), c.prev_view[1] + (c.view[1] - c.prev_view[1]) * ip.alpha, c.prev_view[2] + (c.view[2] - c.prev_view[2]) * ip.alpha];
        sb.model(craft_model(c.kind as usize, c.team as usize), p, v[0], v[1], v[2], true, w.frame);
    }
    for t in &w.trucks {
        let p = ip.p(t.prev_pos, t.pos);
        sb.model(MODEL_RECOV, p, ip.a(t.prev_yaw, t.yaw), 0.0, 0.0, true, w.frame);
    }
    for t in &w.things {
        if !t.alive {
            continue;
        }
        let p = ip.p(t.prev_pos, t.pos);
        match t.kind {
            TK::Scenery { sub } => match sub {
                6 | 7 => sb.model(MODEL_CONE0 + (sub - 6) as usize, p, t.yaw, t.pitch, 0.0, true, w.frame),
                8..=10 => sb.model(MODEL_BAREL0 + (sub - 8) as usize, p, 0.0, 0.0, 0.0, true, w.frame),
                0 => sb.model(9, p, 0.0, 0.0, 0.0, true, w.frame),
                1 => sb.sprite(4, p, 0),
                _ => {}
            },
            TK::Collect { .. } => {
                if t.hidden == 0 {
                    sb.sprite(t.sprite, p, 0);
                }
            }
            TK::Bullet | TK::Flare | TK::ExplMedium | TK::ExplBig | TK::ExplMassive => {}
            _ => {
                if t.sprite >= 0 {
                    sb.sprite(t.sprite, p, t.age);
                }
            }
        }
    }
}
