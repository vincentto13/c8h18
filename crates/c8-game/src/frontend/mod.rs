//! Front end (spec chapter 02). PUBLIC API IS FIXED — the app calls it.
mod carousel;
pub mod keys;
mod markup;
mod menu;
mod results;
mod textbox;

#[allow(unused_imports)]
pub use keys::{load_bindings, Bindings};

use crate::ctx::Ctx;
use crate::data::config::{Config, DEFAULT_LAPS};
use crate::gfx::canvas::{Blit, Canvas};
use crate::gfx::Scene;
use crate::input::{KeyCode, KeyPress};
use crate::race::types::{PlayerSetup, RaceResult, RaceSetup, AI_NAMES};
use crate::tracks;
use carousel::{Carousel, Kind};
use markup::{sfx, st, Mk, Typer, S_CHANGE, S_PRINTTYP, S_SELECT};
use menu::{ActId, MenuEv, MenuId, MenuState};
use results::Screen;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use textbox::{TextBox, BOX_CAROUSEL, BOX_MENU, BOX_PAUSE, BOX_STATS, BOX_TABLE};

pub enum FeEntry {
    /// program start (first run if cfg.fresh)
    Start,
    AfterRace(RaceResult),
    AfterDemo,
}

pub enum FeOut {
    Continue,
    /// start a race (also demo races: setup.demo); the app does the fade, ONET picture and LOADING LEVEL
    Race(RaceSetup),
    /// quit the program (the app fades out and saves the config)
    Quit,
}

/// Attract-mode flag: flipped at every idle timeout (on → demo race, off → high-score table).
static ATTRACT_DEMO: AtomicBool = AtomicBool::new(false);

const IDLE_FRAMES: u32 = 15 * 70;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum St {
    Lang,
    Name,
    Menu,
    Track,
    Craft,
    Hiscores,
    Results,
    LoadList,
    SaveList,
}

/// In-place text editing (name entry, save names): the first key replaces the old text.
struct Edit {
    old: String,
    buf: String,
    touched: bool,
    max: usize,
}

impl Edit {
    fn new(old: String, max: usize) -> Edit {
        Edit { old, buf: String::new(), touched: false, max }
    }
    fn text(&self) -> &str {
        if self.touched { &self.buf } else { &self.old }
    }
    /// Apply a key; true if it was accepted (changed the text).
    fn key(&mut self, k: &KeyPress) -> bool {
        if k.code == KeyCode::Backspace {
            if self.buf.pop().is_some() {
                self.touched = true;
                return true;
            }
            return false;
        }
        let Some(c) = k.ch else { return false };
        let c = c.to_ascii_uppercase();
        if !(' '..='~').contains(&c) {
            return false;
        }
        if self.buf.len() >= self.max {
            self.buf.pop();
        }
        self.buf.push(c);
        self.touched = true;
        true
    }
}

pub struct FrontEnd {
    st: St,
    tbox: TextBox,
    menu: MenuState,
    lang_menu: MenuState,
    typer: Typer,
    typer2: Typer,
    text: Vec<u8>,
    text2: Vec<u8>,
    text2_sound: bool,
    sig: (St, MenuId, usize, usize, usize),
    champ: bool,
    carousel: Option<Carousel>,
    screens: VecDeque<Screen>,
    idle: u32,
    hs_timer: u32,
    edit: Option<Edit>,
    list_cur: usize,
    slots: Vec<Option<String>>,
    blink: u32,
}

fn shift_q(k: &KeyPress) -> bool {
    k.code == KeyCode::KeyQ && k.shift && !k.alt && !k.ctrl
}

fn is_enter(k: &KeyPress) -> bool {
    matches!(k.code, KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space)
}

impl FrontEnd {
    /// Waiting for a key or pad button to bind (pad presses must then reach the menu raw).
    pub fn binding(&self) -> bool {
        self.menu.waiting.is_some()
    }

