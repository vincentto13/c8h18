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
mod tracks;

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
    /// headless capture mode: render size, no window (see `run_headless`)
    headless: Option<(u32, u32)>,
}

/// Default render size of `--headless`.
const HEADLESS_SIZE: (u32, u32) = (1920, 1080);
/// Fixed frame rate of `--headless`: each display frame advances the game by 1/60 s.
const HEADLESS_FPS: f64 = 60.0;

/// `WxH` (e.g. `1920x1080`) as a render size; None when malformed or out of 16..=8192.
fn parse_size(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.to_ascii_lowercase().split_once('x').map(|(w, h)| (w.trim().parse().ok(), h.trim().parse().ok()))?;
    let (w, h): (u32, u32) = (w?, h?);
    let ok = |v: u32| (16..=8192).contains(&v);
    (ok(w) && ok(h)).then_some((w, h))
}

/// Headless capture mode: no winit event loop, window or surface. The game runs at a fixed
/// HEADLESS_FPS, every frame is drawn into an offscreen texture and submitted, and the
/// C8H18_AUTO script drives it as in a window (`shot` saves that frame). It ends at `quit`,
/// when the game quits, or one frame after the script's last op.
fn run_headless(opts: &Opts, (w, h): (u32, u32)) -> Result<(), String> {
    let gfx = pollster::block_on(Gfx::new_headless(w, h))?;
    let mut game = Game::new(gfx, opts);
    game.fixed_dt = Some(1.0 / HEADLESS_FPS);
    let last = game.auto.events.iter().map(|e| e.0).max().ok_or("--headless needs a C8H18_AUTO script (end it with `quit`)")?;
    while game.auto.frame <= last {
        game.auto.frame += 1;
        if game.run_auto() {
            break;
        }
        game.update();
        if game.exit {
            break;
        }
    }
    Ok(())
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
    /// the game asked to end (the window loop exits, the headless loop stops)
    exit: bool,
    /// headless capture mode: fixed time step per frame instead of the wall clock
    fixed_dt: Option<f64>,
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

    /// Load the assets and settings and start in the mode the options ask for.
    fn new(mut gfx: Gfx, opts: &Opts) -> Game {
        pad::load_settings();
        video::load();
        let _ = data::strings::load();
        let assets = Assets::load();
        gfx.set_palette(&assets.palette.vga);
        gfx.set_colour_table(&assets.fe_table.data);
        gfx.set_model_atlas(&assets.atlas.pix, assets.atlas.h);
        gfx.set_tmaps(&assets.tmaps);
        let cfg = Config::load();
        let avail = tracks::available();
        log::info!("tracks available: {:?}", avail.iter().map(|t| t + 1).collect::<Vec<_>>());
        let mut audio = Audio::new(opts.sound);
        audio.set_stereo(cfg.stereo());
        audio.set_effects_volume(cfg.effects_volume());
        audio.set_music_volume(cfg.music_volume());
        if !opts.intro {
            audio.load_bank(0);
        }
        let (cw, ch) = gfx.canvas_dims();
        let bg = Canvas::new(cw, ch, assets.fe_table.clone());
        let fg = Canvas::new(cw, ch, Rc::clone(&assets.fe_table));
        let mode = if let Some((t, demo)) = opts.race {
            audio.load_bank(0);
            Mode::Loading(debug_setup(t, demo, &cfg), 0)
        } else if opts.intro {
            // the movie is optional: without (or with an unreadable) INTRO.DAT go on as with --no-intro
            match intro::Intro::new() {
                Ok(i) => Mode::Intro(i),
                Err(e) => {
                    data::files::warn_asset("data/intro.dat", Some(&e), "skipping the intro movie");
                    audio.load_bank(0);
                    Mode::Title(0.0)
                }
            }
        } else {
            Mode::Title(0.0)
        };
        Game {
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
            sound: opts.sound,
            auto: Auto::from_env(),
            exit: false,
            fixed_dt: None,
        }
    }

    /// Run this frame's C8H18_AUTO ops; true = `quit`.
    fn run_auto(&mut self) -> bool {
        let f = self.auto.frame;
        for (_, op, arg) in self.auto.events.iter().filter(|e| e.0 == f) {
            match op.as_str() {
                "shot" => self.gfx.shot = Some(arg.clone()),
                "key" => {
                    if let Some(code) = key_from_name(arg) {
                        self.input.pressed.push(KeyPress { code, shift: false, right_shift: false, ctrl: false, alt: false, ch: None });
                    } else if let Some(c) = arg.chars().next() {
                        self.input.pressed.push(KeyPress { code: KeyCode::KeyA, shift: false, right_shift: false, ctrl: false, alt: false, ch: Some(c) });
                    }
                }
                "hold" => { if let Some(code) = key_from_name(arg) { self.input.held.insert(code); } }
                "release" => { if let Some(code) = key_from_name(arg) { self.input.held.remove(&code); } }
                "padhold" => { if let Some(b) = pad::btn_from_name(arg) { self.pad.fake[b as usize] = true; } }
                "padrelease" => { if let Some(b) = pad::btn_from_name(arg) { self.pad.fake[b as usize] = false; } }
                "quit" => return true,
                _ => {}
            }
        }
        false
    }

    fn update(&mut self) {
        let now = Instant::now();
        let dt = self.fixed_dt.unwrap_or_else(|| (now - self.last).as_secs_f64().min(0.25));
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
                        self.exit = true;
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
                        self.exit = true;
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
        if !self.gfx.render(scene.as_ref(), bg, &self.fg, self.bright) {
            // no drawable (window hidden or occluded): there is no vsync wait to pace the
            // loop, so don't spin; game time follows the wall clock, so nothing is lost
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
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
        let gfx = pollster::block_on(Gfx::new(window.clone()));
        self.game = Some(Game::new(gfx, &self.opts));
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
                if game.run_auto() {
                    el.exit();
                    return;
                }
                game.update();
                if game.exit {
                    el.exit();
                    return;
                }
                if let Some(w) = &game.gfx.window {
                    w.request_redraw();
                }
            }
            _ => {}
        }
    }
}

