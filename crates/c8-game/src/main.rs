mod assets;
mod audio;
mod ctx;
use c8_data as data;
mod frontend;
mod gfx;
mod input;
mod intro;
mod pad;
mod video;
mod race;

use assets::Assets;
use audio::Audio;
use ctx::Ctx;
use data::config::Config;
use frontend::{FeEntry, FeOut, FrontEnd};
use gfx::canvas::Canvas;
use gfx::Gfx;
use input::{Input, KeyCode, KeyPress};
use race::types::RaceSetup;
use race::Race;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{Window, WindowId};

pub const FE_HZ: f64 = 70.0;

enum Mode {
    Intro(intro::Intro),
    Title(f64),
    Front(FrontEnd),
    /// starting grid picture shown, level loading next frame
    Loading(RaceSetup, u32),
    Race(Race),
    Quit,
}

/// What happens when a running fade completes.
enum After {
    Nothing,
    EnterFront(FeEntry),
    ShowLoading(RaceSetup),
    Exit,
}

struct Fade {
    from: f32,
    to: f32,
    dur: f64,
    t: f64,
    after: After,
}

struct Opts {
    sound: bool,
    intro: bool,
    /// debug: start a race on this track directly (demo if the flag is set)
    race: Option<(usize, bool)>,
}

fn debug_setup(track: usize, demo: bool, cfg: &Config) -> RaceSetup {
    use race::types::{PlayerSetup, AI_NAMES};
    let mut players = Vec::new();
    for slot in 0..8 {
        let human = slot == 0 && !demo;
        players.push(PlayerSetup {
            name: if human { if cfg.name().is_empty() { "PLAYER".into() } else { cfg.name() } } else { AI_NAMES[slot].to_string() },
            human,
            team: ((slot as u8) + cfg.team()) % 8,
            craft: ((slot as u8) + cfg.craft()) % 6,
            difficulty: cfg.difficulty().max(1),
        });
    }
    RaceSetup { track, laps: std::env::var("C8H18_LAPS").ok().and_then(|v| v.parse().ok()).unwrap_or(data::config::DEFAULT_LAPS[track - 1] as u32), players, demo, championship: false }
}

/// Test automation from the C8H18_AUTO environment variable:
/// `frame:shot:path;frame:key:KeyName;frame:hold:KeyName;frame:release:KeyName;frame:quit` (frame = redraw count).
struct Auto {
    events: Vec<(u64, String, String)>,
    frame: u64,
}

impl Auto {
    fn from_env() -> Auto {
        let mut events = Vec::new();
        if let Ok(s) = std::env::var("C8H18_AUTO") {
            for part in s.split(';') {
                let mut it = part.splitn(3, ':');
                let (Some(f), Some(op)) = (it.next(), it.next()) else { continue };
                if let Ok(f) = f.trim().parse() {
                    events.push((f, op.to_string(), it.next().unwrap_or("").to_string()));
                }
            }
        }
        Auto { events, frame: 0 }
    }
}

pub fn key_from_name(n: &str) -> Option<KeyCode> {
    use KeyCode::*;
    Some(match n {
        "Enter" => Enter, "Escape" => Escape, "Space" => Space, "Up" => ArrowUp, "Down" => ArrowDown,
        "Left" => ArrowLeft, "Right" => ArrowRight, "Backspace" => Backspace, "Tab" => Tab,
        "Z" => KeyZ, "X" => KeyX, "C" => KeyC, "P" => KeyP, "A" => KeyA, "B" => KeyB, "Y" => KeyY, "R" => KeyR,
        "1" => Digit1, "2" => Digit2, "3" => Digit3, "4" => Digit4,
        "F1" => F1, "F2" => F2, "F3" => F3, "F4" => F4, "F5" => F5, "F6" => F6, "F7" => F7, "F8" => F8, "F9" => F9, "F10" => F10,
        _ => return None,
    })
}

struct Game {
    gfx: Gfx,
    assets: Assets,
    audio: Audio,
    cfg: Config,
    input: Input,
    pad: pad::Pad,
    bg: Canvas,
    fg: Canvas,
    mode: Mode,
    fade: Option<Fade>,
    bright: f32,
    last: Instant,
    acc: f64,
    sound: bool,
    auto: Auto,
}

struct App {
    opts: Opts,
    game: Option<Game>,
}