    pub fn new(entry: FeEntry, ctx: &mut Ctx) -> FrontEnd {
        let mut fe = FrontEnd {
            st: St::Menu,
            tbox: TextBox::parked(),
            menu: MenuState::new(MenuId::Main),
            lang_menu: MenuState::new(MenuId::Lang),
            typer: Typer::default(),
            typer2: Typer::default(),
            text: Vec::new(),
            text2: Vec::new(),
            text2_sound: false,
            sig: (St::Menu, MenuId::Main, 99, 0, 0),
            champ: false,
            carousel: None,
            screens: VecDeque::new(),
            idle: 0,
            hs_timer: 0,
            edit: None,
            list_cur: 0,
            slots: Vec::new(),
            blink: 0,
        };
        match entry {
            FeEntry::Start => {
                if let Ok(t) = std::env::var("C8H18_FE_TEST") {
                    // developer aid: pretend a race just ended (t = "single" or "champ")
                    return FrontEnd::new(FeEntry::AfterRace(fake_result(ctx, t == "champ")), ctx);
                }
                if ctx.cfg.fresh {
                    fe.st = St::Lang;
                } else if ctx.cfg.name().is_empty() {
                    fe.start_name(ctx);
                }
            }
            FeEntry::AfterDemo => {}
            FeEntry::AfterRace(res) => {
                if res.demo {
                    return fe;
                }
                let out = results::after_race(res, ctx.cfg);
                fe.champ = out.champ;
                fe.screens = out.screens.into();
                fe.menu_after_race(ctx);
                if !fe.screens.is_empty() {
                    fe.st = St::Results;
                }
            }
        }
        fe
    }

    fn menu_after_race(&mut self, ctx: &Ctx) {
        self.menu = MenuState::new(MenuId::Main);
        if self.champ {
            self.menu.stack.push((MenuId::Race, 0));
            let c = if ctx.cfg.race_number() as usize != tracks::first() { 0 } else { 1 };
            self.menu.stack.push((MenuId::Champ, c));
        }
    }

    fn start_name(&mut self, ctx: &Ctx) {
        self.st = St::Name;
        self.edit = Some(Edit::new(ctx.cfg.name(), 8));
        self.typer2.reset();
    }

    /// Track selection: the ring of the available tracks (single race) or the current race's
    /// track (championship). Stays in the menu if no track is available (port addition).
    fn start_track(&mut self, ctx: &Ctx) {
        let avail = tracks::available();
        if avail.is_empty() {
            return;
        }
        let rn = tracks::snap(ctx.cfg.race_number() as usize);
        let kind = if self.champ { Kind::ChampTrack(rn) } else { Kind::Tracks };
        let items = if self.champ { vec![rn] } else { avail.to_vec() };
        self.carousel = Some(Carousel::new(kind, items, rn));
        self.st = St::Track;
    }

    fn start_craft(&mut self, ctx: &Ctx) {
        self.carousel = Some(Carousel::new(Kind::Crafts, (0..6).collect(), ctx.cfg.craft() as usize));
        self.tbox.reanimate();
        self.st = St::Craft;
    }

    fn open_list(&mut self, ctx: &Ctx, save: bool) {
        self.st = if save { St::SaveList } else { St::LoadList };
        self.list_cur = ctx.cfg.last_slot();
        self.slots = (0..5).map(Config::slot_name).collect();
        self.edit = None;
    }

