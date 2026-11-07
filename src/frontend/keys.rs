//! Port addition: race key bindings, `keys.txt` and the OPTIONS → CONTROLS labels (spec 03 §11, 01 §16).
use crate::data::files::save_dir;
use crate::input::KeyCode;
use std::cell::RefCell;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Accelerate,
    Brake,
    Left,
    Right,
    Fire,
    Missile,
    Booster,
    Pause,
    ZoomIn,
    ZoomOut,
}

const ACTIONS: [(Action, &str); 10] = [
    (Action::Accelerate, "accelerate"),
    (Action::Brake, "brake"),
    (Action::Left, "left"),
    (Action::Right, "right"),
    (Action::Fire, "fire"),
    (Action::Missile, "missile"),
    (Action::Booster, "booster"),
    (Action::Pause, "pause"),
    (Action::ZoomIn, "zoom_in"),
    (Action::ZoomOut, "zoom_out"),
];

#[derive(Clone, Debug, PartialEq)]
pub struct Bindings {
    pub accelerate: Vec<KeyCode>,
    pub brake: Vec<KeyCode>,
    pub left: Vec<KeyCode>,
    pub right: Vec<KeyCode>,
    pub fire: Vec<KeyCode>,
    pub missile: Vec<KeyCode>,
    pub booster: Vec<KeyCode>,
    pub pause: Vec<KeyCode>,
    pub zoom_in: Vec<KeyCode>,
    pub zoom_out: Vec<KeyCode>,
}

impl Default for Bindings {
    fn default() -> Bindings {
        use KeyCode::*;
        Bindings {
            accelerate: vec![ArrowUp, KeyW],
            brake: vec![ArrowDown, KeyS],
            left: vec![ArrowLeft, KeyA],
            right: vec![ArrowRight, KeyD],
            fire: vec![KeyZ, Comma, KeyV],
            missile: vec![KeyX, Period, KeyB],
            booster: vec![KeyC, Slash, Space],
            pause: vec![KeyP],
            zoom_in: vec![Equal],
            zoom_out: vec![Minus],
        }
    }
}

impl Bindings {
    pub fn get(&self, a: Action) -> &Vec<KeyCode> {
        match a {
            Action::Accelerate => &self.accelerate,
            Action::Brake => &self.brake,
            Action::Left => &self.left,
            Action::Right => &self.right,
            Action::Fire => &self.fire,
            Action::Missile => &self.missile,
            Action::Booster => &self.booster,
            Action::Pause => &self.pause,
            Action::ZoomIn => &self.zoom_in,
            Action::ZoomOut => &self.zoom_out,
        }
    }
    pub fn get_mut(&mut self, a: Action) -> &mut Vec<KeyCode> {
        match a {
            Action::Accelerate => &mut self.accelerate,
            Action::Brake => &mut self.brake,
            Action::Left => &mut self.left,
            Action::Right => &mut self.right,
            Action::Fire => &mut self.fire,
            Action::Missile => &mut self.missile,
            Action::Booster => &mut self.booster,
            Action::Pause => &mut self.pause,
            Action::ZoomIn => &mut self.zoom_in,
            Action::ZoomOut => &mut self.zoom_out,
        }
    }
    fn to_text(&self) -> String {
        let mut s = String::new();
        for (a, n) in ACTIONS {
            let names: Vec<&str> = self.get(a).iter().filter_map(|&k| key_name(k)).collect();
            s.push_str(&format!("{n} = {}\n", names.join(" ")));
        }
        s
    }
    fn parse(t: &str) -> Bindings {
        let mut b = Bindings::default();
        for line in t.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let k = k.trim();
            let Some(&(a, _)) = ACTIONS.iter().find(|(_, n)| *n == k) else { continue };
            let keys: Vec<KeyCode> = v.split_whitespace().filter_map(key_from_name).filter(|&k| bindable(k)).take(4).collect();
            *b.get_mut(a) = keys;
        }
        b
    }
}

fn path() -> std::path::PathBuf {
    save_dir().join("keys.txt")
}

/// Current bindings (from `keys.txt` in the save directory, defaults when missing).
pub fn load_bindings() -> Bindings {
    match std::fs::read_to_string(path()) {
        Ok(t) => Bindings::parse(&t),
        Err(_) => Bindings::default(),
    }
}