impl Game {
    fn ctx_parts(&mut self) -> Ctx<'_> {
        let keys = self.input.take();
        Ctx { assets: &self.assets, audio: &mut self.audio, cfg: &mut self.cfg, input: &self.input, pad: &self.pad.state, keys, sound: self.sound }
    }

    fn start_fade(&mut self, to: f32, steps: u32, after: After) {
        self.fade = Some(Fade { from: self.bright, to, dur: steps as f64 / 70.0, t: 0.0, after });
    }

    fn enter_front(&mut self, entry: FeEntry) {
        self.gfx.set_palette(&self.assets.palette.vga);
        self.gfx.set_colour_table(&self.assets.fe_table.data);
        self.fg.table = self.assets.fe_table.clone();
        self.bg.table = self.assets.fe_table.clone();
        self.audio.stop_all();
        self.audio.stop_music();
        let mut ctx = self.ctx_parts();
        let fe = FrontEnd::new(entry, &mut ctx);
        self.mode = Mode::Front(fe);
        self.bright = 0.0;
        self.start_fade(1.0, 32, After::Nothing);
    }

    fn update(&mut self, el: &ActiveEventLoop) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f64().min(0.25);
        self.last = now;
        self.pad.poll(dt);
        // menus and the "any key" screens see pad presses as keys; the race reads the pad itself
        match self.mode {
            Mode::Intro(_) | Mode::Title(_) => {
                if self.pad.state.any_pressed() {
                    self.input.pressed.push(input::KeyPress { code: KeyCode::Enter, shift: false, right_shift: false, ctrl: false, alt: false, ch: None });
                }
            }
            Mode::Front(ref fe) if !fe.binding() => self.input.pressed.extend(self.pad.state.menu_keys(false)),
            _ => {}
        }
        let (cw, ch) = self.gfx.canvas_dims();
        self.bg.resize(cw, ch);
        self.fg.resize(cw, ch);

        // fades freeze the program (keys pressed meanwhile are lost)
        if let Some(f) = &mut self.fade {
            f.t += dt;
            let k = (f.t / f.dur).min(1.0) as f32;
            self.bright = f.from + (f.to - f.from) * k;
            self.input.pressed.clear();
            if f.t >= f.dur {
                let f = self.fade.take().unwrap();
                match f.after {
                    After::Nothing => {}
                    After::EnterFront(e) => self.enter_front(e),
                    After::ShowLoading(setup) => {
                        self.mode = Mode::Loading(setup, 0);
                        self.start_fade(1.0, 32, After::Nothing);
                    }
                    After::Exit => {
                        self.cfg.save();
                        self.mode = Mode::Quit;
                        el.exit();
                    }
                }
            }
            self.draw();
            return;
        }

        // global quit: Shift+Q in the front end handled there; right Shift+Q in the race by the race
        let mut next: Option<Mode> = None;
        match &mut self.mode {
            Mode::Intro(intro) => {
                if self.input.any_pressed() {
                    intro.skip();
                    self.input.pressed.clear();
                }
                let done = intro.advance(dt, &mut self.audio);
                if done {
                    intro.finish(&mut self.audio);
                    self.audio.load_bank(0);
                    self.gfx.set_palette(&self.assets.palette.vga);
                    next = Some(Mode::Title(0.0));
                }
            }
            Mode::Title(t) => {
                *t += dt;
                if *t >= 3.0 || self.input.any_pressed() {
                    self.input.pressed.clear();
                    self.start_fade(0.0, 16, After::EnterFront(FeEntry::Start));
                }
            }
            Mode::Front(_) => {
                self.acc += dt;
                let step = 1.0 / FE_HZ;
                while self.acc >= step {
                    self.acc -= step;
                    let Mode::Front(fe) = &mut self.mode else { break };
                    let keys = self.input.take();
                    let mut ctx = Ctx { assets: &self.assets, audio: &mut self.audio, cfg: &mut self.cfg, input: &self.input, pad: &self.pad.state, keys, sound: self.sound };
                    match fe.frame(&mut ctx) {
                        FeOut::Continue => {}
                        FeOut::Race(setup) => {
                            self.start_fade(0.0, 16, After::ShowLoading(setup));
                            self.acc = 0.0;
                            break;
                        }
                        FeOut::Quit => {
                            self.start_fade(0.0, 16, After::Exit);
                            self.acc = 0.0;
                            break;
                        }
                    }
                }
            }
            Mode::Loading(setup, n) => {
                *n += 1;
                if *n >= 2 {
                    let setup = setup.clone();
                    let keys = self.input.take();
                    let mut ctx = Ctx { assets: &self.assets, audio: &mut self.audio, cfg: &mut self.cfg, input: &self.input, pad: &self.pad.state, keys, sound: self.sound };
                    match Race::new(setup, &mut ctx, &mut self.gfx) {
                        Ok(r) => next = Some(Mode::Race(r)),
                        Err(e) => {
                            eprintln!("race failed to load: {e}");
                            self.enter_front(FeEntry::AfterDemo);
                        }
                    }
                    self.last = Instant::now();
                }
            }
            Mode::Race(race) => {
                let keys = self.input.take();
                let mut ctx = Ctx { assets: &self.assets, audio: &mut self.audio, cfg: &mut self.cfg, input: &self.input, pad: &self.pad.state, keys, sound: self.sound };
                if let Some(res) = race.frame(&mut ctx, dt) {
                    self.audio.stop_all();
                    self.audio.stop_music();
                    if res.quit_game {
                        self.cfg.save();
                        self.mode = Mode::Quit;
                        el.exit();
                        return;
                    }
                    let entry = if res.demo { FeEntry::AfterDemo } else { FeEntry::AfterRace(res) };
                    self.enter_front(entry);
                    self.bright = 1.0;
                    self.fade = None;
                }
            }
            Mode::Quit => {}
        }
        if let Some(m) = next {
            self.mode = m;
        }
        self.draw();
    }

    fn draw(&mut self) {
        let mut scene = None;
        let mut use_bg = false;
        match &mut self.mode {
            Mode::Intro(intro) => {
                self.gfx.set_palette(&intro.palette);
                self.fg.clear_colour(0);
                self.fg.blit_image(&intro.pixels, 320, 200, 0, 40, 2);
            }
            Mode::Title(_) => {
                self.fg.clear_colour(0);
                self.fg.blit_image(&self.assets.title.pix, 320, 200, 0, 40, 2);
            }
            Mode::Front(fe) => {
                let keys = Vec::new();
                let ctx = Ctx { assets: &self.assets, audio: &mut self.audio, cfg: &mut self.cfg, input: &self.input, pad: &self.pad.state, keys, sound: self.sound };
                scene = fe.draw(&ctx, &mut self.bg, &mut self.fg);
                use_bg = true;
            }
            Mode::Loading(_, _) => {
                self.fg.clear_colour(0);
                self.fg.blit_image(&self.assets.onet.pix, 640, 480, 0, 0, 1);
                if self.fade.is_none() {
                    let lang = self.cfg.language();
                    let s = data::strings::to_cp437(data::strings::get(lang, 87));
                    let font = &self.assets.fonts[2];
                    self.fg.text_centred(font, &s, 240, -6, 184, Some(64));
                }
            }
            Mode::Race(race) => {
                let keys = Vec::new();
                let ctx = Ctx { assets: &self.assets, audio: &mut self.audio, cfg: &mut self.cfg, input: &self.input, pad: &self.pad.state, keys, sound: self.sound };
                scene = race.draw_gfx(&ctx, &mut self.gfx, &mut self.fg);
            }
            Mode::Quit => return,
        }
        let bg = if use_bg { Some(&self.bg) } else { None };
        self.gfx.render(scene.as_ref(), bg, &self.fg, self.bright);
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.game.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("C8H18")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));
        let window = Arc::new(el.create_window(attrs).expect("window"));
        let mut gfx = pollster::block_on(Gfx::new(window.clone()));
        pad::load_settings();
        video::load();
        let assets = match data::strings::load().and_then(|_| Assets::load()) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("failed to load data: {e}");
                el.exit();
                return;
            }
        };
        gfx.set_palette(&assets.palette.vga);
        gfx.set_colour_table(&assets.fe_table.data);
        gfx.set_model_atlas(&assets.atlas.pix, assets.atlas.h);
        gfx.set_tmaps(&assets.tmaps);
        let cfg = Config::load();
        let mut audio = Audio::new(self.opts.sound);
        audio.set_stereo(cfg.stereo());
        audio.set_effects_volume(cfg.effects_volume());
        audio.set_music_volume(cfg.music_volume());
        if !self.opts.intro {
            audio.load_bank(0);
        }
        let (cw, ch) = gfx.canvas_dims();
        let bg = Canvas::new(cw, ch, assets.fe_table.clone());
        let fg = Canvas::new(cw, ch, Rc::clone(&assets.fe_table));
        let mode = if let Some((t, demo)) = self.opts.race {
            audio.load_bank(0);
            Mode::Loading(debug_setup(t, demo, &cfg), 0)
        } else if self.opts.intro {
            match intro::Intro::new() {
                Ok(i) => Mode::Intro(i),
                Err(_) => { audio.load_bank(0); Mode::Title(0.0) }
            }
        } else {
            Mode::Title(0.0)
        };
        self.game = Some(Game {
            gfx,
            assets,
            audio,
            cfg,
            input: Input::default(),
            pad: pad::Pad::new(),
            bg,
            fg,
            mode,
            fade: None,
            bright: 1.0,
            last: Instant::now(),
            acc: 0.0,
            sound: self.opts.sound,
            auto: Auto::from_env(),
        });
        window.request_redraw();
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(game) = &mut self.game else { return };
        match event {
            WindowEvent::CloseRequested => {
                game.cfg.save();
                el.exit();
            }
            WindowEvent::Resized(s) => game.gfx.resize(s.width, s.height),
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    match event.state {
                        ElementState::Pressed => {
                            game.input.held.insert(code);
                            let ch = event.text.as_ref().and_then(|t| t.chars().next()).filter(|c| !c.is_control());
                            let kp = KeyPress {
                                code,
                                shift: game.input.shift(),
                                right_shift: game.input.down(KeyCode::ShiftRight),
                                ctrl: game.input.ctrl(),
                                alt: game.input.alt(),
                                ch,
                            };
                            if !matches!(code, KeyCode::ShiftLeft | KeyCode::ShiftRight | KeyCode::ControlLeft | KeyCode::ControlRight | KeyCode::AltLeft | KeyCode::AltRight | KeyCode::SuperLeft | KeyCode::SuperRight) {
                                game.input.pressed.push(kp);
                            }
                        }
                        ElementState::Released => {
                            game.input.held.remove(&code);
                        }
                    }
                }
            }
            WindowEvent::Focused(false) => game.input.held.clear(),
            WindowEvent::RedrawRequested => {
                game.auto.frame += 1;
                if std::env::var_os("C8H18_FPS").is_some() && game.auto.frame % 120 == 0 {
                    eprintln!("frame {} at {:?}", game.auto.frame, std::time::SystemTime::now());
                }
                let f = game.auto.frame;
                for (_, op, arg) in game.auto.events.iter().filter(|e| e.0 == f) {
                    match op.as_str() {
                        "shot" => game.gfx.shot = Some(arg.clone()),
                        "key" => {
                            if let Some(code) = key_from_name(arg) {
                                game.input.pressed.push(KeyPress { code, shift: false, right_shift: false, ctrl: false, alt: false, ch: None });
                            } else if let Some(c) = arg.chars().next() {
                                game.input.pressed.push(KeyPress { code: KeyCode::KeyA, shift: false, right_shift: false, ctrl: false, alt: false, ch: Some(c) });
                            }
                        }
                        "hold" => { if let Some(code) = key_from_name(arg) { game.input.held.insert(code); } }
                        "release" => { if let Some(code) = key_from_name(arg) { game.input.held.remove(&code); } }
                        "padhold" => { if let Some(b) = pad::btn_from_name(arg) { game.pad.fake[b as usize] = true; } }
                        "padrelease" => { if let Some(b) = pad::btn_from_name(arg) { game.pad.fake[b as usize] = false; } }
                        "quit" => { el.exit(); return; }
                        _ => {}
                    }
                }
                game.update(el);
                game.gfx.window.request_redraw();
            }
            _ => {}
        }
    }
}

