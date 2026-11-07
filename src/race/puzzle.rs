//! The sliding puzzle (spec 03 §5); its state lives in the config block.
use crate::assets::Assets;
use crate::data::config::Config;
use crate::gfx::canvas::{Blit, Canvas};
use crate::input::KeyCode;

const BASE: usize = Config::PUZZLE;
const STATE: usize = BASE;
const COUNT: usize = BASE + 4;
const VISIBLE: usize = BASE + 8;
const SEED: usize = BASE + 12;
const GRID: usize = BASE + 16;

fn cell(cfg: &Config, c: usize, r: usize) -> i8 {
    cfg.raw[GRID + r * 16 + c] as i8
}
fn set_cell(cfg: &mut Config, c: usize, r: usize, v: i8) {
    cfg.raw[GRID + r * 16 + c] = v as u8;
}
fn gap(cfg: &Config) -> (usize, usize) {
    for r in 0..4 {
        for c in 0..4 {
            if cell(cfg, c, r) < 0 {
                return (c, r);
            }
        }
    }
    (3, 3)
}

pub fn visible(cfg: &Config) -> bool {
    cfg.i32(VISIBLE) != 0
}
pub fn toggle(cfg: &mut Config) {
    let v = !visible(cfg);
    cfg.set_i32(VISIBLE, v as i32);
}

/// Move the tile at (from) into the gap; returns false if illegal.
fn slide(cfg: &mut Config, dc: i32, dr: i32) -> bool {
    let (gc, gr) = gap(cfg);
    let (fc, fr) = (gc as i32 + dc, gr as i32 + dr);
    if !(0..4).contains(&fc) || !(0..4).contains(&fr) {
        return false;
    }
    let t = cell(cfg, fc as usize, fr as usize);
    set_cell(cfg, gc, gr, t);
    set_cell(cfg, fc as usize, fr as usize, -1);
    true
}

/// One state step per race frame while visible. `keys` = arrow presses (consumed).
pub fn step(cfg: &mut Config, keys: &[KeyCode]) {
    match cfg.i32(STATE) {
        0 => {
            for i in 0..16 {
                set_cell(cfg, i % 4, i / 4, if i == 15 { -1 } else { i as i8 });
            }
            cfg.set_i32(COUNT, 150);
            cfg.set_i32(STATE, 1);
        }
        1 => {
            loop {
                let mut seed = cfg.i32(SEED) as u32;
                seed = seed.wrapping_mul(0x24a1).wrapping_add(0x24df);
                cfg.set_i32(SEED, seed as i32);
                let ok = match seed % 5 {
                    0 => slide(cfg, 0, 1),
                    1 => slide(cfg, 0, -1),
                    2 => slide(cfg, 1, 0),
                    3 => slide(cfg, -1, 0),
                    _ => false,
                };
                if ok {
                    break;
                }
            }
            let n = cfg.i32(COUNT);
            if n < 0 {
                cfg.set_i32(STATE, 2);
            }
            cfg.set_i32(COUNT, n - 1);
        }
        2 => {
            for &k in keys {
                match k {
                    KeyCode::ArrowUp => slide(cfg, 0, 1),
                    KeyCode::ArrowDown => slide(cfg, 0, -1),
                    KeyCode::ArrowLeft => slide(cfg, 1, 0),
                    KeyCode::ArrowRight => slide(cfg, -1, 0),
                    _ => false,
                };
            }
            let home = (0..16).filter(|&i| cell(cfg, i % 4, i / 4) == i as i8).count();
            if home >= 15 && gap(cfg) == (3, 3) {
                cfg.set_cheat(true);
                set_cell(cfg, 3, 3, 15);
                cfg.set_i32(COUNT, 200);
                cfg.set_i32(STATE, 3);
            }
        }
        _ => {
            let n = cfg.i32(COUNT) - 1;
            cfg.set_i32(COUNT, n);
            if n < 0 {
                cfg.set_i32(STATE, 0);
            }
        }
    }
}

pub fn draw(cfg: &Config, a: &Assets, fg: &mut Canvas) {
    let img = &a.puzzle;
    for r in 0..4 {
        for c in 0..4 {
            let t = cell(cfg, c, r);
            if t < 0 || t > 15 {
                continue;
            }
            let (tc, tr) = (t as i32 % 4, t as i32 / 4);
            for y in 0..24 {
                for x in 0..28 {
                    let px = img.pix[((tr * 24 + y) * 112 + tc * 28 + x) as usize];
                    fg.put(104 + 28 * c as i32 + x, 52 + 24 * r as i32 + y, px, Blit::Solid);
                }
            }
        }
    }
}
