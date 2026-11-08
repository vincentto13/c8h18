//! The morphing translucent text box (spec 02 §2.2). Rectangles in 320×200 units.
use super::markup::{sfx, S_REFUEL, S_WINOPEN};
use crate::assets::Assets;
use crate::audio::{Audio, MODE_RESTART};
use crate::gfx::canvas::{Blit, Canvas};

pub type Rect = [i32; 4];

pub const BOX_MENU: Rect = [16, 70, 288, 119];
pub const BOX_CAROUSEL: Rect = [16, 8, 288, 64];
pub const BOX_TABLE: Rect = [16, 16, 288, 168];
pub const BOX_STATS: Rect = [4, 4, 312, 192];
pub const BOX_PAUSE: Rect = [16, 39, 288, 121];
pub const BOX_PARKED: Rect = [135, -12, 50, 24];

#[derive(Clone)]
pub struct TextBox {
    cur: Rect,
    counter: i32,
    shown: Rect,
    in_place: bool,
}

impl TextBox {
    pub fn parked() -> TextBox {
        TextBox { cur: BOX_PARKED, counter: 0, shown: BOX_PARKED, in_place: false }
    }
    /// Make the same rectangle animate again.
    pub fn reanimate(&mut self) {
        self.cur[0] -= 1;
        self.in_place = false;
    }
    pub fn in_place(&self) -> bool {
        self.in_place
    }
    /// One frame towards the target; true when the box is in place (text may be drawn).
    pub fn update(&mut self, t: Rect, audio: &mut Audio) -> bool {
        if self.counter == 0 {
            if t == self.cur {
                self.shown = t;
                self.in_place = true;
                return true;
            }
            self.counter = 32;
        }
        self.in_place = false;
        self.counter -= 1;
        let c = self.counter;
        if c == 0 {
            self.cur = t;
            self.shown = t;
            sfx(audio, S_WINOPEN);
        } else if c < 16 {
            for i in 0..4 {
                self.shown[i] = self.cur[i] + (t[i] - self.cur[i]) * (16 - c) / 16;
            }
        } else {
            let s = [t[0] + t[2] / 2 - 25, t[1] + t[3] / 2 - 12, 50, 24];
            for i in 0..4 {
                self.shown[i] = self.cur[i] + (s[i] - self.cur[i]) * (32 - c) / 16;
            }
            if c == 16 {
                self.cur = self.shown;
            }
        }
        audio.play(0, S_REFUEL, 0x7fff, 0x8000, 300, false, MODE_RESTART);
        false
    }
    pub fn draw(&self, cv: &mut Canvas, assets: &Assets) {
        let [x, y, w, h] = self.shown;
        let (vx, vy, vw, vh) = (2 * x, 2 * y, 2 * w, 2 * h);
        // interior: blend[184][old]
        for yy in vy + 1..vy + vh - 1 {
            if yy < 0 || yy >= cv.h {
                continue;
            }
            for xx in vx + 1..vx + vw - 1 {
                cv.put(xx, yy, 184, Blit::Blend);
            }
        }
        for xx in vx..vx + vw {
            cv.put(xx, vy, 188, Blit::Solid);
            cv.put(xx, vy + vh - 1, 118, Blit::Solid);
        }
        for yy in vy..vy + vh {
            cv.put(vx, yy, 188, Blit::Solid);
            cv.put(vx + vw - 1, yy, 118, Blit::Solid);
        }
        let p = &assets.panel;
        cv.sprite(p.get(300), vx - 8, vy - 8, Blit::Solid);
        cv.sprite(p.get(301), vx + vw - 48, vy - 8, Blit::Solid);
        cv.sprite(p.get(302), vx - 8, vy + vh - 24, Blit::Solid);
        cv.sprite(p.get(303), vx + vw - 48, vy + vh - 24, Blit::Solid);
    }
}
