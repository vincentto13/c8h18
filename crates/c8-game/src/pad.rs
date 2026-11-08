//! Port addition: gamepad input (gilrs). Replaces the original's joystick (spec 03 §11).
//!
//! The pad is polled once per display frame. Races read [`PadState`] directly (held buttons, the
//! analog stick, button presses); menus get the presses translated into the key codes they already
//! handle ([`PadState::menu_keys`]).
use crate::input::{KeyCode, KeyPress};
use gilrs::{Axis, Button, EventType, GamepadId, Gilrs};
use crate::frontend::keys::{Action, ACTIONS};
use std::sync::Mutex;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Btn {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    X,
    Y,
    Lb,
    Rb,
    Lt,
    Rt,
    Start,
    Back,
}

const N: usize = 14;
const ALL: [Btn; N] = [Btn::Up, Btn::Down, Btn::Left, Btn::Right, Btn::A, Btn::B, Btn::X, Btn::Y, Btn::Lb, Btn::Rb, Btn::Lt, Btn::Rt, Btn::Start, Btn::Back];

/// Stick dead zone and the thresholds of the stick acting as a direction (with hysteresis).
const DEAD_ZONE: f32 = 0.25;
const DIR_ON: f32 = 0.5;
const DIR_OFF: f32 = 0.35;
/// Analog triggers count as pressed above this.
const TRIGGER_ON: f32 = 0.3;
/// Menu auto-repeat for held directions.
const REPEAT_DELAY: f64 = 0.4;
const REPEAT_EVERY: f64 = 0.1;

/// Pad settings (port addition), saved in `pad.txt` in the save directory: analog steering and one
/// button per bindable action (OPTIONS → CONTROLS). The stick / D-pad directions, Back (end race)
/// and the menu buttons are fixed.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Settings {
    analog: bool,
    /// indexed by `Action as usize` (the order of `keys::ACTIONS`)
    buttons: [Option<Btn>; 10],
}

const DEFAULT: Settings = Settings {
    analog: false,
    buttons: [
        Some(Btn::Rt),    // accelerate
        Some(Btn::Lt),    // brake
        None,             // left (stick / D-pad)
        None,             // right (stick / D-pad)
        Some(Btn::A),     // minigun
        Some(Btn::B),     // missile
        Some(Btn::X),     // booster
        Some(Btn::Start), // pause
        Some(Btn::Rb),    // zoom in
        Some(Btn::Lb),    // zoom out
    ],
};

static SETTINGS: Mutex<Settings> = Mutex::new(DEFAULT);

fn settings() -> Settings {
    *SETTINGS.lock().unwrap_or_else(|e| e.into_inner())
}

fn update(f: impl FnOnce(&mut Settings)) {
    let mut s = SETTINGS.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut s);
    save(&s);
}

fn path() -> std::path::PathBuf {
    crate::data::files::save_dir().join("pad.txt")
}

fn save(s: &Settings) {
    if *s == DEFAULT {
        let _ = std::fs::remove_file(path());
        return;
    }
    let mut t = format!("analog_steering = {}\n", if s.analog { "on" } else { "off" });
    for (a, n) in ACTIONS {
        t.push_str(&format!("{n} = {}\n", s.buttons[a as usize].map(btn_name).unwrap_or("")));
    }
    let _ = std::fs::create_dir_all(crate::data::files::save_dir());
    if let Err(e) = std::fs::write(path(), t) {
        log::warn!("cannot write {}: {e}", path().display());
    }
}

fn parse(t: &str) -> Settings {
    let mut s = DEFAULT;
    for line in t.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        if k == "analog_steering" {
            s.analog = v == "on";
        } else if let Some(&(a, _)) = ACTIONS.iter().find(|(_, n)| *n == k) {
            s.buttons[a as usize] = btn_from_name(v).filter(|&b| bindable(b));
        }
    }
    // a button drives one action only
    for i in 0..s.buttons.len() {
        if s.buttons[i].is_some() && s.buttons[..i].contains(&s.buttons[i]) {
            s.buttons[i] = None;
        }
    }
    s
}

/// Read `pad.txt` from the save directory (defaults when missing).
pub fn load_settings() {
    let s = std::fs::read_to_string(path()).map(|t| parse(&t)).unwrap_or(DEFAULT);
    *SETTINGS.lock().unwrap_or_else(|e| e.into_inner()) = s;
}

pub fn analog_steering() -> bool {
    settings().analog
}