    /// RaceSetup for a race (spec 02 §10) or a demo race (§8).
    fn race_setup(&self, ctx: &Ctx, demo: bool) -> RaceSetup {
        let cfg = &*ctx.cfg;
        let rn = tracks::snap(cfg.race_number() as usize);
        let track = rn + 1;
        let champ = self.champ && !demo;
        let laps = if champ || demo { DEFAULT_LAPS[rn] as u32 } else { cfg.track_laps(rn) as u32 };
        let d = cfg.difficulty();
        // the championship's k-th race (k = race number with all six tracks) is played at min(k, d)
        let diff = if champ { d.min(tracks::ordinal_in(tracks::available(), rn) as u8) } else { d };
        let (team, craft) = (cfg.team() as usize, cfg.craft() as usize);
        let mut players = Vec::new();
        if !demo {
            players.push(PlayerSetup { name: cfg.name(), human: true, team: team as u8, craft: craft as u8, difficulty: diff });
        }
        if demo || menu::computer_players() {
            for k in if demo { 0 } else { 1 }..8 {
                players.push(PlayerSetup {
                    name: AI_NAMES[k].to_string(),
                    human: false,
                    team: ((k + team) % 8) as u8,
                    craft: ((k + craft) % 6) as u8,
                    difficulty: diff,
                });
            }
        }
        RaceSetup { track, laps, players, demo, championship: champ }
    }

    /// Global hotkeys (spec 02 §7). Returns true for Shift+Q.
    fn hotkeys(&mut self, ctx: &mut Ctx) -> bool {
        let text_entry = self.st == St::Name || (self.st == St::SaveList && self.edit.is_some());
        let mut quit = false;
        ctx.keys.retain(|k| {
            if shift_q(k) && !text_entry {
                quit = true;
                return false;
            }
            if k.alt {
                match k.code {
                    KeyCode::Digit1 | KeyCode::Digit2 | KeyCode::Digit3 | KeyCode::Digit4 => {
                        let l = match k.code {
                            KeyCode::Digit1 => 0,
                            KeyCode::Digit2 => 1,
                            KeyCode::Digit3 => 2,
                            _ => 3,
                        };
                        ctx.cfg.set_language(l);
                        return false;
                    }
                    KeyCode::KeyV => {
                        ctx.cfg.toggle_debug_overlay();
                        return false;
                    }
                    KeyCode::KeyC => return false,
                    _ => {}
                }
            }
            true
        });
        quit
    }

    /// One front-end frame (70 per second): input + logic.
    pub fn frame(&mut self, ctx: &mut Ctx) -> FeOut {
        if self.hotkeys(ctx) {
            return FeOut::Quit;
        }
        self.blink = self.blink.wrapping_add(1);
        // port addition: the race number always names an available track (a no-op with all six)
        let rn = ctx.cfg.race_number() as usize;
        if tracks::snap(rn) != rn {
            ctx.cfg.set_race_number(tracks::snap(rn) as u16);
        }
        let target = match self.st {
            St::Lang | St::Name | St::Menu | St::LoadList | St::SaveList => BOX_MENU,
            St::Track | St::Craft => BOX_CAROUSEL,
            St::Hiscores => BOX_TABLE,
            St::Results => match self.screens.front() {
                Some(Screen::Stats(_)) => BOX_STATS,
                _ => BOX_TABLE,
            },
        };
        let in_place = self.tbox.update(target, ctx.audio);
        if let Some(c) = &mut self.carousel {
            if c.frame() {
                match c.kind {
                    Kind::Tracks => ctx.cfg.set_race_number(c.item() as u16),
                    Kind::Crafts => ctx.cfg.set_craft(c.item() as u8),
                    _ => {}
                }
            }
        }
        let out = self.logic(ctx, in_place);
        if !matches!(out, FeOut::Continue) {
            return out;
        }
        self.build_text(ctx);
        // typewriter restarts whenever the screen or the drawn menu changes
        let sig = (
            self.st,
            if self.st == St::Lang { self.lang_menu.top() } else { self.menu.top() },
            self.menu.depth(),
            self.carousel.as_ref().map(|c| c.sel).unwrap_or(0),
            self.screens.len() + 10 * self.edit.is_some() as usize,
        );
        if sig != self.sig {
            self.sig = sig;
            self.typer.reset();
            self.typer2.reset();
        }
        if self.tbox.in_place() {
            let skip = ctx.input.down(KeyCode::Space);
            let silent = self.st == St::Name;
            self.typer.step(markup::visible_len(&self.text), skip, !silent, ctx.audio);
            self.typer2.step(markup::visible_len(&self.text2), skip, self.text2_sound, ctx.audio);
        }
        FeOut::Continue
    }