thread_local! {
    static CUR: RefCell<Option<Bindings>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Bindings) -> R) -> R {
    CUR.with(|c| {
        let mut c = c.borrow_mut();
        let b = c.get_or_insert_with(load_bindings);
        f(b)
    })
}

fn store(b: &Bindings) {
    if *b == Bindings::default() {
        let _ = std::fs::remove_file(path());
    } else {
        let _ = std::fs::create_dir_all(save_dir());
        let _ = std::fs::write(path(), b.to_text());
    }
}

/// Keys with fixed meanings cannot be bound.
pub fn bindable(k: KeyCode) -> bool {
    use KeyCode::*;
    !matches!(
        k,
        Escape | Enter | NumpadEnter | Tab | AltLeft | AltRight | F1 | F2 | F3 | F4 | F5 | F6 | F7 | F8 | F9 | F10
            | Digit1 | Digit2 | Digit3 | Digit4 | Digit5 | Digit6 | Digit7 | Digit8
    ) && key_name(k).is_some()
}

/// Bind `k` as the only key of `a` (removed from every other action).
pub fn bind(a: Action, k: KeyCode) {
    if !bindable(k) {
        return;
    }
    with(|b| {
        for (x, _) in ACTIONS {
            b.get_mut(x).retain(|&y| y != k);
        }
        *b.get_mut(a) = vec![k];
        store(b);
    });
}

pub fn reset() {
    with(|b| {
        *b = Bindings::default();
        store(b);
    });
}

const KEYS: &[(KeyCode, &str, &str)] = {
    use KeyCode::*;
    &[
        (ArrowUp, "Up", "UP"), (ArrowDown, "Down", "DOWN"), (ArrowLeft, "Left", "LEFT"), (ArrowRight, "Right", "RIGHT"),
        (KeyA, "A", "A"), (KeyB, "B", "B"), (KeyC, "C", "C"), (KeyD, "D", "D"), (KeyE, "E", "E"), (KeyF, "F", "F"),
        (KeyG, "G", "G"), (KeyH, "H", "H"), (KeyI, "I", "I"), (KeyJ, "J", "J"), (KeyK, "K", "K"), (KeyL, "L", "L"),
        (KeyM, "M", "M"), (KeyN, "N", "N"), (KeyO, "O", "O"), (KeyP, "P", "P"), (KeyQ, "Q", "Q"), (KeyR, "R", "R"),
        (KeyS, "S", "S"), (KeyT, "T", "T"), (KeyU, "U", "U"), (KeyV, "V", "V"), (KeyW, "W", "W"), (KeyX, "X", "X"),
        (KeyY, "Y", "Y"), (KeyZ, "Z", "Z"),
        (Digit0, "0", "0"), (Digit9, "9", "9"),
        (Space, "Space", "SPACE"), (Comma, "Comma", ","), (Period, "Period", "."), (Slash, "Slash", "/"),
        (Semicolon, "Semicolon", ";"), (Quote, "Quote", "'"), (BracketLeft, "BracketLeft", "["), (BracketRight, "BracketRight", "]"),
        (Backslash, "Backslash", "\\"), (Minus, "Minus", "-"), (Equal, "Equal", "="), (Backquote, "Backquote", "`"),
        (ShiftLeft, "ShiftLeft", "LSHIFT"), (ShiftRight, "ShiftRight", "RSHIFT"),
        (ControlLeft, "ControlLeft", "LCTRL"), (ControlRight, "ControlRight", "RCTRL"),
        (Backspace, "Backspace", "BKSP"), (Insert, "Insert", "INS"), (Delete, "Delete", "DEL"), (Home, "Home", "HOME"),
        (End, "End", "END"), (PageUp, "PageUp", "PGUP"), (PageDown, "PageDown", "PGDN"),
        (F11, "F11", "F11"), (F12, "F12", "F12"),
        (Numpad0, "Numpad0", "KP0"), (Numpad1, "Numpad1", "KP1"), (Numpad2, "Numpad2", "KP2"), (Numpad3, "Numpad3", "KP3"),
        (Numpad4, "Numpad4", "KP4"), (Numpad5, "Numpad5", "KP5"), (Numpad6, "Numpad6", "KP6"), (Numpad7, "Numpad7", "KP7"),
        (Numpad8, "Numpad8", "KP8"), (Numpad9, "Numpad9", "KP9"), (NumpadAdd, "NumpadAdd", "KP+"), (NumpadSubtract, "NumpadSubtract", "KP-"),
        (NumpadMultiply, "NumpadMultiply", "KP*"), (NumpadDivide, "NumpadDivide", "KP/"), (NumpadDecimal, "NumpadDecimal", "KP."),
    ]
};