pub fn set_analog_steering(on: bool) {
    update(|s| s.analog = on);
}

/// Buttons that can be bound: everything but the directions and Back.
pub fn bindable(b: Btn) -> bool {
    !matches!(b, Btn::Up | Btn::Down | Btn::Left | Btn::Right | Btn::Back)
}

/// The pad button of an action, if any.
pub fn button(a: Action) -> Option<Btn> {
    settings().buttons[a as usize]
}

/// Whether some action uses the button.
pub fn is_bound(b: Btn) -> bool {
    settings().buttons.contains(&Some(b))
}

/// Make `b` the pad button of `a` (taken away from any other action).
pub fn bind(a: Action, b: Btn) {
    if !bindable(b) {
        return;
    }
    update(|s| {
        for x in s.buttons.iter_mut() {
            if *x == Some(b) {
                *x = None;
            }
        }
        s.buttons[a as usize] = Some(b);
    });
}

/// Default pad buttons (keeps the analog steering setting).
pub fn reset_buttons() {
    update(|s| s.buttons = DEFAULT.buttons);
}

/// The pad as seen by the game this frame.
#[derive(Default)]
pub struct PadState {
    pub connected: bool,
    held: [bool; N],
    /// buttons that went down this frame
    pressed: Vec<Btn>,
    /// direction presses including menu auto-repeat
    dir_fires: Vec<Btn>,
    /// left stick X after the dead zone, -1 (left) .. 1 (right)
    pub steer: f32,
    /// how long Back has been held, seconds
    pub back_held: f64,
    /// D-pad up, down, left, right on their own (steering with analog steering on)
    pub dpad: [bool; 4],
    /// counts polls, so a press can be told apart from one seen in an earlier frame
    pub serial: u64,
}

impl PadState {
    pub fn down(&self, b: Btn) -> bool {
        self.held[b as usize]
    }
    pub fn pressed(&self, b: Btn) -> bool {
        self.pressed.contains(&b)
    }
    /// Buttons that went down this frame.
    pub fn pressed_buttons(&self) -> &[Btn] {
        &self.pressed
    }
    /// The action's pad button is held.
    pub fn action_down(&self, a: Action) -> bool {
        button(a).is_some_and(|b| self.down(b))
    }
    /// The action's pad button went down this frame.
    pub fn action_pressed(&self, a: Action) -> bool {
        button(a).is_some_and(|b| self.pressed(b))
    }
    pub fn any_pressed(&self) -> bool {
        !self.pressed.is_empty()
    }
    /// The frame's presses as menu keys: directions → arrows (auto-repeated), A → Enter,
    /// B → Escape, Start → Enter (or Escape, to leave the pause menu).
    pub fn menu_keys(&self, start_is_escape: bool) -> Vec<KeyPress> {
        let key = |code| KeyPress { code, shift: false, right_shift: false, ctrl: false, alt: false, ch: None };
        let mut v: Vec<KeyPress> = self
            .dir_fires
            .iter()
            .map(|b| match b {
                Btn::Up => KeyCode::ArrowUp,
                Btn::Down => KeyCode::ArrowDown,
                Btn::Left => KeyCode::ArrowLeft,
                _ => KeyCode::ArrowRight,
            })
            .map(key)
            .collect();
        for b in &self.pressed {
            match b {
                Btn::A => v.push(key(KeyCode::Enter)),
                Btn::B => v.push(key(KeyCode::Escape)),
                Btn::Start => v.push(key(if start_is_escape { KeyCode::Escape } else { KeyCode::Enter })),
                _ => {}
            }
        }
        v
    }
}

pub struct Pad {
    gilrs: Option<Gilrs>,
    active: Option<GamepadId>,
    pub state: PadState,
    stick_dir: [bool; 4],
    repeat: [f64; 4],
    /// buttons held by the test automation (C8H18_AUTO padhold/padrelease)
    pub fake: [bool; N],
}

impl Pad {
    pub fn new() -> Pad {
        let gilrs = match Gilrs::new() {
            Ok(g) => Some(g),
            Err(e) => {
                log::warn!("gamepads unavailable: {e}");
                None
            }
        };
        let active = gilrs.as_ref().and_then(|g| g.gamepads().find(|(_, p)| p.is_connected()).map(|(id, _)| id));
        if let (Some(g), Some(id)) = (&gilrs, active) {
            log::info!("gamepad: {}", g.gamepad(id).name());
        }
        Pad { gilrs, active, state: PadState::default(), stick_dir: [false; 4], repeat: [0.0; 4], fake: [false; N] }
    }

