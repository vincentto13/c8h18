//! Menu model, tree, value handlers and drawing (spec 02 §3).
use super::keys::{self, Action};
use super::markup::{sfx, st, Mk, S_CHANGE, S_SELECT};
use crate::ctx::Ctx;
use crate::input::{KeyCode, KeyPress};
use std::sync::atomic::{AtomicBool, Ordering};

/// COMPUTER PLAYERS: on at every start, not saved.
pub static COMPUTER_PLAYERS: AtomicBool = AtomicBool::new(true);

pub fn computer_players() -> bool {
    COMPUTER_PLAYERS.load(Ordering::Relaxed)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuId {
    Main,
    Race,
    Champ,
    Options,
    Detail,
    Sound,
    Lang,
    /// OPTIONS -> LANGUAGE (port addition): the language list plus a way back
    Language,
    PauseRoot,
    Controls,
    PauseZoom,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActId {
    Quit,
    Single,
    Continue,
    New,
    Load,
    Save,
    QuitChamp,
    ChangeName,
    Lang(usize),
    ResetKeys,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValId {
    ComputerPlayers,
    Difficulty,
    Shading,
    Sky,
    Texture,
    MusicVol,
    FxVol,
    Bind(Action),
    /// gamepad analog steering (port addition)
    PadAnalog,
    /// display settings (port additions)
    ViewDistance,
    Antialias,
    Smoothing,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    Open(MenuId),
    /// RACE → CHAMPIONSHIP: sets championship mode, cursor on CONTINUE or NEW
    OpenChamp,
    Back,
    Action(ActId),
    Value(ValId),
}

pub struct Item {
    pub label: Vec<u8>,
    pub act: Act,
    pub hidden: bool,
    pub adjustable: bool,
    pub no_select_sound: bool,
}

fn item(label: Vec<u8>, act: Act) -> Item {
    Item { label, act, hidden: false, adjustable: false, no_select_sound: false }
}

fn onoff(lang: usize, n: usize, on: bool) -> Vec<u8> {
    let mut v = st(lang, n);
    v.extend_from_slice(if on { b" d" } else { b" c" });
    v
}

/// A port addition's on/off item (label from the language files' named entries).
fn named_onoff(lang: usize, key: &str, on: bool) -> Vec<u8> {
    let mut v = crate::data::strings::to_cp437(crate::data::strings::named(lang, key));
    v.extend_from_slice(if on { b" d" } else { b" c" });
    v
}

/// A port addition's bar item.
fn named_bar(lang: usize, key: &str, filled: usize, total: usize) -> Vec<u8> {
    let mut v = crate::data::strings::to_cp437(crate::data::strings::named(lang, key));
    v.push(b' ');
    for i in 0..total {
        v.push(if i < filled { b'_' } else { b'a' });
    }
    v
}

fn bar(lang: usize, n: usize, filled: usize, total: usize) -> Vec<u8> {
    let mut v = st(lang, n);
    v.push(b' ');
    for i in 0..total {
        v.push(if i < filled { b'_' } else { b'a' });
    }
    v
}

/// The language menu's items: the original order EN, DE, FR, IT, ES, then the port's additions.
fn language_items() -> Vec<Item> {
    [0, 1, 2, 4, 3]
        .into_iter()
        .chain(5..crate::data::strings::count())
        .map(|n| item(crate::data::strings::to_cp437(crate::data::strings::language_name(n)), Act::Action(ActId::Lang(n))))
        .collect()
}

/// Items of a menu with the current settings.
pub fn items(menu: MenuId, ctx: &Ctx, waiting_bind: Option<Action>) -> Vec<Item> {
    let cfg = &*ctx.cfg;
    let l = cfg.language();
    let sound = ctx.sound;
    let video = crate::video::get();
    match menu {
        MenuId::Main => vec![
            item(st(l, 35), Act::Open(MenuId::Race)),
            item(st(l, 48), Act::Open(MenuId::Options)),
            item(st(l, 49), Act::Action(ActId::Quit)),
        ],
        MenuId::Race => vec![
            item(st(l, 47), Act::OpenChamp),
            item(st(l, 45), Act::Action(ActId::Single)),
            item(st(l, 51), Act::Back),
        ],
        MenuId::Champ => vec![
            Item { hidden: cfg.race_number() == 0, ..item(st(l, 147), Act::Action(ActId::Continue)) },
            item(st(l, 186), Act::Action(ActId::New)),
            item(st(l, 187), Act::Action(ActId::Load)),
            item(st(l, 188), Act::Action(ActId::Save)),
            item(st(l, 148), Act::Action(ActId::QuitChamp)),
        ],
        MenuId::Options => vec![
            item(st(l, 50), Act::Action(ActId::ChangeName)),
            item(st(l, 72), Act::Open(MenuId::Detail)),
            Item { hidden: !sound, ..item(st(l, 93), Act::Open(MenuId::Sound)) },
            Item { adjustable: true, ..item(onoff(l, 84, computer_players()), Act::Value(ValId::ComputerPlayers)) },
            Item { adjustable: true, ..item(bar(l, 85, cfg.difficulty() as usize, 3), Act::Value(ValId::Difficulty)) },
            item(keys::label(l, keys::L_CONTROLS), Act::Open(MenuId::Controls)),
            item(crate::data::strings::to_cp437(crate::data::strings::named(l, "language_menu")), Act::Open(MenuId::Language)),
            item(st(l, 51), Act::Back),
        ],
        MenuId::Detail => vec![
            Item { adjustable: true, ..item(onoff(l, 180, cfg.shading()), Act::Value(ValId::Shading)) },
            Item { adjustable: true, ..item(onoff(l, 181, cfg.sky()), Act::Value(ValId::Sky)) },
            Item {
                adjustable: true,
                ..item(
                    bar(l, 182, if cfg.texture_mapping() { cfg.texture_level() as usize + 1 } else { 0 }, 5),
                    Act::Value(ValId::Texture),
                )
            },
            // port additions
            Item { adjustable: true, ..item(named_bar(l, "view_distance", video.view + 1, crate::video::VIEW_DISTANCES.len()), Act::Value(ValId::ViewDistance)) },
            Item { adjustable: true, ..item(named_onoff(l, "antialiasing", video.antialias), Act::Value(ValId::Antialias)) },
            Item { adjustable: true, ..item(named_onoff(l, "smoothing", video.smoothing), Act::Value(ValId::Smoothing)) },
            item(st(l, 56), Act::Back),
        ],
        MenuId::Sound => vec![
            Item {
                adjustable: true,
                no_select_sound: true,
                hidden: !sound,
                ..item(bar(l, 53, (cfg.music_volume() / 0x7e8) as usize, 16), Act::Value(ValId::MusicVol))
            },
            Item {
                adjustable: true,
                hidden: !sound,
                ..item(bar(l, 54, (cfg.effects_volume() / 0x7e8) as usize, 16), Act::Value(ValId::FxVol))
            },
            item(st(l, 56), Act::Back),
        ],
        MenuId::Lang => language_items(),
        MenuId::Language => {
            let mut v = language_items();
            v.push(item(st(l, 56), Act::Back));
            v
        }
        MenuId::PauseRoot => vec![
            Item { hidden: !sound, ..item(st(l, 93), Act::Open(MenuId::Sound)) },
            item(st(l, 72), Act::Open(MenuId::Detail)),
        ],
        MenuId::Controls => {
            let mut v: Vec<Item> = [Action::Accelerate, Action::Brake, Action::Left, Action::Right, Action::Fire, Action::Missile, Action::Booster]
                .iter()
                .map(|&a| item(keys::bind_label(l, a, waiting_bind == Some(a)), Act::Value(ValId::Bind(a))))
                .collect();
            v.push(item(keys::label(l, keys::L_PAUSE_ZOOM), Act::Open(MenuId::PauseZoom)));
            let mut analog = crate::data::strings::to_cp437(crate::data::strings::named(l, "analog_steering"));
            analog.extend_from_slice(if crate::pad::analog_steering() { b" d" } else { b" c" });
            v.push(Item { adjustable: true, ..item(analog, Act::Value(ValId::PadAnalog)) });
            v.push(item(keys::label(l, keys::L_RESET), Act::Action(ActId::ResetKeys)));
            v.push(item(st(l, 56), Act::Back));
            v
        }
        MenuId::PauseZoom => vec![
            item(keys::bind_label(l, Action::Pause, waiting_bind == Some(Action::Pause)), Act::Value(ValId::Bind(Action::Pause))),
            item(keys::bind_label(l, Action::ZoomIn, waiting_bind == Some(Action::ZoomIn)), Act::Value(ValId::Bind(Action::ZoomIn))),
            item(keys::bind_label(l, Action::ZoomOut, waiting_bind == Some(Action::ZoomOut)), Act::Value(ValId::Bind(Action::ZoomOut))),
            item(keys::label(l, keys::L_CONTROLS), Act::Back),
        ],
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Enter,
    Left,
    Right,
}

/// Value item handler (shared by the front end and the pause menu).
fn value(v: ValId, d: Dir, ctx: &mut Ctx) {
    let cfg = &mut *ctx.cfg;
    match v {
        ValId::ComputerPlayers => {
            COMPUTER_PLAYERS.store(!computer_players(), Ordering::Relaxed);
        }
        ValId::PadAnalog => crate::pad::set_analog_steering(!crate::pad::analog_steering()),
        ValId::Difficulty => {
            let x = cfg.difficulty() as i32;
            match d {
                Dir::Left => cfg.set_difficulty((x - 1).max(0) as u8),
                Dir::Right => cfg.set_difficulty((x + 1).min(3) as u8),
                Dir::Enter => {}
            }
        }
        ValId::Shading => cfg.set_shading(!cfg.shading()),
        ValId::Sky => cfg.set_sky(!cfg.sky()),
        ValId::ViewDistance => crate::video::update(|v| {
            let n = crate::video::VIEW_DISTANCES.len();
            v.view = match d {
                Dir::Left => v.view.saturating_sub(1),
                Dir::Right => (v.view + 1).min(n - 1),
                _ => (v.view + 1) % n,
            };
        }),
        ValId::Antialias => crate::video::update(|v| v.antialias = !v.antialias),
        ValId::Smoothing => crate::video::update(|v| v.smoothing = !v.smoothing),
        ValId::Texture => {
            match d {
                Dir::Left => {
                    let lv = cfg.texture_level();
                    if lv > 0 {
                        cfg.set_texture_level(lv - 1);
                    } else {
                        cfg.set_texture_mapping(false);
                    }
                }
                Dir::Right => {
                    if !cfg.texture_mapping() {
                        cfg.set_texture_mapping(true);
                    } else {
                        cfg.set_texture_level((cfg.texture_level() + 1).min(4));
                    }
                }
                Dir::Enter => return,
            }
            let lv = cfg.texture_level();
            cfg.set_near_distance(lv.min(3));
        }
        ValId::MusicVol | ValId::FxVol => {
            let music = v == ValId::MusicVol;
            let mut x = if music { cfg.music_volume() } else { cfg.effects_volume() };
            match d {
                Dir::Left if x > 0 => x -= 0x800,
                Dir::Right if x < 0x8000 => x += 0x800,
                _ => return,
            }
            if music {
                cfg.set_music_volume(x);
                ctx.audio.set_music_volume(cfg.music_volume());
                // the MUSIC VOLUME handler plays its own sample (not documented which): CHANGE
                sfx(ctx.audio, S_CHANGE);
            } else {
                cfg.set_effects_volume(x);
                ctx.audio.set_effects_volume(cfg.effects_volume());
            }
        }
        ValId::Bind(_) => {}
    }
}

pub enum MenuEv {
    None,
    /// an action item finished glowing
    Action(ActId),
    /// RACE → CHAMPIONSHIP opened (sets championship mode)
    ChampOpened,
    /// a menu was opened or closed (typewriter restarts)
    Changed,
}

pub struct MenuState {
    /// open menus with their cursor (item index)
    pub stack: Vec<(MenuId, usize)>,
    /// glowing action: (item index, frames left)
    glow: Option<(usize, u32)>,
    /// key rebinding: waiting for a key for this action
    pub waiting: Option<Action>,
    /// pad poll count when the wait began
    wait_serial: u64,
}

fn first_visible(it: &[Item]) -> usize {
    it.iter().position(|i| !i.hidden).unwrap_or(0)
}

impl MenuState {
    pub fn new(root: MenuId) -> MenuState {
        MenuState { stack: vec![(root, 0)], glow: None, waiting: None, wait_serial: 0 }
    }
    pub fn top(&self) -> MenuId {
        self.stack.last().unwrap().0
    }
    pub fn cursor(&self) -> usize {
        self.stack.last().unwrap().1
    }
    pub fn set_cursor(&mut self, c: usize) {
        self.stack.last_mut().unwrap().1 = c;
    }
    pub fn depth(&self) -> usize {
        self.stack.len()
    }
    pub fn close_all(&mut self) {
        self.stack.truncate(1);
        self.glow = None;
        self.waiting = None;
    }
    pub fn open(&mut self, m: MenuId, ctx: &Ctx) {
        let it = items(m, ctx, None);
        let mut c = first_visible(&it);
        if m == MenuId::Language {
            // start on the current language
            let cur = ctx.cfg.language();
            c = it.iter().position(|i| matches!(i.act, Act::Action(ActId::Lang(n)) if n == cur)).unwrap_or(c);
        }
        self.stack.push((m, c));
    }
    /// Close the top submenu.
    pub fn back(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
        }
    }
    /// Fix a cursor that points at a hidden item.
    fn fix_cursor(&mut self, it: &[Item]) {
        let c = self.cursor();
        if c >= it.len() || it[c].hidden {
            self.set_cursor(first_visible(it));
        }
    }

    /// Handle one frame (one key per frame, only while the box is in place).
    /// Returns the event and whether a key was handled.
    pub fn frame(&mut self, ctx: &mut Ctx, in_place: bool) -> (MenuEv, bool) {
        if let Some((i, n)) = self.glow {
            if n > 1 {
                self.glow = Some((i, n - 1));
                return (MenuEv::None, false);
            }
            self.glow = None;
            let it = items(self.top(), ctx, None);
            if let Some(Act::Action(a)) = it.get(i).map(|x| x.act) {
                return (MenuEv::Action(a), false);
            }
            return (MenuEv::None, false);
        }
        if !in_place {
            return (MenuEv::None, false);
        }
        // key rebinding capture
        if let Some(a) = self.waiting {
            // keys add up (up to 4), Enter confirms, Escape cancels; a pad button pressed after the
            // capture began becomes the action's pad button and confirms (Back cancels)
            if ctx.pad.serial > self.wait_serial {
                if let Some(&b) = ctx.pad.pressed_buttons().iter().find(|&&b| crate::pad::bindable(b) || b == crate::pad::Btn::Back) {
                    self.waiting = None;
                    if b == crate::pad::Btn::Back {
                        keys::cancel_capture();
                        sfx(ctx.audio, S_CHANGE);
                    } else {
                        keys::finish_capture(a);
                        crate::pad::bind(a, b);
                        sfx(ctx.audio, S_SELECT);
                    }
                    return (MenuEv::None, true);
                }
            }
            let Some(k) = take_key(ctx) else { return (MenuEv::None, false) };
            match k.code {
                KeyCode::Escape => {
                    self.waiting = None;
                    keys::cancel_capture();
                    sfx(ctx.audio, S_CHANGE);
                }
                KeyCode::Enter | KeyCode::NumpadEnter => {
                    self.waiting = None;
                    keys::finish_capture(a);
                    sfx(ctx.audio, S_SELECT);
                }
                c if keys::bindable(c) => {
                    if keys::capture_key(c) {
                        self.waiting = None;
                        keys::finish_capture(a);
                        sfx(ctx.audio, S_SELECT);
                    } else {
                        sfx(ctx.audio, S_CHANGE);
                    }
                }
                _ => {}
            }
            return (MenuEv::None, true);
        }
        let it = items(self.top(), ctx, None);
        self.fix_cursor(&it);
        let Some(k) = take_key(ctx) else { return (MenuEv::None, false) };
        let cur = self.cursor();
        let n = it.len();
        match k.code {
            KeyCode::ArrowUp | KeyCode::ArrowDown => {
                let up = k.code == KeyCode::ArrowUp;
                let mut c = cur;
                for _ in 0..n {
                    c = if up { (c + n - 1) % n } else { (c + 1) % n };
                    if !it[c].hidden {
                        break;
                    }
                }
                self.set_cursor(c);
                sfx(ctx.audio, S_CHANGE);
                (MenuEv::None, true)
            }
            KeyCode::Escape => {
                if self.stack.len() > 1 {
                    self.stack.pop();
                    sfx(ctx.audio, S_CHANGE);
                    (MenuEv::Changed, true)
                } else {
                    (MenuEv::None, true)
                }
            }
            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space | KeyCode::ArrowLeft | KeyCode::ArrowRight => {
                let d = match k.code {
                    KeyCode::ArrowLeft => Dir::Left,
                    KeyCode::ArrowRight => Dir::Right,
                    _ => Dir::Enter,
                };
                let item = &it[cur];
                if d != Dir::Enter && !item.adjustable {
                    return (MenuEv::None, true);
                }
                if !item.no_select_sound {
                    sfx(ctx.audio, S_SELECT);
                }
                match item.act {
                    Act::Open(m) => {
                        self.open(m, ctx);
                        (MenuEv::Changed, true)
                    }
                    Act::OpenChamp => {
                        let c = if ctx.cfg.race_number() != 0 { 0 } else { 1 };
                        self.stack.push((MenuId::Champ, c));
                        (MenuEv::ChampOpened, true)
                    }
                    Act::Back => {
                        if self.stack.len() > 1 {
                            self.stack.pop();
                        }
                        (MenuEv::Changed, true)
                    }
                    Act::Action(_) => {
                        self.glow = Some((cur, 3));
                        (MenuEv::None, true)
                    }
                    Act::Value(ValId::Bind(a)) => {
                        if d == Dir::Enter {
                            self.waiting = Some(a);
                            self.wait_serial = ctx.pad.serial;
                            keys::begin_capture();
                        }
                        (MenuEv::None, true)
                    }
                    Act::Value(v) => {
                        value(v, d, ctx);
                        (MenuEv::None, true)
                    }
                }
            }
            _ => (MenuEv::None, false),
        }
    }

    /// Markup for the deepest open menu in rows y0 / height (spec 02 §3.1).
    pub fn markup(&self, ctx: &Ctx, y0: i32, height: i32) -> Vec<u8> {
        let it = items(self.top(), ctx, self.waiting);
        let lh = ctx.assets.fonts[2].space_h() as i32 + 2;
        let vis: Vec<usize> = (0..it.len()).filter(|&i| !it[i].hidden).collect();
        let n = vis.len() as i32;
        let mut m = Mk::new();
        m.c('U', 2);
        for (row, &i) in vis.iter().enumerate() {
            let y = y0 + (height - lh * n - 2) / 2 + lh * row as i32;
            m.c('Y', y).c('J', 1);
            if self.glow.map(|g| g.0) == Some(i) {
                m.c('O', 1).c('F', 1).txt(&it[i].label).end_hl().c('O', 64);
            } else if i == self.cursor() {
                m.c('O', 0).c('F', 1).txt(&it[i].label).end_hl().c('O', 64);
            } else {
                m.c('C', 184).c('O', 64).txt(&it[i].label);
            }
        }
        m.0
    }
}

/// Take the first non-modifier key press of this frame.
pub fn take_key(ctx: &mut Ctx) -> Option<KeyPress> {
    if ctx.keys.is_empty() {
        None
    } else {
        Some(ctx.keys.remove(0))
    }
}