pub fn key_name(k: KeyCode) -> Option<&'static str> {
    KEYS.iter().find(|e| e.0 == k).map(|e| e.1)
}
pub fn key_from_name(n: &str) -> Option<KeyCode> {
    KEYS.iter().find(|e| e.1.eq_ignore_ascii_case(n)).map(|e| e.0)
}
fn key_label(k: KeyCode) -> &'static str {
    KEYS.iter().find(|e| e.0 == k).map(|e| e.2).unwrap_or("?")
}

// ---- labels (port addition, all five languages: EN, DE, FR, ES, IT) ----
pub const L_CONTROLS: usize = 0;
pub const L_PAUSE_ZOOM: usize = 8;
pub const L_RESET: usize = 12;
const L_PRESS: usize = 13;

const LABELS: [[&str; 5]; 14] = [
    ["CONTROLS", "STEUERUNG", "COMMANDES", "CONTROLES", "COMANDI"],
    ["ACCELERATE", "GAS", "ACCELERER", "ACELERAR", "ACCELERA"],
    ["BRAKE", "BREMSE", "FREINER", "FRENAR", "FRENA"],
    ["LEFT", "LINKS", "GAUCHE", "IZQUIERDA", "SINISTRA"],
    ["RIGHT", "RECHTS", "DROITE", "DERECHA", "DESTRA"],
    ["MINIGUN", "KNARRE", "MITRAILLEUSE", "CAÑON", "MITRAGLIATRICE"],
    ["MISSILE", "RAKETEN", "MISSILES", "MISILES", "RAZZI"],
    ["BOOSTER", "BOOSTER", "TURBO", "PROPULSOR", "BOOSTER"],
    ["PAUSE AND ZOOM", "PAUSE UND ZOOM", "PAUSE ET ZOOM", "PAUSA Y ZOOM", "PAUSA E ZOOM"],
    ["PAUSE", "PAUSE", "PAUSE", "PAUSA", "PAUSA"],
    ["ZOOM IN", "ZOOM NAH", "ZOOM AVANT", "ACERCAR", "ZOOM AVANTI"],
    ["ZOOM OUT", "ZOOM FERN", "ZOOM ARRIERE", "ALEJAR", "ZOOM INDIETRO"],
    ["RESET TO DEFAULTS", "STANDARD WIEDERHERSTELLEN", "VALEURS PAR DEFAUT", "VALORES INICIALES", "RIPRISTINA DEFAULT"],
    ["PRESS A KEY", "TASTE DRÜCKEN", "APPUYEZ SUR UNE TOUCHE", "PULSA UNA TECLA", "PREMI UN TASTO"],
];

pub fn label(lang: usize, n: usize) -> Vec<u8> {
    crate::data::strings::to_cp437(LABELS[n][lang.min(4)])
}

fn action_label(a: Action) -> usize {
    match a {
        Action::Accelerate => 1,
        Action::Brake => 2,
        Action::Left => 3,
        Action::Right => 4,
        Action::Fire => 5,
        Action::Missile => 6,
        Action::Booster => 7,
        Action::Pause => 9,
        Action::ZoomIn => 10,
        Action::ZoomOut => 11,
    }
}

/// `"<ACTION>: <keys>"` or `"<ACTION>: PRESS A KEY"`.
pub fn bind_label(lang: usize, a: Action, waiting: bool) -> Vec<u8> {
    let mut v = label(lang, action_label(a));
    v.extend_from_slice(b": ");
    if waiting {
        v.extend_from_slice(&label(lang, L_PRESS));
    } else {
        let names: Vec<&str> = with(|b| b.get(a).iter().map(|&k| key_label(k)).collect());
        v.extend_from_slice(names.join(" ").as_bytes());
    }
    v
}