    fn logic(&mut self, ctx: &mut Ctx, in_place: bool) -> FeOut {
        match self.st {
            St::Lang => {
                let (ev, _) = self.lang_menu.frame(ctx, in_place);
                if let MenuEv::Action(ActId::Lang(l)) = ev {
                    ctx.cfg.set_language(l);
                    self.start_name(ctx);
                }
            }
            St::Name => {
                let keys = std::mem::take(&mut ctx.keys);
                for k in keys {
                    if matches!(k.code, KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Escape) {
                        self.edit = None;
                        self.st = St::Menu;
                        break;
                    }
                    if let Some(e) = &mut self.edit {
                        if e.key(&k) {
                            ctx.cfg.set_name(&e.buf.clone());
                            sfx(ctx.audio, S_PRINTTYP);
                            self.typer2.reset();
                        }
                    }
                }
            }
            St::Menu => return self.menu_logic(ctx, in_place),
            St::Track => {
                let Some(k) = menu::take_key(ctx) else { return FeOut::Continue };
                let champ = self.champ;
                let c = self.carousel.as_mut().unwrap();
                match k.code {
                    KeyCode::ArrowLeft | KeyCode::ArrowRight if !champ && !c.turning() && c.can_turn() => {
                        c.start_turn(k.code == KeyCode::ArrowRight);
                        sfx(ctx.audio, S_CHANGE);
                    }
                    KeyCode::ArrowUp | KeyCode::ArrowDown if !champ && !c.turning() => {
                        let t = c.item();
                        let l = ctx.cfg.track_laps(t) as i32;
                        let n = if k.code == KeyCode::ArrowUp { if l >= 50 { 1 } else { l + 1 } } else if l <= 1 { 50 } else { l - 1 };
                        ctx.cfg.set_track_laps(t, n as u8);
                        sfx(ctx.audio, S_CHANGE);
                    }
                    KeyCode::Escape if !c.turning() => {
                        sfx(ctx.audio, S_CHANGE);
                        self.carousel = None;
                        self.st = St::Menu;
                    }
                    _ if is_enter(&k) && !c.turning() => {
                        sfx(ctx.audio, S_SELECT);
                        if champ && ctx.cfg.race_number() as usize != tracks::first() {
                            return FeOut::Race(self.race_setup(ctx, false));
                        }
                        self.start_craft(ctx);
                    }
                    _ => {}
                }
            }
            St::Craft => {
                let Some(k) = menu::take_key(ctx) else { return FeOut::Continue };
                let c = self.carousel.as_mut().unwrap();
                if c.turning() {
                    return FeOut::Continue;
                }
                match k.code {
                    KeyCode::ArrowLeft | KeyCode::ArrowRight => {
                        c.start_turn(k.code == KeyCode::ArrowRight);
                        sfx(ctx.audio, S_CHANGE);
                    }
                    KeyCode::ArrowUp | KeyCode::ArrowDown => {
                        let t = ctx.cfg.team() as i32 + if k.code == KeyCode::ArrowUp { 1 } else { -1 };
                        ctx.cfg.set_team(t.rem_euclid(8) as u8);
                        sfx(ctx.audio, S_CHANGE);
                    }
                    KeyCode::Escape => {
                        sfx(ctx.audio, S_CHANGE);
                        self.tbox.reanimate();
                        self.start_track(ctx);
                    }
                    _ if is_enter(&k) => {
                        sfx(ctx.audio, S_SELECT);
                        self.tbox.reanimate();
                        return FeOut::Race(self.race_setup(ctx, false));
                    }
                    _ => {}
                }
            }
            St::Hiscores => {
                self.hs_timer += 1;
                let any = menu::take_key(ctx).is_some();
                if any || self.hs_timer > IDLE_FRAMES {
                    self.st = St::Menu;
                    self.idle = 0;
                }
            }
            St::Results => {
                if !in_place {
                    return FeOut::Continue;
                }
                let Some(k) = menu::take_key(ctx) else { return FeOut::Continue };
                if is_enter(&k) || k.code == KeyCode::Escape {
                    let was_table = matches!(self.screens.front(), Some(Screen::Table(..)));
                    self.screens.pop_front();
                    match self.screens.front() {
                        None => self.st = St::Menu,
                        Some(Screen::Table(..)) if was_table => self.tbox.reanimate(),
                        _ => {}
                    }
                }
            }
            St::LoadList | St::SaveList => return self.list_logic(ctx, in_place),
        }
        FeOut::Continue
    }

