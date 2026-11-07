//! Angles (raw 16-bit units as f32/i32), movement helpers. Positions are f32 cells.
use std::f32::consts::TAU;

pub const TURN: f32 = 65536.0;
pub const DEG: f32 = 65536.0 / 360.0;

#[inline]
pub fn rad(a: f32) -> f32 {
    a * TAU / TURN
}
#[inline]
pub fn sin_a(a: f32) -> f32 {
    rad(a).sin()
}
#[inline]
pub fn cos_a(a: f32) -> f32 {
    rad(a).cos()
}
/// Wrap to 0..65536.
#[inline]
pub fn wrap(a: f32) -> f32 {
    a.rem_euclid(TURN)
}
/// Signed difference a - b in -32768..32768.
#[inline]
pub fn diff(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(TURN);
    if d >= 32768.0 { d - TURN } else { d }
}
/// atan2 in raw units.
#[inline]
pub fn atan2_raw(y: f32, x: f32) -> f32 {
    y.atan2(x) * TURN / TAU
}
/// Heading from a to b (0 = towards decreasing row, clockwise).
#[inline]
pub fn heading(dx: f32, drow: f32) -> f32 {
    wrap(atan2_raw(dx, -drow))
}
/// Move by distance d along (yaw, pitch): returns (dx, drow, dheight).
#[inline]
pub fn along(yaw: f32, pitch: f32, d: f32) -> [f32; 3] {
    let dh = -sin_a(pitch) * d;
    let d2 = cos_a(pitch) * d;
    [sin_a(yaw) * d2, -cos_a(yaw) * d2, dh]
}
#[inline]
pub fn along2(yaw: f32, d: f32) -> [f32; 2] {
    [sin_a(yaw) * d, -cos_a(yaw) * d]
}
#[inline]
pub fn len2(v: [f32; 2]) -> f32 {
    (v[0] * v[0] + v[1] * v[1]).sqrt()
}
#[inline]
pub fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}
#[inline]
pub fn dist3(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}
pub const U: f32 = 1.0 / 256.0;

/// The game's LCG: seed = seed * 9377 + 9439.
#[derive(Clone, Copy, Debug)]
pub struct Lcg(pub u32);
impl Lcg {
    #[inline]
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(9377).wrapping_add(9439);
        self.0
    }
    /// next value as 16-bit (thing generators are 16-bit words)
    #[inline]
    pub fn r(&mut self) -> u32 {
        (self.next() >> 8) & 0xffff
    }
    #[inline]
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.r() % n }
    }
}

/// Segment intersection test (closed: touching counts).
pub fn seg_intersect(p: [f32; 2], p2: [f32; 2], a: [f32; 2], b: [f32; 2]) -> bool {
    let d1 = [p2[0] - p[0], p2[1] - p[1]];
    let d2 = [b[0] - a[0], b[1] - a[1]];
    let den = d1[0] * d2[1] - d1[1] * d2[0];
    if den.abs() < 1e-9 {
        return false;
    }
    let w = [a[0] - p[0], a[1] - p[1]];
    let t = (w[0] * d2[1] - w[1] * d2[0]) / den;
    let u = (w[0] * d1[1] - w[1] * d1[0]) / den;
    (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)
}