    /// Read the pad; call once per display frame.
    pub fn poll(&mut self, dt: f64) {
        let mut held = self.fake;
        let mut steer = 0.0;
        let mut connected = self.fake.iter().any(|&b| b);
        let mut s_dpad = [self.fake[0], self.fake[1], self.fake[2], self.fake[3]];
        if let Some(g) = &mut self.gilrs {
            while let Some(ev) = g.next_event() {
                match ev.event {
                    EventType::Connected if self.active.is_none() => {
                        self.active = Some(ev.id);
                        log::info!("gamepad connected: {}", g.gamepad(ev.id).name());
                    }
                    EventType::Disconnected if self.active == Some(ev.id) => {
                        self.active = g.gamepads().find(|(id, p)| *id != ev.id && p.is_connected()).map(|(id, _)| id);
                    }
                    // the pad last used is the one that drives the game
                    EventType::ButtonPressed(..) => self.active = Some(ev.id),
                    _ => {}
                }
            }
            if let Some(p) = self.active.map(|id| g.gamepad(id)).filter(|p| p.is_connected()) {
                connected = true;
                let (x, y) = (p.value(Axis::LeftStickX), p.value(Axis::LeftStickY));
                let dir = [y, -y, -x, x]; // up, down, left, right (stick Y is positive up)
                for i in 0..4 {
                    self.stick_dir[i] = if self.stick_dir[i] { dir[i] > DIR_OFF } else { dir[i] > DIR_ON };
                }
                steer = if x.abs() < DEAD_ZONE { 0.0 } else { (x.abs() - DEAD_ZONE) / (1.0 - DEAD_ZONE) * x.signum() };
                let trig = |b| p.button_data(b).map(|d| d.value()).unwrap_or(0.0) > TRIGGER_ON;
                let pairs = [
                    (Btn::Up, p.is_pressed(Button::DPadUp) || self.stick_dir[0]),
                    (Btn::Down, p.is_pressed(Button::DPadDown) || self.stick_dir[1]),
                    (Btn::Left, p.is_pressed(Button::DPadLeft) || self.stick_dir[2]),
                    (Btn::Right, p.is_pressed(Button::DPadRight) || self.stick_dir[3]),
                    (Btn::A, p.is_pressed(Button::South)),
                    (Btn::B, p.is_pressed(Button::East)),
                    (Btn::X, p.is_pressed(Button::West)),
                    (Btn::Y, p.is_pressed(Button::North)),
                    (Btn::Lb, p.is_pressed(Button::LeftTrigger)),
                    (Btn::Rb, p.is_pressed(Button::RightTrigger)),
                    (Btn::Lt, p.is_pressed(Button::LeftTrigger2) || trig(Button::LeftTrigger2)),
                    (Btn::Rt, p.is_pressed(Button::RightTrigger2) || trig(Button::RightTrigger2)),
                    (Btn::Start, p.is_pressed(Button::Start)),
                    (Btn::Back, p.is_pressed(Button::Select)),
                ];
                s_dpad = [p.is_pressed(Button::DPadUp), p.is_pressed(Button::DPadDown), p.is_pressed(Button::DPadLeft), p.is_pressed(Button::DPadRight)];
                for (b, on) in pairs {
                    held[b as usize] |= on;
                }
            } else {
                self.stick_dir = [false; 4];
            }
        }
        let s = &mut self.state;
        s.pressed = ALL.iter().copied().filter(|&b| held[b as usize] && !s.held[b as usize]).collect();
        s.dir_fires.clear();
        for (i, b) in [Btn::Up, Btn::Down, Btn::Left, Btn::Right].into_iter().enumerate() {
            if !held[b as usize] {
                self.repeat[i] = 0.0;
            } else if !s.held[b as usize] {
                s.dir_fires.push(b);
                self.repeat[i] = REPEAT_DELAY;
            } else {
                self.repeat[i] -= dt;
                if self.repeat[i] <= 0.0 {
                    s.dir_fires.push(b);
                    self.repeat[i] += REPEAT_EVERY;
                }
            }
        }
        s.back_held = if held[Btn::Back as usize] { s.back_held + dt } else { 0.0 };
        s.held = held;
        s.steer = steer;
        s.dpad = s_dpad;
        s.serial += 1;
        s.connected = connected;
    }
}