    fn menu_logic(&mut self, ctx: &mut Ctx, in_place: bool) -> FeOut {
        let (ev, handled) = self.menu.frame(ctx, in_place);
        if handled || self.menu.depth() > 1 {
            self.idle = 0;
        } else {
            self.idle += 1;
            if self.idle > IDLE_FRAMES {
                self.idle = 0;
                let demo = !ATTRACT_DEMO.load(Ordering::Relaxed);
                ATTRACT_DEMO.store(demo, Ordering::Relaxed);
                if demo && !tracks::available().is_empty() {
                    return FeOut::Race(self.race_setup(ctx, true));
                }
                self.st = St::Hiscores;
                self.hs_timer = 0;
                return FeOut::Continue;
            }
        }
        match ev {
            MenuEv::ChampOpened => self.champ = true,
            MenuEv::Action(a) => match a {
                ActId::Quit => return FeOut::Quit,
                ActId::Single => {
                    self.champ = false;
                    self.start_track(ctx);
                }
                ActId::Continue => self.start_track(ctx),
                ActId::New => {
                    ctx.cfg.set_race_number(tracks::first() as u16);
                    for k in 0..8 {
                        ctx.cfg.set_champ_points(k, 0);
                    }
                    self.start_track(ctx);
                }
                ActId::Load => self.open_list(ctx, false),
                ActId::Save => self.open_list(ctx, true),
                ActId::QuitChamp => {
                    self.champ = false;
                    results::enter_hiscores(ctx.cfg);
                    self.menu.close_all();
                    self.menu.set_cursor(0);
                }
                ActId::ChangeName => self.start_name(ctx),
                ActId::Lang(l) => {
                    ctx.cfg.set_language(l);
                    self.menu.back();
                }
                ActId::ResetKeys => {
                    keys::reset();
                    crate::pad::reset_buttons();
                }
            },
            _ => {}
        }
        FeOut::Continue
    }

    fn list_logic(&mut self, ctx: &mut Ctx, in_place: bool) -> FeOut {
        if !in_place {
            return FeOut::Continue;
        }
        let save = self.st == St::SaveList;
        if let Some(e) = &mut self.edit {
            let keys = std::mem::take(&mut ctx.keys);
            for k in keys {
                match k.code {
                    KeyCode::Escape => {
                        sfx(ctx.audio, S_CHANGE);
                        self.edit = None;
                        break;
                    }
                    KeyCode::Enter | KeyCode::NumpadEnter => {
                        let name = if e.text().is_empty() { "C8H18".to_string() } else { e.text().to_string() };
                        let slot = self.list_cur;
                        ctx.cfg.set_slot_title(&name);
                        ctx.cfg.set_last_slot(slot);
                        let mut c = ctx.cfg.clone();
                        c.set_cheat(false);
                        c.save_slot(slot);
                        self.edit = None;
                        self.st = St::Menu;
                        break;
                    }
                    _ => {
                        if e.key(&k) {
                            sfx(ctx.audio, S_PRINTTYP);
                        }
                    }
                }
            }
            return FeOut::Continue;
        }
        let Some(k) = menu::take_key(ctx) else { return FeOut::Continue };
        match k.code {
            KeyCode::ArrowUp => {
                self.list_cur = (self.list_cur + 5) % 6;
                sfx(ctx.audio, S_CHANGE);
            }
            KeyCode::ArrowDown => {
                self.list_cur = (self.list_cur + 1) % 6;
                sfx(ctx.audio, S_CHANGE);
            }
            KeyCode::Escape => {
                sfx(ctx.audio, S_CHANGE);
                self.st = St::Menu;
            }
            _ if is_enter(&k) => {
                let i = self.list_cur;
                if i == 5 {
                    sfx(ctx.audio, S_SELECT);
                    self.st = St::Menu;
                } else if save {
                    sfx(ctx.audio, S_SELECT);
                    self.edit = Some(Edit::new(self.slots[i].clone().unwrap_or_default(), 12));
                } else if self.slots[i].is_some() {
                    if let Some(c) = Config::load_slot(i) {
                        sfx(ctx.audio, S_SELECT);
                        *ctx.cfg = c;
                        ctx.cfg.set_last_slot(i);
                        ctx.audio.set_effects_volume(ctx.cfg.effects_volume());
                        ctx.audio.set_music_volume(ctx.cfg.music_volume());
                        self.champ = true;
                        self.start_track(ctx);
                    }
                }
            }
            _ => {}
        }
        FeOut::Continue
    }