/// Choose the asset package and its save folder (see `c8_data::locate`) and make it current.
fn use_package(asset: Option<&std::path::Path>) {
    let r = data::locate::resolve(asset, &data::locate::Env::system());
    for w in &r.warnings {
        println!("warning: {w}");
    }
    log::info!("asset package {} ({:?}, name {:?}), saves in {}", r.path.display(), r.origin, r.name, r.save_dir.display());
    match data::locate::migrate_saves(&r) {
        Ok(Some(old)) => println!("copied the saves from {} to {}", old.display(), r.save_dir.display()),
        Ok(None) => {}
        Err(e) => println!("warning: cannot copy the old saves: {e}"),
    }
    let p = data::package::Package::open(&r.path).unwrap_or_else(|e| {
        println!("warning: {e}: no assets can be loaded");
        data::package::Package::dir(r.path.clone())
    });
    data::files::set_package(p.with_save_dir(r.save_dir));
}

fn main() {
    env_logger::init();
    let mut opts = Opts { sound: true, intro: true, race: None, headless: None };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut asset: Option<std::path::PathBuf> = None;
    let mut sim = None;
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
            sim = Some((t, n));
            i += 2;
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
            // editor-internal ("Test in game"): a folder whose files win over the package's
            if let Some(p) = args.get(i + 1) {
                data::files::set_override_dir(Some(p.into()));
            }
            i += 1;
        } else if l == "--headless" {
            // optional WxH after it
            match args.get(i + 1).and_then(|v| parse_size(v)) {
                Some(sz) => {
                    opts.headless = Some(sz);
                    i += 1;
                }
                None => opts.headless = Some(HEADLESS_SIZE),
            }
        } else if l == "--race" || l == "--demo" {
            let t = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(1usize).clamp(1, 6);
            opts.race = Some((t, l == "--demo"));
            i += 1;
        } else if l == "--asset" || l == "--data" {
            // `--data` is the old name of `--asset`
            if let Some(p) = args.get(i + 1) {
                asset = Some(p.into());
            }
            i += 1;
        } else if l == "-h" || l == "--help" {
            println!("usage: c8h18 [--asset <dir|file.c8pak>] [-s|--silent] [--no-intro] [--lang <code>] [--speed <25..400>]");
            println!("             [--race <1..6> | --demo <1..6>] [--level <file>] [--sim <track> <ticks>] [--headless [WxH]]");
            println!("  --asset   the asset package: a directory with the original layout (DATA/, MAPS/, OBJECTS/, SOUND/,");
            println!("            lang/) or a .c8pak bundle of one; also `--data <path>` or a bare <path>. Without it:");
            println!("            $C8H18_ASSET or $GAME_DATA, then `package = <name>` in {},", data::locate::Env::system().settings_path().display());
            println!("            then the shipped {}", data::locate::SHIPPED_PACKAGE);
            println!("  --headless  no window: run the C8H18_AUTO script at 60 frames/s offscreen (default 1920x1080)");
            return;
        } else if !l.starts_with('-') {
            asset = Some(a.into());
        }
        i += 1;
    }
    use_package(asset.as_deref());
    if let Some((t, n)) = sim {
        let rings = data::images::load_search_rings_or_generate();
        race::headless(t, n, rings, true);
        return;
    }
    if let Some(size) = opts.headless {
        if let Err(e) = run_headless(&opts, size) {
            eprintln!("headless: {e}");
            std::process::exit(1);
        }
        return;
    }
    let el = EventLoop::new().expect("event loop");
    el.set_control_flow(ControlFlow::Poll);
    let mut app = App { opts, game: None };
    el.run_app(&mut app).expect("run");
}

#[cfg(test)]
mod tests {
    use super::parse_size;

    #[test]
    fn headless_size() {
        assert_eq!(parse_size("1920x1080"), Some((1920, 1080)));
        assert_eq!(parse_size("640X360"), Some((640, 360)));
        assert_eq!(parse_size("1920"), None);
        assert_eq!(parse_size("0x100"), None);
        assert_eq!(parse_size("--demo"), None);
    }
}
