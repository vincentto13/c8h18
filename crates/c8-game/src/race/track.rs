//! Static race data derived from the level: walls, checkpoints, waypoints, regions, start grid.
use super::math::*;
use crate::data::level::{Level, MAP_H, MAP_W};

#[derive(Clone, Copy, Debug)]
pub struct Wall {
    pub a: [f32; 2],
    pub b: [f32; 2],
    pub dir: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Checkpoint {
    pub value: u16,
    pub min: [f32; 2],
    pub max: [f32; 2],
}
impl Checkpoint {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.min[0] && x <= self.max[0] && y >= self.min[1] && y <= self.max[1]
    }
    pub fn centre(&self) -> [f32; 2] {
        [(self.min[0] + self.max[0]) / 2.0, (self.min[1] + self.max[1]) / 2.0]
    }
    pub fn dist(&self, x: f32, y: f32) -> f32 {
        let c = self.centre();
        len2([x - c[0], y - c[1]])
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Waypoint {
    pub x: f32,
    pub y: f32,
    pub kind: u8,
    pub child: usize,
    pub peer: usize,
    pub floor: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub kind: u8,
    pub min: [f32; 2],
    pub max: [f32; 2],
}
impl Rect {
    pub fn contains_cell(&self, x: f32, y: f32) -> bool {
        let (cx, cy) = (x.floor() + 0.5, y.floor() + 0.5);
        cx >= self.min[0] && cx <= self.max[0] && cy >= self.min[1] && cy <= self.max[1]
    }
}

pub struct Track {
    pub walls: Vec<Wall>,
    pub checkpoints: Vec<Checkpoint>,
    /// index 0 unused ("none")
    pub wps: Vec<Waypoint>,
    pub chargers: Vec<Rect>,
    pub start_cells: Vec<(i32, i32)>,
    pub start_heading: f32,
}

pub fn is_junction(kind: u8) -> bool {
    matches!(kind, 2 | 3 | 4 | 6)
}

impl Track {
    pub fn build(lvl: &Level) -> Track {
        let ent_by_index = |i: u16| lvl.entities.iter().find(|e| e.index == i as usize);
        // walls
        let mut walls = Vec::new();
        for e in lvl.entities.iter().filter(|e| e.kind == 9 && e.sub == 0 && e.link != 0) {
            if let Some(n) = ent_by_index(e.link) {
                let a = [e.x as f32 + 0.5, e.y as f32 + 0.5];
                let b = [n.x as f32 + 0.5, n.y as f32 + 0.5];
                walls.push(Wall { a, b, dir: heading(b[0] - a[0], b[1] - a[1]) });
            }
        }
        // checkpoints
        let mut checkpoints: Vec<Checkpoint> = lvl
            .entities
            .iter()
            .filter(|e| e.kind == 1 && e.sub == 5)
            .map(|e| {
                let corner = [e.x as f32 + 0.5, e.y as f32 + 0.5];
                let hw = (e.w as f32 + 1.0) / 2.0;
                let hh = (e.h as f32 + 1.0) / 2.0;
                Checkpoint { value: e.value, min: corner, max: [corner[0] + 2.0 * hw, corner[1] + 2.0 * hh] }
            })
            .collect();
        checkpoints.sort_by_key(|c| c.value);
        // waypoints
        let mut wps = vec![Waypoint::default()];
        let find_or_add = |wps: &mut Vec<Waypoint>, x: f32, y: f32, kind: u8| -> usize {
            if let Some(i) = wps.iter().skip(1).position(|w| w.x == x && w.y == y) {
                return i + 1;
            }
            if wps.len() >= 201 {
                return 0;
            }
            let idx = wps.len();
            wps.push(Waypoint { x, y, kind, child: 0, peer: idx, floor: lvl.floor_below(x, y, 255.0) });
            // junction ring
            let mut j = 0;
            for k in 1..idx {
                let w = wps[k];
                if len2([w.x - x, w.y - y]) < 500.0 / 256.0 {
                    // not yet in its ring
                    let mut in_ring = false;
                    let mut r = wps[idx].peer;
                    while r != idx {
                        if r == k {
                            in_ring = true;
                        }
                        r = wps[r].peer;
                    }
                    if !in_ring && (is_junction(kind) || is_junction(w.kind)) {
                        j = k;
                        break;
                    }
                }
            }
            if j != 0 {
                let t = wps[idx].peer;
                wps[idx].peer = wps[j].peer;
                wps[j].peer = t;
            }
            idx
        };
        for e in lvl.entities.iter().filter(|e| e.kind == 9 && e.sub != 0 && e.link != 0) {
            let Some(n) = ent_by_index(e.link) else { continue };
            let a = find_or_add(&mut wps, e.x as f32 + 0.5, e.y as f32 + 0.5, e.sub);
            let b = find_or_add(&mut wps, n.x as f32 + 0.5, n.y as f32 + 0.5, e.sub);
            if a != 0 && b != 0 && wps[a].child == 0 && a != b {
                wps[a].child = b;
            }
        }
        // regions
        let mut chargers = Vec::new();
        let mut start = None;
        for r in &lvl.regions {
            let rect = Rect {
                kind: r.kind,
                min: [(r.cx - r.hw).max(0.0), (r.cy - r.hh).max(0.0)],
                max: [(r.cx + r.hw).min(MAP_W as f32), (r.cy + r.hh).min(MAP_H as f32)],
            };
            if r.kind == 4 {
                start = Some(rect);
            } else {
                chargers.push(rect);
            }
        }
        let mut start_cells = Vec::new();
        if let Some(s) = start {
            for y in s.min[1].floor() as i32..=s.max[1].ceil() as i32 {
                for x in s.min[0].floor() as i32..=s.max[0].ceil() as i32 {
                    let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
                    if cx >= s.min[0] && cx <= s.max[0] && cy >= s.min[1] && cy <= s.max[1] && lvl.floor_tex(x, y) == 122 {
                        start_cells.push((x, y));
                    }
                }
            }
            if start_cells.is_empty() {
                for y in s.min[1].floor() as i32..s.max[1].ceil() as i32 {
                    for x in s.min[0].floor() as i32..s.max[0].ceil() as i32 {
                        start_cells.push((x, y));
                    }
                }
            }
        }
        // heading towards the start/finish line
        let mut start_heading = 0.0;
        if let (Some(cp), Some(&(sx, sy))) = (checkpoints.first(), start_cells.first()) {
            let c = cp.centre();
            let (mx, my) = start_cells.iter().fold((0.0, 0.0), |a, &(x, y)| (a.0 + x as f32 + 0.5, a.1 + y as f32 + 0.5));
            let n = start_cells.len() as f32;
            let _ = (sx, sy);
            let h = heading(c[0] - mx / n, c[1] - my / n);
            // snap to the nearest quarter turn
            start_heading = wrap((h / 16384.0).round() * 16384.0);
        }
        // order: front row first (furthest along the start heading), then row-major-ish
        let dir = along2(start_heading, 1.0);
        start_cells.sort_by(|a, b| {
            let pa = -(a.0 as f32 * dir[0] + a.1 as f32 * dir[1]);
            let pb = -(b.0 as f32 * dir[0] + b.1 as f32 * dir[1]);
            pa.partial_cmp(&pb).unwrap().then((a.1, a.0).cmp(&(b.1, b.0)))
        });
        Track { walls, checkpoints, wps, chargers, start_cells, start_heading }
    }

    /// Next checkpoint after value v (value + 1, else smallest larger, else the line).
    pub fn next_checkpoint(&self, idx: usize) -> usize {
        let v = self.checkpoints[idx].value;
        if let Some(i) = self.checkpoints.iter().position(|c| c.value == v + 1) {
            return i;
        }
        let mut best: Option<usize> = None;
        for (i, c) in self.checkpoints.iter().enumerate() {
            if c.value > v && best.map(|b| c.value < self.checkpoints[b].value).unwrap_or(true) {
                best = Some(i);
            }
        }
        best.unwrap_or(0)
    }

    pub fn absolute_nearest(&self, x: f32, y: f32) -> usize {
        let mut best = (f32::MAX, 0);
        for (i, w) in self.wps.iter().enumerate().skip(1) {
            if w.child == 0 {
                continue;
            }
            let d = len2([w.x - x, w.y - y]);
            if d < best.0 {
                best = (d, i);
            }
        }
        best.1
    }
    pub fn nearest(&self, x: f32, y: f32) -> usize {
        let mut best = (f32::MAX, 0);
        for (_i, a) in self.wps.iter().enumerate().skip(1) {
            if a.child == 0 {
                continue;
            }
            let b = self.wps[a.child];
            let l = len2([b.x - a.x, b.y - a.y]);
            if l <= 0.0 {
                continue;
            }
            let da = len2([a.x - x, a.y - y]);
            let db = len2([b.x - x, b.y - y]);
            if da < l && db < l {
                let perp = ((b.x - a.x) * (a.y - y) - (a.x - x) * (b.y - a.y)).abs() / l;
                if perp < best.0 {
                    best = (perp, a.child);
                }
            }
        }
        if best.1 == 0 { self.absolute_nearest(x, y) } else { best.1 }
    }
    pub fn junction(&self, p: usize, kind: u8) -> usize {
        if p == 0 {
            return 0;
        }
        let mut r = self.wps[p].peer;
        let mut guard = 0;
        while r != p && guard < 200 {
            if self.wps[r].kind == kind {
                return r;
            }
            r = self.wps[r].peer;
            guard += 1;
        }
        if self.wps[p].kind == kind {
            return p;
        }
        p
    }
}