    fn build_text(&mut self, ctx: &Ctx) {
        let cfg = &*ctx.cfg;
        let l = cfg.language();
        let a = ctx.assets;
        let h2 = a.fonts[2].space_h() as i32;
        let h3 = a.fonts[3].space_h() as i32;
        self.text2.clear();
        self.text2_sound = false;
        let cursor_on = (self.blink / 32) % 2 == 0;
        self.text = match self.st {
            St::Lang => self.lang_menu.markup(ctx, 140, 238),
            St::Menu => self.menu.markup(ctx, 140, 238),
            St::Name => {
                let mut m = Mk::new();
                m.c('U', 2).c('Y', 4 * h2 + 140).c('J', 1).c('C', 184).c('O', 64).txt(&st(l, 34));
                let name = self.edit.as_ref().map(|e| e.text().to_string()).unwrap_or_default();
                let mut f = Mk::new();
                f.c('U', 2).c('Y', 6 * h2 + 140).c('J', 1).c('C', 184).c('O', 64).s(&name).txt(if cursor_on { b"b" } else { b" " });
                self.text2 = f.0;
                m.0
            }
            St::Track => {
                let c = self.carousel.as_ref().unwrap();
                let t = match c.kind {
                    Kind::ChampTrack(t) => t,
                    _ => c.item(),
                };
                const TRACKS: [&str; 6] = [
                    "1. AMAZON DELTA TURNPIKE",
                    "2. TRANS-ASIA INTERSTATE",
                    "3. SHANGHAI DRAGON",
                    "4. NEW CHERNOBYL CENTRAL",
                    "5. SLAM CANYON",
                    "6. THRAK CITY",
                ];
                let mut m = Mk::new();
                {
                    m.c('U', 2).c('C', 1).c('O', 0).c('Y', h2 + 16).c('J', 1).s(TRACKS[t]);
                    let laps = if self.champ { DEFAULT_LAPS[t] } else { cfg.track_laps(t) };
                    let y = h2 + 16 + 3 * h3 - 4;
                    m.c('U', 3).c('C', 184).c('O', 64);
                    m.c('Y', y).c('J', 1).txt(&st(l, 95)).s(&format!(" {laps}"));
                    let rec = |m: &mut Mk, y: i32, n: usize, (ticks, who): (i32, String)| {
                        m.c('Y', y).c('J', 1).txt(&st(l, n));
                        if ticks > 0 {
                            m.s(&format!(" {ticks} {who}"));
                        }
                    };
                    rec(&mut m, y + h3 + 2, 40, cfg.best_lap(t));
                    rec(&mut m, y + 2 * (h3 + 2), 41, cfg.best_race(t));
                }
                m.0
            }
            St::Craft => {
                let c = self.carousel.as_ref().unwrap();
                const CRAFTS: [(&str, usize, usize, usize, usize); 6] = [
                    ("DART", 6, 5, 4, 5),
                    ("BULWARK", 3, 6, 5, 6),
                    ("MAULER", 4, 6, 5, 5),
                    ("KESTREL", 6, 6, 4, 4),
                    ("SCYTHE", 8, 4, 3, 5),
                    ("SKIMMER", 8, 4, 4, 4),
                ];
                const TEAMS: [&str; 8] = ["SALT SAINTS", "NIGHT SHIFT", "RUST HOUNDS", "SULPHUR", "COLD IRON", "FLARE CREW", "STATIC", "OCTANE"];
                let mut m = Mk::new();
                {
                    let (name, sp, we, ar, fi) = CRAFTS[c.sel];
                    let bars = |n: usize| "_".repeat(n);
                    m.c('U', 2).c('C', 1).c('O', 0).c('Y', h2 + 16).c('J', 1).s(name);
                    m.c('Y', h2 + 16 + h2 + 2).c('J', 1).s(TEAMS[cfg.team() as usize]);
                    let y = h2 + 16 + h2 + 2 + 2 * (h3 + 2);
                    m.c('U', 3).c('C', 184).c('O', 64);
                    m.c('Y', y).c('J', 1).txt(&st(l, 98)).s(&format!(" {}    ", bars(sp))).txt(&st(l, 96)).s(&format!(" {}", bars(we)));
                    m.c('Y', y + h3 + 2).c('J', 1).txt(&st(l, 97)).s(&format!(" {}    ", bars(ar))).txt(&st(l, 99)).s(&format!(" {}", bars(fi)));
                }
                m.0
            }
            St::Hiscores => results::hiscore_markup(cfg, a),
            St::Results => match self.screens.front() {
                Some(Screen::Stats(r)) => results::stats_markup(r, cfg, a),
                Some(Screen::Table(t, rows)) => results::table_markup(*t, rows, cfg, a),
                None => Vec::new(),
            },
            St::LoadList | St::SaveList => {
                let save = self.st == St::SaveList;
                let mut m = Mk::new();
                m.c('U', 2).c('C', 184).c('O', 64).c('Y', h2 + 142).c('J', 1).txt(&st(l, if save { 176 } else { 175 }));
                for i in 0..6 {
                    let y = if i < 5 { (h2 + 2) * (i as i32 + 3) + 140 } else { (h2 + 2) * 9 + 140 };
                    let mut label = if i == 5 {
                        st(l, 56)
                    } else {
                        match &self.slots[i] {
                            Some(n) => crate::data::strings::to_cp437(n),
                            None => st(l, 177),
                        }
                    };
                    if i == self.list_cur {
                        if let Some(e) = &self.edit {
                            label = crate::data::strings::to_cp437(e.text());
                            label.push(if cursor_on { b'b' } else { b' ' });
                        }
                    }
                    m.c('Y', y).c('J', 1);
                    if i == self.list_cur {
                        m.c('O', 0).c('F', 1).txt(&label).end_hl().c('O', 64);
                    } else {
                        m.c('C', 184).c('O', 64).txt(&label);
                    }
                }
                m.0
            }
        };
    }

