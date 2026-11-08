//! Keyboard input state.
use std::collections::HashSet;
pub use winit::keyboard::KeyCode;

#[derive(Clone, Copy, Debug)]
pub struct KeyPress {
    pub code: KeyCode,
    pub shift: bool,
    pub right_shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// printable character produced (already upper-cased for letters? no: raw), if any
    pub ch: Option<char>,
}

#[derive(Default)]
pub struct Input {
    pub held: HashSet<KeyCode>,
    /// presses since the last consumer drained them
    pub pressed: Vec<KeyPress>,
}

impl Input {
    pub fn down(&self, k: KeyCode) -> bool {
        self.held.contains(&k)
    }
    pub fn shift(&self) -> bool {
        self.down(KeyCode::ShiftLeft) || self.down(KeyCode::ShiftRight)
    }
    pub fn ctrl(&self) -> bool {
        self.down(KeyCode::ControlLeft) || self.down(KeyCode::ControlRight)
    }
    pub fn alt(&self) -> bool {
        self.down(KeyCode::AltLeft) || self.down(KeyCode::AltRight)
    }
    pub fn take(&mut self) -> Vec<KeyPress> {
        std::mem::take(&mut self.pressed)
    }
    pub fn any_pressed(&self) -> bool {
        !self.pressed.is_empty()
    }
}