fn main() {
    env_logger::init();
    let mut opts = Opts { sound: true, intro: true, race: None };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let l = a.to_ascii_lowercase();
        if l == "-s" || l == "/s" || l == "--silent" {
            opts.sound = false;
        } else if l == "--no-intro" {
            opts.intro = false;
        } else if l == "--speed" {
            if let Some(v) = args.get(i + 1).and_then(|v| v.parse::<u32>().ok()) {
                race::SPEED_PCT.store(v.clamp(25, 400), std::sync::atomic::Ordering::Relaxed);
            }
            i += 1;
        } else if l == "--sim" {
            let t = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(1usize).clamp(1, 6);
            let n = args.get(i + 2).and_then(|v| v.parse().ok()).unwrap_or(1000u32);
            let rings = data::images::load_search_rings().unwrap();
            race::headless(t, n, rings, true);
            return;
        } else if l == "--level" {
            if let Some(p) = args.get(i + 1) {
                let _ = race::LEVEL_FILE.set(p.into());
            }
            i += 1;
        } else if l == "--lang" {
            if let Some(code) = args.get(i + 1) {
                if let Err(e) = data::strings::force_language(code) {
                    eprintln!("{e}");
                }
            }
            i += 1;
        } else if l == "--override" {
            if let Some(p) = args.get(i + 1) {
                data::files::set_override_dir(Some(p.into()));
            }
            i += 1;
        } else if l == "--race" || l == "--demo" {
            let t = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(1usize).clamp(1, 6);
            opts.race = Some((t, l == "--demo"));
            i += 1;
        } else if !l.starts_with('-') {
            data::files::set_data_dir(a.into());
        }
        i += 1;
    }
    if let Ok(d) = std::env::var("GAME_DATA") {
        data::files::set_data_dir(d.into());
    }
    let el = EventLoop::new().expect("event loop");
    el.set_control_flow(ControlFlow::Poll);
    let mut app = App { opts, game: None };
    el.run_app(&mut app).expect("run");
}