    fn draw_logo(ctx: &Ctx, cv: &mut Canvas) {
        let logo = &ctx.assets.logo;
        let w: i32 = (1..=4).map(|i| logo.get(i).w as i32).sum();
        let mut x = (640 - w) / 2;
        let y = 40;
        for i in 1..=4 {
            cv.sprite(logo.get(i), x, y, Blit::Solid);
            x += logo.get(i).w as i32;
        }
        let h0 = logo.get(1).h as i32;
        let w2: i32 = (5..=6).map(|i| logo.get(i).w as i32).sum();
        let mut x = (640 - w2) / 2;
        for i in 5..=6 {
            cv.sprite(logo.get(i), x, y + h0 + 4, Blit::Solid);
            x += logo.get(i).w as i32;
        }
    }

    /// Draw: `bg` lies below the 3D carousel models, `fg` above. Returns the carousel 3D scene if any.
    pub fn draw(&mut self, ctx: &Ctx, bg: &mut Canvas, fg: &mut Canvas) -> Option<Scene> {
        bg.clear_colour(0);
        fg.clear();
        bg.blit_image(&ctx.assets.oscr.pix, 640, 480, 0, 0, 1);
        if matches!(self.st, St::Lang | St::Name | St::Menu | St::LoadList | St::SaveList) {
            Self::draw_logo(ctx, bg);
        }
        self.tbox.draw(bg, ctx.assets);
        if self.tbox.in_place() {
            markup::draw(bg, ctx.assets, &self.text, self.typer.n);
            if !self.text2.is_empty() {
                markup::draw(bg, ctx.assets, &self.text2, self.typer2.n);
            }
        }
        self.carousel.as_ref().filter(|_| matches!(self.st, St::Track | St::Craft)).map(|c| c.scene(ctx.assets, ctx.cfg.team()))
    }
}