/// Button name as shown in the CONTROLS menu and written to `pad.txt`.
pub fn btn_name(b: Btn) -> &'static str {
    match b {
        Btn::Up => "UP",
        Btn::Down => "DOWN",
        Btn::Left => "LEFT",
        Btn::Right => "RIGHT",
        Btn::A => "A",
        Btn::B => "B",
        Btn::X => "X",
        Btn::Y => "Y",
        Btn::Lb => "LB",
        Btn::Rb => "RB",
        Btn::Lt => "LT",
        Btn::Rt => "RT",
        Btn::Start => "START",
        Btn::Back => "BACK",
    }
}

/// Button from its name (`pad.txt`, test automation); case-insensitive.
pub fn btn_from_name(n: &str) -> Option<Btn> {
    let n = n.to_ascii_uppercase();
    let n = n.as_str();
    Some(match n {
        "UP" => Btn::Up,
        "DOWN" => Btn::Down,
        "LEFT" => Btn::Left,
        "RIGHT" => Btn::Right,
        "A" => Btn::A,
        "B" => Btn::B,
        "X" => Btn::X,
        "Y" => Btn::Y,
        "LB" => Btn::Lb,
        "RB" => Btn::Rb,
        "LT" => Btn::Lt,
        "RT" => Btn::Rt,
        "START" => Btn::Start,
        "BACK" => Btn::Back,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad() -> Pad {
        Pad { gilrs: None, active: None, state: PadState::default(), stick_dir: [false; 4], repeat: [0.0; 4], fake: [false; N] }
    }
    fn codes(p: &Pad, esc: bool) -> Vec<KeyCode> {
        p.state.menu_keys(esc).iter().map(|k| k.code).collect()
    }

    #[test]
    fn presses_are_edges() {
        let mut p = pad();
        p.fake[Btn::A as usize] = true;
        p.poll(0.016);
        assert!(p.state.pressed(Btn::A) && p.state.down(Btn::A) && p.state.connected);
        assert_eq!(codes(&p, false), vec![KeyCode::Enter]);
        p.poll(0.016);
        assert!(!p.state.pressed(Btn::A) && p.state.down(Btn::A));
        assert!(codes(&p, false).is_empty());
        p.fake[Btn::A as usize] = false;
        p.poll(0.016);
        assert!(!p.state.down(Btn::A) && !p.state.connected);
    }

    #[test]
    fn menu_mapping_and_start_in_pause() {
        let mut p = pad();
        p.fake[Btn::B as usize] = true;
        p.fake[Btn::Start as usize] = true;
        p.poll(0.016);
        assert_eq!(codes(&p, false), vec![KeyCode::Escape, KeyCode::Enter]);
        assert_eq!(codes(&p, true), vec![KeyCode::Escape, KeyCode::Escape]);
    }

    #[test]
    fn held_direction_repeats() {
        let mut p = pad();
        p.fake[Btn::Down as usize] = true;
        let mut fires = 0;
        for _ in 0..60 {
            p.poll(1.0 / 60.0); // one second held
            fires += codes(&p, false).iter().filter(|&&c| c == KeyCode::ArrowDown).count();
        }
        // first press, then after 0.4 s one every 0.1 s: 1 + 6 or 7
        assert!((7..=8).contains(&fires), "{fires}");
    }

    #[test]
    fn settings_file() {
        let s = parse("analog_steering = on\nfire = y\nmissile = Y\nbooster = BACK\npause =\n");
        assert!(s.analog);
        assert_eq!(s.buttons[Action::Fire as usize], Some(Btn::Y));
        assert_eq!(s.buttons[Action::Missile as usize], None, "a button drives one action only");
        assert_eq!(s.buttons[Action::Booster as usize], None, "Back cannot be bound");
        assert_eq!(s.buttons[Action::Pause as usize], None);
        assert_eq!(s.buttons[Action::Accelerate as usize], Some(Btn::Rt), "missing lines keep defaults");
        assert_eq!(parse(""), DEFAULT);
    }

    #[test]
    fn back_hold_time() {
        let mut p = pad();
        p.fake[Btn::Back as usize] = true;
        for _ in 0..61 {
            p.poll(1.0 / 60.0);
        }
        assert!(p.state.back_held >= 1.0);
        p.fake[Btn::Back as usize] = false;
        p.poll(1.0 / 60.0);
        assert_eq!(p.state.back_held, 0.0);
    }
}