/// A made-up classified result (developer aid, env C8H18_FE_TEST).
fn fake_result(ctx: &Ctx, champ: bool) -> RaceResult {
    use crate::race::types::PlayerResult;
    let rn = ctx.cfg.race_number() as usize;
    let laps = DEFAULT_LAPS[rn] as u32;
    let players = (0..8)
        .map(|k| {
            let (name, team, craft) = results::driver(ctx.cfg, k);
            let place = [3u8, 1, 2, 5, 4, 8, 6, 7][k];
            PlayerResult {
                name,
                team,
                craft,
                human: k == 0,
                finish_place: place,
                finish_time: 9000 + 137 * place as i32,
                best_lap: 700 + 11 * k as i32,
                laps,
                kills: (k % 3) as i32,
                deaths: (k % 2) as i32,
                hit_accuracy: 10 + 7 * k as i32,
                assessment: (k * 2) as u8,
                points: 0,
            }
        })
        .collect();
    RaceResult { demo: false, championship: champ, track: rn + 1, quit_game: false, classified: true, players }
}

/// In-race pause menu (spec 02 §13).
pub struct PauseMenu {
    tbox: TextBox,
    menu: MenuState,
    typer: Typer,
    text: Vec<u8>,
    sig: (MenuId, usize),
}

pub enum PauseOut {
    Stay,
    Resume,
}

impl Default for PauseMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl PauseMenu {
    pub fn new() -> PauseMenu {
        PauseMenu { tbox: TextBox::parked(), menu: MenuState::new(MenuId::PauseRoot), typer: Typer::default(), text: Vec::new(), sig: (MenuId::PauseRoot, 1) }
    }
    /// One frame of the pause menu (called at 70 Hz while paused). Escape / P resume.
    pub fn frame(&mut self, ctx: &mut Ctx) -> PauseOut {
        if self.menu.waiting.is_none() && ctx.keys.iter().any(|k| matches!(k.code, KeyCode::Escape | KeyCode::KeyP) && !k.alt) {
            return PauseOut::Resume;
        }
        let in_place = self.tbox.update(BOX_PAUSE, ctx.audio);
        let _ = self.menu.frame(ctx, in_place);
        self.text = self.menu.markup(ctx, 78, 242);
        let sig = (self.menu.top(), self.menu.depth());
        if sig != self.sig {
            self.sig = sig;
            self.typer.reset();
        }
        if in_place {
            self.typer.step(markup::visible_len(&self.text), ctx.input.down(KeyCode::Space), true, ctx.audio);
        }
        PauseOut::Stay
    }
    pub fn draw(&mut self, ctx: &Ctx, fg: &mut Canvas) {
        self.tbox.draw(fg, ctx.assets);
        if self.tbox.in_place() {
            markup::draw(fg, ctx.assets, &self.text, self.typer.n);
        }
    }
}
